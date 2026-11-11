//! Tauri commands = the API the UI calls with `invoke(...)`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, oneshot};

use crate::config::{AuthMethod, Folder, Forward, ForwardKind, Macro, Session, Store, Tunnel};
use crate::bundle;
use crate::editor;
use crate::importer;
use crate::error::{AppError, AppResult};
use crate::secrets;
use crate::scp;
use crate::sftp::{self, Entry};
use crate::pty::{self, LocalShell, PtyConnection, PtySpawn};
use crate::serial::SerialConnection;
use crate::forward;
use crate::ssh::{
    ActiveForward, CloseSink, ConnectParams, Credentials, DataSink, HostKeyPrompter, HostKeyQuestion, JumpParams, ShellMode,
    SshConnection,
};
use crate::x11;
use crate::ftp::{FtpConnection, FtpParams, FtpTls};
use crate::rdp::{self, ExternalProcess, RdpOptions};
use crate::rdp_client::{self, RdpParams};
use crate::telnet::{Mode, TcpTermConnection};
use crate::terminal::Conn;
use crate::vnc::VncBridge;

pub struct AppState {
    pub config_dir: PathBuf,
    pub store: Mutex<Store>,
    pub conns: Mutex<HashMap<String, Conn>>,
    pub hostkey_waiters: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
    /// running standalone tunnels (tunnel manager), by tunnel id
    pub tunnels: Mutex<HashMap<String, Arc<SshConnection>>>,
    /// SSH gateways of RDP / VNC tabs, by connection id (closed with the tab)
    pub gateways: Mutex<HashMap<String, Arc<SshConnection>>>,
    /// built-in HTTP / TFTP / FTP / SSH servers
    pub servers: crate::servers::ServerManager,
    /// system information of RDP tabs (SSH to the same host), by connection id
    pub monitors: Mutex<HashMap<String, MonitorSlot>>,
    /// session logs, by connection id (lines rendered by the terminal tab)
    pub logs: std::sync::Mutex<HashMap<String, Arc<crate::termlog::TermLog>>>,
}

/// SSH access used to read the system information of an RDP host.
pub enum MonitorSlot {
    /// not tried yet: host and credentials of the RDP session
    Pending { host: String, username: String, password: String },
    Ready(Arc<SshConnection>),
    Unavailable(String),
}

/// Remote file access of a tab: SFTP (SSH sessions), shell + SCP (SSH without SFTP) or FTP.
enum Files {
    Sftp(Arc<russh_sftp::client::SftpSession>),
    Scp(Arc<SshConnection>),
    Ftp(Arc<FtpConnection>),
}

impl AppState {
    pub fn new(config_dir: PathBuf) -> AppResult<Self> {
        let store = Store::load(&config_dir)?;
        Ok(Self {
            servers: crate::servers::ServerManager::new(config_dir.clone()),
            config_dir,
            store: Mutex::new(store),
            conns: Mutex::new(HashMap::new()),
            hostkey_waiters: Arc::new(Mutex::new(HashMap::new())),
            tunnels: Mutex::new(HashMap::new()),
            gateways: Mutex::new(HashMap::new()),
            monitors: Mutex::new(HashMap::new()),
            logs: std::sync::Mutex::new(HashMap::new()),
        })
    }

    async fn files(&self, id: &str) -> AppResult<Files> {
        match self.conn(id).await? {
            Conn::Ssh(c) => Ok(match c.sftp_opt().await? {
                Some(s) => Files::Sftp(s),
                None => Files::Scp(c),
            }),
            Conn::Ftp(f) => Ok(Files::Ftp(f)),
            _ => Err(AppError::Unsupported("pas d'accès aux fichiers pour cette session".into())),
        }
    }

    async fn conn(&self, id: &str) -> AppResult<Conn> {
        self.conns.lock().await.get(id).cloned().ok_or_else(|| AppError::NotFound(id.into()))
    }

    async fn ssh(&self, id: &str) -> AppResult<Arc<SshConnection>> {
        self.conn(id).await?.ssh()
    }
}

// ---------------------------------------------------------------- sessions

#[tauri::command]
pub async fn sessions_load(state: State<'_, AppState>) -> AppResult<Store> {
    Ok(state.store.lock().await.clone())
}

#[tauri::command]
pub async fn session_save(state: State<'_, AppState>, session: Session) -> AppResult<Session> {
    let mut store = state.store.lock().await;
    let saved = store.upsert_session(session);
    if !saved.remember_password {
        let _ = secrets::delete_password(&saved.id);
    }
    store.save(&state.config_dir)?;
    Ok(saved)
}

#[tauri::command]
pub async fn session_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let mut store = state.store.lock().await;
    store.delete_session(&id);
    store.save(&state.config_dir)?;
    let _ = secrets::delete_password(&id);
    let _ = secrets::delete_password(&jump_account(&id));
    let _ = secrets::delete_password(&vnc_account(&id));
    Ok(())
}

#[tauri::command]
pub async fn folder_save(state: State<'_, AppState>, folder: Folder) -> AppResult<Folder> {
    let mut store = state.store.lock().await;
    let saved = store.upsert_folder(folder)?;
    store.save(&state.config_dir)?;
    Ok(saved)
}

#[tauri::command]
pub async fn folder_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let mut store = state.store.lock().await;
    for sid in store.delete_folder(&id) {
        let _ = secrets::delete_password(&sid);
    }
    store.save(&state.config_dir)?;
    Ok(())
}

#[tauri::command]
pub async fn password_forget(id: String) -> AppResult<()> {
    secrets::delete_password(&id)
}

/// Copies the remembered passwords (session + jump host) of a saved session
/// to another one, inside the keyring (the secrets never reach the UI).
/// Returns true when a session password was copied.
#[tauri::command]
pub async fn password_copy(state: State<'_, AppState>, from: String, to: String) -> AppResult<bool> {
    {
        let store = state.store.lock().await;
        let (src, dst) = (store.session(&from)?, store.session(&to)?);
        if !src.remember_password || !dst.remember_password {
            return Ok(false);
        }
    }
    let mut copied = false;
    if let Some(pw) = secrets::get_password(&from)? {
        secrets::set_password(&to, &pw)?;
        copied = true;
    }
    if let Some(pw) = secrets::get_password(&jump_account(&from))? {
        secrets::set_password(&jump_account(&to), &pw)?;
    }
    Ok(copied)
}

// ---------------------------------------------------------------- import / macros

#[tauri::command]
pub async fn import_sources() -> AppResult<Vec<importer::ImportSource>> {
    tokio::task::spawn_blocking(importer::detect).await.map_err(|e| AppError::Conn(e.to_string()))
}

