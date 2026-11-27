//! System summary written when a shell opens, in the manner of fastfetch: the logo of the
//! distribution on the left, the main facts about the machine on the right. The facts are read
//! by Orbiterm itself (one short command), nothing has to be installed on the server.
//!
//! Unix-like systems (servers, WSL, this computer under Linux) run a POSIX `sh` script; this
//! computer under Windows is read directly. Windows SSH servers get no summary: their console
//! (ConPTY) clears the screen when the shell starts.

use std::fmt::Write as _;

/// What the summary shows. Sizes in kB; empty / 0 = unknown (the line is left out).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Facts {
    /// distribution id (os-release ID: "ubuntu", "debian"…, or "macos", "freebsd", "windows")
    pub id: String,
    /// os-release ID_LIKE ("ubuntu debian"…)
    pub id_like: String,
    pub name: String,
    pub user: String,
    pub host: String,
    pub kernel: String,
    pub arch: String,
    /// seconds
    pub uptime: u64,
    pub shell: String,
    pub cpu: String,
    pub cpus: u32,
    pub mem_total: u64,
    pub mem_used: u64,
    pub disk_total: u64,
    pub disk_used: u64,
    pub ip: String,
    pub packages: String,
}

// ---------------------------------------------------------------- collecting

/// POSIX `sh` script: one tagged line per fact, later lines win (OS-specific ones come last).
const UNIX_SCRIPT: &str = r#"LC_ALL=C; export LC_ALL
[ -r /etc/os-release ] && . /etc/os-release
echo "@id ${ID:-$(uname -s)}"
echo "@like ${ID_LIKE:-}"
echo "@name ${PRETTY_NAME:-$(uname -s)}"
echo "@user ${USER:-$(id -un 2>/dev/null)}"
echo "@host $(hostname 2>/dev/null || uname -n)"
echo "@kernel $(uname -sr 2>/dev/null)"
echo "@arch $(uname -m 2>/dev/null)"
echo "@shell ${SHELL##*/}"
[ -r /proc/uptime ] && echo "@up $(cut -d" " -f1 /proc/uptime)"
[ -r /proc/cpuinfo ] && echo "@cpu $(grep -m1 -E "^(model name|Hardware|Model)" /proc/cpuinfo | cut -d: -f2-)"
[ -r /proc/cpuinfo ] && echo "@ncpu $(grep -c ^processor /proc/cpuinfo)"
[ -r /proc/meminfo ] && grep -E "^(MemTotal|MemAvailable):" /proc/meminfo | sed "s/^/@mem /"
echo "@disk $(df -kP / 2>/dev/null | tail -n1)"
ip=$(hostname -I 2>/dev/null | cut -d" " -f1)
[ -z "$ip" ] && command -v ip >/dev/null && ip=$(ip -4 -o addr show scope global 2>/dev/null | awk "NR==1{split(\$4,a,\"/\");print a[1]}")
echo "@ip $ip"
p=""
command -v dpkg-query >/dev/null && p="$p $(dpkg-query -f "." -W 2>/dev/null | wc -c) (dpkg)"
command -v rpm >/dev/null && [ ! -x /usr/bin/dpkg-query ] && p="$p $(rpm -qa 2>/dev/null | wc -l) (rpm)"
command -v pacman >/dev/null && p="$p $(pacman -Qq 2>/dev/null | wc -l) (pacman)"
command -v apk >/dev/null && p="$p $(apk info 2>/dev/null | wc -l) (apk)"
command -v flatpak >/dev/null && p="$p $(flatpak list 2>/dev/null | wc -l) (flatpak)"
command -v snap >/dev/null && p="$p $(snap list 2>/dev/null | tail -n +2 | wc -l) (snap)"
echo "@pkgs $p"
case "$(uname -s)" in
Darwin)
  echo "@id macos"
  echo "@name macOS $(sw_vers -productVersion 2>/dev/null)"
  echo "@cpu $(sysctl -n machdep.cpu.brand_string 2>/dev/null)"
  echo "@ncpu $(sysctl -n hw.ncpu 2>/dev/null)"
  echo "@memb $(sysctl -n hw.memsize 2>/dev/null)"
  echo "@boot $(sysctl -n kern.boottime 2>/dev/null | sed "s/.*sec = \([0-9]*\).*/\1/") $(date +%s)"
  echo "@ip $(ipconfig getifaddr en0 2>/dev/null)"
  command -v brew >/dev/null && echo "@pkgs $(brew list 2>/dev/null | wc -l) (brew)";;
