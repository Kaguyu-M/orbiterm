//! Welcome box written in an SSH terminal before the server's own output: what the
//! session uses (direct or through a jump host, compression, SFTP browser, X11, agent).

/// Facts about the session known when it opens.
pub struct BannerInfo {
    pub user: String,
    pub host: String,
    pub port: u16,
    /// "user@gateway" when the session goes through a jump host
    pub jump: Option<String>,
    pub compression: bool,
    pub sftp_browser: bool,
    pub agent_forward: bool,
}

/// Outcome of the X11 forwarding request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X11State {
    /// not asked for in the session settings
    Off,
    Accepted,
    /// the server refused it (X11Forwarding no, xauth missing…)
    Refused,
    /// the local display setting could not be used
    BadDisplay,
}

/// Inner width of the box (characters between the borders).
const WIDTH: usize = 70;

const FRAME: &str = "\x1b[33m";
const TITLE: &str = "\x1b[1;97m";
const SOFT: &str = "\x1b[37m";
const ARROW: &str = "\x1b[1;36m";
const YES: &str = "\x1b[1;32m✓\x1b[0m";
const NO: &str = "\x1b[1;31m✗\x1b[0m";
const RESET: &str = "\x1b[0m";

/// One line of the box: `text` holds colour codes, `plain` is the same text without them.
fn row(out: &mut String, text: &str, plain: &str) {
    let pad = WIDTH.saturating_sub(1 + plain.chars().count());
    out.push_str(&format!("{FRAME}│{RESET} {text}{}{FRAME}│{RESET}\r\n", " ".repeat(pad)));
}

fn centered(out: &mut String, colour: &str, plain: &str) {
    let len = plain.chars().count();
    let left = WIDTH.saturating_sub(len) / 2;
    let right = WIDTH.saturating_sub(len + left);
    out.push_str(&format!("{FRAME}│{RESET}{}{colour}{plain}{RESET}{}{FRAME}│{RESET}\r\n", " ".repeat(left), " ".repeat(right)));
}

/// Cuts `s` to `max` characters (with an ellipsis).
fn fit(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

fn item(out: &mut String, label: &str, on: bool, note: &str) {
    let mark = if on { YES } else { NO };
    let note = if note.is_empty() { String::new() } else { format!("  ({})", fit(note, 38)) };
    let plain = format!("  • {label:<18}:  ✓{note}");
    row(out, &format!("  • {label:<18}:  {mark}{SOFT}{note}{RESET}"), &plain);
}

/// The box, with CR LF line ends, ready to be written into the terminal.
pub fn render(info: &BannerInfo, x11: X11State) -> String {
    let mut out = String::from("\r\n");
    out.push_str(&format!("{FRAME}┌{}┐{RESET}\r\n", "─".repeat(WIDTH)));
    centered(&mut out, TITLE, &format!("• Orbiterm v{} •", env!("CARGO_PKG_VERSION")));
    centered(&mut out, SOFT, "(terminal, SSH, SFTP, bureau à distance et outils réseau)");
    row(&mut out, "", "");
    let target = match (info.user.is_empty(), info.port) {
        (true, 22) => info.host.clone(),
        (true, p) => format!("{}:{p}", info.host),
        (false, 22) => format!("{}@{}", info.user, info.host),
        (false, p) => format!("{}@{}:{p}", info.user, info.host),
    };
    let target = fit(&target, WIDTH - 22);
    row(&mut out, &format!("{ARROW}⮞{RESET} Session SSH vers {TITLE}{target}{RESET}"), &format!("⮞ Session SSH vers {target}"));
    match &info.jump {
        None => item(&mut out, "SSH direct", true, ""),
        Some(j) => item(&mut out, "Rebond SSH", true, &format!("via {j}")),
    }
    item(&mut out, "Compression SSH", info.compression, "");
    item(&mut out, "Navigateur SFTP", info.sftp_browser, "");
    let (on, note) = match x11 {
        X11State::Off => (false, "désactivée dans la session"),
        X11State::Accepted => (true, ""),
        X11State::Refused => (false, "refusée par le serveur"),
        X11State::BadDisplay => (false, "affichage local invalide"),
    };
    item(&mut out, "Redirection X11", on, note);
    item(&mut out, "Transfert d'agent", info.agent_forward, "");
    row(&mut out, "", "");
    row(
        &mut out,
        &format!("{ARROW}⮞{RESET} Aide et raccourcis clavier : menu {TITLE}Aide{RESET}."),
        "⮞ Aide et raccourcis clavier : menu Aide.",
    );
    out.push_str(&format!("{FRAME}└{}┘{RESET}\r\n\r\n", "─".repeat(WIDTH)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(s: &str) -> String {
        let mut out = String::new();
        let mut esc = false;
        for c in s.chars() {
            if esc {
                if c.is_ascii_alphabetic() {
                    esc = false;
                }
            } else if c == '\x1b' {
                esc = true;
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn every_line_has_the_same_width() {
        let info = BannerInfo {
            user: "root".into(),
            host: "un-serveur-avec-un-nom-vraiment-tres-tres-long.exemple.interne.lan".into(),
            port: 2222,
            jump: Some("admin@passerelle-avec-un-nom-assez-long.exemple.org".into()),
            compression: true,
            sftp_browser: true,
            agent_forward: false,
        };
        for x11 in [X11State::Off, X11State::Accepted, X11State::Refused, X11State::BadDisplay] {
            let text = strip(&render(&info, x11));
            let lines: Vec<&str> = text.split("\r\n").filter(|l| !l.is_empty()).collect();
            assert_eq!(lines.len(), 13);
            for l in &lines {
                assert_eq!(l.chars().count(), WIDTH + 2, "{l:?}");
            }
        }
    }

    #[test]
    fn shows_the_session_facts() {
        let info = BannerInfo {
            user: "root".into(),
            host: "192.168.0.205".into(),
            port: 22,
            jump: None,
            compression: false,
            sftp_browser: true,
            agent_forward: false,
        };
        let text = strip(&render(&info, X11State::Refused));
        assert!(text.contains("⮞ Session SSH vers root@192.168.0.205 "));
        assert!(text.contains("• SSH direct        :  ✓"));
        assert!(text.contains("• Compression SSH   :  ✗"));
        assert!(text.contains("• Redirection X11   :  ✗  (refusée par le serveur)"));
        assert!(text.contains(&format!("Orbiterm v{}", env!("CARGO_PKG_VERSION"))));
    }
}