#[tauri::command]
pub async fn import_run(
    state: State<'_, AppState>,
    source: String,
    path: Option<String>,
) -> AppResult<importer::ImportReport> {
    let (root, batch) =
        tokio::task::spawn_blocking(move || importer::load(&source, path.as_deref()))
            .await
            .map_err(|e| AppError::Conn(e.to_string()))??;
    let mut store = state.store.lock().await;
    let report = importer::apply(&mut store, &format!("Import {root}"), batch);
    store.save(&state.config_dir)?;
    Ok(report)
}

#[tauri::command]
pub async fn macro_save(state: State<'_, AppState>, m: Macro) -> AppResult<Macro> {
    let mut store = state.store.lock().await;
    let saved = store.upsert_macro(m);
    store.save(&state.config_dir)?;
    Ok(saved)
}

#[tauri::command]
pub async fn macro_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let mut store = state.store.lock().await;
    store.delete_macro(&id);
    store.save(&state.config_dir)?;
    Ok(())
}

#[tauri::command]
pub async fn config_path(state: State<'_, AppState>) -> AppResult<String> {
    Ok(state.config_dir.to_string_lossy().into_owned())
}

// ---------------------------------------------------------------- ssh

#[derive(Clone, Serialize)]
struct TermData {
    conn_id: String,
    data: String, // base64
}

#[derive(Clone, Serialize)]
struct TermClosed {
    conn_id: String,
    reason: String,
}

#[derive(Clone, Serialize)]
struct HostKeyPrompt {
    request_id: String,
    #[serde(flatten)]
    question: HostKeyQuestion,
}

#[derive(Deserialize)]
pub struct OpenArgs {
    /// The session to open. Saved sessions are re-read from the store when `id` is set;
    /// quick-connect / local terminals pass an unsaved session (empty id).
    pub session: Session,
    pub cols: u32,
    pub rows: u32,
    /// Password typed by the user (when not stored, or stored one was refused).
    pub password: Option<String>,
    pub passphrase: Option<String>,
    /// Store `password` in the keyring after a successful login.
    #[serde(default)]
    pub remember: bool,
    pub jump_password: Option<String>,
}

fn make_prompter(app: AppHandle, waiters: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>) -> HostKeyPrompter {
    Arc::new(move |question: HostKeyQuestion| {
        let app = app.clone();
        let waiters = waiters.clone();
        Box::pin(async move {
            let request_id = crate::config::new_id("hk");
            let (tx, rx) = oneshot::channel();
            waiters.lock().await.insert(request_id.clone(), tx);
            if app.emit("hostkey-prompt", HostKeyPrompt { request_id: request_id.clone(), question }).is_err() {
                waiters.lock().await.remove(&request_id);
                return false;
            }
            let answer = tokio::time::timeout(Duration::from_secs(280), rx).await;
            waiters.lock().await.remove(&request_id);
            matches!(answer, Ok(Ok(true)))
        })
    })
}

fn sinks(app: &AppHandle, conn_id: &str, log: Option<Arc<crate::termlog::TermLog>>) -> (DataSink, CloseSink) {
    let (app_d, id_d) = (app.clone(), conn_id.to_string());
    let on_data: DataSink = Arc::new(move |bytes: Vec<u8>| {
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        let _ = app_d.emit("term-data", TermData { conn_id: id_d.clone(), data });
    });
    let (app_c, id_c) = (app.clone(), conn_id.to_string());
    let on_close: CloseSink = Arc::new(move |reason: String| {
        if let Some(l) = log.clone() {
            // the tab still sends its last lines: the file is closed a moment later
            let (app_l, id_l, why) = (app_c.clone(), id_c.clone(), reason.clone());
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(1500));
                l.close(&why);
                if let Ok(mut logs) = app_l.state::<AppState>().logs.lock() {
                    logs.remove(&id_l);
                }
            });
        }
        let _ = app_c.emit("term-closed", TermClosed { conn_id: id_c.clone(), reason });
    });
    (on_data, on_close)
}

fn clamp16(v: u32) -> u16 {
    v.clamp(1, u16::MAX as u32) as u16
}

/// Builds the local program to run for `shell`, `wsl` and `mosh` sessions.
fn pty_spec(session: &Session, cols: u32, rows: u32) -> AppResult<PtySpawn> {
    let (program, args) = match session.kind.as_str() {
        "shell" => {
            let custom = session.opt("program").trim();
            if !custom.is_empty() {
                (custom.to_string(), session.opt("args").split_whitespace().map(String::from).collect())
            } else {
                let shells = pty::local_shells();
                let wanted = session.opt("shell");
                let sh = shells
                    .iter()
                    .find(|s| s.id == wanted)
                    .or_else(|| shells.first())
                    .ok_or_else(|| AppError::Unsupported("aucun shell local trouvé".into()))?;
                (sh.program.clone(), sh.args.clone())
            }
        }
        "wsl" => {
            let wsl = which::which("wsl.exe")
                .map_err(|_| AppError::Unsupported("WSL n'est pas installé sur ce poste".into()))?;
            let mut args = vec![];
            let distro = session.opt("distro");
            if !distro.is_empty() {
                args.extend(["-d".to_string(), distro.to_string()]);
            }
            args.extend(["--cd".to_string(), "~".to_string()]);
            (wsl.to_string_lossy().into_owned(), args)
        }
        "mosh" => {
            let mosh = which::which("mosh").map_err(|_| {
                AppError::Unsupported("le client « mosh » est introuvable dans le PATH".into())
            })?;
            let target = if session.username.is_empty() {
                session.host.clone()
            } else {
                format!("{}@{}", session.username, session.host)
            };
            let mut args = vec![];
            if session.port != 22 && session.port != 0 {
                args.push(format!("--ssh=ssh -p {}", session.port));
            }
            args.push(target);
            (mosh.to_string_lossy().into_owned(), args)
        }
        k => return Err(AppError::Unsupported(format!("type de session « {k} »"))),
    };
    let cwd = session.opt("cwd");
    Ok(PtySpawn {
        program,
        args,
        cwd: if cwd.is_empty() { None } else { Some(PathBuf::from(cwd)) },
        env: vec![],
        cols: clamp16(cols),
        rows: clamp16(rows),
    })
}

/// Secrets typed by the user for an SSH connection.
pub struct SshSecrets {
    pub password: Option<String>,
    pub passphrase: Option<String>,
    pub jump_password: Option<String>,
    pub remember: bool,
}

fn jump_account(session_id: &str) -> String {
    format!("{session_id}#jump")
}

pub fn forward_label(f: &Forward, port: u16) -> String {
    let bind = forward::bind_addr(&f.bind_host);
    match f.kind {
        ForwardKind::Local => format!("local {bind}:{port} → {}:{}", f.dest_host, f.dest_port),
        ForwardKind::Remote => format!(
            "distant {}:{port} → {}:{}",
            if f.bind_host.is_empty() { "serveur" } else { &f.bind_host },
            f.dest_host,
            f.dest_port
        ),
        ForwardKind::Dynamic => format!("SOCKS {bind}:{port}"),
    }
}

