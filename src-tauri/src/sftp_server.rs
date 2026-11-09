//! SFTP server side (russh-sftp), confined to a root directory. Used by the built-in SSH
//! server and by the test SSH server.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

use russh_sftp::protocol::{Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode, Version};

/// Normalizes a client path to an absolute virtual path ("/a/b"), resolving "." and "..".
pub fn virt(path: &str) -> String {
    let mut parts: Vec<&str> = vec![];
    for c in path.split(['/', '\\']) {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            x => parts.push(x),
        }
    }
    format!("/{}", parts.join("/"))
}

/// Maps a client path to a real path inside `root`; it can never escape `root`
/// (only plain path components are kept, drive prefixes and ".." are dropped).
pub fn confine(root: &Path, path: &str) -> PathBuf {
    let v = virt(path);
    let mut p = root.to_path_buf();
    for c in Path::new(v.trim_start_matches('/')).components() {
        if let Component::Normal(n) = c {
            p.push(n);
        }
    }
    p
}

enum OpenHandle {
    Dir { path: PathBuf, done: bool },
    File(std::fs::File),
}

/// Called with a short description of each change made by a client ("envoi /a.txt"…).
pub type ActivitySink = std::sync::Arc<dyn Fn(String) + Send + Sync>;

pub struct SftpServer {
    root: PathBuf,
    writable: bool,
    handles: HashMap<String, OpenHandle>,
    next: u64,
    activity: Option<ActivitySink>,
}

impl SftpServer {
    pub fn new(root: PathBuf, writable: bool) -> Self {
        Self { root, writable, handles: HashMap::new(), next: 0, activity: None }
    }

    pub fn with_activity(mut self, sink: ActivitySink) -> Self {
        self.activity = Some(sink);
        self
    }

    fn real(&self, path: &str) -> PathBuf {
        confine(&self.root, path)
    }

    fn add(&mut self, h: OpenHandle) -> String {
        self.next += 1;
        let id = self.next.to_string();
        self.handles.insert(id.clone(), h);
        id
    }

    fn check_write(&self) -> Result<(), StatusCode> {
        if self.writable { Ok(()) } else { Err(StatusCode::PermissionDenied) }
    }

    fn note(&self, what: String) {
        if let Some(s) = &self.activity {
            s(what);
        }
    }
}

fn ok(id: u32) -> Status {
    Status { id, status_code: StatusCode::Ok, error_message: "Ok".into(), language_tag: "en-US".into() }
}

fn io_err(e: std::io::Error) -> StatusCode {
    match e.kind() {
        std::io::ErrorKind::NotFound => StatusCode::NoSuchFile,
        std::io::ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
        _ => StatusCode::Failure,
    }
}

impl russh_sftp::server::Handler for SftpServer {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(&mut self, _version: u32, _ext: HashMap<String, String>) -> Result<Version, Self::Error> {
        Ok(Version::new())
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        Ok(Name { id, files: vec![File::dummy(virt(&path))] })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let md = std::fs::metadata(self.real(&path)).map_err(io_err)?;
        Ok(Attrs { id, attrs: FileAttributes::from(&md) })
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let md = std::fs::symlink_metadata(self.real(&path)).map_err(io_err)?;
        Ok(Attrs { id, attrs: FileAttributes::from(&md) })
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, Self::Error> {
        match self.handles.get(&handle) {
            Some(OpenHandle::File(f)) => Ok(Attrs { id, attrs: FileAttributes::from(&f.metadata().map_err(io_err)?) }),
            Some(OpenHandle::Dir { path, .. }) => {
                Ok(Attrs { id, attrs: FileAttributes::from(&std::fs::metadata(path).map_err(io_err)?) })
            }
            None => Err(StatusCode::Failure),
        }
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        let real = self.real(&path);
        if !real.is_dir() {
            return Err(StatusCode::NoSuchFile);
        }
        let handle = self.add(OpenHandle::Dir { path: real, done: false });
        Ok(Handle { id, handle })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        let Some(OpenHandle::Dir { path, done }) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };
        if *done {
            return Err(StatusCode::Eof);
        }
        *done = true;
        let mut files = vec![];
        for e in std::fs::read_dir(path).map_err(io_err)?.flatten() {
            if let Ok(md) = e.metadata() {
                files.push(File::new(e.file_name().to_string_lossy(), FileAttributes::from(&md)));
            }
        }
        Ok(Name { id, files })
    }

