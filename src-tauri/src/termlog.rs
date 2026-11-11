//! Session logging: the text shown by a terminal tab, line by line, in
//! `<dir>/<session>_<date>_<time>.log`.
//!
//! The lines come from the tab once they are complete (as displayed, after the terminal
//! has interpreted cursor moves and redraws), so the file reads like the screen did.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct TermLog {
    /// None once closed
    out: Mutex<Option<BufWriter<File>>>,
    pub path: PathBuf,
}

impl TermLog {
    /// Creates the log file (and its folder).
    pub fn create(dir: &Path, session_name: &str) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let now = chrono::Local::now();
        let base = file_stem(session_name);
        let stamp = now.format("%Y-%m-%d_%H-%M-%S");
        let mut path = dir.join(format!("{base}_{stamp}.log"));
        let mut n = 2;
        while path.exists() {
            path = dir.join(format!("{base}_{stamp}_{n}.log"));
            n += 1;
        }
        let mut w = BufWriter::new(File::create(&path)?);
        if cfg!(windows) {
            w.write_all("\u{feff}".as_bytes())?; // UTF-8 marker for Windows editors
        }
        writeln!(w, "=== Orbiterm : {session_name} — {} ===", now.format("%d/%m/%Y %H:%M:%S"))?;
        w.flush()?;
        Ok(Self { out: Mutex::new(Some(w)), path })
    }

    /// Appends lines (separated by "\n"); control characters are dropped.
    pub fn write_text(&self, text: &str) {
        let clean: String = text.chars().filter(|&c| c == '\n' || c == '\t' || !c.is_control()).collect();
        if let Ok(mut g) = self.out.lock()
            && let Some(w) = g.as_mut()
        {
            let _ = w.write_all(clean.as_bytes());
            if !clean.ends_with('\n') {
                let _ = w.write_all(b"\n");
            }
            let _ = w.flush();
        }
    }

    /// Writes the closing line once.
    pub fn close(&self, reason: &str) {
        if let Ok(mut g) = self.out.lock()
            && let Some(mut w) = g.take()
        {
            let now = chrono::Local::now();
            let _ = writeln!(w, "=== {reason} — {} ===", now.format("%d/%m/%Y %H:%M:%S"));
            let _ = w.flush();
        }
    }
}

/// Default folder: Documents/Orbiterm/journaux (or ~/Orbiterm/journaux).
pub fn default_dir() -> PathBuf {
    directories::UserDirs::new()
        .map(|u| u.document_dir().unwrap_or(u.home_dir()).to_path_buf())
        .unwrap_or_else(std::env::temp_dir)
        .join("Orbiterm")
        .join("journaux")
}

fn file_stem(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '@') { c } else { '_' })
        .take(60)
        .collect();
    let s = s.trim_matches(['_', '.']).to_string();
    if s.is_empty() { "session".into() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_file() {
        let dir = tempfile::tempdir().unwrap();
        let l = TermLog::create(dir.path(), "srv / prod").unwrap();
        l.write_text("user@srv:~$ ls\x07\nfichier\tok");
        l.close("session fermée");
        l.close("deuxième fermeture ignorée");
        l.write_text("après la fermeture : ignoré");
        let name = l.path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("srv___prod_") && name.ends_with(".log"), "{name}");
        let text = std::fs::read_to_string(&l.path).unwrap();
        assert!(text.contains("user@srv:~$ ls\nfichier\tok\n=== session fermée"), "{text}");
        assert!(!text.contains("deuxième") && !text.contains("après"));
    }
}
