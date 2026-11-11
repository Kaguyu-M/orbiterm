//! Local file explorer shown next to the remote one in SFTP / FTP tabs: folder listing,
//! shortcuts (home, desktop, drives…), new folder, rename, move to the recycle bin.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::sftp::safe_local_join;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalEntry {
    pub name: String,
    pub path: String,
    /// "dir", "file" or "link" (a link to a folder counts as "dir")
    pub kind: String,
    pub size: u64,
    /// seconds since the Unix epoch (0 when unknown)
    pub mtime: u64,
    pub hidden: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalListing {
    /// absolute path of the listed folder
    pub path: String,
    /// parent folder, None at the root of a drive / of the file system
    pub parent: Option<String>,
    pub entries: Vec<LocalEntry>,
}

/// A shortcut of the local explorer (home, desktop, a drive…).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalPlace {
    pub label: String,
    pub path: String,
    /// "home", "folder" or "drive"
    pub kind: String,
}

pub fn home_dir() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_dir()).unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "C:\\" } else { "/" }))
}

fn text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[cfg(windows)]
fn is_hidden(_name: &str, md: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    md.file_attributes() & 0x2 != 0 // FILE_ATTRIBUTE_HIDDEN
}

#[cfg(not(windows))]
fn is_hidden(name: &str, _md: &std::fs::Metadata) -> bool {
    name.starts_with('.')
}

/// Lists a folder (the home folder when `path` is empty): folders first, then by name.
pub fn list(path: &str) -> AppResult<LocalListing> {
    let dir = if path.trim().is_empty() { home_dir() } else { PathBuf::from(path.trim()) };
    if !dir.is_absolute() {
        return Err(AppError::InvalidPath(format!("{} : chemin absolu attendu", dir.display())));
    }
    let mut entries = Vec::new();
    for item in std::fs::read_dir(&dir).map_err(|e| AppError::InvalidPath(format!("{} : {e}", dir.display())))? {
        let Ok(item) = item else { continue };
        let name = item.file_name().to_string_lossy().into_owned();
        // the entry itself (not what a link points to) tells hidden / link; the target tells folder
        let Ok(own) = item.metadata() else { continue };
        let target = std::fs::metadata(item.path()).ok();
        let is_dir = target.as_ref().is_some_and(|m| m.is_dir());
        let kind = if is_dir {
            "dir"
        } else if own.file_type().is_symlink() {
            "link"
        } else {
            "file"
        };
        let md = target.as_ref().unwrap_or(&own);
        let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        entries.push(LocalEntry {
            hidden: is_hidden(&name, &own),
            path: text(&item.path()),
            kind: kind.into(),
            size: if is_dir { 0 } else { md.len() },
            mtime,
            name,
        });
    }
    entries.sort_by(|a, b| (a.kind != "dir").cmp(&(b.kind != "dir")).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(LocalListing { parent: dir.parent().map(text), path: text(&dir), entries })
}

/// Shortcuts: home, usual folders that exist, then the drives (Windows) or mount points (Linux).
pub fn places() -> Vec<LocalPlace> {
    let home = home_dir();
    let mut out = vec![LocalPlace { label: "Dossier personnel".into(), path: text(&home), kind: "home".into() }];
    for (label, names) in [
        ("Bureau", &["Desktop", "Bureau"][..]),
        ("Documents", &["Documents"][..]),
        ("Téléchargements", &["Downloads", "Téléchargements"][..]),
    ] {
        if let Some(p) = names.iter().map(|n| home.join(n)).find(|p| p.is_dir()) {
            out.push(LocalPlace { label: label.into(), path: text(&p), kind: "folder".into() });
        }
    }
    #[cfg(windows)]
    for letter in b'A'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        // A: and B: are floppy drives: probing them can block for seconds
        if letter > b'B' && Path::new(&root).exists() {
            out.push(LocalPlace { label: format!("Disque {}:", letter as char), path: root, kind: "drive".into() });
        }
    }
    #[cfg(not(windows))]
    {
        out.push(LocalPlace { label: "Système de fichiers (/)".into(), path: "/".into(), kind: "drive".into() });
        let user = std::env::var("USER").unwrap_or_default();
        for base in [format!("/run/media/{user}"), format!("/media/{user}"), "/mnt".to_string()] {
            let Ok(rd) = std::fs::read_dir(&base) else { continue };
            for e in rd.flatten().filter(|e| e.path().is_dir()) {
                out.push(LocalPlace { label: e.file_name().to_string_lossy().into_owned(), path: text(&e.path()), kind: "drive".into() });
            }
        }
    }
    out
}

