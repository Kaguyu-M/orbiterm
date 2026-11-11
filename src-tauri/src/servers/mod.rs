//! Built-in servers (HTTP, TFTP, FTP, SSH/SFTP, Telnet, Iperf, VNC): configuration, start/stop and
//! activity log. Each server shares one directory of this computer.
//!
//! A server runs in a tokio task; stopping it drops the listener and every client
//! connection task (they live in a `JoinSet` owned by the server task).

mod ftpd;
mod httpd;
mod iperfd;
mod screen;
mod sshd;
mod telnetd;
mod tftpd;
mod vncd;

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinHandle;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerKind {
    Http,
    Tftp,
    Ftp,
    Ssh,
    Telnet,
    Iperf,
    Vnc,
}

pub const ALL: [ServerKind; 7] = [
    ServerKind::Http,
    ServerKind::Tftp,
    ServerKind::Ftp,
    ServerKind::Ssh,
    ServerKind::Telnet,
    ServerKind::Iperf,
    ServerKind::Vnc,
];

impl ServerKind {
    /// Usual port (unprivileged alternative on Unix, where ports < 1024 need root).
    pub fn default_port(self) -> u16 {
        let (std, alt) = match self {
            ServerKind::Http => (80, 8080),
            ServerKind::Tftp => (69, 6969),
            ServerKind::Ftp => (21, 2121),
            ServerKind::Ssh => (22, 2222),
            ServerKind::Telnet => (23, 2323),
            ServerKind::Iperf => (5201, 5201),
            ServerKind::Vnc => (5900, 5900),
        };
        if cfg!(windows) { std } else { alt }
    }

    pub fn label(self) -> &'static str {
        match self {
            ServerKind::Http => "HTTP",
            ServerKind::Tftp => "TFTP",
            ServerKind::Ftp => "FTP",
            ServerKind::Ssh => "SSH/SFTP",
            ServerKind::Telnet => "Telnet",
            ServerKind::Iperf => "Iperf",
            ServerKind::Vnc => "VNC",
        }
    }

    /// Keyring entry of the server password (FTP, SSH, Telnet and VNC).
    pub fn secret_key(self) -> String {
        format!("server:{}", serde_json::to_value(self).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub kind: ServerKind,
    pub port: u16,
    /// shared directory ("" = default folder)
    pub root: String,
    /// listen on 127.0.0.1 only (otherwise every network interface)
    pub local_only: bool,
    /// clients may upload / modify files (TFTP, FTP, SFTP); VNC: keyboard and mouse allowed
    pub writable: bool,
    /// FTP: accept "anonymous" without password
    pub anonymous: bool,
    /// FTP / SSH / Telnet login
    pub user: String,
    /// HTTP: list folders without index.html
    pub listing: bool,
    /// SSH / Telnet: id of the local shell started for clients ("" = default)
    pub shell: String,
    pub autostart: bool,
    /// SSH: public keys allowed to log in as `user` (authorized_keys lines)
    pub authorized_keys: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self::new(ServerKind::Http)
    }
}

impl ServerConfig {
    pub fn new(kind: ServerKind) -> Self {
        Self {
            kind,
            port: kind.default_port(),
            root: String::new(),
            local_only: false,
            writable: !matches!(kind, ServerKind::Http | ServerKind::Iperf | ServerKind::Vnc),
            anonymous: false,
            user: whoami(),
            listing: true,
            shell: String::new(),
            autostart: false,
            authorized_keys: String::new(),
        }
    }

    /// Valid keys of `authorized_keys` (blank lines, comments and unreadable lines skipped;
    /// OpenSSH options before the key type are ignored).
    pub fn authorized_keys(&self) -> Vec<russh::keys::PublicKey> {
        self.authorized_keys
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|l| {
                let start = l.find("ssh-").or_else(|| l.find("ecdsa-")).or_else(|| l.find("sk-"))?;
                russh::keys::PublicKey::from_openssh(&l[start..]).ok()
            })
            .collect()
    }

    pub fn root_dir(&self) -> PathBuf {
        if self.root.trim().is_empty() { default_root() } else { PathBuf::from(self.root.trim()) }
    }

    fn bind_ip(&self) -> IpAddr {
        if self.local_only { IpAddr::from([127, 0, 0, 1]) } else { IpAddr::from([0, 0, 0, 0]) }
    }
}

pub fn whoami() -> String {
    std::env::var(if cfg!(windows) { "USERNAME" } else { "USER" }).unwrap_or_else(|_| "orbiterm".into())
}

/// Default shared folder: Documents/Orbiterm (created on first start).
pub fn default_root() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|u| u.document_dir().map(|d| d.join("Orbiterm")))
        .unwrap_or_else(|| std::env::temp_dir().join("orbiterm-partage"))
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ServersFile {
    #[serde(default)]
    servers: Vec<ServerConfig>,
}

