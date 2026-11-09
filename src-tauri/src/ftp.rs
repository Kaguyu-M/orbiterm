//! FTP / FTPS (explicit TLS) file sessions, used by the file browser like SFTP.

use std::net::ToSocketAddrs;
use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, UNIX_EPOCH};

use suppaftp::list::File;
use suppaftp::tokio::{AsyncNativeTlsConnector, AsyncNativeTlsFtpStream};
use suppaftp::types::FileType;
use suppaftp::{FtpError, Status};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

use crate::error::{AppError, AppResult};
use crate::sftp::{Entry, remote_join};

/// The client type works for plain FTP too (TLS is only used after `into_secure`).
type Ftp = AsyncNativeTlsFtpStream;
const CHUNK: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FtpTls {
    None,
    /// AUTH TLS on the control connection (FTPES)
    Explicit,
}

pub struct FtpParams {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub tls: FtpTls,
    /// accept self-signed / mismatching certificates (common on NAS / appliances)
    pub insecure: bool,
}

pub struct FtpConnection {
    inner: Mutex<Option<Ftp>>,
    home: String,
}

fn fe(e: FtpError) -> AppError {
    if let FtpError::UnexpectedResponse(r) = &e
        && r.status == Status::NotLoggedIn
    {
        return AppError::AuthFailed;
    }
    AppError::Conn(format!("FTP : {e}"))
}

fn entry(dir: &str, f: &File) -> Entry {
    let kind = if f.is_directory() {
        "dir"
    } else if f.is_symlink() {
        "link"
    } else {
        "file"
    };
    let mtime = f.modified().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    use suppaftp::list::PosixPexQuery::{Group, Others, Owner};
    let bits = [
        (f.can_read(Owner), 'r'),
        (f.can_write(Owner), 'w'),
        (f.can_execute(Owner), 'x'),
        (f.can_read(Group), 'r'),
        (f.can_write(Group), 'w'),
        (f.can_execute(Group), 'x'),
        (f.can_read(Others), 'r'),
        (f.can_write(Others), 'w'),
        (f.can_execute(Others), 'x'),
    ];
    Entry {
        name: f.name().to_string(),
        path: remote_join(dir, f.name()),
        kind: kind.into(),
        size: f.size() as u64,
        mtime,
        perms: bits.iter().map(|(on, c)| if *on { *c } else { '-' }).collect(),
    }
}

