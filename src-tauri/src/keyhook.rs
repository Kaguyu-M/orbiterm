//! Windows: system shortcuts for a full-screen remote desktop, like the Windows Remote
//! Desktop client (mstsc). While a capture is active and Orbiterm's window is in the
//! foreground, a low-level keyboard hook takes every key before Windows does — the Windows
//! key, Alt+Tab, Alt+Esc, Ctrl+Esc, Alt+F4… — and sends it to the remote computer instead.
//!
//! Captured in full screen, and by default also in a window while the remote desktop has the
//! keyboard (like RustDesk): the Start menu then opens on the remote computer, not this one.
//!
//! As in mstsc: Ctrl+Alt+End sends Ctrl+Alt+Del (which Windows never gives to programs), and
//! Ctrl+Alt+Pause switches full screen on or off. Win+L and Ctrl+Alt+Del themselves stay local.
//!
//! The hook runs in a helper process (`orbiterm --key-hook`), not in Orbiterm itself: a
//! low-level hook installed by the process that owns the foreground window (Orbiterm with its
//! web view) is not called by Windows and gets silently removed — the Start menu then opened
//! on both computers. The helper reads "on <window>" / "off" on its standard input and writes
//! the captured keys on its standard output; it ends with Orbiterm (its input closes).

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_CONTROL, VK_END, VK_LCONTROL, VK_LMENU, VK_MENU, VK_PAUSE, VK_RCONTROL, VK_RMENU};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_EXTENDED,
    LLKHF_INJECTED, LLKHF_UP, MSG, SetWindowsHookExW, TranslateMessage, WH_KEYBOARD_LL,
};

/// Command-line argument that makes Orbiterm run as the keyboard hook helper.
pub const HELPER_ARG: &str = "--key-hook";

/// What the hook hands to the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    /// scancode (set 1, 0xE0xx = extended), released?
    Key(u16, bool),
    CtrlAltDel,
    /// Ctrl+Alt+Pause: full screen on / off
    ToggleFullScreen,
}

pub type HookSink = Box<dyn Fn(HookEvent) + Send>;

// ------------------------------------------------------------------ Orbiterm side

struct Target {
    /// who asked for the capture (a connection id)
    owner: String,
    sink: HookSink,
}

static TARGET: Mutex<Option<Target>> = Mutex::new(None);
/// input of the running helper
static HELPER: Mutex<Option<ChildStdin>> = Mutex::new(None);
/// keys sent as pressed (released on the remote side when the capture ends)
static PRESSED: Mutex<BTreeSet<u16>> = Mutex::new(BTreeSet::new());

/// Starts sending the keys typed in `window` (HWND) to `sink`, for `owner`.
pub fn capture(owner: &str, window: isize, sink: HookSink) {
    release();
    *TARGET.lock().unwrap_or_else(|e| e.into_inner()) = Some(Target { owner: owner.to_string(), sink });
    tell(&format!("on {window}"), true);
}

/// Stops the capture of `owner` (another tab may have taken it over meanwhile).
pub fn release_for(owner: &str) {
    let mine = TARGET.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|t| t.owner == owner);
    if mine {
        release();
    }
}

/// Stops the capture: keys held down are released on the remote side, the helper lets every
/// key through again.
pub fn release() {
    tell("off", false);
    let target = TARGET.lock().unwrap_or_else(|e| e.into_inner()).take();
    let held: Vec<u16> = std::mem::take(&mut *PRESSED.lock().unwrap_or_else(|e| e.into_inner())).into_iter().collect();
    if let Some(t) = target {
        for sc in held {
            (t.sink)(HookEvent::Key(sc, true));
        }
    }
}

/// Sends a line to the helper, started first if `start` (and again if it has gone away).
fn tell(line: &str, start: bool) {
    let mut helper = HELPER.lock().unwrap_or_else(|e| e.into_inner());
    for _ in 0..2 {
        if helper.is_none() {
            if !start {
                return;
            }
            *helper = spawn_helper();
        }
        let Some(input) = helper.as_mut() else { return };
        if writeln!(input, "{line}").and_then(|_| input.flush()).is_ok() {
            return;
        }
        *helper = None;
    }
}

