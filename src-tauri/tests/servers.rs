//! End-to-end tests of the built-in servers (HTTP, TFTP, FTP, SSH/SFTP) through the
//! `ServerManager`, on 127.0.0.1 and random ports, with Orbiterm's own clients where
//! they exist. Passwords are passed directly (the system keyring is not touched).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use orbiterm_lib::error::AppError;
use orbiterm_lib::ftp::{FtpConnection, FtpParams, FtpTls};
use orbiterm_lib::servers::{ServerConfig, ServerKind, ServerManager};
use orbiterm_lib::ssh::{ConnectParams, Credentials, SshConnection};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};

fn cfg(kind: ServerKind, root: &std::path::Path) -> ServerConfig {
    let mut c = ServerConfig::new(kind);
    c.port = 0;
    c.local_only = true;
    c.root = root.to_string_lossy().into_owned();
    c.user = "orbi".into();
    c
}

async fn http(port: u16, req: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = vec![];
    tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut out)).await.expect("http timeout").unwrap();
    String::from_utf8_lossy(&out).into_owned()
}

#[tokio::test]
async fn http_server() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("docs")).unwrap();
    std::fs::write(root.path().join("docs").join("note été.txt"), "bonjour HTTP").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let m = ServerManager::new(dir.path().to_path_buf());
    let port = m.start_with(cfg(ServerKind::Http, root.path()), String::new()).await.unwrap();

    let r = http(port, "GET / HTTP/1.0\r\n\r\n").await;
    assert!(r.starts_with("HTTP/1.1 200") && r.contains("href=\"/docs/\""), "{r}");
    let r = http(port, "GET /docs HTTP/1.0\r\n\r\n").await;
    assert!(r.starts_with("HTTP/1.1 301") && r.contains("Location: /docs/"), "{r}");
    let r = http(port, "GET /docs/note%20%C3%A9t%C3%A9.txt HTTP/1.0\r\n\r\n").await;
    assert!(r.contains("Content-Type: text/plain") && r.ends_with("\r\n\r\nbonjour HTTP"), "{r}");
    let r = http(port, "GET /docs/note%20%C3%A9t%C3%A9.txt HTTP/1.1\r\nRange: bytes=8-11\r\nConnection: close\r\n\r\n").await;
    assert!(r.starts_with("HTTP/1.1 206") && r.contains("Content-Range: bytes 8-11/12") && r.ends_with("HTTP"), "{r}");
    let r = http(port, "HEAD /docs/note%20%C3%A9t%C3%A9.txt HTTP/1.0\r\n\r\n").await;
    assert!(r.contains("Content-Length: 12") && r.ends_with("\r\n\r\n"), "{r}");
    // path traversal stays inside the shared folder
    let r = http(port, "GET /../../../Windows/win.ini HTTP/1.0\r\n\r\n").await;
    assert!(r.starts_with("HTTP/1.1 404"), "{r}");
    let r = http(port, "GET /..%5C..%5Cx HTTP/1.0\r\n\r\n").await;
    assert!(r.starts_with("HTTP/1.1 404"), "{r}");
    let r = http(port, "POST / HTTP/1.0\r\n\r\n").await;
    assert!(r.starts_with("HTTP/1.1 405"), "{r}");
    // keep-alive: two requests on one connection
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(b"HEAD / HTTP/1.1\r\nHost: x\r\n\r\nGET /docs/ HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    assert_eq!(out.matches("HTTP/1.1 200").count(), 2, "{out}");

    let st = m.status().await;
    let h = st.iter().find(|s| s.config.kind == ServerKind::Http).unwrap();
    assert!(h.running && h.port == port);
    assert!(h.log.iter().any(|l| l.msg.contains("GET /docs/note été.txt → 200")), "{:?}", h.log);
    m.stop(ServerKind::Http).await;
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err(), "port must be closed after stop");
}

// ------------------------------------------------------------------ TFTP client (test only)

fn rrq(op: u8, file: &str, opts: &[(&str, &str)]) -> Vec<u8> {
    let mut v = vec![0, op];
    for s in [file, "octet"].into_iter().chain(opts.iter().flat_map(|(k, v)| [*k, *v])) {
        v.extend(s.as_bytes());
        v.push(0);
    }
    v
}

