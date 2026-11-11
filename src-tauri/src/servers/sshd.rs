//! SSH server (russh): password login for one user, interactive shell in a
//! pseudo-terminal (local shell of this computer), remote commands (exec), SFTP confined
//! to the shared folder, and local port forwarding (direct-tcpip).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use russh::keys::{Algorithm, PrivateKey, ssh_key::LineEnding};
use russh::server::{Auth, Handle, Msg, Session};
use russh::{Channel, ChannelId};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio::task::{JoinHandle, JoinSet};

use super::{ClientGuard, Ctx};
use crate::pty::{PtyConnection, PtySpawn, local_shells};
use crate::sftp_server::SftpServer;

const KEY_FILE: &str = "ssh_server_ed25519";

/// Loads the server host key, or creates it on first use (kept so clients see a stable key).
pub fn host_key(dir: &Path) -> std::io::Result<PrivateKey> {
    let path = dir.join(KEY_FILE);
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(k) = PrivateKey::from_openssh(&text)
    {
        return Ok(k);
    }
    let k = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).map_err(std::io::Error::other)?;
    let text = k.to_openssh(LineEnding::LF).map_err(std::io::Error::other)?;
    std::fs::write(&path, text.as_bytes())?;
    Ok(k)
}

pub async fn start(addr: SocketAddr, ctx: Ctx, stop: oneshot::Receiver<()>) -> std::io::Result<(JoinHandle<()>, u16)> {
    let key = host_key(&ctx.config_dir)?;
    ctx.log.log(format!("Empreinte de la clé d'hôte : {}", key.public_key().fingerprint(Default::default())));
    let config = Arc::new(russh::server::Config {
        auth_rejection_time: Duration::from_secs(1),
        auth_rejection_time_initial: Some(Duration::from_secs(0)),
        inactivity_timeout: None,
        keys: vec![key],
        ..Default::default()
    });
    let listener = TcpListener::bind(addr).await?;
    let port = listener.local_addr()?.port();
    let ctx = Arc::new(ctx);
    let task = tokio::spawn(async move {
        let mut conns = JoinSet::new();
        let accept = async {
            loop {
                let Ok((stream, peer)) = listener.accept().await else { break };
                let handler = Sshd::new(ctx.clone(), peer);
                let config = config.clone();
                conns.spawn(async move {
                    if let Ok(running) = russh::server::run_stream(config, stream, handler).await {
                        let _ = running.await;
                    }
                });
                while conns.try_join_next().is_some() {}
            }
        };
        tokio::select! {
            _ = stop => {}
            _ = accept => {}
        }
    });
    Ok((task, port))
}

/// Program and arguments of the local shell `id` ("" or unknown = the first one).
pub(super) fn configured_shell(id: &str) -> (String, Vec<String>) {
    let shells = local_shells();
    let sh = shells.iter().find(|s| s.id == id).or_else(|| shells.first());
    match sh {
        Some(s) => (s.program.clone(), s.args.clone()),
        None => (if cfg!(windows) { "cmd.exe".into() } else { "/bin/sh".into() }, vec![]),
    }
}

struct Sshd {
    ctx: Arc<Ctx>,
    peer: SocketAddr,
    channels: HashMap<ChannelId, Channel<Msg>>,
    sizes: HashMap<ChannelId, (u32, u32)>,
    ptys: HashMap<ChannelId, Arc<PtyConnection>>,
    stdins: HashMap<ChannelId, tokio::process::ChildStdin>,
    /// command / forwarding tasks of this client (aborted when it disconnects)
    tasks: JoinSet<()>,
    user: Option<String>,
    _guard: ClientGuard,
}

impl Drop for Sshd {
    fn drop(&mut self) {
        for p in self.ptys.values() {
            p.kill();
        }
        if let Some(u) = &self.user {
            self.ctx.log.log(format!("{} ({u}) déconnecté", self.peer.ip()));
        }
    }
}

enum Out {
    Data(Vec<u8>),
    Exit(u32),
}

impl Sshd {
    fn new(ctx: Arc<Ctx>, peer: SocketAddr) -> Self {
        let _guard = ctx.log.client();
        Self {
            ctx,
            peer,
            channels: HashMap::new(),
            sizes: HashMap::new(),
            ptys: HashMap::new(),
            stdins: HashMap::new(),
            tasks: JoinSet::new(),
            user: None,
            _guard,
        }
    }

