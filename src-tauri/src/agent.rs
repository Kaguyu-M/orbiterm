//! SSH agent support, like MobaXterm's MobAgent / Pageant integration:
//!
//! - an internal agent: private keys loaded (and decrypted) in this process for the
//!   lifetime of Orbiterm, never written anywhere;
//! - the system agent: OpenSSH agent (named pipe) or Pageant on Windows, `SSH_AUTH_SOCK`
//!   elsewhere;
//! - authentication with the keys of both agents (before asking for a password);
//! - agent forwarding: forwarded channels are answered by [`serve`], which offers the internal
//!   keys and relays everything else to the system agent.

use std::sync::{Arc, RwLock};

use russh::keys::agent::AgentIdentity;
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::ssh_encoding::Encode;
use russh::keys::{Algorithm, HashAlg, PrivateKey, PublicKey};
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{AppError, AppResult};

const FAILURE: u8 = 5;
const REQUEST_IDENTITIES: u8 = 11;
const IDENTITIES_ANSWER: u8 = 12;
const SIGN_REQUEST: u8 = 13;
const SIGN_RESPONSE: u8 = 14;
const MAX_FRAME: usize = 256 * 1024;

struct Loaded {
    key: Arc<PrivateKey>,
    source: String,
}

/// Keys of the internal agent.
static KEYS: RwLock<Vec<Loaded>> = RwLock::new(Vec::new());

#[derive(Serialize, Clone, Debug)]
pub struct AgentKey {
    pub fingerprint: String,
    pub algorithm: String,
    pub comment: String,
    /// file the key was loaded from ("" for keys of the system agent)
    pub source: String,
    /// true: internal agent, false: system agent
    pub internal: bool,
}

fn info(key: &PublicKey, comment: &str, source: &str, internal: bool) -> AgentKey {
    AgentKey {
        fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
        algorithm: key.algorithm().to_string(),
        comment: comment.to_string(),
        source: source.to_string(),
        internal,
    }
}

/// Adds a key to the internal agent (replaces the same key loaded before).
pub fn add(key: PrivateKey, source: &str) -> AgentKey {
    let i = info(key.public_key(), key.comment().as_ref(), source, true);
    let mut keys = KEYS.write().unwrap_or_else(|e| e.into_inner());
    keys.retain(|l| l.key.public_key().key_data() != key.public_key().key_data());
    keys.push(Loaded { key: Arc::new(key), source: source.to_string() });
    i
}

pub fn remove(fingerprint: &str) {
    KEYS.write()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|l| l.key.fingerprint(HashAlg::Sha256).to_string() != fingerprint);
}

pub fn clear() {
    KEYS.write().unwrap_or_else(|e| e.into_inner()).clear();
}

fn internal_keys() -> Vec<Arc<PrivateKey>> {
    KEYS.read().unwrap_or_else(|e| e.into_inner()).iter().map(|l| l.key.clone()).collect()
}

fn internal_list() -> Vec<AgentKey> {
    KEYS.read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|l| info(l.key.public_key(), l.key.comment().as_ref(), &l.source, true))
        .collect()
}

type Dyn = AgentClient<Box<dyn AgentStream + Send + Unpin + 'static>>;

/// Connection to the system agent, if one is running.
pub async fn system() -> Option<Dyn> {
    #[cfg(windows)]
    {
        if let Ok(Ok(c)) = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent"),
        )
        .await
        {
            return Some(c.dynamic());
        }
        // connecting to Pageant always succeeds: check that it answers
        if let Ok(Ok(mut c)) = tokio::time::timeout(std::time::Duration::from_secs(2), AgentClient::connect_pageant()).await
            && let Ok(Ok(_)) = tokio::time::timeout(std::time::Duration::from_secs(2), c.request_identities()).await
        {
            return Some(c.dynamic());
        }
        None
    }
    #[cfg(unix)]
    {
        match tokio::time::timeout(std::time::Duration::from_secs(2), AgentClient::connect_env()).await {
            Ok(Ok(c)) => Some(c.dynamic()),
            _ => None,
        }
    }
    #[cfg(not(any(windows, unix)))]
    None
}

