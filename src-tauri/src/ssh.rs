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
use russh::client::AuthResult;
use russh::{Channel, ChannelMsg, ChannelOpenFailure, ChannelWriteHalf, Disconnect, MethodKind, MethodSet};
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
/// Questions of a keyboard-interactive login the saved password cannot answer
/// (one-time code, PIN, Duo choice…), shown to the user.
#[derive(Debug, Clone, Serialize)]
pub struct AuthQuestion {
    pub host: String,
    pub user: String,
    /// title and explanation sent by the server (often empty)
    pub name: String,
    pub instructions: String,
    pub prompts: Vec<AuthPromptText>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthPromptText {
    pub text: String,
    /// the answer may be shown while typed (false: hidden, like a password)
    pub echo: bool,
}

/// Asks the user the server's questions: one answer per prompt, None when cancelled.
pub type AuthPrompter =
    Arc<dyn Fn(AuthQuestion) -> Pin<Box<dyn Future<Output = Option<Vec<String>>> + Send>> + Send + Sync>;
/// Receives terminal output bytes.
pub type DataSink = Arc<dyn Fn(Vec<u8>) + Send + Sync>;
/// Called once when the shell ends (reason text, see [`ended`]).
pub type CloseSink = Arc<dyn Fn(String) + Send + Sync>;

/// Marks a close reason as a normal end of session (the user quit: `exit`, logoff…), as
/// opposed to an error or a lost connection. The tab can then close by itself.
const ENDED: &str = "\u{1}";

pub fn ended(reason: impl Into<String>) -> String {
    format!("{ENDED}{}", reason.into())
}

/// Marks a close reason as a lost connection (network failure, the computer slept): the tab
/// may reconnect by itself, unlike a session the server ended on purpose.
const LOST: &str = "\u{2}";

pub fn lost(reason: impl Into<String>) -> String {
    format!("{LOST}{}", reason.into())
}

/// Splits a close reason into (normal end?, text).
pub fn split_reason(reason: String) -> (bool, String) {
    match reason.strip_prefix(ENDED) {
        Some(r) => (true, r.to_string()),
        None => (false, reason),
    }
}

/// Splits a close reason into (lost connection?, rest).
pub fn split_lost(reason: String) -> (bool, String) {
    match reason.strip_prefix(LOST) {
        Some(r) => (true, r.to_string()),
        None => (false, reason),
    }
}

pub enum Credentials {
    Password(String),
    Key { path: PathBuf, passphrase: Option<String> },
    /// only the keys of the SSH agents (no password known yet: NeedPassword when refused)
    Agent,
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
    /// try the keys of the SSH agents (internal + system) first
    pub agent: bool,
    /// forward the agent to the server
    pub agent_forward: bool,
    /// welcome box written before the server output (Some = shown; value = SFTP browser on)
    pub banner: Option<bool>,
    /// system summary (logo + facts, like fastfetch) written when the shell opens
    pub fetch: bool,
    /// asks the user the questions of a two-factor login (None: hidden prompts get the password)
    pub auth_prompter: Option<AuthPrompter>,
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
            agent: false,
            agent_forward: false,
            banner: None,
            fetch: false,
            auth_prompter: None,
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
    /// agent forwarding requested for this connection
    agent_forward: std::sync::atomic::AtomicBool,
}

pub struct ClientHandler {
    host: String,
    port: u16,
    known_hosts: PathBuf,
    prompter: HostKeyPrompter,
    /// Set when the user refused the key, to report a precise error.
    rejected: Arc<std::sync::atomic::AtomicBool>,
    shared: Arc<Shared>,
    /// pauses the handshake deadline while the user answers the host-key question
    waiting: Arc<Waiting>,
}

/// Time to reach the server (TCP connect, or the channel through a jump host).
const REACH_TIMEOUT: Duration = Duration::from_secs(15);
/// Silence allowed during the key exchange, not counting the time spent on a question.
const HANDSHAKE_IDLE: Duration = Duration::from_secs(30);
/// Silence allowed during authentication, not counting the time spent on a two-factor question.
const AUTH_TIMEOUT: Duration = Duration::from_secs(90);

/// Tracks whether a question is on screen and when the last one was answered.
pub struct Waiting {
    asking: std::sync::atomic::AtomicBool,
    since: std::sync::Mutex<std::time::Instant>,
}

impl Waiting {
    fn new() -> Self {
        Waiting { asking: false.into(), since: std::sync::Mutex::new(std::time::Instant::now()) }
    }
    fn expired(&self, limit: Duration) -> bool {
        !self.asking.load(std::sync::atomic::Ordering::SeqCst)
            && self.since.lock().unwrap_or_else(|e| e.into_inner()).elapsed() > limit
    }
}

/// Runs `fut` until it ends or stays silent for longer than `limit` (time spent on a question excluded).
async fn idle_deadline<F: std::future::Future>(fut: F, waiting: &Waiting, limit: Duration) -> Option<F::Output> {
    tokio::pin!(fut);
    loop {
        tokio::select! {
            r = &mut fut => return Some(r),
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                if waiting.expired(limit) {
                    return None;
                }
            }
        }
    }
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
        self.waiting.asking.store(true, std::sync::atomic::Ordering::SeqCst);
        let accepted = (self.prompter)(question).await;
        *self.waiting.since.lock().unwrap_or_else(|e| e.into_inner()) = std::time::Instant::now();
        self.waiting.asking.store(false, std::sync::atomic::Ordering::SeqCst);
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

