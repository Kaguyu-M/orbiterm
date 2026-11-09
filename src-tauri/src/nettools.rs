//! Network tools: TCP port scanner, list of this computer's sockets, Wake-on-LAN.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use futures_util::StreamExt;
use serde::Serialize;

use crate::error::{AppError, AppResult};

// ------------------------------------------------------------------ target / port parsing

const MAX_HOSTS: usize = 4096;
const MAX_PROBES: usize = 1_000_000;

/// Expands "192.168.1.0/24, 10.0.0.5-20, 10.0.1.1-10.0.1.9, serveur.local" into addresses.
pub async fn parse_targets(spec: &str) -> AppResult<Vec<(String, IpAddr)>> {
    let mut out: Vec<(String, IpAddr)> = vec![];
    for item in spec.split([',', ' ', ';', '\n']).map(str::trim).filter(|s| !s.is_empty()) {
        if let Some((net, bits)) = item.split_once('/') {
            let ip: Ipv4Addr = net.parse().map_err(|_| bad(item))?;
            let bits: u32 = bits.parse().map_err(|_| bad(item))?;
            if !(16..=32).contains(&bits) {
                return Err(AppError::Config(format!("{item} : masque /16 à /32 seulement")));
            }
            let mask = if bits == 32 { u32::MAX } else { !(u32::MAX >> bits) };
            let base = u32::from(ip) & mask;
            let count = 1u64 << (32 - bits);
            for i in 0..count as u32 {
                // skip network and broadcast addresses of real subnets
                if count > 2 && (i == 0 || i as u64 == count - 1) {
                    continue;
                }
                let a = IpAddr::V4(Ipv4Addr::from(base + i));
                out.push((a.to_string(), a));
            }
        } else if let Some((a, b)) = item.split_once('-') {
            let start: Ipv4Addr = a.trim().parse().map_err(|_| bad(item))?;
            let end: Ipv4Addr = match b.trim().parse::<Ipv4Addr>() {
                Ok(e) => e,
                Err(_) => {
                    let last: u8 = b.trim().parse().map_err(|_| bad(item))?;
                    let o = start.octets();
                    Ipv4Addr::new(o[0], o[1], o[2], last)
                }
            };
            let (s, e) = (u32::from(start), u32::from(end));
            if e < s {
                return Err(bad(item));
            }
            if (e - s) as usize >= MAX_HOSTS {
                return Err(AppError::Config(format!("{item} : plage trop grande")));
            }
            for v in s..=e {
                let a = IpAddr::V4(Ipv4Addr::from(v));
                out.push((a.to_string(), a));
            }
        } else if let Ok(ip) = item.parse::<IpAddr>() {
            out.push((ip.to_string(), ip));
        } else {
            let addrs = tokio::net::lookup_host((item, 0))
                .await
                .map_err(|_| AppError::Config(format!("nom inconnu : {item}")))?;
            let ip = addrs
                .map(|a| a.ip())
                .min_by_key(|ip| ip.is_ipv6())
                .ok_or_else(|| AppError::Config(format!("nom inconnu : {item}")))?;
            out.push((item.to_string(), ip));
        }
        if out.len() > MAX_HOSTS {
            return Err(AppError::Config(format!("trop d'adresses (maximum {MAX_HOSTS})")));
        }
    }
    if out.is_empty() {
        return Err(AppError::Config("aucune adresse à analyser".into()));
    }
    Ok(out)
}

fn bad(item: &str) -> AppError {
    AppError::Config(format!("adresse ou plage invalide : {item}"))
}

pub const COMMON_PORTS: &[u16] = &[
    21, 22, 23, 25, 53, 80, 110, 111, 135, 139, 143, 389, 443, 445, 465, 587, 631, 636, 873, 993, 995, 1433, 1521, 1723,
    1883, 2049, 2375, 3000, 3128, 3306, 3389, 5000, 5432, 5900, 5901, 5985, 5986, 6379, 6443, 8000, 8006, 8080, 8443,
    8888, 9000, 9090, 9100, 9200, 11211, 27017,
];