fn spawn_helper() -> Option<ChildStdin> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let exe = std::env::current_exe().ok()?;
    let mut child = Command::new(exe)
        .arg(HELPER_ARG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| log::warn!("raccourcis Windows : assistant clavier impossible ({e})"))
        .ok()?;
    let out = child.stdout.take()?;
    let input = child.stdin.take()?;
    std::thread::Builder::new()
        .name("key-hook".into())
        .spawn(move || {
            for line in BufReader::new(out).lines() {
                let Ok(line) = line else { break };
                if let Some(ev) = parse_event(&line) {
                    deliver(ev);
                }
            }
            let _ = child.wait();
        })
        .ok()?;
    Some(input)
}

/// A captured key from the helper goes to the session that holds the capture.
fn deliver(ev: HookEvent) {
    let target = TARGET.lock().unwrap_or_else(|e| e.into_inner());
    let Some(t) = target.as_ref() else { return };
    if let HookEvent::Key(sc, released) = ev {
        let mut held = PRESSED.lock().unwrap_or_else(|e| e.into_inner());
        if released {
            held.remove(&sc);
        } else {
            held.insert(sc);
        }
    }
    (t.sink)(ev);
}

fn format_event(ev: HookEvent) -> String {
    match ev {
        HookEvent::Key(sc, up) => format!("k {sc} {}", u8::from(up)),
        HookEvent::CtrlAltDel => "cad".into(),
        HookEvent::ToggleFullScreen => "full".into(),
    }
}

fn parse_event(line: &str) -> Option<HookEvent> {
    let mut w = line.split_whitespace();
    match w.next()? {
        "k" => Some(HookEvent::Key(w.next()?.parse().ok()?, w.next()? == "1")),
        "cad" => Some(HookEvent::CtrlAltDel),
        "full" => Some(HookEvent::ToggleFullScreen),
        _ => None,
    }
}

// ------------------------------------------------------------------ helper process

/// window whose keys are captured (0: none)
static WINDOW: AtomicIsize = AtomicIsize::new(0);
// modifiers seen by the hook (keys it swallows never reach the system key state)
static CTRL: AtomicBool = AtomicBool::new(false);
static ALT: AtomicBool = AtomicBool::new(false);
/// injected keys captured too (GUI tests only: ORBITERM_HOOK_INJECTED)
static INJECTED: AtomicBool = AtomicBool::new(false);
static EVENTS: std::sync::OnceLock<std::sync::mpsc::Sender<HookEvent>> = std::sync::OnceLock::new();

