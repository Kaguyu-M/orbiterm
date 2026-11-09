//! Tauri commands of the network tools and the built-in servers.

use tauri::State;

use crate::commands::AppState;
use crate::error::AppResult;
use crate::nettools::{self, ScanState, SocketInfo};
use crate::secrets;
use crate::servers::{self, ServerConfig, ServerKind, ServerStatus};

#[tauri::command]
pub async fn servers_status(state: State<'_, AppState>) -> AppResult<Vec<ServerStatus>> {
    Ok(state.servers.status().await)
}

/// Saves a server configuration; `password` (if given and not empty) goes to the keyring.
/// A running server is restarted so the new settings apply.
#[tauri::command]
pub async fn server_save(state: State<'_, AppState>, config: ServerConfig, password: Option<String>) -> AppResult<()> {
    if let Some(p) = password.filter(|p| !p.is_empty()) {
        secrets::set_password(&config.kind.secret_key(), &p)?;
    }
    servers::save(&state.config_dir, &config)?;
    let running = state.servers.status().await.iter().any(|s| s.config.kind == config.kind && s.running);
    if running {
        state.servers.stop(config.kind).await;
        state.servers.start(config.kind).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn server_start(state: State<'_, AppState>, kind: ServerKind) -> AppResult<u16> {
    state.servers.start(kind).await
}

#[tauri::command]
pub async fn server_stop(state: State<'_, AppState>, kind: ServerKind) -> AppResult<()> {
    state.servers.stop(kind).await;
    Ok(())
}

#[tauri::command]
pub async fn server_clear_log(state: State<'_, AppState>, kind: ServerKind) -> AppResult<()> {
    state.servers.clear_log(kind);
    Ok(())
}

#[tauri::command]
pub async fn scan_start(id: String, targets: String, ports: String, timeout_ms: u64, concurrency: usize) -> AppResult<usize> {
    nettools::scan_start(id, &targets, &ports, timeout_ms, concurrency).await
}

#[tauri::command]
pub async fn scan_state(id: String) -> AppResult<Option<ScanState>> {
    Ok(nettools::scan_state(&id))
}

#[tauri::command]
pub async fn scan_cancel(id: String) -> AppResult<()> {
    nettools::scan_cancel(&id);
    Ok(())
}

#[tauri::command]
pub async fn wol_send(mac: String, broadcast: String, port: u16) -> AppResult<()> {
    nettools::wol_send(&mac, &broadcast, port).await
}

#[tauri::command]
pub async fn open_ports() -> AppResult<Vec<SocketInfo>> {
    nettools::open_ports().await
}
