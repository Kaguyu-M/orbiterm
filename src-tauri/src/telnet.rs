//! Telnet (RFC 854) and Rlogin (RFC 1282) clients over TCP.
//!
//! Telnet negotiation: we accept server ECHO and SUPPRESS-GO-AHEAD, offer
//! TERMINAL-TYPE (xterm-256color) and NAWS (window size), refuse everything else.

use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::Mutex;

use crate::error::{AppError, AppResult};
use crate::ssh::{CloseSink, DataSink};

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;
const OPT_ECHO: u8 = 1;
const OPT_SGA: u8 = 3;
const OPT_TTYPE: u8 = 24;
const OPT_NAWS: u8 = 31;

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Telnet,
    Rlogin,
}

pub struct TcpTermConnection {
    mode: Mode,
    writer: Mutex<OwnedWriteHalf>,
    size: StdMutex<(u16, u16)>,
    naws_enabled: Arc<StdMutex<bool>>,
}

fn naws(cols: u16, rows: u16) -> Vec<u8> {
    let mut v = vec![IAC, SB, OPT_NAWS];
    for b in [cols.to_be_bytes(), rows.to_be_bytes()].concat() {
        v.push(b);
        if b == IAC {
            v.push(IAC);
        }
    }
    v.extend([IAC, SE]);
    v
}

/// Parses telnet commands out of `input`, returns (payload, replies).
pub struct TelnetParser {
    state: u8, // 0 data, 1 IAC, 2 option verb, 3 subneg, 4 subneg IAC
    verb: u8,
    sub: Vec<u8>,
}

impl TelnetParser {
    pub fn new() -> Self {
        Self { state: 0, verb: 0, sub: vec![] }
    }

    pub fn feed(&mut self, input: &[u8], size: (u16, u16), naws_on: &mut bool) -> (Vec<u8>, Vec<u8>) {
        let mut data = Vec::with_capacity(input.len());
        let mut reply = vec![];
        for &b in input {
            match self.state {
                0 => {
                    if b == IAC { self.state = 1 } else { data.push(b) }
                }
                1 => match b {
                    IAC => {
                        data.push(IAC);
                        self.state = 0;
                    }
                    DO | DONT | WILL | WONT => {
                        self.verb = b;
                        self.state = 2;
                    }
                    SB => {
                        self.sub.clear();
                        self.state = 3;
                    }
                    _ => self.state = 0,
                },
                2 => {
                    match (self.verb, b) {
                        (DO, OPT_TTYPE) => reply.extend([IAC, WILL, OPT_TTYPE]),
                        (DO, OPT_NAWS) => {
                            reply.extend([IAC, WILL, OPT_NAWS]);
                            reply.extend(naws(size.0, size.1));
                            *naws_on = true;
                        }
                        (DO, OPT_SGA) => reply.extend([IAC, WILL, OPT_SGA]),
                        (DO, o) => reply.extend([IAC, WONT, o]),
                        (WILL, OPT_ECHO) | (WILL, OPT_SGA) => reply.extend([IAC, DO, b]),
                        (WILL, o) => reply.extend([IAC, DONT, o]),
                        _ => {}
                    }
                    self.state = 0;
                }
                3 => {
                    if b == IAC { self.state = 4 } else { self.sub.push(b) }
                }
                _ => {
                    if b == SE {
                        // TERMINAL-TYPE SEND -> IS xterm-256color
                        if self.sub.first() == Some(&OPT_TTYPE) && self.sub.get(1) == Some(&1) {
                            reply.extend([IAC, SB, OPT_TTYPE, 0]);
                            reply.extend(b"xterm-256color");
                            reply.extend([IAC, SE]);
                        }
                        self.state = 0;
                    } else {
                        self.sub.push(b);
                        self.state = 3;
                    }
                }
            }
        }
        (data, reply)
    }
}

impl Default for TelnetParser {
    fn default() -> Self {
        Self::new()
    }
}