/// Opens an SSH connection for `session` (credentials from the keyring or typed),
/// through its jump host and with X11 when configured.
#[allow(clippy::too_many_arguments)]
async fn open_ssh(
    app: &AppHandle,
    state: &AppState,
    session: &Session,
    secrets_in: &SshSecrets,
    mode: ShellMode,
    cols: u32,
    rows: u32,
    on_data: DataSink,
    on_close: CloseSink,
) -> AppResult<Arc<SshConnection>> {
    let saved = !session.id.is_empty();
    let credentials = match session.auth {
        AuthMethod::Password => {
            let pw = match secrets_in.password.clone() {
                Some(p) => p,
                None if saved && session.remember_password => {
                    secrets::saved_password(&session.id).ok_or(AppError::NeedPassword)?
                }
                None => return Err(AppError::NeedPassword),
            };
            Credentials::Password(pw)
        }
        AuthMethod::Key => {
            if session.key_path.trim().is_empty() {
                return Err(AppError::Config("aucune clé privée sélectionnée".into()));
            }
            Credentials::Key { path: PathBuf::from(&session.key_path), passphrase: secrets_in.passphrase.clone() }
        }
    };

    // jump host (gateway): key file, or password (typed / keyring)
    let jump_host = session.opt("jump_host").trim().to_string();
    let jump = if jump_host.is_empty() {
        None
    } else {
        let key = session.opt("jump_key").trim();
        let credentials = if !key.is_empty() {
            Credentials::Key { path: PathBuf::from(key), passphrase: None }
        } else {
            let pw = match secrets_in.jump_password.clone() {
                Some(p) => p,
                None if saved && session.remember_password => {
                    secrets::saved_password(&jump_account(&session.id)).ok_or(AppError::NeedJumpPassword)?
                }
                None => return Err(AppError::NeedJumpPassword),
            };
            Credentials::Password(pw)
        };
        let user = session.opt("jump_user").trim();
        Some(JumpParams {
            host: jump_host,
            port: session.opt("jump_port").trim().parse().unwrap_or(22),
            username: if user.is_empty() { session.username.clone() } else { user.to_string() },
            credentials,
        })
    };

    let x11_display = if session.opt_bool("x11") { Some(x11::local_display(session.opt("x11_display"))) } else { None };
    let params = ConnectParams {
        host: session.host.clone(),
        port: session.port,
        username: session.username.clone(),
        credentials,
        cols,
        rows,
        known_hosts: state.config_dir.join("known_hosts"),
        jump,
        mode,
        x11_display,
        compression: session.opt_bool("compression"),
        // global setting injected by the UI ("0" disables it)
        keepalive: session.opt("keepalive").trim().parse().unwrap_or(30),
    };
    let prompter = make_prompter(app.clone(), state.hostkey_waiters.clone());
    match SshConnection::connect(params, prompter, on_data, on_close).await {
        Ok(c) => {
            let store = saved && session.remember_password && secrets_in.remember;
            if store {
                if let Some(pw) = &secrets_in.password
                    && let Err(e) = secrets::set_password(&session.id, pw)
                {
                    log::warn!("keyring: {e}");
                }
                if let Some(pw) = &secrets_in.jump_password
                    && let Err(e) = secrets::set_password(&jump_account(&session.id), pw)
                {
                    log::warn!("keyring: {e}");
                }
            }
            Ok(c)
        }
        // a stored password that no longer works must be asked again
        Err(AppError::AuthFailed) if secrets_in.password.is_none() && session.auth == AuthMethod::Password => {
            Err(AppError::NeedPassword)
        }
        Err(AppError::JumpAuthFailed) if secrets_in.jump_password.is_none() && session.opt("jump_key").is_empty() => {
            Err(AppError::NeedJumpPassword)
        }
        Err(e) => Err(e),
    }
}

/// Opens a terminal tab for any session kind. Returns the connection id.
/// For RDP / VNC sessions with an SSH gateway (`jump_host`): connects to the gateway
/// and opens a local tunnel to the target. Returns the address to connect to.
async fn via_gateway(
    app: &AppHandle,
    state: &AppState,
    session: &Session,
    secrets_in: &SshSecrets,
) -> AppResult<(String, u16, Option<Arc<SshConnection>>)> {
    let gw = session.opt("jump_host").trim();
    if gw.is_empty() {
        return Ok((session.host.clone(), session.port, None));
    }
    let key = session.opt("jump_key").trim();
    let user = session.opt("jump_user").trim();
    let gateway = Session {
        // the gateway password lives under the "{id}#jump" keyring account
        id: if session.id.is_empty() { String::new() } else { jump_account(&session.id) },
        name: format!("{} (passerelle)", session.name),
        folder: String::new(),
        kind: "ssh".into(),
        host: gw.to_string(),
        port: session.opt("jump_port").trim().parse().unwrap_or(22),
        username: if user.is_empty() { session.username.clone() } else { user.to_string() },
        auth: if key.is_empty() { AuthMethod::Password } else { AuthMethod::Key },
        key_path: key.to_string(),
        remember_password: session.remember_password,
        sftp_enabled: false,
        terminal: Default::default(),
        extra: Default::default(),
        forwards: vec![],
    };
    let creds = SshSecrets {
        password: secrets_in.jump_password.clone(),
        passphrase: None,
        jump_password: None,
        remember: secrets_in.remember,
    };
    let quiet_data: DataSink = Arc::new(|_| {});
    let quiet_close: CloseSink = Arc::new(|_| {});
    let c = match open_ssh(app, state, &gateway, &creds, ShellMode::None, 80, 24, quiet_data, quiet_close).await {
        Ok(c) => c,
        Err(AppError::NeedPassword) => return Err(AppError::NeedJumpPassword),
        Err(AppError::AuthFailed) => return Err(AppError::JumpAuthFailed),
        Err(e) => return Err(e),
    };
    let tunnel = Forward {
        kind: ForwardKind::Local,
        bind_host: "127.0.0.1".into(),
        bind_port: 0,
        dest_host: session.host.clone(),
        dest_port: session.port,
    };
    match c.start_forward(&tunnel).await {
        Ok(port) => Ok(("127.0.0.1".into(), port, Some(c))),
        Err(e) => {
            c.disconnect().await;
            Err(e)
        }
    }
}

/// Password of a non-SSH session (FTP): typed, or from the keyring.
fn session_password(session: &Session, typed: &Option<String>) -> AppResult<String> {
    match typed {
        Some(p) => Ok(p.clone()),
        None if !session.id.is_empty() && session.remember_password => {
            secrets::saved_password(&session.id).ok_or(AppError::NeedPassword)
        }
        None => Err(AppError::NeedPassword),
    }
}