FreeBSD|OpenBSD|NetBSD)
  echo "@cpu $(sysctl -n hw.model 2>/dev/null)"
  echo "@ncpu $(sysctl -n hw.ncpu 2>/dev/null)"
  echo "@memb $(sysctl -n hw.physmem 2>/dev/null)"
  echo "@boot $(sysctl -n kern.boottime 2>/dev/null | sed "s/.*sec = \([0-9]*\).*/\1/") $(date +%s)";;
esac
"#;

/// The command sent to a Unix-like server.
pub fn unix_command() -> String {
    format!("sh -c '{}'", UNIX_SCRIPT.replace('\'', r"'\''"))
}

fn num(s: &str) -> u64 {
    s.trim().split(['.', ' ']).next().and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// Reads the output of the script. None when it is not one of ours (no "@id" line).
pub fn parse(out: &str) -> Option<Facts> {
    let mut f = Facts::default();
    let mut avail = None;
    let mut seen = false;
    for line in out.lines() {
        let line = line.trim_end_matches('\r');
        let Some((tag, rest)) = line.split_once(' ').or_else(|| line.starts_with('@').then_some((line, ""))) else {
            continue;
        };
        let rest = rest.trim();
        match tag {
            "@id" => {
                seen = true;
                f.id = rest.to_ascii_lowercase();
            }
            "@like" => f.id_like = rest.to_ascii_lowercase(),
            "@name" => f.name = rest.trim_matches('"').to_string(),
            "@user" => f.user = rest.into(),
            "@host" => f.host = rest.into(),
            "@kernel" => f.kernel = rest.into(),
            "@arch" => f.arch = rest.into(),
            "@shell" => f.shell = rest.into(),
            "@up" => f.uptime = num(rest),
            "@cpu" if !rest.is_empty() => f.cpu = rest.split_whitespace().collect::<Vec<_>>().join(" "),
            "@ncpu" => f.cpus = num(rest) as u32,
            "@mem" => {
                let mut w = rest.split_whitespace();
                match (w.next(), w.next().map(num)) {
                    (Some("MemTotal:"), Some(v)) => f.mem_total = v,
                    (Some("MemAvailable:"), Some(v)) => avail = Some(v),
                    _ => {}
                }
            }
            "@memb" if num(rest) > 0 => f.mem_total = num(rest) / 1024,
            "@memk" => {
                let w: Vec<u64> = rest.split_whitespace().map(num).collect();
                if let [total, free] = w[..] {
                    f.mem_total = total;
                    avail = Some(free);
                }
            }
            "@disk" => {
                // filesystem 1024-blocks used available capacity mount
                let w: Vec<&str> = rest.split_whitespace().collect();
                if w.len() >= 4 {
                    f.disk_total = num(w[1]);
                    f.disk_used = num(w[2]);
                }
            }
            "@diskb" => {
                let w: Vec<u64> = rest.split_whitespace().map(num).collect();
                if let [size, free] = w[..] {
                    f.disk_total = size / 1024;
                    f.disk_used = size.saturating_sub(free) / 1024;
                }
            }
            "@boot" => {
                let w: Vec<u64> = rest.split_whitespace().map(num).collect();
                if let [boot, now] = w[..]
                    && boot > 0
                {
                    f.uptime = now.saturating_sub(boot);
                }
            }
            "@ip" if !rest.is_empty() => f.ip = rest.into(),
            "@pkgs" if !rest.is_empty() => f.packages = rest.split(") ").collect::<Vec<_>>().join("), "),
            _ => {}
        }
    }
    if let Some(a) = avail {
        f.mem_used = f.mem_total.saturating_sub(a);
    }
    if f.name.is_empty() {
        f.name = f.id.clone();
    }
    seen.then_some(f)
}

/// Facts about this computer, for local shells (Windows: read from the system).
#[cfg(windows)]
pub fn local_windows(shell: &str) -> Facts {
    use windows_sys::Win32::System::SystemInformation::{GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let cv = hklm.open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion").ok();
    let get = |k: &str| cv.as_ref().and_then(|c| c.get_value::<String, _>(k).ok()).unwrap_or_default();
    let build: u32 = get("CurrentBuildNumber").parse().unwrap_or(0);
    // the registry still says "Windows 10" on Windows 11
    let mut product = get("ProductName");
    if build >= 22000 {
        product = product.replace("Windows 10", "Windows 11");
    }
    let ubr = cv.as_ref().and_then(|c| c.get_value::<u32, _>("UBR").ok()).unwrap_or(0);
    let cpu = hklm
        .open_subkey(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0")
        .and_then(|k| k.get_value::<String, _>("ProcessorNameString"))
        .unwrap_or_default();
    // SAFETY: plain Win32 calls on local structures
    let (mem_total, mem_avail, uptime) = unsafe {
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        let ok = GlobalMemoryStatusEx(&mut m) != 0;
        let (t, a) = if ok { (m.ullTotalPhys / 1024, m.ullAvailPhys / 1024) } else { (0, 0) };
        (t, a, GetTickCount64() / 1000)
    };
    let (disk_total, disk_used) = system_drive_space();
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    Facts {
        id: "windows".into(),
        name: format!("{product} {}", get("DisplayVersion")).trim().to_string(),
        user: env("USERNAME"),
        host: env("COMPUTERNAME"),
        kernel: format!("{}.{build}.{ubr}", if build >= 10240 { "10.0" } else { "6.x" }),
        arch: env("PROCESSOR_ARCHITECTURE"),
        uptime,
        shell: shell.into(),
        cpu: cpu.split_whitespace().collect::<Vec<_>>().join(" "),
        cpus: env("NUMBER_OF_PROCESSORS").parse().unwrap_or(0),
        mem_total,
        mem_used: mem_total.saturating_sub(mem_avail),
        disk_total,
        disk_used,
        ip: local_ip(),
        ..Default::default()
    }
}

#[cfg(windows)]
fn system_drive_space() -> (u64, u64) {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()) + "\\";
    let wide: Vec<u16> = drive.encode_utf16().chain(Some(0)).collect();
    let (mut free, mut total, mut total_free) = (0u64, 0u64, 0u64);
    // SAFETY: the path is NUL-terminated, the outputs are local
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, &mut total_free) } != 0;
    if ok { (total / 1024, total.saturating_sub(total_free) / 1024) } else { (0, 0) }
}

