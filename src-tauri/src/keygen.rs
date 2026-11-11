//! SSH key generator: creates Ed25519 / ECDSA / RSA keys, loads existing private keys
//! (OpenSSH or PuTTY .ppk) and saves them in either format, optionally protected by a
//! passphrase. The key being edited stays in this process only (never sent to the UI).

use std::path::Path;
use std::sync::Mutex;

use russh::keys::ssh_key::private::{KeypairData, RsaKeypair};
use russh::keys::ssh_key::{EcdsaCurve, HashAlg, LineEnding};
use russh::keys::{Algorithm, PrivateKey};
use serde::Serialize;

use crate::error::{AppError, AppResult};

/// Key currently shown in the generator window.
static CURRENT: Mutex<Option<PrivateKey>> = Mutex::new(None);

#[derive(Serialize, Clone, Debug)]
pub struct KeyInfo {
    /// e.g. "ssh-ed25519", "ecdsa-sha2-nistp256", "ssh-rsa"
    pub algorithm: String,
    pub bits: u32,
    pub comment: String,
    /// one line for authorized_keys
    pub public_openssh: String,
    /// SHA256:…
    pub fingerprint: String,
    pub randomart: String,
}

fn err(e: impl std::fmt::Display) -> AppError {
    AppError::Config(format!("clé : {e}"))
}

/// Generates a key. `kind`: ed25519, ecdsa256, ecdsa384, ecdsa521, rsa2048, rsa3072, rsa4096.
pub fn generate(kind: &str, comment: &str) -> AppResult<PrivateKey> {
    let mut rng = rand::rng();
    let key = match kind {
        "ed25519" => loop {
            // seeds whose first / last byte is 0 (or first 0xff) are encoded ambiguously in
            // .ppk files by some readers: draw another one
            let k = PrivateKey::random(&mut rng, Algorithm::Ed25519).map_err(err)?;
            let seed = k.key_data().ed25519().map(|e| e.private.to_bytes()).unwrap_or_default();
            if !matches!(seed[0], 0 | 0xff) && seed[31] != 0 {
                break k;
            }
        },
        "ecdsa256" | "ecdsa384" | "ecdsa521" => {
            let curve = match kind {
                "ecdsa256" => EcdsaCurve::NistP256,
                "ecdsa384" => EcdsaCurve::NistP384,
                _ => EcdsaCurve::NistP521,
            };
            PrivateKey::random(&mut rng, Algorithm::Ecdsa { curve }).map_err(err)?
        }
        k if k.starts_with("rsa") => {
            let bits: usize = k[3..].parse().map_err(|_| err(format!("type inconnu {k}")))?;
            if !(2048..=8192).contains(&bits) {
                return Err(err("taille RSA entre 2048 et 8192 bits"));
            }
            let pair = RsaKeypair::random(&mut rng, bits).map_err(err)?;
            PrivateKey::new(KeypairData::from(pair), "").map_err(err)?
        }
        k => return Err(err(format!("type inconnu {k}"))),
    };
    let mut key = key;
    key.set_comment(comment);
    Ok(key)
}

/// Loads a private key file (OpenSSH, PEM or PuTTY .ppk v2/v3), decrypting it when needed.
pub fn load(path: &Path, passphrase: Option<&str>) -> AppResult<PrivateKey> {
    let text = std::fs::read_to_string(path)?;
    let pass = passphrase.filter(|p| !p.is_empty());
    if text.starts_with("PuTTY-User-Key-File-") {
        let encrypted = text.lines().any(|l| l.trim() == "Encryption: aes256-cbc");
        if encrypted && pass.is_none() {
            return Err(AppError::NeedPassphrase);
        }
        return PrivateKey::from_ppk(&text, pass.map(str::to_string)).map_err(|e| match e.to_string() {
            m if m.contains("MAC") => AppError::Config("phrase secrète incorrecte".into()),
            m => err(m),
        });
    }
    let key = russh::keys::decode_secret_key(&text, pass).map_err(|e| match e {
        russh::keys::Error::KeyIsEncrypted => AppError::NeedPassphrase,
        e if pass.is_some() && e.to_string().to_lowercase().contains("decrypt") => {
            AppError::Config("phrase secrète incorrecte".into())
        }
        e => err(e),
    })?;
    Ok(key)
}

pub fn info(key: &PrivateKey) -> KeyInfo {
    let public = key.public_key();
    let fp = key.fingerprint(HashAlg::Sha256);
    let bits = match key.key_data() {
        KeypairData::Ed25519(_) => 256,
        KeypairData::Ecdsa(e) => match e.curve() {
            EcdsaCurve::NistP256 => 256,
            EcdsaCurve::NistP384 => 384,
            EcdsaCurve::NistP521 => 521,
        },
        KeypairData::Rsa(r) => r.key_size(),
        _ => 0,
    };
    let algorithm = public.algorithm().to_string();
    KeyInfo {
        algorithm: algorithm.clone(),
        bits,
        comment: key.comment().to_string(),
        public_openssh: public.to_openssh().unwrap_or_default(),
        fingerprint: fp.to_string(),
        randomart: fp.to_randomart(&format!("{} {bits}", algo_label(&algorithm))),
    }
}

