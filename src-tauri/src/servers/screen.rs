//! Screen of this computer for the VNC server: capture (GDI on Windows, X11 on Linux) and
//! keyboard / mouse injection (SendInput, XTest). Pixels are 0x00RRGGBB.

pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

impl Frame {
    /// Same picture on a canvas of another size (cropped or padded with black).
    pub fn fit(&self, w: usize, h: usize) -> Frame {
        let mut px = vec![0u32; w * h];
        for y in 0..h.min(self.h) {
            let n = w.min(self.w);
            px[y * w..y * w + n].copy_from_slice(&self.px[y * self.w..y * self.w + n]);
        }
        Frame { w, h, px }
    }

    /// Draws a mouse arrow (the captured picture does not contain the pointer).
    pub fn draw_pointer(&mut self, x: i32, y: i32) {
        const ARROW: [&str; 19] = [
            "X", "XX", "X.X", "X..X", "X...X", "X....X", "X.....X", "X......X", "X.......X", "X........X", "X.........X",
            "X..........X", "X......XXXXX", "X...X..X", "X..XX..X", "X.X  X..X", "XX   X..X", "X     X..X", "      XXX",
        ];
        for (dy, row) in ARROW.iter().enumerate() {
            for (dx, c) in row.bytes().enumerate() {
                let (px, py) = (x + dx as i32, y + dy as i32);
                if c == b' ' || px < 0 || py < 0 || px as usize >= self.w || py as usize >= self.h {
                    continue;
                }
                self.px[py as usize * self.w + px as usize] = if c == b'X' { 0 } else { 0xFF_FFFF };
            }
        }
    }
}

/// Character typed by a printable keysym (Latin-1 or Unicode keysyms).
pub fn keysym_char(ks: u32) -> Option<char> {
    match ks {
        0x20..=0x7e | 0xa0..=0xff => char::from_u32(ks),
        0x0100_0000..=0x0110_ffff => char::from_u32(ks - 0x0100_0000),
        0x20ac => Some('€'),
        _ => None,
    }
}

pub use imp::{Capturer, Injector};

// ------------------------------------------------------------------ Windows

