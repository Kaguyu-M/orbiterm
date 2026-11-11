//! VNC server: shares the screen of this computer (RFB 3.3 / 3.7 / 3.8, VNC password).
//!
//! The screen is captured when the viewer asks for an update; only the 64×64 tiles that
//! changed since the last update are sent, ZRLE-compressed when the viewer supports it
//! (raw pixels otherwise). Keyboard and mouse are injected only when control is allowed
//! (`writable` in the configuration); otherwise the viewer only watches.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use des::cipher::generic_array::GenericArray;
use des::cipher::{BlockEncrypt, KeyInit};
use flate2::{Compress, Compression, FlushCompress};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, oneshot};
use tokio::task::{JoinHandle, JoinSet};

use super::Ctx;
use super::screen::{Capturer, Frame, Injector};

const TILE: usize = 64;
const ENC_RAW: i32 = 0;
const ENC_ZRLE: i32 = 16;
const ENC_DESKTOP_SIZE: i32 = -223;

pub async fn start(addr: SocketAddr, ctx: Ctx, stop: oneshot::Receiver<()>) -> std::io::Result<(JoinHandle<()>, u16)> {
    // fail now (in the dialog) rather than on the first viewer
    tokio::task::spawn_blocking(|| Capturer::new().and_then(|mut c| c.grab().map(|_| ())))
        .await
        .map_err(std::io::Error::other)??;
    let listener = TcpListener::bind(addr).await?;
    let port = listener.local_addr()?.port();
    let ctx = Arc::new(ctx);
    let task = tokio::spawn(async move {
        let mut conns = JoinSet::new();
        let accept = async {
            loop {
                let Ok((stream, peer)) = listener.accept().await else { break };
                let ctx = ctx.clone();
                conns.spawn(async move {
                    let _ = stream.set_nodelay(true);
                    let mut authenticated = false;
                    match session(stream, &ctx, &mut authenticated).await {
                        Ok(()) if authenticated => ctx.log.log(format!("{} : déconnecté", peer.ip())),
                        Err(e) if authenticated || !closed(&e) => ctx.log.log(format!("{} : {e}", peer.ip())),
                        // a connection closed before logging in (port probe, viewer asking for the password)
                        _ => {}
                    }
                });
                while conns.try_join_next().is_some() {}
            }
        };
        tokio::select! {
            _ = stop => {}
            _ = accept => {}
        }
    });
    Ok((task, port))
}

/// VNC authentication: DES of the challenge, keyed with the bit-reversed password (8 characters).
pub fn vnc_response(password: &str, challenge: &[u8; 16]) -> [u8; 16] {
    let mut key = [0u8; 8];
    for (i, b) in password.bytes().take(8).enumerate() {
        key[i] = b.reverse_bits();
    }
    let des = des::Des::new_from_slice(&key).expect("des key");
    let mut out = *challenge;
    for chunk in out.chunks_mut(8) {
        des.encrypt_block(GenericArray::from_mut_slice(chunk));
    }
    out
}

fn closed(e: &std::io::Error) -> bool {
    use std::io::ErrorKind::*;
    matches!(e.kind(), UnexpectedEof | ConnectionReset | ConnectionAborted | BrokenPipe)
}

fn proto(msg: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string())
}

