//! X11: forwarding helpers (display parsing, fake-cookie substitution) and
//! management of a local X server (VcXsrv / Xming on Windows, native on Linux).

use std::sync::Mutex;

use serde::Serialize;

use crate::error::{AppError, AppResult};

pub const COOKIE_PROTO: &str = "MIT-MAGIC-COOKIE-1";

/// Where X11 connections forwarded by the server must be delivered locally.
#[derive(Debug, Clone, PartialEq)]
pub enum XTarget {
    Tcp { host: String, port: u16 },
    #[cfg(unix)]
    Unix(std::path::PathBuf),
}

/// Parses a DISPLAY value (`:0`, `localhost:10.0`, `127.0.0.1:0`, `unix:0`).
pub fn parse_display(display: &str) -> Option<(XTarget, u32)> {
    let d = display.trim();
    let (host, rest) = d.rsplit_once(':')?;
    let num_str = rest.split('.').next().unwrap_or("0");
    let num: u16 = num_str.parse().ok()?;
    let screen: u32 = rest.split('.').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let host = host.trim();
    #[cfg(unix)]
    if host.is_empty() || host == "unix" {
        return Some((XTarget::Unix(format!("/tmp/.X11-unix/X{num}").into()), screen));
    }
    let host = if host.is_empty() || host == "unix" { "127.0.0.1" } else { host };
    Some((XTarget::Tcp { host: host.to_string(), port: 6000 + num }, screen))
}

/// The local display to use: explicit setting, else $DISPLAY, else `127.0.0.1:0`.
pub fn local_display(setting: &str) -> String {
    if !setting.trim().is_empty() {
        return setting.trim().to_string();
    }
    std::env::var("DISPLAY").ok().filter(|d| !d.is_empty()).unwrap_or_else(|| "127.0.0.1:0".into())
}

/// Random 128-bit cookie sent to the server instead of the real one: (bytes, hex).
pub fn fake_cookie() -> (Vec<u8>, String) {
    let bytes = uuid::Uuid::new_v4().as_bytes().to_vec();
    let h = hex(&bytes);
    (bytes, h)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

/// Real cookie of the local display (`xauth list`), when there is one.
pub fn real_cookie(display: &str) -> Option<Vec<u8>> {
    let out = std::process::Command::new("xauth").args(["list", display]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.contains(COOKIE_PROTO))?;
    unhex(line.split_whitespace().last()?)
}

fn pad4(n: usize) -> usize {
    (n + 3) & !3
}

/// Rewrites the X11 connection setup sent by a remote client: the fake cookie is
/// checked and replaced by the real one (or removed when the local server has no
/// access control). Returns `None` while more bytes are needed, `Some(Err)` when
/// the cookie does not match.
pub fn rewrite_setup(buf: &[u8], fake: &[u8], real: Option<&[u8]>) -> Option<Result<Vec<u8>, ()>> {
    if buf.len() < 12 {
        return None;
    }
    let big = match buf[0] {
        b'B' => true,
        b'l' => false,
        _ => return Some(Err(())),
    };
    let rd = |i: usize| -> usize {
        let v = [buf[i], buf[i + 1]];
        (if big { u16::from_be_bytes(v) } else { u16::from_le_bytes(v) }) as usize
    };
    let (name_len, data_len) = (rd(6), rd(8));
    let total = 12 + pad4(name_len) + pad4(data_len);
    if buf.len() < total {
        return None;
    }
    let name = &buf[12..12 + name_len];
    let data_off = 12 + pad4(name_len);
    let data = &buf[data_off..data_off + data_len];
    if name != COOKIE_PROTO.as_bytes() || data != fake {
        return Some(Err(()));
    }
    let wr = |v: usize| -> [u8; 2] { if big { (v as u16).to_be_bytes() } else { (v as u16).to_le_bytes() } };
    let mut out = buf[..6].to_vec();
    match real {
        Some(cookie) => {
            out.extend(wr(name_len));
            out.extend(wr(cookie.len()));
            out.extend(&buf[10..12]);
            out.extend(name);
            out.resize(12 + pad4(name_len), 0);
            out.extend(cookie);
            out.resize(12 + pad4(name_len) + pad4(cookie.len()), 0);
        }
        None => {
            out.extend(wr(0));
            out.extend(wr(0));
            out.extend(&buf[10..12]);
        }
    }
    out.extend(&buf[total..]);
    Some(Ok(out))
}

// ---------------------------------------------------------------- local X server

#[derive(Debug, Clone, Serialize)]
pub struct XServerStatus {
    /// "vcxsrv", "xming", "native" or "" when none was found
    pub kind: String,
    pub path: String,
    pub running: bool,
    pub display: String,
    /// started by Orbiterm (so it can be stopped)
    pub owned: bool,
}

static CHILD: Mutex<Option<std::process::Child>> = Mutex::new(None);

fn find_server() -> Option<(String, String)> {
    if cfg!(windows) {
        let candidates = [
            ("vcxsrv", r"C:\Program Files\VcXsrv\vcxsrv.exe"),
            ("vcxsrv", r"C:\Program Files (x86)\VcXsrv\vcxsrv.exe"),
            ("xming", r"C:\Program Files (x86)\Xming\Xming.exe"),
            ("xming", r"C:\Program Files\Xming\Xming.exe"),
        ];
        for (k, p) in candidates {
            if std::path::Path::new(p).exists() {
                return Some((k.into(), p.into()));
            }
        }
        which::which("vcxsrv.exe").ok().map(|p| ("vcxsrv".into(), p.to_string_lossy().into_owned()))
    } else if std::env::var("DISPLAY").map(|d| !d.is_empty()).unwrap_or(false)
        || std::env::var("WAYLAND_DISPLAY").is_ok()
    {
        Some(("native".into(), String::new()))
    } else {
        None
    }
}

fn port_open(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), std::time::Duration::from_millis(300)).is_ok()
}

