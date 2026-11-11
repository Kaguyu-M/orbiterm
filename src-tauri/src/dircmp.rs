//! Folder comparison (like the diff tool of the original): which files exist only on one
//! side, and which differ (size first, then content).

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};

/// Entries compared at most (both sides together).
const MAX_ENTRIES: usize = 50_000;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Same,
    Different,
    LeftOnly,
    RightOnly,
    /// a file on one side, a folder on the other
    KindMismatch,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffEntry {
    /// path relative to both roots, with "/"
    pub path: String,
    pub dir: bool,
    pub status: Status,
    pub left_size: Option<u64>,
    pub right_size: Option<u64>,
    pub left_mtime: Option<i64>,
    pub right_mtime: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DirDiff {
    pub entries: Vec<DiffEntry>,
    /// stopped at MAX_ENTRIES
    pub truncated: bool,
}

#[derive(Clone, Copy)]
struct Meta {
    dir: bool,
    size: u64,
    mtime: i64,
}

fn walk(root: &Path, out: &mut BTreeMap<String, Meta>, budget: &mut usize) -> AppResult<bool> {
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        let rd = match std::fs::read_dir(root.join(&rel)) {
            Ok(r) => r,
            Err(e) if rel.as_os_str().is_empty() => return Err(e.into()),
            Err(_) => continue, // unreadable sub-folder
        };
        for e in rd.flatten() {
            if *budget == 0 {
                return Ok(true);
            }
            *budget -= 1;
            let Ok(m) = e.metadata() else { continue };
            let r = rel.join(e.file_name());
            let key = r.to_string_lossy().replace('\\', "/");
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            // symbolic links are not followed (no loops)
            let dir = m.is_dir() && !e.file_type().map(|t| t.is_symlink()).unwrap_or(false);
            out.insert(key, Meta { dir, size: if dir { 0 } else { m.len() }, mtime });
            if dir {
                stack.push(r);
            }
        }
    }
    Ok(false)
}

fn hash(p: &Path) -> Option<[u8; 32]> {
    let mut f = std::fs::File::open(p).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(h.finalize().into())
}

pub fn compare(left: &Path, right: &Path) -> AppResult<DirDiff> {
    for p in [left, right] {
        if !p.is_dir() {
            return Err(AppError::InvalidPath(format!("{} n'est pas un dossier", p.display())));
        }
    }
    let mut budget = MAX_ENTRIES;
    let (mut l, mut r) = (BTreeMap::new(), BTreeMap::new());
    let mut truncated = walk(left, &mut l, &mut budget)?;
    truncated |= walk(right, &mut r, &mut budget)?;

    let mut keys: Vec<&String> = l.keys().chain(r.keys()).collect();
    keys.sort();
    keys.dedup();
    let mut entries = Vec::with_capacity(keys.len());
    for k in keys {
        let (a, b) = (l.get(k).copied(), r.get(k).copied());
        let status = match (a, b) {
            (Some(_), None) => Status::LeftOnly,
            (None, Some(_)) => Status::RightOnly,
            (Some(a), Some(b)) if a.dir != b.dir => Status::KindMismatch,
            (Some(a), Some(_)) if a.dir => Status::Same, // folders: their content is listed
            (Some(a), Some(b)) if a.size != b.size => Status::Different,
            (Some(_), Some(_)) => {
                if hash(&left.join(k)) == hash(&right.join(k)) { Status::Same } else { Status::Different }
            }
            (None, None) => continue,
        };
        let dir = a.or(b).map(|m| m.dir).unwrap_or(false);
        entries.push(DiffEntry {
            path: k.clone(),
            dir,
            status,
            left_size: a.filter(|m| !m.dir).map(|m| m.size),
            right_size: b.filter(|m| !m.dir).map(|m| m.size),
            left_mtime: a.map(|m| m.mtime),
            right_mtime: b.map(|m| m.mtime),
        });
    }
    // a folder that contains a difference is shown as different
    let changed: Vec<String> = entries.iter().filter(|e| e.status != Status::Same).map(|e| e.path.clone()).collect();
    for e in entries.iter_mut().filter(|e| e.dir && e.status == Status::Same) {
        let prefix = format!("{}/", e.path);
        if changed.iter().any(|c| c.starts_with(&prefix)) {
            e.status = Status::Different;
        }
    }
    Ok(DirDiff { entries, truncated })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_two_trees() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let w = |d: &Path, p: &str, c: &str| {
            let f = d.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, c).unwrap();
        };
        w(a.path(), "same.txt", "x");
        w(b.path(), "same.txt", "x");
        w(a.path(), "conf/app.ini", "port=1");
        w(b.path(), "conf/app.ini", "port=2"); // same size, other content
        w(a.path(), "only_left.txt", "l");
        w(b.path(), "only_right.txt", "r");
        w(a.path(), "lib/same.so", "bin");
        w(b.path(), "lib/same.so", "bin");
        let d = compare(a.path(), b.path()).unwrap();
        let st = |p: &str| d.entries.iter().find(|e| e.path == p).unwrap().status;
        assert_eq!(st("same.txt"), Status::Same);
        assert_eq!(st("conf/app.ini"), Status::Different);
        assert_eq!(st("conf"), Status::Different);
        assert_eq!(st("lib"), Status::Same);
        assert_eq!(st("only_left.txt"), Status::LeftOnly);
        assert_eq!(st("only_right.txt"), Status::RightOnly);
        assert!(!d.truncated);
        assert!(compare(a.path(), &a.path().join("same.txt")).is_err());
    }
}