    fn check(&mut self, user: &str, password: &str) -> Auth {
        let ok = !self.ctx.password.is_empty() && user == self.ctx.cfg.user && password == self.ctx.password;
        if ok {
            self.user = Some(user.to_string());
            self.ctx.log.log(format!("{} connecté en tant que {user}", self.peer.ip()));
            Auth::Accept
        } else {
            self.ctx.log.log(format!("{} connexion refusée pour « {user} »", self.peer.ip()));
            Auth::reject()
        }
    }

    /// The local shell configured for clients (program, args).
    fn shell(&self) -> (String, Vec<String>) {
        configured_shell(&self.ctx.cfg.shell)
    }

    /// Arguments that make the configured shell run one command.
    fn command_args(&self, cmd: &str) -> (String, Vec<String>) {
        let (program, _) = self.shell();
        let name = Path::new(&program).file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
        let args = if name.contains("powershell") || name == "pwsh" {
            vec!["-NoLogo".into(), "-NonInteractive".into(), "-Command".into(), cmd.into()]
        } else if name == "cmd" {
            vec!["/C".into(), cmd.into()]
        } else if name == "wsl" {
            vec!["-e".into(), "sh".into(), "-c".into(), cmd.into()]
        } else {
            vec!["-c".into(), cmd.into()]
        };
        (program, args)
    }

    /// Starts `program` in a pseudo-terminal bound to `channel`.
    fn spawn_pty(&mut self, channel: ChannelId, program: String, args: Vec<String>, session: &mut Session) -> bool {
        let (cols, rows) = self.sizes.get(&channel).copied().unwrap_or((80, 24));
        let (tx, rx) = mpsc::unbounded_channel::<Out>();
        let tx2 = tx.clone();
        let spec = PtySpawn {
            program,
            args,
            cwd: Some(self.ctx.root.clone()),
            env: vec![],
            cols: cols as u16,
            rows: rows as u16,
        };
        let pty = match PtyConnection::spawn(
            spec,
            Arc::new(move |d| {
                let _ = tx.send(Out::Data(d));
            }),
            Arc::new(move |reason: String| {
                let code = reason.rsplit("code ").next().and_then(|c| c.trim_end_matches(')').parse().ok()).unwrap_or(0);
                let _ = tx2.send(Out::Exit(code));
            }),
        ) {
            Ok(p) => p,
            Err(e) => {
                self.ctx.log.log(format!("shell impossible : {e}"));
                return false;
            }
        };
        self.ptys.insert(channel, pty);
        let handle = session.handle();
        self.tasks.spawn(pump(handle, channel, rx));
        true
    }
}

/// Forwards process output to the channel, in order, then reports the exit status.
async fn pump(handle: Handle, channel: ChannelId, mut rx: mpsc::UnboundedReceiver<Out>) {
    while let Some(m) = rx.recv().await {
        match m {
            Out::Data(d) => {
                if handle.data(channel, d).await.is_err() {
                    break;
                }
            }
            Out::Exit(code) => {
                // drain what the reader thread may still deliver
                while let Ok(Out::Data(d)) = rx.try_recv() {
                    let _ = handle.data(channel, d).await;
                }
                let _ = handle.exit_status_request(channel, code).await;
                let _ = handle.eof(channel).await;
                let _ = handle.close(channel).await;
                break;
            }
        }
    }
}