pub fn status() -> XServerStatus {
    let found = find_server();
    let mut owned = false;
    if let Some(c) = CHILD.lock().unwrap().as_mut() {
        owned = matches!(c.try_wait(), Ok(None));
    }
    let (kind, path) = found.unwrap_or_default();
    let running = if kind == "native" { true } else { port_open(6000) };
    let display = if kind == "native" { std::env::var("DISPLAY").unwrap_or_default() } else { "127.0.0.1:0".into() };
    XServerStatus { kind, path, running, display, owned }
}

pub fn start() -> AppResult<XServerStatus> {
    let st = status();
    if st.running {
        return Ok(st);
    }
    let Some((kind, path)) = find_server() else {
        return Err(AppError::Unsupported(
            "aucun serveur X trouvé. Installe VcXsrv (winget install marha.VcXsrv) puis réessaie.".into(),
        ));
    };
    let mut cmd = std::process::Command::new(&path);
    match kind.as_str() {
        "vcxsrv" => cmd.args([":0", "-multiwindow", "-clipboard", "-wgl", "-ac"]),
        "xming" => cmd.args([":0", "-multiwindow", "-clipboard", "-ac"]),
        _ => return Ok(st),
    };
    let child = cmd.spawn().map_err(|e| AppError::Conn(format!("{path} : {e}")))?;
    *CHILD.lock().unwrap() = Some(child);
    for _ in 0..30 {
        if port_open(6000) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Ok(status())
}

pub fn stop() -> XServerStatus {
    if let Some(mut c) = CHILD.lock().unwrap().take() {
        let _ = c.kill();
        let _ = c.wait();
    }
    status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays() {
        assert_eq!(
            parse_display("localhost:10.0"),
            Some((XTarget::Tcp { host: "localhost".into(), port: 6010 }, 0))
        );
        assert_eq!(parse_display("127.0.0.1:0.1"), Some((XTarget::Tcp { host: "127.0.0.1".into(), port: 6000 }, 1)));
        #[cfg(windows)]
        assert_eq!(parse_display(":0"), Some((XTarget::Tcp { host: "127.0.0.1".into(), port: 6000 }, 0)));
        assert_eq!(parse_display("nonsense"), None);
    }

    fn setup(big: bool, name: &[u8], data: &[u8], tail: &[u8]) -> Vec<u8> {
        let w = |v: u16| if big { v.to_be_bytes() } else { v.to_le_bytes() };
        let mut v = vec![if big { b'B' } else { b'l' }, 0];
        v.extend(w(11));
        v.extend(w(0));
        v.extend(w(name.len() as u16));
        v.extend(w(data.len() as u16));
        v.extend([0, 0]);
        v.extend(name);
        v.resize(12 + pad4(name.len()), 0);
        v.extend(data);
        v.resize(12 + pad4(name.len()) + pad4(data.len()), 0);
        v.extend(tail);
        v
    }

    #[test]
    fn cookie_is_replaced_or_removed() {
        let fake = [7u8; 16];
        let real = [9u8; 16];
        for big in [true, false] {
            let input = setup(big, COOKIE_PROTO.as_bytes(), &fake, b"rest");
            assert!(rewrite_setup(&input[..20], &fake, None).is_none(), "needs more bytes");
            let out = rewrite_setup(&input, &fake, Some(&real)).unwrap().unwrap();
            assert_eq!(out, setup(big, COOKIE_PROTO.as_bytes(), &real, b"rest"));
            let out = rewrite_setup(&input, &fake, None).unwrap().unwrap();
            assert_eq!(out, setup(big, b"", b"", b"rest"));
        }
        let wrong = setup(false, COOKIE_PROTO.as_bytes(), &real, b"");
        assert_eq!(rewrite_setup(&wrong, &fake, None), Some(Err(())));
    }

    #[test]
    fn hex_roundtrip() {
        assert_eq!(unhex(&hex(&[0, 255, 16])), Some(vec![0, 255, 16]));
        let (b, h) = fake_cookie();
        assert_eq!((b.len(), unhex(&h)), (16, Some(b)));
    }
}