/// Address of the interface that leads outside (no packet is sent).
#[cfg(windows)]
fn local_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("192.0.2.1:9").and_then(|_| s.local_addr()))
        .map(|a| a.ip().to_string())
        .unwrap_or_default()
}

/// Runs the Unix script with a local program (sh, or wsl.exe for a WSL distribution).
pub fn run_local(program: &str, args: &[&str]) -> Option<Facts> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().ok()?;
    parse(&String::from_utf8_lossy(&out.stdout))
}

/// The Unix script itself (for `sh -c <script>` run locally).
pub fn unix_script() -> &'static str {
    UNIX_SCRIPT
}

// ---------------------------------------------------------------- logos

/// A logo: lines drawn with `$1` / `$2` colour switches, and its two colours (ANSI 256).
struct Logo {
    lines: &'static [&'static str],
    colors: (u8, u8),
}

static TUX: Logo = Logo {
    lines: &[
        "$1     .--.",
        "$1    |$2o$1_$2o$1 |",
        "$1    |$2:_/$1 |",
        "$1   //   \\ \\",
        "$1  (|     | )",
        "$1 /'\\_   _/`\\",
        "$2 \\___)$1=$2(___/",
    ],
    colors: (15, 220),
};

static UBUNTU: Logo = Logo {
    lines: &[
        "$1          ..",
        "$1     .--.(  )",
        "$1   .'    `''",
        "$1  (  ..      $2.",
        "$1  ( (  )    $2( )",
        "$1   `.`'      $2'",
        "$1     '--.(  )",
        "$1          ''",
    ],
    colors: (208, 15),
};

