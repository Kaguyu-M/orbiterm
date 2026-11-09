//! Host key verification against an OpenSSH-compatible `known_hosts` file.

use std::path::Path;

use russh::keys::ssh_key::{HashAlg, PublicKey};
use serde::Serialize;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HostKeyStatus {
    /// Key recorded and identical.
    Known,
    /// Host never seen.
    Unknown,
    /// Host recorded with a DIFFERENT key of the same type: possible MITM.
    Changed,
}

pub fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

pub fn check(path: &Path, host: &str, port: u16, key: &PublicKey) -> AppResult<HostKeyStatus> {
    if !path.exists() {
        return Ok(HostKeyStatus::Unknown);
    }
    match russh::keys::check_known_hosts_path(host, port, key, path) {
        Ok(true) => Ok(HostKeyStatus::Known),
        Ok(false) => Ok(HostKeyStatus::Unknown),
        Err(russh::keys::Error::KeyChanged { .. }) => Ok(HostKeyStatus::Changed),
        Err(e) => Err(AppError::Ssh(format!("known_hosts: {e}"))),
    }
}

/// Host pattern as written by OpenSSH: `host` for port 22, `[host]:port` otherwise.
fn host_pattern(host: &str, port: u16) -> String {
    if port == 22 { host.to_string() } else { format!("[{host}]:{port}") }
}

/// Records the key. When `replace` is true, previous plain-text entries for this host
/// with the same key algorithm are removed first (used after a confirmed key change).
pub fn learn(path: &Path, host: &str, port: u16, key: &PublicKey, replace: bool) -> AppResult<()> {
    if replace && path.exists() {
        let pattern = host_pattern(host, port);
        let algo = key.algorithm().to_string();
        let text = std::fs::read_to_string(path)?;
        let kept: Vec<&str> = text
            .lines()
            .filter(|line| {
                let mut parts = line.split_whitespace();
                let hosts = parts.next().unwrap_or("");
                let line_algo = parts.next().unwrap_or("");
                !(hosts.split(',').any(|h| h == pattern) && line_algo == algo)
            })
            .collect();
        let mut out = kept.join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        std::fs::write(path, out)?;
    }
    russh::keys::known_hosts::learn_known_hosts_path(host, port, key, path)
        .map_err(|e| AppError::Ssh(format!("known_hosts: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::{Algorithm, PrivateKey};

    fn key() -> PublicKey {
        PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap().public_key().clone()
    }

    #[test]
    fn unknown_then_known_then_changed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let k1 = key();
        let k2 = key();
        assert_eq!(check(&path, "srv", 2222, &k1).unwrap(), HostKeyStatus::Unknown);
        learn(&path, "srv", 2222, &k1, false).unwrap();
        assert_eq!(check(&path, "srv", 2222, &k1).unwrap(), HostKeyStatus::Known);
        assert_eq!(check(&path, "srv", 2222, &k2).unwrap(), HostKeyStatus::Changed);
        // other host/port unaffected
        assert_eq!(check(&path, "srv", 22, &k1).unwrap(), HostKeyStatus::Unknown);
        // accepting the change replaces the entry
        learn(&path, "srv", 2222, &k2, true).unwrap();
        assert_eq!(check(&path, "srv", 2222, &k2).unwrap(), HostKeyStatus::Known);
        assert_eq!(check(&path, "srv", 2222, &k1).unwrap(), HostKeyStatus::Changed);
    }

    #[test]
    fn fingerprint_is_sha256() {
        assert!(fingerprint(&key()).starts_with("SHA256:"));
    }
}
