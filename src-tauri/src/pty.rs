//! Local programs in a pseudo-terminal: ConPTY on Windows, openpty on Linux/macOS.
//! Used for the local shell, WSL, Mosh and any "execute a command" session.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::ssh::{CloseSink, DataSink};

pub struct PtyConnection {
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
}

/// A shell the user can start locally.
#[derive(Debug, Clone, Serialize)]
pub struct LocalShell {
    pub id: String,
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
}

fn find(program: &str) -> Option<String> {
    which::which(program).ok().map(|p| p.to_string_lossy().into_owned())
}

/// Lists the shells available on this machine (first = default).
pub fn local_shells() -> Vec<LocalShell> {
    let mut out = vec![];
    let mut add = |id: &str, label: &str, program: Option<String>, args: &[&str]| {
        if let Some(p) = program {
            out.push(LocalShell {
                id: id.into(),
                label: label.into(),
                program: p,
                args: args.iter().map(|s| s.to_string()).collect(),
            });
        }
    };
    if cfg!(windows) {
        add("powershell", "Windows PowerShell", find("powershell.exe"), &["-NoLogo"]);
        add("pwsh", "PowerShell 7", find("pwsh.exe"), &["-NoLogo"]);
        add("cmd", "Invite de commandes (cmd)", find("cmd.exe"), &[]);
        let git_bash = [
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\Program Files (x86)\Git\bin\bash.exe",
        ]
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .map(|p| p.to_string());
        add("gitbash", "Git Bash", git_bash, &["--login", "-i"]);
        if let Some(wsl) = find("wsl.exe") {
            for d in wsl_distros() {
                out.push(LocalShell {
                    id: format!("wsl:{d}"),
                    label: format!("WSL : {d}"),
                    program: wsl.clone(),
                    args: vec!["-d".into(), d.clone(), "--cd".into(), "~".into()],
                });
            }
        }
    } else {
        let user_shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty());
        if let Some(sh) = user_shell.clone() {
            out.push(LocalShell { id: "default".into(), label: format!("Shell par défaut ({sh})"), program: sh, args: vec!["-l".into()] });
        }
        for (id, name) in [("bash", "bash"), ("zsh", "zsh"), ("fish", "fish"), ("sh", "sh")] {
            if let Some(p) = find(name)
                && user_shell.as_deref() != Some(p.as_str())
            {
                out.push(LocalShell { id: id.into(), label: name.into(), program: p, args: vec!["-l".into()] });
            }
        }
    }
    out
}

/// Names of installed WSL distributions (wsl.exe prints UTF-16LE).
pub fn wsl_distros() -> Vec<String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let Ok(out) = std::process::Command::new("wsl.exe")
            .args(["-l", "-q"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        else {
            return vec![];
        };
        let u16s: Vec<u16> = out.stdout.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        String::from_utf16_lossy(&u16s)
            .lines()
            .map(|l| l.trim().trim_matches('\0').to_string())
            .filter(|l| !l.is_empty())
            .collect()
    }
    #[cfg(not(windows))]
    {
        vec![]
    }
}

pub struct PtySpawn {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
}

impl PtyConnection {
    pub fn spawn(spec: PtySpawn, on_data: DataSink, on_close: CloseSink) -> AppResult<Arc<PtyConnection>> {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize { rows: spec.rows.max(1), cols: spec.cols.max(1), pixel_width: 0, pixel_height: 0 })
            .map_err(|e| AppError::Conn(format!("pty: {e}")))?;
        let mut cmd = CommandBuilder::new(&spec.program);
        cmd.args(&spec.args);
        let cwd = spec
            .cwd
            .filter(|p| p.is_dir())
            .or_else(|| directories::UserDirs::new().map(|u| u.home_dir().to_path_buf()));
        if let Some(cwd) = cwd {
            cmd.cwd(cwd);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for (k, v) in spec.env {
            cmd.env(k, v);
        }
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| AppError::Conn(format!("impossible de lancer {} : {e}", spec.program)))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| AppError::Conn(e.to_string()))?;
        let writer = pair.master.take_writer().map_err(|e| AppError::Conn(e.to_string()))?;
        let killer = child.clone_killer();

        std::thread::spawn(move || {
            let mut buf = [0u8; 16 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => on_data(buf[..n].to_vec()),
                }
            }
        });
        let closer = on_close.clone();
        std::thread::spawn(move || {
            let reason = match child.wait() {
                Ok(st) if st.success() => "Processus terminé".to_string(),
                Ok(st) => format!("Processus terminé (code {})", st.exit_code()),
                Err(e) => format!("Processus terminé ({e})"),
            };
            // give the reader a moment to flush the last output
            std::thread::sleep(std::time::Duration::from_millis(150));
            closer(reason);
        });

        Ok(Arc::new(PtyConnection {
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            killer: Mutex::new(killer),
        }))
    }

    pub fn write(&self, data: &[u8]) -> AppResult<()> {
        let mut w = self.writer.lock().unwrap();
        w.write_all(data)?;
        w.flush()?;
        Ok(())
    }

    pub fn resize(&self, cols: u32, rows: u32) -> AppResult<()> {
        self.master
            .lock()
            .unwrap()
            .resize(PtySize { rows: rows.max(1) as u16, cols: cols.max(1) as u16, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| AppError::Conn(e.to_string()))
    }

    pub fn kill(&self) {
        let _ = self.killer.lock().unwrap().kill();
    }
}