/// Name of the system agent found (for the settings window).
pub async fn system_name() -> Option<&'static str> {
    #[cfg(windows)]
    {
        if tokio::net::windows::named_pipe::ClientOptions::new().open(r"\\.\pipe\openssh-ssh-agent").is_ok() {
            return Some("agent OpenSSH de Windows");
        }
        system().await.map(|_| "Pageant")
    }
    #[cfg(not(windows))]
    {
        system().await.map(|_| "agent SSH du système (SSH_AUTH_SOCK)")
    }
}

/// Keys of both agents.
pub async fn list() -> Vec<AgentKey> {
    let mut out = internal_list();
    if let Some(mut c) = system().await
        && let Ok(ids) = c.request_identities().await
    {
        for id in ids {
            if let AgentIdentity::PublicKey { key, comment } = id
                && !out.iter().any(|k| k.fingerprint == key.fingerprint(HashAlg::Sha256).to_string())
            {
                out.push(info(&key, &comment, "", false));
            }
        }
    }
    out
}

/// True when one of the agents holds at least one key.
pub async fn has_keys() -> bool {
    if !internal_keys().is_empty() {
        return true;
    }
    match system().await {
        Some(mut c) => c.request_identities().await.is_ok_and(|ids| !ids.is_empty()),
        None => false,
    }
}

/// SSH signature (as the agent protocol and russh expect it) of `data` with an internal key.
fn sign(key: &PrivateKey, hash: Option<HashAlg>, data: &[u8]) -> AppResult<Vec<u8>> {
    use russh::keys::signature::Signer;
    let sig: russh::keys::ssh_key::Signature = match key.key_data() {
        russh::keys::ssh_key::private::KeypairData::Rsa(rsa) => {
            Signer::try_sign(&(rsa, hash), data).map_err(|e| AppError::Ssh(e.to_string()))?
        }
        _ => Signer::try_sign(key, data).map_err(|e| AppError::Ssh(e.to_string()))?,
    };
    let mut blob = Vec::new();
    sig.encode(&mut blob).map_err(|e| AppError::Ssh(e.to_string()))?;
    Ok(blob)
}

/// russh signer that uses the internal keys, or the system agent for its own keys.
struct Signer {
    system: Option<Dyn>,
}

impl russh::Signer for Signer {
    type Error = russh::AgentAuthError;

    async fn auth_sign(&mut self, id: &AgentIdentity, hash: Option<HashAlg>, mut to_sign: Vec<u8>) -> Result<Vec<u8>, Self::Error> {
        let AgentIdentity::PublicKey { key, .. } = id else {
            return Err(russh::keys::Error::AgentFailure.into());
        };
        if let Some(k) = internal_keys().into_iter().find(|k| k.public_key().key_data() == key.key_data()) {
            let blob = sign(&k, hash, &to_sign).map_err(|_| russh::keys::Error::AgentFailure)?;
            (blob.len() as u32).encode(&mut to_sign).map_err(russh::keys::Error::from)?;
            to_sign.extend_from_slice(&blob);
            return Ok(to_sign);
        }
        match self.system.as_mut() {
            Some(c) => Ok(c.sign_request(id, hash, to_sign).await?),
            None => Err(russh::keys::Error::AgentFailure.into()),
        }
    }
}

