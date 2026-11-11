//! Throughput test server compatible with iperf3 clients (`iperf3 -c <this computer>`):
//! TCP, one or several streams (-P), normal or reverse (-R). One test at a time.
//!
//! Protocol (iperf3): the client opens a control connection and sends a 37-byte cookie;
//! the server drives it through one-byte states, exchanges JSON parameters and results
//! (4-byte big-endian length + JSON); data connections start with the same cookie.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::{JoinHandle, JoinSet};

use super::Ctx;

pub const COOKIE: usize = 37;
pub const TEST_START: u8 = 1;
pub const TEST_RUNNING: u8 = 2;
pub const TEST_END: u8 = 4;
pub const PARAM_EXCHANGE: u8 = 9;
pub const CREATE_STREAMS: u8 = 10;
pub const CLIENT_TERMINATE: u8 = 12;
pub const EXCHANGE_RESULTS: u8 = 13;
pub const DISPLAY_RESULTS: u8 = 14;
pub const IPERF_DONE: u8 = 16;
pub const ACCESS_DENIED: u8 = 0xFF; // -1
pub const SERVER_ERROR: u8 = 0xFE; // -2
/// iperf3 error code: "an option you are trying to set is not implemented yet"
const IEUNIMP: u8 = 13;

/// Data connections waiting for the test that owns their cookie.
type Pending = Arc<Mutex<HashMap<Vec<u8>, mpsc::UnboundedSender<TcpStream>>>>;

