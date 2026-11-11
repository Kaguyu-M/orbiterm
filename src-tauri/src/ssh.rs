//! SSH connection: authentication (optionally through a jump host), interactive
//! shell or remote command, lazy SFTP channel, port forwardings and X11.
//!
//! One `SshConnection` = one SSH transport, multiplexed:
//!   - the interactive shell channel with a PTY (the terminal tab), optional
//!   - the SFTP subsystem channel, opened on first use (the file browser)
//!   - direct-tcpip / forwarded-tcpip / x11 channels for tunnels and X11
//!
//! This module does not depend on Tauri: output and host-key questions go through
//! callbacks, so it can be tested with a plain SSH server.

use std::borrow::Cow;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use russh::client::{self, ChannelOpenHandle, Handle, Msg};
use russh::keys::{PrivateKeyWithHashAlg, PublicKeyOrCertificate, ssh_key::PublicKey};
use russh::{Channel, ChannelMsg, ChannelOpenFailure, ChannelWriteHalf, Disconnect};
use russh_sftp::client::SftpSession;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::config::{Forward, ForwardKind};
use crate::error::{AppError, AppResult};
use crate::forward;
use crate::known_hosts::{self, HostKeyStatus};
use crate::x11::{self, XTarget};

/// Information shown to the user when a host key must be confirmed.
#[derive(Debug, Clone, Serialize)]
pub struct HostKeyQuestion {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    pub fingerprint: String,
    pub status: HostKeyStatus,
}

/// Asks the user whether to trust a host key (returns true to accept).
pub type HostKeyPrompter =
    Arc<dyn Fn(HostKeyQuestion) -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync>;
/// Receives terminal output bytes.
pub type DataSink = Arc<dyn Fn(Vec<u8>) + Send + Sync>;
/// Called once when the shell ends (reason text).
pub type CloseSink = Arc<dyn Fn(String) + Send + Sync>;

pub enum Credentials {
    Password(String),
    Key { path: PathBuf, passphrase: Option<String> },
}

/// What to run on the main channel.
pub enum ShellMode {
    /// interactive login shell
    Shell,
    /// a remote command in a PTY (session "execute command")
    Command(String),
    /// no channel at all (tunnels only)
    None,
}

pub struct JumpParams {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub credentials: Credentials,
}

pub struct ConnectParams {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub credentials: Credentials,
    pub cols: u32,
    pub rows: u32,
    pub known_hosts: PathBuf,
    pub jump: Option<JumpParams>,
    pub mode: ShellMode,
    /// local DISPLAY to forward X11 to (None = no X11 forwarding)
    pub x11_display: Option<String>,
    pub compression: bool,
    /// keepalive interval in seconds (0 = disabled)
    pub keepalive: u32,
}

impl ConnectParams {
    /// Plain interactive session (used by tests and simple callers).
    pub fn simple(host: &str, port: u16, username: &str, credentials: Credentials, known_hosts: PathBuf) -> Self {
        Self {
            host: host.into(),
            port,
            username: username.into(),
            credentials,
            cols: 80,
            rows: 24,
            known_hosts,
            jump: None,
            mode: ShellMode::Shell,
            x11_display: None,
            compression: false,
            keepalive: 30,
        }
    }
}

#[derive(Clone)]
struct X11Fwd {
    target: XTarget,
    fake: Vec<u8>,
    real: Option<Vec<u8>>,
}

/// State shared between the connection and its handler (server-initiated channels).
#[derive(Default)]
struct Shared {
    /// remote forwardings: port on the server -> local destination
    remote: StdMutex<HashMap<u32, (String, u16)>>,
    x11: StdMutex<Option<X11Fwd>>,
}

pub struct ClientHandler {
    host: String,
    port: u16,
    known_hosts: PathBuf,
    prompter: HostKeyPrompter,
    /// Set when the user refused the key, to report a precise error.
    rejected: Arc<std::sync::atomic::AtomicBool>,
    shared: Arc<Shared>,
}

