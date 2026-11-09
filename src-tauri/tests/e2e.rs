//! End-to-end tests: real SSH/SFTP protocol against the in-process test server.

#[path = "../examples/test_server.rs"]
mod server;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use orbiterm_lib::error::AppError;
use orbiterm_lib::sftp;
use orbiterm_lib::ssh::{ConnectParams, Credentials, HostKeyPrompter, SshConnection};

struct Fixture {
    port: u16,
    root: tempfile::TempDir,
    cfg: tempfile::TempDir,
}

async fn fixture() -> Fixture {
    let _ = env_logger::builder().is_test(true).try_init();
    let root = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    let (port, _key) = server::start(0, root.path().to_path_buf()).await;
    Fixture { port, root, cfg }
}

fn params(f: &Fixture, password: &str) -> ConnectParams {
    ConnectParams::simple(
        "127.0.0.1",
        f.port,
        server::USER,
        Credentials::Password(password.into()),
        f.cfg.path().join("known_hosts"),
    )
}

fn prompter(answer: bool, counter: Arc<AtomicUsize>) -> HostKeyPrompter {
    Arc::new(move |_q| {
        counter.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { answer })
    })
}

type Output = Arc<Mutex<Vec<u8>>>;

async fn connect(f: &Fixture, password: &str, accept: bool, asked: Arc<AtomicUsize>) -> Result<(Arc<SshConnection>, Output), AppError> {
    let out: Output = Default::default();
    let o = out.clone();
    let conn = SshConnection::connect(
        params(f, password),
        prompter(accept, asked),
        Arc::new(move |b| o.lock().unwrap().extend(b)),
        Arc::new(|_| {}),
    )
    .await?;
    Ok((conn, out))
}

async fn wait_for(out: &Output, needle: &str) {
    for _ in 0..100 {
        if String::from_utf8_lossy(&out.lock().unwrap()).contains(needle) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    panic!("'{needle}' not received; got: {}", String::from_utf8_lossy(&out.lock().unwrap()));
}

#[tokio::test]
async fn shell_echo_and_resize() {
    let f = fixture().await;
    let asked = Arc::new(AtomicUsize::new(0));
    let (conn, out) = connect(&f, server::PASSWORD, true, asked.clone()).await.unwrap();
    wait_for(&out, "Bienvenue").await;
    conn.write(b"echo bonjour orbiterm\r").await.unwrap();
    wait_for(&out, "bonjour orbiterm\r\n$").await;
    conn.resize(132, 43).await.unwrap();
    conn.write(b"size\r").await.unwrap();
    wait_for(&out, "132x43").await;
    conn.disconnect().await;
}

#[tokio::test]
async fn host_key_is_learned_then_trusted() {
    let f = fixture().await;
    let asked = Arc::new(AtomicUsize::new(0));
    let (c1, _) = connect(&f, server::PASSWORD, true, asked.clone()).await.unwrap();
    c1.disconnect().await;
    assert_eq!(asked.load(Ordering::SeqCst), 1, "unknown key must be asked once");
    let (c2, _) = connect(&f, server::PASSWORD, true, asked.clone()).await.unwrap();
    c2.disconnect().await;
    assert_eq!(asked.load(Ordering::SeqCst), 1, "known key must not be asked again");
}

#[tokio::test]
async fn refused_host_key_aborts() {
    let f = fixture().await;
    let asked = Arc::new(AtomicUsize::new(0));
    let err = connect(&f, server::PASSWORD, false, asked).await.err().expect("must fail");
    assert!(matches!(err, AppError::HostKeyRejected), "got {err:?}");
    assert!(!f.cfg.path().join("known_hosts").exists() || std::fs::read_to_string(f.cfg.path().join("known_hosts")).unwrap().is_empty());
}

#[tokio::test]
async fn changed_host_key_is_detected() {
    let f = fixture().await;
    // pretend we knew another key for this host:port
    let other = russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519).unwrap();
    orbiterm_lib::known_hosts::learn(&f.cfg.path().join("known_hosts"), "127.0.0.1", f.port, other.public_key(), false).unwrap();
    let seen = Arc::new(Mutex::new(None));
    let s2 = seen.clone();
    let p: HostKeyPrompter = Arc::new(move |q| {
        *s2.lock().unwrap() = Some(q.status);
        Box::pin(async { false })
    });
    let r = SshConnection::connect(params(&f, server::PASSWORD), p, Arc::new(|_| {}), Arc::new(|_| {})).await;
    assert!(matches!(r, Err(AppError::HostKeyRejected)));
    assert_eq!(*seen.lock().unwrap(), Some(orbiterm_lib::known_hosts::HostKeyStatus::Changed));
}

#[tokio::test]
async fn wrong_password_is_auth_failed() {
    let f = fixture().await;
    let err = connect(&f, "mauvais", true, Arc::new(AtomicUsize::new(0))).await.err().expect("must fail");
    assert!(matches!(err, AppError::AuthFailed), "got {err:?}");
}

