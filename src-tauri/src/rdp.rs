//! RDP sessions: Windows' own client (mstsc) on Windows, FreeRDP elsewhere.
//!
//! No password is ever written to the .rdp file or passed on a command line:
//! mstsc asks for it itself (and can remember it in the Windows credential manager),
//! FreeRDP asks in the terminal tab it runs in.

use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::Mutex;

use crate::config::Session;
use crate::error::{AppError, AppResult};
use crate::ssh::CloseSink;

/// RDP options of a session (`extra` keys).
pub struct RdpOptions {
    /// "fit" (size of the Orbiterm tab), "fullscreen", "multimon" (every monitor of this computer), or "WxH"
    pub size: String,
    pub width: u32,
    pub height: u32,
    pub clipboard: bool,
    pub drives: bool,
    pub printers: bool,
    /// "local" | "remote" | "none"
    pub sound: String,
    pub admin: bool,
    pub domain: String,
}

impl RdpOptions {
    pub fn from_session(s: &Session) -> Self {
        let num = |k: &str, d: u32| s.opt(k).trim().parse().unwrap_or(d);
        let flag = |k: &str, d: bool| match s.opt(k) {
            "" => d,
            v => matches!(v, "1" | "true" | "yes"),
        };
        let mut size = s.opt("rdp_size").trim().to_string();
        if size.is_empty() {
            size = "fit".into();
        }
        let (mut width, mut height) = (num("rdp_w", 1280), num("rdp_h", 800));
        if let Some((w, h)) = size.split_once('x')
            && let (Ok(w), Ok(h)) = (w.parse(), h.parse())
        {
            (width, height) = (w, h);
        }
        RdpOptions {
            size,
            width: width.clamp(640, 8192),
            height: height.clamp(480, 8192),
            clipboard: flag("rdp_clipboard", true),
            drives: flag("rdp_drives", false),
            printers: flag("rdp_printers", false),
            sound: match s.opt("rdp_sound") {
                "" => "local".into(),
                v => v.to_string(),
            },
            admin: flag("rdp_admin", false),
            domain: s.opt("rdp_domain").trim().to_string(),
        }
    }
}

/// Content of the .rdp file for mstsc.
pub fn rdp_file(host: &str, port: u16, user: &str, o: &RdpOptions) -> String {
    let mut l = vec![
        format!("full address:s:{host}:{port}"),
        format!("screen mode id:i:{}", if o.size == "fullscreen" || o.size == "multimon" { 2 } else { 1 }),
        format!("use multimon:i:{}", (o.size == "multimon") as u8),
        format!("desktopwidth:i:{}", o.width),
        format!("desktopheight:i:{}", o.height),
        "session bpp:i:32".into(),
        "smart sizing:i:1".into(),
        "dynamic resolution:i:1".into(),
        "prompt for credentials:i:0".into(),
        "authentication level:i:2".into(),
        format!("redirectclipboard:i:{}", o.clipboard as u8),
        format!("redirectprinters:i:{}", o.printers as u8),
        format!("drivestoredirect:s:{}", if o.drives { "*" } else { "" }),
        format!(
            "audiomode:i:{}",
            match o.sound.as_str() {
                "remote" => 1,
                "none" => 2,
                _ => 0,
            }
        ),
        format!("administrative session:i:{}", o.admin as u8),
    ];
    if !user.is_empty() {
        let u = if o.domain.is_empty() || user.contains('\\') { user.to_string() } else { format!("{}\\{user}", o.domain) };
        l.push(format!("username:s:{u}"));
    }
    l.join("\r\n") + "\r\n"
}

/// FreeRDP command line (Linux / macOS).
pub fn freerdp_args(host: &str, port: u16, user: &str, o: &RdpOptions) -> Vec<String> {
    let mut a = vec![format!("/v:{host}:{port}"), "/cert:tofu".into()];
    if !user.is_empty() {
        a.push(format!("/u:{user}"));
    }
    if !o.domain.is_empty() {
        a.push(format!("/d:{}", o.domain));
    }
    if o.size == "multimon" {
        a.push("/multimon".into());
        a.push("/f".into());
    } else if o.size == "fullscreen" {
        a.push("/f".into());
    } else {
        a.push(format!("/size:{}x{}", o.width, o.height));
        a.push("/dynamic-resolution".into());
    }
    if o.clipboard {
        a.push("+clipboard".into());
    }
    if o.drives {
        a.push("/drive:home,~".into());
    }
    if o.printers {
        a.push("/printer".into());
    }
    match o.sound.as_str() {
        "remote" => a.push("/audio-mode:1".into()),
        "none" => a.push("/audio-mode:2".into()),
        _ => a.push("/sound".into()),
    }
    if o.admin {
        a.push("/admin".into());
    }
    a
}