impl TcpTermConnection {
    #[allow(clippy::too_many_arguments)]
    pub async fn connect(
        mode: Mode,
        host: &str,
        port: u16,
        username: &str,
        cols: u16,
        rows: u16,
        on_data: DataSink,
        on_close: CloseSink,
    ) -> AppResult<Arc<TcpTermConnection>> {
        let stream = tokio::time::timeout(Duration::from_secs(15), TcpStream::connect((host, port)))
            .await
            .map_err(|_| AppError::Timeout)??;
        let _ = stream.set_nodelay(true);
        let (mut rd, mut wr) = stream.into_split();
        if mode == Mode::Rlogin {
            let user = if username.is_empty() { "user" } else { username };
            let hello = format!("\0{user}\0{user}\0xterm-256color/38400\0");
            wr.write_all(hello.as_bytes()).await?;
        }
        let conn = Arc::new(TcpTermConnection {
            mode,
            writer: Mutex::new(wr),
            size: StdMutex::new((cols, rows)),
            naws_enabled: Arc::new(StdMutex::new(false)),
        });
        let c2 = conn.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 16 * 1024];
            let mut parser = TelnetParser::new();
            let mut rlogin_first = true;
            loop {
                let n = match rd.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                match c2.mode {
                    Mode::Telnet => {
                        let size = *c2.size.lock().unwrap();
                        let mut on = *c2.naws_enabled.lock().unwrap();
                        let (data, reply) = parser.feed(&buf[..n], size, &mut on);
                        *c2.naws_enabled.lock().unwrap() = on;
                        if !reply.is_empty() {
                            let _ = c2.writer.lock().await.write_all(&reply).await;
                        }
                        if !data.is_empty() {
                            on_data(data);
                        }
                    }
                    Mode::Rlogin => {
                        // server acknowledges with a single NUL byte first
                        let mut slice = &buf[..n];
                        if rlogin_first {
                            rlogin_first = false;
                            if slice.first() == Some(&0) {
                                slice = &slice[1..];
                            }
                        }
                        if !slice.is_empty() {
                            on_data(slice.to_vec());
                        }
                    }
                }
            }
            on_close("Connexion fermée".into());
        });
        Ok(conn)
    }

    pub async fn write(&self, data: &[u8]) -> AppResult<()> {
        let mut w = self.writer.lock().await;
        if self.mode == Mode::Telnet && data.contains(&IAC) {
            let mut esc = Vec::with_capacity(data.len() + 4);
            for &b in data {
                esc.push(b);
                if b == IAC {
                    esc.push(IAC);
                }
            }
            w.write_all(&esc).await?;
        } else {
            w.write_all(data).await?;
        }
        Ok(())
    }

    pub async fn resize(&self, cols: u32, rows: u32) -> AppResult<()> {
        let (c, r) = (cols.min(u16::MAX as u32) as u16, rows.min(u16::MAX as u32) as u16);
        *self.size.lock().unwrap() = (c, r);
        if self.mode == Mode::Telnet && *self.naws_enabled.lock().unwrap() {
            self.writer.lock().await.write_all(&naws(c, r)).await?;
        }
        Ok(())
    }

    pub async fn close(&self) {
        let _ = self.writer.lock().await.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiation() {
        let mut p = TelnetParser::new();
        let mut on = false;
        let input = [b'h', IAC, DO, OPT_NAWS, b'i', IAC, WILL, OPT_ECHO, IAC, DO, 39, IAC, IAC];
        let (data, reply) = p.feed(&input, (80, 24), &mut on);
        assert_eq!(data, vec![b'h', b'i', IAC]);
        assert!(on);
        assert_eq!(&reply[..3], &[IAC, WILL, OPT_NAWS]);
        assert_eq!(&reply[3..12], &[IAC, SB, OPT_NAWS, 0, 80, 0, 24, IAC, SE]);
        assert_eq!(&reply[12..15], &[IAC, DO, OPT_ECHO]);
        assert_eq!(&reply[15..18], &[IAC, WONT, 39]);
    }

    #[test]
    fn terminal_type_subnegotiation() {
        let mut p = TelnetParser::new();
        let mut on = false;
        let (_, reply) = p.feed(&[IAC, SB, OPT_TTYPE, 1, IAC, SE], (80, 24), &mut on);
        let mut expected = vec![IAC, SB, OPT_TTYPE, 0];
        expected.extend(b"xterm-256color");
        expected.extend([IAC, SE]);
        assert_eq!(reply, expected);
    }
}
