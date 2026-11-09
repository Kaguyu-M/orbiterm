//! SSH port forwarding: local (-L), dynamic SOCKS4/5 (-D) listeners and the
//! byte pump used by every forwarded channel (remote -R and X11 included).

use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::Arc;

use russh::client::{self, Handle, Msg};
use russh::Channel;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::error::{AppError, AppResult};

/// Copies bytes both ways between a forwarded SSH channel and a local stream.
pub async fn pump<S>(channel: Channel<Msg>, mut local: S)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut remote = channel.into_stream();
    let _ = tokio::io::copy_bidirectional(&mut local, &mut remote).await;
    let _ = remote.shutdown().await;
}

/// Accept loop of a local (-L) forwarding.
pub async fn run_local<H: client::Handler + 'static>(handle: Arc<Handle<H>>, listener: TcpListener, dest_host: String, dest_port: u16) {
    loop {
        let Ok((tcp, peer)) = listener.accept().await else { break };
        let handle = handle.clone();
        let dest_host = dest_host.clone();
        tokio::spawn(async move {
            let _ = tcp.set_nodelay(true);
            match handle
                .channel_open_direct_tcpip(dest_host.clone(), dest_port as u32, peer.ip().to_string(), peer.port() as u32)
                .await
            {
                Ok(ch) => pump(ch, tcp).await,
                Err(e) => log::warn!("forward to {dest_host}:{dest_port} refused: {e}"),
            }
        });
    }
}

/// Accept loop of a dynamic (-D) forwarding = SOCKS4/4a/5 proxy.
pub async fn run_dynamic<H: client::Handler + 'static>(handle: Arc<Handle<H>>, listener: TcpListener) {
    loop {
        let Ok((mut tcp, peer)) = listener.accept().await else { break };
        let handle = handle.clone();
        tokio::spawn(async move {
            let _ = tcp.set_nodelay(true);
            let Ok(req) = socks_handshake(&mut tcp).await else { return };
            match handle
                .channel_open_direct_tcpip(req.host.clone(), req.port as u32, peer.ip().to_string(), peer.port() as u32)
                .await
            {
                Ok(ch) => {
                    if socks_reply(&mut tcp, req.version, true).await.is_ok() {
                        pump(ch, tcp).await;
                    }
                }
                Err(e) => {
                    log::warn!("socks {}:{} refused: {e}", req.host, req.port);
                    let _ = socks_reply(&mut tcp, req.version, false).await;
                }
            }
        });
    }
}

/// Connects to a local destination for a remote (-R) or X11 channel.
pub async fn connect_local(host: &str, port: u16) -> std::io::Result<TcpStream> {
    let s = TcpStream::connect((host, port)).await?;
    let _ = s.set_nodelay(true);
    Ok(s)
}

pub fn bind_addr(host: &str) -> &str {
    if host.trim().is_empty() { "127.0.0.1" } else { host.trim() }
}

pub async fn bind(host: &str, port: u16) -> AppResult<TcpListener> {
    TcpListener::bind((bind_addr(host), port))
        .await
        .map_err(|e| AppError::Conn(format!("impossible d'écouter sur {}:{port} : {e}", bind_addr(host))))
}

// ---------------------------------------------------------------- SOCKS

#[derive(Debug, Clone, PartialEq)]
pub struct SocksRequest {
    pub version: u8,
    pub host: String,
    pub port: u16,
}

async fn read_until_nul<S: AsyncRead + Unpin>(s: &mut S) -> std::io::Result<Vec<u8>> {
    let mut out = vec![];
    loop {
        let b = s.read_u8().await?;
        if b == 0 {
            return Ok(out);
        }
        if out.len() > 255 {
            return Err(std::io::Error::other("field too long"));
        }
        out.push(b);
    }
}