/// Finds a FreeRDP client in the PATH.
pub fn freerdp() -> AppResult<PathBuf> {
    ["xfreerdp3", "xfreerdp", "wlfreerdp3", "wlfreerdp", "sdl-freerdp3", "sdl-freerdp"]
        .iter()
        .find_map(|n| which::which(n).ok())
        .ok_or_else(|| AppError::Unsupported("client RDP introuvable : installe FreeRDP (paquet freerdp3-x11 ou freerdp2-x11)".into()))
}

/// An external program window (mstsc) tied to a tab.
pub struct ExternalProcess {
    child: Mutex<Option<Child>>,
    file: Option<PathBuf>,
}

impl ExternalProcess {
    /// Writes the .rdp file into `dir` and starts mstsc on it.
    pub fn launch_mstsc(dir: &Path, name: &str, content: &str, on_close: CloseSink) -> AppResult<std::sync::Arc<Self>> {
        std::fs::create_dir_all(dir)?;
        let file = dir.join(format!("{name}.rdp"));
        std::fs::write(&file, content)?;
        let mut cmd = std::process::Command::new("mstsc.exe");
        cmd.arg(&file);
        let child = cmd.spawn().map_err(|e| AppError::Conn(format!("impossible de lancer mstsc : {e}")))?;
        let me = std::sync::Arc::new(ExternalProcess { child: Mutex::new(Some(child)), file: Some(file) });
        let watch = me.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let mut g = watch.child.lock().unwrap();
                match g.as_mut().map(|c| c.try_wait()) {
                    Some(Ok(None)) => continue,
                    Some(_) => {
                        g.take();
                        drop(g);
                        on_close("fenêtre Bureau à distance fermée".into());
                    }
                    None => {}
                }
                break;
            }
            if let Some(f) = &watch.file {
                let _ = std::fs::remove_file(f);
            }
        });
        Ok(me)
    }

    pub fn running(&self) -> bool {
        self.child.lock().unwrap().is_some()
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.lock().unwrap().as_ref().map(|c| c.id())
    }

    pub fn kill(&self) {
        if let Some(mut c) = self.child.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        if let Some(f) = &self.file {
            let _ = std::fs::remove_file(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AuthMethod, TerminalSettings};

    fn sess(extra: &[(&str, &str)]) -> Session {
        Session {
            id: String::new(),
            name: "w".into(),
            folder: String::new(),
            kind: "rdp".into(),
            host: "srv".into(),
            port: 3389,
            username: "bob".into(),
            auth: AuthMethod::Password,
            key_path: String::new(),
            remember_password: true,
            sftp_enabled: false,
            terminal: TerminalSettings::default(),
            extra: extra.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            forwards: vec![],
        }
    }

    #[test]
    fn multimon_external_clients() {
        let o = RdpOptions::from_session(&sess(&[("rdp_size", "multimon")]));
        let f = rdp_file("h", 3389, "u", &o);
        assert!(f.contains("use multimon:i:1") && f.contains("screen mode id:i:2"));
        let a = freerdp_args("h", 3389, "u", &o);
        assert!(a.contains(&"/multimon".to_string()) && a.contains(&"/f".to_string()));
        assert!(rdp_file("h", 3389, "u", &RdpOptions::from_session(&sess(&[]))).contains("use multimon:i:0"));
    }

    #[test]
    fn rdp_file_options() {
        let o = RdpOptions::from_session(&sess(&[("rdp_size", "1600x900"), ("rdp_drives", "1"), ("rdp_domain", "CORP")]));
        let f = rdp_file("srv", 3390, "bob", &o);
        assert!(f.contains("full address:s:srv:3390\r\n"));
        assert!(f.contains("desktopwidth:i:1600"));
        assert!(f.contains("drivestoredirect:s:*"));
        assert!(f.contains("redirectclipboard:i:1"));
        assert!(f.contains("username:s:CORP\\bob"));
        assert!(!f.to_lowercase().contains("password"));
        let fs = RdpOptions::from_session(&sess(&[("rdp_size", "fullscreen"), ("rdp_clipboard", "0")]));
        let f = rdp_file("srv", 3389, "", &fs);
        assert!(f.contains("screen mode id:i:2"));
        assert!(f.contains("redirectclipboard:i:0"));
        assert!(!f.contains("username"));
        let a = freerdp_args("srv", 3389, "bob", &fs);
        assert!(a.contains(&"/f".to_string()) && !a.contains(&"+clipboard".to_string()));
    }
}
