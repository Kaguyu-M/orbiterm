//! Export / import of the whole configuration (sessions, folders, macros, tunnels and the
//! interface settings) in one JSON file, to move it to another computer or keep a backup.
//! Passwords are never exported: they stay in the system keyring.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::{Folder, Store, new_id};
use crate::error::{AppError, AppResult};

pub const FORMAT: &str = "orbiterm-config";
pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    pub format: String,
    pub format_version: u32,
    #[serde(default)]
    pub app_version: String,
    #[serde(default)]
    pub exported: String,
    #[serde(default)]
    pub store: Store,
    /// interface settings of the front-end (free JSON), when exported
    #[serde(default)]
    pub prefs: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MergeStats {
    pub sessions: usize,
    pub folders: usize,
    pub macros: usize,
    pub tunnels: usize,
    /// already present (same content): not duplicated
    pub skipped: usize,
}

pub fn export(path: &Path, store: &Store, prefs: Option<serde_json::Value>, app_version: &str) -> AppResult<()> {
    let mut store = store.clone();
    for s in &mut store.sessions {
        // "remember the password" means nothing on the other computer: asked once there
        s.remember_password = false;
    }
    let b = Bundle {
        format: FORMAT.into(),
        format_version: FORMAT_VERSION,
        app_version: app_version.into(),
        exported: chrono::Local::now().to_rfc3339(),
        store,
        prefs,
    };
    let text = serde_json::to_string_pretty(&b).map_err(|e| AppError::Config(e.to_string()))?;
    std::fs::write(path, text)?;
    Ok(())
}

pub fn read(path: &Path) -> AppResult<Bundle> {
    let text = std::fs::read_to_string(path)?;
    let b: Bundle = serde_json::from_str(&text)
        .map_err(|e| AppError::Config(format!("ce fichier n'est pas une configuration Orbiterm ({e})")))?;
    if b.format != FORMAT {
        return Err(AppError::Config("ce fichier n'est pas une configuration Orbiterm".into()));
    }
    if b.format_version > FORMAT_VERSION {
        return Err(AppError::Config("configuration créée par une version plus récente d'Orbiterm".into()));
    }
    Ok(b)
}

/// Folder path from the root ("Prod/Web"), used to match folders between two stores.
fn folder_path(folders: &[Folder], id: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = id;
    for _ in 0..64 {
        let Some(f) = folders.iter().find(|f| f.id == cur) else { break };
        out.push(f.name.clone());
        cur = &f.parent;
    }
    out.reverse();
    out
}

fn ensure_path(store: &mut Store, path: &[String], created: &mut usize) -> String {
    let mut parent = String::new();
    for name in path {
        let before = store.folders.len();
        parent = store.ensure_folder(&parent, name);
        *created += store.folders.len() - before;
    }
    parent
}

/// Part of a store to put in a file: the folder `folder` with all it contains (`Some("")` =
/// every session), or else the sessions `sessions` alone, without their folders. The macros
/// run at startup and the tunnels of these sessions come with them.
pub fn subset(store: &Store, sessions: &[String], folder: Option<&str>) -> Store {
    let mut out = Store::default();
    match folder {
        Some("") => {
            out.folders = store.folders.clone();
            out.sessions = store.sessions.clone();
        }
        Some(root) => {
            let mut keep: Vec<&str> = vec![root];
            // descendants, whatever the order of the list
            loop {
                let more: Vec<&str> = store
                    .folders
                    .iter()
                    .filter(|f| keep.contains(&f.parent.as_str()) && !keep.contains(&f.id.as_str()))
                    .map(|f| f.id.as_str())
                    .collect();
                if more.is_empty() {
                    break;
                }
                keep.extend(more);
            }
            out.folders = store.folders.iter().filter(|f| keep.contains(&f.id.as_str())).cloned().collect();
            for f in &mut out.folders {
                if f.id == root {
                    f.parent.clear(); // the exported folder becomes a top-level one in the file
                }
            }
            out.sessions = store.sessions.iter().filter(|s| keep.contains(&s.folder.as_str())).cloned().collect();
        }
        None => {
            out.sessions = store.sessions.iter().filter(|s| sessions.contains(&s.id)).cloned().collect();
            for s in &mut out.sessions {
                s.folder.clear();
            }
        }
    }
    let used: Vec<&String> = out.sessions.iter().filter_map(|s| s.extra.get("startup_macro")).collect();
    out.macros = store.macros.iter().filter(|m| used.contains(&&m.id)).cloned().collect();
    out.tunnels = store.tunnels.iter().filter(|t| out.sessions.iter().any(|s| s.id == t.session_id)).cloned().collect();
    out
}

