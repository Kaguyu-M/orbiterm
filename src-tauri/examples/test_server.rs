//! Tiny SSH + SFTP server used to test Orbiterm without a real Linux machine.
//!
//!   cargo run --example test_server -- [port] [root_dir]
//!
//! - Login: user `test`, password `test` (password or keyboard-interactive)
//! - Shell: a toy line-based shell (commands: help, pwd, whoami, size, echo, colors, exit)
//! - SFTP: real files, confined to `root_dir` (default: a temp directory)
//!
//! It is ALSO compiled into the integration tests (tests/e2e.rs) through `#[path]`.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use orbiterm_lib::sftp_server::SftpServer;
use russh::server::{Auth, Msg, Session};
use russh::{Channel, ChannelId};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

pub const USER: &str = "test";
pub const PASSWORD: &str = "test";

/// Starts the server on 127.0.0.1:`port` (0 = random). Returns the bound port and the
/// server host public key.
pub async fn start(port: u16, root: PathBuf) -> (u16, russh::keys::PublicKey) {
    let key = russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519).unwrap();
    let public = key.public_key().clone();
    let config = Arc::new(russh::server::Config {
        auth_rejection_time: std::time::Duration::from_millis(200),
        auth_rejection_time_initial: Some(std::time::Duration::from_secs(0)),
        keys: vec![key],
        ..Default::default()
    });
    let listener = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { break };
            let handler = SshHandler::new(root.clone());
            let config = config.clone();
            tokio::spawn(async move {
                if let Ok(running) = russh::server::run_stream(config, stream, handler).await {
                    let _ = running.await;
                }
            });
        }
    });
    (port, public)
}

// ------------------------------------------------------------------ SSH side

struct SshHandler {
    root: PathBuf,
    channels: Arc<Mutex<HashMap<ChannelId, Channel<Msg>>>>,
    line: Vec<u8>,
    size: (u32, u32),
    shell: Option<ChannelId>,
    /// X11 cookie received with x11-req (hex)
    x11: Option<String>,
    /// agent forwarding requested by the client
    agent: bool,
}

impl SshHandler {
    fn new(root: PathBuf) -> Self {
        Self { root, channels: Default::default(), line: vec![], size: (80, 24), shell: None, x11: None, agent: false }
    }

    fn run_command(&mut self, cmd: &str) -> (String, bool) {
        let mut parts = cmd.trim().splitn(2, ' ');
        let name = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("");
        let out = match name {
            "" => String::new(),
            "help" => "commandes : help pwd whoami size echo colors xtest agenttest exit\r\n".into(),
            "pwd" => "/\r\n".into(),
            "whoami" => format!("{USER}\r\n"),
            "size" => format!("{}x{}\r\n", self.size.0, self.size.1),
            "echo" => format!("{arg}\r\n"),
            "colors" => (0..8).map(|i| format!("\x1b[4{i}m  \x1b[0m")).collect::<String>() + "\r\n",
            "exit" => return ("au revoir\r\n".into(), true),
            other => format!("{other}: commande inconnue\r\n"),
        };
        (out, false)
    }
}