/// Creates a folder in `dir`; returns its path.
pub fn mkdir(dir: &str, name: &str) -> AppResult<String> {
    let p = safe_local_join(Path::new(dir), name.trim())?;
    std::fs::create_dir(&p).map_err(|e| AppError::InvalidPath(format!("{} : {e}", p.display())))?;
    Ok(text(&p))
}

/// Renames a file or folder inside its folder (never over an existing one); returns the new path.
pub fn rename(path: &str, new_name: &str) -> AppResult<String> {
    let from = Path::new(path);
    let dir = from.parent().ok_or_else(|| AppError::InvalidPath(path.to_string()))?;
    let to = safe_local_join(dir, new_name.trim())?;
    if to.exists() && !same_file_name_ignoring_case(from, &to) {
        return Err(AppError::InvalidPath(format!("« {} » existe déjà", new_name.trim())));
    }
    std::fs::rename(from, &to).map_err(|e| AppError::InvalidPath(format!("{} : {e}", from.display())))?;
    Ok(text(&to))
}

/// Renaming "a.txt" to "A.TXT" on a case-insensitive file system targets the same file.
fn same_file_name_ignoring_case(a: &Path, b: &Path) -> bool {
    a != b && text(a).to_lowercase() == text(b).to_lowercase()
}

/// Moves a file or folder to the recycle bin (nothing is erased for good).
pub fn to_trash(path: &str) -> AppResult<()> {
    let p = Path::new(path);
    if !p.is_absolute() || p.parent().is_none() {
        return Err(AppError::InvalidPath(path.to_string()));
    }
    trash::delete(p).map_err(|e| AppError::InvalidPath(format!("corbeille : {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_and_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        std::fs::write(d.join("b.txt"), "bbb").unwrap();
        std::fs::write(d.join("A.txt"), "a").unwrap();
        std::fs::create_dir(d.join("zdossier")).unwrap();
        let l = list(&text(d)).unwrap();
        let names: Vec<&str> = l.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["zdossier", "A.txt", "b.txt"], "folders first, then case-insensitive names");
        assert_eq!(l.entries[2].size, 3);
        assert_eq!(l.entries[0].kind, "dir");
        assert!(l.entries[1].mtime > 0);
        assert_eq!(l.parent.as_deref(), d.parent().map(text).as_deref());

        let made = mkdir(&text(d), "neuf").unwrap();
        assert!(Path::new(&made).is_dir());
        assert!(mkdir(&text(d), "neuf").is_err(), "already there");
        assert!(mkdir(&text(d), "../dehors").is_err());

        let renamed = rename(&text(&d.join("b.txt")), "c.txt").unwrap();
        assert!(Path::new(&renamed).is_file() && !d.join("b.txt").exists());
        assert!(rename(&renamed, "A.txt").is_err(), "never over an existing file");
        assert!(rename(&renamed, "x/y").is_err());

        assert!(list("relatif").is_err());
        assert!(list(&text(&d.join("absent"))).is_err());
        assert!(to_trash("relatif.txt").is_err());
    }

    #[test]
    fn shortcuts() {
        let p = places();
        assert_eq!(p[0].kind, "home");
        assert!(Path::new(&p[0].path).is_dir());
        assert!(p.iter().any(|x| x.kind == "drive"));
        assert!(list("").unwrap().path == p[0].path);
    }
}
