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
}