#[tauri::command]
pub async fn term_open(app: AppHandle, state: State<'_, AppState>, args: OpenArgs) -> AppResult<String> {
    // a saved session is authoritative (the UI may hold a stale copy)
    let mut session = if args.session.id.is_empty() {
        args.session.clone()
    } else {
        state.store.lock().await.session(&args.session.id).cloned().unwrap_or(args.session.clone())
    };
    // global settings injected by the UI
    if let Some(k) = args.session.extra.get("keepalive") {
        session.extra.insert("keepalive".into(), k.clone());
    }
    let conn_id = crate::config::new_id("c");
    // session log (global setting injected by the UI): terminals only
    let log = match args.session.extra.get("log_dir") {
        Some(dir) if !matches!(session.kind.as_str(), "rdp" | "vnc" | "ftp" | "sftp" | "browser") => {
            let dir = if dir.trim().is_empty() { crate::termlog::default_dir() } else { PathBuf::from(dir.trim()) };
            let name = if session.name.trim().is_empty() { session.host.clone() } else { session.name.clone() };
            match crate::termlog::TermLog::create(&dir, &name) {
                Ok(l) => Some(Arc::new(l)),
                Err(e) => {
                    log::warn!("journal de session impossible dans {} : {e}", dir.display());
                    None
                }
            }
        }
        _ => None,
    };
    if let (Some(l), Ok(mut logs)) = (&log, state.logs.lock()) {
        logs.insert(conn_id.clone(), l.clone());
    }
    let (on_data, on_close) = sinks(&app, &conn_id, log);

    let conn = match session.kind.as_str() {
        "ssh" => {
            let mode = match session.opt("command").trim() {
                "" => ShellMode::Shell,
                cmd => ShellMode::Command(cmd.to_string()),
            };
            let creds = SshSecrets {
                password: args.password.clone(),
                passphrase: args.passphrase.clone(),
                jump_password: args.jump_password.clone(),
                remember: args.remember,
            };
            let c = open_ssh(&app, &state, &session, &creds, mode, args.cols, args.rows, on_data.clone(), on_close).await?;
            // session tunnels: a failing one is reported in the terminal, not fatal
            for f in &session.forwards {
                match c.start_forward(f).await {
                    Ok(port) => on_data(format!("\x1b[90m[tunnel {}]\x1b[0m\r\n", forward_label(f, port)).into_bytes()),
                    Err(e) => on_data(format!("\x1b[33m[tunnel {} : {e}]\x1b[0m\r\n", forward_label(f, f.bind_port)).into_bytes()),
                }
            }
            Conn::Ssh(c)
        }
        "sftp" => {
            // file browser only: SSH connection without a shell
            let creds = SshSecrets {
                password: args.password.clone(),
                passphrase: args.passphrase.clone(),
                jump_password: args.jump_password.clone(),
                remember: args.remember,
            };
            Conn::Ssh(open_ssh(&app, &state, &session, &creds, ShellMode::None, 80, 24, on_data, on_close).await?)
        }
        "ftp" => {
            let anonymous = session.username.trim().is_empty() || session.username.eq_ignore_ascii_case("anonymous");
            let password = if anonymous { String::new() } else { session_password(&session, &args.password)? };
            let params = FtpParams {
                host: session.host.clone(),
                port: session.port,
                username: session.username.trim().to_string(),
                password: password.clone(),
                tls: if session.opt("ftp_tls") == "explicit" { FtpTls::Explicit } else { FtpTls::None },
                insecure: session.opt_bool("ftp_insecure"),
            };
            let c = match FtpConnection::connect(params).await {
                Err(AppError::AuthFailed) if args.password.is_none() && !anonymous => return Err(AppError::NeedPassword),
                r => r?,
            };
            if !anonymous && args.remember && session.remember_password && !session.id.is_empty() && args.password.is_some() {
                let _ = secrets::set_password(&session.id, &password);
            }
            Conn::Ftp(Arc::new(c))
        }
        "rdp" => {
            let creds = SshSecrets { password: None, passphrase: None, jump_password: args.jump_password.clone(), remember: args.remember };
            let (host, port, gateway) = via_gateway(&app, &state, &session, &creds).await?;
            let opts = RdpOptions::from_session(&session);
            let external = session.opt("rdp_client") == "mstsc";
            let conn = if !external {
                // embedded client (default): drawn in the tab, password via the keyring / prompt
                let password = session_password(&session, &args.password)?;
                let (mut user, mut domain) = (session.username.trim().to_string(), opts.domain.clone());
                // "DOMAIN\user" (user@domain.tld is passed as is: CredSSP accepts UPNs)
                if let Some((d, u)) = user.clone().split_once('\\') {
                    (domain, user) = (d.to_string(), u.to_string());
                }
                let params = RdpParams {
                    host: host.clone(),
                    port,
                    cert_host: session.host.clone(),
                    cert_port: session.port,
                    username: user,
                    password: password.clone(),
                    domain: if domain.is_empty() { None } else { Some(domain) },
                    width: opts.width.min(8192) as u16,
                    height: opts.height.min(8192) as u16,
                    known_hosts: state.config_dir.join("rdp_known_hosts"),
                    clipboard: opts.clipboard,
                    share_dir: opts.drives.then(|| rdp_share_dir(&session)),
                    sound: opts.sound == "local",
                };
                let prompter = make_prompter(app.clone(), state.hostkey_waiters.clone());
                match rdp_client::connect(params, prompter, on_close).await {
                    Ok(s) => {
                        if args.remember && session.remember_password && !session.id.is_empty() && args.password.is_some() {
                            let _ = secrets::set_password(&session.id, &password);
                        }
                        // system information: SSH to the same host with the same account (on demand)
                        let slot = if session.opt("rdp_monitor") == "0" {
                            MonitorSlot::Unavailable("informations système désactivées pour cette session".into())
                        } else if gateway.is_some() {
                            MonitorSlot::Unavailable("session via un rebond SSH : informations système indisponibles".into())
                        } else {
                            let u = session.username.trim();
                            let u = u.rsplit_once('\\').map(|(_, u)| u).unwrap_or(u);
                            MonitorSlot::Pending { host: session.host.clone(), username: u.to_string(), password: password.clone() }
                        };
                        state.monitors.lock().await.insert(conn_id.clone(), slot);
                        Conn::Rdp(Arc::new(s))
                    }
                    Err(e) => {
                        if let Some(g) = gateway {
                            g.disconnect().await;
                        }
                        // a stored password that no longer works must be asked again
                        return Err(match e {
                            AppError::AuthFailed if args.password.is_none() => AppError::NeedPassword,
                            e => e,
                        });
                    }
                }
            } else if cfg!(windows) {
                let content = rdp::rdp_file(&host, port, &session.username, &opts);
                let name = format!("orbiterm-{conn_id}");
                match ExternalProcess::launch_mstsc(&state.config_dir.join("rdp"), &name, &content, on_close) {
                    Ok(p) => Conn::External(p),
                    Err(e) => {
                        if let Some(g) = gateway {
                            g.disconnect().await;
                        }
                        return Err(e);
                    }
                }
            } else {
                let spec = PtySpawn {
                    program: rdp::freerdp()?.to_string_lossy().into_owned(),
                    args: rdp::freerdp_args(&host, port, &session.username, &opts),
                    cwd: None,
                    env: vec![],
                    cols: clamp16(args.cols),
                    rows: clamp16(args.rows),
                };
                let c = tokio::task::spawn_blocking(move || PtyConnection::spawn(spec, on_data, on_close))
                    .await
                    .map_err(|e| AppError::Conn(e.to_string()))??;
                Conn::Pty(c)
            };
            if let Some(g) = gateway {
                state.gateways.lock().await.insert(conn_id.clone(), g);
            }
            conn
        }
        "vnc" => {
            let creds = SshSecrets { password: None, passphrase: None, jump_password: args.jump_password.clone(), remember: args.remember };
            let (host, port, gateway) = via_gateway(&app, &state, &session, &creds).await?;
            let bridge = match VncBridge::start(host, port, on_close).await {
                Ok(b) => b,
                Err(e) => {
                    if let Some(g) = gateway {
                        g.disconnect().await;
                    }
                    return Err(e);
                }
            };
            if let Some(g) = gateway {
                state.gateways.lock().await.insert(conn_id.clone(), g);
            }
            Conn::Vnc(Arc::new(bridge))
        }
        "telnet" | "rlogin" => {
            let mode = if session.kind == "telnet" { Mode::Telnet } else { Mode::Rlogin };
            let c = TcpTermConnection::connect(
                mode,
                &session.host,
                session.port,
                &session.username,
                clamp16(args.cols),
                clamp16(args.rows),
                on_data,
                on_close,
            )
            .await?;
            Conn::Tcp(c)
        }
        "serial" => {
            let s = session.clone();
            let c = tokio::task::spawn_blocking(move || SerialConnection::open(&s, on_data, on_close))
                .await
                .map_err(|e| AppError::Conn(e.to_string()))??;
            Conn::Serial(c)
        }
        _ => {
            let spec = pty_spec(&session, args.cols, args.rows)?;
            let c = tokio::task::spawn_blocking(move || PtyConnection::spawn(spec, on_data, on_close))
                .await
                .map_err(|e| AppError::Conn(e.to_string()))??;
            Conn::Pty(c)
        }
    };
    state.conns.lock().await.insert(conn_id.clone(), conn);
    Ok(conn_id)
}

