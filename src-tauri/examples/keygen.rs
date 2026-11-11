//! Key generator from the command line (same code as Tools → SSH key generator):
//!
//!   cargo run --example keygen -- <ed25519|ecdsa256|ecdsa384|ecdsa521|rsa2048|rsa3072|rsa4096> <out> [comment]
//!
//! Writes <out> (OpenSSH), <out>.ppk (PuTTY) and <out>.pub; passphrase from ORB_KEY_PASS (none if unset).

use orbiterm_lib::keygen;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(kind), Some(out)) = (args.first(), args.get(1)) else {
        eprintln!("usage : keygen <type> <fichier> [commentaire]");
        std::process::exit(2);
    };
    let pass = std::env::var("ORB_KEY_PASS").unwrap_or_default();
    let key = keygen::generate(kind, args.get(2).map(String::as_str).unwrap_or("")).expect("génération");
    std::fs::write(out, keygen::to_openssh(&key, &pass).unwrap()).unwrap();
    std::fs::write(format!("{out}.ppk"), keygen::to_ppk(&key, &pass).unwrap()).unwrap();
    let info = keygen::info(&key);
    std::fs::write(format!("{out}.pub"), format!("{}
", info.public_openssh)).unwrap();
    println!("{}
{}", info.fingerprint, info.randomart);
}