fn extract_key(k: &PublicKeyOrCertificate) -> PublicKey {
    match k {
        PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
        PublicKeyOrCertificate::Certificate(c) => PublicKey::from(c.public_key().clone()),
    }
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, server_key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let key = extract_key(server_key);
        let status = match known_hosts::check(&self.known_hosts, &self.host, self.port, &key) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("known_hosts check failed: {e}");
                HostKeyStatus::Unknown
            }
        };
        if status == HostKeyStatus::Known {
            return Ok(true);
        }
        let question = HostKeyQuestion {
            host: self.host.clone(),
            port: self.port,
            algorithm: key.algorithm().to_string(),
            fingerprint: known_hosts::fingerprint(&key),
            status,
        };
        let accepted = (self.prompter)(question).await;
        if !accepted {
            self.rejected.store(true, std::sync::atomic::Ordering::SeqCst);
            return Ok(false);
        }
        let replace = status == HostKeyStatus::Changed;
        if let Err(e) = known_hosts::learn(&self.known_hosts, &self.host, self.port, &key, replace) {
            log::warn!("could not record host key: {e}");
        }
        Ok(true)
    }

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: Channel<Msg>,
        _connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let target = self.shared.remote.lock().unwrap().get(&connected_port).cloned();
        match target {
            Some((host, port)) => match forward::connect_local(&host, port).await {
                Ok(tcp) => {
                    reply.accept().await;
                    tokio::spawn(forward::pump(channel, tcp));
                }
                Err(e) => {
                    log::warn!("remote forward -> {host}:{port}: {e}");
                    reply.reject(ChannelOpenFailure::ConnectFailed).await;
                }
            },
            None => reply.reject(ChannelOpenFailure::AdministrativelyProhibited).await,
        }
        Ok(())
    }

    async fn server_channel_open_x11(
        &mut self,
        channel: Channel<Msg>,
        _originator_address: &str,
        _originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let fwd = self.shared.x11.lock().unwrap().clone();
        match fwd {
            Some(f) => {
                reply.accept().await;
                tokio::spawn(x11_pump(channel, f));
            }
            None => reply.reject(ChannelOpenFailure::AdministrativelyProhibited).await,
        }
        Ok(())
    }
}

/// Delivers one forwarded X11 connection to the local X server, after replacing
/// the fake authentication cookie.
async fn x11_pump(channel: Channel<Msg>, f: X11Fwd) {
    let mut remote = channel.into_stream();
    let mut buf = Vec::with_capacity(256);
    let mut tmp = [0u8; 1024];
    let setup = loop {
        match x11::rewrite_setup(&buf, &f.fake, f.real.as_deref()) {
            Some(Ok(v)) => break v,
            Some(Err(())) => {
                log::warn!("X11: wrong authentication cookie, connection dropped");
                return;
            }
            None => match remote.read(&mut tmp).await {
                Ok(0) | Err(_) => return,
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
            },
        }
        if buf.len() > 64 * 1024 {
            return;
        }
    };
    let result = match &f.target {
        XTarget::Tcp { host, port } => match forward::connect_local(host, *port).await {
            Ok(mut local) => {
                if local.write_all(&setup).await.is_ok() {
                    let _ = tokio::io::copy_bidirectional(&mut local, &mut remote).await;
                }
                Ok(())
            }
            Err(e) => Err(e),
        },
        #[cfg(unix)]
        XTarget::Unix(path) => match tokio::net::UnixStream::connect(path).await {
            Ok(mut local) => {
                if local.write_all(&setup).await.is_ok() {
                    let _ = tokio::io::copy_bidirectional(&mut local, &mut remote).await;
                }
                Ok(())
            }
            Err(e) => Err(e),
        },
    };
    if let Err(e) = result {
        log::warn!("X11: local X server unreachable: {e}");
    }
    let _ = remote.shutdown().await;
}

fn client_config(compression: bool, keepalive: u32) -> Arc<client::Config> {
    let mut cfg = client::Config {
        keepalive_interval: (keepalive > 0).then(|| Duration::from_secs(keepalive as u64)),
        keepalive_max: 3,
        ..Default::default()
    };
    if compression {
        cfg.preferred.compression = Cow::Borrowed(&[
            russh::compression::ZLIB_LEGACY,
            russh::compression::ZLIB,
            russh::compression::NONE,
        ]);
    }
    Arc::new(cfg)
}

/// An active forwarding (for listing / stopping).
#[derive(Debug, Clone, Serialize)]
pub struct ActiveForward {
    pub forward: Forward,
    /// actual listening port (useful when 0 was requested)
    pub port: u16,
}

