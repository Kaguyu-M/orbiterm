//! FTP server (libunftp + local filesystem back-end), with a read-only mode and an
//! activity log of logins and transfers.

use std::fmt::Debug;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use libunftp::ServerBuilder;
use libunftp::notification::{DataEvent, DataListener, EventMeta, PresenceEvent, PresenceListener};
use libunftp::options::Shutdown;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use unftp_core::auth::{AuthenticationError, Authenticator, Credentials, DefaultUser, Principal};
use unftp_core::storage::{Error, ErrorKind, Fileinfo, Result, StorageBackend};
use unftp_sbe_fs::{Filesystem, Meta};

use super::{Ctx, Logger};

#[derive(Debug)]
struct Auth {
    user: String,
    password: String,
    anonymous: bool,
    log: LogRef,
}

#[derive(Clone)]
struct LogRef(Logger);

impl Debug for LogRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Logger")
    }
}

#[async_trait::async_trait]
impl Authenticator for Auth {
    async fn authenticate(&self, username: &str, creds: &Credentials) -> std::result::Result<Principal, AuthenticationError> {
        let anon = self.anonymous && (username == "anonymous" || username == "ftp");
        let ok = anon
            || (!self.password.is_empty()
                && username == self.user
                && creds.password.as_deref() == Some(self.password.as_str()));
        let from = creds.source_ip.to_string();
        if ok {
            Ok(Principal { username: username.to_string() })
        } else {
            self.log.0.log(format!("{from} connexion refusée pour « {username} »"));
            Err(AuthenticationError::BadPassword)
        }
    }
}

/// Local filesystem, optionally read-only.
#[derive(Debug)]
struct Fs {
    inner: Filesystem,
    writable: bool,
}

impl Fs {
    fn check(&self) -> Result<()> {
        if self.writable { Ok(()) } else { Err(Error::from(ErrorKind::PermissionDenied)) }
    }
}

#[async_trait::async_trait]
impl StorageBackend<DefaultUser> for Fs {
    type Metadata = Meta;

    fn supported_features(&self) -> u32 {
        unftp_core::storage::FEATURE_RESTART
    }

    async fn metadata<P: AsRef<Path> + Send + Debug>(&self, u: &DefaultUser, path: P) -> Result<Self::Metadata> {
        self.inner.metadata(u, path).await
    }

    async fn list<P: AsRef<Path> + Send + Debug>(&self, u: &DefaultUser, path: P) -> Result<Vec<Fileinfo<std::path::PathBuf, Self::Metadata>>> {
        self.inner.list(u, path).await
    }

    async fn get<P: AsRef<Path> + Send + Debug>(&self, u: &DefaultUser, path: P, start: u64) -> Result<Box<dyn tokio::io::AsyncRead + Send + Sync + Unpin>> {
        self.inner.get(u, path, start).await
    }

    async fn put<P: AsRef<Path> + Send + Debug, R: tokio::io::AsyncRead + Send + Sync + Unpin + 'static>(
        &self,
        u: &DefaultUser,
        input: R,
        path: P,
        start: u64,
    ) -> Result<u64> {
        self.check()?;
        self.inner.put(u, input, path, start).await
    }

    async fn del<P: AsRef<Path> + Send + Debug>(&self, u: &DefaultUser, path: P) -> Result<()> {
        self.check()?;
        self.inner.del(u, path).await
    }

    async fn mkd<P: AsRef<Path> + Send + Debug>(&self, u: &DefaultUser, path: P) -> Result<()> {
        self.check()?;
        self.inner.mkd(u, path).await
    }

    async fn rename<P: AsRef<Path> + Send + Debug>(&self, u: &DefaultUser, from: P, to: P) -> Result<()> {
        self.check()?;
        self.inner.rename(u, from, to).await
    }

    async fn rmd<P: AsRef<Path> + Send + Debug>(&self, u: &DefaultUser, path: P) -> Result<()> {
        self.check()?;
        self.inner.rmd(u, path).await
    }

    async fn cwd<P: AsRef<Path> + Send + Debug>(&self, u: &DefaultUser, path: P) -> Result<()> {
        self.inner.cwd(u, path).await
    }
}

#[derive(Debug)]
struct Events(LogRef);

#[async_trait::async_trait]
impl DataListener for Events {
    async fn receive_data_event(&self, e: DataEvent, m: EventMeta) {
        let what = match e {
            DataEvent::Got { path, bytes } => format!("a téléchargé {path} ({bytes} octets)"),
            DataEvent::Put { path, bytes } => format!("a envoyé {path} ({bytes} octets)"),
            DataEvent::Deleted { path } => format!("a supprimé {path}"),
            DataEvent::MadeDir { path } => format!("a créé le dossier {path}"),
            DataEvent::RemovedDir { path } => format!("a supprimé le dossier {path}"),
            DataEvent::Renamed { from, to } => format!("a renommé {from} → {to}"),
        };
        self.0.0.log(format!("{} {what}", m.username));
    }
}

#[async_trait::async_trait]
impl PresenceListener for Events {
    async fn receive_presence_event(&self, e: PresenceEvent, m: EventMeta) {
        match e {
            PresenceEvent::LoggedIn => self.0.0.log(format!("{} connecté", m.username)),
            PresenceEvent::LoggedOut => self.0.0.log(format!("{} déconnecté", m.username)),
        }
    }
}

pub async fn start(addr: SocketAddr, ctx: Ctx, stop: oneshot::Receiver<()>) -> std::io::Result<(JoinHandle<()>, u16)> {
    // check the port first (libunftp binds by itself), so the error reaches the UI
    let probe = std::net::TcpListener::bind(addr)?;
    let port = probe.local_addr()?.port();
    drop(probe);
    let log = LogRef(ctx.log.clone());
    let root = ctx.root.clone();
    let writable = ctx.cfg.writable;
    let mut builder = ServerBuilder::new(Box::new(move || Fs {
        inner: Filesystem::new(root.clone()).expect("dossier partagé FTP"),
        writable,
    }))
    .authenticator(Arc::new(Auth {
        user: ctx.cfg.user.clone(),
        password: ctx.password.clone(),
        anonymous: ctx.cfg.anonymous,
        log: log.clone(),
    }))
    .greeting("Serveur FTP Orbiterm")
    .notify_data(Events(log.clone()))
    .notify_presence(Events(log))
    .passive_ports(50000..=50100)
    .idle_session_timeout(600)
    .shutdown_indicator(async move {
        let _ = stop.await;
        Shutdown::new().grace_period(Duration::from_millis(500))
    });
    if ctx.cfg.local_only {
        builder = builder.passive_host([127, 0, 0, 1]);
    }
    let server = builder.build().map_err(|e| std::io::Error::other(e.to_string()))?;
    let log = ctx.log.clone();
    let bind = format!("{}:{port}", addr.ip());
    let task = tokio::spawn(async move {
        if let Err(e) = server.listen(bind).await {
            log.log(format!("Serveur FTP arrêté : {e}"));
        }
    });
    // libunftp binds inside its task: only report "started" once the port really accepts
    // connections (on Linux the first client could otherwise be refused).
    let target = if addr.ip().is_unspecified() { SocketAddr::from(([127, 0, 0, 1], port)) } else { SocketAddr::new(addr.ip(), port) };
    for _ in 0..100 {
        if task.is_finished() {
            return Err(std::io::Error::other("le serveur FTP n'a pas pu démarrer"));
        }
        if tokio::net::TcpStream::connect(target).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok((task, port))
}
