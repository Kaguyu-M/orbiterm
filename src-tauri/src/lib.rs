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
pub mod rdp_cache;
pub mod rdp_channels;
pub mod rdp_client;
#[cfg(windows)]
mod keyhook;
#[cfg(target_os = "linux")]
mod gst_h264;
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
/// WebKitGTK's DMA-BUF renderer kills the window at start ("Error 22/71 dispatching to Wayland
/// display") in virtual machines (VirtualBox, VMware, QEMU, Hyper-V) and with NVIDIA's proprietary
/// driver. Turned off there only, unless the user set WEBKIT_DISABLE_DMABUF_RENDERER themselves.
#[cfg(target_os = "linux")]
fn needs_dmabuf_off(vendor: &str, product: &str, nvidia: bool) -> bool {
    let id = format!("{vendor} {product}").to_lowercase();
    nvidia
        || ["virtualbox", "innotek", "vmware", "qemu", "kvm", "bochs", "virtual machine", "parallels", "xen"]
            .iter()
            .any(|vm| id.contains(vm))
}

#[cfg(target_os = "linux")]
fn webkit_workarounds() {
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some() {
        return;
    }
    let read = |p: &str| std::fs::read_to_string(p).unwrap_or_default().trim().to_string();
    let nvidia = std::path::Path::new("/proc/driver/nvidia/version").exists();
    if needs_dmabuf_off(&read("/sys/class/dmi/id/sys_vendor"), &read("/sys/class/dmi/id/product_name"), nvidia) {
        // SAFETY: first thing run() does, before any other thread exists
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }
}

/// WebKitGTK keeps the pages of the app in its HTTP cache: after an update (apt, dnf, pacman)
/// the window showed the previous version's interface with the new program. The cache is
/// emptied once when the version changes. Returns whether it was.
#[cfg(target_os = "linux")]
fn drop_stale_web_cache(data_dir: &std::path::Path, version: &str) -> bool {
    let marker = data_dir.join("orbiterm-version");
    if std::fs::read_to_string(&marker).is_ok_and(|v| v.trim() == version) {
        return false;
    }
    for sub in ["WebKitCache", "CacheStorage"] {
        let _ = std::fs::remove_dir_all(data_dir.join(sub));
    }
    let _ = std::fs::create_dir_all(data_dir);
    let _ = std::fs::write(&marker, version);
    true
}

#[cfg(target_os = "linux")]
fn web_data_dir() -> Option<std::path::PathBuf> {
    // where Tauri's WebKitGTK keeps its data: $XDG_DATA_HOME/<identifier>
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))?;
    Some(base.join("com.orbiterm.app"))
}

pub fn run() {
    #[cfg(target_os = "linux")]
    webkit_workarounds();
    #[cfg(target_os = "linux")]
    if let Some(dir) = web_data_dir() {
        drop_stale_web_cache(&dir, env!("CARGO_PKG_VERSION"));
    }
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
            commands::rdp_capture_keys,
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn web_cache_dropped_once_per_version() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("WebKitCache");
        std::fs::create_dir_all(&cache).unwrap();
        assert!(super::drop_stale_web_cache(dir.path(), "1.0.0"));
        assert!(!cache.exists());
        std::fs::create_dir_all(&cache).unwrap();
        assert!(!super::drop_stale_web_cache(dir.path(), "1.0.0"));
        assert!(cache.exists());
        assert!(super::drop_stale_web_cache(dir.path(), "1.0.1"));
        assert!(!cache.exists());
    }

    #[test]
    fn dmabuf_off_only_where_it_breaks() {
        assert!(super::needs_dmabuf_off("innotek GmbH", "VirtualBox", false));
        assert!(super::needs_dmabuf_off("QEMU", "Standard PC (Q35 + ICH9, 2009)", false));
        assert!(super::needs_dmabuf_off("VMware, Inc.", "VMware Virtual Platform", false));
        assert!(super::needs_dmabuf_off("LENOVO", "21K5", true));
        assert!(!super::needs_dmabuf_off("LENOVO", "21K5", false));
        assert!(!super::needs_dmabuf_off("Framework", "Laptop 13", false));
    }
}
