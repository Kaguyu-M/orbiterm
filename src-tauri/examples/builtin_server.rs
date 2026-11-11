//! Runs one of Orbiterm's built-in servers from the command line, on 127.0.0.1, to try it
//! with real clients (iperf3, telnet, ftp…) without the interface:
//!
//!   cargo run --example builtin_server -- <http|tftp|ftp|ssh|telnet|iperf|vnc> <port> [root]
//!
//! The password of FTP / SSH / Telnet / VNC is read from the environment variable
//! ORB_SERVER_PW (user: ORB_SERVER_USER, default "orbi"). VNC: ORB_SERVER_CONTROL=1 allows
//! keyboard and mouse. SSH: ORB_SERVER_KEYS = authorized_keys file. Stops with Ctrl+C.

use orbiterm_lib::servers::{ServerConfig, ServerKind, ServerManager};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let kind: ServerKind = serde_json::from_value(serde_json::Value::String(args.first().cloned().unwrap_or_default()))
        .expect("type de serveur : http, tftp, ftp, ssh, telnet, iperf ou vnc");
    let mut cfg = ServerConfig::new(kind);
    cfg.port = args.get(1).and_then(|p| p.parse().ok()).unwrap_or(0);
    cfg.local_only = true;
    let root = args.get(2).cloned().unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned());
    cfg.root = root;
    if kind == ServerKind::Vnc {
        cfg.writable = std::env::var("ORB_SERVER_CONTROL").as_deref() == Ok("1");
    }
    if let Ok(p) = std::env::var("ORB_SERVER_KEYS") {
        cfg.authorized_keys = std::fs::read_to_string(p).expect("fichier de clés autorisées");
    }
    cfg.user = std::env::var("ORB_SERVER_USER").unwrap_or_else(|_| "orbi".into());
    let dir = tempfile::tempdir().unwrap();
    let m = ServerManager::new(dir.path().to_path_buf());
    let port = m.start_with(cfg, std::env::var("ORB_SERVER_PW").unwrap_or_default()).await.expect("démarrage");
    println!("serveur {} sur 127.0.0.1:{port}", kind.label());
    let mut seen = 0;
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tokio::time::sleep(std::time::Duration::from_millis(300)) => {}
        }
        let st = m.status().await;
        if let Some(s) = st.iter().find(|s| s.config.kind == kind) {
            for l in s.log.iter().skip(seen) {
                println!("{}", l.msg);
            }
            seen = s.log.len();
        }
    }
    m.stop_all().await;
}
