//! Session import from MobaXterm (`MobaXterm.ini`, `.moba` exports), PuTTY
//! (registry on Windows, `~/.putty/sessions` elsewhere) and OpenSSH `~/.ssh/config`.
//!
//! Only connection settings are imported: saved passwords are never read.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::{AuthMethod, Forward, ForwardKind, Session, Store, TerminalSettings};
use crate::error::{AppError, AppResult};

/// One parsed session and the folder path it belongs to (relative to the import root).
#[derive(Debug, Clone)]
pub struct Item {
    pub path: Vec<String>,
    pub session: Session,
}

#[derive(Debug, Default)]
pub struct Batch {
    pub items: Vec<Item>,
    /// folders to create even when empty
    pub folders: Vec<Vec<String>>,
    /// human readable reasons for entries that were not imported
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ImportReport {
    pub source: String,
    pub added: usize,
    pub duplicates: usize,
    pub folders: usize,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportSource {
    pub id: String,
    pub label: String,
    pub path: String,
    pub count: usize,
}

fn blank(kind: &str, name: &str) -> Session {
    Session {
        id: String::new(),
        name: name.to_string(),
        folder: String::new(),
        kind: kind.to_string(),
        host: String::new(),
        port: default_port(kind),
        username: String::new(),
        auth: AuthMethod::Password,
        key_path: String::new(),
        remember_password: true,
        sftp_enabled: true,
        terminal: TerminalSettings::default(),
        extra: Default::default(),
        forwards: vec![],
    }
}

fn default_port(kind: &str) -> u16 {
    match kind {
        "telnet" => 23,
        "rlogin" => 513,
        "rdp" => 3389,
        "vnc" => 5900,
        "ftp" => 21,
        _ => 22,
    }
}

fn home() -> PathBuf {
    directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf()).unwrap_or_default()
}

/// Text files written by Windows tools may be UTF-8 (with BOM) or ANSI.
fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(), // Latin-1
    }
}

// ------------------------------------------------------------------ MobaXterm

fn moba_unescape(s: &str) -> String {
    s.replace("__PTVIRG__", ";").replace("__DIEZE__", "#")
}

/// Expands MobaXterm path variables (`_ProfileDir_\.ssh\id_rsa`, …).
fn moba_path(s: &str, home: &Path) -> String {
    let h = home.to_string_lossy();
    let docs = home.join("Documents");
    s.replace("_ProfileDir_", &h)
        .replace("_MyDocuments_", &docs.to_string_lossy())
        .replace("_CurrentDrive_", "C:")
}

fn moba_bool(s: Option<&&str>) -> bool {
    matches!(s.map(|v| v.trim()), Some("-1") | Some("1"))
}

/// Parses one bookmark value: `#<icon>#<proto>%<fields…>#<terminal>#…`.
fn moba_session(name: &str, value: &str, home: &Path) -> Result<Session, String> {
    let blocks: Vec<&str> = value.trim().trim_start_matches('#').split('#').collect();
    let proto_block = blocks.get(1).ok_or_else(|| format!("{name} : format inconnu"))?;
    let f: Vec<&str> = proto_block.split('%').collect();
    let proto = f[0].trim();
    let kind = match proto {
        "0" => "ssh",
        "1" => "telnet",
        "2" => "rlogin",
        "4" => "rdp",
        "5" => "vnc",
        "6" => "ftp",
        "7" => "sftp",
        "11" => "browser",
        other => return Err(format!("{name} : type de session MobaXterm n°{other} non pris en charge")),
    };
    let field = |i: usize| f.get(i).map(|v| moba_unescape(v.trim())).unwrap_or_default();
    let mut s = blank(kind, &moba_unescape(name.trim()));
    s.host = field(1);
    if s.host.is_empty() {
        return Err(format!("{name} : hôte vide"));
    }
    if kind != "browser" {
        s.port = field(2).parse().unwrap_or(s.port);
        s.username = field(3);
    }
    if kind == "ssh" {
        // 5 X11, 6 compression, 7 remote command, 8-10 gateway, 14 private key
        if moba_bool(f.get(5)) {
            s.extra.insert("x11".into(), "1".into());
        }
        if moba_bool(f.get(6)) {
            s.extra.insert("compression".into(), "1".into());
        }
        let cmd = field(7);
        if !cmd.is_empty() {
            s.extra.insert("command".into(), cmd);
        }
        let gw = field(8);
        if !gw.is_empty() {
            s.extra.insert("jump_host".into(), gw);
            let gp = field(9);
            if !gp.is_empty() {
                s.extra.insert("jump_port".into(), gp);
            }
            let gu = field(10);
            if !gu.is_empty() {
                s.extra.insert("jump_user".into(), gu);
            }
        }
        let key = field(14);
        if !key.is_empty() {
            s.auth = AuthMethod::Key;
            s.key_path = moba_path(&key, home);
        }
    }
    // terminal block: font, size (points)
    if let Some(term) = blocks.get(2) {
        let t: Vec<&str> = term.split('%').collect();
        if let Some(pt) = t.get(1).and_then(|v| v.trim().parse::<f32>().ok())
            && pt > 4.0
            && (pt - 10.0).abs() > f32::EPSILON
        {
            s.terminal.font_size = (pt * 4.0 / 3.0).round() as u16;
        }
    }
    Ok(s)
}