fn algo_label(a: &str) -> &'static str {
    match a {
        "ssh-ed25519" => "ED25519",
        "ssh-rsa" => "RSA",
        a if a.starts_with("ecdsa") => "ECDSA",
        _ => "KEY",
    }
}

/// Private key in the OpenSSH format (bcrypt + AES-256-CTR when a passphrase is given).
pub fn to_openssh(key: &PrivateKey, passphrase: &str) -> AppResult<String> {
    let k = if passphrase.is_empty() { key.clone() } else { key.encrypt(&mut rand::rng(), passphrase).map_err(err)? };
    Ok(k.to_openssh(if cfg!(windows) { LineEnding::CRLF } else { LineEnding::LF }).map_err(err)?.to_string())
}

// ---- PuTTY .ppk (format 3) ----

fn put_string(out: &mut Vec<u8>, b: &[u8]) {
    out.extend_from_slice(&(b.len() as u32).to_be_bytes());
    out.extend_from_slice(b);
}

/// SSH mpint of a positive big-endian integer.
fn put_mpint(out: &mut Vec<u8>, b: &[u8]) {
    let b = &b[b.iter().take_while(|&&x| x == 0).count()..];
    if b.first().is_some_and(|&x| x & 0x80 != 0) {
        let mut v = vec![0];
        v.extend_from_slice(b);
        put_string(out, &v);
    } else {
        put_string(out, b);
    }
}

fn private_blob(key: &PrivateKey) -> AppResult<Vec<u8>> {
    let mut b = Vec::new();
    match key.key_data() {
        // PuTTY stores the 32-byte seed as a little-endian integer, written as is
        KeypairData::Ed25519(k) => put_string(&mut b, &k.private.to_bytes()),
        KeypairData::Ecdsa(k) => put_mpint(&mut b, k.private_key_bytes()),
        KeypairData::Rsa(k) => {
            for m in [k.private().d(), k.private().p(), k.private().q(), k.private().iqmp()] {
                put_mpint(&mut b, m.as_positive_bytes().ok_or_else(|| err("RSA invalide"))?);
            }
        }
        _ => return Err(AppError::Unsupported("type de clé non exportable en .ppk".into())),
    }
    Ok(b)
}

fn base64_lines(data: &[u8]) -> Vec<String> {
    use base64::Engine as _;
    let s = base64::engine::general_purpose::STANDARD.encode(data);
    s.as_bytes().chunks(64).map(|c| String::from_utf8_lossy(c).to_string()).collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Private key in PuTTY's .ppk format 3 (Argon2id + AES-256-CBC when a passphrase is given).
pub fn to_ppk(key: &PrivateKey, passphrase: &str) -> AppResult<String> {
    use hmac::{Hmac, KeyInit, Mac};
    let algorithm = key.public_key().algorithm().to_string();
    let comment = key.comment().to_string();
    let public = key.public_key().to_bytes().map_err(err)?;
    let mut private = private_blob(key)?;
    let encrypted = !passphrase.is_empty();
    let mut header = vec![
        format!("PuTTY-User-Key-File-3: {algorithm}"),
        format!("Encryption: {}", if encrypted { "aes256-cbc" } else { "none" }),
        format!("Comment: {comment}"),
    ];
    let mut mac_key = Vec::new();
    let mut aes = None;
    if encrypted {
        let salt: [u8; 16] = rand::random();
        let (mem, passes, par) = (8192u32, 21u32, 1u32);
        let params = argon2::Params::new(mem, passes, par, Some(80)).map_err(err)?;
        let mut out = [0u8; 80];
        argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
            .hash_password_into(passphrase.as_bytes(), &salt, &mut out)
            .map_err(err)?;
        header.push("Key-Derivation: Argon2id".into());
        header.push(format!("Argon2-Memory: {mem}"));
        header.push(format!("Argon2-Passes: {passes}"));
        header.push(format!("Argon2-Parallelism: {par}"));
        header.push(format!("Argon2-Salt: {}", hex(&salt)));
        mac_key = out[48..80].to_vec();
        aes = Some((out[..32].to_vec(), out[32..48].to_vec()));
        // pad to the cipher block size with random bytes
        while private.len() % 16 != 0 {
            private.push(rand::random());
        }
    }
    // MAC over the plaintext (padded) private blob
    let mut mac_data = Vec::new();
    put_string(&mut mac_data, algorithm.as_bytes());
    put_string(&mut mac_data, if encrypted { b"aes256-cbc" } else { b"none" });
    put_string(&mut mac_data, comment.as_bytes());
    put_string(&mut mac_data, &public);
    put_string(&mut mac_data, &private);
    let mut mac = <Hmac<sha2_11::Sha256> as KeyInit>::new_from_slice(&mac_key).map_err(err)?;
    mac.update(&mac_data);
    let mac = mac.finalize().into_bytes();
    if let Some((k, iv)) = aes {
        ssh_cipher::Cipher::Aes256Cbc.encrypt(&k, &iv, &mut private).map_err(err)?;
    }
    let mut lines = header;
    let pl = base64_lines(&public);
    lines.push(format!("Public-Lines: {}", pl.len()));
    lines.extend(pl);
    let vl = base64_lines(&private);
    lines.push(format!("Private-Lines: {}", vl.len()));
    lines.extend(vl);
    lines.push(format!("Private-MAC: {}", hex(&mac)));
    let nl = if cfg!(windows) { "\r\n" } else { "\n" };
    Ok(lines.join(nl) + nl)
}

/// Writes a private key file, readable by its owner only on Unix.
fn write_private(path: &Path, text: &str) -> AppResult<()> {
    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
        f.write_all(text.as_bytes())?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, text)?;
        Ok(())
    }
}