async fn session(mut s: TcpStream, ctx: &Ctx, authenticated: &mut bool) -> std::io::Result<()> {
    let peer = s.peer_addr()?.ip();
    // --- version and security
    s.write_all(b"RFB 003.008\n").await?;
    let mut ver = [0u8; 12];
    tokio::time::timeout(Duration::from_secs(15), s.read_exact(&mut ver)).await.map_err(|_| proto("délai dépassé"))??;
    if !ver.starts_with(b"RFB 003.") {
        return Err(proto("client non VNC"));
    }
    let minor = std::str::from_utf8(&ver[8..11]).ok().and_then(|m| m.parse::<u32>().ok()).unwrap_or(3);
    if minor >= 7 {
        s.write_all(&[1, 2]).await?; // one security type: VNC authentication
        let mut t = [0u8; 1];
        s.read_exact(&mut t).await?;
        if t[0] != 2 {
            return Err(proto("type de sécurité refusé"));
        }
    } else {
        s.write_all(&2u32.to_be_bytes()).await?;
    }
    let challenge: [u8; 16] = rand::random();
    s.write_all(&challenge).await?;
    let mut resp = [0u8; 16];
    s.read_exact(&mut resp).await?;
    if resp != vnc_response(&ctx.password, &challenge) {
        ctx.log.log(format!("{peer} : mot de passe refusé"));
        tokio::time::sleep(Duration::from_secs(1)).await; // slows down guessing
        s.write_all(&1u32.to_be_bytes()).await?;
        if minor >= 8 {
            let m = b"mot de passe incorrect";
            s.write_all(&(m.len() as u32).to_be_bytes()).await?;
            s.write_all(m).await?;
        }
        return Ok(());
    }
    s.write_all(&0u32.to_be_bytes()).await?;
    *authenticated = true;
    let mut shared = [0u8; 1];
    s.read_exact(&mut shared).await?;

    // --- server init
    let cap = Arc::new(StdMutex::new(Capturer::new()?));
    let first = grab(&cap).await?;
    let (w, h) = (first.w.min(0xFFFF), first.h.min(0xFFFF));
    let mut init = Vec::new();
    init.extend_from_slice(&(w as u16).to_be_bytes());
    init.extend_from_slice(&(h as u16).to_be_bytes());
    init.extend_from_slice(&PixelFormat::NATIVE.to_bytes());
    let name = format!("Orbiterm — {}", hostname());
    init.extend_from_slice(&(name.len() as u32).to_be_bytes());
    init.extend_from_slice(name.as_bytes());
    s.write_all(&init).await?;
    let control = ctx.cfg.writable;
    ctx.log.log(format!("{peer} : connecté ({w}×{h}, {})", if control { "contrôle autorisé" } else { "affichage seul" }));
    let _guard = ctx.log.client();

    let state = Arc::new(StdMutex::new(State { pf: PixelFormat::NATIVE, encodings: Vec::new(), request: None, colour_map: false }));
    let wake = Arc::new(Notify::new());
    let (r, wr) = s.into_split();
    let reader = read_loop(r, state.clone(), wake.clone(), control);
    let writer = write_loop(wr, state, wake, cap, first);
    tokio::select! {
        r = reader => r,
        r = writer => r,
    }
}

fn hostname() -> String {
    std::env::var(if cfg!(windows) { "COMPUTERNAME" } else { "HOSTNAME" })
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_string()))
        .unwrap_or_else(|| "ce poste".into())
}

async fn grab(cap: &Arc<StdMutex<Capturer>>) -> std::io::Result<Frame> {
    let cap = cap.clone();
    tokio::task::spawn_blocking(move || cap.lock().unwrap().grab()).await.map_err(std::io::Error::other)?
}