pub fn parse_mobaxterm(text: &str, home: &Path) -> Batch {
    let mut b = Batch::default();
    let mut in_bookmarks = false;
    let mut path: Vec<String> = vec![];
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(sec) = line.trim().strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_bookmarks = sec == "Bookmarks" || sec.starts_with("Bookmarks_");
            path.clear();
            continue;
        }
        if !in_bookmarks {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        match key.trim() {
            "SubRep" => {
                path = value.split('\\').map(|p| moba_unescape(p.trim())).filter(|p| !p.is_empty()).collect();
                if !path.is_empty() {
                    b.folders.push(path.clone());
                }
            }
            "ImgNum" => {}
            name if value.trim_start().starts_with('#') => match moba_session(name, value, home) {
                Ok(session) => b.items.push(Item { path: path.clone(), session }),
                Err(e) => b.skipped.push(e),
            },
            _ => {}
        }
    }
    b
}

// ------------------------------------------------------------------ PuTTY

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(v) = u8::from_str_radix(hex, 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `L8080=host:80,4R2222=localhost:22,D1080,L127.0.0.1:3000=web:3000`
fn putty_forwards(spec: &str) -> Vec<Forward> {
    let mut out = vec![];
    for item in spec.split(',').map(str::trim).filter(|x| !x.is_empty()) {
        let item = item.trim_start_matches(['4', '6']);
        let (kind, rest) = match item.chars().next() {
            Some('L') => (ForwardKind::Local, &item[1..]),
            Some('R') => (ForwardKind::Remote, &item[1..]),
            Some('D') => (ForwardKind::Dynamic, &item[1..]),
            _ => continue,
        };
        let (src, dst) = rest.split_once('=').unwrap_or((rest, ""));
        let (bind_host, bind_port) = match src.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p),
            None => (String::new(), src),
        };
        let Ok(bind_port) = bind_port.parse() else { continue };
        let (dest_host, dest_port) = match dst.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse().unwrap_or(0)),
            None => (String::new(), 0),
        };
        if kind != ForwardKind::Dynamic && (dest_host.is_empty() || dest_port == 0) {
            continue;
        }
        out.push(Forward { kind, bind_host, bind_port, dest_host, dest_port });
    }
    out
}