#[tauri::command]
pub async fn hostkey_answer(state: State<'_, AppState>, request_id: String, accept: bool) -> AppResult<()> {
    if let Some(tx) = state.hostkey_waiters.lock().await.remove(&request_id) {
        let _ = tx.send(accept);
    }
    Ok(())
}

#[tauri::command]
pub async fn term_write(state: State<'_, AppState>, conn_id: String, data: String) -> AppResult<()> {
    state.conn(&conn_id).await?.write(data.as_bytes()).await
}

/// Raw bytes typed / sent to a terminal (base64): ZMODEM transfers.
#[tauri::command]
pub async fn term_write_bytes(state: State<'_, AppState>, conn_id: String, data: String) -> AppResult<()> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| AppError::Conn(format!("données invalides : {e}")))?;
    state.conn(&conn_id).await?.write(&bytes).await
}

#[derive(Serialize)]
pub struct LocalFileInfo {
    pub size: u64,
    pub mtime: u64,
}

/// Size and modification time of a local file (ZMODEM sending).
#[tauri::command]
pub async fn local_file_info(path: String) -> AppResult<LocalFileInfo> {
    let md = tokio::fs::metadata(&path).await?;
    if !md.is_file() {
        return Err(AppError::InvalidPath(format!("{path} n'est pas un fichier")));
    }
    let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    Ok(LocalFileInfo { size: md.len(), mtime })
}

/// Reads up to `len` bytes (1 MiB at most) of a local file at `offset`, as base64.
#[tauri::command]
pub async fn local_read_chunk(path: String, offset: u64, len: u32) -> AppResult<String> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let mut f = tokio::fs::File::open(&path).await?;
    f.seek(std::io::SeekFrom::Start(offset)).await?;
    let mut buf = vec![0u8; len.min(1 << 20) as usize];
    let mut n = 0;
    while n < buf.len() {
        let k = f.read(&mut buf[n..]).await?;
        if k == 0 {
            break;
        }
        n += k;
    }
    buf.truncate(n);
    Ok(base64::engine::general_purpose::STANDARD.encode(buf))
}

/// Writes (create = start a new file) or appends base64 bytes to a local file in `dir`
/// (ZMODEM receiving). `name` must be a plain file name; a new file never replaces an
/// existing one (" (1)" is added). Returns the full path.
#[tauri::command]
pub async fn local_write_chunk(dir: String, name: String, data: String, create: bool) -> AppResult<String> {
    use tokio::io::AsyncWriteExt;
    let mut path = sftp::safe_local_join(std::path::Path::new(&dir), &name)?;
    if create && path.exists() {
        // never overwrite: "name (1).ext", "name (2).ext"…
        let p = std::path::Path::new(&name);
        let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        for i in 1.. {
            let cand = sftp::safe_local_join(std::path::Path::new(&dir), &format!("{stem} ({i}){ext}"))?;
            if !cand.exists() {
                path = cand;
                break;
            }
        }
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| AppError::Conn(format!("données invalides : {e}")))?;
    let mut f = if create {
        tokio::fs::File::create(&path).await?
    } else {
        tokio::fs::OpenOptions::new().append(true).open(&path).await?
    };
    f.write_all(&bytes).await?;
    f.flush().await?;
    Ok(path.to_string_lossy().into_owned())
}

/// Lines completed in a terminal tab, appended to its session log (if any).
#[tauri::command]
pub fn term_log(state: State<'_, AppState>, conn_id: String, text: String) {
    let log = state.logs.lock().ok().and_then(|l| l.get(&conn_id).cloned());
    if let Some(l) = log {
        l.write_text(&text);
    }
}

#[tauri::command]
pub async fn term_resize(state: State<'_, AppState>, conn_id: String, cols: u32, rows: u32) -> AppResult<()> {
    state.conn(&conn_id).await?.resize(cols, rows).await
}

