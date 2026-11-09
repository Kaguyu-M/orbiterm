//! Tauri commands = the API the UI calls with `invoke(...)`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{Mutex, oneshot};

use crate::config::{AuthMethod, Folder, Session, Store};
use crate::error::{AppError, AppResult};
use crate::secrets;
use crate::sftp::{self, Entry};
use crate::pty::{self, LocalShell, PtyConnection, PtySpawn};
use crate::serial::SerialConnection;
use crate::ssh::{CloseSink, ConnectParams, Credentials, DataSink, HostKeyPrompter, HostKeyQuestion, SshConnection};
use crate::telnet::{Mode, TcpTermConnection};
use crate::terminal::Conn;

pub struct AppState {
    pub config_dir: PathBuf,
    pub store: Mutex<Store>,
    pub conns: Mutex<HashMap<String, Conn>>,
    pub hostkey_waiters: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
}

impl AppState {
    pub fn new(config_dir: PathBuf) -> AppResult<Self> {
        let store = Store::load(&config_dir)?;
        Ok(Self {
            config_dir,
            store: Mutex::new(store),
            conns: Mutex::new(HashMap::new()),
            hostkey_waiters: Arc::new(Mutex::new(HashMap::new())),
        })
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

fn sinks(app: &AppHandle, conn_id: &str) -> (DataSink, CloseSink) {
    let (app_d, id_d) = (app.clone(), conn_id.to_string());
    let on_data: DataSink = Arc::new(move |bytes: Vec<u8>| {
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        let _ = app_d.emit("term-data", TermData { conn_id: id_d.clone(), data });
    });
    let (app_c, id_c) = (app.clone(), conn_id.to_string());
    let on_close: CloseSink = Arc::new(move |reason: String| {
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

/// Opens a terminal tab for any session kind. Returns the connection id.
#[tauri::command]
pub async fn term_open(app: AppHandle, state: State<'_, AppState>, args: OpenArgs) -> AppResult<String> {
    // a saved session is authoritative (the UI may hold a stale copy)
    let session = if args.session.id.is_empty() {
        args.session.clone()
    } else {
        state.store.lock().await.session(&args.session.id).cloned().unwrap_or(args.session.clone())
    };
    let saved = !session.id.is_empty();
    let conn_id = crate::config::new_id("c");
    let (on_data, on_close) = sinks(&app, &conn_id);

    let conn = match session.kind.as_str() {
        "ssh" => {
            let typed_password = args.password.clone();
            let credentials = match session.auth {
                AuthMethod::Password => {
                    let pw = match typed_password.clone() {
                        Some(p) => p,
                        None if saved && session.remember_password => {
                            secrets::get_password(&session.id)?.ok_or(AppError::NeedPassword)?
                        }
                        None => return Err(AppError::NeedPassword),
                    };
                    Credentials::Password(pw)
                }
                AuthMethod::Key => {
                    if session.key_path.trim().is_empty() {
                        return Err(AppError::Config("aucune clé privée sélectionnée".into()));
                    }
                    Credentials::Key { path: PathBuf::from(&session.key_path), passphrase: args.passphrase.clone() }
                }
            };
            let params = ConnectParams {
                host: session.host.clone(),
                port: session.port,
                username: session.username.clone(),
                credentials,
                cols: args.cols,
                rows: args.rows,
                known_hosts: state.config_dir.join("known_hosts"),
            };
            let prompter = make_prompter(app.clone(), state.hostkey_waiters.clone());
            match SshConnection::connect(params, prompter, on_data, on_close).await {
                Ok(c) => {
                    if let (Some(pw), true) = (typed_password, saved && session.remember_password && args.remember)
                        && let Err(e) = secrets::set_password(&session.id, &pw)
                    {
                        log::warn!("keyring: {e}");
                    }
                    Conn::Ssh(c)
                }
                // a stored password that no longer works must be asked again
                Err(AppError::AuthFailed) if args.password.is_none() && session.auth == AuthMethod::Password => {
                    return Err(AppError::NeedPassword);
                }
                Err(e) => return Err(e),
            }
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

#[tauri::command]
pub async fn term_resize(state: State<'_, AppState>, conn_id: String, cols: u32, rows: u32) -> AppResult<()> {
    state.conn(&conn_id).await?.resize(cols, rows).await
}

#[tauri::command]
pub async fn term_close(state: State<'_, AppState>, conn_id: String) -> AppResult<()> {
    let conn = state.conns.lock().await.remove(&conn_id);
    if let Some(c) = conn {
        c.close().await;
    }
    Ok(())
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

#[tauri::command]
pub async fn sftp_home(state: State<'_, AppState>, conn_id: String) -> AppResult<String> {
    let s = state.ssh(&conn_id).await?.sftp().await?;
    sftp::home(&s).await
}

#[tauri::command]
pub async fn sftp_list(state: State<'_, AppState>, conn_id: String, path: String) -> AppResult<Vec<Entry>> {
    let s = state.ssh(&conn_id).await?.sftp().await?;
    sftp::list(&s, &path).await
}

#[tauri::command]
pub async fn sftp_mkdir(state: State<'_, AppState>, conn_id: String, path: String) -> AppResult<()> {
    let s = state.ssh(&conn_id).await?.sftp().await?;
    sftp::mkdir(&s, &path).await
}

#[tauri::command]
pub async fn sftp_rename(state: State<'_, AppState>, conn_id: String, from: String, to: String) -> AppResult<()> {
    let s = state.ssh(&conn_id).await?.sftp().await?;
    sftp::rename(&s, &from, &to).await
}

#[tauri::command]
pub async fn sftp_remove(state: State<'_, AppState>, conn_id: String, path: String) -> AppResult<()> {
    let s = state.ssh(&conn_id).await?.sftp().await?;
    sftp::remove(&s, &path).await
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
    let s = state.ssh(&conn_id).await?.sftp().await?;
    let name = remote.rsplit('/').next().unwrap_or("").to_string();
    let local = sftp::safe_local_join(std::path::Path::new(&local_dir), &name)?;
    let progress = progress_emitter(app, transfer_id);
    sftp::download(&s, &remote, &local, &progress).await?;
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
    let s = state.ssh(&conn_id).await?.sftp().await?;
    let local_path = PathBuf::from(&local);
    let name = local_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| AppError::InvalidPath(local.clone()))?;
    let remote = sftp::remote_join(&remote_dir, &name);
    let progress = progress_emitter(app, transfer_id);
    sftp::upload(&s, &local_path, &remote, &progress).await?;
    Ok(remote)
}