/// Builds a session from the values of one PuTTY saved session.
pub fn putty_session(raw_name: &str, v: &HashMap<String, String>) -> Result<Session, String> {
    let name = url_decode(raw_name);
    let get = |k: &str| v.get(k).map(|s| s.trim().to_string()).unwrap_or_default();
    let proto = get("Protocol");
    let kind = match proto.as_str() {
        "ssh" | "" => "ssh",
        "telnet" => "telnet",
        "rlogin" => "rlogin",
        "serial" => "serial",
        other => return Err(format!("{name} : protocole PuTTY « {other} » non pris en charge")),
    };
    let mut s = blank(kind, &name);
    if kind == "serial" {
        let line = get("SerialLine");
        if line.is_empty() {
            return Err(format!("{name} : port série vide"));
        }
        s.extra.insert("serial_port".into(), line);
        s.extra.insert("baud".into(), get("SerialSpeed").if_empty("9600"));
        s.extra.insert("data_bits".into(), get("SerialDataBits").if_empty("8"));
        s.extra.insert("stop_bits".into(), if get("SerialStopHalfbits") == "4" { "2" } else { "1" }.into());
        s.extra.insert(
            "parity".into(),
            match get("SerialParity").as_str() {
                "1" => "odd",
                "2" => "even",
                _ => "none",
            }
            .into(),
        );
        s.extra.insert(
            "flow".into(),
            match get("SerialFlowControl").as_str() {
                "1" => "software",
                "2" => "hardware",
                _ => "none",
            }
            .into(),
        );
        return Ok(s);
    }
    let mut host = get("HostName");
    if let Some((u, h)) = host.clone().rsplit_once('@') {
        s.username = u.to_string();
        host = h.to_string();
    }
    if host.is_empty() {
        return Err(format!("{name} : hôte vide"));
    }
    s.host = host;
    s.port = get("PortNumber").parse().unwrap_or(s.port);
    let user = get("UserName");
    if !user.is_empty() {
        s.username = user;
    }
    if kind == "ssh" {
        let key = get("PublicKeyFile");
        if !key.is_empty() {
            s.auth = AuthMethod::Key;
            s.key_path = key;
        }
        if get("X11Forward") == "1" {
            s.extra.insert("x11".into(), "1".into());
        }
        if get("Compression") == "1" {
            s.extra.insert("compression".into(), "1".into());
        }
        let cmd = get("RemoteCommand");
        if !cmd.is_empty() {
            s.extra.insert("command".into(), cmd);
        }
        s.forwards = putty_forwards(&get("PortForwardings"));
    }
    Ok(s)
}

trait IfEmpty {
    fn if_empty(self, d: &str) -> String;
}
impl IfEmpty for String {
    fn if_empty(self, d: &str) -> String {
        if self.is_empty() { d.to_string() } else { self }
    }
}

fn putty_batch(sessions: Vec<(String, HashMap<String, String>)>) -> Batch {
    let mut b = Batch::default();
    for (name, values) in sessions {
        if name == "Default%20Settings" || name == "Default Settings" {
            continue;
        }
        match putty_session(&name, &values) {
            Ok(session) => b.items.push(Item { path: vec![], session }),
            Err(e) => b.skipped.push(e),
        }
    }
    b
}

#[cfg(windows)]
fn read_putty_raw() -> Vec<(String, HashMap<String, String>)> {
    read_putty_registry(r"Software\SimonTatham\PuTTY\Sessions")
}

/// Reads every sub key of `HKCU\<path>` as a PuTTY session.
#[cfg(windows)]
pub fn read_putty_registry(path: &str) -> Vec<(String, HashMap<String, String>)> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let Ok(root) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(path) else { return vec![] };
    let mut out = vec![];
    for name in root.enum_keys().flatten() {
        let Ok(k) = root.open_subkey(&name) else { continue };
        let mut map = HashMap::new();
        for (vname, _) in k.enum_values().flatten() {
            if let Ok(s) = k.get_value::<String, _>(&vname) {
                map.insert(vname, s);
            } else if let Ok(n) = k.get_value::<u32, _>(&vname) {
                map.insert(vname, n.to_string());
            }
        }
        out.push((name, map));
    }
    out
}

#[cfg(not(windows))]
fn read_putty_raw() -> Vec<(String, HashMap<String, String>)> {
    let dir = home().join(".putty").join("sessions");
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    rd.flatten()
        .filter_map(|e| {
            let text = std::fs::read(e.path()).ok()?;
            let map = decode(&text)
                .lines()
                .filter_map(|l| l.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
                .collect();
            Some((e.file_name().to_string_lossy().into_owned(), map))
        })
        .collect()
}

// ------------------------------------------------------------------ OpenSSH config

fn expand_tilde(p: &str, home: &Path) -> String {
    match p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
        Some(rest) => home.join(rest).to_string_lossy().into_owned(),
        None => p.to_string(),
    }
}