fn with_current<T>(f: impl FnOnce(&mut PrivateKey) -> AppResult<T>) -> AppResult<T> {
    let mut g = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
    let key = g.as_mut().ok_or_else(|| AppError::Config("aucune clé générée ou chargée".into()))?;
    f(key)
}

fn set_current(key: PrivateKey) -> KeyInfo {
    let i = info(&key);
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(key);
    i
}

// ---- Tauri commands ----

#[tauri::command]
pub async fn key_generate(kind: String, comment: String) -> AppResult<KeyInfo> {
    let key = tokio::task::spawn_blocking(move || generate(&kind, &comment)).await.map_err(err)??;
    Ok(set_current(key))
}

#[tauri::command]
pub async fn key_load(path: String, passphrase: Option<String>) -> AppResult<KeyInfo> {
    let key = tokio::task::spawn_blocking(move || load(Path::new(&path), passphrase.as_deref())).await.map_err(err)??;
    Ok(set_current(key))
}

#[tauri::command]
pub fn key_set_comment(comment: String) -> AppResult<KeyInfo> {
    with_current(|k| {
        k.set_comment(comment.trim());
        Ok(info(k))
    })
}

/// Saves the private key: `format` = "openssh" | "ppk".
#[tauri::command]
pub async fn key_save_private(path: String, format: String, passphrase: String) -> AppResult<()> {
    let key = with_current(|k| Ok(k.clone()))?;
    tokio::task::spawn_blocking(move || {
        let text = match format.as_str() {
            "ppk" => to_ppk(&key, &passphrase)?,
            _ => to_openssh(&key, &passphrase)?,
        };
        write_private(Path::new(&path), &text)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub fn key_save_public(path: String) -> AppResult<()> {
    let line = with_current(|k| Ok(info(k).public_openssh))?;
    std::fs::write(path, line + "\n")?;
    Ok(())
}

/// Forgets the key held by the generator (window closed).
#[tauri::command]
pub fn key_forget() {
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(kind: &str) {
        let k = generate(kind, "test@orbiterm").unwrap();
        let i = info(&k);
        assert!(i.public_openssh.ends_with(" test@orbiterm"), "{}", i.public_openssh);
        assert!(i.fingerprint.starts_with("SHA256:"));
        let dir = std::env::temp_dir().join(format!("orb-keygen-{kind}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (fmt, pass) in [("openssh", ""), ("openssh", "s3cret"), ("ppk", ""), ("ppk", "s3cret")] {
            let text = if fmt == "ppk" { to_ppk(&k, pass).unwrap() } else { to_openssh(&k, pass).unwrap() };
            let p = dir.join(format!("k.{fmt}"));
            write_private(&p, &text).unwrap();
            if !pass.is_empty() {
                assert!(matches!(load(&p, None), Err(AppError::NeedPassphrase)), "{kind} {fmt}: passphrase demandée");
                assert!(load(&p, Some("faux")).is_err(), "{kind} {fmt}: mauvaise phrase refusée");
            }
            let back = load(&p, Some(pass)).unwrap_or_else(|e| panic!("{kind} {fmt} {pass:?}: {e}"));
            assert_eq!(back.public_key().key_data(), k.public_key().key_data(), "{kind} {fmt}");
            assert_eq!(back.key_data(), k.key_data(), "{kind} {fmt}: partie privée");
            assert_eq!(back.comment().to_string(), "test@orbiterm");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ed25519_roundtrip() {
        for _ in 0..3 {
            roundtrip("ed25519");
        }
    }

    #[test]
    fn ecdsa_roundtrip() {
        roundtrip("ecdsa256");
        roundtrip("ecdsa384");
        roundtrip("ecdsa521");
    }

    #[test]
    fn rsa_roundtrip() {
        roundtrip("rsa2048");
        assert_eq!(info(&generate("rsa2048", "").unwrap()).bits, 2048);
        assert!(generate("rsa1024", "").is_err());
    }
}
