//! Minimal VNC (RFB 3.8) test server: VNC password authentication, raw encoding,
//! a test pattern, keyboard and mouse feedback. Used by the tests and for manual checks.
//!
//! usage: cargo run --example vnc_test_server -- [port] [password]   (default 5901, "test")
//! picture: 4 colour bars on top; the bottom half turns yellow after the key "a" is
//! pressed, cyan after "b"; a white square follows the mouse.

use std::sync::Arc;

use des::cipher::generic_array::GenericArray;
use des::cipher::{BlockEncrypt, KeyInit};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

pub const W: u16 = 640;
pub const H: u16 = 400;

#[derive(Clone, Copy)]
struct PixelFormat {
    bpp: u8,
    big_endian: bool,
    r_shift: u8,
    g_shift: u8,
    b_shift: u8,
    r_max: u16,
    g_max: u16,
    b_max: u16,
}

impl Default for PixelFormat {
    fn default() -> Self {
        PixelFormat { bpp: 32, big_endian: false, r_shift: 16, g_shift: 8, b_shift: 0, r_max: 255, g_max: 255, b_max: 255 }
    }
}

impl PixelFormat {
    fn bytes(&self) -> [u8; 16] {
        let mut b = [0u8; 16];
        b[0] = self.bpp;
        b[1] = 24;
        b[2] = self.big_endian as u8;
        b[3] = 1;
        b[4..6].copy_from_slice(&self.r_max.to_be_bytes());
        b[6..8].copy_from_slice(&self.g_max.to_be_bytes());
        b[8..10].copy_from_slice(&self.b_max.to_be_bytes());
        b[10] = self.r_shift;
        b[11] = self.g_shift;
        b[12] = self.b_shift;
        b
    }
    fn parse(b: &[u8; 16]) -> Self {
        PixelFormat {
            bpp: b[0],
            big_endian: b[2] != 0,
            r_max: u16::from_be_bytes([b[4], b[5]]),
            g_max: u16::from_be_bytes([b[6], b[7]]),
            b_max: u16::from_be_bytes([b[8], b[9]]),
            r_shift: b[10],
            g_shift: b[11],
            b_shift: b[12],
        }
    }
    fn encode(&self, (r, g, b): (u8, u8, u8), out: &mut Vec<u8>) {
        let sc = |v: u8, max: u16| v as u32 * max as u32 / 255;
        let px = (sc(r, self.r_max) << self.r_shift) | (sc(g, self.g_max) << self.g_shift) | (sc(b, self.b_max) << self.b_shift);
        match (self.bpp, self.big_endian) {
            (32, false) => out.extend_from_slice(&px.to_le_bytes()),
            (32, true) => out.extend_from_slice(&px.to_be_bytes()),
            (16, false) => out.extend_from_slice(&(px as u16).to_le_bytes()),
            (16, true) => out.extend_from_slice(&(px as u16).to_be_bytes()),
            _ => out.push(px as u8),
        }
    }
}

#[derive(Default)]
struct Screen {
    bottom: (u8, u8, u8),
    mouse: (u16, u16),
    keys: Vec<u32>,
}

impl Screen {
    fn pixel(&self, x: u16, y: u16) -> (u8, u8, u8) {
        let (mx, my) = self.mouse;
        if x.abs_diff(mx) < 6 && y.abs_diff(my) < 6 {
            return (255, 255, 255);
        }
        if y < H / 2 {
            match x / (W / 4) {
                0 => (220, 30, 30),
                1 => (30, 180, 60),
                2 => (40, 80, 220),
                _ => (240, 240, 240),
            }
        } else {
            self.bottom
        }
    }
}

/// VNC authentication: DES with the bit-reversed password as key.
pub fn vnc_response(password: &str, challenge: &[u8; 16]) -> [u8; 16] {
    let mut key = [0u8; 8];
    for (i, b) in password.bytes().take(8).enumerate() {
        key[i] = b.reverse_bits();
    }
    let des = des::Des::new_from_slice(&key).expect("des key");
    let mut out = *challenge;
    for chunk in out.chunks_mut(8) {
        let block = GenericArray::from_mut_slice(chunk);
        des.encrypt_block(block);
    }
    out
}

