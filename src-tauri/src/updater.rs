//! Update from inside the application.
//!
//! Only the Windows installer can be replaced by the application itself: the new installer is
//! downloaded from the GitHub release, its SHA-256 is compared with the one published in
//! `SHA256SUMS.txt` of the same release, then it is started and Orbiterm closes. Every other
//! kind of installation (Scoop, portable zip, .deb, .rpm, Nix) is updated by its own tool:
//! the application only tells which command to run.

use serde::Serialize;

use crate::error::{AppError, AppResult};

const DOWNLOAD: &str = "https://github.com/Kaguyu-M/orbiterm/releases/download";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UpdateMode {
    /// "installer" (updated by the application), "scoop", "portable", "deb", "rpm", "nix", "other"
    pub kind: &'static str,
    /// command to run when the application cannot update itself ("" = none to suggest)
    pub hint: &'static str,
}

/// How this copy of Orbiterm was installed, from the place of its executable.
pub fn mode() -> UpdateMode {
    let exe = std::env::current_exe().unwrap_or_default();
    mode_of(&exe, |p| std::path::Path::new(p).exists())
}

fn mode_of(exe: &std::path::Path, exists: impl Fn(&str) -> bool) -> UpdateMode {
    let path = exe.to_string_lossy().replace('\\', "/").to_lowercase();
    let m = |kind, hint| UpdateMode { kind, hint };
    if cfg!(windows) {
        if path.contains("/scoop/apps/") {
            return m("scoop", "scoop update orbiterm");
        }
        // the installer writes its uninstaller next to the program
        let uninstall = exe.with_file_name("uninstall.exe");
        if exists(&uninstall.to_string_lossy()) {
            return m("installer", "");
        }
        return m("portable", "");
    }
    if path.starts_with("/nix/store/") {
        return m("nix", "nix flake update");
    }
    if exists("/var/lib/dpkg/info/orbiterm.list") {
        return m("deb", "sudo apt update && sudo apt install --only-upgrade orbiterm");
    }
    if path.starts_with("/usr/") && (exists("/usr/bin/dnf") || exists("/usr/bin/rpm")) {
        return m("rpm", "sudo dnf upgrade --refresh orbiterm");
    }
    m("other", "")
}