#[tokio::test]
async fn sftp_full_cycle() {
    let f = fixture().await;
    let (conn, _) = connect(&f, server::PASSWORD, true, Arc::new(AtomicUsize::new(0))).await.unwrap();
    let s = conn.sftp().await.unwrap();

    let home = sftp::home(&s).await.unwrap();
    assert_eq!(home, "/");

    // mkdir + upload
    sftp::mkdir(&s, "/projets").await.unwrap();
    let local_dir = tempfile::tempdir().unwrap();
    let src = local_dir.path().join("données.bin");
    let payload: Vec<u8> = (0..700_000u32).map(|i| (i % 251) as u8).collect(); // > 2 chunks
    std::fs::write(&src, &payload).unwrap();
    let last = Arc::new(Mutex::new((0u64, 0u64)));
    let l2 = last.clone();
    let progress = move |d: u64, t: u64| *l2.lock().unwrap() = (d, t);
    sftp::upload(&s, &src, "/projets/données.bin", &progress).await.unwrap();
    assert_eq!(*last.lock().unwrap(), (700_000, 700_000));
    assert_eq!(std::fs::read(f.root.path().join("projets").join("données.bin")).unwrap(), payload);

    // list (dirs first, sizes)
    std::fs::write(f.root.path().join("a.txt"), "x").unwrap();
    let entries = sftp::list(&s, "/").await.unwrap();
    assert_eq!(entries[0].name, "projets");
    assert_eq!(entries[0].kind, "dir");
    let listed = sftp::list(&s, "/projets").await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].size, 700_000);
    assert_eq!(listed[0].path, "/projets/données.bin");

    // download and compare
    let dl_dir = tempfile::tempdir().unwrap();
    let target = sftp::safe_local_join(dl_dir.path(), "données.bin").unwrap();
    sftp::download(&s, "/projets/données.bin", &target, &|_, _| {}).await.unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), payload);
    assert!(!PathBuf::from(format!("{}", target.with_extension("orbiterm-part").display())).exists());

    // rename + recursive remove
    sftp::rename(&s, "/projets/données.bin", "/projets/renommé.bin").await.unwrap();
    sftp::mkdir(&s, "/projets/sous").await.unwrap();
    std::fs::write(f.root.path().join("projets/sous/b.txt"), "y").unwrap();
    sftp::remove(&s, "/projets").await.unwrap();
    assert!(!f.root.path().join("projets").exists());

    // the terminal keeps working on the same connection
    conn.write(b"whoami\r").await.unwrap();
    conn.disconnect().await;
}

// ---------------------------------------------------------------- tunnels / jump / X11

use orbiterm_lib::config::{Forward, ForwardKind};
use orbiterm_lib::ssh::{JumpParams, ShellMode};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// TCP echo server on a random port.
async fn echo_server() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else { break };
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                loop {
                    match s.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if s.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
    });
    port
}

async fn roundtrip(mut s: tokio::net::TcpStream, msg: &[u8]) -> Vec<u8> {
    s.write_all(msg).await.unwrap();
    let mut buf = vec![0u8; msg.len()];
    tokio::time::timeout(Duration::from_secs(5), s.read_exact(&mut buf)).await.expect("timeout").unwrap();
    buf
}

#[tokio::test]
async fn local_forward() {
    let f = fixture().await;
    let echo = echo_server().await;
    let (conn, _) = connect(&f, server::PASSWORD, true, Arc::new(AtomicUsize::new(0))).await.unwrap();
    let fw = Forward { kind: ForwardKind::Local, bind_host: String::new(), bind_port: 0, dest_host: "127.0.0.1".into(), dest_port: echo };
    let port = conn.start_forward(&fw).await.unwrap();
    assert_ne!(port, 0);
    let s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    assert_eq!(roundtrip(s, b"via -L").await, b"via -L");
    assert_eq!(conn.active_forwards().len(), 1);
    conn.disconnect().await;
}

#[tokio::test]
async fn dynamic_socks_forward() {
    let f = fixture().await;
    let echo = echo_server().await;
    let (conn, _) = connect(&f, server::PASSWORD, true, Arc::new(AtomicUsize::new(0))).await.unwrap();
    let fw = Forward { kind: ForwardKind::Dynamic, bind_port: 0, ..Default::default() };
    let port = conn.start_forward(&fw).await.unwrap();
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    // SOCKS5, no auth, CONNECT localhost:echo (domain name form)
    s.write_all(&[5, 1, 0]).await.unwrap();
    let mut r = [0u8; 2];
    s.read_exact(&mut r).await.unwrap();
    assert_eq!(r, [5, 0]);
    let mut req = vec![5, 1, 0, 3, 9];
    req.extend(b"127.0.0.1");
    req.extend(echo.to_be_bytes());
    s.write_all(&req).await.unwrap();
    let mut rep = [0u8; 10];
    s.read_exact(&mut rep).await.unwrap();
    assert_eq!(rep[1], 0, "socks connect must succeed");
    assert_eq!(roundtrip(s, b"via socks").await, b"via socks");
    conn.disconnect().await;
}

