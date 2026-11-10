//! Passwords live only in the OS keyring:
//! Windows Credential Manager / Linux Secret Service (GNOME Keyring, KWallet…).
//!
//! On Linux the Secret Service may show its own "unlock the keyring" dialog, or never
//! answer at all (no provider, dialog dismissed): every call is therefore bounded in time
//! so a connection never hangs on it, and the caller falls back to asking the password.

use std::sync::Mutex;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult};

const SERVICE: &str = "orbiterm";
/// Long enough to type the keyring password in the system dialog.
const TIMEOUT: Duration = Duration::from_secs(90);
/// After a timeout, later calls fail at once for this long instead of waiting again.
const BACKOFF: Duration = Duration::from_secs(300);

static UNRESPONSIVE_SINCE: Mutex<Option<Instant>> = Mutex::new(None);

fn unavailable() -> AppError {
    AppError::Keyring("le trousseau de mots de passe du système ne répond pas (déverrouillage annulé ?)".into())
}

/// Runs a keyring operation on its own thread, bounded by [`TIMEOUT`].
fn guarded<T: Send + 'static>(f: impl FnOnce() -> AppResult<T> + Send + 'static) -> AppResult<T> {
    if let Some(t) = *UNRESPONSIVE_SINCE.lock().unwrap()
        && t.elapsed() < BACKOFF
    {
        return Err(unavailable());
    }
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("keyring".into())
        .spawn(move || {
            let _ = tx.send(f());
        })
        .map_err(|e| AppError::Keyring(e.to_string()))?;
    // waiting may take long (system unlock dialog): let tokio move its other tasks away
    let wait = || rx.recv_timeout(TIMEOUT);
    let multi = tokio::runtime::Handle::try_current()
        .is_ok_and(|h| h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread);
    let answer = if multi { tokio::task::block_in_place(wait) } else { wait() };
    match answer {
        Ok(r) => {
            *UNRESPONSIVE_SINCE.lock().unwrap() = None;
            r
        }
        Err(_) => {
            *UNRESPONSIVE_SINCE.lock().unwrap() = Some(Instant::now());
            log::warn!("keyring: no answer after {TIMEOUT:?}");
            Err(unavailable())
        }
    }
}

fn entry(session_id: &str) -> AppResult<keyring::Entry> {
    keyring::Entry::new(SERVICE, session_id).map_err(|e| AppError::Keyring(e.to_string()))
}

pub fn get_password(session_id: &str) -> AppResult<Option<String>> {
    let id = session_id.to_string();
    guarded(move || match entry(&id)?.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Keyring(e.to_string())),
    })
}

/// Like [`get_password`], but a keyring failure counts as "no saved password"
/// (the user is then asked for it instead of the connection failing).
pub fn saved_password(session_id: &str) -> Option<String> {
    get_password(session_id).unwrap_or_else(|e| {
        log::warn!("keyring read failed: {e}");
        None
    })
}

pub fn set_password(session_id: &str, password: &str) -> AppResult<()> {
    let (id, pw) = (session_id.to_string(), password.to_string());
    guarded(move || entry(&id)?.set_password(&pw).map_err(|e| AppError::Keyring(e.to_string())))
}

pub fn delete_password(session_id: &str) -> AppResult<()> {
    let id = session_id.to_string();
    guarded(move || match entry(&id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Keyring(e.to_string())),
    })
}
