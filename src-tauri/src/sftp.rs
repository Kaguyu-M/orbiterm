//! SFTP operations used by the file browser panel.

use std::path::{Path, PathBuf};

use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::{AppError, AppResult};

const CHUNK: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Entry {
    pub name: String,
    pub path: String,
    /// "dir" | "file" | "link" | "other"
    pub kind: String,
    pub size: u64,
    /// Unix timestamp (seconds), 0 if unknown.
    pub mtime: u64,
    /// e.g. "rwxr-xr-x"
    pub perms: String,
}

/// Joins a remote directory and a name with '/' (remote paths are always POSIX).
pub fn remote_join(dir: &str, name: &str) -> String {
    if dir.is_empty() || dir == "." {
        name.to_string()
    } else if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// Parent of a remote POSIX path ("/a/b" -> "/a", "/a" -> "/").
pub fn remote_parent(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(i) => trimmed[..i].to_string(),
    }
}

/// Validates a file name coming from the server before using it locally:
/// no separators, no "..", no drive letters, not empty.
pub fn safe_local_join(dir: &Path, name: &str) -> AppResult<PathBuf> {
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains(':')
        || name.contains('\0');
    if bad {
        return Err(AppError::InvalidPath(name.to_string()));
    }
    Ok(dir.join(name))
}

pub fn perms_string(mode: u32) -> String {
    let flags = [
        (0o400, 'r'), (0o200, 'w'), (0o100, 'x'),
        (0o040, 'r'), (0o020, 'w'), (0o010, 'x'),
        (0o004, 'r'), (0o002, 'w'), (0o001, 'x'),
    ];
    flags.iter().map(|(bit, c)| if mode & bit != 0 { *c } else { '-' }).collect()
}

pub async fn home(sftp: &SftpSession) -> AppResult<String> {
    Ok(sftp.canonicalize(".").await?)
}

pub async fn list(sftp: &SftpSession, path: &str) -> AppResult<Vec<Entry>> {
    let path = sftp.canonicalize(path).await?;
    let mut out = Vec::new();
    for e in sftp.read_dir(path.clone()).await? {
        let name = e.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let md = e.metadata();
        let ft = e.file_type();
        let kind = if ft.is_dir() {
            "dir"
        } else if ft.is_symlink() {
            "link"
        } else if ft.is_file() {
            "file"
        } else {
            "other"
        };
        out.push(Entry {
            path: remote_join(&path, &name),
            name,
            kind: kind.into(),
            size: md.size.unwrap_or(0),
            mtime: md.mtime.unwrap_or(0) as u64,
            perms: perms_string(md.permissions.unwrap_or(0)),
        });
    }
    // Symlinks pointing to directories should be browsable: resolve them.
    for e in out.iter_mut().filter(|e| e.kind == "link") {
        if let Ok(md) = sftp.metadata(e.path.clone()).await
            && md.file_type().is_dir() {
                e.kind = "dir".into();
            }
    }
    out.sort_by(|a, b| {
        (a.kind != "dir").cmp(&(b.kind != "dir")).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

pub async fn mkdir(sftp: &SftpSession, path: &str) -> AppResult<()> {
    Ok(sftp.create_dir(path).await?)
}

/// Changes the permission bits of a file or directory.
pub async fn chmod(sftp: &SftpSession, path: &str, mode: u32) -> AppResult<()> {
    let mut attrs = russh_sftp::protocol::FileAttributes::empty();
    attrs.permissions = Some(mode & 0o7777);
    Ok(sftp.set_metadata(path, attrs).await?)
}

/// Creates an empty file (fails if the name is already taken).
pub async fn touch(sftp: &SftpSession, path: &str) -> AppResult<()> {
    let mut f = sftp.open_with_flags(path, OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE).await?;
    f.shutdown().await?;
    Ok(())
}

pub async fn rename(sftp: &SftpSession, from: &str, to: &str) -> AppResult<()> {
    Ok(sftp.rename(from, to).await?)
}

/// Removes a file, or a directory recursively.
pub async fn remove(sftp: &SftpSession, path: &str) -> AppResult<()> {
    let md = sftp.symlink_metadata(path).await?;
    if md.file_type().is_dir() {
        // iterative depth-first removal
        let mut stack = vec![(path.to_string(), false)];
        while let Some((dir, children_done)) = stack.pop() {
            if children_done {
                sftp.remove_dir(dir).await?;
                continue;
            }
            stack.push((dir.clone(), true));
            for e in sftp.read_dir(dir.clone()).await? {
                let name = e.file_name();
                if name == "." || name == ".." {
                    continue;
                }
                let child = remote_join(&dir, &name);
                if e.file_type().is_dir() {
                    stack.push((child, false));
                } else {
                    sftp.remove_file(child).await?;
                }
            }
        }
        Ok(())
    } else {
        Ok(sftp.remove_file(path).await?)
    }
}

/// Downloads a remote file to `local`. `progress(done, total)` is called after each chunk.
pub async fn download(
    sftp: &SftpSession,
    remote: &str,
    local: &Path,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> AppResult<()> {
    let total = sftp.metadata(remote).await?.size.unwrap_or(0);
    let mut src = sftp.open(remote).await?;
    // write to a temp file first so a failed transfer never leaves a truncated file
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
    let _ = src.shutdown().await;
    tokio::fs::rename(&tmp, local).await?;
    progress(done, done.max(total));
    Ok(())
}

/// Uploads a local file to `remote` (created or truncated).
pub async fn upload(
    sftp: &SftpSession,
    local: &Path,
    remote: &str,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> AppResult<()> {
    let total = tokio::fs::metadata(local).await?.len();
    let mut src = tokio::fs::File::open(local).await?;
    let mut dst = sftp
        .open_with_flags(remote, OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE)
        .await?;
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
    dst.shutdown().await?;
    progress(done, total);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_and_parents() {
        assert_eq!(remote_join("/home/a", "f"), "/home/a/f");
        assert_eq!(remote_join("/", "f"), "/f");
        assert_eq!(remote_parent("/home/a"), "/home");
        assert_eq!(remote_parent("/home"), "/");
        assert_eq!(remote_parent("/"), "/");
    }

    #[test]
    fn rejects_dangerous_names() {
        let d = Path::new("C:/tmp");
        for bad in ["", ".", "..", "../x", "a/b", "a\\b", "C:evil", "x\0"] {
            assert!(safe_local_join(d, bad).is_err(), "{bad:?} should be rejected");
        }
        assert!(safe_local_join(d, "rapport.pdf").is_ok());
    }

    #[test]
    fn perms() {
        assert_eq!(perms_string(0o755), "rwxr-xr-x");
        assert_eq!(perms_string(0o640), "rw-r-----");
    }
}
