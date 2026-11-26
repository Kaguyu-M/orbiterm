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

use std::collections::BTreeSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_CONTROL, VK_END, VK_LCONTROL, VK_LMENU, VK_MENU, VK_PAUSE, VK_RCONTROL, VK_RMENU};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_EXTENDED,
    LLKHF_INJECTED, LLKHF_UP, MSG, PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WM_QUIT,
};

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

struct Target {
    /// who asked for the capture (a connection id)
    owner: String,
    window: isize,
    sink: HookSink,
}

static TARGET: Mutex<Option<Target>> = Mutex::new(None);
/// thread running the hook's message loop
static THREAD: Mutex<Option<u32>> = Mutex::new(None);
/// keys sent as pressed (released on the remote side when the capture ends)
static PRESSED: Mutex<BTreeSet<u16>> = Mutex::new(BTreeSet::new());
// modifiers seen by the hook (keys it swallows never reach the system key state)
static CTRL: AtomicBool = AtomicBool::new(false);
static ALT: AtomicBool = AtomicBool::new(false);

/// Starts sending the keys typed in `window` (HWND) to `sink`, for `owner`.
pub fn capture(owner: &str, window: isize, sink: HookSink) {
    release();
    *TARGET.lock().unwrap_or_else(|e| e.into_inner()) = Some(Target { owner: owner.to_string(), window, sink });
    CTRL.store(false, Ordering::Relaxed);
    ALT.store(false, Ordering::Relaxed);
    let mut thread = THREAD.lock().unwrap_or_else(|e| e.into_inner());
    if thread.is_none() {
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("key-hook".into()).spawn(move || {
            // SAFETY: plain Win32 calls; the hook procedure lives as long as the process
            unsafe {
                let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), GetModuleHandleW(std::ptr::null()), 0);
                let _ = tx.send(GetCurrentThreadId());
                if hook.is_null() {
                    log::warn!("raccourcis Windows : hook clavier impossible");
                    return;
                }
                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                UnhookWindowsHookEx(hook);
            }
        });
        if spawned.is_ok() {
            *thread = rx.recv().ok();
        }
    }
}

/// Stops the capture of `owner` (another tab may have taken it over meanwhile).
pub fn release_for(owner: &str) {
    let mine = TARGET.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|t| t.owner == owner);
    if mine {
        release();
    }
}

/// Stops the capture: keys held down are released on the remote side, the hook is removed.
pub fn release() {
    let target = TARGET.lock().unwrap_or_else(|e| e.into_inner()).take();
    let held: Vec<u16> = std::mem::take(&mut *PRESSED.lock().unwrap_or_else(|e| e.into_inner())).into_iter().collect();
    if let Some(t) = target {
        for sc in held {
            (t.sink)(HookEvent::Key(sc, true));
        }
    }
    if let Some(id) = THREAD.lock().unwrap_or_else(|e| e.into_inner()).take() {
        // SAFETY: posting WM_QUIT to the hook thread ends its message loop
        unsafe {
            PostThreadMessageW(id, WM_QUIT, 0, 0);
        }
    }
}

/// Decides what a key does while captured: None lets the system have it.
fn decide(vk: u32, scan: u32, flags: u32, ctrl: bool, alt: bool) -> Option<Option<HookEvent>> {
    if flags & LLKHF_INJECTED != 0 {
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
        // never wait in a low-level hook: a busy lock lets the key through
        if let Ok(guard) = TARGET.try_lock()
            && let Some(t) = guard.as_ref()
            // SAFETY: no arguments
            && unsafe { GetForegroundWindow() } as isize == t.window
            && let Some(action) = decide(vk, k.scanCode, k.flags, CTRL.load(Ordering::Relaxed), ALT.load(Ordering::Relaxed))
        {
            if let Some(ev) = action {
                if let HookEvent::Key(sc, released) = ev
                    && let Ok(mut held) = PRESSED.try_lock()
                {
                    if released {
                        held.remove(&sc);
                    } else {
                        held.insert(sc);
                    }
                }
                (t.sink)(ev);
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
        assert_eq!(decide(0x5B, 0x5B, LLKHF_EXTENDED, false, false), Some(Some(HookEvent::Key(0xE05B, false))));
        // Tab released
        assert_eq!(decide(0x09, 0x0F, LLKHF_UP, false, true), Some(Some(HookEvent::Key(0x0F, true))));
        // Ctrl+Alt+End → Ctrl+Alt+Del, its release swallowed
        assert_eq!(decide(u32::from(VK_END), 0x4F, LLKHF_EXTENDED, true, true), Some(Some(HookEvent::CtrlAltDel)));
        assert_eq!(decide(u32::from(VK_END), 0x4F, LLKHF_EXTENDED | LLKHF_UP, true, true), Some(None));
        // End alone is a normal key
        assert_eq!(decide(u32::from(VK_END), 0x4F, LLKHF_EXTENDED, false, false), Some(Some(HookEvent::Key(0xE04F, false))));
        // Ctrl+Alt+Pause switches full screen
        assert_eq!(decide(u32::from(VK_PAUSE), 0x45, 0, true, true), Some(Some(HookEvent::ToggleFullScreen)));
        // AltGr's fake Ctrl is dropped, injected keys go to the system
        assert_eq!(decide(u32::from(VK_LCONTROL), 0x21D, 0, false, false), Some(None));
        assert_eq!(decide(0x41, 0x1E, LLKHF_INJECTED, false, false), None);
    }
}
