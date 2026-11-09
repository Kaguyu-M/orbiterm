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
    ConnectParams {
        host: "127.0.0.1".into(),
        port: f.port,
        username: server::USER.into(),
        credentials: Credentials::Password(password.into()),
        cols: 80,
        rows: 24,
        known_hosts: f.cfg.path().join("known_hosts"),
    }
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