/// "0.8.4": digits and dots only (the version goes into a download address).
pub fn valid_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    (2..=4).contains(&parts.len()) && parts.iter().all(|p| !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Numeric comparison: "0.10.0" is newer than "0.9.3".
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let p = |v: &str| v.split('.').map(|x| x.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    let (a, b) = (p(candidate), p(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

pub fn installer_name(version: &str) -> String {
    format!("Orbiterm_{version}_x64-setup.exe")
}

/// SHA-256 (lower-case hexadecimal) of `name` in a `sha256sum` listing.
pub fn expected_sum(listing: &str, name: &str) -> Option<String> {
    listing.lines().find_map(|l| {
        let (sum, file) = l.trim().split_once(char::is_whitespace)?;
        let file = file.trim().trim_start_matches('*');
        (file == name && sum.len() == 64 && sum.bytes().all(|b| b.is_ascii_hexdigit())).then(|| sum.to_lowercase())
    })
}

pub fn release_url(version: &str, file: &str) -> String {
    format!("{DOWNLOAD}/v{version}/{file}")
}

/// Downloads the installer of `version` and checks it against the published SHA-256.
/// `progress(received, total)` is called as the bytes arrive. The file is deleted when the
/// check fails: nothing that was not verified stays on the disk.
#[cfg(windows)]
pub async fn download(version: &str, progress: impl Fn(u64, Option<u64>)) -> AppResult<std::path::PathBuf> {
    use sha2::{Digest, Sha256};
    use std::io::Write;

    const MAX: u64 = 400 * 1024 * 1024;
    if !valid_version(version) {
        return Err(AppError::Config("numéro de version invalide".into()));
    }
    let net = |e: reqwest::Error| AppError::Conn(format!("téléchargement impossible : {e}"));
    let client = reqwest::Client::builder()
        .user_agent("orbiterm-updater")
        .https_only(true)
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(net)?;

    let name = installer_name(version);
    let sums = client.get(release_url(version, "SHA256SUMS.txt")).send().await.map_err(net)?;
    if sums.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(AppError::Config(format!("la publication {version} est introuvable")));
    }
    let sums = sums.error_for_status().map_err(net)?.text().await.map_err(net)?;
    let want = expected_sum(&sums, &name).ok_or_else(|| AppError::Config("empreinte de l'installateur absente de la publication".into()))?;

    let mut resp = client.get(release_url(version, &name)).send().await.map_err(net)?.error_for_status().map_err(net)?;
    let total = resp.content_length();
    if total.is_some_and(|t| t > MAX) {
        return Err(AppError::Config("installateur anormalement volumineux".into()));
    }
    let dir = std::env::temp_dir().join("orbiterm-maj");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(&name);
    let mut file = std::fs::File::create(&path)?;
    let mut hash = Sha256::new();
    let mut got: u64 = 0;
    let res: AppResult<()> = async {
        while let Some(chunk) = resp.chunk().await.map_err(net)? {
            got += chunk.len() as u64;
            if got > MAX {
                return Err(AppError::Config("installateur anormalement volumineux".into()));
            }
            hash.update(&chunk);
            file.write_all(&chunk)?;
            progress(got, total);
        }
        file.flush()?;
        Ok(())
    }
    .await;
    drop(file);
    let sum: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    let res = res.and_then(|()| {
        if sum == want {
            Ok(())
        } else {
            Err(AppError::Config("l'empreinte SHA-256 du fichier téléchargé ne correspond pas à celle de la publication : installation annulée".into()))
        }
    });
    if let Err(e) = res {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    Ok(path)
}

#[cfg(not(windows))]
pub async fn download(_version: &str, _progress: impl Fn(u64, Option<u64>)) -> AppResult<std::path::PathBuf> {
    Err(AppError::Config("la mise à jour intégrée n'existe que pour l'installateur Windows".into()))
}

/// Switches of the installer for an update without any question: `/P` only shows a progress
/// bar, `/UPDATE` keeps the choices of the first installation, `/R` starts Orbiterm again.
pub const INSTALLER_ARGS: [&str; 3] = ["/P", "/R", "/UPDATE"];

/// Starts a verified installer. Windows asks for the authorisation itself when one is needed.
#[cfg(windows)]
pub fn launch(path: &std::path::Path) -> AppResult<()> {
    match std::process::Command::new(path).args(INSTALLER_ARGS).spawn() {
        Ok(_) => Ok(()),
        // 740: the program asks for elevation, which only the shell can grant
        Err(e) if e.raw_os_error() == Some(740) => {
            use std::os::windows::process::CommandExt;
            std::process::Command::new("cmd").args(["/C", "start", ""]).arg(path).args(INSTALLER_ARGS).creation_flags(0x0800_0000).spawn()?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

#[cfg(not(windows))]
pub fn launch(_path: &std::path::Path) -> AppResult<()> {
    Err(AppError::Config("la mise à jour intégrée n'existe que pour l'installateur Windows".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(valid_version("0.8.4") && valid_version("1.0") && valid_version("10.20.30"));
        for bad in ["", "0.8.4/../x", "v0.8.4", "0.8.4-beta", "0..1", "1", "0.8.4 ", "0.8.x", "1.2.3.4.5"] {
            assert!(!valid_version(bad), "{bad}");
        }
        assert!(is_newer("0.8.4", "0.8.3") && is_newer("0.10.0", "0.9.9") && is_newer("1.0", "0.99.99"));
        assert!(!is_newer("0.8.3", "0.8.3") && !is_newer("0.8.2", "0.8.3") && !is_newer("0.8", "0.8.0"));
    }

    #[test]
    fn checksum_listing() {
        let a = "a".repeat(64);
        let b = "B".repeat(64);
        let text = format!("{a}  Orbiterm_0.8.4_amd64.deb\n{b} *Orbiterm_0.8.4_x64-setup.exe\nabc  court.exe\n");
        assert_eq!(expected_sum(&text, "Orbiterm_0.8.4_x64-setup.exe"), Some("b".repeat(64)));
        assert_eq!(expected_sum(&text, "Orbiterm_0.8.4_amd64.deb"), Some(a));
        assert_eq!(expected_sum(&text, "court.exe"), None); // not a SHA-256
        assert_eq!(expected_sum(&text, "setup.exe"), None); // the whole name must match
        assert_eq!(release_url("0.8.4", "SHA256SUMS.txt"), "https://github.com/Kaguyu-M/orbiterm/releases/download/v0.8.4/SHA256SUMS.txt");
    }

    #[cfg(windows)]
    #[test]
    fn install_kind_windows() {
        let p = std::path::Path::new;
        let none = |_: &str| false;
        assert_eq!(mode_of(p(r"C:\Users\x\scoop\apps\orbiterm\current\orbiterm.exe"), none).kind, "scoop");
        assert_eq!(mode_of(p(r"C:\Outils\Orbiterm\orbiterm.exe"), none).kind, "portable");
        let m = mode_of(p(r"C:\Users\x\AppData\Local\Orbiterm\orbiterm.exe"), |f| f.ends_with(r"Orbiterm\uninstall.exe"));
        assert_eq!(m, UpdateMode { kind: "installer", hint: "" });
    }

    #[cfg(not(windows))]
    #[test]
    fn install_kind_linux() {
        let p = std::path::Path::new;
        assert_eq!(mode_of(p("/nix/store/abc-orbiterm/bin/orbiterm"), |_| false).kind, "nix");
        assert_eq!(mode_of(p("/usr/bin/orbiterm"), |f| f == "/var/lib/dpkg/info/orbiterm.list").kind, "deb");
        assert_eq!(mode_of(p("/usr/bin/orbiterm"), |f| f == "/usr/bin/dnf").kind, "rpm");
        assert_eq!(mode_of(p("/home/x/orbiterm"), |_| false).kind, "other");
    }
}