#[tokio::test]
async fn remote_forward() {
    let f = fixture().await;
    let echo = echo_server().await;
    let (conn, _) = connect(&f, server::PASSWORD, true, Arc::new(AtomicUsize::new(0))).await.unwrap();
    let fw = Forward { kind: ForwardKind::Remote, bind_host: String::new(), bind_port: 0, dest_host: "127.0.0.1".into(), dest_port: echo };
    let port = conn.start_forward(&fw).await.unwrap();
    assert_ne!(port, 0, "server must report the allocated port");
    // the (test) server listens on 127.0.0.1:port and sends connections back to us
    let s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    assert_eq!(roundtrip(s, b"via -R").await, b"via -R");
    conn.disconnect().await;
}

#[tokio::test]
async fn jump_host_then_target() {
    let f = fixture().await;
    let asked = Arc::new(AtomicUsize::new(0));
    let mut p = params(&f, server::PASSWORD);
    // the same test server is used as gateway and as target (reached through the gateway)
    p.jump = Some(JumpParams {
        host: "127.0.0.1".into(),
        port: f.port,
        username: server::USER.into(),
        credentials: Credentials::Password(server::PASSWORD.into()),
    });
    let out: Output = Default::default();
    let o = out.clone();
    let conn = SshConnection::connect(p, prompter(true, asked.clone()), Arc::new(move |b| o.lock().unwrap().extend(b)), Arc::new(|_| {}))
        .await
        .unwrap();
    wait_for(&out, "Bienvenue").await;
    conn.write(b"whoami\r").await.unwrap();
    wait_for(&out, "test\r\n$").await;
    // gateway and target share host:port here, so the key is asked once then known
    assert!(asked.load(Ordering::SeqCst) >= 1);
    conn.disconnect().await;

    // wrong gateway password -> JumpAuthFailed
    let mut p = params(&f, server::PASSWORD);
    p.jump = Some(JumpParams {
        host: "127.0.0.1".into(),
        port: f.port,
        username: server::USER.into(),
        credentials: Credentials::Password("mauvais".into()),
    });
    let r = SshConnection::connect(p, prompter(true, asked), Arc::new(|_| {}), Arc::new(|_| {})).await;
    assert!(matches!(r, Err(AppError::JumpAuthFailed)), "got {:?}", r.err());
}

#[tokio::test]
async fn tunnel_only_connection() {
    let f = fixture().await;
    let echo = echo_server().await;
    let mut p = params(&f, server::PASSWORD);
    p.mode = ShellMode::None;
    let conn = SshConnection::connect(p, prompter(true, Arc::new(AtomicUsize::new(0))), Arc::new(|_| {}), Arc::new(|_| {}))
        .await
        .unwrap();
    let fw = Forward { kind: ForwardKind::Local, bind_port: 0, dest_host: "127.0.0.1".into(), dest_port: echo, ..Default::default() };
    let port = conn.start_forward(&fw).await.unwrap();
    let s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    assert_eq!(roundtrip(s, b"tunnel").await, b"tunnel");
    assert!(!conn.is_closed());
    conn.disconnect().await;
}

#[tokio::test]
async fn x11_forwarding_reaches_local_display() {
    let f = fixture().await;
    // fake local X server on 127.0.0.1:(6000+N)
    let mut found = None;
    for n in 60..120u16 {
        if let Ok(l) = tokio::net::TcpListener::bind(("127.0.0.1", 6000 + n)).await {
            found = Some((n, l));
            break;
        }
    }
    let (n, listener) = found.expect("no free X display port");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s2 = seen.clone();
    tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 512];
        let mut got = vec![];
        while !got.ends_with(b"PING") {
            let n = s.read(&mut buf).await.unwrap();
            if n == 0 {
                break;
            }
            got.extend_from_slice(&buf[..n]);
        }
        *s2.lock().unwrap() = got;
        s.write_all(b"PONG").await.unwrap();
    });
    let mut p = params(&f, server::PASSWORD);
    p.x11_display = Some(format!("127.0.0.1:{n}"));
    let out: Output = Default::default();
    let o = out.clone();
    let conn = SshConnection::connect(p, prompter(true, Arc::new(AtomicUsize::new(0))), Arc::new(move |b| o.lock().unwrap().extend(b)), Arc::new(|_| {}))
        .await
        .unwrap();
    wait_for(&out, "Bienvenue").await;
    conn.write(b"xtest\r").await.unwrap();
    wait_for(&out, "X11:PONG").await;
    // the fake cookie was replaced: no xauth here, so the authentication is removed
    let got = seen.lock().unwrap().clone();
    assert_eq!(&got[..2], &[b'l', 0]);
    assert_eq!(&got[6..10], &[0, 0, 0, 0], "auth name/data lengths must be zero");
    assert!(got.ends_with(b"PING"));
    conn.disconnect().await;
}