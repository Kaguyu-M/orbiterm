//! Minimal Remote Desktop Gateway (MS-TSGU, HTTP transport over WebSocket) to test Orbiterm's
//! gateway support: HTTPS with a self-signed certificate, Basic authentication
//! (`gw` / `gwpass`), then the RDP bytes are relayed to the resource the client asks for.
//!
//! Logs: `gw:refusé`, `gw:auth-ok`, `gw:canal <resource>:<port>`, `gw:fin <bytes>`.

use std::net::SocketAddr;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::Role;

pub const USER: &str = "gw";
pub const PASSWORD: &str = "gwpass";

const HANDSHAKE_REQ: u16 = 0x01;
const HANDSHAKE_RESP: u16 = 0x02;
const TUNNEL_CREATE: u16 = 0x04;
const TUNNEL_RESP: u16 = 0x05;
const TUNNEL_AUTH: u16 = 0x06;
const TUNNEL_AUTH_RESP: u16 = 0x07;
const CHANNEL_CREATE: u16 = 0x08;
const CHANNEL_RESP: u16 = 0x09;
const DATA: u16 = 0x0A;
const KEEPALIVE: u16 = 0x0D;
const CHANNEL_CLOSE: u16 = 0x10;

fn packet(ty: u16, body: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(8 + body.len());
    p.extend_from_slice(&ty.to_le_bytes());
    p.extend_from_slice(&0u16.to_le_bytes());
    p.extend_from_slice(&((8 + body.len()) as u32).to_le_bytes());
    p.extend_from_slice(body);
    p
}

pub async fn serve(addr: SocketAddr, log: mpsc::UnboundedSender<String>) -> anyhow::Result<()> {
    let (acceptor, _) = super::tls()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    loop {
        let (stream, _) = listener.accept().await?;
        let (acceptor, log) = (acceptor.clone(), log.clone());
        tokio::spawn(async move {
            let r = async {
                let tls = acceptor.accept(stream).await?;
                connection(tls, &log).await
            }
            .await;
            if let Err(e) = r {
                let _ = log.send(format!("gw:erreur {e:#}"));
            }
        });
    }
}

/// HTTP requests until an authenticated RDG_OUT_DATA WebSocket upgrade.
async fn connection<S: AsyncRead + AsyncWrite + Unpin>(mut s: S, log: &mpsc::UnboundedSender<String>) -> anyhow::Result<()> {
    let expected = format!("Basic {}", b64(format!("{USER}:{PASSWORD}").as_bytes()));
    loop {
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            let mut b = [0u8; 1];
            if s.read(&mut b).await? == 0 {
                return Ok(());
            }
            head.push(b[0]);
            anyhow::ensure!(head.len() < 64 * 1024, "en-tête HTTP trop long");
        }
        let text = String::from_utf8_lossy(&head).to_string();
        let header = |name: &str| {
            text.lines()
                .find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case(name)).map(|(_, v)| v.trim().to_string()))
        };
        let method = text.split(' ').next().unwrap_or_default().to_string();
        if header("authorization").as_deref() != Some(expected.as_str()) {
            if header("authorization").is_some() {
                let _ = log.send("gw:refusé".into());
            }
            s.write_all(b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"orbiterm-test\"\r\nContent-Length: 0\r\n\r\n")
                .await?;
            continue;
        }
        anyhow::ensure!(method == "RDG_OUT_DATA", "méthode {method} non prise en charge (WebSocket seulement)");
        let key = header("sec-websocket-key").ok_or_else(|| anyhow::anyhow!("pas de clé WebSocket"))?;
        let accept = derive_accept_key(key.as_bytes());
        s.write_all(
            format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n")
                .as_bytes(),
        )
        .await?;
        let _ = log.send("gw:auth-ok".into());
        let ws = WebSocketStream::from_raw_socket(s, Role::Server, None).await;
        return tunnel(ws, log).await;
    }
}

async fn tunnel<S: AsyncRead + AsyncWrite + Unpin>(
    mut ws: WebSocketStream<S>,
    log: &mpsc::UnboundedSender<String>,
) -> anyhow::Result<()> {
    let mut rdp: Option<TcpStream> = None;
    // setup: one MS-TSGU packet per WebSocket message
    while rdp.is_none() {
        let Some(m) = ws.next().await else { return Ok(()) };
        let Message::Binary(p) = m? else { continue };
        anyhow::ensure!(p.len() >= 8, "paquet trop court");
        let ty = u16::from_le_bytes([p[0], p[1]]);
        let body = &p[8..];
        let reply = match ty {
            HANDSHAKE_REQ => packet(HANDSHAKE_RESP, &[0, 0, 0, 0, 1, 0, 0, 0, 0, 0]),
            TUNNEL_CREATE => packet(TUNNEL_RESP, &[0; 10]),
            TUNNEL_AUTH => packet(TUNNEL_AUTH_RESP, &[0; 8]),
            CHANNEL_CREATE => {
                let port = u16::from_le_bytes([body[2], body[3]]);
                let len = usize::from(u16::from_le_bytes([body[6], body[7]]));
                let units: Vec<u16> = body[8..8 + len].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                let resource = String::from_utf16_lossy(&units).trim_end_matches('\0').to_string();
                let _ = log.send(format!("gw:canal {resource}:{port}"));
                match TcpStream::connect((resource.as_str(), port)).await {
                    Ok(t) => {
                        rdp = Some(t);
                        packet(CHANNEL_RESP, &[0; 8])
                    }
                    // E_PROXY_TS_CONNECTFAILED
                    Err(_) => packet(CHANNEL_RESP, &[0xDB, 0x59, 0x07, 0x80, 0, 0, 0, 0]),
                }
            }
            KEEPALIVE => continue,
            other => anyhow::bail!("paquet inattendu 0x{other:x}"),
        };
        ws.send(Message::binary(reply)).await?;
    }

    // relay
    let (mut rd, mut wr) = rdp.unwrap().into_split();
    let (mut sink, mut stream) = ws.split();
    let mut total = 0usize;
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        tokio::select! {
            n = rd.read(&mut buf) => {
                let n = n?;
                if n == 0 { break; }
                let mut body = (n as u16).to_le_bytes().to_vec();
                body.extend_from_slice(&buf[..n]);
                sink.send(Message::binary(packet(DATA, &body))).await?;
            }
            m = stream.next() => {
                let Some(m) = m else { break };
                let Message::Binary(p) = m? else { continue };
                match u16::from_le_bytes([p[0], p[1]]) {
                    DATA => {
                        let len = usize::from(u16::from_le_bytes([p[8], p[9]]));
                        wr.write_all(&p[10..10 + len]).await?;
                        total += len;
                    }
                    CHANNEL_CLOSE => break,
                    _ => {}
                }
            }
        }
    }
    let _ = log.send(format!("gw:fin {total}"));
    Ok(())
}

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