#[tauri::command]
pub async fn term_close(app: AppHandle, state: State<'_, AppState>, conn_id: String) -> AppResult<()> {
    let log = state.logs.lock().ok().and_then(|l| l.get(&conn_id).cloned());
    if let Some(l) = log {
        // the tab may still send its last lines: closed a moment later
        let id = conn_id.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            l.close("onglet fermé");
            if let Ok(mut logs) = app.state::<AppState>().logs.lock() {
                logs.remove(&id);
            }
        });
    }
    let conn = state.conns.lock().await.remove(&conn_id);
    if let Some(c) = conn {
        c.close().await;
    }
    let gateway = state.gateways.lock().await.remove(&conn_id);
    if let Some(g) = gateway {
        g.disconnect().await;
    }
    let monitor = state.monitors.lock().await.remove(&conn_id);
    if let Some(MonitorSlot::Ready(m)) = monitor {
        m.disconnect().await;
    }
    Ok(())
}

/// System information of the remote host of a tab (SSH session, or RDP session
/// whose host also accepts SSH with the same account).
#[tauri::command]
pub async fn monitor_sample(app: AppHandle, state: State<'_, AppState>, conn_id: String) -> AppResult<crate::monitor::RemoteStats> {
    let direct = match state.conns.lock().await.get(&conn_id) {
        Some(Conn::Ssh(c)) => Some(c.clone()),
        Some(Conn::Rdp(_)) => None,
        Some(_) => return Err(AppError::Unsupported("pas d'informations système pour ce type de session".into())),
        None => return Err(AppError::Conn("connexion fermée".into())),
    };
    let ssh = match direct {
        Some(c) => c,
        None => {
            // RDP: one SSH connection per tab, opened on the first request
            let mut monitors = state.monitors.lock().await;
            let slot = monitors
                .remove(&conn_id)
                .unwrap_or_else(|| MonitorSlot::Unavailable("informations système désactivées pour cette session".into()));
            let (slot, res) = match slot {
                MonitorSlot::Ready(c) => (MonitorSlot::Ready(c.clone()), Ok(c)),
                MonitorSlot::Unavailable(m) => (MonitorSlot::Unavailable(m.clone()), Err(m)),
                MonitorSlot::Pending { host, username, password } => {
                    match open_monitor(&app, &state, &host, &username, &password).await {
                        Ok(c) => (MonitorSlot::Ready(c.clone()), Ok(c)),
                        Err(m) => (MonitorSlot::Unavailable(m.clone()), Err(m)),
                    }
                }
            };
            monitors.insert(conn_id.clone(), slot);
            res.map_err(AppError::Unsupported)?
        }
    };
    let out = ssh.exec_capture(&crate::monitor::command(), std::time::Duration::from_secs(10)).await?;
    Ok(crate::monitor::parse(&out))
}

async fn open_monitor(app: &AppHandle, state: &AppState, host: &str, username: &str, password: &str) -> Result<Arc<SshConnection>, String> {
    // quick probe: most desktops do not run an SSH server
    let probe = tokio::time::timeout(std::time::Duration::from_secs(3), tokio::net::TcpStream::connect((host, 22))).await;
    if !matches!(probe, Ok(Ok(_))) {
        return Err(format!("aucun accès SSH sur {host} (port 22 fermé) : CPU / mémoire indisponibles"));
    }
    let mut p = crate::ssh::ConnectParams::simple(
        host,
        22,
        username,
        Credentials::Password(password.to_string()),
        state.config_dir.join("known_hosts"),
    );
    p.mode = ShellMode::None;
    let prompter = make_prompter(app.clone(), state.hostkey_waiters.clone());
    let on_data: DataSink = Arc::new(|_| {});
    let on_close: CloseSink = Arc::new(|_| {});
    SshConnection::connect(p, prompter, on_data, on_close).await.map_err(|e| match e {
        AppError::AuthFailed | AppError::NeedPassword => "SSH : le compte RDP est refusé, informations système indisponibles".to_string(),
        AppError::HostKeyRejected => "clé SSH refusée : informations système indisponibles".to_string(),
        e => format!("SSH : {e}"),
    })
}

#[derive(Serialize)]
pub struct RdpView {
    url: String,
    width: u16,
    height: u16,
}

/// WebSocket URL (and desktop size) of an embedded RDP session.
#[tauri::command]
pub async fn rdp_url(state: State<'_, AppState>, conn_id: String) -> AppResult<RdpView> {
    match state.conn(&conn_id).await? {
        Conn::Rdp(s) => Ok(RdpView { url: s.url(), width: s.desktop.0, height: s.desktop.1 }),
        _ => Err(AppError::NotFound(conn_id)),
    }
}

/// WebSocket URL the VNC viewer connects to.
#[tauri::command]
pub async fn vnc_url(state: State<'_, AppState>, conn_id: String) -> AppResult<String> {
    match state.conn(&conn_id).await? {
        Conn::Vnc(b) => Ok(b.url()),
        _ => Err(AppError::NotFound(conn_id)),
    }
}

fn vnc_account(session_id: &str) -> String {
    format!("{session_id}#vnc")
}

/// VNC password remembered for a saved session (keyring), if any.
#[tauri::command]
pub async fn vnc_password_get(state: State<'_, AppState>, session_id: String) -> AppResult<Option<String>> {
    let store = state.store.lock().await;
    match store.session(&session_id) {
        Ok(s) if s.kind == "vnc" && s.remember_password => secrets::get_password(&vnc_account(&session_id)),
        _ => Ok(None),
    }
}

#[tauri::command]
pub async fn vnc_password_set(session_id: String, password: Option<String>) -> AppResult<()> {
    match password {
        Some(p) => secrets::set_password(&vnc_account(&session_id), &p),
        None => secrets::delete_password(&vnc_account(&session_id)),
    }
}

/// Whether the external window (mstsc) of a tab is still open.
#[tauri::command]
pub async fn external_running(state: State<'_, AppState>, conn_id: String) -> AppResult<bool> {
    match state.conn(&conn_id).await {
        Ok(Conn::External(p)) => Ok(p.running()),
        _ => Ok(false),
    }
}

#[tauri::command]
pub async fn local_shells() -> AppResult<Vec<LocalShell>> {
    Ok(tokio::task::spawn_blocking(pty::local_shells).await.unwrap_or_default())
}

#[tauri::command]
pub async fn serial_ports() -> AppResult<Vec<String>> {
    Ok(tokio::task::spawn_blocking(crate::serial::list_ports).await.unwrap_or_default())
}

// ---------------------------------------------------------------- sftp

// The "sftp_*" commands serve every file browser: SFTP over SSH, or FTP.

#[tauri::command]
pub async fn sftp_home(state: State<'_, AppState>, conn_id: String) -> AppResult<String> {
    match state.files(&conn_id).await? {
        Files::Sftp(s) => sftp::home(&s).await,
        Files::Scp(c) => scp::home(&c).await,
        Files::Ftp(f) => Ok(f.home()),
    }
}