pub struct SshConnection {
    handle: Arc<Handle<ClientHandler>>,
    writer: Option<ChannelWriteHalf<Msg>>,
    sftp: Mutex<Option<Arc<SftpSession>>>,
    /// the jump host connection, kept alive as long as this one
    jump: Option<Handle<ClientHandler>>,
    shared: Arc<Shared>,
    tasks: StdMutex<Vec<JoinHandle<()>>>,
    forwards: StdMutex<Vec<ActiveForward>>,
    /// value of LC_ORBITERM_ID given to the interactive shell: finds it again on the server
    /// (current folder followed by the file browser)
    shell_tag: String,
}

async fn open_transport(
    config: Arc<client::Config>,
    host: &str,
    port: u16,
    known_hosts: &std::path::Path,
    prompter: HostKeyPrompter,
    shared: Arc<Shared>,
    via: Option<&Handle<ClientHandler>>,
) -> AppResult<Handle<ClientHandler>> {
    let rejected = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handler = ClientHandler {
        host: host.to_string(),
        port,
        known_hosts: known_hosts.to_path_buf(),
        prompter,
        rejected: rejected.clone(),
        shared,
    };
    // The TCP connect + key exchange gets a deadline; the host-key prompt may wait
    // for the user, so the deadline is generous.
    let res = match via {
        None => tokio::time::timeout(Duration::from_secs(300), client::connect(config, (host, port), handler)).await,
        Some(jump) => {
            let ch = jump
                .channel_open_direct_tcpip(host, port as u32, "127.0.0.1", 0)
                .await
                .map_err(|e| AppError::Conn(format!("le rebond SSH n'a pas pu joindre {host}:{port} : {e}")))?;
            tokio::time::timeout(Duration::from_secs(300), client::connect_stream(config, ch.into_stream(), handler)).await
        }
    };
    match res {
        Err(_) => Err(AppError::Timeout),
        Ok(Err(e)) => {
            if rejected.load(std::sync::atomic::Ordering::SeqCst) {
                Err(AppError::HostKeyRejected)
            } else {
                Err(e.into())
            }
        }
        Ok(Ok(h)) => Ok(h),
    }
}

impl SshConnection {
    pub async fn connect(
        p: ConnectParams,
        prompter: HostKeyPrompter,
        on_data: DataSink,
        on_close: CloseSink,
    ) -> AppResult<Arc<SshConnection>> {
        let config = client_config(p.compression, p.keepalive);
        let shared = Arc::new(Shared::default());
        let shell_tag = uuid::Uuid::new_v4().simple().to_string();

        // optional jump host (ProxyJump), authenticated with its own credentials
        let jump = match p.jump {
            Some(j) => {
                let mut jh = open_transport(
                    config.clone(),
                    &j.host,
                    j.port,
                    &p.known_hosts,
                    prompter.clone(),
                    Arc::new(Shared::default()),
                    None,
                )
                .await?;
                match authenticate(&mut jh, &j.username, j.credentials).await {
                    Ok(()) => {}
                    Err(AppError::AuthFailed) => return Err(AppError::JumpAuthFailed),
                    Err(AppError::NeedPassphrase) => {
                        return Err(AppError::Config("la clé du rebond SSH est protégée par une passphrase (non prise en charge)".into()));
                    }
                    Err(e) => return Err(e),
                }
                Some(jh)
            }
            None => None,
        };

        let mut handle =
            open_transport(config, &p.host, p.port, &p.known_hosts, prompter, shared.clone(), jump.as_ref()).await?;
        authenticate(&mut handle, &p.username, p.credentials).await?;

        let writer = match p.mode {
            ShellMode::None => None,
            mode => {
                let channel = handle.channel_open_session().await?;
                if let Some(display) = &p.x11_display {
                    if let Some((target, screen)) = x11::parse_display(display) {
                        let (fake, fake_hex) = x11::fake_cookie();
                        let real = x11::real_cookie(display);
                        *shared.x11.lock().unwrap() = Some(X11Fwd { target, fake, real });
                        channel.request_x11(true, false, x11::COOKIE_PROTO, fake_hex, screen).await?;
                    } else {
                        log::warn!("X11: invalid display {display}");
                    }
                }
                // LC_* variables are accepted by default by OpenSSH servers (AcceptEnv LANG LC_*)
                let _ = channel.set_env(false, "LC_ORBITERM_ID", shell_tag.as_str()).await;
                channel.request_pty(true, "xterm-256color", p.cols.max(1), p.rows.max(1), 0, 0, &[]).await?;
                match mode {
                    ShellMode::Command(cmd) => channel.exec(true, cmd).await?,
                    _ => channel.request_shell(true).await?,
                }
                let (mut reader, writer) = channel.split();
                tokio::spawn(async move {
                    let mut exit: Option<u32> = None;
                    loop {
                        match reader.wait().await {
                            Some(ChannelMsg::Data { data }) => on_data(data.to_vec()),
                            Some(ChannelMsg::ExtendedData { data, .. }) => on_data(data.to_vec()),
                            Some(ChannelMsg::ExitStatus { exit_status }) => exit = Some(exit_status),
                            Some(ChannelMsg::Close) | None => break,
                            _ => {}
                        }
                    }
                    on_close(match exit {
                        Some(code) => format!("Session terminée (code {code})"),
                        None => "Connexion fermée".to_string(),
                    });
                });
                Some(writer)
            }
        };

        Ok(Arc::new(SshConnection {
            handle: Arc::new(handle),
            writer,
            sftp: Mutex::new(None),
            jump,
            shared,
            tasks: StdMutex::new(vec![]),
            forwards: StdMutex::new(vec![]),
            shell_tag,
        }))
    }