impl russh::server::Handler for Sshd {
    type Error = anyhow::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        Ok(self.check(user, password))
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
                prompts: vec![("Mot de passe : ".into(), false)].into(),
            }),
            Some(mut r) => {
                let pw = r.next().map(|b| String::from_utf8_lossy(b.as_ref()).into_owned()).unwrap_or_default();
                Ok(self.check(user, &pw))
            }
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.channels.insert(channel.id(), channel);
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
        self.sizes.insert(channel, (col_width.max(1), row_height.max(1)));
        session.channel_success(channel)?;
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        channel: ChannelId,
        col_width: u32,
        row_height: u32,
        _pw: u32,
        _ph: u32,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.sizes.insert(channel, (col_width, row_height));
        if let Some(p) = self.ptys.get(&channel) {
            let _ = p.resize(col_width, row_height);
        }
        Ok(())
    }

    async fn shell_request(&mut self, channel: ChannelId, session: &mut Session) -> Result<(), Self::Error> {
        let (program, args) = self.shell();
        self.ctx.log.log(format!("{} ouvre un shell ({})", self.peer.ip(), Path::new(&program).file_name().unwrap_or_default().to_string_lossy()));
        if self.spawn_pty(channel, program, args, session) {
            session.channel_success(channel)?;
        } else {
            session.channel_failure(channel)?;
        }
        Ok(())
    }

    async fn exec_request(&mut self, channel: ChannelId, data: &[u8], session: &mut Session) -> Result<(), Self::Error> {
        let cmd = String::from_utf8_lossy(data).into_owned();
        self.ctx.log.log(format!("{} exécute : {cmd}", self.peer.ip()));
        let (program, args) = self.command_args(&cmd);
        if self.sizes.contains_key(&channel) {
            // a terminal was requested: run the command in a pseudo-terminal
            if self.spawn_pty(channel, program, args, session) {
                session.channel_success(channel)?;
            } else {
                session.channel_failure(channel)?;
            }
            return Ok(());
        }
        let mut cmd = tokio::process::Command::new(&program);
        cmd.args(&args)
            .current_dir(&self.ctx.root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                session.channel_failure(channel)?;
                self.ctx.log.log(format!("commande impossible : {e}"));
                return Ok(());
            }
        };
        session.channel_success(channel)?;
        if let Some(i) = child.stdin.take() {
            self.stdins.insert(channel, i);
        }
        let handle = session.handle();
        let mut out = child.stdout.take();
        let mut err = child.stderr.take();
        self.tasks.spawn(async move {
            let h2 = handle.clone();
            let o = async {
                if let Some(o) = out.as_mut() {
                    let mut b = vec![0u8; 16 * 1024];
                    while let Ok(n) = o.read(&mut b).await {
                        if n == 0 || handle.data(channel, b[..n].to_vec()).await.is_err() {
                            break;
                        }
                    }
                }
            };
            let e = async {
                if let Some(e) = err.as_mut() {
                    let mut b = vec![0u8; 16 * 1024];
                    while let Ok(n) = e.read(&mut b).await {
                        if n == 0 || h2.extended_data(channel, 1, b[..n].to_vec()).await.is_err() {
                            break;
                        }
                    }
                }
            };
            tokio::join!(o, e);
            let code = child.wait().await.ok().and_then(|s| s.code()).unwrap_or(1) as u32;
            let _ = handle.exit_status_request(channel, code).await;
            let _ = handle.eof(channel).await;
            let _ = handle.close(channel).await;
        });
        Ok(())
    }

    async fn data(&mut self, channel: ChannelId, data: &[u8], _session: &mut Session) -> Result<(), Self::Error> {
        if let Some(p) = self.ptys.get(&channel) {
            let _ = p.write(data);
        } else if let Some(i) = self.stdins.get_mut(&channel) {
            let _ = i.write_all(data).await;
        }
        Ok(())
    }

    async fn channel_eof(&mut self, channel: ChannelId, _session: &mut Session) -> Result<(), Self::Error> {
        // end of the client input: close the command's stdin
        self.stdins.remove(&channel);
        Ok(())
    }

    async fn channel_close(&mut self, channel: ChannelId, _session: &mut Session) -> Result<(), Self::Error> {
        if let Some(p) = self.ptys.remove(&channel) {
            p.kill();
        }
        self.stdins.remove(&channel);
        self.sizes.remove(&channel);
        self.channels.remove(&channel);
        Ok(())
    }

    async fn subsystem_request(&mut self, channel_id: ChannelId, name: &str, session: &mut Session) -> Result<(), Self::Error> {
        let channel = self.channels.remove(&channel_id);
        match (name, channel) {
            ("sftp", Some(channel)) => {
                session.channel_success(channel_id)?;
                self.ctx.log.log(format!("{} ouvre une session SFTP", self.peer.ip()));
                let log = self.ctx.log.clone();
                let ip = self.peer.ip();
                let sftp = SftpServer::new(self.ctx.root.clone(), self.ctx.cfg.writable)
                    .with_activity(Arc::new(move |what| log.log(format!("{ip} SFTP : {what}"))));
                russh_sftp::server::run(channel.into_stream(), sftp).await;
            }
            _ => session.channel_failure(channel_id)?,
        }
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
        let target = format!("{host_to_connect}:{port_to_connect}");
        match tokio::time::timeout(Duration::from_secs(10), tokio::net::TcpStream::connect(&target)).await {
            Ok(Ok(mut tcp)) => {
                reply.accept().await;
                self.ctx.log.log(format!("{} redirection vers {target}", self.peer.ip()));
                self.tasks.spawn(async move {
                    let mut s = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut tcp, &mut s).await;
                });
            }
            _ => reply.reject(russh::ChannelOpenFailure::ConnectFailed).await,
        }
        Ok(())
    }
}