async fn tftp_get(port: u16, file: &str, blksize: Option<usize>) -> Result<Vec<u8>, String> {
    let s = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let bs = blksize.unwrap_or(512);
    let b = bs.to_string();
    let opts: Vec<(&str, &str)> = if blksize.is_some() { vec![("blksize", &b), ("tsize", "0")] } else { vec![] };
    s.send_to(&rrq(1, file, &opts), ("127.0.0.1", port)).await.unwrap();
    let mut buf = vec![0u8; bs + 4];
    let mut out = vec![];
    let mut expect = 1u16;
    loop {
        let (n, from) = tokio::time::timeout(Duration::from_secs(5), s.recv_from(&mut buf)).await.expect("tftp timeout").unwrap();
        match u16::from_be_bytes([buf[0], buf[1]]) {
            6 => s.send_to(&[0, 4, 0, 0], from).await.map(|_| ()).unwrap(), // OACK → ACK 0
            3 => {
                let blk = u16::from_be_bytes([buf[2], buf[3]]);
                s.send_to(&[0, 4, buf[2], buf[3]], from).await.unwrap();
                if blk == expect {
                    out.extend(&buf[4..n]);
                    expect += 1;
                    if n - 4 < bs {
                        return Ok(out);
                    }
                }
            }
            5 => return Err(String::from_utf8_lossy(&buf[4..n - 1]).into_owned()),
            o => panic!("opcode {o}"),
        }
    }
}

async fn tftp_put(port: u16, file: &str, data: &[u8]) -> Result<(), String> {
    let s = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    s.send_to(&rrq(2, file, &[]), ("127.0.0.1", port)).await.unwrap();
    let mut buf = [0u8; 600];
    let (n, from) = s.recv_from(&mut buf).await.unwrap();
    if u16::from_be_bytes([buf[0], buf[1]]) == 5 {
        return Err(String::from_utf8_lossy(&buf[4..n - 1]).into_owned());
    }
    let mut chunks: Vec<&[u8]> = data.chunks(512).collect();
    if data.len().is_multiple_of(512) {
        chunks.push(&[]);
    }
    for (i, c) in chunks.iter().enumerate() {
        let blk = (i + 1) as u16;
        let mut p = vec![0, 3];
        p.extend(blk.to_be_bytes());
        p.extend(*c);
        s.send_to(&p, from).await.unwrap();
        let (_, _) = tokio::time::timeout(Duration::from_secs(5), s.recv_from(&mut buf)).await.expect("ack").unwrap();
        assert_eq!(&buf[..4], &[0, 4, (blk >> 8) as u8, blk as u8]);
    }
    Ok(())
}

#[tokio::test]
async fn tftp_server() {
    let root = tempfile::tempdir().unwrap();
    let data: Vec<u8> = (0..5000u32).map(|i| (i % 253) as u8).collect();
    std::fs::create_dir(root.path().join("boot")).unwrap();
    std::fs::write(root.path().join("boot").join("image.bin"), &data).unwrap();
    std::fs::write(root.path().join("exact.bin"), vec![7u8; 1024]).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let m = ServerManager::new(dir.path().to_path_buf());
    let port = m.start_with(cfg(ServerKind::Tftp, root.path()), String::new()).await.unwrap();

    assert_eq!(tftp_get(port, "boot/image.bin", None).await.unwrap(), data);
    assert_eq!(tftp_get(port, "/boot/image.bin", Some(1428)).await.unwrap(), data);
    assert_eq!(tftp_get(port, "exact.bin", None).await.unwrap(), vec![7u8; 1024]); // ends with an empty block
    assert!(tftp_get(port, "absent.bin", None).await.is_err());
    assert!(tftp_get(port, "../../../etc/passwd", None).await.is_err());

    let up: Vec<u8> = (0..1300u32).map(|i| i as u8).collect();
    tftp_put(port, "envois/config.txt", &up).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(std::fs::read(root.path().join("envois").join("config.txt")).unwrap(), up);
    m.stop(ServerKind::Tftp).await;

    // read-only: uploads refused
    let mut c = cfg(ServerKind::Tftp, root.path());
    c.writable = false;
    let port = m.start_with(c, String::new()).await.unwrap();
    assert!(tftp_put(port, "x.txt", b"x").await.is_err());
    assert!(!root.path().join("x.txt").exists());
    let log = m.status().await.into_iter().find(|s| s.config.kind == ServerKind::Tftp).unwrap().log;
    assert!(log.iter().any(|l| l.msg.contains("a téléchargé /boot/image.bin (5000 octets)")), "{log:?}");
    m.stop(ServerKind::Tftp).await;
}

