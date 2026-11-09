//! Passwords live only in the OS keyring:
//! Windows Credential Manager / Linux Secret Service (GNOME Keyring, KWallet…).

use crate::error::{AppError, AppResult};

const SERVICE: &str = "orbiterm";

fn entry(session_id: &str) -> AppResult<keyring::Entry> {
    keyring::Entry::new(SERVICE, session_id).map_err(|e| AppError::Keyring(e.to_string()))
}

pub fn get_password(session_id: &str) -> AppResult<Option<String>> {
    match entry(session_id)?.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Keyring(e.to_string())),
    }
}

pub fn set_password(session_id: &str, password: &str) -> AppResult<()> {
    entry(session_id)?.set_password(password).map_err(|e| AppError::Keyring(e.to_string()))
}

pub fn delete_password(session_id: &str) -> AppResult<()> {
    match entry(session_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Keyring(e.to_string())),
    }
}