/// `[bind:]port`
fn split_bind(s: &str) -> Option<(String, u16)> {
    match s.rsplit_once(':') {
        Some((h, p)) => Some((h.trim_matches(['[', ']']).to_string(), p.parse().ok()?)),
        None => Some((String::new(), s.parse().ok()?)),
    }
}

fn ssh_forward(kind: ForwardKind, value: &str) -> Option<Forward> {
    let mut parts = value.split_whitespace();
    let (bind_host, bind_port) = split_bind(parts.next()?)?;
    let (dest_host, dest_port) = match kind {
        ForwardKind::Dynamic => (String::new(), 0),
        _ => {
            let (h, p) = parts.next()?.rsplit_once(':')?;
            (h.trim_matches(['[', ']']).to_string(), p.parse().ok()?)
        }
    };
    Some(Forward { kind, bind_host, bind_port, dest_host, dest_port })
}

pub fn parse_ssh_config(text: &str, home: &Path) -> Batch {
    let mut b = Batch::default();
    // (aliases, options in order)
    type HostBlock = (Vec<String>, Vec<(String, String)>);
    let mut blocks: Vec<HostBlock> = vec![];
    let mut in_match = false;
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let (k, v) = match l.split_once(|c: char| c.is_whitespace() || c == '=') {
            Some((k, v)) => (k.to_ascii_lowercase(), v.trim_start_matches(|c: char| c.is_whitespace() || c == '=').trim().to_string()),
            None => continue,
        };
        match k.as_str() {
            "host" => {
                in_match = false;
                let aliases: Vec<String> =
                    v.split_whitespace().filter(|a| !a.contains(['*', '?', '!'])).map(String::from).collect();
                blocks.push((aliases, vec![]));
            }
            "match" => in_match = true,
            _ if !in_match => {
                if let Some(last) = blocks.last_mut() {
                    last.1.push((k, v.trim_matches('"').to_string()));
                }
            }
            _ => {}
        }
    }
    for (aliases, opts) in blocks {
        for alias in aliases {
            let mut s = blank("ssh", &alias);
            s.host = alias.clone();
            let mut seen = std::collections::HashSet::new();
            for (k, v) in &opts {
                // first value wins, like OpenSSH (except forwards, which accumulate)
                let repeatable = matches!(k.as_str(), "localforward" | "remoteforward" | "dynamicforward");
                if !repeatable && !seen.insert(k.clone()) {
                    continue;
                }
                match k.as_str() {
                    "hostname" => s.host = v.replace("%h", &alias),
                    "user" => s.username = v.clone(),
                    "port" => s.port = v.parse().unwrap_or(22),
                    "identityfile" => {
                        s.auth = AuthMethod::Key;
                        s.key_path = expand_tilde(v, home);
                    }
                    "forwardx11" if v.eq_ignore_ascii_case("yes") => {
                        s.extra.insert("x11".into(), "1".into());
                    }
                    "compression" if v.eq_ignore_ascii_case("yes") => {
                        s.extra.insert("compression".into(), "1".into());
                    }
                    "proxyjump" if !v.eq_ignore_ascii_case("none") => {
                        let first = v.split(',').next().unwrap_or("").trim_start_matches("ssh://");
                        let (user, rest) = match first.rsplit_once('@') {
                            Some((u, r)) => (u, r),
                            None => ("", first),
                        };
                        let (h, p) = match rest.rsplit_once(':') {
                            Some((h, p)) if p.parse::<u16>().is_ok() => (h, p),
                            _ => (rest, ""),
                        };
                        s.extra.insert("jump_host".into(), h.to_string());
                        if !p.is_empty() {
                            s.extra.insert("jump_port".into(), p.to_string());
                        }
                        if !user.is_empty() {
                            s.extra.insert("jump_user".into(), user.to_string());
                        }
                    }
                    "localforward" => s.forwards.extend(ssh_forward(ForwardKind::Local, v)),
                    "remoteforward" => s.forwards.extend(ssh_forward(ForwardKind::Remote, v)),
                    "dynamicforward" => s.forwards.extend(ssh_forward(ForwardKind::Dynamic, v)),
                    _ => {}
                }
            }
            b.items.push(Item { path: vec![], session: s });
        }
    }
    b
}