    async fn open(&mut self, id: u32, filename: String, pflags: OpenFlags, _a: FileAttributes) -> Result<Handle, Self::Error> {
        let write = pflags.intersects(OpenFlags::WRITE | OpenFlags::APPEND | OpenFlags::CREATE | OpenFlags::TRUNCATE);
        if write {
            self.check_write()?;
        }
        let real = self.real(&filename);
        let f = std::fs::OpenOptions::new()
            .read(pflags.contains(OpenFlags::READ))
            .write(pflags.contains(OpenFlags::WRITE) || pflags.contains(OpenFlags::APPEND))
            .append(pflags.contains(OpenFlags::APPEND))
            .create(pflags.contains(OpenFlags::CREATE))
            .truncate(pflags.contains(OpenFlags::TRUNCATE))
            .open(real)
            .map_err(io_err)?;
        let v = virt(&filename);
        self.note(if write { format!("envoi {v}") } else { format!("lecture {v}") });
        let handle = self.add(OpenHandle::File(f));
        Ok(Handle { id, handle })
    }

    async fn read(&mut self, id: u32, handle: String, offset: u64, len: u32) -> Result<Data, Self::Error> {
        let Some(OpenHandle::File(f)) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };
        f.seek(SeekFrom::Start(offset)).map_err(io_err)?;
        // Like real servers, cap the reply so the packet stays under the 256 KiB SFTP limit.
        let mut buf = vec![0u8; len.min(32 * 1024) as usize];
        let n = f.read(&mut buf).map_err(io_err)?;
        if n == 0 {
            return Err(StatusCode::Eof);
        }
        buf.truncate(n);
        Ok(Data { id, data: buf })
    }

    async fn write(&mut self, id: u32, handle: String, offset: u64, data: Vec<u8>) -> Result<Status, Self::Error> {
        self.check_write()?;
        let Some(OpenHandle::File(f)) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };
        f.seek(SeekFrom::Start(offset)).map_err(io_err)?;
        f.write_all(&data).map_err(io_err)?;
        Ok(ok(id))
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        self.handles.remove(&handle);
        Ok(ok(id))
    }

    async fn mkdir(&mut self, id: u32, path: String, _a: FileAttributes) -> Result<Status, Self::Error> {
        self.check_write()?;
        std::fs::create_dir(self.real(&path)).map_err(io_err)?;
        self.note(format!("nouveau dossier {}", virt(&path)));
        Ok(ok(id))
    }

    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, Self::Error> {
        self.check_write()?;
        std::fs::remove_dir(self.real(&path)).map_err(io_err)?;
        self.note(format!("dossier supprimé {}", virt(&path)));
        Ok(ok(id))
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
        self.check_write()?;
        std::fs::remove_file(self.real(&filename)).map_err(io_err)?;
        self.note(format!("fichier supprimé {}", virt(&filename)));
        Ok(ok(id))
    }

    async fn rename(&mut self, id: u32, oldpath: String, newpath: String) -> Result<Status, Self::Error> {
        self.check_write()?;
        std::fs::rename(self.real(&oldpath), self.real(&newpath)).map_err(io_err)?;
        self.note(format!("renommé {} → {}", virt(&oldpath), virt(&newpath)));
        Ok(ok(id))
    }

    async fn setstat(&mut self, id: u32, _path: String, _a: FileAttributes) -> Result<Status, Self::Error> {
        Ok(ok(id))
    }

    async fn fsetstat(&mut self, id: u32, _h: String, _a: FileAttributes) -> Result<Status, Self::Error> {
        Ok(ok(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confine_never_escapes() {
        let root = PathBuf::from("racine");
        for p in ["../../etc/passwd", "/../x", "a/../../b", "..\\..\\win.ini", "C:/Windows/x", "./a/./b/"] {
            let r = confine(&root, p);
            assert!(r.starts_with(&root), "{p} -> {r:?}");
            assert!(!r.components().any(|c| matches!(c, Component::ParentDir)), "{p} -> {r:?}");
        }
        assert_eq!(confine(&root, "/a/b/../c"), root.join("a").join("c"));
        assert_eq!(virt("a//b/./c/.."), "/a/b");
    }
}