const FILE: &str = "servers.toml";

/// Loads the server configurations (missing ones get their defaults).
pub fn load(dir: &Path) -> Vec<ServerConfig> {
    let saved: ServersFile =
        std::fs::read_to_string(dir.join(FILE)).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default();
    ALL.iter()
        .map(|k| saved.servers.iter().find(|c| c.kind == *k).cloned().unwrap_or_else(|| ServerConfig::new(*k)))
        .collect()
}

pub fn save(dir: &Path, cfg: &ServerConfig) -> AppResult<()> {
    let mut all = load(dir);
    if let Some(c) = all.iter_mut().find(|c| c.kind == cfg.kind) {
        *c = cfg.clone();
    }
    let text = toml::to_string_pretty(&ServersFile { servers: all }).map_err(|e| AppError::Config(e.to_string()))?;
    std::fs::write(dir.join(FILE), text)?;
    Ok(())
}

// ------------------------------------------------------------------ activity log

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    /// milliseconds since the Unix epoch (formatted by the UI in local time)
    pub t: u64,
    pub msg: String,
}

#[derive(Default)]
struct Activity {
    log: VecDeque<LogLine>,
    clients: usize,
    total: u64,
}

/// Shared activity record of one server, handed to the server implementation.
#[derive(Clone)]
pub struct Logger(Arc<StdMutex<Activity>>);

impl Logger {
    fn new() -> Self {
        Self(Arc::new(StdMutex::new(Activity::default())))
    }

    pub fn log(&self, msg: impl Into<String>) {
        let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        let mut a = self.0.lock().unwrap();
        a.log.push_back(LogLine { t, msg: msg.into() });
        while a.log.len() > 300 {
            a.log.pop_front();
        }
    }

    /// Counts a connected client until the guard is dropped.
    pub fn client(&self) -> ClientGuard {
        let mut a = self.0.lock().unwrap();
        a.clients += 1;
        a.total += 1;
        ClientGuard(self.clone())
    }
}

pub struct ClientGuard(Logger);

impl Drop for ClientGuard {
    fn drop(&mut self) {
        let mut a = self.0.0.lock().unwrap();
        a.clients = a.clients.saturating_sub(1);
    }
}

/// What a server implementation receives when it starts.
pub struct Ctx {
    pub cfg: ServerConfig,
    pub root: PathBuf,
    pub password: String,
    pub log: Logger,
    pub config_dir: PathBuf,
}

// ------------------------------------------------------------------ manager