/// Adds the content of `other` to `store`: new ids, folders matched by path, references
/// (tunnel → session, session → startup macro) remapped; identical items are not duplicated.
pub fn merge(store: &mut Store, other: &Store) -> MergeStats {
    merge_into(store, other, "")
}

/// Same as [`merge`], the folders and sessions of `other` going under the folder `base`
/// of `store` ("" or an unknown id = the root).
pub fn merge_into(store: &mut Store, other: &Store, base: &str) -> MergeStats {
    let mut st = MergeStats::default();
    let base = folder_path(&store.folders, base);
    let under = |p: Vec<String>| [base.clone(), p].concat();

    // macros (by name + steps)
    let mut macro_ids: HashMap<String, String> = HashMap::new();
    for m in &other.macros {
        if let Some(same) = store.macros.iter().find(|x| x.name == m.name && x.steps == m.steps) {
            macro_ids.insert(m.id.clone(), same.id.clone());
            st.skipped += 1;
            continue;
        }
        let mut n = m.clone();
        n.id = new_id("m");
        macro_ids.insert(m.id.clone(), n.id.clone());
        store.macros.push(n);
        st.macros += 1;
    }

    // folders, then sessions (same content in the same folder = already there)
    for f in &other.folders {
        let p = under(folder_path(&other.folders, &f.id));
        ensure_path(store, &p, &mut st.folders);
    }
    let mut session_ids: HashMap<String, String> = HashMap::new();
    for s in &other.sessions {
        let p = under(folder_path(&other.folders, &s.folder));
        let folder = ensure_path(store, &p, &mut st.folders);
        let mut n = s.clone();
        n.folder = folder;
        if let Some(mid) = n.extra.get("startup_macro").cloned() {
            match macro_ids.get(&mid) {
                Some(new) => {
                    n.extra.insert("startup_macro".into(), new.clone());
                }
                None => {
                    n.extra.remove("startup_macro");
                }
            }
        }
        let same = store.sessions.iter().find(|x| {
            let mut a = (*x).clone();
            a.id.clear();
            a.remember_password = false;
            let mut b = n.clone();
            b.id.clear();
            b.remember_password = false;
            a == b
        });
        if let Some(same) = same {
            session_ids.insert(s.id.clone(), same.id.clone());
            st.skipped += 1;
            continue;
        }
        n.id = new_id("s");
        n.remember_password = false;
        session_ids.insert(s.id.clone(), n.id.clone());
        store.sessions.push(n);
        st.sessions += 1;
    }

    // tunnels (their SSH session must be known)
    for t in &other.tunnels {
        let Some(sid) = session_ids.get(&t.session_id) else { continue };
        let mut n = t.clone();
        n.session_id = sid.clone();
        n.autostart = false; // not started on the new computer without asking
        if store.tunnels.iter().any(|x| x.session_id == n.session_id && x.forward == n.forward) {
            st.skipped += 1;
            continue;
        }
        n.id = new_id("t");
        store.tunnels.push(n);
        st.tunnels += 1;
    }
    st
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Forward, Macro, MacroStep, Session, Tunnel};

    fn session(id: &str, name: &str, folder: &str) -> Session {
        let mut s: Session = toml::from_str(&format!("id = \"{id}\"\nname = \"{name}\"\nhost = \"srv\"\n")).unwrap();
        s.folder = folder.into();
        s.remember_password = true;
        s
    }

    fn sample() -> Store {
        let mut st = Store::default();
        st.folders.push(Folder { id: "f1".into(), name: "Prod".into(), parent: String::new() });
        st.folders.push(Folder { id: "f2".into(), name: "Web".into(), parent: "f1".into() });
        st.macros.push(Macro { id: "m1".into(), name: "ls".into(), steps: vec![MacroStep { data: "ls\r".into(), delay_ms: 0 }] });
        let mut s = session("s1", "web1", "f2");
        s.extra.insert("startup_macro".into(), "m1".into());
        st.sessions.push(s);
        st.tunnels.push(Tunnel { id: "t1".into(), name: "db".into(), session_id: "s1".into(), forward: Forward::default(), autostart: true });
        st
    }

    #[test]
    fn export_then_import_into_empty_and_again() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("conf.orbiterm.json");
        export(&file, &sample(), Some(serde_json::json!({"theme": "dark"})), "0.7.8").unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(!text.contains("\"remember_password\": true"));
        let b = read(&file).unwrap();
        assert_eq!(b.prefs.unwrap()["theme"], "dark");

        let mut mine = Store::default();
        let st = merge(&mut mine, &b.store);
        assert_eq!(st, MergeStats { sessions: 1, folders: 2, macros: 1, tunnels: 1, skipped: 0 });
        let s = &mine.sessions[0];
        assert_eq!(folder_path(&mine.folders, &s.folder), ["Prod", "Web"]);
        assert_eq!(s.extra["startup_macro"], mine.macros[0].id);
        assert_eq!(mine.tunnels[0].session_id, s.id);
        assert!(!mine.tunnels[0].autostart);

        // second import: nothing duplicated
        let st = merge(&mut mine, &b.store);
        assert_eq!(st, MergeStats { skipped: 3, ..Default::default() });
        assert_eq!((mine.sessions.len(), mine.folders.len(), mine.macros.len(), mine.tunnels.len()), (1, 2, 1, 1));
    }

    #[test]
    fn subset_of_a_folder_then_import_under_another() {
        let mut st = sample();
        st.sessions.push(session("s2", "ailleurs", ""));
        st.macros.push(Macro { id: "m2".into(), name: "inutile".into(), steps: vec![] });

        // folder "Web" (inside "Prod"): top-level in the file, with its macro and its tunnel
        let part = subset(&st, &[], Some("f2"));
        assert_eq!(part.folders.len(), 1);
        assert_eq!(part.folders[0].parent, "");
        assert_eq!(part.sessions.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["web1"]);
        assert_eq!((part.macros.len(), part.tunnels.len()), (1, 1));

        // a folder and what is below it
        let part = subset(&st, &[], Some("f1"));
        assert_eq!((part.folders.len(), part.sessions.len()), (2, 1));
        // everything
        let all = subset(&st, &[], Some(""));
        assert_eq!((all.folders.len(), all.sessions.len(), all.macros.len()), (2, 2, 1));
        // chosen sessions: no folder
        let two = subset(&st, &["s1".into(), "s2".into()], None);
        assert!(two.folders.is_empty() && two.sessions.iter().all(|s| s.folder.is_empty()));
        assert_eq!(two.sessions.len(), 2);

        // imported under an existing folder of another store
        let mut mine = Store::default();
        mine.folders.push(Folder { id: "a".into(), name: "Clients".into(), parent: String::new() });
        let part = subset(&st, &[], Some("f2"));
        let r = merge_into(&mut mine, &part, "a");
        assert_eq!((r.sessions, r.folders), (1, 1));
        assert_eq!(folder_path(&mine.folders, &mine.sessions[0].folder), ["Clients", "Web"]);
        // an unknown destination means the root
        let r = merge_into(&mut mine, &two, "disparu");
        assert_eq!(r.sessions, 2);
        assert!(mine.sessions.iter().filter(|s| s.folder.is_empty()).count() == 2);
    }

    #[test]
    fn rejects_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.json");
        std::fs::write(&f, r#"{"format":"autre","format_version":1}"#).unwrap();
        assert!(read(&f).is_err());
        std::fs::write(&f, "pas du json").unwrap();
        assert!(read(&f).is_err());
    }
}