    pub async fn write(&self, data: &[u8]) -> AppResult<()> {
        match &self.writer {
            Some(w) => w.data(data).await.map_err(Into::into),
            None => Ok(()),
        }
    }

    pub async fn resize(&self, cols: u32, rows: u32) -> AppResult<()> {
        match &self.writer {
            Some(w) => w.window_change(cols.max(1), rows.max(1), 0, 0).await.map_err(Into::into),
            None => Ok(()),
        }
    }

    pub fn is_closed(&self) -> bool {
        self.handle.is_closed()
    }

    /// Opens (once) and returns the SFTP session carried by this connection.
    pub async fn sftp(&self) -> AppResult<Arc<SftpSession>> {
        let mut guard = self.sftp.lock().await;
        if let Some(s) = guard.as_ref() {
            return Ok(s.clone());
        }
        let ch = self.handle.channel_open_session().await?;
        ch.request_subsystem(true, "sftp").await?;
        let s = Arc::new(SftpSession::new(ch.into_stream()).await?);
        *guard = Some(s.clone());
        Ok(s)
    }

    /// Current folder of the interactive shell (Linux servers: /proc), or None.
    pub async fn shell_cwd(&self) -> AppResult<Option<String>> {
        let out = self.exec_capture(&cwd_command(&self.shell_tag), Duration::from_secs(5)).await?;
        let line = out.lines().next().unwrap_or("").trim();
        Ok(line.starts_with('/').then(|| line.to_string()))
    }