/// "22,80,8000-8100" or "communs" (well-known service ports).
pub fn parse_ports(spec: &str) -> AppResult<Vec<u16>> {
    let mut out: Vec<u16> = vec![];
    for item in spec.split([',', ' ', ';']).map(str::trim).filter(|s| !s.is_empty()) {
        let lower = item.to_lowercase();
        if lower == "communs" || lower == "common" {
            out.extend_from_slice(COMMON_PORTS);
        } else if let Some((a, b)) = item.split_once('-') {
            let a: u16 = a.trim().parse().map_err(|_| badp(item))?;
            let b: u16 = b.trim().parse().map_err(|_| badp(item))?;
            if a == 0 || b < a {
                return Err(badp(item));
            }
            out.extend(a..=b);
        } else {
            let p: u16 = item.parse().map_err(|_| badp(item))?;
            if p == 0 {
                return Err(badp(item));
            }
            out.push(p);
        }
    }
    out.sort_unstable();
    out.dedup();
    if out.is_empty() {
        return Err(AppError::Config("aucun port à analyser".into()));
    }
    Ok(out)
}

fn badp(item: &str) -> AppError {
    AppError::Config(format!("port ou plage invalide : {item}"))
}

pub fn service_name(port: u16) -> &'static str {
    match port {
        20 | 21 => "FTP",
        22 => "SSH",
        23 => "Telnet",
        25 | 465 | 587 => "SMTP",
        53 => "DNS",
        80 | 8000 | 8080 | 8888 => "HTTP",
        110 | 995 => "POP3",
        111 => "RPC",
        135 => "MS-RPC",
        139 | 445 => "SMB",
        143 | 993 => "IMAP",
        389 | 636 => "LDAP",
        443 | 8443 => "HTTPS",
        631 => "IPP",
        873 => "rsync",
        1433 => "SQL Server",
        1521 => "Oracle",
        1723 => "PPTP",
        1883 => "MQTT",
        2049 => "NFS",
        2375 => "Docker",
        3000 => "HTTP (dev)",
        3128 => "Proxy",
        3306 => "MySQL",
        3389 => "RDP",
        5000 => "UPnP/HTTP",
        5432 => "PostgreSQL",
        5900..=5909 => "VNC",
        5985 | 5986 => "WinRM",
        6379 => "Redis",
        6443 => "Kubernetes",
        8006 => "Proxmox",
        9000 => "HTTP",
        9090 => "Prometheus",
        9100 => "Imprimante",
        9200 => "Elasticsearch",
        11211 => "Memcached",
        27017 => "MongoDB",
        _ => "",
    }
}

// ------------------------------------------------------------------ scanner

#[derive(Debug, Clone, Serialize)]
pub struct ScanHit {
    pub host: String,
    pub ip: String,
    pub port: u16,
    pub service: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ScanState {
    pub total: usize,
    pub done: usize,
    pub hits: Vec<ScanHit>,
    pub finished: bool,
    pub cancelled: bool,
    pub hosts: usize,
}

struct Scan {
    state: Arc<Mutex<ScanState>>,
    task: tokio::task::JoinHandle<()>,
}

fn scans() -> &'static Mutex<HashMap<String, Scan>> {
    static S: OnceLock<Mutex<HashMap<String, Scan>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Starts a scan in the background; poll it with [`scan_state`].
pub async fn scan_start(id: String, targets: &str, ports: &str, timeout_ms: u64, concurrency: usize) -> AppResult<usize> {
    let hosts = parse_targets(targets).await?;
    let ports = parse_ports(ports)?;
    let total = hosts.len() * ports.len();
    if total > MAX_PROBES {
        return Err(AppError::Config(format!("{total} tests demandés : réduis la plage (maximum {MAX_PROBES})")));
    }
    scan_cancel(&id);
    let state = Arc::new(Mutex::new(ScanState { total, hosts: hosts.len(), ..Default::default() }));
    let st = state.clone();
    let timeout = Duration::from_millis(timeout_ms.clamp(50, 10_000));
    // host-major order: results of one machine arrive together
    let probes: Vec<(String, IpAddr, u16)> =
        hosts.into_iter().flat_map(|(name, ip)| ports.iter().map(move |p| (name.clone(), ip, *p))).collect();
    let task = tokio::spawn(async move {
        let mut stream = futures_util::stream::iter(probes)
            .map(move |(name, ip, port)| probe(name, ip, port, timeout))
            .buffer_unordered(concurrency.clamp(1, 1024));
        while let Some((host, ip, port, open)) = stream.next().await {
            let mut s = st.lock().unwrap();
            s.done += 1;
            if open {
                s.hits.push(ScanHit { host, ip: ip.to_string(), port, service: service_name(port).into() });
            }
        }
        let mut s = st.lock().unwrap();
        s.hits.sort_by(|a, b| a.ip.parse::<IpAddr>().ok().cmp(&b.ip.parse().ok()).then(a.port.cmp(&b.port)));
        s.finished = true;
    });
    scans().lock().unwrap().insert(id, Scan { state, task });
    Ok(total)
}

async fn probe(name: String, ip: IpAddr, port: u16, timeout: Duration) -> (String, IpAddr, u16, bool) {
    let open =
        matches!(tokio::time::timeout(timeout, tokio::net::TcpStream::connect(SocketAddr::new(ip, port))).await, Ok(Ok(_)));
    (name, ip, port, open)
}

pub fn scan_state(id: &str) -> Option<ScanState> {
    scans().lock().unwrap().get(id).map(|s| s.state.lock().unwrap().clone())
}

pub fn scan_cancel(id: &str) {
    if let Some(s) = scans().lock().unwrap().remove(id) {
        s.task.abort();
    }
}

// ------------------------------------------------------------------ Wake-on-LAN

pub fn parse_mac(mac: &str) -> AppResult<[u8; 6]> {
    let hex: String = mac.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let sep_ok = mac.chars().all(|c| c.is_ascii_hexdigit() || ":-. ".contains(c));
    if hex.len() != 12 || !sep_ok {
        return Err(AppError::Config(format!("adresse MAC invalide : {mac}")));
    }
    let mut out = [0u8; 6];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap_or(0);
    }
    Ok(out)
}

