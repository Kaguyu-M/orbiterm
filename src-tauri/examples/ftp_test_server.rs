//! FTP test server (libunftp), for manual tests and the end-to-end tests.
//!
//! usage: cargo run --example ftp_test_server -- [port] [root]
//! users: test / test, or anonymous (any password)

use std::path::PathBuf;
use std::sync::Arc;

use libunftp::ServerBuilder;
use unftp_core::auth::{AuthenticationError, Authenticator, Credentials, Principal};
use unftp_sbe_fs::Filesystem;

#[derive(Debug)]
pub struct TestAuth;

#[async_trait::async_trait]
impl Authenticator for TestAuth {
    async fn authenticate(&self, username: &str, creds: &Credentials) -> Result<Principal, AuthenticationError> {
        let ok = username == "anonymous" || (username == "test" && creds.password.as_deref() == Some("test"));
        if ok { Ok(Principal { username: username.to_string() }) } else { Err(AuthenticationError::BadPassword) }
    }
}

/// Serves `root` on 127.0.0.1:`port` until the task is dropped.
pub async fn serve(root: PathBuf, port: u16) {
    let server = ServerBuilder::new(Box::new(move || Filesystem::new(root.clone()).expect("ftp root")))
        .authenticator(Arc::new(TestAuth))
        .greeting("Serveur FTP de test Orbiterm")
        .passive_host([127, 0, 0, 1])
        .passive_ports(50100..=50200)
        .build()
        .expect("ftp server");
    let _ = server.listen(format!("127.0.0.1:{port}")).await;
}

#[allow(dead_code)]
#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let port: u16 = args.next().and_then(|p| p.parse().ok()).unwrap_or(2121);
    let root = args.next().map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("orbiterm-ftp-test"));
    std::fs::create_dir_all(&root).expect("root dir");
    println!("Serveur FTP de test prêt sur 127.0.0.1:{port}");
    println!("  utilisateur : test   mot de passe : test   (ou anonymous)");
    println!("  dossier     : {}", root.display());
    serve(root, port).await;
}