static DEBIAN: Logo = Logo {
    lines: &[
        "$1     _____",
        "$1    /  __ \\",
        "$1   |  /    |",
        "$1   |  \\___-",
        "$1   -_",
        "$1     --_",
    ],
    colors: (161, 15),
};

static FEDORA: Logo = Logo {
    lines: &[
        "$1      _____",
        "$1     /   __)$2\\",
        "$1     |  /  $2\\ \\",
        "$1  ___|  |__$2/ /",
        "$1 / $2(_    _)$1_/",
        "$1/ /  $2|  |",
        "$1\\ \\$2__/  |",
        "$1 \\$2(_____/",
    ],
    colors: (33, 15),
};

static ARCH: Logo = Logo {
    lines: &[
        "$1       /\\",
        "$1      /  \\",
        "$1     /\\   \\",
        "$1    /      \\",
        "$1   /   ,,   \\",
        "$1  /   |  |  -\\",
        "$1 /_-''    ''-_\\",
    ],
    colors: (38, 38),
};

static MANJARO: Logo = Logo {
    lines: &[
        "$1 ||||||||| ||||",
        "$1 ||||||||| ||||",
        "$1 ||||      ||||",
        "$1 |||| |||| ||||",
        "$1 |||| |||| ||||",
        "$1 |||| |||| ||||",
        "$1 |||| |||| ||||",
    ],
    colors: (41, 41),
};

static KALI: Logo = Logo {
    lines: &[
        "$1   _____",
        "$1  /  ___\\___",
        "$1 /  /       \\__",
        "$1 \\  \\_____     \\",
        "$1  \\_____  \\_   |",
        "$1        \\   \\  |",
        "$1         \\___\\/",
    ],
    colors: (33, 15),
};

static ALPINE: Logo = Logo {
    lines: &[
        "$1    /\\ /\\",
        "$1   /  \\  \\",
        "$1  /    \\  \\",
        "$1 /      \\  \\",
        "$1/        \\  \\",
        "$1          \\  \\",
    ],
    colors: (25, 15),
};

static REDHAT: Logo = Logo {
    lines: &[
        "$1      .M.:MMM",
        "$1     MMMMMMMMMM.",
        "$1    ,MMMMMMMMMMM",
        "$1 .MM MMMMMMMMMMM",
        "$1MMMM   MMMMMMMMM",
        "$1MMMMMM        MM",
        "$1 MMMMMMMMM,MMMM",
        "$1   MMMMMMMMMMMM.",
    ],
    colors: (160, 15),
};

static ROCKY: Logo = Logo {
    lines: &[
        "$1    __wgliliiligw_,",
        "$1  _williiiiiiliilililw,",
        "$1 qilililiiilililliiliil",
        "$1jiiiiiiiiil^   ~~liilil",
        "$1liliiiiii^       \\iliil",
        "$1 qiliil^    _gw   ~liil",
        "$1  ~qil^ ,gilliliw, ~li",
    ],
    colors: (35, 35),
};

static SUSE: Logo = Logo {
    lines: &[
        "$1  _______",
        "$1 /  ___  \\__",
        "$1|  /   \\  $2(o)$1",
        "$1| |  _  |  /",
        "$1|  \\___/  /",
        "$1 \\_______/",
    ],
    colors: (112, 15),
};

static MINT: Logo = Logo {
    lines: &[
        "$1 ___________",
        "$1|_          \\",
        "$1  | $2| _____ $1|",
        "$1  | $2| | | | $1|",
        "$1  | $2| | | | $1|",
        "$1  | $2\\_____/ $1|",
        "$1  \\_________/",
    ],
    colors: (113, 15),
};

static RASPBIAN: Logo = Logo {
    lines: &[
        "$1   .~~.   .~~.",
        "$1  '. \\ ' ' / .'",
        "$2   .~ .~~~..~.",
        "$2  : .~.'~'.~. :",
        "$2 ~ (   ) (   ) ~",
        "$2( : '~'.~.'~' : )",
        "$2 ~ .~       ~. ~",
        "$2  '~'  ~~~  '~'",
    ],
    colors: (113, 161),
};