/// Runs the keyboard hook helper until Orbiterm closes its input.
pub fn helper_main() {
    INJECTED.store(std::env::var_os("ORBITERM_HOOK_INJECTED").is_some(), Ordering::Relaxed);
    // captured keys are written by their own thread: the hook itself never waits
    let (tx, rx) = std::sync::mpsc::channel::<HookEvent>();
    let _ = EVENTS.set(tx);
    std::thread::spawn(move || {
        let mut out = std::io::stdout().lock();
        for ev in rx {
            if writeln!(out, "{}", format_event(ev)).and_then(|_| out.flush()).is_err() {
                std::process::exit(0);
            }
        }
    });
    std::thread::spawn(|| {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            let window = line.strip_prefix("on ").and_then(|w| w.trim().parse().ok()).unwrap_or(0);
            CTRL.store(false, Ordering::Relaxed);
            ALT.store(false, Ordering::Relaxed);
            WINDOW.store(window, Ordering::Relaxed);
        }
        // Orbiterm has gone
        std::process::exit(0);
    });
    // SAFETY: plain Win32 calls; the hook procedure lives as long as the process
    unsafe {
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), GetModuleHandleW(std::ptr::null()), 0);
        if hook.is_null() {
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Decides what a key does while captured: None lets the system have it.
fn decide(vk: u32, scan: u32, flags: u32, ctrl: bool, alt: bool, injected_too: bool) -> Option<Option<HookEvent>> {
    if flags & LLKHF_INJECTED != 0 && !injected_too {
        return None;
    }
    let up = flags & LLKHF_UP != 0;
    if vk == u32::from(VK_PAUSE) {
        // Ctrl+Alt+Pause switches full screen (Pause itself has no plain scancode)
        return Some((ctrl && alt && !up).then_some(HookEvent::ToggleFullScreen));
    }
    if vk == u32::from(VK_END) && ctrl && alt {
        return Some((!up).then_some(HookEvent::CtrlAltDel));
    }
    // AltGr also produces a fake left Ctrl (scancode 0x21D): the remote gets the real AltGr only
    if scan & 0x200 != 0 {
        return Some(None);
    }
    let sc = (scan & 0xff) as u16;
    if sc == 0 {
        return None;
    }
    let sc = if flags & LLKHF_EXTENDED != 0 { 0xE000 | sc } else { sc };
    Some(Some(HookEvent::Key(sc, up)))
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for WH_KEYBOARD_LL, lparam points to a KBDLLHOOKSTRUCT
        let k = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let up = k.flags & LLKHF_UP != 0;
        let vk = k.vkCode;
        if [VK_CONTROL, VK_LCONTROL, VK_RCONTROL].iter().any(|&v| u32::from(v) == vk) && k.scanCode & 0x200 == 0 {
            CTRL.store(!up, Ordering::Relaxed);
        }
        if [VK_MENU, VK_LMENU, VK_RMENU].iter().any(|&v| u32::from(v) == vk) {
            ALT.store(!up, Ordering::Relaxed);
        }
        let window = WINDOW.load(Ordering::Relaxed);
        if window != 0
            // SAFETY: no arguments
            && unsafe { GetForegroundWindow() } as isize == window
            && let Some(action) = decide(
                vk,
                k.scanCode,
                k.flags,
                CTRL.load(Ordering::Relaxed),
                ALT.load(Ordering::Relaxed),
                INJECTED.load(Ordering::Relaxed),
            )
        {
            if let (Some(ev), Some(tx)) = (action, EVENTS.get()) {
                let _ = tx.send(ev);
            }
            return 1;
        }
    }
    // SAFETY: passes the event on to the next hook
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_while_captured() {
        // Windows key: extended scancode 0x5B
        assert_eq!(decide(0x5B, 0x5B, LLKHF_EXTENDED, false, false, false), Some(Some(HookEvent::Key(0xE05B, false))));
        // Tab released
        assert_eq!(decide(0x09, 0x0F, LLKHF_UP, false, true, false), Some(Some(HookEvent::Key(0x0F, true))));
        // Ctrl+Alt+End → Ctrl+Alt+Del, its release swallowed
        assert_eq!(decide(u32::from(VK_END), 0x4F, LLKHF_EXTENDED, true, true, false), Some(Some(HookEvent::CtrlAltDel)));
        assert_eq!(decide(u32::from(VK_END), 0x4F, LLKHF_EXTENDED | LLKHF_UP, true, true, false), Some(None));
        // End alone is a normal key
        assert_eq!(decide(u32::from(VK_END), 0x4F, LLKHF_EXTENDED, false, false, false), Some(Some(HookEvent::Key(0xE04F, false))));
        // Ctrl+Alt+Pause switches full screen
        assert_eq!(decide(u32::from(VK_PAUSE), 0x45, 0, true, true, false), Some(Some(HookEvent::ToggleFullScreen)));
        // AltGr's fake Ctrl is dropped, injected keys go to the system (unless a GUI test asks)
        assert_eq!(decide(u32::from(VK_LCONTROL), 0x21D, 0, false, false, false), Some(None));
        assert_eq!(decide(0x41, 0x1E, LLKHF_INJECTED, false, false, false), None);
        assert_eq!(decide(0x41, 0x1E, LLKHF_INJECTED, false, false, true), Some(Some(HookEvent::Key(0x1E, false))));
    }

    #[test]
    fn helper_lines_round_trip() {
        for ev in [HookEvent::Key(0xE05B, false), HookEvent::Key(0x0F, true), HookEvent::CtrlAltDel, HookEvent::ToggleFullScreen] {
            assert_eq!(parse_event(&format_event(ev)), Some(ev));
        }
        assert_eq!(parse_event("bogus"), None);
    }
}
