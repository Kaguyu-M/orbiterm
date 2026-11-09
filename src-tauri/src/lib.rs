//! Orbiterm backend entry point.

pub mod commands;
pub mod config;
pub mod error;
pub mod known_hosts;
pub mod secrets;
pub mod sftp;
pub mod ssh;

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
            commands::ssh_connect,
            commands::hostkey_answer,
            commands::ssh_write,
            commands::ssh_resize,
            commands::ssh_disconnect,
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