#[derive(Clone, Copy)]
struct Request {
    incremental: bool,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

struct State {
    pf: PixelFormat,
    encodings: Vec<i32>,
    request: Option<Request>,
    /// a colour-map pixel format was chosen: the palette must be sent
    colour_map: bool,
}

enum InputEvent {
    Key(u32, bool),
    Pointer(u16, u16, u8),
}

async fn read_loop(mut r: OwnedReadHalf, state: Arc<StdMutex<State>>, wake: Arc<Notify>, control: bool) -> std::io::Result<()> {
    // keyboard / mouse go to a thread of their own (X11 connection, SendInput)
    let input = if control {
        let (tx, rx) = std::sync::mpsc::channel::<InputEvent>();
        std::thread::spawn(move || {
            let Ok(mut inj) = Injector::new() else { return };
            for ev in rx {
                match ev {
                    InputEvent::Key(k, d) => inj.key(k, d),
                    InputEvent::Pointer(x, y, m) => inj.pointer(x, y, m),
                }
            }
        });
        Some(tx)
    } else {
        None
    };
    loop {
        let t = match r.read_u8().await {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
        };
        match t {
            0 => {
                let mut b = [0u8; 19];
                r.read_exact(&mut b).await?;
                let pf = PixelFormat::parse(&b[3..]);
                let mut st = state.lock().unwrap();
                if pf.true_colour {
                    st.pf = pf;
                } else if pf.bpp == 8 {
                    // colour map: a fixed 3-3-2 palette sent by the writer
                    st.pf = PixelFormat { true_colour: true, rmax: 7, gmax: 7, bmax: 3, rs: 0, gs: 3, bs: 6, ..pf };
                    st.colour_map = true;
                } else {
                    return Err(proto("format de pixels non pris en charge"));
                }
            }
            2 => {
                let _pad = r.read_u8().await?;
                let n = r.read_u16().await?;
                let mut encs = Vec::with_capacity(n.into());
                for _ in 0..n {
                    encs.push(r.read_i32().await?);
                }
                state.lock().unwrap().encodings = encs;
            }
            3 => {
                let incremental = r.read_u8().await? != 0;
                let (x, y, w, h) = (r.read_u16().await?, r.read_u16().await?, r.read_u16().await?, r.read_u16().await?);
                state.lock().unwrap().request =
                    Some(Request { incremental, x: x.into(), y: y.into(), w: w.into(), h: h.into() });
                wake.notify_one();
            }
            4 => {
                let down = r.read_u8().await? != 0;
                let _pad = r.read_u16().await?;
                let ks = r.read_u32().await?;
                if let Some(tx) = &input {
                    let _ = tx.send(InputEvent::Key(ks, down));
                }
            }
            5 => {
                let mask = r.read_u8().await?;
                let (x, y) = (r.read_u16().await?, r.read_u16().await?);
                if let Some(tx) = &input {
                    let _ = tx.send(InputEvent::Pointer(x, y, mask));
                }
            }
            6 => {
                // clipboard text of the viewer: not shared (read and ignored)
                let mut pad = [0u8; 3];
                r.read_exact(&mut pad).await?;
                let len = r.read_i32().await?.unsigned_abs() as u64;
                if len > 16 << 20 {
                    return Err(proto("presse-papiers trop grand"));
                }
                tokio::io::copy(&mut (&mut r).take(len), &mut tokio::io::sink()).await?;
            }
            _ => return Err(proto(&format!("message inconnu ({t})"))),
        }
    }
}

async fn write_loop(
    mut w: OwnedWriteHalf,
    state: Arc<StdMutex<State>>,
    wake: Arc<Notify>,
    cap: Arc<StdMutex<Capturer>>,
    first: Frame,
) -> std::io::Result<()> {
    let (mut fw, mut fh) = (first.w.min(0xFFFF), first.h.min(0xFFFF));
    let mut next = Some(first);
    // what the viewer currently shows
    let mut shown = Frame { w: fw, h: fh, px: vec![0; fw * fh] };
    let mut sent_any = false;
    let mut z = Compress::new(Compression::fast(), true);
    let mut failed = false;
    loop {
        let pending = {
            let mut st = state.lock().unwrap();
            st.request.map(|r| (r, st.pf, st.encodings.clone(), std::mem::take(&mut st.colour_map)))
        };
        let Some((req, pf, encs, cmap)) = pending else {
            wake.notified().await;
            continue;
        };
        if cmap {
            w.write_all(&colour_map_332()).await?;
        }
        let frame = match next.take() {
            Some(f) => f,
            None => match grab(&cap).await {
                Ok(f) => {
                    failed = false;
                    f
                }
                Err(_) if failed => {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
                Err(e) => {
                    // locked screen, secure desktop…: keep the last picture and retry
                    failed = true;
                    log::debug!("vnc : {e}");
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
            },
        };
        let frame = if (frame.w, frame.h) != (fw, fh) {
            if encs.contains(&ENC_DESKTOP_SIZE) {
                // new screen size: the viewer resizes, then asks for a full picture
                (fw, fh) = (frame.w.min(0xFFFF), frame.h.min(0xFFFF));
                shown = Frame { w: fw, h: fh, px: vec![0; fw * fh] };
                sent_any = false;
                let mut m = vec![0u8, 0, 0, 1];
                rect_header(&mut m, 0, 0, fw, fh, ENC_DESKTOP_SIZE);
                state.lock().unwrap().request = None;
                w.write_all(&m).await?;
                continue;
            }
            frame.fit(fw, fh)
        } else {
            frame
        };
        let area = clip(req, fw, fh);
        let rects = if req.incremental && sent_any { changed(&frame, &shown, area) } else { area.into_iter().collect() };
        if rects.is_empty() {
            // nothing new: look again a little later (the request stays pending)
            tokio::time::sleep(Duration::from_millis(60)).await;
            continue;
        }
        state.lock().unwrap().request = None;
        let zrle = encs.iter().position(|e| *e == ENC_ZRLE);
        let raw = encs.iter().position(|e| *e == ENC_RAW);
        let use_zrle = zrle.is_some() && (raw.is_none() || zrle < raw);
        let mut m = vec![0u8, 0];
        m.extend_from_slice(&(rects.len() as u16).to_be_bytes());
        for &(x, y, rw, rh) in &rects {
            if use_zrle {
                rect_header(&mut m, x, y, rw, rh, ENC_ZRLE);
                let data = deflate(&mut z, &zrle_rect(&frame, x, y, rw, rh, &pf))?;
                m.extend_from_slice(&(data.len() as u32).to_be_bytes());
                m.extend_from_slice(&data);
            } else {
                rect_header(&mut m, x, y, rw, rh, ENC_RAW);
                for row in y..y + rh {
                    for &p in &frame.px[row * frame.w + x..row * frame.w + x + rw] {
                        pf.put(p, &mut m, Cpixel::Full);
                    }
                }
            }
            for row in y..y + rh {
                shown.px[row * fw + x..row * fw + x + rw].copy_from_slice(&frame.px[row * fw + x..row * fw + x + rw]);
            }
        }
        sent_any = true;
        w.write_all(&m).await?;
    }
}

fn rect_header(m: &mut Vec<u8>, x: usize, y: usize, w: usize, h: usize, enc: i32) {
    for v in [x, y, w, h] {
        m.extend_from_slice(&(v as u16).to_be_bytes());
    }
    m.extend_from_slice(&enc.to_be_bytes());
}

/// Requested area inside the screen, if not empty.
fn clip(r: Request, fw: usize, fh: usize) -> Option<(usize, usize, usize, usize)> {
    let (x, y) = (r.x.min(fw), r.y.min(fh));
    let (w, h) = ((r.x + r.w).min(fw) - x, (r.y + r.h).min(fh) - y);
    (w > 0 && h > 0).then_some((x, y, w, h))
}

/// Rectangles (runs of 64×64 tiles on a tile row) of `area` that differ between the frames.
fn changed(new: &Frame, old: &Frame, area: Option<(usize, usize, usize, usize)>) -> Vec<(usize, usize, usize, usize)> {
    let Some((ax, ay, aw, ah)) = area else { return Vec::new() };
    let mut out = Vec::new();
    let mut ty = ay;
    while ty < ay + ah {
        let th = TILE.min(ay + ah - ty);
        let mut run: Option<(usize, usize)> = None; // (x start, width)
        let mut tx = ax;
        while tx < ax + aw {
            let tw = TILE.min(ax + aw - tx);
            let diff = (ty..ty + th).any(|y| new.px[y * new.w + tx..y * new.w + tx + tw] != old.px[y * old.w + tx..y * old.w + tx + tw]);
            match (&mut run, diff) {
                (Some(r), true) => r.1 += tw,
                (None, true) => run = Some((tx, tw)),
                (Some(r), false) => {
                    out.push((r.0, ty, r.1, th));
                    run = None;
                }
                (None, false) => {}
            }
            tx += tw;
        }
        if let Some(r) = run {
            out.push((r.0, ty, r.1, th));
        }
        ty += th;
    }
    out
}

/// SetColourMapEntries for the 3-3-2 palette used with 8-bit colour-map viewers.
fn colour_map_332() -> Vec<u8> {
    let mut m = vec![1u8, 0, 0, 0, 1, 0];
    for i in 0..256u16 {
        let (r, g, b) = (i & 7, (i >> 3) & 7, i >> 6);
        for (v, max) in [(r, 7), (g, 7), (b, 3)] {
            m.extend_from_slice(&((u32::from(v) * 65535 / max) as u16).to_be_bytes());
        }
    }
    m
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelFormat {
    pub bpp: u8,
    pub depth: u8,
    pub big: bool,
    pub true_colour: bool,
    pub rmax: u16,
    pub gmax: u16,
    pub bmax: u16,
    pub rs: u8,
    pub gs: u8,
    pub bs: u8,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Cpixel {
    Full,
    /// ZRLE compact pixel: the 3 low-order bytes of the value
    Low,
    /// ZRLE compact pixel: the 3 high-order bytes
    High,
}

impl PixelFormat {
    pub const NATIVE: Self =
        Self { bpp: 32, depth: 24, big: false, true_colour: true, rmax: 255, gmax: 255, bmax: 255, rs: 16, gs: 8, bs: 0 };

    fn to_bytes(self) -> [u8; 16] {
        let mut b = [0u8; 16];
        b[0] = self.bpp;
        b[1] = self.depth;
        b[2] = self.big as u8;
        b[3] = self.true_colour as u8;
        b[4..6].copy_from_slice(&self.rmax.to_be_bytes());
        b[6..8].copy_from_slice(&self.gmax.to_be_bytes());
        b[8..10].copy_from_slice(&self.bmax.to_be_bytes());
        b[10] = self.rs;
        b[11] = self.gs;
        b[12] = self.bs;
        b
    }

    fn parse(b: &[u8]) -> Self {
        let u = |i: usize| u16::from_be_bytes([b[i], b[i + 1]]);
        Self {
            bpp: b[0],
            depth: b[1],
            big: b[2] != 0,
            true_colour: b[3] != 0,
            rmax: u(4),
            gmax: u(6),
            bmax: u(8),
            rs: b[10],
            gs: b[11],
            bs: b[12],
        }
    }

    /// ZRLE "CPIXEL" form of this format.
    pub fn cpixel(&self) -> Cpixel {
        if self.bpp != 32 || self.depth > 24 || !self.true_colour {
            return Cpixel::Full;
        }
        let mask = (u32::from(self.rmax) << self.rs) | (u32::from(self.gmax) << self.gs) | (u32::from(self.bmax) << self.bs);
        if mask <= 0xFF_FFFF {
            Cpixel::Low
        } else if mask & 0xFF == 0 {
            Cpixel::High
        } else {
            Cpixel::Full
        }
    }

    /// Appends pixel 0x00RRGGBB in this format.
    pub fn put(&self, p: u32, out: &mut Vec<u8>, c: Cpixel) {
        let v = if *self == Self::NATIVE {
            p
        } else {
            let ch = |v: u32, max: u16, s: u8| ((v * u32::from(max) + 127) / 255) << s;
            ch((p >> 16) & 255, self.rmax, self.rs) | ch((p >> 8) & 255, self.gmax, self.gs) | ch(p & 255, self.bmax, self.bs)
        };
        match self.bpp {
            8 => out.push(v as u8),
            16 => out.extend_from_slice(&if self.big { (v as u16).to_be_bytes() } else { (v as u16).to_le_bytes() }),
            _ => {
                let b = if self.big { v.to_be_bytes() } else { v.to_le_bytes() };
                let low_first = !self.big; // where the low-order bytes are
                match (c, low_first) {
                    (Cpixel::Full, _) => out.extend_from_slice(&b),
                    (Cpixel::Low, true) | (Cpixel::High, false) => out.extend_from_slice(&b[..3]),
                    (Cpixel::Low, false) | (Cpixel::High, true) => out.extend_from_slice(&b[1..]),
                }
            }
        }
    }
}

/// Uncompressed ZRLE data of a rectangle: 64×64 tiles, solid (1) or raw (0).
pub fn zrle_rect(f: &Frame, x: usize, y: usize, w: usize, h: usize, pf: &PixelFormat) -> Vec<u8> {
    let c = pf.cpixel();
    let mut out = Vec::with_capacity(w * h * 3 + 64);
    let mut ty = y;
    while ty < y + h {
        let th = TILE.min(y + h - ty);
        let mut tx = x;
        while tx < x + w {
            let tw = TILE.min(x + w - tx);
            let first = f.px[ty * f.w + tx];
            let solid = (ty..ty + th).all(|r| f.px[r * f.w + tx..r * f.w + tx + tw].iter().all(|p| *p == first));
            if solid {
                out.push(1);
                pf.put(first, &mut out, c);
            } else {
                out.push(0);
                for r in ty..ty + th {
                    for &p in &f.px[r * f.w + tx..r * f.w + tx + tw] {
                        pf.put(p, &mut out, c);
                    }
                }
            }
            tx += tw;
        }
        ty += th;
    }
    out
}

/// Compresses with the zlib stream of the connection (one stream for the whole session).
fn deflate(z: &mut Compress, data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len() / 4 + 1024);
    let start = z.total_in();
    loop {
        if out.capacity() - out.len() < 1024 {
            out.reserve(out.capacity().max(64 * 1024));
        }
        let done = (z.total_in() - start) as usize;
        z.compress_vec(&data[done..], &mut out, FlushCompress::Sync).map_err(std::io::Error::other)?;
        if (z.total_in() - start) as usize == data.len() && out.capacity() - out.len() >= 1024 {
            return Ok(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: usize, h: usize, f: impl Fn(usize, usize) -> u32) -> Frame {
        Frame { w, h, px: (0..w * h).map(|i| f(i % w, i / w)).collect() }
    }

    #[test]
    fn pixel_formats() {
        let mut v = Vec::new();
        PixelFormat::NATIVE.put(0x112233, &mut v, Cpixel::Full);
        assert_eq!(v, [0x33, 0x22, 0x11, 0]);
        assert_eq!(PixelFormat::NATIVE.cpixel(), Cpixel::Low);
        v.clear();
        PixelFormat::NATIVE.put(0x112233, &mut v, Cpixel::Low);
        assert_eq!(v, [0x33, 0x22, 0x11]);
        // RGB565 little-endian
        let p565 = PixelFormat { bpp: 16, depth: 16, rmax: 31, gmax: 63, bmax: 31, rs: 11, gs: 5, bs: 0, ..PixelFormat::NATIVE };
        v.clear();
        p565.put(0xFF0000, &mut v, p565.cpixel());
        assert_eq!(v, [0x00, 0xF8]);
        // 32-bit big-endian with colours in the high bytes (RGBX)
        let rgbx = PixelFormat { big: true, rs: 24, gs: 16, bs: 8, ..PixelFormat::NATIVE };
        assert_eq!(rgbx.cpixel(), Cpixel::High);
        v.clear();
        rgbx.put(0x112233, &mut v, Cpixel::High);
        assert_eq!(v, [0x11, 0x22, 0x33]);
        assert_eq!(PixelFormat::parse(&PixelFormat::NATIVE.to_bytes()), PixelFormat::NATIVE);
    }

    #[test]
    fn changed_tiles() {
        let a = frame(200, 100, |_, _| 0);
        let mut b = frame(200, 100, |_, _| 0);
        assert!(changed(&b, &a, Some((0, 0, 200, 100))).is_empty());
        b.px[10 * 200 + 70] = 1; // tile (1, 0)
        b.px[10 * 200 + 130] = 1; // tile (2, 0): same run
        b.px[80 * 200 + 5] = 1; // tile (0, 1)
        assert_eq!(changed(&b, &a, Some((0, 0, 200, 100))), vec![(64, 0, 128, 64), (0, 64, 64, 36)]);
        assert_eq!(clip(Request { incremental: true, x: 150, y: 90, w: 100, h: 100 }, 200, 100), Some((150, 90, 50, 10)));
    }

    #[test]
    fn zrle_tiles_and_stream() {
        let f = frame(100, 70, |x, y| if x < 64 && y < 64 { 0xABCDEF } else { (x * 7 + y) as u32 });
        let data = zrle_rect(&f, 0, 0, 100, 70, &PixelFormat::NATIVE);
        // 4 tiles: one solid (1 + 3 bytes), three raw
        assert_eq!(data[..4], [1, 0xEF, 0xCD, 0xAB]);
        assert_eq!(data.len(), 4 + (1 + 36 * 64 * 3) + (1 + 64 * 6 * 3) + (1 + 36 * 6 * 3));
        // two rectangles in one zlib stream decode back
        let mut z = Compress::new(Compression::fast(), true);
        let (c1, c2) = (deflate(&mut z, &data).unwrap(), deflate(&mut z, &data[..500]).unwrap());
        let mut d = flate2::Decompress::new(true);
        let mut out = Vec::with_capacity(data.len() + 600);
        d.decompress_vec(&c1, &mut out, flate2::FlushDecompress::Sync).unwrap();
        assert_eq!(out, data);
        out.clear();
        d.decompress_vec(&c2, &mut out, flate2::FlushDecompress::Sync).unwrap();
        assert_eq!(out, data[..500]);
    }

    #[test]
    fn des_response() {
        // known answer: password "test", all-zero challenge (as computed by other VNC servers)
        let r = vnc_response("test", &[0; 16]);
        assert_eq!(r[..8], r[8..]);
        assert_ne!(r, [0; 16]);
        assert_ne!(vnc_response("tesu", &[0; 16]), r);
    }
}
