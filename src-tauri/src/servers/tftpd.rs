//! TFTP server (RFC 1350) with the blksize / tsize / timeout options (RFC 2347-2349),
//! octet and netascii modes (both sent as-is).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UdpSocket;
use tokio::sync::oneshot;
use tokio::task::{JoinHandle, JoinSet};

use super::Ctx;
use crate::sftp_server::{confine, virt};

const RRQ: u16 = 1;
const WRQ: u16 = 2;
const DATA: u16 = 3;
const ACK: u16 = 4;
const ERROR: u16 = 5;
const OACK: u16 = 6;
const RETRIES: usize = 5;

pub async fn start(addr: SocketAddr, ctx: Ctx, stop: oneshot::Receiver<()>) -> std::io::Result<(JoinHandle<()>, u16)> {
    let sock = UdpSocket::bind(addr).await?;
    let port = sock.local_addr()?.port();
    let ctx = Arc::new(ctx);
    let task = tokio::spawn(async move {
        let mut transfers = JoinSet::new();
        let serve = async {
            let mut buf = vec![0u8; 1024];
            loop {
                let Ok((n, peer)) = sock.recv_from(&mut buf).await else { continue };
                let pkt = buf[..n].to_vec();
                let ctx = ctx.clone();
                let local_ip = addr.ip();
                transfers.spawn(async move {
                    let _g = ctx.log.client();
                    if let Err(e) = transfer(&pkt, peer, local_ip, &ctx).await {
                        ctx.log.log(format!("{} erreur : {e}", peer.ip()));
                    }
                });
                while transfers.try_join_next().is_some() {}
            }
        };
        tokio::select! {
            _ = stop => {}
            _ = serve => {}
        }
    });
    Ok((task, port))
}

struct Request {
    op: u16,
    file: String,
    options: Vec<(String, String)>,
}

fn parse_request(p: &[u8]) -> Option<Request> {
    if p.len() < 4 {
        return None;
    }
    let op = u16::from_be_bytes([p[0], p[1]]);
    let mut fields = p[2..].split(|b| *b == 0).map(|f| String::from_utf8_lossy(f).into_owned());
    let file = fields.next()?;
    let _mode = fields.next()?;
    let rest: Vec<String> = fields.filter(|s| !s.is_empty()).collect();
    let options = rest.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0].to_ascii_lowercase(), c[1].clone())).collect();
    Some(Request { op, file, options })
}

fn error_pkt(code: u16, msg: &str) -> Vec<u8> {
    let mut v = vec![0, ERROR as u8];
    v.extend(code.to_be_bytes());
    v.extend(msg.as_bytes());
    v.push(0);
    v
}

