//! SCP fallback of the file browser against a real SSH server WITHOUT the SFTP subsystem.
//! Ignored by default; run with:
//!   ORB_SCP_HOST=… ORB_SCP_PORT=… ORB_SCP_USER=… ORB_SCP_PW=… cargo test --test scp -- --ignored
//! Works in a temporary folder under /tmp of the server and removes it.

use std::sync::Arc;

use orbiterm_lib::scp;
use orbiterm_lib::ssh::{ConnectParams, Credentials, SshConnection};

#[tokio::test]
#[ignore]
async fn scp_fallback_on_real_server() {
    let env = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("variable {k} manquante"));
    let known = tempfile::tempdir().unwrap();
    let p = ConnectParams::simple(
        &env("ORB_SCP_HOST"),
        env("ORB_SCP_PORT").parse().unwrap(),
        &env("ORB_SCP_USER"),
        Credentials::Password(env("ORB_SCP_PW")),
        known.path().join("known_hosts"),
    );
    let conn = SshConnection::connect(p, Arc::new(|_| Box::pin(async { true })), Arc::new(|_| {}), Arc::new(|_| {}))
        .await
        .expect("connexion");
    assert!(conn.sftp_opt().await.unwrap().is_none(), "ce serveur ne doit pas proposer SFTP");
    assert!(conn.sftp_opt().await.unwrap().is_none(), "refus mémorisé");

    let home = scp::home(&conn).await.unwrap();
    assert!(home.starts_with('/'), "{home}");
    let base = format!("/tmp/orbiterm-scp-{}", std::process::id());
    scp::mkdir(&conn, &base).await.unwrap();
    let dir = format!("{base}/dossier d'été");
    scp::mkdir(&conn, &dir).await.unwrap();
    assert!(scp::mkdir(&conn, &dir).await.is_err(), "already exists");

    // upload, list, download (1 MiB + a few bytes, binary)
    let local = tempfile::tempdir().unwrap();
    let data: Vec<u8> = (0..(1 << 20) + 37).map(|i| (i * 7 % 251) as u8).collect();
    let src = local.path().join("données.bin");
    std::fs::write(&src, &data).unwrap();
    let remote = format!("{dir}/données.bin");
    let last = std::sync::Mutex::new((0, 0));
    scp::upload(&conn, &src, &remote, &|d, t| *last.lock().unwrap() = (d, t)).await.unwrap();
    assert_eq!(*last.lock().unwrap(), (data.len() as u64, data.len() as u64));
    let l = scp::list(&conn, &dir).await.unwrap();
    assert_eq!(l.len(), 1, "{l:?}");
    assert_eq!((l[0].name.as_str(), l[0].kind.as_str(), l[0].size), ("données.bin", "file", data.len() as u64));
    assert!(l[0].mtime > 1_600_000_000 && l[0].perms.len() == 9, "{l:?}");
    let l = scp::list(&conn, &base).await.unwrap();
    assert_eq!((l[0].name.as_str(), l[0].kind.as_str()), ("dossier d'été", "dir"));

    let back = local.path().join("retour.bin");
    scp::download(&conn, &remote, &back, &|_, _| {}).await.unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), data);
    // empty file
    let empty = local.path().join("vide.txt");
    std::fs::write(&empty, b"").unwrap();
    scp::upload(&conn, &empty, &format!("{dir}/vide.txt"), &|_, _| {}).await.unwrap();
    scp::download(&conn, &format!("{dir}/vide.txt"), &local.path().join("vide2.txt"), &|_, _| {}).await.unwrap();
    assert_eq!(std::fs::read(local.path().join("vide2.txt")).unwrap(), b"");

    // errors are reported, no partial file left
    let r = scp::download(&conn, &format!("{dir}/absent"), &local.path().join("absent"), &|_, _| {}).await;
    assert!(r.is_err() && !local.path().join("absent").exists() && !local.path().join("absent.orbiterm-part").exists());
    assert!(scp::list(&conn, "/n/existe/pas").await.is_err());

    scp::rename(&conn, &remote, &format!("{dir}/renommé.bin")).await.unwrap();
    assert!(scp::list(&conn, &dir).await.unwrap().iter().any(|e| e.name == "renommé.bin"));
    scp::remove(&conn, &base).await.unwrap();
    assert!(scp::list(&conn, &base).await.is_err());
    conn.disconnect().await;
}