// ------------------------------------------------------------------ sources / apply

fn moba_candidates() -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(d) = directories::BaseDirs::new() {
        v.push(d.config_dir().join("MobaXterm").join("MobaXterm.ini")); // %APPDATA%
    }
    v.push(home().join("Documents").join("MobaXterm").join("MobaXterm.ini"));
    v
}

fn ssh_config_path() -> PathBuf {
    home().join(".ssh").join("config")
}

fn read_text(p: &Path) -> AppResult<String> {
    Ok(decode(&std::fs::read(p)?))
}

/// Lists the import sources found on this computer.
pub fn detect() -> Vec<ImportSource> {
    let mut out = vec![];
    let h = home();
    if let Some(p) = moba_candidates().into_iter().find(|p| p.is_file()) {
        let count = read_text(&p).map(|t| parse_mobaxterm(&t, &h).items.len()).unwrap_or(0);
        out.push(ImportSource { id: "mobaxterm".into(), label: "MobaXterm".into(), path: p.to_string_lossy().into(), count });
    }
    let putty = putty_batch(read_putty_raw());
    if !putty.items.is_empty() || !putty.skipped.is_empty() {
        let path = if cfg!(windows) { r"HKCU\Software\SimonTatham\PuTTY\Sessions".to_string() } else { "~/.putty/sessions".into() };
        out.push(ImportSource { id: "putty".into(), label: "PuTTY".into(), path, count: putty.items.len() });
    }
    let sc = ssh_config_path();
    if sc.is_file() {
        let count = read_text(&sc).map(|t| parse_ssh_config(&t, &h).items.len()).unwrap_or(0);
        out.push(ImportSource { id: "ssh_config".into(), label: "OpenSSH (~/.ssh/config)".into(), path: sc.to_string_lossy().into(), count });
    }
    out
}

/// Reads a source: "mobaxterm", "putty", "ssh_config", or "file" (with `path`:
/// a MobaXterm .ini / .moba file or an OpenSSH config file).
pub fn load(source: &str, path: Option<&str>) -> AppResult<(String, Batch)> {
    let h = home();
    match source {
        "mobaxterm" => {
            let p = match path {
                Some(p) => PathBuf::from(p),
                None => moba_candidates()
                    .into_iter()
                    .find(|p| p.is_file())
                    .ok_or_else(|| AppError::NotFound("MobaXterm.ini".into()))?,
            };
            Ok(("MobaXterm".into(), parse_mobaxterm(&read_text(&p)?, &h)))
        }
        "putty" => Ok(("PuTTY".into(), putty_batch(read_putty_raw()))),
        "ssh_config" => {
            let p = path.map(PathBuf::from).unwrap_or_else(ssh_config_path);
            Ok(("OpenSSH".into(), parse_ssh_config(&read_text(&p)?, &h)))
        }
        "file" => {
            let p = PathBuf::from(path.ok_or_else(|| AppError::Config("aucun fichier".into()))?);
            let text = read_text(&p)?;
            if text.contains("[Bookmarks") {
                Ok(("MobaXterm".into(), parse_mobaxterm(&text, &h)))
            } else {
                Ok(("OpenSSH".into(), parse_ssh_config(&text, &h)))
            }
        }
        other => Err(AppError::Config(format!("source d'import inconnue : {other}"))),
    }
}

