//! End-to-end tests: FTP client against a real FTP server (libunftp), VNC WebSocket
//! bridge against a real RFB server.

#[path = "../examples/ftp_test_server.rs"]
mod ftp_server;
#[path = "../examples/vnc_test_server.rs"]
mod vnc_server;

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use orbiterm_lib::error::AppError;
use orbiterm_lib::ftp::{FtpConnection, FtpParams, FtpTls};
use orbiterm_lib::vnc::VncBridge;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

async fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port()
}

async fn wait_port(port: u16) {
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("port {port} never opened");
}

fn params(port: u16, user: &str, pw: &str) -> FtpParams {
    FtpParams {
        host: "127.0.0.1".into(),
        port,
        username: user.into(),
        password: pw.into(),
        tls: FtpTls::None,
        insecure: false,
    }
}

#[tokio::test]
async fn ftp_full_cycle() {
    let root = tempfile::tempdir().unwrap();
    let port = free_port().await;
    let r = root.path().to_path_buf();
    tokio::spawn(ftp_server::serve(r, port));
    wait_port(port).await;

    // wrong password
    match FtpConnection::connect(params(port, "test", "nope")).await {
        Err(AppError::AuthFailed) => {}
        Err(e) => panic!("expected AuthFailed, got {e}"),
        Ok(_) => panic!("expected AuthFailed, got a connection"),
    }
    // anonymous
    let anon = FtpConnection::connect(params(port, "", "")).await.expect("anonymous");
    anon.close().await;

    let c = FtpConnection::connect(params(port, "test", "test")).await.expect("login");
    let home = c.home();
    assert!(home.starts_with('/'), "home = {home}");
    c.mkdir("/docs").await.unwrap();
    c.mkdir("/docs/sub").await.unwrap();

    let local = tempfile::tempdir().unwrap();
    let src = local.path().join("rapport.txt");
    let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&src, &data).unwrap();
    let last = Arc::new(std::sync::Mutex::new((0u64, 0u64)));
    let l2 = last.clone();
    let progress = move |d, t| *l2.lock().unwrap() = (d, t);
    c.upload(&src, "/docs/rapport.txt", &progress).await.unwrap();
    assert_eq!(*last.lock().unwrap(), (300_000, 300_000));
    assert_eq!(std::fs::read(root.path().join("docs/rapport.txt")).unwrap(), data);

    let list = c.list("/docs").await.unwrap();
    let names: Vec<_> = list.iter().map(|e| (e.name.as_str(), e.kind.as_str())).collect();
    assert_eq!(names, vec![("sub", "dir"), ("rapport.txt", "file")]);
    assert_eq!(list[1].size, 300_000);
    assert_eq!(list[1].path, "/docs/rapport.txt");

    c.rename("/docs/rapport.txt", "/docs/final.txt").await.unwrap();
    let dl = local.path().join("out");
    std::fs::create_dir(&dl).unwrap();
    let target = dl.join("final.txt");
    c.download("/docs/final.txt", &target, &|_, _| {}).await.unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), data);

    // recursive removal
    std::fs::write(root.path().join("docs/sub/x.bin"), b"x").unwrap();
    c.remove("/docs").await.unwrap();
    assert!(!root.path().join("docs").exists());
    assert!(c.list("/").await.unwrap().is_empty());
    c.close().await;
}

#[tokio::test]
async fn vnc_bridge_relays_rfb_with_password() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let vport = listener.local_addr().unwrap().port();
    let log = Arc::new(tokio::sync::Mutex::new(vec![]));
    tokio::spawn(vnc_server::serve(listener, Some("secret".into()), log.clone()));

    let closed = Arc::new(std::sync::Mutex::new(None::<String>));
    let c2 = closed.clone();
    let bridge = VncBridge::start("127.0.0.1".into(), vport, Arc::new(move |r| *c2.lock().unwrap() = Some(r)))
        .await
        .unwrap();
    let url = bridge.url();
    assert!(url.starts_with("ws://127.0.0.1:"));

    // a wrong token is refused
    let bad = url.rsplit_once('/').unwrap().0.to_string() + "/nope";
    assert!(tokio_tungstenite::connect_async(bad).await.is_err());

    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.expect("ws");
    let mut inbox: Vec<u8> = vec![];
    async fn need(ws: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin), inbox: &mut Vec<u8>, n: usize) -> Vec<u8> {
        while inbox.len() < n {
            match tokio::time::timeout(Duration::from_secs(5), ws.next()).await.expect("timeout") {
                Some(Ok(Message::Binary(b))) => inbox.extend_from_slice(&b),
                other => panic!("unexpected {other:?}"),
            }
        }
        inbox.drain(..n).collect()
    }
    assert_eq!(need(&mut ws, &mut inbox, 12).await, b"RFB 003.008\n");
    ws.send(Message::Binary(b"RFB 003.008\n".to_vec().into())).await.unwrap();
    assert_eq!(need(&mut ws, &mut inbox, 2).await, vec![1, 2]); // VNC auth
    ws.send(Message::Binary(vec![2].into())).await.unwrap();
    let ch: [u8; 16] = need(&mut ws, &mut inbox, 16).await.try_into().unwrap();
    ws.send(Message::Binary(vnc_server::vnc_response("secret", &ch).to_vec().into())).await.unwrap();
    assert_eq!(need(&mut ws, &mut inbox, 4).await, vec![0, 0, 0, 0]); // auth ok
    ws.send(Message::Binary(vec![1].into())).await.unwrap(); // ClientInit
    let init = need(&mut ws, &mut inbox, 24).await;
    assert_eq!(u16::from_be_bytes([init[0], init[1]]), vnc_server::W);
    // key "a"
    ws.send(Message::Binary(vec![4, 1, 0, 0, 0, 0, 0, 0x61].into())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(*log.lock().await, vec!["auth:ok".to_string(), "key:0x61".to_string()]);

    ws.close(None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(closed.lock().unwrap().is_some(), "on_close called when the viewer leaves");
    bridge.stop();
}

#[tokio::test]
async fn vnc_unreachable_server_fails_fast() {
    let port = free_port().await; // nothing listens there
    let r = VncBridge::start("127.0.0.1".into(), port, Arc::new(|_| {})).await;
    assert!(r.is_err());
}