/// How the file browser of a tab reaches the files: "sftp", "scp" (SSH without SFTP) or "ftp".
#[tauri::command]
pub async fn sftp_mode(state: State<'_, AppState>, conn_id: String) -> AppResult<String> {
    Ok(match state.files(&conn_id).await? {
        Files::Sftp(_) => "sftp",
        Files::Scp(_) => "scp",
        Files::Ftp(_) => "ftp",
    }
    .into())
}

#[tauri::command]
pub async fn sftp_list(state: State<'_, AppState>, conn_id: String, path: String) -> AppResult<Vec<Entry>> {
    match state.files(&conn_id).await? {
        Files::Sftp(s) => sftp::list(&s, &path).await,
        Files::Scp(c) => scp::list(&c, &path).await,
        Files::Ftp(f) => f.list(&path).await,
    }
}

#[tauri::command]
pub async fn sftp_mkdir(state: State<'_, AppState>, conn_id: String, path: String) -> AppResult<()> {
    match state.files(&conn_id).await? {
        Files::Sftp(s) => sftp::mkdir(&s, &path).await,
        Files::Scp(c) => scp::mkdir(&c, &path).await,
        Files::Ftp(f) => f.mkdir(&path).await,
    }
}

#[tauri::command]
pub async fn sftp_rename(state: State<'_, AppState>, conn_id: String, from: String, to: String) -> AppResult<()> {
    match state.files(&conn_id).await? {
        Files::Sftp(s) => sftp::rename(&s, &from, &to).await,
        Files::Scp(c) => scp::rename(&c, &from, &to).await,
        Files::Ftp(f) => f.rename(&from, &to).await,
    }
}

#[tauri::command]
pub async fn sftp_remove(state: State<'_, AppState>, conn_id: String, path: String) -> AppResult<()> {
    match state.files(&conn_id).await? {
        Files::Sftp(s) => sftp::remove(&s, &path).await,
        Files::Scp(c) => scp::remove(&c, &path).await,
        Files::Ftp(f) => f.remove(&path).await,
    }
}

#[derive(Clone, Serialize)]
struct TransferProgress {
    transfer_id: String,
    done: u64,
    total: u64,
}

fn progress_emitter(app: AppHandle, transfer_id: String) -> impl Fn(u64, u64) + Send + Sync {
    let last = std::sync::Mutex::new(std::time::Instant::now() - Duration::from_secs(1));
    move |done, total| {
        // throttle to ~10 events/s, but always send the final one
        let mut l = last.lock().unwrap();
        if done >= total || l.elapsed() >= Duration::from_millis(100) {
            *l = std::time::Instant::now();
            let _ = app.emit("transfer-progress", TransferProgress { transfer_id: transfer_id.clone(), done, total });
        }
    }
}

/// Downloads `remote` into the local directory `local_dir` (keeps the remote file name).
#[tauri::command]
pub async fn sftp_download(
    app: AppHandle,
    state: State<'_, AppState>,
    conn_id: String,
    remote: String,
    local_dir: String,
    transfer_id: String,
) -> AppResult<String> {
    let files = state.files(&conn_id).await?;
    let name = remote.rsplit('/').next().unwrap_or("").to_string();
    let local = sftp::safe_local_join(std::path::Path::new(&local_dir), &name)?;
    let progress = progress_emitter(app, transfer_id);
    match files {
        Files::Sftp(s) => sftp::download(&s, &remote, &local, &progress).await?,
        Files::Scp(c) => scp::download(&c, &remote, &local, &progress).await?,
        Files::Ftp(f) => f.download(&remote, &local, &progress).await?,
    }
    Ok(local.to_string_lossy().into_owned())
}

/// Uploads the local file `local` into the remote directory `remote_dir`.
#[tauri::command]
pub async fn sftp_upload(
    app: AppHandle,
    state: State<'_, AppState>,
    conn_id: String,
    local: String,
    remote_dir: String,
    transfer_id: String,
) -> AppResult<String> {
    let files = state.files(&conn_id).await?;
    let local_path = PathBuf::from(&local);
    let name = local_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| AppError::InvalidPath(local.clone()))?;
    let remote = sftp::remote_join(&remote_dir, &name);
    let progress = progress_emitter(app, transfer_id);
    match files {
        Files::Sftp(s) => sftp::upload(&s, &local_path, &remote, &progress).await?,
        Files::Scp(c) => scp::upload(&c, &local_path, &remote, &progress).await?,
        Files::Ftp(f) => f.upload(&local_path, &remote, &progress).await?,
    }
    Ok(remote)
}

/// Current folder of the shell of an SSH tab (None when the server cannot tell).
#[tauri::command]
pub async fn ssh_cwd(state: State<'_, AppState>, conn_id: String) -> AppResult<Option<String>> {
    state.ssh(&conn_id).await?.shell_cwd().await
}

// ---------------------------------------------------------------- configuration export / import

/// Writes sessions, folders, macros, tunnels (and the interface settings when given) to `path`.
#[tauri::command]
pub async fn config_export(state: State<'_, AppState>, path: String, prefs: Option<serde_json::Value>) -> AppResult<()> {
    let store = state.store.lock().await.clone();
    bundle::export(std::path::Path::new(&path), &store, prefs, env!("CARGO_PKG_VERSION"))
}

#[derive(Serialize)]
pub struct ConfigImport {
    pub stats: bundle::MergeStats,
    /// interface settings found in the file (applied by the interface if asked)
    pub prefs: Option<serde_json::Value>,
}

/// Adds the content of a configuration file to the current one (nothing is replaced).
#[tauri::command]
pub async fn config_import(state: State<'_, AppState>, path: String) -> AppResult<ConfigImport> {
    let b = bundle::read(std::path::Path::new(&path))?;
    let mut store = state.store.lock().await;
    let stats = bundle::merge(&mut store, &b.store);
    store.save(&state.config_dir)?;
    Ok(ConfigImport { stats, prefs: b.prefs })
}

/// Compares two local folders (recursively).
#[tauri::command]
pub async fn dir_compare(left: String, right: String) -> AppResult<crate::dircmp::DirDiff> {
    tokio::task::spawn_blocking(move || crate::dircmp::compare(std::path::Path::new(&left), std::path::Path::new(&right)))
        .await
        .map_err(|e| AppError::Conn(e.to_string()))?
}

// ---------------------------------------------------------------- text editor

/// Temporary copy used to read / write a remote file through the file connection.
fn edit_temp() -> AppResult<(tempfile::TempDir, PathBuf)> {
    let dir = tempfile::Builder::new().prefix("orbiterm-edit").tempdir()?;
    let p = dir.path().join("file");
    Ok((dir, p))
}