pub fn magic_packet(mac: [u8; 6]) -> Vec<u8> {
    let mut p = vec![0xFFu8; 6];
    for _ in 0..16 {
        p.extend_from_slice(&mac);
    }
    p
}

/// Sends the magic packet (3 times, as many tools do) to `broadcast:port`.
pub async fn wol_send(mac: &str, broadcast: &str, port: u16) -> AppResult<()> {
    let mac = parse_mac(mac)?;
    let target = if broadcast.trim().is_empty() { "255.255.255.255" } else { broadcast.trim() };
    let ip: IpAddr = target.parse().map_err(|_| AppError::Config(format!("adresse de diffusion invalide : {target}")))?;
    let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    sock.set_broadcast(true)?;
    let pkt = magic_packet(mac);
    for _ in 0..3 {
        sock.send_to(&pkt, SocketAddr::new(ip, if port == 0 { 9 } else { port })).await?;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}

// ------------------------------------------------------------------ local sockets

#[derive(Debug, Clone, Serialize)]
pub struct SocketInfo {
    pub proto: String,
    pub local: String,
    pub local_port: u16,
    pub remote: String,
    pub state: String,
    pub pid: u32,
    pub process: String,
}

/// TCP/UDP sockets of this computer with their owning process (netstat / ss).
pub async fn open_ports() -> AppResult<Vec<SocketInfo>> {
    tokio::task::spawn_blocking(open_ports_sync).await.map_err(|e| AppError::Conn(e.to_string()))?
}

fn run(program: &str, args: &[&str]) -> AppResult<String> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().map_err(|e| AppError::Conn(format!("{program} : {e}")))?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn split_addr(a: &str) -> (String, u16) {
    match a.rsplit_once(':') {
        Some((h, p)) => (h.trim_matches(['[', ']']).to_string(), p.parse().unwrap_or(0)),
        None => (a.to_string(), 0),
    }
}

#[cfg(windows)]
fn open_ports_sync() -> AppResult<Vec<SocketInfo>> {
    let names: HashMap<u32, String> = run("tasklist", &["/fo", "csv", "/nh"])?
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split("\",\"").map(|s| s.trim_matches('"')).collect();
            Some((f.get(1)?.parse().ok()?, f.first()?.to_string()))
        })
        .collect();
    Ok(parse_netstat(&run("netstat", &["-ano"])?, &names))
}

