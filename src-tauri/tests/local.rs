//! Local terminal (PTY) and Telnet backends.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use orbiterm_lib::pty::{self, PtyConnection, PtySpawn};
use orbiterm_lib::ssh::{CloseSink, DataSink};
use orbiterm_lib::telnet::{Mode, TcpTermConnection};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type Captured = (DataSink, CloseSink, Arc<Mutex<Vec<u8>>>, Arc<Mutex<Option<String>>>);

fn sinks() -> Captured {
    let out = Arc::new(Mutex::new(Vec::new()));
    let closed = Arc::new(Mutex::new(None));
    let (o, c) = (out.clone(), closed.clone());
    (
        Arc::new(move |b: Vec<u8>| o.lock().unwrap().extend(b)),
        Arc::new(move |r: String| *c.lock().unwrap() = Some(r)),
        out,
        closed,
    )
}

async fn wait_until(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..200 {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

/// ConPTY asks for the cursor position (ESC[6n) and blocks until answered;
/// in the app xterm.js replies, here we do it by hand.
async fn answer_cursor_query(c: &PtyConnection, out: &Arc<Mutex<Vec<u8>>>) {
    if cfg!(windows) {
        wait_until(|| out.lock().unwrap().windows(4).any(|w| w == b"\x1b[6n")).await;
        c.write(b"\x1b[1;1R").unwrap();
    }
}

#[tokio::test]
async fn local_shells_are_detected() {
    let shells = pty::local_shells();
    assert!(!shells.is_empty(), "no local shell found");
}

#[tokio::test]
async fn pty_runs_a_command_and_reports_exit() {
    let (on_data, on_close, out, closed) = sinks();
    let (program, args) = if cfg!(windows) {
        ("cmd.exe".to_string(), vec!["/c".into(), "echo orbiterm-pty-ok".into()])
    } else {
        ("/bin/sh".to_string(), vec!["-c".into(), "echo orbiterm-pty-ok".into()])
    };
    let c = PtyConnection::spawn(PtySpawn { program, args, cwd: None, env: vec![], cols: 80, rows: 24 }, on_data, on_close)
        .unwrap();
    answer_cursor_query(&c, &out).await;
    assert!(wait_until(|| closed.lock().unwrap().is_some()).await, "process never ended");
    let text = String::from_utf8_lossy(&out.lock().unwrap()).to_string();
    assert!(text.contains("orbiterm-pty-ok"), "output was {text:?}");
}

#[tokio::test]
async fn pty_interactive_write_and_kill() {
    let (on_data, on_close, out, closed) = sinks();
    let program = if cfg!(windows) { "cmd.exe" } else { "/bin/sh" };
    let c = PtyConnection::spawn(
        PtySpawn { program: program.into(), args: vec![], cwd: None, env: vec![], cols: 100, rows: 30 },
        on_data,
        on_close,
    )
    .unwrap();
    answer_cursor_query(&c, &out).await;
    c.resize(120, 40).unwrap();
    c.write(b"echo hello-from-test\r").unwrap();
    assert!(wait_until(|| String::from_utf8_lossy(&out.lock().unwrap()).contains("hello-from-test")).await);
    c.kill();
    assert!(wait_until(|| closed.lock().unwrap().is_some()).await, "kill did not end the process");
}

#[tokio::test]
async fn telnet_negotiates_and_relays_data() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        // DO NAWS, WILL ECHO, then a greeting
        s.write_all(&[255, 253, 31, 255, 251, 1]).await.unwrap();
        s.write_all(b"login: ").await.unwrap();
        let mut got = vec![];
        let mut buf = [0u8; 256];
        while !got.windows(4).any(|w| w == b"bob\r") {
            let n = s.read(&mut buf).await.unwrap();
            if n == 0 {
                break;
            }
            got.extend_from_slice(&buf[..n]);
        }
        got
    });
    let (on_data, on_close, out, _closed) = sinks();
    let c = TcpTermConnection::connect(Mode::Telnet, "127.0.0.1", port, "", 90, 33, on_data, on_close).await.unwrap();
    assert!(wait_until(|| String::from_utf8_lossy(&out.lock().unwrap()).contains("login: ")).await);
    c.write(b"bob\r").await.unwrap();
    let got = tokio::time::timeout(Duration::from_secs(5), server).await.unwrap().unwrap();
    // our replies: WILL NAWS + NAWS(90x33) + DO ECHO, then the user input
    let naws = [255, 250, 31, 0, 90, 0, 33, 255, 240];
    assert!(got.windows(3).any(|w| w == [255, 251, 31]));
    assert!(got.windows(naws.len()).any(|w| w == naws));
    assert!(got.windows(3).any(|w| w == [255, 253, 1]));
    assert!(got.ends_with(b"bob\r"));
    // the IAC sequences must not leak into the terminal output
    assert_eq!(String::from_utf8_lossy(&out.lock().unwrap()), "login: ");
}
