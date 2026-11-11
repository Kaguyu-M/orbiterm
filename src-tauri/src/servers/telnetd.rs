//! Telnet server: login (user + password), then the local shell of this computer in a
//! pseudo-terminal. Telnet is not encrypted: meant for a local network or old equipment.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio::task::{JoinHandle, JoinSet};

use super::Ctx;
use super::sshd::configured_shell;
use crate::pty::{PtyConnection, PtySpawn};

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;
const ECHO: u8 = 1;
const SGA: u8 = 3;
const NAWS: u8 = 31;

pub async fn start(addr: SocketAddr, ctx: Ctx, stop: oneshot::Receiver<()>) -> std::io::Result<(JoinHandle<()>, u16)> {
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
                    let _guard = ctx.log.client();
                    if let Err(e) = client(stream, peer, &ctx).await {
                        ctx.log.log(format!("{} : {e}", peer.ip()));
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

/// Telnet input decoder: strips commands, reports window sizes, turns CR LF / CR NUL into CR.
#[derive(Default)]
pub struct Decoder {
    state: u8,
    sb: Vec<u8>,
    last_cr: bool,
}

pub enum Event {
    Size(u16, u16),
}

impl Decoder {
    /// Returns the user data and the events found in `input`.
    pub fn feed(&mut self, input: &[u8]) -> (Vec<u8>, Vec<Event>) {
        let (mut out, mut ev) = (Vec::with_capacity(input.len()), vec![]);
        for &b in input {
            match self.state {
                0 => {
                    if b == IAC {
                        self.state = 1;
                    } else if self.last_cr && (b == b'\n' || b == 0) {
                        self.last_cr = false; // second byte of an end of line
                    } else {
                        self.last_cr = b == b'\r';
                        out.push(b);
                    }
                }
                1 => match b {
                    IAC => {
                        out.push(IAC);
                        self.state = 0;
                    }
                    WILL | WONT | DO | DONT => self.state = 2,
                    SB => {
                        self.sb.clear();
                        self.state = 3;
                    }
                    _ => self.state = 0,
                },
                2 => self.state = 0, // option byte of WILL / WONT / DO / DONT
                3 => {
                    if b == IAC {
                        self.state = 4;
                    } else if self.sb.len() < 64 {
                        self.sb.push(b);
                    }
                }
                _ => {
                    if b == SE {
                        if self.sb.len() >= 5 && self.sb[0] == NAWS {
                            let w = u16::from_be_bytes([self.sb[1], self.sb[2]]);
                            let h = u16::from_be_bytes([self.sb[3], self.sb[4]]);
                            if w > 0 && h > 0 {
                                ev.push(Event::Size(w, h));
                            }
                        }
                        self.state = 0;
                    } else {
                        if b == IAC {
                            self.sb.push(IAC); // escaped 255 inside the sub-negotiation
                        }
                        self.state = 3;
                    }
                }
            }
        }
        (out, ev)
    }
}

/// Escapes 0xFF for telnet.
fn escape(data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(data.len() + 4);
    for &b in data {
        v.push(b);
        if b == IAC {
            v.push(IAC);
        }
    }
    v
}

/// Login-time input: decoder, window size and decoded bytes not consumed yet (the client
/// may send several lines at once, e.g. a pasted login and password).
struct Input {
    dec: Decoder,
    size: (u16, u16),
    pending: std::collections::VecDeque<u8>,
}

/// Reads one line typed by the client (echoed or not), with backspace.
async fn read_line(s: &mut TcpStream, inp: &mut Input, echo: bool) -> std::io::Result<String> {
    let mut line = Vec::new();
    let mut buf = [0u8; 256];
    loop {
        if inp.pending.is_empty() {
            let n = tokio::time::timeout(Duration::from_secs(60), s.read(&mut buf))
                .await
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "délai de connexion dépassé"))??;
            if n == 0 {
                return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "déconnecté"));
            }
            let (data, ev) = inp.dec.feed(&buf[..n]);
            for Event::Size(w, h) in ev {
                inp.size = (w, h);
            }
            inp.pending.extend(data);
        }
        while let Some(b) = inp.pending.pop_front() {
            match b {
                b'\r' | b'\n' => {
                    s.write_all(b"\r\n").await?;
                    return Ok(String::from_utf8_lossy(&line).trim().to_string());
                }
                8 | 127 => {
                    if line.pop().is_some() && echo {
                        s.write_all(b"\x08 \x08").await?;
                    }
                }
                3 | 4 => return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "abandon")),
                b if b >= 32 && line.len() < 256 => {
                    line.push(b);
                    if echo {
                        s.write_all(&[b]).await?;
                    }
                }
                _ => {}
            }
        }
    }
}

