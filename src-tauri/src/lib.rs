//! Orbiterm backend entry point.

pub mod agent;
pub mod browser;
pub mod bundle;
pub mod commands;
pub mod editor;
pub mod config;
pub mod dircmp;
pub mod error;
pub mod forward;
pub mod ftp;
pub mod rdp;
pub mod rdp_channels;
pub mod rdp_client;
pub mod vnc;
pub mod importer;
pub mod keygen;
pub mod monitor;
pub mod known_hosts;
pub mod localfs;
pub mod nettools;
pub mod pty;
pub mod servers;
pub mod sftp_server;
pub mod tools_commands;
pub mod secrets;
pub mod serial;
pub mod scp;
pub mod sftp;
pub mod ssh;
pub mod telnet;
pub mod terminal;
pub mod termlog;
pub mod updater;
pub mod x11;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // diagnostics on demand: ORBITERM_LOG=debug (or any env_logger filter), written to stderr
    if let Ok(filter) = std::env::var("ORBITERM_LOG") {
        let _ = env_logger::Builder::new().parse_filters(&filter).try_init();
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let dir = config::config_dir()?;
            std::fs::create_dir_all(&dir)?;
            app.manage(commands::AppState::new(dir)?);
            // built-in servers marked "start with Orbiterm"
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                handle.state::<commands::AppState>().servers.autostart().await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::sessions_load,
            commands::session_save,
            commands::session_delete,
            commands::folder_save,
            commands::folder_delete,
            commands::password_forget,
            commands::password_copy,
            commands::passwords_list,
            commands::password_replace,
            commands::passwords_remove,
            commands::monitor_sample,
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
            commands::term_log,
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
            commands::xserver_install,
            commands::sftp_home,
            commands::sftp_list,
            commands::sftp_mode,
            commands::term_write_bytes,
            commands::local_file_info,
            commands::local_kind,
            commands::local_list,
            commands::local_places,
            commands::local_mkdir,
            commands::local_rename,
            commands::local_trash,
            commands::local_open,
            commands::local_reveal,
            commands::local_read_chunk,
            commands::local_write_chunk,
            commands::sftp_mkdir,
            commands::sftp_rename,
            commands::sftp_remove,
            commands::sftp_chmod,
            commands::sftp_touch,
            commands::sftp_download_dir,
            commands::sftp_upload_dir,
            commands::sftp_open_external,
            commands::sftp_download,
            commands::sftp_upload,
            commands::text_read,
            commands::dir_compare,
            commands::config_export,
            commands::config_import,
            commands::sessions_export,
            commands::sessions_import,
            commands::update_mode,
            commands::update_download,
            commands::update_launch,
            commands::ssh_cwd,
            commands::text_write,
            tools_commands::servers_status,
            tools_commands::server_save,
            tools_commands::server_start,
            tools_commands::server_stop,
            tools_commands::server_clear_log,
            tools_commands::scan_start,
            tools_commands::scan_state,
            tools_commands::scan_cancel,
            tools_commands::wol_send,
            tools_commands::open_ports,
            keygen::key_generate,
            keygen::key_load,
            keygen::key_set_comment,
            keygen::key_save_private,
            keygen::key_save_public,
            keygen::key_forget,
            agent::agent_state,
            agent::agent_add,
            agent::agent_remove,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