impl russh::server::Handler for SshHandler {
    type Error = anyhow::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        Ok(if user == USER && password == PASSWORD { Auth::Accept } else { Auth::reject() })
    }

    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        user: &str,
        _submethods: &str,
        response: Option<russh::server::Response<'a>>,
    ) -> Result<Auth, Self::Error> {
        match response {
            None => Ok(Auth::Partial {
                name: "".into(),
                instructions: "".into(),
                prompts: vec![("Password: ".into(), false)].into(),
            }),
            Some(mut r) => {
                let ok = user == USER && r.next().map(|b| b.as_ref() == PASSWORD.as_bytes()).unwrap_or(false);
                Ok(if ok { Auth::Accept } else { Auth::reject() })
            }
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.channels.lock().await.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        col_width: u32,
        row_height: u32,
        _pw: u32,
        _ph: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.size = (col_width, row_height);
        session.channel_success(channel)?;
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        _channel: ChannelId,
        col_width: u32,
        row_height: u32,
        _pw: u32,
        _ph: u32,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.size = (col_width, row_height);
        Ok(())
    }

    async fn shell_request(&mut self, channel: ChannelId, session: &mut Session) -> Result<(), Self::Error> {
        self.shell = Some(channel);
        session.channel_success(channel)?;
        session.data(channel, b"Bienvenue sur le serveur de test Orbiterm (tape 'help')\r\n$ ".to_vec())?;
        Ok(())
    }

    async fn data(&mut self, channel: ChannelId, data: &[u8], session: &mut Session) -> Result<(), Self::Error> {
        // Only the interactive shell channel is handled here (SFTP data goes to its own task).
        if self.shell != Some(channel) {
            return Ok(());
        }
        for &b in data {
            match b {
                b'\r' | b'\n' => {
                    let cmd = String::from_utf8_lossy(&self.line).to_string();
                    self.line.clear();
                    if cmd.trim() == "agenttest" {
                        let handle = session.handle();
                        let agent = self.agent;
                        tokio::spawn(async move {
                            let msg = agent_test(&handle, agent).await;
                            let _ = handle.data(channel, format!("\r\n{msg}\r\n$ ").into_bytes()).await;
                        });
                        continue;
                    }
                    if cmd.trim() == "xtest" {
                        let handle = session.handle();
                        let cookie = self.x11.clone();
                        tokio::spawn(async move {
                            let msg = x11_test(&handle, cookie).await;
                            let _ = handle.data(channel, format!("\r\n{msg}\r\n$ ").into_bytes()).await;
                        });
                        continue;
                    }
                    let (out, quit) = self.run_command(&cmd);
                    session.data(channel, format!("\r\n{out}").into_bytes())?;
                    if quit {
                        session.exit_status_request(channel, 0)?;
                        session.eof(channel)?;
                        session.close(channel)?;
                        return Ok(());
                    }
                    session.data(channel, b"$ ".to_vec())?;
                }
                0x7f | 0x08 => {
                    if self.line.pop().is_some() {
                        session.data(channel, b"\x08 \x08".to_vec())?;
                    }
                }
                0x03 => {
                    self.line.clear();
                    session.data(channel, b"^C\r\n$ ".to_vec())?;
                }
                _ => {
                    self.line.push(b);
                    session.data(channel, vec![b])?;
                }
            }
        }
        Ok(())
    }

    async fn agent_request(&mut self, channel: ChannelId, session: &mut Session) -> Result<bool, Self::Error> {
        self.agent = true;
        session.channel_success(channel)?;
        Ok(true)
    }

    async fn x11_request(
        &mut self,
        channel: ChannelId,
        _single_connection: bool,
        _x11_auth_protocol: &str,
        x11_auth_cookie: &str,
        _x11_screen_number: u32,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.x11 = Some(x11_auth_cookie.to_string());
        session.channel_success(channel)?;
        Ok(())
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        match tokio::net::TcpStream::connect((host_to_connect, port_to_connect as u16)).await {
            Ok(mut tcp) => {
                reply.accept().await;
                tokio::spawn(async move {
                    let mut s = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut tcp, &mut s).await;
                });
            }
            Err(_) => reply.reject(russh::ChannelOpenFailure::ConnectFailed).await,
        }
        Ok(())
    }

    async fn tcpip_forward(&mut self, address: &str, port: &mut u32, session: &mut Session) -> Result<bool, Self::Error> {
        let Ok(listener) = TcpListener::bind(("127.0.0.1", *port as u16)).await else { return Ok(false) };
        let actual = listener.local_addr()?.port() as u32;
        if *port == 0 {
            *port = actual;
        }
        let handle = session.handle();
        let addr = address.to_string();
        tokio::spawn(async move {
            loop {
                let Ok((mut tcp, peer)) = listener.accept().await else { break };
                let Ok(ch) = handle
                    .channel_open_forwarded_tcpip(addr.clone(), actual, peer.ip().to_string(), peer.port() as u32)
                    .await
                else {
                    continue;
                };
                tokio::spawn(async move {
                    let mut s = ch.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut tcp, &mut s).await;
                });
            }
        });
        Ok(true)
    }

    async fn channel_eof(&mut self, channel: ChannelId, session: &mut Session) -> Result<(), Self::Error> {
        session.close(channel)?;
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel_id: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let channel = self.channels.lock().await.remove(&channel_id);
        match (name, channel) {
            ("sftp", Some(channel)) => {
                session.channel_success(channel_id)?;
                let sftp = SftpServer::new(self.root.clone(), true);
                russh_sftp::server::run(channel.into_stream(), sftp).await;
            }
            _ => session.channel_failure(channel_id)?,
        }
        Ok(())
    }
}

