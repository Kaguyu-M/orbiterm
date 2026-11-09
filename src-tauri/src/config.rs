//! Session store: folders + sessions persisted in `sessions.toml`.
//!
//! Passwords are NEVER stored here (see `secrets.rs`). Writes are atomic:
//! the file is written to `sessions.toml.tmp` and then renamed over the original.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

pub const STORE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Folder {
    pub id: String,
    pub name: String,
    /// Parent folder id, "" for the root.
    #[serde(default)]
    pub parent: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    #[default]
    Password,
    Key,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TerminalSettings {
    pub font_family: String,
    pub font_size: u16,
    pub scrollback: u32,
    pub theme: String,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            font_family: "Cascadia Mono, Consolas, 'DejaVu Sans Mono', monospace".into(),
            font_size: 14,
            scrollback: 10_000,
            theme: "dark".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Session {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub folder: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub auth: AuthMethod,
    #[serde(default)]
    pub key_path: String,
    #[serde(default)]
    pub remember_password: bool,
    #[serde(default = "default_true")]
    pub sftp_enabled: bool,
    #[serde(default)]
    pub terminal: TerminalSettings,
}

fn default_kind() -> String {
    "ssh".into()
}
fn default_port() -> u16 {
    22
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Store {
    pub version: u32,
    #[serde(default)]
    pub folders: Vec<Folder>,
    #[serde(default)]
    pub sessions: Vec<Session>,
}

impl Default for Store {
    fn default() -> Self {
        Self { version: STORE_VERSION, folders: vec![], sessions: vec![] }
    }
}

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4().simple())
}

/// Application data directory. `ORBITERM_CONFIG_DIR` overrides it (tests, portable mode).
pub fn config_dir() -> AppResult<PathBuf> {
    if let Ok(dir) = std::env::var("ORBITERM_CONFIG_DIR") {
        return Ok(PathBuf::from(dir));
    }
    directories::ProjectDirs::from("com", "Orbiterm", "Orbiterm")
        .map(|d| d.config_dir().to_path_buf())
        .ok_or_else(|| AppError::Config("dossier de configuration introuvable".into()))
}

impl Store {
    fn file(dir: &Path) -> PathBuf {
        dir.join("sessions.toml")
    }

    pub fn load(dir: &Path) -> AppResult<Store> {
        let path = Self::file(dir);
        if !path.exists() {
            return Ok(Store::default());
        }
        let text = std::fs::read_to_string(&path)?;
        let store: Store =
            toml::from_str(&text).map_err(|e| AppError::Config(format!("{}: {e}", path.display())))?;
        Ok(store)
    }

    pub fn save(&self, dir: &Path) -> AppResult<()> {
        std::fs::create_dir_all(dir)?;
        let text = toml::to_string_pretty(self).map_err(|e| AppError::Config(e.to_string()))?;
        let path = Self::file(dir);
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Insert or replace a session (matched by id). An empty id gets a new one.
    pub fn upsert_session(&mut self, mut s: Session) -> Session {
        if s.id.is_empty() {
            s.id = new_id("s");
        }
        match self.sessions.iter_mut().find(|x| x.id == s.id) {
            Some(existing) => *existing = s.clone(),
            None => self.sessions.push(s.clone()),
        }
        s
    }

    pub fn upsert_folder(&mut self, mut f: Folder) -> AppResult<Folder> {
        if f.id.is_empty() {
            f.id = new_id("f");
        }
        // A folder cannot be moved inside itself or one of its descendants.
        let mut p = f.parent.clone();
        while !p.is_empty() {
            if p == f.id {
                return Err(AppError::Config("un dossier ne peut pas être son propre parent".into()));
            }
            p = self.folders.iter().find(|x| x.id == p).map(|x| x.parent.clone()).unwrap_or_default();
        }
        match self.folders.iter_mut().find(|x| x.id == f.id) {
            Some(existing) => *existing = f.clone(),
            None => self.folders.push(f.clone()),
        }
        Ok(f)
    }

    pub fn delete_session(&mut self, id: &str) -> bool {
        let before = self.sessions.len();
        self.sessions.retain(|s| s.id != id);
        before != self.sessions.len()
    }

    /// Deletes a folder, its sub-folders and every session they contain.
    /// Returns the ids of deleted sessions (so their secrets can be removed too).
    pub fn delete_folder(&mut self, id: &str) -> Vec<String> {
        let mut to_delete = vec![id.to_string()];
        let mut i = 0;
        while i < to_delete.len() {
            let cur = to_delete[i].clone();
            to_delete.extend(self.folders.iter().filter(|f| f.parent == cur).map(|f| f.id.clone()));
            i += 1;
        }
        self.folders.retain(|f| !to_delete.contains(&f.id));
        let removed: Vec<String> = self
            .sessions
            .iter()
            .filter(|s| to_delete.contains(&s.folder))
            .map(|s| s.id.clone())
            .collect();
        self.sessions.retain(|s| !to_delete.contains(&s.folder));
        removed
    }

    pub fn session(&self, id: &str) -> AppResult<&Session> {
        self.sessions.iter().find(|s| s.id == id).ok_or_else(|| AppError::NotFound(id.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Session {
        Session {
            id: String::new(),
            name: "web-01".into(),
            folder: String::new(),
            kind: "ssh".into(),
            host: "10.0.0.5".into(),
            port: 22,
            username: "admin".into(),
            auth: AuthMethod::Password,
            key_path: String::new(),
            remember_password: true,
            sftp_enabled: true,
            terminal: TerminalSettings::default(),
        }
    }

    #[test]
    fn roundtrip_toml() {
        let dir = tempfile::tempdir().unwrap();
        let mut st = Store::default();
        let f = st.upsert_folder(Folder { id: String::new(), name: "Prod".into(), parent: String::new() }).unwrap();
        let mut s = sample();
        s.folder = f.id.clone();
        let s = st.upsert_session(s);
        st.save(dir.path()).unwrap();
        let loaded = Store::load(dir.path()).unwrap();
        assert_eq!(loaded, st);
        assert!(s.id.starts_with("s-"));
        // no password field may ever appear in the file
        let text = std::fs::read_to_string(dir.path().join("sessions.toml")).unwrap();
        assert!(!text.lines().any(|l| l.trim_start().starts_with("password")));
    }

    #[test]
    fn missing_file_gives_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Store::load(dir.path()).unwrap(), Store::default());
    }

    #[test]
    fn delete_folder_is_recursive() {
        let mut st = Store::default();
        let a = st.upsert_folder(Folder { id: String::new(), name: "A".into(), parent: String::new() }).unwrap();
        let b = st.upsert_folder(Folder { id: String::new(), name: "B".into(), parent: a.id.clone() }).unwrap();
        let mut s1 = sample();
        s1.folder = b.id.clone();
        let s1 = st.upsert_session(s1);
        let s2 = st.upsert_session(sample()); // root
        let removed = st.delete_folder(&a.id);
        assert_eq!(removed, vec![s1.id]);
        assert!(st.folders.is_empty());
        assert_eq!(st.sessions.len(), 1);
        assert_eq!(st.sessions[0].id, s2.id);
    }

    #[test]
    fn folder_cycle_rejected() {
        let mut st = Store::default();
        let a = st.upsert_folder(Folder { id: String::new(), name: "A".into(), parent: String::new() }).unwrap();
        let b = st.upsert_folder(Folder { id: String::new(), name: "B".into(), parent: a.id.clone() }).unwrap();
        let moved = Folder { id: a.id.clone(), name: "A".into(), parent: b.id.clone() };
        assert!(st.upsert_folder(moved).is_err());
    }

    #[test]
    fn upsert_replaces_existing() {
        let mut st = Store::default();
        let mut s = st.upsert_session(sample());
        s.name = "renamed".into();
        st.upsert_session(s.clone());
        assert_eq!(st.sessions.len(), 1);
        assert_eq!(st.sessions[0].name, "renamed");
    }
}
