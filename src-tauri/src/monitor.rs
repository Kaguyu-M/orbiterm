//! Remote system information (CPU, memory, disk, network, uptime…) read over SSH,
//! for the information bar under SSH terminals and RDP desktops.
//!
//! One sample = one command on its own SSH channel; the UI computes rates (CPU %,
//! network speed) from two consecutive samples.

use serde::Serialize;

/// POSIX `sh` script (run through `sh -c` so the login shell does not matter).
/// Every line starts with a tag; missing tools simply leave a field empty.
const SCRIPT: &str = r#"LC_ALL=C; export LC_ALL
echo "@os $(uname -sr 2>/dev/null)"
echo "@host $(hostname 2>/dev/null || uname -n)"
echo "@cpu $(head -n1 /proc/stat 2>/dev/null)"
echo "@ncpu $(grep -c ^processor /proc/cpuinfo 2>/dev/null)"
echo "@load $(cut -d" " -f1-3 /proc/loadavg 2>/dev/null)"
grep -E "^(MemTotal|MemAvailable|SwapTotal|SwapFree):" /proc/meminfo 2>/dev/null | sed "s/^/@mem /"
echo "@disk $(df -kP / 2>/dev/null | tail -n1)"
echo "@up $(cut -d" " -f1 /proc/uptime 2>/dev/null)"
echo "@users $(who 2>/dev/null | wc -l)"
awk "NR>2 {gsub(\":\",\" \"); if (\$1!=\"lo\") {rx+=\$2; tx+=\$10}} END {printf \"@net %.0f %.0f\n\", rx, tx}" /proc/net/dev 2>/dev/null
"#;

/// The command sent to the server.
pub fn command() -> String {
    format!("sh -c '{}'", SCRIPT.replace('\'', r"'\''"))
}

#[derive(Serialize, Default, Debug, Clone, PartialEq)]
pub struct RemoteStats {
    /// false when the server is not a Linux-like system (no /proc)
    pub supported: bool,
    pub os: String,
    pub host: String,
    /// cumulated CPU jiffies (all states) and idle + iowait
    pub cpu_total: u64,
    pub cpu_idle: u64,
    pub cpus: u32,
    pub load: String,
    /// kB
    pub mem_total: u64,
    pub mem_avail: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    /// kB, root file system
    pub disk_total: u64,
    pub disk_used: u64,
    /// cumulated bytes, all interfaces but loopback
    pub net_rx: u64,
    pub net_tx: u64,
    /// seconds
    pub uptime: f64,
    pub users: u32,
}

pub fn parse(out: &str) -> RemoteStats {
    let mut s = RemoteStats::default();
    for line in out.lines() {
        let Some((tag, rest)) = line.split_once(' ') else { continue };
        let rest = rest.trim();
        let nums = || rest.split_whitespace().map(|x| x.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
        match tag {
            "@os" => s.os = rest.to_string(),
            "@host" => s.host = rest.to_string(),
            "@cpu" => {
                // "cpu user nice system idle iowait irq softirq steal …"
                let v: Vec<u64> = rest.split_whitespace().skip(1).map(|x| x.parse().unwrap_or(0)).collect();
                if v.len() >= 4 {
                    s.cpu_total = v.iter().take(8).sum();
                    s.cpu_idle = v[3] + v.get(4).copied().unwrap_or(0);
                    s.supported = true;
                }
            }
            "@ncpu" => s.cpus = rest.parse().unwrap_or(0),
            "@load" => s.load = rest.to_string(),
            "@mem" => {
                let mut it = rest.split_whitespace();
                let (k, v) = (it.next().unwrap_or(""), it.next().and_then(|x| x.parse().ok()).unwrap_or(0));
                match k {
                    "MemTotal:" => s.mem_total = v,
                    "MemAvailable:" => s.mem_avail = v,
                    "SwapTotal:" => s.swap_total = v,
                    "SwapFree:" => s.swap_free = v,
                    _ => {}
                }
            }
            "@disk" => {
                // "Filesystem 1024-blocks Used Available Capacity Mounted"
                let f: Vec<&str> = rest.split_whitespace().collect();
                if f.len() >= 4 {
                    s.disk_total = f[1].parse().unwrap_or(0);
                    s.disk_used = f[2].parse().unwrap_or(0);
                }
            }
            "@up" => s.uptime = rest.parse().unwrap_or(0.0),
            "@users" => s.users = rest.parse().unwrap_or(0),
            "@net" => {
                let v = nums();
                if v.len() >= 2 {
                    (s.net_rx, s.net_tx) = (v[0], v[1]);
                }
            }
            _ => {}
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_linux_sample() {
        let out = "@os Linux 6.12.0\n@host debian\n@cpu cpu  100 5 50 1000 20 0 3 0 0 0\n@ncpu 4\n@load 0.10 0.20 0.30\n\
                   @mem MemTotal:        4000000 kB\n@mem MemAvailable:    3000000 kB\n@mem SwapTotal: 0 kB\n@mem SwapFree: 0 kB\n\
                   @disk /dev/sda1 20000000 5000000 15000000 25% /\n@up 3600.52\n@users 2\n@net 123456 7890\n";
        let s = parse(out);
        assert!(s.supported);
        assert_eq!(s.cpu_total, 1178);
        assert_eq!(s.cpu_idle, 1020);
        assert_eq!((s.cpus, s.mem_total, s.mem_avail), (4, 4_000_000, 3_000_000));
        assert_eq!((s.disk_total, s.disk_used), (20_000_000, 5_000_000));
        assert_eq!((s.net_rx, s.net_tx, s.users), (123_456, 7890, 2));
        assert_eq!(s.host, "debian");
        assert!(command().starts_with("sh -c '"));
    }

    #[test]
    fn non_linux_is_unsupported() {
        let s = parse("'sh' n'est pas reconnu en tant que commande interne\n");
        assert!(!s.supported);
    }
}