/// Parses LIST output (Unix or DOS style); unparsable lines are skipped.
pub fn parse_listing(dir: &str, lines: &[String]) -> Vec<Entry> {
    let mut out: Vec<Entry> = lines
        .iter()
        // "total N" header of `ls -l` style listings
        .filter(|l| !l.starts_with("total "))
        .filter_map(|l| File::from_str(l.trim_end()).ok())
        .filter(|f| f.name() != "." && f.name() != "..")
        .map(|f| entry(dir, &f))
        .collect();
    out.sort_by(|a, b| {
        (a.kind != "dir").cmp(&(b.kind != "dir")).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

impl FtpConnection {
    pub async fn connect(p: FtpParams) -> AppResult<FtpConnection> {
        let addr = (p.host.as_str(), p.port)
            .to_socket_addrs()
            .map_err(|e| AppError::Conn(format!("{}: {e}", p.host)))?
            .next()
            .ok_or_else(|| AppError::Conn(format!("adresse introuvable : {}", p.host)))?;
        let mut ftp = Ftp::connect_timeout(addr, Duration::from_secs(15)).await.map_err(fe)?;
        if p.tls == FtpTls::Explicit {
            let tls = async_native_tls::TlsConnector::new()
                .danger_accept_invalid_certs(p.insecure)
                .danger_accept_invalid_hostnames(p.insecure);
            ftp = ftp.into_secure(AsyncNativeTlsConnector::from(tls), &p.host).await.map_err(fe)?;
        }
        let (user, pass) = if p.username.is_empty() || p.username.eq_ignore_ascii_case("anonymous") {
            ("anonymous".to_string(), if p.password.is_empty() { "orbiterm@".to_string() } else { p.password })
        } else {
            (p.username, p.password)
        };
        ftp.login(user.as_str(), pass.as_str()).await.map_err(fe)?;
        ftp.transfer_type(FileType::Binary).await.map_err(fe)?;
        let home = ftp.pwd().await.unwrap_or_else(|_| "/".into());
        Ok(FtpConnection { inner: Mutex::new(Some(ftp)), home })
    }

    pub fn home(&self) -> String {
        self.home.clone()
    }

    async fn with<T>(&self, f: impl AsyncFnOnce(&mut Ftp) -> AppResult<T>) -> AppResult<T> {
        let mut g = self.inner.lock().await;
        let ftp = g.as_mut().ok_or_else(|| AppError::Conn("connexion FTP fermée".into()))?;
        f(ftp).await
    }

    async fn list_in(ftp: &mut Ftp, path: &str) -> AppResult<(String, Vec<Entry>)> {
        ftp.cwd(path).await.map_err(fe)?;
        let dir = ftp.pwd().await.unwrap_or_else(|_| path.to_string());
        let lines = ftp.list(None).await.map_err(fe)?;
        Ok((dir.clone(), parse_listing(&dir, &lines)))
    }

    /// Lists a directory; returns entries with absolute paths.
    pub async fn list(&self, path: &str) -> AppResult<Vec<Entry>> {
        self.with(async |ftp| Ok(Self::list_in(ftp, path).await?.1)).await
    }

    pub async fn mkdir(&self, path: &str) -> AppResult<()> {
        self.with(async |ftp| ftp.mkdir(path).await.map_err(fe)).await
    }

    pub async fn rename(&self, from: &str, to: &str) -> AppResult<()> {
        self.with(async |ftp| ftp.rename(from, to).await.map_err(fe)).await
    }

    /// Removes a file, or a directory recursively.
    pub async fn remove(&self, path: &str) -> AppResult<()> {
        self.with(async |ftp| {
            if ftp.rm(path).await.is_ok() {
                return Ok(());
            }
            // a directory: depth-first
            let mut stack = vec![(path.to_string(), false)];
            while let Some((dir, done)) = stack.pop() {
                if done {
                    ftp.rmdir(&dir).await.map_err(fe)?;
                    continue;
                }
                stack.push((dir.clone(), true));
                let (_, entries) = Self::list_in(ftp, &dir).await?;
                for e in entries {
                    if e.kind == "dir" {
                        stack.push((e.path, false));
                    } else {
                        ftp.rm(&e.path).await.map_err(fe)?;
                    }
                }
            }
            let _ = ftp.cwd("/").await;
            Ok(())
        })
        .await
    }

    pub async fn download(&self, remote: &str, local: &Path, progress: &(dyn Fn(u64, u64) + Send + Sync)) -> AppResult<()> {
        self.with(async |ftp| {
            let total = ftp.size(remote).await.map(|s| s as u64).unwrap_or(0);
            let mut src = ftp.retr_as_stream(remote).await.map_err(fe)?;
            let tmp = local.with_extension("orbiterm-part");
            let mut dst = tokio::fs::File::create(&tmp).await?;
            let mut buf = vec![0u8; CHUNK];
            let mut done = 0u64;
            loop {
                let n = src.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                dst.write_all(&buf[..n]).await?;
                done += n as u64;
                progress(done, total);
            }
            dst.flush().await?;
            drop(dst);
            src.finish().await.map_err(fe)?;
            tokio::fs::rename(&tmp, local).await?;
            progress(done, done.max(total));
            Ok(())
        })
        .await
    }

    pub async fn upload(&self, local: &Path, remote: &str, progress: &(dyn Fn(u64, u64) + Send + Sync)) -> AppResult<()> {
        self.with(async |ftp| {
            let total = tokio::fs::metadata(local).await?.len();
            let mut src = tokio::fs::File::open(local).await?;
            let mut dst = ftp.put_with_stream(remote).await.map_err(fe)?;
            let mut buf = vec![0u8; CHUNK];
            let mut done = 0u64;
            loop {
                let n = src.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                dst.write_all(&buf[..n]).await?;
                done += n as u64;
                progress(done, total);
            }
            dst.flush().await?;
            dst.finish().await.map_err(fe)?;
            progress(done, total);
            Ok(())
        })
        .await
    }

    pub async fn close(&self) {
        if let Some(mut ftp) = self.inner.lock().await.take() {
            let _ = tokio::time::timeout(Duration::from_secs(3), ftp.quit()).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_unix_and_dos() {
        let lines: Vec<String> = [
            "drwxr-xr-x    2 1000     1000         4096 Jan 10 12:00 docs",
            "-rw-r--r--    1 1000     1000         1234 Jan 10 12:00 notes.txt",
            "lrwxrwxrwx    1 0        0               7 Jan 10 12:00 link -> notes.txt",
            "drwxr-xr-x    2 1000     1000         4096 Jan 10 12:00 .",
            "01-10-24  12:00PM       <DIR>          Windows",
            "01-10-24  12:00PM                 42 a.bin",
            "total 12",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let e = parse_listing("/home/u", &lines);
        let names: Vec<_> = e.iter().map(|x| (x.name.as_str(), x.kind.as_str())).collect();
        assert_eq!(names, vec![("docs", "dir"), ("Windows", "dir"), ("a.bin", "file"), ("link", "link"), ("notes.txt", "file")]);
        assert_eq!(e[0].path, "/home/u/docs");
        assert_eq!(e[4].size, 1234);
        assert_eq!(e[4].perms, "rw-r--r--");
    }
}