fn ftp(port: u16, user: &str, pw: &str) -> FtpParams {
    FtpParams { host: "127.0.0.1".into(), port, username: user.into(), password: pw.into(), tls: FtpTls::None, insecure: false }
}

#[tokio::test]
async fn ftp_server() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("lisezmoi.txt"), "FTP Orbiterm").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let m = ServerManager::new(dir.path().to_path_buf());
    let port = m.start_with(cfg(ServerKind::Ftp, root.path()), "s3cret".into()).await.unwrap();

    assert!(matches!(FtpConnection::connect(ftp(port, "orbi", "faux")).await, Err(AppError::AuthFailed)));
    assert!(FtpConnection::connect(ftp(port, "", "")).await.is_err(), "anonymous is off by default");
    let c = FtpConnection::connect(ftp(port, "orbi", "s3cret")).await.expect("login");
    let names: Vec<String> = c.list("/").await.unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, vec!["lisezmoi.txt"]);
    let local = tempfile::tempdir().unwrap();
    let src = local.path().join("envoi.bin");
    std::fs::write(&src, vec![42u8; 70_000]).unwrap();
    c.upload(&src, "/envoi.bin", &|_, _| {}).await.unwrap();
    assert_eq!(std::fs::read(root.path().join("envoi.bin")).unwrap().len(), 70_000);
    let dl = local.path().join("l.txt");
    c.download("/lisezmoi.txt", &dl, &|_, _| {}).await.unwrap();
    assert_eq!(std::fs::read_to_string(&dl).unwrap(), "FTP Orbiterm");
    c.close().await;
    m.stop(ServerKind::Ftp).await;

    // read-only + anonymous
    let mut c2 = cfg(ServerKind::Ftp, root.path());
    c2.writable = false;
    c2.anonymous = true;
    let port = m.start_with(c2, String::new()).await.unwrap();
    let a = FtpConnection::connect(ftp(port, "", "")).await.expect("anonymous");
    assert_eq!(a.list("/").await.unwrap().len(), 2);
    assert!(a.upload(&src, "/refus.bin", &|_, _| {}).await.is_err());
    assert!(a.mkdir("/nouveau").await.is_err());
    assert!(!root.path().join("refus.bin").exists() && !root.path().join("nouveau").exists());
    a.close().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let log = m.status().await.into_iter().find(|s| s.config.kind == ServerKind::Ftp).unwrap().log;
    assert!(log.iter().any(|l| l.msg.contains("orbi a envoyé")), "{log:?}");
    assert!(log.iter().any(|l| l.msg.contains("connexion refusée pour « orbi »")), "{log:?}");
    m.stop(ServerKind::Ftp).await;
}

