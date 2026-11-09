//! Orbiterm backend entry point.

pub mod browser;
pub mod commands;
pub mod config;
pub mod error;
pub mod forward;
pub mod ftp;
pub mod rdp;
pub mod rdp_client;
pub mod vnc;
pub mod importer;
pub mod known_hosts;
pub mod pty;
pub mod secrets;
pub mod serial;
pub mod sftp;
pub mod ssh;
pub mod telnet;
pub mod terminal;
pub mod x11;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = config::config_dir()?;
            std::fs::create_dir_all(&dir)?;
            app.manage(commands::AppState::new(dir)?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::sessions_load,
            commands::session_save,
            commands::session_delete,
            commands::folder_save,
            commands::folder_delete,
            commands::password_forget,
            commands::import_sources,
            commands::import_run,
            commands::macro_save,
            commands::macro_delete,
            commands::config_path,
            commands::vnc_url,
            commands::rdp_url,
            commands::vnc_password_get,
            commands::vnc_password_set,
            commands::external_running,
            browser::browser_open,
            browser::browser_bounds,
            browser::browser_navigate,
            browser::browser_action,
            browser::browser_close,
            commands::term_open,
            commands::hostkey_answer,
            commands::term_write,
            commands::term_resize,
            commands::term_close,
            commands::local_shells,
            commands::serial_ports,
            commands::term_forwards,
            commands::tunnels_list,
            commands::tunnel_save,
            commands::tunnel_delete,
            commands::tunnel_start,
            commands::tunnel_stop,
            commands::xserver_status,
            commands::xserver_start,
            commands::xserver_stop,
            commands::sftp_home,
            commands::sftp_list,
            commands::sftp_mkdir,
            commands::sftp_rename,
            commands::sftp_remove,
            commands::sftp_download,
            commands::sftp_upload,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