/// Reads a text file: local when `conn_id` is empty, else through the SFTP / FTP connection.
#[tauri::command]
pub async fn text_read(state: State<'_, AppState>, conn_id: String, path: String) -> AppResult<editor::TextFile> {
    let bytes = if conn_id.is_empty() {
        let len = tokio::fs::metadata(&path).await?.len();
        if len > editor::MAX_BYTES {
            return Err(AppError::Unsupported("fichier trop volumineux pour l'éditeur (20 Mo au plus)".into()));
        }
        tokio::fs::read(&path).await?
    } else {
        let files = state.files(&conn_id).await?;
        let (_dir, tmp) = edit_temp()?;
        let too_big = AppError::Unsupported("fichier trop volumineux pour l'éditeur (20 Mo au plus)".into());
        let check = |_: u64, _: u64| {};
        match files {
            Files::Sftp(s) => {
                if s.metadata(path.as_str()).await?.size.unwrap_or(0) > editor::MAX_BYTES {
                    return Err(too_big);
                }
                sftp::download(&s, &path, &tmp, &check).await?
            }
            Files::Scp(c) => scp::download(&c, &path, &tmp, &check).await?,
            Files::Ftp(f) => f.download(&path, &tmp, &check).await?,
        }
        let b = tokio::fs::read(&tmp).await?;
        if b.len() as u64 > editor::MAX_BYTES {
            return Err(too_big);
        }
        b
    };
    editor::decode(&bytes)
}

/// Writes a text file back with its original encoding and line endings.
#[tauri::command]
pub async fn text_write(state: State<'_, AppState>, conn_id: String, path: String, file: editor::TextFile) -> AppResult<()> {
    let bytes = editor::encode(&file)?;
    if conn_id.is_empty() {
        let p = PathBuf::from(&path);
        return tokio::task::spawn_blocking(move || editor::write_local(&p, &bytes))
            .await
            .map_err(|e| AppError::Conn(e.to_string()))?;
    }
    let files = state.files(&conn_id).await?;
    let (_dir, tmp) = edit_temp()?;
    tokio::fs::write(&tmp, &bytes).await?;
    let none = |_: u64, _: u64| {};
    match files {
        Files::Sftp(s) => sftp::upload(&s, &tmp, &path, &none).await,
        Files::Scp(c) => scp::upload(&c, &tmp, &path, &none).await,
        Files::Ftp(f) => f.upload(&tmp, &path, &none).await,
    }
}

// ---------------------------------------------------------------- tunnels

/// Port forwardings currently active on a terminal connection.
#[tauri::command]
pub async fn term_forwards(state: State<'_, AppState>, conn_id: String) -> AppResult<Vec<ActiveForward>> {
    Ok(state.ssh(&conn_id).await?.active_forwards())
}

#[derive(Serialize)]
pub struct TunnelStatus {
    #[serde(flatten)]
    pub tunnel: Tunnel,
    pub running: bool,
    pub port: u16,
}

#[tauri::command]
pub async fn tunnels_list(state: State<'_, AppState>) -> AppResult<Vec<TunnelStatus>> {
    let tunnels = state.store.lock().await.tunnels.clone();
    let mut running = state.tunnels.lock().await;
    // forget tunnels whose SSH connection dropped
    running.retain(|_, c| !c.is_closed());
    Ok(tunnels
        .into_iter()
        .map(|t| {
            let conn = running.get(&t.id);
            let port = conn.and_then(|c| c.active_forwards().first().map(|f| f.port)).unwrap_or(0);
            TunnelStatus { running: conn.is_some(), port, tunnel: t }
        })
        .collect())
}

#[tauri::command]
pub async fn tunnel_save(state: State<'_, AppState>, tunnel: Tunnel) -> AppResult<Tunnel> {
    let mut store = state.store.lock().await;
    let saved = store.upsert_tunnel(tunnel);
    store.save(&state.config_dir)?;
    Ok(saved)
}

#[tauri::command]
pub async fn tunnel_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    tunnel_stop(state.clone(), id.clone()).await?;
    let mut store = state.store.lock().await;
    store.delete_tunnel(&id);
    store.save(&state.config_dir)
}

#[derive(Deserialize)]
pub struct TunnelStartArgs {
    pub id: String,
    pub password: Option<String>,
    pub passphrase: Option<String>,
    pub jump_password: Option<String>,
    #[serde(default)]
    pub remember: bool,
}

#[tauri::command]
pub async fn tunnel_start(app: AppHandle, state: State<'_, AppState>, args: TunnelStartArgs) -> AppResult<u16> {
    if let Some(c) = state.tunnels.lock().await.get(&args.id)
        && !c.is_closed()
    {
        return Ok(c.active_forwards().first().map(|f| f.port).unwrap_or(0));
    }
    let (tunnel, session) = {
        let store = state.store.lock().await;
        let t = store.tunnels.iter().find(|t| t.id == args.id).cloned().ok_or_else(|| AppError::NotFound(args.id.clone()))?;
        let s = store.session(&t.session_id)?.clone();
        (t, s)
    };
    if session.kind != "ssh" {
        return Err(AppError::Config(format!("la session « {} » n'est pas une session SSH", session.name)));
    }
    let creds = SshSecrets {
        password: args.password,
        passphrase: args.passphrase,
        jump_password: args.jump_password,
        remember: args.remember,
    };
    let quiet_data: DataSink = Arc::new(|_| {});
    let quiet_close: CloseSink = Arc::new(|_| {});
    let conn = open_ssh(&app, &state, &session, &creds, ShellMode::None, 80, 24, quiet_data, quiet_close).await?;
    match conn.start_forward(&tunnel.forward).await {
        Ok(port) => {
            state.tunnels.lock().await.insert(tunnel.id.clone(), conn);
            Ok(port)
        }
        Err(e) => {
            conn.disconnect().await;
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn tunnel_stop(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let conn = state.tunnels.lock().await.remove(&id);
    if let Some(c) = conn {
        c.disconnect().await;
    }
    Ok(())
}

// ---------------------------------------------------------------- X server

#[tauri::command]
pub async fn xserver_status() -> AppResult<x11::XServerStatus> {
    tokio::task::spawn_blocking(x11::status).await.map_err(|e| AppError::Conn(e.to_string()))
}

#[tauri::command]
pub async fn xserver_start() -> AppResult<x11::XServerStatus> {
    tokio::task::spawn_blocking(x11::start).await.map_err(|e| AppError::Conn(e.to_string()))?
}

#[tauri::command]
pub async fn xserver_stop() -> AppResult<x11::XServerStatus> {
    tokio::task::spawn_blocking(x11::stop).await.map_err(|e| AppError::Conn(e.to_string()))
}
/// Local folder shared with the remote computer (embedded RDP): the chosen one, else the home folder.
fn rdp_share_dir(session: &Session) -> std::path::PathBuf {
    let d = session.opt("rdp_share_dir").trim();
    if !d.is_empty() {
        return std::path::PathBuf::from(d);
    }
    directories::UserDirs::new().map(|u| u.home_dir().to_path_buf()).unwrap_or_else(|| std::path::PathBuf::from("."))
}
