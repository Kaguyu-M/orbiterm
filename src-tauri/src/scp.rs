//! File browser for SSH servers without SFTP (dropbear, appliances, restricted servers):
//! listing and file operations through shell commands, transfers with the SCP protocol
//! (`scp -f` / `scp -t` on the server).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use russh::ChannelMsg;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::error::{AppError, AppResult};
use crate::sftp::{Entry, perms_string, remote_parent};
use crate::ssh::SshConnection;

const CHUNK: usize = 128 * 1024;

/// Quotes a value for a POSIX shell.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn err(msg: impl Into<String>) -> AppError {
    AppError::Sftp(msg.into())
}

/// Runs a command and returns (stdout, stderr, exit status).
async fn run(conn: &SshConnection, cmd: &str) -> AppResult<(String, String, u32)> {
    let mut ch = conn.exec_channel(cmd).await?;
    let (mut out, mut errb, mut status) = (Vec::new(), Vec::new(), None);
    let read = async {
        while let Some(m) = ch.wait().await {
            match m {
                ChannelMsg::Data { data } if out.len() < 8 << 20 => out.extend_from_slice(&data),
                ChannelMsg::ExtendedData { data, .. } if errb.len() < 64 << 10 => errb.extend_from_slice(&data),
                ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status),
                ChannelMsg::Close => break,
                _ => {}
            }
        }
    };
    if tokio::time::timeout(Duration::from_secs(60), read).await.is_err() {
        let _ = ch.close().await;
        return Err(AppError::Timeout);
    }
    Ok((String::from_utf8_lossy(&out).into_owned(), String::from_utf8_lossy(&errb).trim().to_string(), status.unwrap_or(0)))
}

/// Runs a command that must succeed; its error output becomes the error message.
async fn run_ok(conn: &SshConnection, cmd: &str) -> AppResult<String> {
    let (out, e, status) = run(conn, cmd).await?;
    if status != 0 {
        return Err(err(if e.is_empty() { format!("la commande a échoué (code {status})") } else { e }));
    }
    Ok(out)
}

pub async fn home(conn: &SshConnection) -> AppResult<String> {
    let out = run_ok(conn, "pwd").await?;
    Ok(out.lines().next().unwrap_or("/").trim().to_string())
}

/// Listing script: GNU find when available, else stat -c (busybox), one entry per line:
/// type, size, mtime, octal mode, name (tab separated). The first line is the real path.
fn list_command(path: &str) -> String {
    format!(
        "cd -- {} || exit 3; pwd; \
         if ! find . -mindepth 1 -maxdepth 1 -printf '%Y\\t%s\\t%T@\\t%m\\t%P\\n' 2>/dev/null; then \
         for f in .[!.]* ..?* *; do [ -e \"$f\" ] || [ -L \"$f\" ] || continue; \
         stat -L -c '%F\t%s\t%Y\t%a\t%n' -- \"$f\" 2>/dev/null; done; fi",
        sh_quote(path)
    )
}

fn parse_kind(t: &str) -> &'static str {
    match t {
        "d" | "directory" => "dir",
        "f" | "regular file" | "regular empty file" => "file",
        "l" | "symbolic link" => "link",
        _ => "other",
    }
}

