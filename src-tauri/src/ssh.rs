//! SSH connection: authentication, interactive shell channel, lazy SFTP channel.
//!
//! One `SshConnection` = one TCP connection, multiplexed:
//!   - channel #1: interactive shell with a PTY (the terminal tab)
//!   - channel #2: SFTP subsystem, opened on first use (the file browser)
//!
//! This module does not depend on Tauri: output and host-key questions go through
//! callbacks, so it can be tested with a plain SSH server.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use russh::client::{self, Handle, Msg};
use russh::keys::{PrivateKeyWithHashAlg, PublicKeyOrCertificate, ssh_key::PublicKey};
use russh::{ChannelMsg, ChannelWriteHalf, Disconnect};
use russh_sftp::client::SftpSession;
use serde::Serialize;
use tokio::sync::Mutex;

use crate::error::{AppError, AppResult};
use crate::known_hosts::{self, HostKeyStatus};

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

pub struct ConnectParams {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub credentials: Credentials,
    pub cols: u32,
    pub rows: u32,
    pub known_hosts: PathBuf,
}

struct ClientHandler {
    host: String,
    port: u16,
    known_hosts: PathBuf,
    prompter: HostKeyPrompter,
    /// Set when the user refused the key, to report a precise error.
    rejected: Arc<std::sync::atomic::AtomicBool>,
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
}

pub struct SshConnection {
    handle: Handle<ClientHandler>,
    writer: ChannelWriteHalf<Msg>,
    sftp: Mutex<Option<Arc<SftpSession>>>,
}

impl SshConnection {
    pub async fn connect(
        p: ConnectParams,
        prompter: HostKeyPrompter,
        on_data: DataSink,
        on_close: CloseSink,
    ) -> AppResult<Arc<SshConnection>> {
        let config = Arc::new(client::Config {
            keepalive_interval: Some(Duration::from_secs(30)),
            keepalive_max: 3,
            ..Default::default()
        });
        let rejected = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handler = ClientHandler {
            host: p.host.clone(),
            port: p.port,
            known_hosts: p.known_hosts.clone(),
            prompter,
            rejected: rejected.clone(),
        };

        // The TCP connect + key exchange gets a deadline; the host-key prompt may
        // wait for the user, so the deadline is generous.
        let connect = client::connect(config, (p.host.as_str(), p.port), handler);
        let mut handle = match tokio::time::timeout(Duration::from_secs(300), connect).await {
            Err(_) => return Err(AppError::Timeout),
            Ok(Err(e)) => {
                if rejected.load(std::sync::atomic::Ordering::SeqCst) {
                    return Err(AppError::HostKeyRejected);
                }
                return Err(e.into());
            }
            Ok(Ok(h)) => h,
        };

        authenticate(&mut handle, &p.username, p.credentials).await?;

        let channel = handle.channel_open_session().await?;
        channel.request_pty(true, "xterm-256color", p.cols.max(1), p.rows.max(1), 0, 0, &[]).await?;
        channel.request_shell(true).await?;
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

        Ok(Arc::new(SshConnection { handle, writer, sftp: Mutex::new(None) }))
    }

    pub async fn write(&self, data: &[u8]) -> AppResult<()> {
        self.writer.data(data).await.map_err(Into::into)
    }

    pub async fn resize(&self, cols: u32, rows: u32) -> AppResult<()> {
        self.writer.window_change(cols.max(1), rows.max(1), 0, 0).await.map_err(Into::into)
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

    pub async fn disconnect(&self) {
        if let Some(s) = self.sftp.lock().await.take() {
            let _ = s.close().await;
        }
        let _ = self.writer.eof().await;
        let _ = self.handle.disconnect(Disconnect::ByApplication, "", "fr").await;
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