async fn client(mut s: TcpStream, peer: SocketAddr, ctx: &Ctx) -> std::io::Result<()> {
    let _ = s.set_nodelay(true);
    // the server echoes and works in character mode; the client tells its window size
    s.write_all(&[IAC, WILL, ECHO, IAC, WILL, SGA, IAC, DO, NAWS]).await?;
    let mut inp = Input { dec: Decoder::default(), size: (80, 24), pending: Default::default() };
    s.write_all(b"\r\nOrbiterm - serveur Telnet\r\n\r\n").await?;
    let mut ok = false;
    for _ in 0..3 {
        s.write_all(b"login: ").await?;
        let user = read_line(&mut s, &mut inp, true).await?;
        s.write_all(b"Password: ").await?;
        let pw = read_line(&mut s, &mut inp, false).await?;
        if !ctx.password.is_empty() && user == ctx.cfg.user && pw == ctx.password {
            ok = true;
            ctx.log.log(format!("{} connecté en tant que {user}", peer.ip()));
            break;
        }
        ctx.log.log(format!("{} connexion refusée pour « {user} »", peer.ip()));
        tokio::time::sleep(Duration::from_secs(1)).await;
        inp.pending.clear(); // no type-ahead after a failure
        s.write_all(b"Login incorrect\r\n\r\n").await?;
    }
    if !ok {
        return Ok(());
    }

    let Input { mut dec, size, pending } = inp;
    let (program, args) = configured_shell(&ctx.cfg.shell);
    let (tx, mut rx) = mpsc::unbounded_channel::<Option<Vec<u8>>>();
    let tx2 = tx.clone();
    let pty = PtyConnection::spawn(
        PtySpawn { program, args, cwd: Some(ctx.root.clone()), env: vec![], cols: size.0, rows: size.1 },
        Arc::new(move |d| {
            let _ = tx.send(Some(d));
        }),
        Arc::new(move |_| {
            let _ = tx2.send(None);
        }),
    )
    .map_err(|e| std::io::Error::other(e.to_string()))?;

    if !pending.is_empty() {
        let _ = pty.write(&Vec::from(pending)); // typed ahead after the password
    }
    let (mut rd, mut wr) = s.into_split();
    let to_client = async {
        while let Some(Some(d)) = rx.recv().await {
            if wr.write_all(&escape(&d)).await.is_err() {
                break;
            }
        }
        // the shell ended: drain then close
        while let Ok(Some(d)) = rx.try_recv() {
            let _ = wr.write_all(&escape(&d)).await;
        }
        let _ = wr.shutdown().await;
    };
    let from_client = async {
        let mut buf = [0u8; 4096];
        loop {
            let n = match rd.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let (data, ev) = dec.feed(&buf[..n]);
            for Event::Size(w, h) in ev {
                let _ = pty.resize(w as u32, h as u32);
            }
            if !data.is_empty() && pty.write(&data).is_err() {
                break;
            }
        }
    };
    tokio::select! {
        _ = to_client => {}
        _ = from_client => {}
    }
    pty.kill();
    ctx.log.log(format!("{} déconnecté", peer.ip()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_commands_sizes_and_line_ends() {
        let mut d = Decoder::default();
        let (out, ev) = d.feed(&[IAC, WILL, NAWS, b'l', b's', b'\r', 0, IAC, SB, NAWS, 0, 120, 0, 40, IAC, SE, b'x', b'\r', b'\n']);
        assert_eq!(out, b"ls\rx\r");
        assert!(matches!(ev.as_slice(), [Event::Size(120, 40)]));
        // a command split over two reads, and an escaped 255
        let (out, _) = d.feed(&[b'a', IAC]);
        assert_eq!(out, b"a");
        let (out, _) = d.feed(&[DO, ECHO, IAC, IAC]);
        assert_eq!(out, [IAC]);
        assert_eq!(escape(&[1, IAC, 2]), [1, IAC, IAC, 2]);
    }
}