pub fn parse_listing(out: &str) -> (String, Vec<Entry>) {
    let mut lines = out.lines();
    let dir = lines.next().unwrap_or("/").trim().to_string();
    let mut v: Vec<Entry> = lines
        .filter_map(|l| {
            let mut it = l.splitn(5, '\t');
            let (t, size, mtime, mode, name) = (it.next()?, it.next()?, it.next()?, it.next()?, it.next()?);
            if name.is_empty() || name == "." || name == ".." {
                return None;
            }
            Some(Entry {
                name: name.to_string(),
                path: crate::sftp::remote_join(&dir, name),
                kind: parse_kind(t).to_string(),
                size: size.parse().unwrap_or(0),
                mtime: mtime.split('.').next().and_then(|s| s.parse().ok()).unwrap_or(0),
                perms: u32::from_str_radix(mode, 8).map(perms_string).unwrap_or_default(),
            })
        })
        .collect();
    v.sort_by(|a, b| (a.kind != "dir").cmp(&(b.kind != "dir")).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    (dir, v)
}

pub async fn list(conn: &SshConnection, path: &str) -> AppResult<Vec<Entry>> {
    let (out, e, status) = run(conn, &list_command(path)).await?;
    if status != 0 {
        return Err(err(if e.is_empty() { format!("{path} : dossier inaccessible") } else { e }));
    }
    Ok(parse_listing(&out).1)
}

pub async fn mkdir(conn: &SshConnection, path: &str) -> AppResult<()> {
    run_ok(conn, &format!("mkdir -- {}", sh_quote(path))).await.map(|_| ())
}

pub async fn rename(conn: &SshConnection, from: &str, to: &str) -> AppResult<()> {
    run_ok(conn, &format!("mv -- {} {}", sh_quote(from), sh_quote(to))).await.map(|_| ())
}

pub async fn remove(conn: &SshConnection, path: &str) -> AppResult<()> {
    if path.trim_end_matches('/').is_empty() || remote_parent(path) == path {
        return Err(AppError::InvalidPath(path.into()));
    }
    run_ok(conn, &format!("rm -rf -- {}", sh_quote(path))).await.map(|_| ())
}

// ------------------------------------------------------------------ SCP protocol

/// Reads an SCP acknowledgement: 0 = ok, 1 / 2 = error followed by a message line.
async fn ack<R: AsyncRead + Unpin>(r: &mut BufReader<R>) -> AppResult<()> {
    let b = r.read_u8().await.map_err(|_| err("SCP : le serveur a fermé la connexion (scp absent ?)"))?;
    if b == 0 {
        return Ok(());
    }
    let mut line = String::new();
    r.read_line(&mut line).await?;
    Err(err(format!("SCP : {}", line.trim())))
}

/// Parses "C0644 1234 name".
pub fn parse_c_line(line: &str) -> Option<(u32, u64, String)> {
    let rest = line.strip_prefix('C')?;
    let mut it = rest.trim_end_matches('\n').splitn(3, ' ');
    let mode = u32::from_str_radix(it.next()?, 8).ok()?;
    let size = it.next()?.parse().ok()?;
    Some((mode, size, it.next()?.to_string()))
}

pub async fn download(
    conn: &Arc<SshConnection>,
    remote: &str,
    local: &Path,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> AppResult<()> {
    let ch = conn.exec_channel(&format!("scp -f {}", sh_quote(remote))).await?;
    let (rd, mut wr) = tokio::io::split(ch.into_stream());
    let mut rd = BufReader::new(rd);
    wr.write_all(&[0]).await?;
    let (size, _) = loop {
        let mut line = String::new();
        if rd.read_line(&mut line).await? == 0 {
            return Err(err("SCP : le serveur a fermé la connexion (scp absent ?)"));
        }
        match line.as_bytes().first() {
            Some(b'C') => {
                let (_, size, name) = parse_c_line(&line).ok_or_else(|| err(format!("SCP : réponse inattendue {line:?}")))?;
                break (size, name);
            }
            Some(b'T') => wr.write_all(&[0]).await?, // times: acknowledged, not used
            Some(b'D') => return Err(err("SCP : c'est un dossier")),
            Some(1) | Some(2) => return Err(err(format!("SCP : {}", line[1..].trim()))),
            _ => return Err(err(format!("SCP : réponse inattendue {line:?}"))),
        }
    };
    wr.write_all(&[0]).await?;
    let tmp = local.with_extension("orbiterm-part");
    let result = async {
        let mut dst = tokio::fs::File::create(&tmp).await?;
        let mut buf = vec![0u8; CHUNK];
        let mut done = 0u64;
        while done < size {
            let want = buf.len().min((size - done) as usize);
            let n = rd.read(&mut buf[..want]).await?;
            if n == 0 {
                return Err(err("SCP : transfert interrompu"));
            }
            dst.write_all(&buf[..n]).await?;
            done += n as u64;
            progress(done, size);
        }
        dst.flush().await?;
        ack(&mut rd).await?;
        wr.write_all(&[0]).await?;
        let _ = wr.shutdown().await;
        AppResult::Ok(())
    }
    .await;
    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e);
    }
    tokio::fs::rename(&tmp, local).await?;
    progress(size, size);
    Ok(())
}

pub async fn upload(
    conn: &Arc<SshConnection>,
    local: &Path,
    remote: &str,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> AppResult<()> {
    let md = tokio::fs::metadata(local).await?;
    let total = md.len();
    let name = remote.rsplit('/').next().filter(|n| !n.is_empty()).ok_or_else(|| AppError::InvalidPath(remote.into()))?;
    if name.contains('\n') {
        return Err(AppError::InvalidPath(remote.into()));
    }
    let ch = conn.exec_channel(&format!("scp -t {}", sh_quote(remote))).await?;
    let (rd, mut wr) = tokio::io::split(ch.into_stream());
    let mut rd = BufReader::new(rd);
    ack(&mut rd).await?;
    wr.write_all(format!("C0644 {total} {name}\n").as_bytes()).await?;
    ack(&mut rd).await?;
    let mut src = tokio::fs::File::open(local).await?;
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    loop {
        let n = src.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        wr.write_all(&buf[..n]).await?;
        done += n as u64;
        progress(done, total);
    }
    if done != total {
        return Err(err("le fichier local a changé pendant l'envoi"));
    }
    wr.write_all(&[0]).await?;
    ack(&mut rd).await?;
    let _ = wr.shutdown().await;
    progress(total, total);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_and_parsing() {
        assert_eq!(sh_quote("a b'c"), r"'a b'\''c'");
        assert_eq!(parse_c_line("C0644 12 note été.txt\n"), Some((0o644, 12, "note été.txt".into())));
        assert_eq!(parse_c_line("D0755 0 x"), None);
        let (dir, v) = parse_listing(
            "/home/u\nf\t5\t1700000000.5\t644\tb.txt\nd\t4096\t1700000001.0\t755\tsrc\n\
             symbolic link\t7\t1700000002\t777\tlien\nregular file\t1\t3\t600\tA.txt\n",
        );
        assert_eq!(dir, "/home/u");
        assert_eq!(v.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["src", "A.txt", "b.txt", "lien"]);
        assert_eq!((v[0].kind.as_str(), v[0].path.as_str()), ("dir", "/home/u/src"));
        assert_eq!((v[2].size, v[2].mtime, v[2].perms.as_str()), (5, 1700000000, "rw-r--r--"));
        assert_eq!(v[3].kind, "link");
    }
}
