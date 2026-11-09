//! Error type shared by every backend module.
//!
//! Errors are serialized to the UI as `{ kind, message }` so the frontend can react to
//! specific situations (e.g. ask for a password when `kind == "need_password"`).

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Mot de passe requis")]
    NeedPassword,
    #[error("Passphrase de la clé privée requise")]
    NeedPassphrase,
    #[error("Authentification refusée par le serveur")]
    AuthFailed,
    #[error("Clé d'hôte refusée")]
    HostKeyRejected,
    #[error("Délai de connexion dépassé")]
    Timeout,
    #[error("Session introuvable : {0}")]
    NotFound(String),
    #[error("Chemin invalide : {0}")]
    InvalidPath(String),
    #[error("Configuration : {0}")]
    Config(String),
    #[error("Trousseau système : {0}")]
    Keyring(String),
    #[error("SSH : {0}")]
    Ssh(String),
    #[error("SFTP : {0}")]
    Sftp(String),
    #[error("E/S : {0}")]
    Io(#[from] std::io::Error),
}

impl AppError {
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::NeedPassword => "need_password",
            AppError::NeedPassphrase => "need_passphrase",
            AppError::AuthFailed => "auth_failed",
            AppError::HostKeyRejected => "hostkey_rejected",
            AppError::Timeout => "timeout",
            AppError::NotFound(_) => "not_found",
            AppError::InvalidPath(_) => "invalid_path",
            AppError::Config(_) => "config",
            AppError::Keyring(_) => "keyring",
            AppError::Ssh(_) => "ssh",
            AppError::Sftp(_) => "sftp",
            AppError::Io(_) => "io",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("AppError", 2)?;
        st.serialize_field("kind", self.kind())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}

impl From<russh::Error> for AppError {
    fn from(e: russh::Error) -> Self {
        AppError::Ssh(e.to_string())
    }
}

impl From<russh_sftp::client::error::Error> for AppError {
    fn from(e: russh_sftp::client::error::Error) -> Self {
        AppError::Sftp(e.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