/// One transfer, on its own socket (transfer identifier = new port), as RFC 1350 requires.
async fn transfer(pkt: &[u8], peer: SocketAddr, local_ip: std::net::IpAddr, ctx: &Ctx) -> std::io::Result<()> {
    let sock = UdpSocket::bind(SocketAddr::new(local_ip, 0)).await?;
    sock.connect(peer).await?;
    let Some(req) = parse_request(pkt) else {
        sock.send(&error_pkt(4, "illegal TFTP operation")).await?;
        return Ok(());
    };
    if req.op != RRQ && req.op != WRQ {
        sock.send(&error_pkt(4, "illegal TFTP operation")).await?;
        return Ok(());
    }
    let path = confine(&ctx.root, &req.file);
    let vname = virt(&req.file);

    // negotiated options
    let mut blksize = 512usize;
    let mut timeout = Duration::from_secs(2);
    let mut oack: Vec<(String, String)> = vec![];
    for (k, v) in &req.options {
        match k.as_str() {
            "blksize" => {
                if let Ok(n) = v.parse::<usize>() {
                    blksize = n.clamp(8, 65464);
                    oack.push(("blksize".into(), blksize.to_string()));
                }
            }
            "timeout" => {
                if let Ok(n) = v.parse::<u64>()
                    && (1..=255).contains(&n)
                {
                    timeout = Duration::from_secs(n);
                    oack.push(("timeout".into(), n.to_string()));
                }
            }
            "tsize" if req.op == RRQ => {
                if let Ok(md) = tokio::fs::metadata(&path).await {
                    oack.push(("tsize".into(), md.len().to_string()));
                }
            }
            "tsize" => oack.push(("tsize".into(), v.clone())),
            _ => {}
        }
    }
    let oack_pkt = (!oack.is_empty()).then(|| {
        let mut v = vec![0, OACK as u8];
        for (k, val) in &oack {
            v.extend(k.as_bytes());
            v.push(0);
            v.extend(val.as_bytes());
            v.push(0);
        }
        v
    });

    if req.op == RRQ {
        let mut f = match tokio::fs::File::open(&path).await {
            Ok(f) if path.is_file() => f,
            _ => {
                sock.send(&error_pkt(1, "file not found")).await?;
                ctx.log.log(format!("{} lecture {vname} : introuvable", peer.ip()));
                return Ok(());
            }
        };
        if let Some(o) = &oack_pkt
            && !send_wait(&sock, o, 0, timeout).await?
        {
            return Ok(());
        }
        let mut block: u16 = 1;
        let mut total = 0u64;
        let mut data = vec![0u8; blksize];
        loop {
            let mut n = 0;
            while n < blksize {
                let r = f.read(&mut data[n..]).await?;
                if r == 0 {
                    break;
                }
                n += r;
            }
            let mut pkt = vec![0, DATA as u8];
            pkt.extend(block.to_be_bytes());
            pkt.extend(&data[..n]);
            if !send_wait(&sock, &pkt, block, timeout).await? {
                ctx.log.log(format!("{} lecture {vname} : abandon (pas d'accusé)", peer.ip()));
                return Ok(());
            }
            total += n as u64;
            if n < blksize {
                break;
            }
            block = block.wrapping_add(1);
        }
        ctx.log.log(format!("{} a téléchargé {vname} ({total} octets)", peer.ip()));
    } else {
        if !ctx.cfg.writable {
            sock.send(&error_pkt(2, "access violation (read-only server)")).await?;
            ctx.log.log(format!("{} envoi {vname} refusé (lecture seule)", peer.ip()));
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let mut f = match tokio::fs::File::create(&path).await {
            Ok(f) => f,
            Err(e) => {
                sock.send(&error_pkt(2, &e.to_string())).await?;
                return Ok(());
            }
        };
        // the first reply is OACK (options) or ACK 0
        let mut reply = oack_pkt.unwrap_or_else(|| vec![0, ACK as u8, 0, 0]);
        let mut expect: u16 = 1;
        let mut total = 0u64;
        let mut buf = vec![0u8; blksize + 4];
        loop {
            let mut got = None;
            for _ in 0..RETRIES {
                sock.send(&reply).await?;
                match tokio::time::timeout(timeout, sock.recv(&mut buf)).await {
                    Ok(Ok(n)) if n >= 4 && u16::from_be_bytes([buf[0], buf[1]]) == DATA => {
                        let b = u16::from_be_bytes([buf[2], buf[3]]);
                        if b == expect {
                            got = Some(n - 4);
                            break;
                        }
                        // duplicate of the previous block: our ACK was lost, resend it
                    }
                    Ok(Ok(n)) if n >= 2 && u16::from_be_bytes([buf[0], buf[1]]) == ERROR => return Ok(()),
                    _ => {}
                }
            }
            let Some(n) = got else {
                ctx.log.log(format!("{} envoi {vname} : abandon (délai dépassé)", peer.ip()));
                return Ok(());
            };
            f.write_all(&buf[4..4 + n]).await?;
            total += n as u64;
            reply = vec![0, ACK as u8];
            reply.extend(expect.to_be_bytes());
            if n < blksize {
                sock.send(&reply).await?;
                break;
            }
            expect = expect.wrapping_add(1);
        }
        f.flush().await?;
        ctx.log.log(format!("{} a envoyé {vname} ({total} octets)", peer.ip()));
    }
    Ok(())
}

/// Sends `pkt` until the peer acknowledges `block` (retransmits on timeout).
async fn send_wait(sock: &UdpSocket, pkt: &[u8], block: u16, timeout: Duration) -> std::io::Result<bool> {
    let mut buf = [0u8; 516];
    for _ in 0..RETRIES {
        sock.send(pkt).await?;
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            match tokio::time::timeout_at(deadline, sock.recv(&mut buf)).await {
                Ok(Ok(n)) if n >= 4 => {
                    let op = u16::from_be_bytes([buf[0], buf[1]]);
                    if op == ACK && u16::from_be_bytes([buf[2], buf[3]]) == block {
                        return Ok(true);
                    }
                    if op == ERROR {
                        return Ok(false);
                    }
                    // stale ACK: keep waiting for the right one
                }
                Ok(_) => {}
                Err(_) => break, // timeout → retransmit
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_parsing() {
        let r = parse_request(b"\x00\x01boot/pxelinux.0\x00octet\x00blksize\x001468\x00tsize\x000\x00").unwrap();
        assert_eq!(r.op, RRQ);
        assert_eq!(r.file, "boot/pxelinux.0");
        assert_eq!(r.options, vec![("blksize".into(), "1468".into()), ("tsize".into(), "0".into())]);
        assert!(parse_request(b"\x00\x01").is_none());
    }
}