#[cfg(windows)]
mod imp {
    use super::{Frame, keysym_char};
    use std::io;
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    fn virtual_screen() -> (i32, i32, i32, i32) {
        unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        }
    }

    pub struct Capturer;

    impl Capturer {
        pub fn new() -> io::Result<Self> {
            Ok(Capturer)
        }

        /// Whole virtual desktop (every monitor).
        pub fn grab(&mut self) -> io::Result<Frame> {
            let (x, y, w, h) = virtual_screen();
            if w <= 0 || h <= 0 {
                return Err(io::Error::other("aucun écran"));
            }
            let mut px = vec![0u32; (w * h) as usize];
            unsafe {
                let src = GetDC(std::ptr::null_mut());
                if src.is_null() {
                    return Err(io::Error::other("écran inaccessible"));
                }
                let mem = CreateCompatibleDC(src);
                let bmp = CreateCompatibleBitmap(src, w, h);
                let old = SelectObject(mem, bmp as HGDIOBJ);
                let ok = BitBlt(mem, 0, 0, w, h, src, x, y, SRCCOPY) != 0;
                let mut bi: BITMAPINFO = zeroed();
                bi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
                bi.bmiHeader.biWidth = w;
                bi.bmiHeader.biHeight = -h; // top-down rows
                bi.bmiHeader.biPlanes = 1;
                bi.bmiHeader.biBitCount = 32;
                bi.bmiHeader.biCompression = BI_RGB as _;
                let lines = GetDIBits(mem, bmp, 0, h as u32, px.as_mut_ptr().cast(), &mut bi, DIB_RGB_COLORS);
                SelectObject(mem, old);
                DeleteObject(bmp as HGDIOBJ);
                DeleteDC(mem);
                ReleaseDC(std::ptr::null_mut(), src);
                // the secure desktop (UAC, lock screen) cannot be captured
                if !ok || lines != h {
                    return Err(io::Error::other("capture de l'écran impossible (écran verrouillé ou bureau sécurisé ?)"));
                }
            }
            for p in px.iter_mut() {
                *p &= 0xFF_FFFF;
            }
            let mut f = Frame { w: w as usize, h: h as usize, px };
            unsafe {
                let mut ci: CURSORINFO = zeroed();
                ci.cbSize = size_of::<CURSORINFO>() as u32;
                if GetCursorInfo(&mut ci) != 0 && ci.flags & CURSOR_SHOWING != 0 {
                    f.draw_pointer(ci.ptScreenPos.x - x, ci.ptScreenPos.y - y);
                }
            }
            Ok(f)
        }
    }

    /// (virtual key, extended key) of the non-printable keysyms.
    pub fn special_vk(ks: u32) -> Option<(u16, bool)> {
        Some(match ks {
            0xff08 => (0x08, false), // BackSpace
            0xff09 | 0xfe20 => (0x09, false),
            0xff0d => (0x0D, false),
            0xff1b => (0x1B, false),
            0xff63 => (0x2D, true), // Insert
            0xffff => (0x2E, true), // Delete
            0xff50 => (0x24, true), // Home
            0xff57 => (0x23, true), // End
            0xff55 => (0x21, true), // Page up
            0xff56 => (0x22, true), // Page down
            0xff51 => (0x25, true),
            0xff52 => (0x26, true),
            0xff53 => (0x27, true),
            0xff54 => (0x28, true),
            0xffbe..=0xffd5 => (0x70 + (ks - 0xffbe) as u16, false), // F1..F24
            0xffe1 => (0xA0, false),
            0xffe2 => (0xA1, false),
            0xffe3 => (0xA2, false),
            0xffe4 => (0xA3, true),
            0xffe9 | 0xffe7 => (0xA4, false),
            0xffea | 0xffe8 | 0xfe03 => (0xA5, true), // right Alt / AltGr
            0xffeb => (0x5B, true),
            0xffec => (0x5C, true),
            0xff67 => (0x5D, true),
            0xffe5 => (0x14, false),
            0xff7f => (0x90, true),
            0xff14 => (0x91, false),
            0xff13 => (0x13, false),
            0xff61 => (0x2C, true),
            0xff8d => (0x0D, true), // keypad Enter
            0xffb0..=0xffb9 => (0x60 + (ks - 0xffb0) as u16, false),
            0xffae => (0x6E, false),
            0xffab => (0x6B, false),
            0xffad => (0x6D, false),
            0xffaa => (0x6A, false),
            0xffaf => (0x6F, true),
            0xff95 => (0x24, false),
            0xff96 => (0x25, false),
            0xff97 => (0x26, false),
            0xff98 => (0x27, false),
            0xff99 => (0x28, false),
            0xff9a => (0x21, false),
            0xff9b => (0x22, false),
            0xff9c => (0x23, false),
            0xff9e => (0x2D, false),
            0xff9f => (0x2E, false),
            _ => return None,
        })
    }

    fn send(inputs: &[INPUT]) {
        unsafe {
            SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32);
        }
    }

    fn key_input(vk: u16, scan: u16, flags: u32) -> INPUT {
        let mut i: INPUT = unsafe { zeroed() };
        i.r#type = INPUT_KEYBOARD;
        i.Anonymous.ki = KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags as _, time: 0, dwExtraInfo: 0 };
        i
    }

    fn mouse_input(dx: i32, dy: i32, data: i32, flags: u32) -> INPUT {
        let mut i: INPUT = unsafe { zeroed() };
        i.r#type = INPUT_MOUSE;
        i.Anonymous.mi = MOUSEINPUT { dx, dy, mouseData: data as _, dwFlags: flags as _, time: 0, dwExtraInfo: 0 };
        i
    }

    #[derive(Default)]
    pub struct Injector {
        mask: u8,
        /// virtual keys held down (released when the viewer leaves)
        held: Vec<(u16, bool)>,
        /// characters typed as Unicode, by keysym (their key-up must match)
        unicode: Vec<u32>,
    }

    impl Injector {
        pub fn new() -> io::Result<Self> {
            Ok(Self::default())
        }

        fn vk(&mut self, vk: u16, ext: bool, down: bool) {
            let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
            let mut flags = if ext { KEYEVENTF_EXTENDEDKEY } else { 0 };
            if !down {
                flags |= KEYEVENTF_KEYUP;
            }
            send(&[key_input(vk, scan, flags)]);
            self.held.retain(|k| k.0 != vk);
            if down {
                self.held.push((vk, ext));
            }
        }

        fn shortcut_held(&self) -> bool {
            // Ctrl, left Alt or Windows key: the key is a shortcut, not text…
            let h = |vk: u16| self.held.iter().any(|k| k.0 == vk);
            let (lctrl, rctrl, lalt, ralt, win) = (h(0xA2), h(0xA3), h(0xA4), h(0xA5), h(0x5B) || h(0x5C));
            // …except left Ctrl + right Alt, which is how Windows viewers send AltGr
            let altgr = ralt && lctrl && !rctrl && !lalt && !win;
            (lctrl || rctrl || lalt || win) && !altgr
        }

        pub fn key(&mut self, ks: u32, down: bool) {
            if let Some((vk, ext)) = special_vk(ks) {
                return self.vk(vk, ext, down);
            }
            let Some(ch) = keysym_char(ks) else { return };
            if !down && self.unicode.contains(&ks) {
                self.unicode.retain(|k| *k != ks);
                return self.text(ch, false);
            }
            if self.shortcut_held() {
                let lower = ch.to_lowercase().next().unwrap_or(ch);
                let r = unsafe { VkKeyScanW(lower as u16) };
                if r != -1 {
                    return self.vk((r & 0xff) as u16, false, down);
                }
            }
            if down {
                self.unicode.push(ks);
                self.text(ch, true);
            }
        }

        /// Types a character whatever the keyboard layout of this computer.
        fn text(&mut self, ch: char, down: bool) {
            let mut buf = [0u16; 2];
            let flags = KEYEVENTF_UNICODE | if down { 0 } else { KEYEVENTF_KEYUP };
            let inputs: Vec<INPUT> = ch.encode_utf16(&mut buf).iter().map(|u| key_input(0, *u, flags)).collect();
            send(&inputs);
        }

        pub fn pointer(&mut self, x: u16, y: u16, mask: u8) {
            let (_, _, w, h) = virtual_screen();
            let nx = (i32::from(x) * 65535) / (w - 1).max(1);
            let ny = (i32::from(y) * 65535) / (h - 1).max(1);
            let mut inputs = vec![mouse_input(nx, ny, 0, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK)];
            let changed = mask ^ self.mask;
            for (bit, dn, up) in [
                (1u8, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
                (2, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
                (4, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
            ] {
                if changed & bit != 0 {
                    inputs.push(mouse_input(0, 0, 0, if mask & bit != 0 { dn } else { up }));
                }
            }
            // wheel: buttons 4/5 (vertical) and 6/7 (horizontal), counted when pressed
            for (bit, delta, flag) in
                [(8u8, 120, MOUSEEVENTF_WHEEL), (16, -120, MOUSEEVENTF_WHEEL), (32, -120, MOUSEEVENTF_HWHEEL), (64, 120, MOUSEEVENTF_HWHEEL)]
            {
                if mask & bit != 0 && self.mask & bit == 0 {
                    inputs.push(mouse_input(0, 0, delta, flag));
                }
            }
            self.mask = mask;
            send(&inputs);
        }
    }

    impl Drop for Injector {
        fn drop(&mut self) {
            for (vk, ext) in std::mem::take(&mut self.held) {
                self.vk(vk, ext, false);
            }
            if self.mask & 7 != 0 {
                let mut p = POINT { x: 0, y: 0 };
                unsafe { GetCursorPos(&mut p) };
                let (vx, vy, _, _) = virtual_screen();
                self.pointer((p.x - vx).max(0) as u16, (p.y - vy).max(0) as u16, 0);
            }
        }
    }
}

// ------------------------------------------------------------------ Linux (X11)

#[cfg(target_os = "linux")]
mod imp {
    use super::{Frame, keysym_char};
    use std::io;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{self, ConnectionExt as _, ImageFormat, ImageOrder};
    use x11rb::protocol::xtest::ConnectionExt as _;
    use x11rb::rust_connection::RustConnection;

    fn err(e: impl std::fmt::Display) -> io::Error {
        io::Error::other(format!("X11 : {e}"))
    }

    fn connect() -> io::Result<(RustConnection, xproto::Window, u16, u16)> {
        if std::env::var_os("DISPLAY").is_none() {
            return Err(io::Error::other(if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                "session Wayland sans X11 : le partage d'écran n'est pas pris en charge"
            } else {
                "aucun affichage X11 (DISPLAY)"
            }));
        }
        let (c, n) = x11rb::connect(None).map_err(err)?;
        let s = &c.setup().roots[n];
        let (root, w, h) = (s.root, s.width_in_pixels, s.height_in_pixels);
        Ok((c, root, w, h))
    }

    pub struct Capturer {
        c: RustConnection,
        root: xproto::Window,
    }

    impl Capturer {
        pub fn new() -> io::Result<Self> {
            let (c, root, _, _) = connect()?;
            let s = c.setup();
            let fmt = s.pixmap_formats.iter().find(|f| f.depth == 24).map(|f| f.bits_per_pixel);
            if fmt != Some(32) || s.image_byte_order != ImageOrder::LSB_FIRST {
                return Err(io::Error::other("format d'affichage X11 non pris en charge (24 bits / 32 bpp attendu)"));
            }
            Ok(Self { c, root })
        }

        pub fn grab(&mut self) -> io::Result<Frame> {
            let g = self.c.get_geometry(self.root).map_err(err)?.reply().map_err(err)?;
            let (w, h) = (g.width, g.height);
            let img = self
                .c
                .get_image(ImageFormat::Z_PIXMAP, self.root, 0, 0, w, h, !0)
                .map_err(err)?
                .reply()
                .map_err(err)?;
            let px: Vec<u32> =
                img.data.chunks_exact(4).map(|p| u32::from_le_bytes([p[0], p[1], p[2], 0])).collect();
            if px.len() < usize::from(w) * usize::from(h) {
                return Err(io::Error::other("image X11 incomplète"));
            }
            let mut f = Frame { w: w.into(), h: h.into(), px };
            if let Ok(Ok(p)) = self.c.query_pointer(self.root).map(|r| r.reply()) {
                f.draw_pointer(p.root_x.into(), p.root_y.into());
            }
            Ok(f)
        }
    }

    pub struct Injector {
        c: RustConnection,
        root: xproto::Window,
        min: u8,
        per: usize,
        map: Vec<u32>,
        mask: u8,
        /// keys held down: (keysym, keycode)
        held: Vec<(u32, u8)>,
        /// keycodes without symbols, lent to characters missing from the keyboard map
        spare: Vec<u8>,
        lent: Vec<(u32, u8)>,
    }

    const SHIFT_L: u32 = 0xffe1;

    impl Injector {
        pub fn new() -> io::Result<Self> {
            let (c, root, _, _) = connect()?;
            let (min, max) = (c.setup().min_keycode, c.setup().max_keycode);
            let m = c.get_keyboard_mapping(min, max - min + 1).map_err(err)?.reply().map_err(err)?;
            let per = usize::from(m.keysyms_per_keycode).max(1);
            let spare = m.keysyms.chunks(per).enumerate().filter(|(_, k)| k.iter().all(|s| *s == 0)).map(|(i, _)| min + i as u8).collect();
            Ok(Self { c, root, min, per, map: m.keysyms, mask: 0, held: Vec::new(), spare, lent: Vec::new() })
        }

        /// (keycode, column): column 1 = with Shift.
        fn lookup(&self, ks: u32) -> Option<(u8, usize)> {
            // first two columns (plain, Shift) of the main group
            (0..2).find_map(|col| {
                self.map.chunks(self.per).position(|k| k.get(col) == Some(&ks)).map(|i| (self.min + i as u8, col))
            })
        }

        fn keysym(ks: u32) -> u32 {
            let ks = match keysym_char(ks) {
                Some(c) if (c as u32) < 0x100 => c as u32,
                _ => ks,
            };
            if ks == 0xfe20 { 0xff09 } else { ks }
        }

        /// Keycode for a keysym missing from the map: a spare keycode is given that symbol.
        fn lend(&mut self, ks: u32) -> Option<u8> {
            if let Some((_, code)) = self.lent.iter().find(|l| l.0 == ks) {
                return Some(*code);
            }
            let code = if self.lent.len() < self.spare.len() {
                self.spare[self.lent.len()]
            } else {
                let (_, code) = *self.lent.first()?; // reuse the oldest one
                self.lent.remove(0);
                code
            };
            let syms = vec![ks; self.per];
            self.c.change_keyboard_mapping(1, code, self.per as u8, &syms).ok()?;
            let _ = self.c.get_input_focus().map(|r| r.reply()); // applied before the key event
            self.lent.push((ks, code));
            Some(code)
        }

        fn fake(&self, kind: u8, detail: u8) {
            let _ = self.c.xtest_fake_input(kind, detail, 0, self.root, 0, 0, 0);
        }

        pub fn key(&mut self, ks: u32, down: bool) {
            let ks = Self::keysym(ks);
            if !down {
                if let Some(i) = self.held.iter().position(|h| h.0 == ks) {
                    let (_, code) = self.held.remove(i);
                    self.fake(xproto::KEY_RELEASE_EVENT, code);
                    let _ = self.c.flush();
                }
                return;
            }
            let (code, col) = match self.lookup(ks) {
                Some(found) => found,
                None => match self.lend(ks) {
                    Some(code) => (code, 0),
                    None => return,
                },
            };
            // a character typed with Shift on this keyboard, while the viewer did not send Shift
            let shift_held = self.held.iter().any(|h| matches!(h.0, 0xffe1 | 0xffe2));
            let shift = (col == 1 && !shift_held).then(|| self.lookup(SHIFT_L)).flatten();
            if let Some((s, _)) = shift {
                self.fake(xproto::KEY_PRESS_EVENT, s);
            }
            self.fake(xproto::KEY_PRESS_EVENT, code);
            if let Some((s, _)) = shift {
                self.fake(xproto::KEY_RELEASE_EVENT, s);
            }
            self.held.push((ks, code));
            let _ = self.c.flush();
        }

        pub fn pointer(&mut self, x: u16, y: u16, mask: u8) {
            let _ = self.c.xtest_fake_input(xproto::MOTION_NOTIFY_EVENT, 0, 0, self.root, x as i16, y as i16, 0);
            for b in 0..7u8 {
                let bit = 1 << b;
                if (mask ^ self.mask) & bit != 0 {
                    self.fake(if mask & bit != 0 { xproto::BUTTON_PRESS_EVENT } else { xproto::BUTTON_RELEASE_EVENT }, b + 1);
                }
            }
            self.mask = mask;
            let _ = self.c.flush();
        }
    }

    impl Drop for Injector {
        fn drop(&mut self) {
            for (_, code) in std::mem::take(&mut self.held) {
                self.fake(xproto::KEY_RELEASE_EVENT, code);
            }
            // give the lent keycodes back their (empty) mapping
            for (_, code) in std::mem::take(&mut self.lent) {
                let _ = self.c.change_keyboard_mapping(1, code, self.per as u8, &vec![0; self.per]);
            }
            for b in 0..7u8 {
                if self.mask & (1 << b) != 0 {
                    self.fake(xproto::BUTTON_RELEASE_EVENT, b + 1);
                }
            }
            let _ = self.c.flush();
        }
    }
}

// ------------------------------------------------------------------ other systems

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    use super::Frame;
    use std::io;

    fn unsupported() -> io::Error {
        io::Error::new(io::ErrorKind::Unsupported, "partage d'écran non pris en charge sur ce système")
    }

    pub struct Capturer;
    impl Capturer {
        pub fn new() -> io::Result<Self> {
            Err(unsupported())
        }
        pub fn grab(&mut self) -> io::Result<Frame> {
            Err(unsupported())
        }
    }

    pub struct Injector;
    impl Injector {
        pub fn new() -> io::Result<Self> {
            Err(unsupported())
        }
        pub fn key(&mut self, _ks: u32, _down: bool) {}
        pub fn pointer(&mut self, _x: u16, _y: u16, _mask: u8) {}
    }
}