    /// Runs a command on a separate channel (no PTY) and returns its standard output.
    pub async fn exec_capture(&self, cmd: &str, timeout: Duration) -> AppResult<String> {
        let mut ch = self.handle.channel_open_session().await?;
        ch.exec(true, cmd).await?;
        let mut out = Vec::new();
        let read = async {
            while let Some(m) = ch.wait().await {
                match m {
                    ChannelMsg::Data { data } => {
                        if out.len() < 1 << 20 {
                            out.extend_from_slice(&data);
                        }
                    }
                    ChannelMsg::Eof | ChannelMsg::Close => break,
                    _ => {}
                }
            }
        };
        if tokio::time::timeout(timeout, read).await.is_err() {
            let _ = ch.close().await;
            return Err(AppError::Timeout);
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    /// Starts a port forwarding on this connection. Returns the listening port.
    pub async fn start_forward(&self, f: &Forward) -> AppResult<u16> {
        let port = match f.kind {
            ForwardKind::Local | ForwardKind::Dynamic => {
                if f.kind == ForwardKind::Local && (f.dest_host.trim().is_empty() || f.dest_port == 0) {
                    return Err(AppError::Config("destination du tunnel manquante".into()));
                }
                let listener = forward::bind(&f.bind_host, f.bind_port).await?;
                let port = listener.local_addr().map(|a| a.port()).unwrap_or(f.bind_port);
                let handle = self.handle.clone();
                let task = if f.kind == ForwardKind::Local {
                    tokio::spawn(forward::run_local(handle, listener, f.dest_host.trim().to_string(), f.dest_port))
                } else {
                    tokio::spawn(forward::run_dynamic(handle, listener))
                };
                self.tasks.lock().unwrap().push(task);
                port
            }
            ForwardKind::Remote => {
                if f.dest_host.trim().is_empty() || f.dest_port == 0 {
                    return Err(AppError::Config("destination du tunnel manquante".into()));
                }
                let bind = if f.bind_host.trim().is_empty() { "localhost" } else { f.bind_host.trim() };
                let got = self
                    .handle
                    .tcpip_forward(bind, f.bind_port as u32)
                    .await
                    .map_err(|e| AppError::Conn(format!("le serveur refuse d'écouter sur {bind}:{} : {e}", f.bind_port)))?;
                let port = if f.bind_port == 0 { got } else { f.bind_port as u32 };
                self.shared.remote.lock().unwrap().insert(port, (f.dest_host.trim().to_string(), f.dest_port));
                port as u16
            }
        };
        self.forwards.lock().unwrap().push(ActiveForward { forward: f.clone(), port });
        Ok(port)
    }

    pub fn active_forwards(&self) -> Vec<ActiveForward> {
        self.forwards.lock().unwrap().clone()
    }

    pub async fn disconnect(&self) {
        for t in self.tasks.lock().unwrap().drain(..) {
            t.abort();
        }
        if let Some(s) = self.sftp.lock().await.take() {
            let _ = s.close().await;
        }
        if let Some(w) = &self.writer {
            let _ = w.eof().await;
        }
        let _ = self.handle.disconnect(Disconnect::ByApplication, "", "fr").await;
        if let Some(j) = &self.jump {
            let _ = j.disconnect(Disconnect::ByApplication, "", "fr").await;
        }
    }
}

async fn authenticate(handle: &mut Handle<ClientHandler>, user: &str, creds: Credentials) -> AppResult<()> {
    match creds {
        Credentials::Password(password) => {
            if handle.authenticate_password(user, password.clone()).await?.success() {
                return Ok(());
            }
            // Many servers only enable keyboard-interactive: answer every hidden
            // prompt with the password (covers the classic "Password:" prompt).
            keyboard_interactive(handle, user, &password).await
        }
        Credentials::Key { path, passphrase } => {
            let key = match russh::keys::load_secret_key(&path, passphrase.as_deref()) {
                Ok(k) => k,
                Err(russh::keys::Error::KeyIsEncrypted) => return Err(AppError::NeedPassphrase),
                Err(e) => {
                    // A wrong passphrase also ends up here.
                    if passphrase.is_none() && e.to_string().to_lowercase().contains("encrypt") {
                        return Err(AppError::NeedPassphrase);
                    }
                    return Err(AppError::Ssh(format!("clé {}: {e}", path.display())));
                }
            };
            let hash = handle.best_supported_rsa_hash().await?.flatten();
            let res = handle
                .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
                .await?;
            if res.success() { Ok(()) } else { Err(AppError::AuthFailed) }
        }
    }
}

async fn keyboard_interactive(handle: &mut Handle<ClientHandler>, user: &str, password: &str) -> AppResult<()> {
    use russh::client::KeyboardInteractiveAuthResponse as R;
    let mut resp = handle.authenticate_keyboard_interactive_start(user, None).await?;
    for _ in 0..5 {
        match resp {
            R::Success => return Ok(()),
            R::Failure { .. } => return Err(AppError::AuthFailed),
            R::InfoRequest { prompts, .. } => {
                let answers = prompts
                    .iter()
                    .map(|p| if p.echo { String::new() } else { password.to_string() })
                    .collect();
                resp = handle.authenticate_keyboard_interactive_respond(answers).await?;
            }
        }
    }
    Err(AppError::AuthFailed)
}

/// Shell command printing the current folder of the process started with
/// LC_ORBITERM_ID=<tag> whose parent does not have it (the session's shell).
fn cwd_command(tag: &str) -> String {
    let tag: String = tag.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    format!(
        "p=$(grep -lsxz 'LC_ORBITERM_ID={tag}' /proc/[0-9]*/environ 2>/dev/null | cut -d/ -f3);          for i in $p; do pp=$(sed -e 's/.*) //' /proc/$i/stat 2>/dev/null | cut -d' ' -f2);          case \" $p \" in *\" $pp \"*) ;; *) readlink /proc/$i/cwd; exit 0;; esac; done"
    )
}