async fn session(mut s: TcpStream, password: Option<String>, log: Arc<Mutex<Vec<String>>>) -> std::io::Result<()> {
    s.write_all(b"RFB 003.008\n").await?;
    let mut ver = [0u8; 12];
    s.read_exact(&mut ver).await?;
    let sec = if password.is_some() { 2u8 } else { 1u8 };
    s.write_all(&[1, sec]).await?;
    let mut chosen = [0u8; 1];
    s.read_exact(&mut chosen).await?;
    if let Some(pw) = &password {
        let challenge: [u8; 16] = *b"orbiterm-vnc-tst";
        s.write_all(&challenge).await?;
        let mut resp = [0u8; 16];
        s.read_exact(&mut resp).await?;
        if resp != vnc_response(pw, &challenge) {
            let reason = b"mot de passe VNC incorrect";
            s.write_all(&1u32.to_be_bytes()).await?;
            s.write_all(&(reason.len() as u32).to_be_bytes()).await?;
            s.write_all(reason).await?;
            log.lock().await.push("auth:fail".into());
            return Ok(());
        }
    }
    s.write_all(&0u32.to_be_bytes()).await?;
    log.lock().await.push("auth:ok".into());
    let mut shared = [0u8; 1];
    s.read_exact(&mut shared).await?;
    let mut pf = PixelFormat::default();
    let name = b"Orbiterm VNC test";
    let mut init = vec![];
    init.extend_from_slice(&W.to_be_bytes());
    init.extend_from_slice(&H.to_be_bytes());
    init.extend_from_slice(&pf.bytes());
    init.extend_from_slice(&(name.len() as u32).to_be_bytes());
    init.extend_from_slice(name);
    s.write_all(&init).await?;

    let mut screen = Screen { bottom: (60, 60, 60), ..Default::default() };
    loop {
        let mut t = [0u8; 1];
        if s.read_exact(&mut t).await.is_err() {
            return Ok(());
        }
        match t[0] {
            0 => {
                let mut b = [0u8; 19];
                s.read_exact(&mut b).await?;
                pf = PixelFormat::parse(b[3..19].try_into().unwrap());
            }
            2 => {
                let mut b = [0u8; 3];
                s.read_exact(&mut b).await?;
                let n = u16::from_be_bytes([b[1], b[2]]) as usize;
                let mut enc = vec![0u8; n * 4];
                s.read_exact(&mut enc).await?;
            }
            3 => {
                let mut b = [0u8; 9];
                s.read_exact(&mut b).await?;
                let incremental = b[0] != 0;
                if incremental {
                    // nothing changes by itself: answer slowly (keeps the viewer polling)
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                let mut msg = vec![0u8, 0, 0, 1];
                for v in [0u16, 0, W, H] {
                    msg.extend_from_slice(&v.to_be_bytes());
                }
                msg.extend_from_slice(&0i32.to_be_bytes()); // raw
                for y in 0..H {
                    for x in 0..W {
                        pf.encode(screen.pixel(x, y), &mut msg);
                    }
                }
                s.write_all(&msg).await?;
            }
            4 => {
                let mut b = [0u8; 7];
                s.read_exact(&mut b).await?;
                let down = b[0] != 0;
                let key = u32::from_be_bytes([b[3], b[4], b[5], b[6]]);
                if down {
                    screen.keys.push(key);
                    log.lock().await.push(format!("key:{key:#x}"));
                    match key {
                        0x61 => screen.bottom = (240, 200, 0),
                        0x62 => screen.bottom = (0, 200, 220),
                        _ => {}
                    }
                }
            }
            5 => {
                let mut b = [0u8; 5];
                s.read_exact(&mut b).await?;
                screen.mouse = (u16::from_be_bytes([b[1], b[2]]), u16::from_be_bytes([b[3], b[4]]));
                if b[0] != 0 {
                    log.lock().await.push(format!("click:{},{}", screen.mouse.0, screen.mouse.1));
                }
            }
            6 => {
                let mut b = [0u8; 7];
                s.read_exact(&mut b).await?;
                let n = u32::from_be_bytes([b[3], b[4], b[5], b[6]]) as usize;
                let mut text = vec![0u8; n.min(1 << 20)];
                s.read_exact(&mut text).await?;
                log.lock().await.push(format!("clip:{}", String::from_utf8_lossy(&text)));
            }
            _ => return Ok(()),
        }
    }
}

/// Accepts VNC clients on `listener` forever; events are appended to `log`.
pub async fn serve(listener: TcpListener, password: Option<String>, log: Arc<Mutex<Vec<String>>>) {
    while let Ok((s, _)) = listener.accept().await {
        let (pw, log) = (password.clone(), log.clone());
        tokio::spawn(async move {
            let _ = session(s, pw, log).await;
        });
    }
}

#[allow(dead_code)]
#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let port: u16 = args.next().and_then(|p| p.parse().ok()).unwrap_or(5901);
    let password = args.next().unwrap_or_else(|| "test".into());
    let listener = TcpListener::bind(("127.0.0.1", port)).await.expect("bind");
    println!("Serveur VNC de test prêt sur 127.0.0.1:{port}  (mot de passe : {password})");
    let log = Arc::new(Mutex::new(vec![]));
    let printer = log.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            for l in printer.lock().await.drain(..) {
                println!("{l}");
            }
        }
    });
    serve(listener, if password.is_empty() { None } else { Some(password) }, log).await;
}