static GENTOO: Logo = Logo {
    lines: &[
        "$1   _-----_",
        "$1  (       \\",
        "$1  \\    0   \\",
        "$1   \\        )",
        "$1   /      _/",
        "$1  (     _-",
        "$1  \\____-",
    ],
    colors: (140, 15),
};

static NIXOS: Logo = Logo {
    lines: &[
        "$1  \\\\  \\\\ //",
        "$1 ==\\\\__\\\\/ //",
        "$1   //   \\\\//",
        "$1==//     //==",
        "$1 //\\\\___//",
        "$1// /\\\\  \\\\==",
        "$1  // \\\\  \\\\",
    ],
    colors: (74, 110),
};

static POP: Logo = Logo {
    lines: &[
        "$1______",
        "$1\\   _ \\        __",
        "$1 \\ \\ \\ \\      / /",
        "$1  \\ \\_\\ \\    / /",
        "$1   \\  ___\\  /_/",
        "$1    \\ \\    _",
        "$1   __\\_\\__(_)_",
        "$1  (___________)",
    ],
    colors: (37, 15),
};

static MACOS: Logo = Logo {
    lines: &[
        "$1        .:'",
        "$1    __ :'__",
        "$1 .'`  `-'  ``.",
        "$2:          .-'",
        "$2:         :",
        "$1 :         `-;",
        "$1  `.__.-.__.'",
    ],
    colors: (113, 214),
};

static FREEBSD: Logo = Logo {
    lines: &[
        "$1/\\,-'''''-,/\\",
        "$1\\_)       (_/",
        "$1|           |",
        "$1|           |",
        "$1 ;         ;",
        "$1  '-_____-'",
    ],
    colors: (160, 160),
};

static WINDOWS: Logo = Logo {
    lines: &[
        "$1 ████████ $2████████",
        "$1 ████████ $2████████",
        "$1 ████████ $2████████",
        "$1 ████████ $2████████",
        "",
        "$1 ████████ $2████████",
        "$1 ████████ $2████████",
        "$1 ████████ $2████████",
        "$1 ████████ $2████████",
    ],
    colors: (39, 39),
};

fn logo_for(f: &Facts) -> &'static Logo {
    let pick = |id: &str| -> Option<&'static Logo> {
        Some(match id {
            "ubuntu" | "kubuntu" | "xubuntu" | "lubuntu" | "neon" => &UBUNTU,
            "debian" => &DEBIAN,
            "fedora" => &FEDORA,
            "arch" | "archarm" | "endeavouros" | "garuda" | "artix" => &ARCH,
            "manjaro" | "manjaro-arm" => &MANJARO,
            "kali" => &KALI,
            "alpine" => &ALPINE,
            "rhel" | "centos" | "ol" | "redhat" => &REDHAT,
            "rocky" | "almalinux" => &ROCKY,
            "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" | "sles" | "suse" => &SUSE,
            "linuxmint" | "mint" => &MINT,
            "raspbian" => &RASPBIAN,
            "gentoo" => &GENTOO,
            "nixos" => &NIXOS,
            "pop" => &POP,
            "macos" | "darwin" => &MACOS,
            "freebsd" => &FREEBSD,
            "windows" => &WINDOWS,
            _ => return None,
        })
    };
    pick(&f.id).or_else(|| f.id_like.split_whitespace().find_map(pick)).unwrap_or(&TUX)
}

// ---------------------------------------------------------------- layout

fn size(kb: u64) -> String {
    let gib = kb as f64 / (1024.0 * 1024.0);
    if gib >= 1.0 { format!("{gib:.1} Gio") } else { format!("{:.0} Mio", kb as f64 / 1024.0) }
}

fn duration(s: u64) -> String {
    let (d, h, m) = (s / 86_400, s % 86_400 / 3600, s % 3600 / 60);
    match (d, h) {
        (0, 0) => format!("{m} min"),
        (0, _) => format!("{h} h {m} min"),
        _ => format!("{d} j {h} h {m} min"),
    }
}

fn percent(used: u64, total: u64) -> u64 {
    (used * 100).checked_div(total).unwrap_or(0)
}

/// Colour of a usage percentage: green, amber, red.
fn usage(used: u64, total: u64) -> String {
    let p = percent(used, total);
    let c = match p {
        0..=59 => 32,
        60..=84 => 33,
        _ => 31,
    };
    format!("{} / {} (\x1b[{c}m{p} %\x1b[0m)", size(used), size(total))
}