/// Tries every key of both agents. Ok(true) when the server accepted one.
pub async fn authenticate<H: russh::client::Handler>(handle: &mut russh::client::Handle<H>, user: &str) -> AppResult<bool> {
    let mut ids: Vec<AgentIdentity> = internal_keys()
        .iter()
        .map(|k| AgentIdentity::PublicKey { key: k.public_key().clone(), comment: k.comment().to_string() })
        .collect();
    let mut system = system().await;
    if let Some(c) = system.as_mut()
        && let Ok(sys) = c.request_identities().await
    {
        for id in sys {
            if let AgentIdentity::PublicKey { key, .. } = &id
                && ids.iter().any(|i| matches!(i, AgentIdentity::PublicKey { key: k, .. } if k.key_data() == key.key_data()))
            {
                continue;
            }
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return Ok(false);
    }
    let mut signer = Signer { system };
    let rsa_hash = handle.best_supported_rsa_hash().await.ok().flatten().flatten();
    for id in ids {
        let AgentIdentity::PublicKey { key, .. } = &id else { continue };
        let hash = if matches!(key.algorithm(), Algorithm::Rsa { .. }) { rsa_hash } else { None };
        match handle.authenticate_publickey_with(user, key.clone(), hash, &mut signer).await {
            Ok(r) if r.success() => return Ok(true),
            Ok(_) => {}
            // the agent declined (e.g. a locked key): try the next one
            Err(e) => log::debug!("agent : {e}"),
        }
    }
    Ok(false)
}

// ---- agent protocol server (forwarded channels) ----

async fn read_frame<S: AsyncRead + Unpin>(s: &mut S) -> std::io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match s.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let n = u32::from_be_bytes(len) as usize;
    if n == 0 || n > MAX_FRAME {
        return Err(std::io::Error::other("trame d'agent invalide"));
    }
    let mut buf = vec![0u8; n];
    s.read_exact(&mut buf).await?;
    Ok(Some(buf))
}

fn frame(body: &[u8]) -> Vec<u8> {
    let mut f = (body.len() as u32).to_be_bytes().to_vec();
    f.extend_from_slice(body);
    f
}

fn put_string(out: &mut Vec<u8>, b: &[u8]) {
    out.extend_from_slice(&(b.len() as u32).to_be_bytes());
    out.extend_from_slice(b);
}

fn get_string<'a>(r: &mut &'a [u8]) -> Option<&'a [u8]> {
    if r.len() < 4 {
        return None;
    }
    let n = u32::from_be_bytes([r[0], r[1], r[2], r[3]]) as usize;
    if r.len() < 4 + n {
        return None;
    }
    let s = &r[4..4 + n];
    *r = &r[4 + n..];
    Some(s)
}

/// Sends one raw request to the system agent and returns its raw reply.
async fn relay(system: &mut Option<Box<dyn AgentStream + Send + Unpin>>, body: &[u8]) -> Option<Vec<u8>> {
    let s = system.as_mut()?;
    if s.write_all(&frame(body)).await.is_err() {
        *system = None;
        return None;
    }
    match read_frame(s).await {
        Ok(Some(r)) => Some(r),
        _ => {
            *system = None;
            None
        }
    }
}

/// Answers one agent request.
async fn respond(body: &[u8], system: &mut Option<Box<dyn AgentStream + Send + Unpin>>) -> Vec<u8> {
    match body.first() {
        Some(&REQUEST_IDENTITIES) => {
            let internal = internal_keys();
            let mut entries: Vec<(Vec<u8>, String)> = internal
                .iter()
                .filter_map(|k| Some((k.public_key().to_bytes().ok()?, k.comment().to_string())))
                .collect();
            // add the system agent's identities
            if let Some(r) = relay(system, &[REQUEST_IDENTITIES]).await
                && r.first() == Some(&IDENTITIES_ANSWER)
                && r.len() >= 5
            {
                let n = u32::from_be_bytes([r[1], r[2], r[3], r[4]]);
                let mut rest = &r[5..];
                for _ in 0..n {
                    let (Some(blob), Some(comment)) = (get_string(&mut rest), get_string(&mut rest)) else { break };
                    if !entries.iter().any(|(b, _)| b == blob) {
                        entries.push((blob.to_vec(), String::from_utf8_lossy(comment).into_owned()));
                    }
                }
            }
            let mut out = vec![IDENTITIES_ANSWER];
            out.extend_from_slice(&(entries.len() as u32).to_be_bytes());
            for (blob, comment) in entries {
                put_string(&mut out, &blob);
                put_string(&mut out, comment.as_bytes());
            }
            out
        }
        Some(&SIGN_REQUEST) => {
            let mut r = &body[1..];
            let (Some(blob), Some(data)) = (get_string(&mut r), get_string(&mut r)) else { return vec![FAILURE] };
            let flags = if r.len() >= 4 { u32::from_be_bytes([r[0], r[1], r[2], r[3]]) } else { 0 };
            let internal = internal_keys().into_iter().find(|k| k.public_key().to_bytes().ok().as_deref() == Some(blob));
            if let Some(k) = internal {
                let hash = match flags {
                    f if f & 4 != 0 => Some(HashAlg::Sha512),
                    f if f & 2 != 0 => Some(HashAlg::Sha256),
                    _ => None,
                };
                match sign(&k, hash, data) {
                    Ok(sig) => {
                        let mut out = vec![SIGN_RESPONSE];
                        put_string(&mut out, &sig);
                        out
                    }
                    Err(_) => vec![FAILURE],
                }
            } else {
                relay(system, body).await.unwrap_or_else(|| vec![FAILURE])
            }
        }
        // adding / removing keys from a remote host is not allowed
        _ => vec![FAILURE],
    }
}