#[cfg(not(windows))]
fn open_ports_sync() -> AppResult<Vec<SocketInfo>> {
    let text = run("ss", &["-tuanpH"])?;
    let mut out = vec![];
    for l in text.lines() {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.len() < 6 {
            continue;
        }
        let (local, local_port) = split_addr(f[4]);
        let users = f.get(6).copied().unwrap_or("");
        let process = users.split('"').nth(1).unwrap_or("").to_string();
        let pid = users.split("pid=").nth(1).and_then(|s| s.split([',', ')']).next()).and_then(|s| s.parse().ok()).unwrap_or(0);
        out.push(SocketInfo {
            proto: f[0].to_uppercase(),
            local,
            local_port,
            remote: if f[5].starts_with('*') || f[5].ends_with(":*") { String::new() } else { f[5].into() },
            state: match f[1] {
                "LISTEN" | "UNCONN" => "ÉCOUTE".into(),
                "ESTAB" => "ÉTABLIE".into(),
                s => s.into(),
            },
            pid,
            process,
        });
    }
    Ok(out)
}

/// Parses `netstat -ano` (Windows; column headers are localized, rows are not).
pub fn parse_netstat(text: &str, names: &HashMap<u32, String>) -> Vec<SocketInfo> {
    let mut out = vec![];
    for l in text.lines() {
        let f: Vec<&str> = l.split_whitespace().collect();
        let (proto, local, remote, state, pid) = match f.as_slice() {
            ["TCP", l, r, s, p] => ("TCP", *l, *r, *s, *p),
            ["UDP", l, r, p] => ("UDP", *l, *r, "", *p),
            _ => continue,
        };
        let (lh, lp) = split_addr(local);
        let pid: u32 = pid.parse().unwrap_or(0);
        out.push(SocketInfo {
            proto: if lh.contains(':') { format!("{proto}v6") } else { proto.into() },
            local: lh,
            local_port: lp,
            remote: if remote.starts_with('*') || remote.ends_with(":0") { String::new() } else { remote.into() },
            state: match state {
                "LISTENING" => "ÉCOUTE".into(),
                "ESTABLISHED" => "ÉTABLIE".into(),
                "" => "ÉCOUTE".into(),
                s => s.into(),
            },
            pid,
            process: names.get(&pid).cloned().unwrap_or_else(|| if pid == 4 { "System".into() } else { String::new() }),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn targets() {
        let t = parse_targets("192.168.1.0/30, 10.0.0.5-7 127.0.0.1").await.unwrap();
        let ips: Vec<String> = t.iter().map(|(_, i)| i.to_string()).collect();
        assert_eq!(ips, ["192.168.1.1", "192.168.1.2", "10.0.0.5", "10.0.0.6", "10.0.0.7", "127.0.0.1"]);
        assert!(parse_targets("10.0.0.0/8").await.is_err());
        assert!(parse_targets("10.0.0.9-3").await.is_err());
        assert_eq!(parse_targets("10.0.0.1-10.0.0.3").await.unwrap().len(), 3);
    }

    #[test]
    fn ports_mac_netstat() {
        assert_eq!(parse_ports("80, 22,20-23").unwrap(), vec![20, 21, 22, 23, 80]);
        assert!(parse_ports("communs").unwrap().contains(&3389));
        assert!(parse_ports("0").is_err() && parse_ports("x").is_err());
        assert_eq!(parse_mac("aa:BB-cc.dd ee:ff").unwrap(), [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        assert!(parse_mac("aa:bb:cc").is_err());
        let p = magic_packet([1, 2, 3, 4, 5, 6]);
        assert_eq!(p.len(), 102);
        assert_eq!(&p[..6], &[0xff; 6]);
        assert_eq!(&p[96..], &[1, 2, 3, 4, 5, 6]);
        let names = HashMap::from([(1772, "svchost.exe".to_string())]);
        let s = parse_netstat(
            "  Proto  Adresse locale  Adresse distante  État\n  TCP    0.0.0.0:135   0.0.0.0:0   LISTENING   1772\n  UDP    [::]:500   *:*   4\n  TCP  192.168.0.2:50000  1.2.3.4:443  ESTABLISHED  99\n",
            &names,
        );
        assert_eq!(s.len(), 3);
        assert_eq!((s[0].local_port, s[0].state.as_str(), s[0].process.as_str()), (135, "ÉCOUTE", "svchost.exe"));
        assert_eq!((s[1].proto.as_str(), s[1].local.as_str(), s[1].process.as_str()), ("UDPv6", "::", "System"));
        assert_eq!((s[2].remote.as_str(), s[2].state.as_str()), ("1.2.3.4:443", "ÉTABLIE"));
    }
}