    async fn server_channel_open_agent_forward(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        if self.shared.agent_forward.load(std::sync::atomic::Ordering::Relaxed) {
            reply.accept().await;
            tokio::spawn(crate::agent::serve(channel.into_stream()));
        } else {
            reply.reject(ChannelOpenFailure::AdministrativelyProhibited).await;
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
    /// the server refused the SFTP subsystem: the file browser uses shell commands + SCP
    no_sftp: std::sync::atomic::AtomicBool,
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
    let waiting = Arc::new(Waiting::new());
    let handler = ClientHandler {
        host: host.to_string(),
        port,
        known_hosts: known_hosts.to_path_buf(),
        prompter,
        rejected: rejected.clone(),
        shared,
        waiting: waiting.clone(),
    };
    // Reaching the server and the key exchange each get a short deadline; the time the
    // user spends on the host-key question is not counted.
    let unreachable = || AppError::Conn(format!("{host}:{port} ne répond pas"));
    let res = match via {
        None => {
            let tcp = tokio::time::timeout(REACH_TIMEOUT, tokio::net::TcpStream::connect((host, port)))
                .await
                .map_err(|_| unreachable())?
                .map_err(|e| AppError::Conn(format!("{host}:{port} : {e}")))?;
            let _ = tcp.set_nodelay(true);
            *waiting.since.lock().unwrap_or_else(|e| e.into_inner()) = std::time::Instant::now();
            idle_deadline(client::connect_stream(config, tcp, handler), &waiting, HANDSHAKE_IDLE).await
        }
        Some(jump) => {
            let ch = tokio::time::timeout(REACH_TIMEOUT, jump.channel_open_direct_tcpip(host, port as u32, "127.0.0.1", 0))
                .await
                .map_err(|_| AppError::Conn(format!("le rebond SSH ne joint pas {host}:{port} (délai dépassé)")))?
                .map_err(|e| AppError::Conn(format!("le rebond SSH n'a pas pu joindre {host}:{port} : {e}")))?;
            *waiting.since.lock().unwrap_or_else(|e| e.into_inner()) = std::time::Instant::now();
            idle_deadline(client::connect_stream(config, ch.into_stream(), handler), &waiting, HANDSHAKE_IDLE).await
        }
    };
    match res {
        None => Err(AppError::Conn(format!("{host}:{port} : le serveur SSH ne termine pas l'échange de clés"))),
        Some(Err(e)) => {
            if rejected.load(std::sync::atomic::Ordering::SeqCst) {
                Err(AppError::HostKeyRejected)
            } else {
                Err(e.into())
            }
        }
        Some(Ok(h)) => Ok(h),
    }
}

/// Runs a command on a separate channel (no PTY) of `handle` and returns its standard output.
async fn capture_on(handle: &Handle<ClientHandler>, cmd: &str, timeout: Duration) -> AppResult<String> {
    let mut ch = handle.channel_open_session().await?;
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

/// The fastfetch-like summary of a Unix-like server, or None (Windows servers: their console
/// clears the screen when the shell starts, the summary would be wiped at once).
async fn system_summary(handle: &Handle<ClientHandler>, cols: u32) -> Option<String> {
    let out = capture_on(handle, &crate::sysfetch::unix_command(), Duration::from_secs(4)).await.ok()?;
    Some(crate::sysfetch::render(&crate::sysfetch::parse(&out)?, cols))
}

impl SshConnection {
    pub async fn connect(
        p: ConnectParams,
        prompter: HostKeyPrompter,
        on_data: DataSink,
        on_close: CloseSink,
    ) -> AppResult<Arc<SshConnection>> {
        let config = client_config(p.compression, p.keepalive);
        let banner_info = p.banner.map(|sftp_browser| crate::banner::BannerInfo {
            user: p.username.clone(),
            host: p.host.clone(),
            port: p.port,
            jump: p.jump.as_ref().map(|j| match j.port {
                22 => format!("{}@{}", j.username, j.host),
                port => format!("{}@{}:{port}", j.username, j.host),
            }),
            compression: p.compression,
            sftp_browser,
            agent_forward: p.agent_forward,
        });
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
                match authenticate(&mut jh, &j.host, &j.username, j.credentials, p.agent, p.auth_prompter.clone()).await {
                    Ok(()) => {}
                    Err(AppError::AuthFailed) => return Err(AppError::JumpAuthFailed),
                    Err(AppError::NeedPassword) => return Err(AppError::NeedJumpPassword),
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
        authenticate(&mut handle, &p.host, &p.username, p.credentials, p.agent, p.auth_prompter).await?;

        // system summary, gathered on its own channel before the shell starts; written
        // under the welcome box, before anything the shell prints
        let mut summary = match p.mode {
            ShellMode::Shell if p.fetch => system_summary(&handle, p.cols).await,
            _ => None,
        };

        let writer = match p.mode {
            ShellMode::None => None,
            mode => {
                let channel = handle.channel_open_session().await?;
                if p.agent_forward {
                    shared.agent_forward.store(true, std::sync::atomic::Ordering::Relaxed);
                    let _ = channel.agent_forward(false).await;
                }
                use crate::banner::X11State;
                let mut x11_state = X11State::Off;
                if let Some(display) = &p.x11_display {
                    if let Some((target, screen)) = x11::parse_display(display) {
                        let (fake, fake_hex) = x11::fake_cookie();
                        let real = x11::real_cookie(display);
                        *shared.x11.lock().unwrap() = Some(X11Fwd { target, fake, real });
                        channel.request_x11(true, false, x11::COOKIE_PROTO, fake_hex, screen).await?;
                        // the server's answer comes back first on the channel (see below)
                        x11_state = X11State::Accepted;
                    } else {
                        log::warn!("X11: invalid display {display}");
                        x11_state = X11State::BadDisplay;
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
                    // The X11 request is the first one with a reply: its success / failure
                    // arrives before any output, then the box can say whether X11 works.
                    let mut x11_wait = x11_state == X11State::Accepted;
                    let mut banner = banner_info;
                    if !x11_wait {
                        if let Some(b) = banner.take() {
                            on_data(crate::banner::render(&b, x11_state).into_bytes());
                        }
                        if let Some(t) = summary.take() {
                            on_data(t.into_bytes());
                        }
                    }
                    loop {
                        let msg = reader.wait().await;
                        if x11_wait {
                            match msg {
                                Some(ChannelMsg::Success | ChannelMsg::Failure) => {
                                    x11_wait = false;
                                    if let Some(b) = banner.take() {
                                        let ok = matches!(msg, Some(ChannelMsg::Success));
                                        let st = if ok { X11State::Accepted } else { X11State::Refused };
                                        on_data(crate::banner::render(&b, st).into_bytes());
                                    }
                                    if let Some(t) = summary.take() {
                                        on_data(t.into_bytes());
                                    }
                                    continue;
                                }
                                // output before the answer (unexpected): no box in the middle of it
                                Some(ChannelMsg::Data { .. } | ChannelMsg::ExtendedData { .. }) | None => {
                                    x11_wait = false;
                                    banner = None;
                                    summary = None;
                                }
                                _ => {}
                            }
                        }
                        match msg {
                            Some(ChannelMsg::Data { data }) => on_data(data.to_vec()),
                            Some(ChannelMsg::ExtendedData { data, .. }) => on_data(data.to_vec()),
                            Some(ChannelMsg::ExitStatus { exit_status }) => exit = Some(exit_status),
                            Some(ChannelMsg::Close) | None => break,
                            _ => {}
                        }
                    }
                    on_close(match exit {
                        // the remote shell ended by itself (exit, logout)
                        Some(code) => ended(format!("Session terminée (code {code})")),
                        None => lost("Connexion fermée"),
                    });
                });
                Some(writer)
            }
        };

        Ok(Arc::new(SshConnection {
            handle: Arc::new(handle),
            writer,
            sftp: Mutex::new(None),
            no_sftp: Default::default(),
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
        self.sftp_opt()
            .await?
            .ok_or_else(|| AppError::Unsupported("le serveur ne propose pas SFTP (seul SCP est disponible)".into()))
    }

    /// SFTP session, or None when the server has no SFTP subsystem (remembered).
    pub async fn sftp_opt(&self) -> AppResult<Option<Arc<SftpSession>>> {
        use std::sync::atomic::Ordering;
        let mut guard = self.sftp.lock().await;
        if let Some(s) = guard.as_ref() {
            return Ok(Some(s.clone()));
        }
        if self.no_sftp.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let mut ch = self.handle.channel_open_session().await?;
        ch.request_subsystem(true, "sftp").await?;
        // wait for the answer: refused = no SFTP on this server (dropbear, appliances…)
        loop {
            match tokio::time::timeout(Duration::from_secs(10), ch.wait()).await {
                Ok(Some(ChannelMsg::Success)) => break,
                Ok(Some(ChannelMsg::Failure)) | Ok(None) | Ok(Some(ChannelMsg::Close)) | Ok(Some(ChannelMsg::Eof)) => {
                    self.no_sftp.store(true, Ordering::Relaxed);
                    return Ok(None);
                }
                Ok(Some(ChannelMsg::ExitStatus { .. })) => {
                    self.no_sftp.store(true, Ordering::Relaxed);
                    return Ok(None);
                }
                Ok(Some(_)) => continue,
                Err(_) => break, // no answer: try anyway
            }
        }
        match tokio::time::timeout(Duration::from_secs(15), SftpSession::new(ch.into_stream())).await {
            Ok(Ok(s)) => {
                let s = Arc::new(s);
                *guard = Some(s.clone());
                Ok(Some(s))
            }
            // accepted but no sftp-server program behind it
            Ok(Err(_)) | Err(_) => {
                self.no_sftp.store(true, Ordering::Relaxed);
                Ok(None)
            }
        }
    }

    /// Opens a channel running `cmd` (no PTY), for callers that need stdin / exit status.
    pub async fn exec_channel(&self, cmd: &str) -> AppResult<Channel<Msg>> {
        let ch = self.handle.channel_open_session().await?;
        ch.exec(true, cmd).await?;
        Ok(ch)
    }

    /// Current folder of the interactive shell (Linux servers: /proc), or None.
    pub async fn shell_cwd(&self) -> AppResult<Option<String>> {
        let out = self.exec_capture(&cwd_command(&self.shell_tag), Duration::from_secs(5)).await?;
        let line = out.lines().next().unwrap_or("").trim();
        Ok(line.starts_with('/').then(|| line.to_string()))
    }

    /// Runs a command on a separate channel (no PTY) and returns its standard output.
    pub async fn exec_capture(&self, cmd: &str, timeout: Duration) -> AppResult<String> {
        capture_on(&self.handle, cmd, timeout).await
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

/// Who asks the user, and the deadline paused while they answer.
struct Asker<'a> {
    prompter: Option<AuthPrompter>,
    host: &'a str,
    user: &'a str,
    waiting: &'a Waiting,
}

async fn authenticate(
    handle: &mut Handle<ClientHandler>,
    host: &str,
    user: &str,
    creds: Credentials,
    agent: bool,
    prompter: Option<AuthPrompter>,
) -> AppResult<()> {
    let waiting = Waiting::new();
    let ask = Asker { prompter, host, user, waiting: &waiting };
    idle_deadline(authenticate_inner(handle, user, creds, agent, &ask), &waiting, AUTH_TIMEOUT)
        .await
        .ok_or_else(|| AppError::Conn("le serveur SSH ne répond plus pendant l'authentification".into()))?
}

async fn authenticate_inner(
    handle: &mut Handle<ClientHandler>,
    user: &str,
    creds: Credentials,
    agent: bool,
    ask: &Asker<'_>,
) -> AppResult<()> {
    if agent {
        match crate::agent::authenticate(handle, user).await? {
            AuthResult::Success => return Ok(()),
            AuthResult::Failure { remaining_methods, partial_success: true } => {
                return second_factor(handle, user, &remaining_methods, None, ask).await;
            }
            AuthResult::Failure { .. } => {}
        }
    }
    match creds {
        Credentials::Agent => Err(AppError::NeedPassword),
        Credentials::Password(password) => {
            match handle.authenticate_password(user, password.clone()).await? {
                AuthResult::Success => return Ok(()),
                AuthResult::Failure { remaining_methods, partial_success: true } => {
                    return second_factor(handle, user, &remaining_methods, Some(&password), ask).await;
                }
                AuthResult::Failure { .. } => {}
            }
            // Many servers only enable keyboard-interactive: the password answers the
            // "Password:" prompt, other questions (one-time code…) go to the user.
            keyboard_interactive(handle, user, Some(&password), ask).await
        }
        Credentials::Key { path, passphrase } => {
            // OpenSSH, PEM or PuTTY .ppk
            let key = match crate::keygen::load(&path, passphrase.as_deref()) {
                Ok(k) => k,
                Err(AppError::NeedPassphrase) => return Err(AppError::NeedPassphrase),
                Err(e) => {
                    if passphrase.is_none() && e.to_string().to_lowercase().contains("encrypt") {
                        return Err(AppError::NeedPassphrase);
                    }
                    return Err(AppError::Ssh(format!("clé {} : {}", path.display(), e.to_string().trim_start_matches("Configuration : "))));
                }
            };
            let hash = handle.best_supported_rsa_hash().await?.flatten();
            match handle.authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash)).await? {
                AuthResult::Success => Ok(()),
                AuthResult::Failure { remaining_methods, partial_success: true } => {
                    second_factor(handle, user, &remaining_methods, None, ask).await
                }
                AuthResult::Failure { .. } => Err(AppError::AuthFailed),
            }
        }
    }
}

/// The first method was accepted but the server wants another one (sshd
/// `AuthenticationMethods publickey,keyboard-interactive`…).
async fn second_factor(
    handle: &mut Handle<ClientHandler>,
    user: &str,
    remaining: &MethodSet,
    password: Option<&str>,
    ask: &Asker<'_>,
) -> AppResult<()> {
    if remaining.contains(&MethodKind::KeyboardInteractive) {
        return keyboard_interactive(handle, user, password, ask).await;
    }
    if let Some(password) = password
        && remaining.contains(&MethodKind::Password)
        && handle.authenticate_password(user, password).await?.success()
    {
        return Ok(());
    }
    let names: Vec<&str> = remaining.iter().map(<&str>::from).collect();
    Err(AppError::Ssh(format!("le serveur exige une seconde méthode d'authentification non prise en charge ({})", names.join(", "))))
}

/// A hidden prompt asking for the account password (answered with the saved one).
fn is_password_prompt(text: &str) -> bool {
    let t = text.to_lowercase();
    ["password", "mot de passe", "passwort", "contraseña", "senha", "wachtwoord", "parola"].iter().any(|w| t.contains(w))
        && !["one-time", "one time", "otp", "verification", "token", "code", "pin"].iter().any(|w| t.contains(w))
}

/// Which prompts the saved password answers (None: one of them asks for it a second
/// time, i.e. the password was refused).
fn auto_answers(prompts: &[(String, bool)], password: Option<&str>, password_used: bool) -> Option<Vec<Option<String>>> {
    let mut out = Vec::with_capacity(prompts.len());
    for (text, echo) in prompts {
        match password {
            Some(pw) if !echo && is_password_prompt(text) => {
                if password_used {
                    return None;
                }
                out.push(Some(pw.to_string()));
            }
            _ => out.push(None),
        }
    }
    Some(out)
}

async fn keyboard_interactive(
    handle: &mut Handle<ClientHandler>,
    user: &str,
    password: Option<&str>,
    ask: &Asker<'_>,
) -> AppResult<()> {
    use russh::client::KeyboardInteractiveAuthResponse as R;
    let mut resp = handle.authenticate_keyboard_interactive_start(user, None).await?;
    let mut password_used = false;
    for _ in 0..8 {
        let (name, instructions, prompts) = match resp {
            R::Success => return Ok(()),
            R::Failure { .. } => return Err(AppError::AuthFailed),
            R::InfoRequest { name, instructions, prompts } => (name, instructions, prompts),
        };
        let prompts: Vec<(String, bool)> = prompts.into_iter().map(|p| (p.prompt, p.echo)).collect();
        let Some(prompter) = ask.prompter.as_ref() else {
            // no one to ask: every hidden prompt gets the password (previous behaviour)
            let answers =
                prompts.iter().map(|(_, echo)| if *echo { String::new() } else { password.unwrap_or_default().to_string() }).collect();
            resp = handle.authenticate_keyboard_interactive_respond(answers).await?;
            continue;
        };
        let Some(auto) = auto_answers(&prompts, password, password_used) else {
            return Err(AppError::AuthFailed);
        };
        password_used |= auto.iter().any(Option::is_some);
        let open: Vec<AuthPromptText> = prompts
            .iter()
            .zip(&auto)
            .filter(|(_, a)| a.is_none())
            .map(|((text, echo), _)| AuthPromptText { text: text.trim().to_string(), echo: *echo })
            .collect();
        let mut typed = if open.is_empty() {
            Vec::new().into_iter()
        } else {
            let question = AuthQuestion {
                host: ask.host.to_string(),
                user: ask.user.to_string(),
                name: name.trim().to_string(),
                instructions: instructions.trim().to_string(),
                prompts: open,
            };
            ask.waiting.asking.store(true, std::sync::atomic::Ordering::SeqCst);
            let answer = prompter(question).await;
            *ask.waiting.since.lock().unwrap_or_else(|e| e.into_inner()) = std::time::Instant::now();
            ask.waiting.asking.store(false, std::sync::atomic::Ordering::SeqCst);
            match answer {
                Some(a) => a.into_iter(),
                None => return Err(AppError::Conn("authentification annulée".into())),
            }
        };
        let answers = auto.into_iter().map(|a| a.unwrap_or_else(|| typed.next().unwrap_or_default())).collect();
        resp = handle.authenticate_keyboard_interactive_respond(answers).await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_password_answers_password_prompts_only() {
        assert!(is_password_prompt("Password: "));
        assert!(is_password_prompt("claude@host's password:"));
        assert!(is_password_prompt("Mot de passe : "));
        assert!(!is_password_prompt("Verification code: "));
        assert!(!is_password_prompt("One-time password (OATH) for `claude':"));
        assert!(!is_password_prompt("Passcode or option (1-3): "));
        assert!(!is_password_prompt("Enter PIN for token:"));
        let p = |t: &str, echo| (t.to_string(), echo);
        // password then code in one request: only the code is left to the user
        assert_eq!(
            auto_answers(&[p("Password: ", false), p("Verification code: ", false)], Some("pw"), false),
            Some(vec![Some("pw".into()), None])
        );
        // no saved password (key login with a second factor): everything is asked
        assert_eq!(auto_answers(&[p("Password: ", false)], None, false), Some(vec![None]));
        // asked again for the password: it was refused
        assert_eq!(auto_answers(&[p("Password: ", false)], Some("pw"), true), None);
        // a visible prompt is never answered with the password
        assert_eq!(auto_answers(&[p("Password hint: ", true)], Some("pw"), false), Some(vec![None]));
    }

    #[test]
    fn ended_reasons_round_trip() {
        assert_eq!(split_reason(ended("Session terminée (code 0)")), (true, "Session terminée (code 0)".into()));
        assert_eq!(split_reason("Connexion fermée".into()), (false, "Connexion fermée".into()));
        assert_eq!(split_lost(lost("Connexion fermée")), (true, "Connexion fermée".into()));
        assert_eq!(split_lost("Connexion fermée".into()), (false, "Connexion fermée".into()));
    }

    #[tokio::test]
    async fn idle_deadline_pauses_during_a_question() {
        let limit = Duration::from_millis(300);
        // silent peer: gives up after the limit
        let w = Waiting::new();
        let r = idle_deadline(std::future::pending::<()>(), &w, limit).await;
        assert!(r.is_none());
        // a question on screen for longer than the limit does not count
        let w = Arc::new(Waiting::new());
        w.asking.store(true, std::sync::atomic::Ordering::SeqCst);
        let w2 = w.clone();
        let answered = async move {
            tokio::time::sleep(Duration::from_millis(900)).await;
            *w2.since.lock().unwrap() = std::time::Instant::now();
            w2.asking.store(false, std::sync::atomic::Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(100)).await;
            7
        };
        assert_eq!(idle_deadline(answered, &w, limit).await, Some(7));
    }

    #[tokio::test]
    async fn silent_server_fails_instead_of_waiting() {
        // a server that accepts the TCP connection but never speaks SSH
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let mut keep = Vec::new();
            while let Ok((s, _)) = listener.accept().await {
                keep.push(s);
            }
        });
        let prompter: HostKeyPrompter = Arc::new(|_| Box::pin(async { true }));
        let dir = tempfile::tempdir().unwrap();
        let t0 = std::time::Instant::now();
        let r = open_transport(
            client_config(false, 0),
            "127.0.0.1",
            port,
            &dir.path().join("kh"),
            prompter,
            Arc::new(Shared::default()),
            None,
        )
        .await;
        let e = r.err().expect("must fail");
        assert!(e.to_string().contains("échange de clés"), "{e}");
        assert!(t0.elapsed() < HANDSHAKE_IDLE + Duration::from_secs(5));
    }
}