/// Serves the agent protocol on a stream (an agent channel opened by the server).
pub async fn serve<S: AsyncRead + AsyncWrite + Unpin>(mut s: S) {
    let mut system = system().await.map(|c| c.into_inner());
    while let Ok(Some(body)) = read_frame(&mut s).await {
        let reply = respond(&body, &mut system).await;
        if s.write_all(&frame(&reply)).await.is_err() {
            break;
        }
    }
}

// ---- Tauri commands ----

#[derive(Serialize)]
pub struct AgentState {
    pub keys: Vec<AgentKey>,
    /// running system agent ("" when none)
    pub system: String,
}

#[tauri::command]
pub async fn agent_state() -> AgentState {
    AgentState { keys: list().await, system: system_name().await.unwrap_or_default().to_string() }
}

/// Loads a private key file into the internal agent.
#[tauri::command]
pub async fn agent_add(path: String, passphrase: Option<String>) -> AppResult<AgentKey> {
    let key = tokio::task::spawn_blocking({
        let path = path.clone();
        move || crate::keygen::load(std::path::Path::new(&path), passphrase.as_deref())
    })
    .await
    .map_err(|e| AppError::Conn(e.to_string()))??;
    Ok(add(key, &path))
}

#[tauri::command]
pub fn agent_remove(fingerprint: String) {
    remove(&fingerprint);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn protocol_with_internal_keys() {
        let k = crate::keygen::generate("ed25519", "agent@test").unwrap();
        let r = crate::keygen::generate("rsa2048", "rsa@test").unwrap();
        let pk = k.public_key().clone();
        add(k, "mem");
        add(r.clone(), "mem");
        // a client talking to serve() over an in-memory pipe
        let (a, b) = tokio::io::duplex(64 * 1024);
        tokio::spawn(serve(b));
        let mut client = AgentClient::connect(a);
        let ids = client.request_identities().await.unwrap();
        assert!(ids.iter().any(|i| matches!(i, AgentIdentity::PublicKey { key, comment } if key.key_data() == pk.key_data() && comment == "agent@test")));
        for (id, hash) in [
            (AgentIdentity::PublicKey { key: pk.clone(), comment: String::new() }, None),
            (AgentIdentity::PublicKey { key: r.public_key().clone(), comment: String::new() }, Some(HashAlg::Sha512)),
        ] {
            let data = b"donnees a signer".to_vec();
            let signed = client.sign_request(&id, hash, data.clone()).await.expect("signature");
            // russh appends string(signature blob) to the data
            let mut rest = &signed[data.len()..];
            let blob = get_string(&mut rest).unwrap();
            let sig = russh::keys::ssh_key::Signature::try_from(blob).unwrap();
            let AgentIdentity::PublicKey { key, .. } = &id else { unreachable!() };
            russh::keys::signature::Verifier::verify(key, &data, &sig).expect("valid signature");
        }
        remove(&pk.fingerprint(HashAlg::Sha256).to_string());
        assert!(!internal_list().iter().any(|i| i.comment == "agent@test"));
        clear();
    }
}
