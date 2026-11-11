//! End-to-end test of the embedded RDP client against `tools/rdp-test-server`
//! (IronRDP server, NLA test/test). The server binary is looked up in
//! `ORBITERM_RDP_TEST_SERVER` or the usual target directories; the test is skipped if absent.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use orbiterm_lib::error::AppError;
use orbiterm_lib::known_hosts::HostKeyStatus;
use orbiterm_lib::rdp_client::{connect, RdpParams};
use orbiterm_lib::ssh::{CloseSink, HostKeyPrompter, HostKeyQuestion};
use tokio_tungstenite::tungstenite::Message;

fn server_exe() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ORBITERM_RDP_TEST_SERVER") {
        return Some(PathBuf::from(p));
    }
    let base = std::env::var("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|_| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("tools").join("rdp-test-server").join("target")
    });
    let exe = if cfg!(windows) { "rdp-test-server.exe" } else { "rdp-test-server" };
    [base.join("tools").join("debug").join(exe), base.join("debug").join(exe), base.join("tools").join("release").join(exe)]
        .into_iter()
        .find(|p| p.exists())
}

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

fn params(port: u16, password: &str, known: &std::path::Path) -> RdpParams {
    RdpParams {
        host: "127.0.0.1".into(),
        port,
        cert_host: "127.0.0.1".into(),
        cert_port: port,
        username: "test".into(),
        password: password.into(),
        domain: None,
        width: 1024,
        height: 768,
        known_hosts: known.to_path_buf(),
        // the clipboard and drive channels must not disturb the session
        clipboard: true,
        share_dir: known.parent().map(|p| p.to_path_buf()),
        sound: true,
        monitors: Vec::new(),
        gateway: None,
    }
}

fn prompter(seen: Arc<Mutex<Vec<HostKeyStatus>>>) -> HostKeyPrompter {
    Arc::new(move |q: HostKeyQuestion| {
        seen.lock().unwrap().push(q.status);
        Box::pin(async { true })
    })
}

/// Reads viewer messages until `pred` accepts one (rect messages are type 1).
async fn wait_msg<S>(ws: &mut S, what: &str, mut pred: impl FnMut(&[u8]) -> bool)
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let m = tokio::time::timeout_at(deadline, ws.next()).await.unwrap_or_else(|_| panic!("timeout: {what}"));
        if let Some(Ok(Message::Binary(b))) = m
            && pred(&b)
        {
            return;
        }
    }
}

/// RGBA of pixel (x, y) if it lies in this rect message.
fn pixel(m: &[u8], x: u16, y: u16) -> Option<[u8; 4]> {
    if m.first() != Some(&1) {
        return None;
    }
    let r = |i: usize| u16::from_le_bytes([m[i], m[i + 1]]);
    let (l, t, w, h) = (r(1), r(3), r(5), r(7));
    if x < l || y < t || x >= l + w || y >= t + h {
        return None;
    }
    let off = 9 + (usize::from(y - t) * usize::from(w) + usize::from(x - l)) * 4;
    Some([m[off], m[off + 1], m[off + 2], m[off + 3]])
}

