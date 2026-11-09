//! Orbiterm backend entry point.

pub mod commands;
pub mod config;
pub mod error;
pub mod known_hosts;
pub mod pty;
pub mod secrets;
pub mod serial;
pub mod sftp;
pub mod ssh;
pub mod telnet;
pub mod terminal;

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
            commands::term_open,
            commands::hostkey_answer,
            commands::term_write,
            commands::term_resize,
            commands::term_close,
            commands::local_shells,
            commands::serial_ports,
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