/// Adds a batch under a root folder named `root`. Sessions already present
/// (same name, folder and host) are skipped, so importing twice is harmless.
pub fn apply(store: &mut Store, root: &str, batch: Batch) -> ImportReport {
    let before = store.folders.len();
    let root_id = store.ensure_folder("", root);
    let folder_of = |store: &mut Store, path: &[String]| {
        let mut id = root_id.clone();
        for name in path {
            id = store.ensure_folder(&id, name);
        }
        id
    };
    for p in &batch.folders {
        folder_of(store, p);
    }
    let mut rep = ImportReport { source: root.to_string(), skipped: batch.skipped, ..Default::default() };
    for mut item in batch.items {
        let folder = folder_of(store, &item.path);
        let dup = store
            .sessions
            .iter()
            .any(|s| s.folder == folder && s.name == item.session.name && s.host == item.session.host);
        if dup {
            rep.duplicates += 1;
            continue;
        }
        item.session.folder = folder;
        store.upsert_session(item.session);
        rep.added += 1;
    }
    rep.folders = store.folders.len() - before;
    rep
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOBA: &str = "[Misc]\r\nFoo=bar\r\n[Bookmarks]\r\nSubRep=\r\nImgNum=42\r\n\
web (root)= #109#0%10.0.0.7%22%root%%-1%0%%%%%0%-1%0%%%-1%-1%0%0%%1080%%0%0%1%%0%%%%0%-1%-1%0%%%0%#MobaFont%10%0%0%-1%15%236,236,236%30,30,30%180,180,192%0%-1%0%%xterm%-1%0%_Std_Colors_0_%80%24%0%1%-1%<none>%%0%0%-1%0%#0# #-1\r\n\
\r\n[Bookmarks_1]\r\nSubRep=Lab\\vps\r\nImgNum=41\r\n\
vps=#109#0%82.70.1.1%2222%ubuntu%%0%-1%uptime%gw.example%2200%jumper%0%-1%0%_ProfileDir_\\.ssh\\k.key%%-1%-1%0%0%%1080%%0%0%1%#MobaFont%12%0#0# #-1\r\n\
files=#140#7%10.0.0.3%22%root%-1%0%%0%0%%0%%1080%%%%0#MobaFont%10%0#0# #-1\r\n\
win=#91#4%10.0.0.13%3389%alice%0%0%0#MobaFont%10%0#0# #-1\r\n\
odd=#999#42%x%1#MobaFont%10#0# #-1\r\n\
[Bookmarks_2]\r\nSubRep=Lab\\empty\r\nImgNum=41\r\n\
[Passwords]\r\nsecret=should_never_be_read\r\n";

    #[test]
    fn mobaxterm_ini() {
        let b = parse_mobaxterm(MOBA, Path::new("C:\\Users\\me"));
        assert_eq!(b.items.len(), 4);
        assert_eq!(b.skipped.len(), 1, "{:?}", b.skipped);
        let web = &b.items[0];
        assert!(web.path.is_empty());
        assert_eq!(web.session.name, "web (root)");
        assert_eq!((web.session.host.as_str(), web.session.port, web.session.username.as_str()), ("10.0.0.7", 22, "root"));
        assert_eq!(web.session.opt("x11"), "1");
        assert_eq!(web.session.terminal.font_size, 14);
        let vps = &b.items[1];
        assert_eq!(vps.path, vec!["Lab", "vps"]);
        assert_eq!(vps.session.port, 2222);
        assert_eq!(vps.session.opt("x11"), "");
        assert_eq!(vps.session.opt("compression"), "1");
        assert_eq!(vps.session.opt("command"), "uptime");
        assert_eq!(vps.session.opt("jump_host"), "gw.example");
        assert_eq!(vps.session.opt("jump_port"), "2200");
        assert_eq!(vps.session.opt("jump_user"), "jumper");
        assert_eq!(vps.session.auth, AuthMethod::Key);
        assert_eq!(vps.session.key_path, "C:\\Users\\me\\.ssh\\k.key");
        assert_eq!(vps.session.terminal.font_size, 16);
        assert_eq!(b.items[2].session.kind, "sftp");
        assert_eq!(b.items[3].session.kind, "rdp");
        assert_eq!(b.items[3].session.port, 3389);
        assert!(b.folders.contains(&vec!["Lab".to_string(), "empty".to_string()]));
        assert!(!b.items.iter().any(|i| i.session.name == "secret"));
    }

    #[test]
    fn apply_builds_tree_and_dedupes() {
        let mut st = Store::default();
        let r1 = apply(&mut st, "MobaXterm", parse_mobaxterm(MOBA, Path::new("/home/me")));
        assert_eq!(r1.added, 4);
        assert_eq!(r1.folders, 4); // MobaXterm, Lab, vps, empty
        let r2 = apply(&mut st, "MobaXterm", parse_mobaxterm(MOBA, Path::new("/home/me")));
        assert_eq!((r2.added, r2.duplicates, r2.folders), (0, 4, 0));
        let lab = st.folders.iter().find(|f| f.name == "Lab").unwrap();
        let vps = st.folders.iter().find(|f| f.name == "vps").unwrap();
        assert_eq!(vps.parent, lab.id);
        assert_eq!(st.sessions.iter().find(|s| s.name == "vps").unwrap().folder, vps.id);
    }

    #[test]
    fn putty() {
        let mut v = HashMap::new();
        for (k, val) in [
            ("HostName", "admin@srv.lan"),
            ("PortNumber", "2022"),
            ("Protocol", "ssh"),
            ("PublicKeyFile", "C:\\keys\\a.ppk"),
            ("X11Forward", "1"),
            ("PortForwardings", "L8080=web:80,4R2222=localhost:22,D1080,L127.0.0.1:3000=app:3000,Lbad"),
        ] {
            v.insert(k.to_string(), val.to_string());
        }
        let s = putty_session("My%20Server", &v).unwrap();
        assert_eq!(s.name, "My Server");
        assert_eq!((s.host.as_str(), s.port, s.username.as_str()), ("srv.lan", 2022, "admin"));
        assert_eq!(s.auth, AuthMethod::Key);
        assert_eq!(s.opt("x11"), "1");
        assert_eq!(s.forwards.len(), 4);
        assert_eq!(s.forwards[1].kind, ForwardKind::Remote);
        assert_eq!(s.forwards[2].kind, ForwardKind::Dynamic);
        assert_eq!(s.forwards[3].bind_host, "127.0.0.1");

        let mut ser = HashMap::new();
        ser.insert("Protocol".to_string(), "serial".to_string());
        ser.insert("SerialLine".to_string(), "COM3".to_string());
        ser.insert("SerialSpeed".to_string(), "115200".to_string());
        let s = putty_session("console", &ser).unwrap();
        assert_eq!((s.kind.as_str(), s.opt("serial_port"), s.opt("baud")), ("serial", "COM3", "115200"));

        let mut raw = HashMap::new();
        raw.insert("Protocol".to_string(), "raw".to_string());
        assert!(putty_session("r", &raw).is_err());
    }

    #[test]
    fn ssh_config() {
        let text = "Host *\n  User nobody\n\nHost web web2\n  HostName %h.example.com\n  User deploy\n  Port 2200\n  IdentityFile ~/.ssh/id_ed25519\n  ForwardX11 yes\n  LocalForward 8080 localhost:80\n  DynamicForward 1080\n  ProxyJump admin@bastion:2222\n\nMatch host x\n  User ignored\n\nHost=db\n  HostName=10.0.0.9\n";
        let b = parse_ssh_config(text, Path::new("/home/me"));
        let names: Vec<_> = b.items.iter().map(|i| i.session.name.as_str()).collect();
        assert_eq!(names, vec!["web", "web2", "db"]);
        let w = &b.items[1].session;
        assert_eq!((w.host.as_str(), w.port, w.username.as_str()), ("web2.example.com", 2200, "deploy"));
        assert_eq!(w.key_path.replace('\\', "/"), "/home/me/.ssh/id_ed25519");
        assert_eq!(w.opt("jump_host"), "bastion");
        assert_eq!(w.opt("jump_port"), "2222");
        assert_eq!(w.opt("jump_user"), "admin");
        assert_eq!(w.forwards.len(), 2);
        assert_eq!(b.items[2].session.host, "10.0.0.9");
    }
}