#[tokio::test]
async fn embedded_rdp_client() {
    let Some(exe) = server_exe() else {
        eprintln!("rdp-test-server introuvable : test ignoré (cargo build dans tools/rdp-test-server)");
        return;
    };
    let port = 3391;
    let log = Arc::new(Mutex::new(String::new()));
    let mut child = Command::new(&exe).arg(port.to_string()).stdout(Stdio::piped()).spawn().expect("server");
    let out = child.stdout.take().unwrap();
    let log2 = log.clone();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for l in std::io::BufReader::new(out).lines().map_while(Result::ok) {
            log2.lock().unwrap().push_str(&(l + "\n"));
        }
    });
    let _server = Server(child);
    tokio::time::sleep(Duration::from_millis(800)).await;

    let dir = tempfile::tempdir().unwrap();
    let known = dir.path().join("rdp_known_hosts");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let closed: CloseSink = Arc::new(|_| {});

    // wrong password → AuthFailed (the certificate is asked about first, and learned)
    let e = connect(params(port, "faux", &known), prompter(seen.clone()), closed.clone()).await.err().expect("must fail");
    assert!(matches!(e, AppError::AuthFailed), "wrong password gave {e:?}");
    assert_eq!(seen.lock().unwrap().as_slice(), &[HostKeyStatus::Unknown]);

    // good password: no question this time (certificate known)
    let s = connect(params(port, "test", &known), prompter(seen.clone()), closed).await.expect("connect");
    assert_eq!(seen.lock().unwrap().len(), 1, "known certificate must not be asked again");
    assert_eq!(s.desktop, (1024, 768));

    let (ws, _) = tokio_tungstenite::connect_async(s.url()).await.expect("viewer");
    let (mut tx, mut rx) = ws.split();
    // size, then the test pattern: red bar at the top left, grey bottom
    wait_msg(&mut rx, "size", |m| m.first() == Some(&2) && m[1..5] == [0, 4, 0, 3]).await;
    wait_msg(&mut rx, "red bar", |m| pixel(m, 50, 50).is_some_and(|p| p[0] > 200 && p[1] < 60 && p[3] == 255)).await;

    // sound: the server plays a 440 Hz tone, forwarded as PCM frames (type 6)
    let (mut pcm, mut loud) = (0usize, 0i16);
    wait_msg(&mut rx, "sound", |m| {
        if m.first() == Some(&6) {
            assert_eq!(m[1], 2, "stereo");
            assert_eq!(u32::from_le_bytes([m[2], m[3], m[4], m[5]]), 44_100);
            pcm += m.len() - 6;
            loud = m[6..].chunks_exact(2).map(|s| i16::from_le_bytes([s[0], s[1]]).saturating_abs()).max().unwrap_or(0).max(loud);
        }
        pcm >= 100_000
    })
    .await;
    assert!(loud > 7000, "the tone must not be silent (peak {loud})");

    // a bad token is refused
    let bad = s.url().rsplit_once('/').unwrap().0.to_string() + "/nope";
    assert!(tokio_tungstenite::connect_async(bad).await.is_err());

    // key "a" (scancode 0x1e) → bottom half yellow
    for up in [false, true] {
        tx.send(Message::text(format!(r#"{{"t":"k","sc":30,"up":{up}}}"#))).await.unwrap();
    }
    wait_msg(&mut rx, "yellow", |m| pixel(m, 500, 600).is_some_and(|p| p[0] > 200 && p[1] > 170 && p[2] < 60)).await;

    // click → white square
    tx.send(Message::text(r#"{"t":"m","x":300,"y":200}"#)).await.unwrap();
    tx.send(Message::text(r#"{"t":"b","b":0,"down":true,"x":300,"y":200}"#)).await.unwrap();
    tx.send(Message::text(r#"{"t":"b","b":0,"down":false,"x":300,"y":200}"#)).await.unwrap();
    wait_msg(&mut rx, "white square", |m| pixel(m, 300, 200).is_some_and(|p| p[0] > 240 && p[1] > 240 && p[2] > 240)).await;

    tokio::time::sleep(Duration::from_millis(300)).await;
    let l = log.lock().unwrap().clone();
    assert!(l.contains("key:0x1e"), "server log: {l}");
    assert!(l.contains("click:300,200"), "server log: {l}");
    drop(s);

    // multi-monitor: two 800×600 monitors side by side (monitor layout in the GCC block) are accepted
    let mut p = params(port, "test", &known);
    p.monitors = vec![(0, 0, 800, 600, true), (800, 0, 800, 600, false)];
    let s = connect(p, prompter(seen.clone()), Arc::new(|_| {})).await.expect("connect multi-monitor");
    let (ws, _) = tokio_tungstenite::connect_async(s.url()).await.expect("viewer");
    let (_tx, mut rx) = ws.split();
    wait_msg(&mut rx, "image (multi-monitor)", |m| m.first() == Some(&1)).await;
    drop(s);
}

/// Against a real RDP server, only when configured (nothing is stored in the repository):
/// `ORBITERM_RDP_REAL=host[:port]`, `ORBITERM_RDP_USER`, `ORBITERM_RDP_PASSWORD`.
/// Checks NLA logon, the first image, a dynamic resize (Display Control) and a clean close.
#[tokio::test]
async fn real_rdp_server() {
    let Ok(target) = std::env::var("ORBITERM_RDP_REAL") else {
        eprintln!("ORBITERM_RDP_REAL non défini : test ignoré");
        return;
    };
    let (host, port) = match target.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().unwrap()),
        None => (target.clone(), 3389),
    };
    let dir = tempfile::tempdir().unwrap();
    let mut p = params(port, &std::env::var("ORBITERM_RDP_PASSWORD").unwrap_or_default(), &dir.path().join("k"));
    p.host = host.clone();
    p.cert_host = host;
    p.username = std::env::var("ORBITERM_RDP_USER").unwrap_or_default();
    p.width = 1280;
    p.height = 720;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let reason = Arc::new(Mutex::new(String::new()));
    let r2 = reason.clone();
    let closed: CloseSink = Arc::new(move |r| *r2.lock().unwrap() = r);

    // wrong password first: must be reported as an authentication failure
    let mut bad = params(port, "certainement-pas-le-bon", &dir.path().join("k"));
    (bad.host, bad.cert_host, bad.username) = (p.host.clone(), p.cert_host.clone(), p.username.clone());
    let e = connect(bad, prompter(seen.clone()), closed.clone()).await.err().expect("must fail");
    eprintln!("mauvais mot de passe → {e:?}");
    assert!(matches!(e, AppError::AuthFailed), "{e:?}");

    let t0 = std::time::Instant::now();
    let s = connect(p, prompter(seen.clone()), closed).await.expect("connect");
    eprintln!("connecté en {:?}, bureau {:?}, certificat demandé {} fois", t0.elapsed(), s.desktop, seen.lock().unwrap().len());

    let (ws, _) = tokio_tungstenite::connect_async(s.url()).await.expect("viewer");
    let (mut tx, mut rx) = ws.split();
    let mut rects = 0usize;
    let mut bytes = 0usize;
    let mut non_black = false;
    wait_msg(&mut rx, "first image", |m| {
        if m.first() == Some(&1) {
            rects += 1;
            bytes += m.len();
            non_black |= m[9..].chunks(4).any(|p| p[0] > 16 || p[1] > 16 || p[2] > 16);
        }
        rects >= 3 && non_black
    })
    .await;
    eprintln!("image reçue : {rects} rectangles, {} Ko", bytes / 1024);

    // dynamic resize
    tx.send(Message::text(r#"{"t":"r","w":1600,"h":900}"#)).await.unwrap();
    let mut got = (0u16, 0u16);
    wait_msg(&mut rx, "resize", |m| {
        if m.first() == Some(&2) {
            got = (u16::from_le_bytes([m[1], m[2]]), u16::from_le_bytes([m[3], m[4]]));
            return true;
        }
        false
    })
    .await;
    eprintln!("redimensionné : {got:?}");
    assert_eq!(got, (1600, 900));
    wait_msg(&mut rx, "image after resize", |m| m.first() == Some(&1)).await;

    s.close();
    tokio::time::sleep(Duration::from_secs(2)).await;
    eprintln!("fermeture : {}", reason.lock().unwrap());
}
/// Through the test RD Gateway (MS-TSGU over WebSocket, Basic gw / gwpass, self-signed).
#[tokio::test]
async fn rdp_through_gateway() {
    use orbiterm_lib::rdp_client::RdGateway;
    let Some(exe) = server_exe() else {
        eprintln!("rdp-test-server introuvable : test ignoré");
        return;
    };
    let (port, gw_port) = (3392u16, 8444u16);
    let log = Arc::new(Mutex::new(String::new()));
    let mut child = Command::new(&exe)
        .args([port.to_string(), "--gateway".into(), gw_port.to_string()])
        .stdout(Stdio::piped())
        .spawn()
        .expect("server");
    let out = child.stdout.take().unwrap();
    let log2 = log.clone();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for l in std::io::BufReader::new(out).lines().map_while(Result::ok) {
            log2.lock().unwrap().push_str(&(l + "\n"));
        }
    });
    let _server = Server(child);
    tokio::time::sleep(Duration::from_millis(800)).await;

    let dir = tempfile::tempdir().unwrap();
    let known = dir.path().join("rdp_known_hosts");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let gw = |password: &str, insecure: bool| {
        Some(RdGateway { endpoint: format!("127.0.0.1:{gw_port}"), username: "gw".into(), password: password.into(), insecure })
    };

    // strict certificate check: the self-signed gateway is refused
    let mut p = params(port, "test", &known);
    p.gateway = gw("gwpass", false);
    let e = connect(p, prompter(seen.clone()), Arc::new(|_| {})).await.err().expect("self-signed gateway refused");
    eprintln!("certificat strict → {e}");
    assert!(matches!(e, AppError::Conn(_)), "{e:?}");

    // wrong gateway password
    let mut p = params(port, "test", &known);
    p.gateway = gw("faux", true);
    let e = connect(p, prompter(seen.clone()), Arc::new(|_| {})).await.err().expect("wrong gateway password");
    assert!(matches!(e, AppError::JumpAuthFailed), "{e:?}");

    // through the gateway: the RDP session works as usual
    let mut p = params(port, "test", &known);
    p.gateway = gw("gwpass", true);
    let s = connect(p, prompter(seen.clone()), Arc::new(|_| {})).await.expect("connect through the gateway");
    let (ws, _) = tokio_tungstenite::connect_async(s.url()).await.expect("viewer");
    let (mut tx, mut rx) = ws.split();
    wait_msg(&mut rx, "red bar via gateway", |m| pixel(m, 50, 50).is_some_and(|px| px[0] > 200 && px[1] < 60)).await;
    for up in [false, true] {
        tx.send(Message::text(format!(r#"{{"t":"k","sc":30,"up":{up}}}"#))).await.unwrap();
    }
    wait_msg(&mut rx, "yellow via gateway", |m| pixel(m, 500, 600).is_some_and(|px| px[0] > 200 && px[1] > 170 && px[2] < 60)).await;
    drop(s);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let l = log.lock().unwrap().clone();
    assert!(l.contains("gw:refusé") && l.contains("gw:auth-ok"), "server log: {l}");
    assert!(l.contains(&format!("gw:canal 127.0.0.1:{port}")) && l.contains("key:0x1e"), "server log: {l}");
}