/// Reads a SOCKS4/4a/5 CONNECT request (no authentication).
pub async fn socks_handshake<S: AsyncRead + AsyncWrite + Unpin>(s: &mut S) -> std::io::Result<SocksRequest> {
    let version = s.read_u8().await?;
    match version {
        4 => {
            let cmd = s.read_u8().await?;
            let port = s.read_u16().await?;
            let mut ip = [0u8; 4];
            s.read_exact(&mut ip).await?;
            let _user = read_until_nul(s).await?;
            let host = if ip[0] == 0 && ip[1] == 0 && ip[2] == 0 && ip[3] != 0 {
                String::from_utf8_lossy(&read_until_nul(s).await?).into_owned() // 4a
            } else {
                Ipv4Addr::from(ip).to_string()
            };
            if cmd != 1 {
                s.write_all(&[0, 0x5b, 0, 0, 0, 0, 0, 0]).await?;
                return Err(std::io::Error::other("unsupported socks4 command"));
            }
            Ok(SocksRequest { version, host, port })
        }
        5 => {
            let n = s.read_u8().await? as usize;
            let mut methods = vec![0u8; n];
            s.read_exact(&mut methods).await?;
            if !methods.contains(&0) {
                s.write_all(&[5, 0xff]).await?;
                return Err(std::io::Error::other("no acceptable socks5 auth method"));
            }
            s.write_all(&[5, 0]).await?;
            let mut head = [0u8; 4];
            s.read_exact(&mut head).await?;
            let host = match head[3] {
                1 => {
                    let mut ip = [0u8; 4];
                    s.read_exact(&mut ip).await?;
                    Ipv4Addr::from(ip).to_string()
                }
                3 => {
                    let len = s.read_u8().await? as usize;
                    let mut name = vec![0u8; len];
                    s.read_exact(&mut name).await?;
                    String::from_utf8_lossy(&name).into_owned()
                }
                4 => {
                    let mut ip = [0u8; 16];
                    s.read_exact(&mut ip).await?;
                    Ipv6Addr::from(ip).to_string()
                }
                _ => {
                    s.write_all(&[5, 8, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
                    return Err(std::io::Error::other("bad socks5 address type"));
                }
            };
            let port = s.read_u16().await?;
            if head[1] != 1 {
                s.write_all(&[5, 7, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
                return Err(std::io::Error::other("unsupported socks5 command"));
            }
            Ok(SocksRequest { version, host, port })
        }
        _ => Err(std::io::Error::other("not a socks request")),
    }
}

pub async fn socks_reply<S: AsyncWrite + Unpin>(s: &mut S, version: u8, ok: bool) -> std::io::Result<()> {
    if version == 4 {
        s.write_all(&[0, if ok { 0x5a } else { 0x5b }, 0, 0, 0, 0, 0, 0]).await
    } else {
        s.write_all(&[5, if ok { 0 } else { 5 }, 0, 1, 0, 0, 0, 0, 0, 0]).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn run(input: &[u8]) -> (std::io::Result<SocksRequest>, Vec<u8>) {
        let (mut client, mut server) = tokio::io::duplex(1024);
        client.write_all(input).await.unwrap();
        let r = socks_handshake(&mut server).await;
        drop(server);
        let mut out = vec![];
        client.read_to_end(&mut out).await.unwrap();
        (r, out)
    }

    #[tokio::test]
    async fn socks5_domain() {
        let mut req = vec![5, 1, 0, 5, 1, 0, 3, 11];
        req.extend(b"example.org");
        req.extend(443u16.to_be_bytes());
        let (r, out) = run(&req).await;
        assert_eq!(r.unwrap(), SocksRequest { version: 5, host: "example.org".into(), port: 443 });
        assert_eq!(out, vec![5, 0]);
    }

    #[tokio::test]
    async fn socks5_ipv4_and_bad_command() {
        let (r, _) = run(&[5, 1, 0, 5, 1, 0, 1, 10, 0, 0, 7, 0, 22]).await;
        assert_eq!(r.unwrap(), SocksRequest { version: 5, host: "10.0.0.7".into(), port: 22 });
        let (r, out) = run(&[5, 1, 0, 5, 2, 0, 1, 10, 0, 0, 7, 0, 22]).await;
        assert!(r.is_err());
        // [5,0] method selection, then [5,7,…] command not supported
        assert_eq!(&out[..4], &[5, 0, 5, 7]);
    }

    #[tokio::test]
    async fn socks4_and_4a() {
        let (r, _) = run(&[4, 1, 0, 80, 192, 168, 1, 2, b'u', 0]).await;
        assert_eq!(r.unwrap(), SocksRequest { version: 4, host: "192.168.1.2".into(), port: 80 });
        let mut req = vec![4, 1, 0, 80, 0, 0, 0, 1, 0];
        req.extend(b"host.lan\0");
        let (r, _) = run(&req).await;
        assert_eq!(r.unwrap(), SocksRequest { version: 4, host: "host.lan".into(), port: 80 });
    }
}