pub async fn start(addr: SocketAddr, ctx: Ctx, stop: oneshot::Receiver<()>) -> std::io::Result<(JoinHandle<()>, u16)> {
    let listener = TcpListener::bind(addr).await?;
    let port = listener.local_addr()?.port();
    let ctx = Arc::new(ctx);
    let pending: Pending = Arc::default();
    let busy = Arc::new(AtomicBool::new(false));
    let task = tokio::spawn(async move {
        let mut conns = JoinSet::new();
        let accept = async {
            loop {
                let Ok((mut stream, peer)) = listener.accept().await else { break };
                let (ctx, pending, busy) = (ctx.clone(), pending.clone(), busy.clone());
                conns.spawn(async move {
                    let _ = stream.set_nodelay(true);
                    let mut cookie = vec![0u8; COOKIE];
                    if tokio::time::timeout(Duration::from_secs(10), stream.read_exact(&mut cookie)).await.is_err() {
                        return;
                    }
                    // a data connection of the running test?
                    if let Some(tx) = pending.lock().await.get(&cookie) {
                        let _ = tx.send(stream);
                        return;
                    }
                    if busy.swap(true, Ordering::SeqCst) {
                        let _ = stream.write_all(&[ACCESS_DENIED]).await;
                        return;
                    }
                    let _guard = ctx.log.client();
                    let (tx, rx) = mpsc::unbounded_channel();
                    pending.lock().await.insert(cookie.clone(), tx);
                    if let Err(e) = run_test(stream, peer, rx, &ctx).await {
                        ctx.log.log(format!("{} : test interrompu ({e})", peer.ip()));
                    }
                    pending.lock().await.remove(&cookie);
                    busy.store(false, Ordering::SeqCst);
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

async fn read_json(s: &mut TcpStream) -> std::io::Result<Value> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len).await?;
    let n = u32::from_be_bytes(len) as usize;
    if n > 1 << 20 {
        return Err(std::io::Error::other("message trop long"));
    }
    let mut buf = vec![0u8; n];
    s.read_exact(&mut buf).await?;
    serde_json::from_slice(&buf).map_err(std::io::Error::other)
}

async fn write_json(s: &mut TcpStream, v: &Value) -> std::io::Result<()> {
    let b = serde_json::to_vec(v).map_err(std::io::Error::other)?;
    s.write_all(&(b.len() as u32).to_be_bytes()).await?;
    s.write_all(&b).await
}

async fn state(s: &mut TcpStream) -> std::io::Result<u8> {
    let mut b = [0u8; 1];
    s.read_exact(&mut b).await?;
    Ok(b[0])
}

/// Stream ids as iperf3 numbers them (1, 3, 4, 5…), so the results match the client's.
fn stream_id(index: usize) -> usize {
    if index == 0 { 1 } else { index + 2 }
}

async fn refuse(ctl: &mut TcpStream) {
    // SERVER_ERROR + i_errno IEUNIMP ("option not implemented") + errno 0, big-endian
    let _ = ctl.write_all(&[SERVER_ERROR, 0, 0, 0, IEUNIMP, 0, 0, 0, 0]).await;
}

async fn run_test(mut ctl: TcpStream, peer: SocketAddr, mut data_rx: mpsc::UnboundedReceiver<TcpStream>, ctx: &Ctx) -> std::io::Result<()> {
    ctl.write_all(&[PARAM_EXCHANGE]).await?;
    let params = tokio::time::timeout(Duration::from_secs(10), read_json(&mut ctl))
        .await
        .map_err(|_| std::io::Error::other("paramètres non reçus"))??;
    if params.get("udp").and_then(Value::as_bool).unwrap_or(false) || params.get("sctp").is_some() {
        ctx.log.log(format!("{} : seul le TCP est pris en charge (pas d'option -u)", peer.ip()));
        refuse(&mut ctl).await;
        return Ok(());
    }
    if params.get("authtoken").is_some() {
        ctx.log.log(format!("{} : l'authentification iperf3 n'est pas prise en charge", peer.ip()));
        refuse(&mut ctl).await;
        return Ok(());
    }
    let parallel = params.get("parallel").and_then(Value::as_u64).unwrap_or(1).clamp(1, 128) as usize;
    let reverse = params.get("reverse").and_then(Value::as_bool).unwrap_or(false);
    let bidir = params.get("bidirectional").and_then(Value::as_bool).unwrap_or(false);
    if bidir {
        ctx.log.log(format!("{} : le mode bidirectionnel (--bidir) n'est pas pris en charge", peer.ip()));
        refuse(&mut ctl).await;
        return Ok(());
    }
    let block = params.get("len").and_then(Value::as_u64).unwrap_or(128 * 1024).clamp(1024, 1 << 20) as usize;
    ctx.log.log(format!(
        "{} : test {} ({} flux{})",
        peer.ip(),
        if reverse { "inverse : ce poste envoie" } else { "normal : ce poste reçoit" },
        parallel,
        params.get("time").and_then(Value::as_u64).map(|t| format!(", {t} s")).unwrap_or_default()
    ));

    ctl.write_all(&[CREATE_STREAMS]).await?;
    let mut streams = Vec::with_capacity(parallel);
    for _ in 0..parallel {
        let s = tokio::time::timeout(Duration::from_secs(10), data_rx.recv())
            .await
            .map_err(|_| std::io::Error::other("connexions de données non reçues"))?
            .ok_or_else(|| std::io::Error::other("arrêt"))?;
        streams.push(s);
    }

    let stop = Arc::new(AtomicBool::new(false));
    let counters: Vec<Arc<AtomicU64>> = (0..parallel).map(|_| Arc::new(AtomicU64::new(0))).collect();
    let started = Instant::now();
    ctl.write_all(&[TEST_START]).await?;
    ctl.write_all(&[TEST_RUNNING]).await?;

    let mut workers = JoinSet::new();
    for (i, mut s) in streams.into_iter().enumerate() {
        let (stop, count) = (stop.clone(), counters[i].clone());
        workers.spawn(async move {
            if reverse {
                let buf = vec![0x5Au8; block];
                while !stop.load(Ordering::Relaxed) {
                    match s.write_all(&buf).await {
                        Ok(()) => count.fetch_add(buf.len() as u64, Ordering::Relaxed),
                        Err(_) => break,
                    };
                }
            } else {
                let mut buf = vec![0u8; block.max(64 * 1024)];
                loop {
                    match s.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => count.fetch_add(n as u64, Ordering::Relaxed),
                    };
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                }
            }
            let _ = s.shutdown().await;
        });
    }

    // the client decides when the test ends
    let end = state(&mut ctl).await;
    stop.store(true, Ordering::Relaxed);
    let elapsed = started.elapsed().as_secs_f64();
    match end {
        Ok(TEST_END) => {}
        Ok(CLIENT_TERMINATE) | Err(_) => {
            workers.abort_all();
            ctx.log.log(format!("{} : test abandonné par le client", peer.ip()));
            return Ok(());
        }
        Ok(other) => return Err(std::io::Error::other(format!("état inattendu {other}"))),
    }
    // receivers: give the last bytes in flight a moment
    let _ = tokio::time::timeout(Duration::from_millis(if reverse { 200 } else { 1000 }), async {
        while workers.join_next().await.is_some() {}
    })
    .await;
    workers.abort_all();

    ctl.write_all(&[EXCHANGE_RESULTS]).await?;
    let _client_results = read_json(&mut ctl).await?;
    let total: u64 = counters.iter().map(|c| c.load(Ordering::Relaxed)).sum();
    let results = json!({
        "cpu_util_total": 0.0,
        "cpu_util_user": 0.0,
        "cpu_util_system": 0.0,
        "sender_has_retransmits": if reverse { 0 } else { -1 },
        "congestion_used": "",
        "streams": counters.iter().enumerate().map(|(i, c)| json!({
            "id": stream_id(i),
            "bytes": c.load(Ordering::Relaxed),
            "retransmits": if reverse { 0 } else { -1 },
            "jitter": 0,
            "errors": 0,
            "omitted_errors": 0,
            "packets": 0,
            "omitted_packets": 0,
            "start_time": 0,
            "end_time": elapsed,
        })).collect::<Vec<_>>(),
    });
    write_json(&mut ctl, &results).await?;
    ctl.write_all(&[DISPLAY_RESULTS]).await?;
    if !matches!(tokio::time::timeout(Duration::from_secs(5), state(&mut ctl)).await, Ok(Ok(IPERF_DONE))) {
        ctx.log.log(format!("{} : fin de test incomplète", peer.ip()));
    }
    let mbits = if elapsed > 0.0 { total as f64 * 8.0 / elapsed / 1e6 } else { 0.0 };
    ctx.log.log(format!(
        "{} : {:.1} Mo en {:.1} s, {:.1} Mbit/s {}",
        peer.ip(),
        total as f64 / 1e6,
        elapsed,
        mbits,
        if reverse { "envoyés" } else { "reçus" }
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_ids_like_iperf3() {
        assert_eq!((0..4).map(stream_id).collect::<Vec<_>>(), [1, 3, 4, 5]);
    }
}