/// Opens an X11 channel to the client, sends an X connection setup carrying the
/// cookie, then "PING", and reports what the client's X server answered.
/// Lists the keys of the forwarded agent and has the first one sign a challenge.
async fn agent_test(handle: &russh::server::Handle, requested: bool) -> String {
    use russh::keys::agent::{AgentIdentity, client::AgentClient};
    if !requested {
        return "agenttest: pas de transfert d'agent".into();
    }
    let Ok(ch) = handle.channel_open_agent().await else { return "agenttest: canal d'agent refusé".into() };
    let mut client = AgentClient::connect(ch.into_stream());
    let ids = match client.request_identities().await {
        Ok(ids) => ids,
        Err(e) => return format!("agenttest: {e}"),
    };
    let comments: Vec<String> = ids
        .iter()
        .filter_map(|i| match i {
            AgentIdentity::PublicKey { comment, .. } => Some(comment.clone()),
            _ => None,
        })
        .collect();
    let signed = match ids.first() {
        Some(id @ AgentIdentity::PublicKey { key, .. }) => {
            let data = b"defi agenttest".to_vec();
            match client.sign_request(id, None, data.clone()).await {
                Ok(out) => {
                    let blob = &out[data.len() + 4..];
                    let ok = russh::keys::ssh_key::Signature::try_from(blob)
                        .ok()
                        .is_some_and(|sig| russh::keys::signature::Verifier::verify(key, &data, &sig).is_ok());
                    if ok { "signature valide" } else { "signature invalide" }
                }
                Err(_) => "signature refusée",
            }
        }
        _ => "aucune clé",
    };
    format!("agenttest: {} clé(s) [{}] {signed}", ids.len(), comments.join(", "))
}

async fn x11_test(handle: &russh::server::Handle, cookie: Option<String>) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let Some(cookie) = cookie else { return "xtest: pas de redirection X11".into() };
    let data: Vec<u8> =
        (0..cookie.len()).step_by(2).filter_map(|i| u8::from_str_radix(&cookie[i..i + 2], 16).ok()).collect();
    let Ok(ch) = handle.channel_open_x11("127.0.0.1", 0).await else { return "xtest: canal X11 refusé".into() };
    let name = b"MIT-MAGIC-COOKIE-1";
    let mut setup = vec![b'l', 0, 11, 0, 0, 0];
    setup.extend((name.len() as u16).to_le_bytes());
    setup.extend((data.len() as u16).to_le_bytes());
    setup.extend([0, 0]);
    setup.extend(name);
    setup.resize(12 + ((name.len() + 3) & !3), 0);
    setup.extend(&data);
    setup.resize(setup.len() + ((4 - data.len() % 4) % 4), 0);
    setup.extend(b"PING");
    let mut s = ch.into_stream();
    if s.write_all(&setup).await.is_err() {
        return "xtest: écriture impossible".into();
    }
    let mut buf = vec![0u8; 256];
    match tokio::time::timeout(std::time::Duration::from_secs(3), s.read(&mut buf)).await {
        Ok(Ok(n)) if n > 0 => format!("X11:{}", String::from_utf8_lossy(&buf[..n])),
        _ => "xtest: pas de réponse".into(),
    }
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let port: u16 = args.next().and_then(|p| p.parse().ok()).unwrap_or(2222);
    let root = args.next().map(PathBuf::from).unwrap_or_else(|| {
        let d = std::env::temp_dir().join("orbiterm-test-server");
        std::fs::create_dir_all(d.join("documents")).unwrap();
        std::fs::write(d.join("bienvenue.txt"), "Fichier de test Orbiterm\n").unwrap();
        d
    });
    std::fs::create_dir_all(&root).unwrap();
    let (port, key) = start(port, root.clone()).await;
    println!("Serveur de test SSH/SFTP prêt sur 127.0.0.1:{port}");
    println!("  utilisateur : {USER}   mot de passe : {PASSWORD}");
    println!("  dossier SFTP : {}", root.display());
    println!("  empreinte    : {}", key.fingerprint(Default::default()));
    println!("Ctrl+C pour arrêter.");
    tokio::signal::ctrl_c().await.unwrap();
}