/// Characters a line takes on screen (colour codes left out).
fn visible(s: &str) -> usize {
    let mut n = 0;
    let mut esc = false;
    for c in s.chars() {
        match (esc, c) {
            (false, '\x1b') => esc = true,
            (true, 'm') => esc = false,
            (true, _) => {}
            _ => n += 1,
        }
    }
    n
}

/// Cuts a line to `max` visible characters (colour codes kept, reset at the end).
fn clip(s: &str, max: usize) -> String {
    if visible(s) <= max {
        return s.to_string();
    }
    let (mut out, mut n, mut esc) = (String::new(), 0, false);
    for c in s.chars() {
        if c == '\x1b' {
            esc = true;
        }
        if esc {
            out.push(c);
            esc = c != 'm';
            continue;
        }
        if n + 1 >= max {
            out.push('…');
            break;
        }
        out.push(c);
        n += 1;
    }
    out + "\x1b[0m"
}

/// The summary, with CR LF line ends, for a terminal `cols` characters wide.
pub fn render(f: &Facts, cols: u32) -> String {
    let logo = logo_for(f);
    let (c1, c2) = logo.colors;
    let accent = format!("\x1b[1;38;5;{c1}m");
    let mut info: Vec<String> = Vec::new();
    let title = match (f.user.is_empty(), f.host.is_empty()) {
        (false, false) => format!("{accent}{}\x1b[0m@{accent}{}\x1b[0m", f.user, f.host),
        (true, false) => format!("{accent}{}\x1b[0m", f.host),
        _ => format!("{accent}{}\x1b[0m", f.name),
    };
    let rule = "─".repeat(visible(&title).clamp(8, 40));
    info.push(title);
    info.push(rule);
    let mut add = |label: &str, value: String| {
        if !value.trim().is_empty() {
            info.push(format!("{accent}{label}\x1b[0m : {value}"));
        }
    };
    let os = if f.arch.is_empty() { f.name.clone() } else { format!("{} {}", f.name, f.arch) };
    add("Système", os);
    add("Noyau", f.kernel.clone());
    add("Allumé depuis", if f.uptime > 0 { duration(f.uptime) } else { String::new() });
    add("Paquets", f.packages.clone());
    add("Shell", f.shell.clone());
    let cpu = match (f.cpu.is_empty(), f.cpus) {
        (true, _) => String::new(),
        (false, 0) => f.cpu.clone(),
        (false, n) => format!("{} ({n})", f.cpu),
    };
    add("Processeur", cpu);
    add("Mémoire", if f.mem_total > 0 { usage(f.mem_used, f.mem_total) } else { String::new() });
    let disk = if f.id == "windows" { "Disque (C:)" } else { "Disque (/)" };
    add(disk, if f.disk_total > 0 { usage(f.disk_used, f.disk_total) } else { String::new() });
    add("Adresse IP", f.ip.clone());
    info.push(String::new());
    let mut blocks = String::new();
    for i in 0..8 {
        let _ = write!(blocks, "\x1b[4{i}m   ");
    }
    info.push(blocks + "\x1b[0m");
    let mut bright = String::new();
    for i in 0..8 {
        let _ = write!(bright, "\x1b[10{i}m   ");
    }
    info.push(bright + "\x1b[0m");

    let cols = cols.max(20) as usize;
    let logo_w = logo.lines.iter().map(|l| visible(&l.replace("$1", "").replace("$2", ""))).max().unwrap_or(0);
    // narrow terminal: the facts only
    let with_logo = cols >= logo_w + 3 + 34;
    let mut out = String::from("\r\n");
    let rows = if with_logo { logo.lines.len().max(info.len()) } else { info.len() };
    for i in 0..rows {
        let mut line = String::new();
        if with_logo {
            let raw = logo.lines.get(i).copied().unwrap_or("");
            let plain_w = visible(&raw.replace("$1", "").replace("$2", ""));
            let drawn = raw.replace("$1", &format!("\x1b[1;38;5;{c1}m")).replace("$2", &format!("\x1b[1;38;5;{c2}m"));
            line.push_str(&drawn);
            line.push_str("\x1b[0m");
            line.push_str(&" ".repeat(logo_w - plain_w + 3));
        } else {
            line.push(' ');
        }
        let room = cols.saturating_sub(visible(&line) + 1);
        if let Some(t) = info.get(i) {
            line.push_str(&clip(t, room));
        }
        out.push_str(line.trim_end());
        out.push_str("\x1b[0m\r\n");
    }
    out.push_str("\r\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const UBUNTU_OUT: &str = "@id ubuntu\n@like debian\n@name Ubuntu 24.04.1 LTS\n@user claude\n@host test-ubuntu\n\
@kernel Linux 6.8.0-49-generic\n@arch x86_64\n@shell bash\n@up 93784.21 370000.00\n\
@cpu  Intel(R) Core(TM) i3-10100 CPU @ 3.60GHz\n@ncpu 4\n@mem MemTotal:        8025004 kB\n\
@mem MemAvailable:    6012345 kB\n@disk /dev/sda2 61611820 12345678 46100000 22% /\n@ip 192.168.0.232\n\
@pkgs  1834 (dpkg) 12 (snap)\n";

    #[test]
    fn reads_a_linux_server() {
        let f = parse(UBUNTU_OUT).expect("facts");
        assert_eq!(f.id, "ubuntu");
        assert_eq!(f.name, "Ubuntu 24.04.1 LTS");
        assert_eq!(f.uptime, 93784);
        assert_eq!(f.cpu, "Intel(R) Core(TM) i3-10100 CPU @ 3.60GHz");
        assert_eq!(f.cpus, 4);
        assert_eq!(f.mem_total, 8025004);
        assert_eq!(f.mem_used, 8025004 - 6012345);
        assert_eq!((f.disk_total, f.disk_used), (61611820, 12345678));
        assert_eq!(f.packages, "1834 (dpkg), 12 (snap)");
        let text = render(&f, 120);
        assert!(text.contains("claude"), "{text}");
        assert!(text.contains("Ubuntu 24.04.1 LTS x86_64"));
        assert!(text.contains("1 j 2 h 3 min"));
        assert!(text.contains("\x1b[1;38;5;208m"), "Ubuntu orange");
        // every line fits in the terminal
        assert!(text.split("\r\n").all(|l| visible(l) <= 120));
        // narrow terminal: no logo, still within the width
        let narrow = render(&f, 40);
        assert!(narrow.split("\r\n").all(|l| visible(l) <= 40), "{narrow}");
        assert!(!narrow.contains("(  )"));
    }

    #[test]
    fn reads_windows_and_macos() {
        let w = parse("@id windows\r\n@name Microsoft Windows 11 Pro 24H2\r\n@memk 16777216 8388608\r\n@diskb 107374182400 53687091200\r\n@up 3600\r\n").unwrap();
        assert_eq!((w.mem_total, w.mem_used), (16777216, 8388608));
        assert_eq!((w.disk_total, w.disk_used), (104857600, 52428800));
        assert!(render(&w, 100).contains("████████"));
        assert!(render(&w, 100).contains("Disque (C:)"));
        let m = parse("@id Darwin\n@id macos\n@name macOS 15.1\n@memb 17179869184\n@boot 1000 4600\n").unwrap();
        assert_eq!(m.id, "macos");
        assert_eq!(m.mem_total, 16 * 1024 * 1024);
        assert_eq!(m.uptime, 3600);
    }

    #[test]
    fn logos_by_id_then_family() {
        let mut f = Facts { id: "pop".into(), ..Default::default() };
        assert!(std::ptr::eq(logo_for(&f), &POP));
        f.id = "zorin".into();
        f.id_like = "ubuntu debian".into();
        assert!(std::ptr::eq(logo_for(&f), &UBUNTU));
        f.id = "something".into();
        f.id_like.clear();
        assert!(std::ptr::eq(logo_for(&f), &TUX));
        assert_eq!(parse("not ours"), None);
    }

    #[test]
    fn script_is_quoted_for_sh_c() {
        let c = unix_command();
        assert!(c.starts_with("sh -c '") && c.ends_with('\''));
    }
}