struct Running {
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
    port: u16,
    started: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerStatus {
    pub config: ServerConfig,
    pub running: bool,
    pub port: u16,
    pub started: u64,
    pub clients: usize,
    pub total: u64,
    pub has_password: bool,
    /// addresses clients can use (local IP addresses of this computer)
    pub addresses: Vec<String>,
    pub root: String,
    pub log: Vec<LogLine>,
}

pub struct ServerManager {
    dir: PathBuf,
    running: Mutex<HashMap<ServerKind, Running>>,
    activity: StdMutex<HashMap<ServerKind, Logger>>,
}

impl ServerManager {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, running: Mutex::new(HashMap::new()), activity: StdMutex::new(HashMap::new()) }
    }

    fn logger(&self, kind: ServerKind) -> Logger {
        self.activity.lock().unwrap().entry(kind).or_insert_with(Logger::new).clone()
    }

    pub async fn status(&self) -> Vec<ServerStatus> {
        let mut running = self.running.lock().await;
        // a server task that ended by itself (fatal error) is no longer running
        running.retain(|_, r| !r.task.is_finished());
        let addrs = local_addresses();
        load(&self.dir)
            .into_iter()
            .map(|config| {
                let kind = config.kind;
                let r = running.get(&kind);
                let a = self.logger(kind);
                let act = a.0.lock().unwrap();
                ServerStatus {
                    running: r.is_some(),
                    port: r.map(|r| r.port).unwrap_or(config.port),
                    started: r.map(|r| r.started).unwrap_or(0),
                    clients: act.clients,
                    total: act.total,
                    has_password: matches!(crate::secrets::get_password(&kind.secret_key()), Ok(Some(_))),
                    addresses: if config.local_only { vec!["127.0.0.1".into()] } else { addrs.clone() },
                    root: config.root_dir().to_string_lossy().into_owned(),
                    log: act.log.iter().cloned().collect(),
                    config,
                }
            })
            .collect()
    }

    /// Starts a server with its saved configuration and keyring password.
    pub async fn start(&self, kind: ServerKind) -> AppResult<u16> {
        let cfg = load(&self.dir).into_iter().find(|c| c.kind == kind).unwrap_or_else(|| ServerConfig::new(kind));
        let password = crate::secrets::get_password(&kind.secret_key())?.unwrap_or_default();
        self.start_with(cfg, password).await
    }

    /// Starts a server with an explicit configuration and password (port 0 = any free port).
    pub async fn start_with(&self, cfg: ServerConfig, password: String) -> AppResult<u16> {
        let kind = cfg.kind;
        let mut running = self.running.lock().await;
        if let Some(r) = running.get(&kind)
            && !r.task.is_finished()
        {
            return Ok(r.port);
        }
        let root = cfg.root_dir();
        std::fs::create_dir_all(&root)
            .map_err(|e| AppError::Config(format!("dossier partagé {} : {e}", root.display())))?;
        if kind == ServerKind::Ssh && password.is_empty() && cfg.authorized_keys().is_empty() {
            return Err(AppError::Config("définis d'abord le mot de passe du serveur SSH ou une clé publique autorisée".into()));
        }
        if matches!(kind, ServerKind::Telnet | ServerKind::Vnc) && password.is_empty() {
            return Err(AppError::Config(format!("définis d'abord le mot de passe du serveur {}", kind.label())));
        }
        if matches!(kind, ServerKind::Ftp) && password.is_empty() && !cfg.anonymous {
            return Err(AppError::Config("définis un mot de passe FTP ou autorise l'accès anonyme".into()));
        }
        let log = self.logger(kind);
        let addr = SocketAddr::new(cfg.bind_ip(), cfg.port);
        let ctx = Ctx { cfg, root, password, log: log.clone(), config_dir: self.dir.clone() };
        let (stop_tx, stop_rx) = oneshot::channel();
        let (task, port) = match kind {
            ServerKind::Http => httpd::start(addr, ctx, stop_rx).await,
            ServerKind::Tftp => tftpd::start(addr, ctx, stop_rx).await,
            ServerKind::Ftp => ftpd::start(addr, ctx, stop_rx).await,
            ServerKind::Ssh => sshd::start(addr, ctx, stop_rx).await,
            ServerKind::Telnet => telnetd::start(addr, ctx, stop_rx).await,
            ServerKind::Iperf => iperfd::start(addr, ctx, stop_rx).await,
            ServerKind::Vnc => vncd::start(addr, ctx, stop_rx).await,
        }
        .map_err(|e| bind_error(kind, addr, e))?;
        log.log(format!("Serveur {} démarré sur le port {port}", kind.label()));
        let started = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        running.insert(kind, Running { stop: Some(stop_tx), task, port, started });
        Ok(port)
    }

    pub async fn stop(&self, kind: ServerKind) {
        let r = self.running.lock().await.remove(&kind);
        if let Some(mut r) = r {
            if let Some(s) = r.stop.take() {
                let _ = s.send(());
            }
            // give a graceful shutdown a moment, then force it
            if tokio::time::timeout(std::time::Duration::from_millis(1500), &mut r.task).await.is_err() {
                r.task.abort();
            }
            self.logger(kind).log(format!("Serveur {} arrêté", kind.label()));
        }
    }

    pub async fn stop_all(&self) {
        for k in ALL {
            self.stop(k).await;
        }
    }

    pub fn clear_log(&self, kind: ServerKind) {
        let l = self.logger(kind);
        l.0.lock().unwrap().log.clear();
    }

    /// Starts the servers marked "start with Orbiterm"; errors are logged.
    pub async fn autostart(&self) {
        for c in load(&self.dir).into_iter().filter(|c| c.autostart) {
            if let Err(e) = self.start(c.kind).await {
                self.logger(c.kind).log(format!("Démarrage automatique impossible : {e}"));
            }
        }
    }
}

fn bind_error(kind: ServerKind, addr: SocketAddr, e: std::io::Error) -> AppError {
    match e.kind() {
        std::io::ErrorKind::AddrInUse => AppError::Conn(format!(
            "le port {} est déjà utilisé par un autre programme (choisis un autre port pour le serveur {})",
            addr.port(),
            kind.label()
        )),
        std::io::ErrorKind::PermissionDenied => AppError::Conn(format!(
            "accès refusé au port {} (les ports inférieurs à 1024 demandent des droits administrateur sous Linux)",
            addr.port()
        )),
        _ => AppError::Conn(format!("serveur {} : {e}", kind.label())),
    }
}

/// IPv4 addresses of this computer that other machines can use (best effort: the
/// address of the default route, plus loopback).
pub fn local_addresses() -> Vec<String> {
    let mut out = vec![];
    if let Ok(s) = std::net::UdpSocket::bind("0.0.0.0:0")
        && s.connect("192.0.2.1:9").is_ok()
        && let Ok(a) = s.local_addr()
        && !a.ip().is_unspecified()
    {
        out.push(a.ip().to_string());
    }
    out.push("127.0.0.1".into());
    out
}