#[tokio::test]
async fn ssh_server() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("partage.txt"), "SFTP Orbiterm").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let m = ServerManager::new(dir.path().to_path_buf());
    let mut c = cfg(ServerKind::Ssh, root.path());
    if cfg!(windows) {
        c.shell = "powershell".into();
    }
    assert!(m.start_with(c.clone(), String::new()).await.is_err(), "a password is mandatory");
    let port = m.start_with(c, "mdp-ssh".into()).await.unwrap();
    // the host key is created once and kept
    let key1 = std::fs::read(dir.path().join("ssh_server_ed25519")).unwrap();

    let known = dir.path().join("known_hosts");
    let asked = Arc::new(AtomicUsize::new(0));
    let a2 = asked.clone();
    let prompter: orbiterm_lib::ssh::HostKeyPrompter = Arc::new(move |_| {
        a2.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { true })
    });
    let p = |pw: &str| ConnectParams::simple("127.0.0.1", port, "orbi", Credentials::Password(pw.into()), known.clone());
    let r = SshConnection::connect(p("faux"), prompter.clone(), Arc::new(|_| {}), Arc::new(|_| {})).await;
    assert!(matches!(r, Err(AppError::AuthFailed)), "{:?}", r.err());

    let out = Arc::new(Mutex::new(Vec::<u8>::new()));
    let o = out.clone();
    let conn = SshConnection::connect(p("mdp-ssh"), prompter, Arc::new(move |b| o.lock().unwrap().extend(b)), Arc::new(|_| {}))
        .await
        .expect("ssh login");
    // interactive shell in a pseudo-terminal: compute something so the echo of the input
    // cannot match the expected output
    let cmd: &[u8] = if cfg!(windows) { b"[string](6*7)+'zz'\r" } else { b"echo $((6*7))zz\r" };
    // ConPTY first asks for the cursor position (ESC[6n) and waits: answer like a terminal
    for _ in 0..100 {
        if out.lock().unwrap().windows(4).any(|w| w == b"\x1b[6n") {
            conn.write(b"\x1b[1;1R").await.unwrap();
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    tokio::time::sleep(Duration::from_millis(1500)).await;
    conn.write(cmd).await.unwrap();
    let mut ok = false;
    for _ in 0..200 {
        if String::from_utf8_lossy(&out.lock().unwrap()).contains("42zz") {
            ok = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(ok, "shell output: {}", String::from_utf8_lossy(&out.lock().unwrap()));

    // SFTP confined to the shared folder
    let s = conn.sftp().await.unwrap();
    let names: Vec<String> = s.read_dir("/").await.unwrap().map(|e| e.file_name()).collect();
    assert!(names.contains(&"partage.txt".to_string()), "{names:?}");
    let mut f = s.open("/partage.txt").await.unwrap();
    let mut txt = String::new();
    f.read_to_string(&mut txt).await.unwrap();
    assert_eq!(txt, "SFTP Orbiterm");
    let mut w = s.create("/../../envoi-sftp.txt").await.unwrap();
    w.write_all(b"via sftp").await.unwrap();
    w.shutdown().await.unwrap();
    drop(w);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(std::fs::read_to_string(root.path().join("envoi-sftp.txt")).unwrap(), "via sftp");
    conn.disconnect().await;
    m.stop(ServerKind::Ssh).await;

    // second start: same host key, the client does not ask again
    let mut c = cfg(ServerKind::Ssh, root.path());
    c.port = port;
    c.writable = false;
    m.start_with(c, "mdp-ssh".into()).await.unwrap();
    assert_eq!(std::fs::read(dir.path().join("ssh_server_ed25519")).unwrap(), key1);
    let p2 = ConnectParams::simple("127.0.0.1", port, "orbi", Credentials::Password("mdp-ssh".into()), known.clone());
    let conn = SshConnection::connect(p2, Arc::new(|_| Box::pin(async { false })), Arc::new(|_| {}), Arc::new(|_| {}))
        .await
        .expect("known host key must not be asked again");
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    let s = conn.sftp().await.unwrap();
    assert!(s.create("/refus.txt").await.is_err(), "read-only SFTP must refuse uploads");
    conn.disconnect().await;
    let log = m.status().await.into_iter().find(|s| s.config.kind == ServerKind::Ssh).unwrap().log;
    assert!(log.iter().any(|l| l.msg.contains("connecté en tant que orbi")), "{log:?}");
    assert!(log.iter().any(|l| l.msg.contains("SFTP : envoi /envoi-sftp.txt")), "{log:?}");
    m.stop(ServerKind::Ssh).await;
}


#[tokio::test]
async fn port_scanner_and_wol() {
    use orbiterm_lib::nettools;
    let open = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let p = open.local_addr().unwrap().port();
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
    let total = nettools::scan_start("t".into(), "127.0.0.1", &format!("{p},{closed}"), 500, 64).await.unwrap();
    assert_eq!(total, 2);
    let mut st = None;
    for _ in 0..100 {
        let s = nettools::scan_state("t").unwrap();
        if s.finished {
            st = Some(s);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let st = st.expect("scan finished");
    assert_eq!(st.done, 2);
    let ports: Vec<u16> = st.hits.iter().map(|h| h.port).collect();
    assert_eq!(ports, vec![p]);
    nettools::scan_cancel("t");
    assert!(nettools::scan_state("t").is_none());

    // Wake-on-LAN magic packet, received on a local UDP port
    let rx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let port = rx.local_addr().unwrap().port();
    nettools::wol_send("01:23:45:67:89:ab", "127.0.0.1", port).await.unwrap();
    let mut buf = [0u8; 200];
    let n = tokio::time::timeout(Duration::from_secs(2), rx.recv(&mut buf)).await.unwrap().unwrap();
    assert_eq!(n, 102);
    assert_eq!(&buf[6..12], &[0x01, 0x23, 0x45, 0x67, 0x89, 0xab]);

    // this computer's sockets include the listener opened above
    let socks = nettools::open_ports().await.unwrap();
    assert!(socks.iter().any(|s| s.local_port == p && s.state == "ÉCOUTE"), "{} sockets", socks.len());
}
// ------------------------------------------------------------------ Telnet

async fn read_until(s: &mut TcpStream, acc: &mut Vec<u8>, needle: &str) -> String {
    let mut buf = [0u8; 4096];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let text = String::from_utf8_lossy(acc).into_owned();
        if text.contains(needle) {
            return text;
        }
        let n = tokio::time::timeout_at(deadline, s.read(&mut buf))
            .await
            .unwrap_or_else(|_| panic!("{needle:?} non reçu ; reçu : {text}"))
            .unwrap();
        assert!(n > 0, "fermé avant {needle:?} ; reçu : {text}");
        acc.extend(&buf[..n]);
    }
}

#[tokio::test]
async fn telnet_server() {
    let root = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let m = ServerManager::new(dir.path().to_path_buf());
    // no password: refused
    assert!(m.start_with(cfg(ServerKind::Telnet, root.path()), String::new()).await.is_err());
    let port = m.start_with(cfg(ServerKind::Telnet, root.path()), "pw-telnet".into()).await.unwrap();

    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut acc = vec![];
    read_until(&mut s, &mut acc, "login: ").await;
    assert!(acc.windows(3).any(|w| w == [255, 251, 1]), "IAC WILL ECHO attendu");
    // the client answers the negotiation and gives its window size
    s.write_all(&[255, 251, 31, 255, 250, 31, 0, 100, 0, 30, 255, 240]).await.unwrap();
    s.write_all(b"orbi\r\nwrong\r\n").await.unwrap();
    acc.clear();
    read_until(&mut s, &mut acc, "Login incorrect\r\n\r\nlogin: ").await;
    s.write_all(b"orbi\r\n").await.unwrap();
    acc.clear();
    read_until(&mut s, &mut acc, "Password: ").await;
    s.write_all(b"pw-telnet\r\n").await.unwrap();
    acc.clear();
    let mut buf = [0u8; 4096];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut sent = false;
    // typed command echoed by the shell + its output; like a terminal, answer the
    // cursor position requests (Windows ConPTY waits for one before starting)
    while String::from_utf8_lossy(&acc).matches("orbi-telnet-ok").count() < 2 {
        let n = match tokio::time::timeout(Duration::from_millis(1500), s.read(&mut buf)).await {
            Ok(r) => r.unwrap(),
            Err(_) => {
                assert!(tokio::time::Instant::now() < deadline, "sortie du shell : {:?}", String::from_utf8_lossy(&acc));
                if !sent {
                    s.write_all(b"echo orbi-telnet-ok\r\n").await.unwrap();
                    sent = true;
                }
                continue;
            }
        };
        assert!(n > 0, "{}", String::from_utf8_lossy(&acc));
        if buf[..n].windows(4).any(|w| w == b"\x1b[6n") {
            s.write_all(b"\x1b[1;1R").await.unwrap();
        }
        acc.extend(&buf[..n]);
    }
    s.write_all(b"exit\r\n").await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(10), s.read_to_end(&mut acc)).await;
    let st = m.status().await;
    let log: Vec<String> =
        st.iter().find(|s| s.config.kind == ServerKind::Telnet).unwrap().log.iter().map(|l| l.msg.clone()).collect();
    assert!(log.iter().any(|l| l.contains("refusée pour")), "{log:?}");
    assert!(log.iter().any(|l| l.contains("connecté en tant que orbi")), "{log:?}");
    m.stop(ServerKind::Telnet).await;
}

// ------------------------------------------------------------------ Iperf (minimal iperf3 client)

async fn json_msg(s: &mut TcpStream, v: &serde_json::Value) {
    let b = serde_json::to_vec(v).unwrap();
    s.write_all(&(b.len() as u32).to_be_bytes()).await.unwrap();
    s.write_all(&b).await.unwrap();
}

async fn read_msg(s: &mut TcpStream) -> serde_json::Value {
    let n = s.read_u32().await.unwrap() as usize;
    let mut b = vec![0u8; n];
    s.read_exact(&mut b).await.unwrap();
    serde_json::from_slice(&b).unwrap()
}

/// Runs one test the way iperf3 does; returns (bytes on the client side, server results).
async fn iperf_run(port: u16, streams: usize, reverse: bool) -> (u64, serde_json::Value) {
    let cookie = format!("{:<36}\0", format!("orbiterm-test-{streams}-{reverse}"));
    let mut ctl = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    ctl.write_all(cookie.as_bytes()).await.unwrap();
    assert_eq!(ctl.read_u8().await.unwrap(), 9); // PARAM_EXCHANGE
    json_msg(&mut ctl, &serde_json::json!({"tcp": true, "time": 1, "parallel": streams, "reverse": reverse, "len": 131072}))
        .await;
    assert_eq!(ctl.read_u8().await.unwrap(), 10); // CREATE_STREAMS
    let mut data = vec![];
    for _ in 0..streams {
        let mut d = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        d.write_all(cookie.as_bytes()).await.unwrap();
        data.push(d);
    }
    assert_eq!(ctl.read_u8().await.unwrap(), 1); // TEST_START
    assert_eq!(ctl.read_u8().await.unwrap(), 2); // TEST_RUNNING
    let mut tasks = vec![];
    for mut d in data {
        tasks.push(tokio::spawn(async move {
            let mut n = 0u64;
            let end = tokio::time::Instant::now() + Duration::from_millis(700);
            let mut buf = vec![7u8; 131072];
            while tokio::time::Instant::now() < end {
                if reverse {
                    match tokio::time::timeout_at(end, d.read(&mut buf)).await {
                        Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
                        Ok(Ok(k)) => n += k as u64,
                    }
                } else {
                    d.write_all(&buf).await.unwrap();
                    n += buf.len() as u64;
                }
            }
            if !reverse {
                d.shutdown().await.unwrap();
            }
            (n, d)
        }));
    }
    let mut total = 0;
    let mut keep = vec![];
    for t in tasks {
        let (n, d) = t.await.unwrap();
        total += n;
        keep.push(d);
    }
    ctl.write_u8(4).await.unwrap(); // TEST_END
    if reverse {
        // keep draining what is still in flight until the server stops
        for d in keep.iter_mut() {
            let mut buf = vec![0u8; 65536];
            while let Ok(Ok(k)) = tokio::time::timeout(Duration::from_millis(300), d.read(&mut buf)).await {
                if k == 0 {
                    break;
                }
                total += k as u64;
            }
        }
    }
    assert_eq!(ctl.read_u8().await.unwrap(), 13); // EXCHANGE_RESULTS
    json_msg(&mut ctl, &serde_json::json!({"cpu_util_total": 1.0, "streams": []})).await;
    let res = read_msg(&mut ctl).await;
    assert_eq!(ctl.read_u8().await.unwrap(), 14); // DISPLAY_RESULTS
    ctl.write_u8(16).await.unwrap(); // IPERF_DONE
    (total, res)
}

#[tokio::test]
async fn iperf_server() {
    let root = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let m = ServerManager::new(dir.path().to_path_buf());
    let port = m.start_with(cfg(ServerKind::Iperf, root.path()), String::new()).await.unwrap();

    // normal: the server receives exactly what the client sent
    let (sent, res) = iperf_run(port, 2, false).await;
    let streams = res["streams"].as_array().unwrap();
    assert_eq!(streams.iter().map(|s| s["id"].as_u64().unwrap()).collect::<Vec<_>>(), [1, 3]);
    let got: u64 = streams.iter().map(|s| s["bytes"].as_u64().unwrap()).sum();
    assert!(sent > 0 && got == sent, "envoyé {sent}, reçu {got}");

    // reverse: the server sends, the client counts
    let (recv, res) = iperf_run(port, 1, true).await;
    let sent_srv = res["streams"][0]["bytes"].as_u64().unwrap();
    assert!(recv > 0 && sent_srv >= recv, "serveur {sent_srv}, client {recv}");

    // UDP is refused with a server error
    let mut ctl = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    ctl.write_all(&[b'u'; 37]).await.unwrap();
    assert_eq!(ctl.read_u8().await.unwrap(), 9);
    json_msg(&mut ctl, &serde_json::json!({"udp": true, "time": 1})).await;
    assert_eq!(ctl.read_u8().await.unwrap(), 0xFE);

    let st = m.status().await;
    let log: Vec<String> =
        st.iter().find(|s| s.config.kind == ServerKind::Iperf).unwrap().log.iter().map(|l| l.msg.clone()).collect();
    assert!(log.iter().any(|l| l.contains("Mbit/s reçus")) && log.iter().any(|l| l.contains("Mbit/s envoyés")), "{log:?}");
    m.stop(ServerKind::Iperf).await;
}

// ------------------------------------------------------------------ VNC (screen sharing)

fn vnc_key(password: &str, challenge: &[u8]) -> Vec<u8> {
    use des::cipher::generic_array::GenericArray;
    use des::cipher::{BlockEncrypt, KeyInit};
    let mut key = [0u8; 8];
    for (i, b) in password.bytes().take(8).enumerate() {
        key[i] = b.reverse_bits();
    }
    let d = des::Des::new_from_slice(&key).unwrap();
    let mut out = challenge.to_vec();
    for c in out.chunks_mut(8) {
        d.encrypt_block(GenericArray::from_mut_slice(c));
    }
    out
}

/// RFB handshake; returns the stream after ServerInit with (width, height), or None if refused.
async fn vnc_login(port: u16, version: &[u8; 12], password: &str) -> Option<(TcpStream, u16, u16)> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut v = [0u8; 12];
    s.read_exact(&mut v).await.unwrap();
    assert_eq!(&v, b"RFB 003.008\n");
    s.write_all(version).await.unwrap();
    if version == b"RFB 003.003\n" {
        assert_eq!(s.read_u32().await.unwrap(), 2, "3.3: VNC authentication imposed");
    } else {
        let mut types = [0u8; 2];
        s.read_exact(&mut types).await.unwrap();
        assert_eq!(types, [1, 2]);
        s.write_all(&[2]).await.unwrap();
    }
    let mut ch = [0u8; 16];
    s.read_exact(&mut ch).await.unwrap();
    s.write_all(&vnc_key(password, &ch)).await.unwrap();
    if s.read_u32().await.unwrap() != 0 {
        return None;
    }
    s.write_all(&[1]).await.unwrap(); // shared
    let (w, h) = (s.read_u16().await.unwrap(), s.read_u16().await.unwrap());
    let mut pf = [0u8; 16];
    s.read_exact(&mut pf).await.unwrap();
    assert_eq!(pf[..4], [32, 24, 0, 1]);
    let n = s.read_u32().await.unwrap();
    let mut name = vec![0u8; n as usize];
    s.read_exact(&mut name).await.unwrap();
    assert!(String::from_utf8(name).unwrap().starts_with("Orbiterm"));
    Some((s, w, h))
}

async fn vnc_request(s: &mut TcpStream, incremental: bool, w: u16, h: u16) {
    let mut m = vec![3u8, incremental as u8, 0, 0, 0, 0];
    m.extend_from_slice(&w.to_be_bytes());
    m.extend_from_slice(&h.to_be_bytes());
    s.write_all(&m).await.unwrap();
}

/// Reads one FramebufferUpdate; decodes ZRLE (32 bpp, compact pixels) or raw (bpp bytes)
/// into `img` (0x00RRGGBB for 32 bpp). Returns the number of rectangles.
async fn vnc_update(s: &mut TcpStream, z: &mut flate2::Decompress, img: &mut [u32], fw: usize, bpp: usize) -> usize {
    let t = tokio::time::timeout(Duration::from_secs(10), s.read_u8()).await.expect("update timeout").unwrap();
    assert_eq!(t, 0, "FramebufferUpdate");
    s.read_u8().await.unwrap();
    let n = s.read_u16().await.unwrap() as usize;
    for _ in 0..n {
        let (x, y, w, h) = (
            s.read_u16().await.unwrap() as usize,
            s.read_u16().await.unwrap() as usize,
            s.read_u16().await.unwrap() as usize,
            s.read_u16().await.unwrap() as usize,
        );
        match s.read_i32().await.unwrap() {
            0 => {
                let mut raw = vec![0u8; w * h * bpp];
                s.read_exact(&mut raw).await.unwrap();
                if bpp == 4 {
                    for (i, p) in raw.chunks_exact(4).enumerate() {
                        img[(y + i / w) * fw + x + i % w] = u32::from_le_bytes([p[0], p[1], p[2], 0]);
                    }
                }
            }
            16 => {
                let len = s.read_u32().await.unwrap() as usize;
                let mut comp = vec![0u8; len];
                s.read_exact(&mut comp).await.unwrap();
                let mut data = Vec::with_capacity(w * h * 3 + 64 * 64);
                z.decompress_vec(&comp, &mut data, flate2::FlushDecompress::Sync).unwrap();
                let mut i = 0;
                let px = |i: usize| u32::from_le_bytes([data[i], data[i + 1], data[i + 2], 0]);
                for ty in (y..y + h).step_by(64) {
                    for tx in (x..x + w).step_by(64) {
                        let (tw, th) = (64.min(x + w - tx), 64.min(y + h - ty));
                        let sub = data[i];
                        assert!(sub <= 1, "subencoding {sub}");
                        i += 1;
                        for r in 0..th {
                            for c in 0..tw {
                                img[(ty + r) * fw + tx + c] = if sub == 1 { px(i) } else { px(i + (r * tw + c) * 3) };
                            }
                        }
                        i += if sub == 1 { 3 } else { tw * th * 3 };
                    }
                }
                assert_eq!(i, data.len(), "ZRLE data fully used");
            }
            e => panic!("encoding {e}"),
        }
    }
    n
}

#[tokio::test]
async fn vnc_server() {
    let root = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let m = ServerManager::new(dir.path().to_path_buf());
    let c = cfg(ServerKind::Vnc, root.path());
    assert!(!c.writable, "view only by default");
    assert!(m.start_with(c.clone(), String::new()).await.is_err(), "a password is mandatory");
    let port = match m.start_with(c, "vnc-pw".into()).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("pas d'écran à partager ici : test ignoré ({e})");
            return;
        }
    };

    assert!(vnc_login(port, b"RFB 003.008\n", "faux").await.is_none(), "wrong password refused");

    // RFB 3.8, ZRLE: a full picture, then incremental updates of the changes only
    let (mut s, w, h) = vnc_login(port, b"RFB 003.008\n", "vnc-pw").await.expect("login");
    assert!(w > 100 && h > 100);
    let mut enc = vec![2u8, 0, 0, 2];
    for e in [16i32, 0] {
        enc.extend_from_slice(&e.to_be_bytes());
    }
    s.write_all(&enc).await.unwrap();
    let (fw, fh) = (w as usize, h as usize);
    let mut img = vec![0xDEAD_BEEFu32; fw * fh];
    let mut z = flate2::Decompress::new(true);
    vnc_request(&mut s, false, w, h).await;
    let t0 = std::time::Instant::now();
    assert!(vnc_update(&mut s, &mut z, &mut img, fw, 4).await >= 1);
    eprintln!("écran {w}×{h} reçu en {:?}", t0.elapsed());
    assert!(img.iter().all(|p| *p <= 0xFF_FFFF), "every pixel received");
    let distinct = img.iter().step_by(97).collect::<std::collections::HashSet<_>>().len();
    assert!(distinct > 4, "a real picture ({distinct} colours)");
    // the next incremental request is answered now or when something changes (e.g. a clock)
    vnc_request(&mut s, true, w, h).await;
    let _ = tokio::time::timeout(Duration::from_secs(3), vnc_update(&mut s, &mut z, &mut img, fw, 4)).await;
    // view only: pointer and clipboard messages are accepted and ignored, the session goes on
    s.write_all(&[5, 0, 0, 10, 0, 10]).await.unwrap();
    s.write_all(&[6, 0, 0, 0, 0, 0, 0, 3, b'a', b'b', b'c']).await.unwrap();
    vnc_request(&mut s, false, 64, 64).await;
    assert!(vnc_update(&mut s, &mut z, &mut img, fw, 4).await >= 1);
    drop(s);

    // RFB 3.3, raw pixels in 16 bits (RGB565)
    let (mut s, w, h) = vnc_login(port, b"RFB 003.003\n", "vnc-pw").await.expect("login 3.3");
    s.write_all(&[0u8, 0, 0, 0, 16, 16, 0, 1, 0, 31, 0, 63, 0, 31, 11, 5, 0, 0, 0, 0]).await.unwrap();
    vnc_request(&mut s, false, w.min(200), h.min(100)).await;
    let mut tmp = vec![0u32; fw * fh];
    assert_eq!(vnc_update(&mut s, &mut z, &mut tmp, fw, 2).await, 1);
    drop(s);

    tokio::time::sleep(Duration::from_millis(300)).await;
    let st = m.status().await;
    let log: Vec<String> = st.iter().find(|s| s.config.kind == ServerKind::Vnc).unwrap().log.iter().map(|l| l.msg.clone()).collect();
    assert!(log.iter().any(|l| l.contains("mot de passe refusé")), "{log:?}");
    assert!(log.iter().any(|l| l.contains("affichage seul")), "{log:?}");
    m.stop_all().await;
}
