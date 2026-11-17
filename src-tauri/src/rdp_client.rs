//! Embedded RDP client (IronRDP): the remote desktop is drawn inside the Orbiterm tab.
//!
//! The RDP connection (TCP → TLS → CredSSP/NLA → active session) runs in the backend.
//! The tab's canvas talks to it through a WebSocket bound to 127.0.0.1 with a random token
//! (like the VNC bridge):
//! - backend → tab (binary): `1` rect (x, y, w, h: u16 LE, then RGBA), `2` desktop size (w, h),
//!   `3` cursor (hot x, hot y, w, h, RGBA), `4` default cursor, `5` hidden cursor,
//!   `8` end of a frame (time in ms: u32 LE);
//! - backend → tab (JSON text): `{"t":"clip","s":…}` text copied on the remote computer;
//! - tab → backend (JSON text): keyboard scancodes, mouse, wheel, resize, Ctrl+Alt+Del,
//!   local clipboard text, frame acknowledgement.
//!
//! Frames are paced by the tab: the next one is only sent once the previous one has been drawn
//! (`{"t":"a"}`). Meanwhile the changed areas accumulate, so a slow display skips images
//! instead of falling behind the remote desktop.
//!
//! Static channels: CLIPRDR (shared clipboard, text) and RDPDR (one shared local folder).
//!
//! The server certificate is checked "trust on first use" (file `rdp_known_hosts`),
//! like SSH host keys. Passwords only travel inside CredSSP; nothing is written to disk.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use ironrdp_connector::connection_activation::ConnectionActivationState;
use ironrdp_connector::{BitmapConfig, ClientConnector, ConnectorErrorKind, Credentials, DesktopSize, ServerName};
use ironrdp_core::WriteBuf;
use ironrdp_displaycontrol::client::DisplayControlClient;
use ironrdp_displaycontrol::pdu::MonitorLayoutEntry;
use ironrdp_dvc::DrdynvcClient;
use ironrdp_egfx::client::{GraphicsPipelineClient, GraphicsPipelineHandler};
use ironrdp_graphics::image_processing::PixelFormat;
use ironrdp_pdu::geometry::InclusiveRectangle;
use ironrdp_pdu::input::fast_path::{FastPathInputEvent, KeyboardFlags, SynchronizeFlags};
use ironrdp_pdu::input::mouse::PointerFlags;
use ironrdp_pdu::input::MousePdu;
use ironrdp_pdu::rdp::capability_sets::{client_codecs_capabilities, MajorPlatformType};
use ironrdp_pdu::rdp::client_info::{PerformanceFlags, TimezoneInfo};
use ironrdp_session::image::DecodedImage;
use ironrdp_session::{fast_path, ActiveStageBuilder, ActiveStageOutput};
use ironrdp_connector::connection_activation::ConnectionActivationSequence;
use ironrdp_connector::{Sequence as _, Written};
use ironrdp_tokio::{split_tokio_framed, FramedWrite as _};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use ironrdp_cliprdr::CliprdrClient;
use ironrdp_cliprdr::pdu::{ClipboardFormat, FormatDataResponse};
use ironrdp_rdpdr::Rdpdr;

use crate::error::{AppError, AppResult};
use crate::rdp_channels::{self, CF_UNICODETEXT, ClipEvent};
use crate::known_hosts::HostKeyStatus;
use crate::ssh::{CloseSink, HostKeyPrompter, HostKeyQuestion, ended};

pub struct RdpParams {
    pub host: String,
    pub port: u16,
    /// Name the certificate is recorded under (the real target, even through an SSH gateway).
    pub cert_host: String,
    pub cert_port: u16,
    pub username: String,
    pub password: String,
    pub domain: Option<String>,
    pub width: u16,
    pub height: u16,
    pub known_hosts: PathBuf,
    /// shared clipboard (text)
    pub clipboard: bool,
    /// local folder shared with the remote computer
    pub share_dir: Option<PathBuf>,
    /// play the remote computer's sound in the tab
    pub sound: bool,
    /// monitors of this computer (multi-monitor session): x, y, width, height, primary
    pub monitors: Vec<(i32, i32, u32, u32, bool)>,
    /// Remote Desktop Gateway the connection goes through (HTTPS)
    pub gateway: Option<RdGateway>,
    /// image quality against responsiveness (session option rdp_quality)
    pub quality: Quality,
    /// display scale of this computer in % (125, 150…), 100 = none (session option rdp_scale)
    pub scale: u32,
    /// file of the persistent bitmap cache (session option rdp_cache = "0" turns it off)
    pub bitmap_cache: Option<PathBuf>,
}

/// Session option "rdp_quality": what the server is asked to send.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Quality {
    /// LAN when the server answers fast, slow link otherwise (VPN, Internet, RD gateway)
    #[default]
    Auto,
    /// best picture: 32-bit colours, wallpaper, smoothed fonts
    Lan,
    /// lighter traffic: 16-bit colours, no wallpaper, no font smoothing, no animations
    Slow,
}

impl Quality {
    pub fn parse(s: &str) -> Self {
        match s {
            "lan" => Quality::Lan,
            "slow" => Quality::Slow,
            _ => Quality::Auto,
        }
    }
}

/// Above this TCP connect time the link is treated as slow: a LAN, even over Wi-Fi, answers in
/// well under 40 ms; a VPN or the Internet usually above.
const SLOW_LINK_RTT: Duration = Duration::from_millis(40);

/// Whether the slow-link profile applies: `rtt` is the TCP connect time (None through a gateway).
fn slow_link(q: Quality, rtt: Option<Duration>) -> bool {
    match q {
        Quality::Lan => false,
        Quality::Slow => true,
        Quality::Auto => rtt.is_none_or(|d| d > SLOW_LINK_RTT),
    }
}

/// Remote Desktop Gateway (MS-TSGU): the RDP connection is carried over HTTPS.
#[derive(Clone)]
pub struct RdGateway {
    /// host[:port] (443 by default)
    pub endpoint: String,
    pub username: String,
    pub password: String,
    /// accept a certificate that does not validate (self-signed gateway)
    pub insecure: bool,
}

type Stream = Box<dyn AsyncReadWrite + Unpin + Send + Sync>;

/// Deadline of each step before the certificate question (negotiation, TLS upgrade).
const NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(20);

/// Opens the transport to the RDP server: a TCP connection, or a tunnel through the gateway.
/// Also returns the TCP connect time (round trip), None through a gateway.
async fn open_transport(p: &RdpParams) -> AppResult<(Stream, SocketAddr, Option<Duration>)> {
    let Some(gw) = &p.gateway else {
        let unreachable = || AppError::Conn(format!("{}:{} ne répond pas", p.host, p.port));
        let started = std::time::Instant::now();
        let (stream, rtt) = tokio::time::timeout(Duration::from_secs(15), async {
            // name resolution apart, so that a slow DNS answer is not taken for a slow link
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((p.host.as_str(), p.port))
                .await
                .map_err(|e| AppError::Conn(format!("{}:{} : {e}", p.host, p.port)))?
                .collect();
            let mut last = None;
            for a in addrs {
                let t0 = std::time::Instant::now();
                match TcpStream::connect(a).await {
                    Ok(s) => return Ok((s, t0.elapsed())),
                    Err(e) => last = Some(e),
                }
            }
            Err(match last {
                Some(e) => AppError::Conn(format!("{}:{} : {e}", p.host, p.port)),
                None => unreachable(),
            })
        })
        .await
        .map_err(|_| unreachable())??;
        log::info!("RDP {}:{} : connexion TCP en {:?} (total {:?})", p.host, p.port, rtt, started.elapsed());
        let _ = stream.set_nodelay(true);
        // a dead link (Wi-Fi lost, the other computer slept) is noticed in about 30 s
        // instead of the system's 2 hours: the tab can then reconnect
        let keepalive = socket2::TcpKeepalive::new().with_time(Duration::from_secs(15)).with_interval(Duration::from_secs(3));
        if let Err(e) = socket2::SockRef::from(&stream).set_tcp_keepalive(&keepalive) {
            log::warn!("RDP : keepalive TCP impossible : {e}");
        }
        let addr = stream.local_addr()?;
        return Ok((Box::new(stream), addr, Some(rtt)));
    };
    use ironrdp_mstsgu::{GwClient, GwConnectTarget, GwErrorKind};
    let target = GwConnectTarget {
        gw_endpoint: gw.endpoint.clone(),
        gw_user: gw.username.clone(),
        gw_pass: gw.password.clone(),
        smart_card: None,
        server: p.host.clone(),
    };
    let validation = if gw.insecure {
        ironrdp_tls::CertificateValidation::DangerouslyAcceptInvalidCertificate
    } else {
        ironrdp_tls::CertificateValidation::Strict
    };
    let name = hostname_for_gateway();
    let r = tokio::time::timeout(
        Duration::from_secs(30),
        GwClient::connect_with_port_and_certificate_validation(&target, &name, p.port, validation, None),
    )
    .await
    .map_err(|_| AppError::Conn(format!("passerelle {} : délai dépassé", gw.endpoint)))?;
    match r {
        Ok((client, addr)) => Ok((Box::new(client), addr, None)),
        Err(e) => Err(match e.kind() {
            GwErrorKind::HttpStatus(401) | GwErrorKind::HttpStatus(403) => AppError::JumpAuthFailed,
            kind => {
                // the root cause is at the end of the source chain
                let mut detail = e.to_string();
                let mut src = std::error::Error::source(&e);
                while let Some(x) = src {
                    detail = x.to_string();
                    src = x.source();
                }
                let cert = !gw.insecure
                    && ["certif", "trust", "verif", "untrusted"].iter().any(|w| format!("{e} {detail}").to_lowercase().contains(w));
                AppError::Conn(if cert {
                    format!(
                        "passerelle {} : certificat non reconnu ({detail}) — option « Accepter le certificat de la passerelle » si tu lui fais confiance",
                        gw.endpoint
                    )
                } else if matches!(kind, GwErrorKind::Custom) {
                    format!("passerelle {} : {detail}", gw.endpoint)
                } else {
                    format!("passerelle {} : {kind} ({detail})", gw.endpoint)
                })
            }
        }),
    }
}

fn hostname_for_gateway() -> String {
    std::env::var(if cfg!(windows) { "COMPUTERNAME" } else { "HOSTNAME" }).unwrap_or_else(|_| "orbiterm".into())
}

/// Remote desktop spanning `monitors` (physical pixels of this computer): its size and the
/// monitor layout sent to the server, where the primary monitor starts at (0, 0).
pub fn monitor_layout(monitors: &[(i32, i32, u32, u32, bool)]) -> Option<(u16, u16, Vec<ironrdp_pdu::gcc::Monitor>)> {
    use ironrdp_pdu::gcc::{Monitor, MonitorFlags};
    let (px, py) = monitors.iter().find(|m| m.4).or(monitors.first()).map(|m| (m.0, m.1))?;
    let mut out: Vec<Monitor> = monitors
        .iter()
        .map(|&(x, y, w, h, primary)| Monitor {
            left: x - px,
            top: y - py,
            right: x - px + w as i32 - 1,
            bottom: y - py + h as i32 - 1,
            flags: if primary { MonitorFlags::PRIMARY } else { MonitorFlags::empty() },
        })
        .collect();
    // exactly one primary monitor (the first one when the system does not say)
    if !out.iter().any(|m| m.flags.contains(MonitorFlags::PRIMARY)) {
        out[0].flags = MonitorFlags::PRIMARY;
    }
    let (l, t) = (out.iter().map(|m| m.left).min()?, out.iter().map(|m| m.top).min()?);
    let (r, b) = (out.iter().map(|m| m.right).max()?, out.iter().map(|m| m.bottom).max()?);
    let (w, h) = ((r - l + 1).clamp(200, 8192) as u16, (b - t + 1).clamp(200, 8192) as u16);
    Some((w, h, out))
}

/// Messages from the tab.
#[derive(Deserialize, Debug)]
#[serde(tag = "t")]
enum WsIn {
    /// scancode (set 1); 0xE0xx = extended key
    #[serde(rename = "k")]
    Key { sc: u16, up: bool },
    #[serde(rename = "m")]
    Move { x: u16, y: u16 },
    /// button 0 left, 1 middle, 2 right
    #[serde(rename = "b")]
    Button { b: u8, down: bool, x: u16, y: u16 },
    #[serde(rename = "w")]
    Wheel { d: i16, #[serde(default)] h: bool, x: u16, y: u16 },
    /// new size of the tab; `s`: display scale in % (0 = unchanged)
    #[serde(rename = "r")]
    Resize { w: u16, h: u16, #[serde(default)] s: u32 },
    /// lock keys state (focus)
    #[serde(rename = "s")]
    Sync { caps: bool, num: bool, scroll: bool },
    #[serde(rename = "cad")]
    CtrlAltDel,
    /// text typed as Unicode key events (paste of the local clipboard)
    #[serde(rename = "u")]
    Text { s: String },
    /// the local clipboard changed (text)
    #[serde(rename = "clip")]
    Clip { s: String },
    /// the last frame has been drawn: the next one may be sent
    #[serde(rename = "a")]
    Ack,
}

enum Cmd {
    In(WsIn),
    Viewer(Box<tokio_tungstenite::WebSocketStream<TcpStream>>),
    Close,
}

pub struct RdpSession {
    port: u16,
    token: String,
    cmd: mpsc::UnboundedSender<Cmd>,
    pub desktop: (u16, u16),
}

impl RdpSession {
    pub fn url(&self) -> String {
        format!("ws://127.0.0.1:{}/{}", self.port, self.token)
    }

    /// A key typed while the system shortcuts are captured (full screen, see keyhook).
    pub fn key(&self, sc: u16, up: bool) {
        let _ = self.cmd.send(Cmd::In(WsIn::Key { sc, up }));
    }

    pub fn ctrl_alt_del(&self) {
        let _ = self.cmd.send(Cmd::In(WsIn::CtrlAltDel));
    }

    pub fn close(&self) {
        let _ = self.cmd.send(Cmd::Close);
    }
}

impl Drop for RdpSession {
    fn drop(&mut self) {
        // the session thread shuts down gracefully and stops its runtime (viewer bridge included)
        let _ = self.cmd.send(Cmd::Close);
    }
}

// ---------------------------------------------------------------- certificates (TOFU)

fn cert_line_host(host: &str, port: u16) -> String {
    format!("{}:{port}", host.to_ascii_lowercase())
}

pub fn cert_check(path: &Path, host: &str, port: u16, fingerprint: &str) -> HostKeyStatus {
    let want = cert_line_host(host, port);
    let Ok(text) = std::fs::read_to_string(path) else {
        return HostKeyStatus::Unknown;
    };
    for line in text.lines() {
        let mut p = line.split_whitespace();
        if p.next() == Some(want.as_str()) {
            return if p.next() == Some(fingerprint) { HostKeyStatus::Known } else { HostKeyStatus::Changed };
        }
    }
    HostKeyStatus::Unknown
}

pub fn cert_learn(path: &Path, host: &str, port: u16, fingerprint: &str) -> AppResult<()> {
    let want = cert_line_host(host, port);
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> =
        text.lines().filter(|l| l.split_whitespace().next() != Some(want.as_str())).map(str::to_string).collect();
    lines.push(format!("{want} {fingerprint}"));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, lines.join("\n") + "\n")?;
    Ok(())
}

pub fn fingerprint(public_key: &[u8]) -> String {
    use base64::Engine as _;
    format!("SHA256:{}", base64::engine::general_purpose::STANDARD_NO_PAD.encode(Sha256::digest(public_key)))
}

// ---------------------------------------------------------------- connection

/// Keyboard layout announced to the server (the remote session uses it).
fn keyboard_layout() -> u32 {
    #[cfg(windows)]
    {
        // HKCU\Keyboard Layout\Preload\1 = "0000040c" (French) …
        if let Ok(k) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).open_subkey("Keyboard Layout\\Preload")
            && let Ok(v) = k.get_value::<String, _>("1")
            && let Ok(n) = u32::from_str_radix(v.trim(), 16)
        {
            return n & 0xffff;
        }
    }
    #[cfg(not(windows))]
    {
        let lang = std::env::var("LANG").unwrap_or_default();
        let l = match lang.get(..5).unwrap_or("") {
            "fr_FR" => 0x040c,
            "fr_BE" => 0x080c,
            "fr_CH" => 0x100c,
            "fr_CA" => 0x0c0c,
            "de_DE" => 0x0407,
            "en_GB" => 0x0809,
            "es_ES" => 0x040a,
            "it_IT" => 0x0410,
            _ => 0,
        };
        if l != 0 {
            return l;
        }
    }
    0x0409
}

fn client_name() -> String {
    let n = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "orbiterm".into());
    n.chars().take(15).collect()
}

/// Graphics pipeline (RDPGFX), as used by the Windows client: the server encodes the desktop
/// in H.264 (or RemoteFX progressive), far less data and server work than bitmap updates.
/// Windows only for now (H.264 decoded by Media Foundation); ORBITERM_RDP_NOGFX=1 turns it off.
fn gfx_enabled() -> bool {
    (cfg!(windows) || cfg!(target_os = "linux")) && std::env::var_os("ORBITERM_RDP_NOGFX").is_none()
}

#[cfg(windows)]
fn h264_decoder() -> Option<Box<dyn ironrdp_egfx::decode::H264Decoder>> {
    use ironrdp_egfx::decode::YuvMatrix;
    match ironrdp_egfx_native::MediaFoundationH264Decoder::new(YuvMatrix::Bt709Full) {
        Ok(d) => Some(Box::new(d)),
        Err(e) => {
            // Windows N editions without the Media Feature Pack: RemoteFX progressive instead
            log::warn!("RDP : décodeur H.264 de Windows indisponible ({e})");
            None
        }
    }
}

/// Linux: the system's GStreamer (graphics card through VA-API, else gst-libav / openh264).
#[cfg(target_os = "linux")]
fn h264_decoder() -> Option<Box<dyn ironrdp_egfx::decode::H264Decoder>> {
    crate::gst_h264::GstH264::new().map(|d| Box::new(d) as Box<dyn ironrdp_egfx::decode::H264Decoder>)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn h264_decoder() -> Option<Box<dyn ironrdp_egfx::decode::H264Decoder>> {
    None
}

struct GfxEvents;
impl GraphicsPipelineHandler for GfxEvents {}

/// Picture coding in use, shown in the statistics: 0 bitmap updates, 1 graphics pipeline,
/// 2 graphics pipeline with H.264.
fn picture_mode(stage: &ironrdp_session::ActiveStage) -> u8 {
    match stage.get_dvc::<GraphicsPipelineClient>() {
        Some(c) if c.processor().is_active() => {
            if c.processor().codec_capabilities().avc420 { 2 } else { 1 }
        }
        _ => 0,
    }
}

/// Desktop scale factor sent to the server (MS-RDPBCGR: 100 to 500 %), None at 100 %.
pub fn scale_factor(scale: u32) -> Option<u32> {
    (scale > 100).then(|| scale.min(500))
}

/// Device scale factor matching a desktop scale (the protocol only knows 100, 140 and 180 %).
fn device_scale(scale: u32) -> u32 {
    match scale {
        0..=132 => 100,
        133..=165 => 140,
        _ => 180,
    }
}

/// Physical size in millimetres of a desktop of `w` × `h` pixels shown at `scale` %
/// (96 dpi at 100 %): the server derives its DPI from it.
fn physical_mm(w: u16, h: u16, scale: u32) -> (u32, u32) {
    let mm = |px: u16| ((f64::from(px) * 25.4 * 100.0 / (96.0 * f64::from(scale.max(100)))).round() as u32).clamp(10, 10000);
    (mm(w), mm(h))
}

fn desktop_w(p: &RdpParams) -> u16 {
    monitor_layout(&p.monitors).map_or(p.width, |(w, _, _)| w)
}

fn desktop_h(p: &RdpParams) -> u16 {
    monitor_layout(&p.monitors).map_or(p.height, |(_, h, _)| h)
}

fn connector_config(p: &RdpParams, slow: bool) -> AppResult<ironrdp_connector::Config> {
    let codecs = client_codecs_capabilities(&[]).map_err(|e| AppError::Conn(format!("codecs RDP : {e}")))?;
    use ironrdp_pdu::gcc::ConnectionType;
    // slow link: the same choices as mstsc's "low-speed broadband" experience
    let (connection_type, color_depth, performance_flags) = if slow {
        (
            ConnectionType::BroadbandLow,
            16,
            PerformanceFlags::DISABLE_WALLPAPER
                | PerformanceFlags::DISABLE_FULLWINDOWDRAG
                | PerformanceFlags::DISABLE_MENUANIMATIONS
                | PerformanceFlags::DISABLE_CURSOR_SHADOW,
        )
    } else {
        // local network: mstsc's "LAN" experience (window contents shown while dragged,
        // font smoothing, desktop composition); IronRDP's default only drew window outlines
        (ConnectionType::Lan, 32, PerformanceFlags::ENABLE_FONT_SMOOTHING | PerformanceFlags::ENABLE_DESKTOP_COMPOSITION)
    };
    // automatic quality: the server measures the link (round trip, bandwidth) at connection time
    // and during the session, and adapts what it sends (H.264 quality…) — like mstsc's
    // "detect connection quality automatically"
    let connection_type = if p.quality == Quality::Auto { ConnectionType::Autodetect } else { connection_type };
    Ok(ironrdp_connector::Config {
        credentials: Credentials::UsernamePassword { username: p.username.clone(), password: p.password.clone() },
        domain: p.domain.clone(),
        enable_tls: true,
        enable_credssp: true,
        keyboard_type: ironrdp_pdu::gcc::KeyboardType::IBM_ENHANCED,
        keyboard_subtype: 0,
        keyboard_layout: keyboard_layout(),
        keyboard_functional_keys_count: 12,
        ime_file_name: String::new(),
        dig_product_id: String::new(),
        desktop_size: match monitor_layout(&p.monitors) {
            Some((w, h, _)) => DesktopSize { width: w, height: h },
            None => DesktopSize { width: p.width, height: p.height },
        },
        desktop_scale_factor: scale_factor(p.scale).unwrap_or(0),
        monitor_layout: monitor_layout(&p.monitors)
            .filter(|(_, _, m)| m.len() > 1)
            .map(|(_, _, monitors)| ironrdp_pdu::gcc::ClientMonitorData { monitors }),
        desktop_physical_size: scale_factor(p.scale).map(|s| physical_mm(desktop_w(p), desktop_h(p), s)),
        desktop_orientation: None,
        device_scale_factor: scale_factor(p.scale).map(device_scale),
        monitor_extended_layout: None,
        relative_mouse_input: false,
        enable_standard_rdp_security: false,
        connection_type,
        remote_application_mode: false,
        rail_support_level: ironrdp_pdu::rdp::capability_sets::RailSupportLevel::empty(),
        support_dyn_vc_gfx_protocol: gfx_enabled(),
        enable_audio_capture: false,
        bitmap: Some(BitmapConfig { color_depth, lossy_compression: true, codecs }),
        client_build: 0,
        client_name: client_name(),
        client_dir: String::new(),
        platform: if cfg!(windows) { MajorPlatformType::WINDOWS } else { MajorPlatformType::UNIX },
        hardware_id: None,
        license_cache: None,
        enable_server_pointer: true,
        // without NLA (TLS only), log on with the same credentials instead of the logon screen
        autologon: !p.password.is_empty(),
        enable_audio_playback: p.sound,
        request_data: None,
        pointer_software_rendering: false,
        multitransport_flags: None,
        // no bulk compression: xrdp then compresses even the share-data PDUs of the
        // Deactivation-Reactivation sequence, which IronRDP cannot decode there (the
        // session dropped right after the logon on Kali / xrdp)
        compression_type: None,
        performance_flags,
        timezone_info: TimezoneInfo::default(),
        alternate_shell: String::new(),
        work_dir: String::new(),
    })
}

/// No Kerberos KDC proxy: CredSSP uses NTLM.
struct NoNetwork;

impl ironrdp_tokio::NetworkClient for NoNetwork {
    async fn send(
        &mut self,
        _req: &ironrdp_connector::sspi::generator::NetworkRequest,
    ) -> ironrdp_connector::ConnectorResult<Vec<u8>> {
        Err(ironrdp_connector::general_err!("Kerberos non pris en charge"))
    }
}

fn map_connect_err(e: ironrdp_connector::ConnectorError) -> AppError {
    let msg = format!("{e:#}");
    match e.kind() {
        ConnectorErrorKind::AccessDenied => AppError::AuthFailed,
        ConnectorErrorKind::Credssp(se) => {
            // the server refused the credentials: it answers with an NTSTATUS
            match se.nstatus.map(|n| n.0) {
                Some(0xC000_0234) => AppError::Conn("RDP : compte verrouillé".into()),
                Some(0xC000_0072) => AppError::Conn("RDP : compte désactivé".into()),
                Some(0xC000_0071 | 0xC000_0193) => AppError::Conn("RDP : mot de passe ou compte expiré".into()),
                Some(0xC000_0224) => AppError::Conn("RDP : le mot de passe doit être changé avant la connexion".into()),
                Some(0xC000_006F | 0xC000_0070) => AppError::Conn("RDP : connexion non autorisée à cette heure / depuis ce poste".into()),
                Some(_) => AppError::AuthFailed,
                None if matches!(se.error_type, sspi_kind::LogonDenied | sspi_kind::NoCredentials) => AppError::AuthFailed,
                None => AppError::Conn(format!("RDP (NLA) : {} ({:?})", se.description, se.error_type)),
            }
        }
        _ => AppError::Conn(format!("RDP : {}", readable(&msg))),
    }
}

/// IronRDP error text without its source locations ("[write all @ …/framed.rs:314] custom
/// error"); an I/O failure without detail means the link dropped during the exchange.
fn readable(msg: &str) -> String {
    let mut out = String::new();
    let mut rest = msg;
    while let Some(start) = rest.find('[') {
        let Some(end) = rest[start..].find(']') else { break };
        if !rest[start..start + end].contains(" @ ") {
            out.push_str(&rest[..start + end + 1]);
            rest = &rest[start + end + 1..];
        } else {
            out.push_str(&rest[..start]);
            rest = rest[start + end + 1..].trim_start();
        }
    }
    out.push_str(rest);
    let out = out.trim().to_string();
    if out.is_empty() || out == "custom error" {
        "la connexion a été coupée pendant l'ouverture de la session".into()
    } else {
        out
    }
}

use ironrdp_connector::sspi::ErrorKind as sspi_kind;
trait AsyncReadWrite: AsyncRead + AsyncWrite {}
impl<T> AsyncReadWrite for T where T: AsyncRead + AsyncWrite {}
type Framed = ironrdp_tokio::TokioFramed<Box<dyn AsyncReadWrite + Unpin + Send + Sync>>;

/// Connects and logs on, then runs the session and its viewer bridge.
///
/// IronRDP's futures are not `Send`: everything runs on a dedicated thread with its own
/// single-threaded runtime; the outcome of the logon comes back through a oneshot.
pub async fn connect(p: RdpParams, prompter: HostKeyPrompter, on_close: CloseSink) -> AppResult<RdpSession> {
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<AppResult<(u16, String, (u16, u16))>>();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let loop_tx = cmd_tx.clone();
    std::thread::Builder::new()
        .name("rdp-session".into())
        .spawn(move || {
            multimedia_thread();
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ready_tx.send(Err(AppError::Conn(e.to_string())));
                    return;
                }
            };
            rt.block_on(async move {
                let scale = p.scale;
                let (framed, result, channels) = match establish(p, prompter).await {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let listener = match TcpListener::bind("127.0.0.1:0").await {
                    Ok(l) => l,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e.into()));
                        return;
                    }
                };
                let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
                let token = uuid::Uuid::new_v4().simple().to_string();
                let desktop = (result.desktop_size.width, result.desktop_size.height);
                tokio::spawn(accept_viewers(listener, format!("/{token}"), loop_tx.clone()));
                if ready_tx.send(Ok((port, token, desktop))).is_err() {
                    return;
                }
                let reason = match active_session(framed, result, channels, loop_tx, cmd_rx, scale).await {
                    Ok(r) => r,
                    Err(e) => crate::ssh::lost(format!("connexion RDP interrompue : {e}")),
                };
                on_close(reason);
            });
        })
        .map_err(|e| AppError::Conn(e.to_string()))?;
    let (port, token, desktop) = ready_rx.await.map_err(|_| AppError::Conn("session RDP interrompue".into()))??;
    Ok(RdpSession { port, token, cmd: cmd_tx, desktop })
}

/// Windows: the session thread (network, decoding, frames for the tab) gets the "Games"
/// multimedia class, as the Windows client does for its own threads (AvSetMmThreadCharacteristics):
/// the scheduler keeps it running while other programs load the processor.
fn multimedia_thread() {
    #[cfg(windows)]
    {
        let task: Vec<u16> = "Games\0".encode_utf16().collect();
        let mut index = 0u32;
        // SAFETY: NUL-terminated string and a valid out pointer; the registration ends with the thread.
        let h = unsafe { windows_sys::Win32::System::Threading::AvSetMmThreadCharacteristicsW(task.as_ptr(), &mut index) };
        if h.is_null() {
            log::debug!("RDP : classe multimédia du thread refusée");
        }
    }
}

/// Typed text ("Coller le texte"): keyboard events sent per tick of the session loop (10 ms).
/// Thousands of events at once overflow the input of the remote program (letters were lost in
/// the Windows run box); 8 events = 4 characters every 10 ms, about 400 characters a second.
const TYPING_EVENTS_PER_TICK: usize = 8;

/// Frame acknowledgements of the graphics pipeline held longer than this are sent anyway
/// (a tab that stopped drawing: window minimized, page busy).
const ACK_HOLD_MAX: Duration = Duration::from_millis(250);

/// The frame acknowledgements held by the graphics pipeline, encoded for the server.
fn take_frame_acks(stage: &mut ironrdp_session::ActiveStage) -> Result<Option<Vec<u8>>, String> {
    let Some(mut gfx) = stage.get_dvc_mut::<GraphicsPipelineClient>() else {
        return Ok(None);
    };
    if gfx.processor().held_frame_acks() == 0 {
        return Ok(None);
    }
    let id = gfx.channel_id();
    let acks = gfx.processor_mut().take_frame_acks(ironrdp_egfx::pdu::QueueDepth::Unavailable);
    let msgs = ironrdp_dvc::encode_dvc_messages(id, acks, ironrdp_svc::ChannelFlags::empty()).map_err(|e| e.to_string())?;
    stage.encode_dvc_messages(msgs).map(Some).map_err(|e| e.to_string())
}

struct Channels {
    clip: Option<mpsc::UnboundedReceiver<ClipEvent>>,
    /// sound frames for the tab
    audio: Option<mpsc::UnboundedReceiver<Vec<u8>>>,
    drive: bool,
}

async fn establish(
    p: RdpParams,
    prompter: HostKeyPrompter,
) -> AppResult<(Framed, ironrdp_connector::ConnectionResult, Channels)> {
    if p.username.trim().is_empty() {
        return Err(AppError::Config("RDP : indique le nom d'utilisateur dans les paramètres de la session".into()));
    }
    let (stream, client_addr, rtt) = open_transport(&p).await?;
    let slow = slow_link(p.quality, rtt);
    log::info!("RDP {}:{} : profil {}", p.host, p.port, if slow { "lien lent" } else { "réseau local" });
    let mut framed = ironrdp_tokio::TokioFramed::new(stream);
    let mut dvc = DrdynvcClient::new().with_dynamic_channel(DisplayControlClient::new(|_| Ok(Vec::new())));
    if gfx_enabled() {
        // IronRDP composes the pipeline's picture into the session image after each PDU
        let mut gfx = GraphicsPipelineClient::new(Box::new(GfxEvents), h264_decoder());
        // frames are acknowledged once the tab has shown them (see `active_session`)
        gfx.set_deferred_frame_ack(true);
        if let Some(path) = &p.bitmap_cache {
            gfx = gfx.with_persistent_cache(Box::new(crate::rdp_cache::FileCache::new(path.clone())));
        }
        dvc = dvc.with_dynamic_channel(gfx);
    }
    let mut connector = ClientConnector::new(connector_config(&p, slow)?, client_addr).with_static_channel(dvc);
    let mut channels = Channels { clip: None, drive: false, audio: None };
    if p.clipboard {
        let (cliprdr, rx) = rdp_channels::clipboard();
        connector.attach_static_channel(cliprdr);
        channels.clip = Some(rx);
    }
    if let Some(dir) = &p.share_dir {
        let rdpdr = rdp_channels::drive_channel(dir)
            .map_err(|e| AppError::Config(format!("RDP, dossier partagé : {e}")))?;
        connector.attach_static_channel(rdpdr);
        channels.drive = true;
    }
    if p.sound {
        let (rdpsnd, rx) = rdp_channels::audio();
        connector.attach_static_channel(rdpsnd);
        channels.audio = Some(rx);
    } else if channels.drive {
        // Windows only answers on RDPDR when RDPSND is advertised too (MS-RDPEFS appendix A<1>);
        // no audio format is offered, so nothing is played
        connector.attach_static_channel(ironrdp_rdpsnd::client::Rdpsnd::new(Box::new(
            ironrdp_rdpsnd::client::NoopRdpsndBackend,
        )));
    }

    let should_upgrade = tokio::time::timeout(NEGOTIATION_TIMEOUT, ironrdp_tokio::connect_begin(&mut framed, &mut connector))
        .await
        .map_err(|_| AppError::Conn(format!("{}:{} : le serveur RDP ne répond pas à la négociation", p.host, p.port)))?
        .map_err(map_connect_err)?;
    let (initial, leftover) = framed.into_inner();
    let (tls, cert) = tokio::time::timeout(NEGOTIATION_TIMEOUT, ironrdp_tls::upgrade(initial, &p.host))
        .await
        .map_err(|_| AppError::Conn(format!("{}:{} : le passage en TLS n'aboutit pas", p.host, p.port)))?
        .map_err(|e| AppError::Conn(format!("TLS : {e}")))?;
    let public_key = ironrdp_tls::extract_tls_server_public_key(&cert)
        .ok_or_else(|| AppError::Conn("certificat du serveur illisible".into()))?
        .to_owned();

    // trust on first use, like SSH host keys
    let fp = fingerprint(&public_key);
    match cert_check(&p.known_hosts, &p.cert_host, p.cert_port, &fp) {
        HostKeyStatus::Known => {}
        status => {
            let q = HostKeyQuestion {
                host: p.cert_host.clone(),
                port: p.cert_port,
                algorithm: "Certificat RDP (TLS)".into(),
                fingerprint: fp.clone(),
                status,
            };
            if !prompter(q).await {
                return Err(AppError::HostKeyRejected);
            }
            cert_learn(&p.known_hosts, &p.cert_host, p.cert_port, &fp)?;
        }
    }

    let upgraded = ironrdp_tokio::mark_as_upgraded(should_upgrade, &mut connector);
    let erased: Box<dyn AsyncReadWrite + Unpin + Send + Sync> = Box::new(tls);
    let mut framed: Framed = ironrdp_tokio::TokioFramed::new_with_leftover(erased, leftover);
    let result = tokio::time::timeout(
        Duration::from_secs(40),
        ironrdp_tokio::connect_finalize(
            upgraded,
            connector,
            &mut framed,
            &mut NoNetwork,
            ServerName::new(p.host.clone()),
            public_key,
            None,
        ),
    )
    .await
    .map_err(|_| AppError::Conn("le serveur RDP ne termine pas la connexion".into()))?
    .and_then(|o| o.into_connected())
    .map_err(map_connect_err)?;

    Ok((framed, result, channels))
}

async fn accept_viewers(listener: TcpListener, want: String, cmd: mpsc::UnboundedSender<Cmd>) {
    use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
    use tokio_tungstenite::tungstenite::http::StatusCode;
    while let Ok((sock, _)) = listener.accept().await {
        let _ = sock.set_nodelay(true);
        let want = want.clone();
        #[allow(clippy::result_large_err)]
        let check = move |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
            if req.uri().path() != want {
                let mut e = ErrorResponse::new(Some("forbidden".into()));
                *e.status_mut() = StatusCode::FORBIDDEN;
                return Err(e);
            }
            Ok(resp)
        };
        if let Ok(ws) = tokio_tungstenite::accept_hdr_async(sock, check).await
            && cmd.send(Cmd::Viewer(Box::new(ws))).is_err()
        {
            break;
        }
    }
}

// ---------------------------------------------------------------- active session

type Sink = futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<TcpStream>, Message>;

/// Inclusive left, top, right, bottom.
type Area = (u16, u16, u16, u16);

fn pixels((l, t, r, b): Area) -> u64 {
    (u64::from(r.saturating_sub(l)) + 1) * (u64::from(b.saturating_sub(t)) + 1)
}

fn union(a: Area, b: Area) -> Area {
    (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))
}

/// Areas of the desktop changed since the last frame sent to the tab.
///
/// Areas are merged when their union holds few pixels that did not change: two small
/// updates far apart (a clock and a cursor) stay two small images instead of one that
/// spans the whole desktop.
#[derive(Default)]
struct Dirty(Vec<Area>);

impl Dirty {
    /// pixels a merge may add before two areas are kept apart
    const SLACK: u64 = 16 * 1024;
    const MAX: usize = 24;

    fn add(&mut self, r: &InclusiveRectangle) {
        self.push((r.left, r.top, r.right, r.bottom));
    }
    fn push(&mut self, mut n: Area) {
        while let Some(i) = self.0.iter().position(|&o| pixels(union(o, n)) <= pixels(o) + pixels(n) + Self::SLACK) {
            n = union(self.0.swap_remove(i), n);
        }
        self.0.push(n);
        if self.0.len() > Self::MAX {
            let all = self.0.iter().copied().reduce(union);
            self.0.clear();
            self.0.extend(all);
        }
    }
    fn all(&mut self, w: u16, h: u16) {
        self.0.clear();
        if w > 0 && h > 0 {
            self.0.push((0, 0, w - 1, h - 1));
        }
    }
    fn clear(&mut self) {
        self.0.clear();
    }
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    fn take(&mut self) -> Vec<Area> {
        std::mem::take(&mut self.0)
    }
}

fn rect_message(image: &DecodedImage, (l, t, r, b): (u16, u16, u16, u16)) -> Option<Vec<u8>> {
    let (iw, ih) = (image.width(), image.height());
    if iw == 0 || ih == 0 || l >= iw || t >= ih {
        return None;
    }
    let (r, b) = (r.min(iw - 1), b.min(ih - 1));
    let (w, h) = (r - l + 1, b - t + 1);
    let stride = image.stride();
    let data = image.data();
    let mut out = Vec::with_capacity(9 + usize::from(w) * usize::from(h) * 4);
    out.push(1u8);
    for v in [l, t, w, h] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for y in t..=b {
        let start = usize::from(y) * stride + usize::from(l) * 4;
        out.extend_from_slice(&data[start..start + usize::from(w) * 4]);
    }
    // opaque: the alpha of the decoded image is not meaningful
    for px in out[9..].as_chunks_mut::<4>().0 {
        px[3] = 0xff;
    }
    Some(out)
}

/// Message 9: network characteristics measured by the server (average round trip in ms,
/// bandwidth in kbit/s, 0 when unknown).
fn link_message(rtt_ms: u32, kbps: u32) -> Vec<u8> {
    let mut m = vec![9u8];
    m.extend_from_slice(&rtt_ms.to_le_bytes());
    m.extend_from_slice(&kbps.to_le_bytes());
    m
}

fn size_message(w: u16, h: u16) -> Vec<u8> {
    let mut m = vec![2u8];
    m.extend_from_slice(&w.to_le_bytes());
    m.extend_from_slice(&h.to_le_bytes());
    m
}

fn key(sc: u16, up: bool) -> FastPathInputEvent {
    let mut flags = KeyboardFlags::empty();
    if up {
        flags |= KeyboardFlags::RELEASE;
    }
    if sc & 0xff00 == 0xe000 {
        flags |= KeyboardFlags::EXTENDED;
    }
    FastPathInputEvent::KeyboardEvent(flags, (sc & 0xff) as u8)
}

fn input_events(m: WsIn) -> Vec<FastPathInputEvent> {
    match m {
        WsIn::Key { sc, up } => vec![key(sc, up)],
        WsIn::Move { x, y } => vec![FastPathInputEvent::MouseEvent(MousePdu {
            flags: PointerFlags::MOVE,
            number_of_wheel_rotation_units: 0,
            x_position: x,
            y_position: y,
        })],
        WsIn::Button { b, down, x, y } => {
            let mut flags = match b {
                0 => PointerFlags::LEFT_BUTTON,
                1 => PointerFlags::MIDDLE_BUTTON_OR_WHEEL,
                _ => PointerFlags::RIGHT_BUTTON,
            };
            if down {
                flags |= PointerFlags::DOWN;
            }
            vec![FastPathInputEvent::MouseEvent(MousePdu { flags, number_of_wheel_rotation_units: 0, x_position: x, y_position: y })]
        }
        WsIn::Wheel { d, h, x, y } => vec![FastPathInputEvent::MouseEvent(MousePdu {
            flags: if h { PointerFlags::HORIZONTAL_WHEEL } else { PointerFlags::VERTICAL_WHEEL },
            number_of_wheel_rotation_units: d.clamp(-255, 255),
            x_position: x,
            y_position: y,
        })],
        WsIn::Sync { caps, num, scroll } => {
            let mut f = SynchronizeFlags::empty();
            f.set(SynchronizeFlags::CAPS_LOCK, caps);
            f.set(SynchronizeFlags::NUM_LOCK, num);
            f.set(SynchronizeFlags::SCROLL_LOCK, scroll);
            vec![FastPathInputEvent::SyncEvent(f)]
        }
        WsIn::CtrlAltDel => vec![key(0x1d, false), key(0x38, false), key(0xe053, false), key(0xe053, true), key(0x38, true), key(0x1d, true)],
        WsIn::Text { s } => s
            .replace("\r\n", "\r")
            .replace('\n', "\r")
            .encode_utf16()
            .take(20_000)
            .flat_map(|c| {
                if c == u16::from(b'\r') {
                    // Enter as a real key
                    [key(0x1c, false), key(0x1c, true)]
                } else {
                    [
                        FastPathInputEvent::UnicodeKeyboardEvent(KeyboardFlags::empty(), c),
                        FastPathInputEvent::UnicodeKeyboardEvent(KeyboardFlags::RELEASE, c),
                    ]
                }
            })
            .collect(),
        WsIn::Resize { .. } | WsIn::Clip { .. } | WsIn::Ack => vec![],
    }
}

/// Messages for the tab. They are written by a task of their own: a tab that reads slowly
/// never holds back the RDP session (keyboard, mouse, server traffic).
struct Viewer {
    tx: mpsc::UnboundedSender<Message>,
    /// messages not written yet
    queued: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Viewer {
    fn start(mut sink: Sink) -> Self {
        use std::sync::atomic::Ordering;
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        let queued = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let left = queued.clone();
        tokio::spawn(async move {
            while let Some(m) = rx.recv().await {
                let mut n = 1;
                let mut ok = sink.feed(m).await.is_ok();
                // everything already waiting goes out in one write
                while ok && let Ok(m) = rx.try_recv() {
                    n += 1;
                    ok = sink.feed(m).await.is_ok();
                }
                ok = ok && sink.flush().await.is_ok();
                left.fetch_sub(n, Ordering::Relaxed);
                if !ok {
                    break;
                }
            }
        });
        Viewer { tx, queued }
    }

    fn idle(&self) -> bool {
        self.queued.load(std::sync::atomic::Ordering::Relaxed) == 0
    }
}

fn emit(viewer: &mut Option<Viewer>, m: Message) {
    if let Some(v) = viewer {
        v.queued.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if v.tx.send(m).is_err() {
            *viewer = None;
        }
    }
}

/// Statistics joined to each frame end.
#[derive(Default, Clone, Copy)]
struct Wire {
    /// bytes received from the server
    bytes: u64,
    /// see [picture_mode]
    mode: u8,
}

/// Sends the changed areas as one frame, closed by the marker the tab acknowledges.
fn send_frame(viewer: &mut Option<Viewer>, image: &DecodedImage, dirty: &mut Dirty, started: std::time::Instant, wire: Wire) {
    for area in dirty.take() {
        if let Some(m) = rect_message(image, area) {
            emit(viewer, Message::binary(m));
        }
    }
    let mut end = vec![8u8];
    end.extend_from_slice(&(started.elapsed().as_millis() as u32).to_le_bytes());
    end.extend_from_slice(&((wire.bytes / 1024) as u32).to_le_bytes());
    end.push(wire.mode);
    emit(viewer, Message::binary(end));
}

/// One step of the Deactivation-Reactivation sequence (server-initiated, e.g. xrdp after
/// the logon, or after a resolution change).
///
/// IronRDP aborts on any share-data PDU it does not expect during the finalization
/// (xrdp sends some there); like mstsc, such PDUs are skipped and the sequence goes on.
async fn reactivation_step<S: ironrdp_tokio::FramedRead>(
    reader: &mut ironrdp_tokio::Framed<S>,
    activation: &mut ConnectionActivationSequence,
    buf: &mut WriteBuf,
    skipped: &mut u32,
    deferred: &mut Vec<Vec<u8>>,
) -> Result<Written, String> {
    loop {
        buf.clear();
        let Some(hint) = activation.next_pdu_hint() else {
            return activation.step_no_input(buf).map_err(|e| e.to_string());
        };
        let pdu = reader.read_by_hint(hint).await.map_err(|e| e.to_string())?;
        // virtual channel data (clipboard, drives…): handled once the session is active again
        if let Ok(ctx) = ironrdp_pdu::mcs::decode_send_data_indication(&pdu)
            && ctx.channel_id != activation.io_channel_id()
        {
            if deferred.len() < 1024 {
                deferred.push(pdu.to_vec());
            }
            continue;
        }
        let backup = activation.clone();
        match activation.step(&pdu, reader.last_read_at(), buf) {
            Ok(w) => return Ok(w),
            Err(e)
                if *skipped < 256
                    && matches!(backup.connection_activation_state(), ConnectionActivationState::ConnectionFinalization { .. }) =>
            {
                log::warn!("RDP : PDU ignoré pendant la réactivation ({}) : {e}", describe_pdu(&pdu));
                #[cfg(debug_assertions)]
                eprintln!("RDP : PDU ignoré pendant la réactivation ({}) : {e}", describe_pdu(&pdu));
                *activation = backup;
                *skipped += 1;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Short description of a slow-path PDU, for the logs.
fn describe_pdu(pdu: &[u8]) -> String {
    use ironrdp_pdu::rdp::headers::ShareControlPdu;
    let d = ironrdp_pdu::mcs::decode_send_data_indication(pdu)
        .ok()
        .and_then(|ctx| ironrdp_pdu::rdp::headers::decode_share_control(ctx).ok())
        .map(|c| match c.pdu {
            ShareControlPdu::Data(h) => format!("{:?}", h.share_data_pdu),
            other => format!("{other:?}"),
        })
        .unwrap_or_else(|| format!("{} octets", pdu.len()));
    d.chars().take(80).collect()
}

/// The user left the remote session (logoff, "Disconnect" in the remote menu), as opposed
/// to an error, a timeout or another connection taking the session over.
fn user_ended(reason: &ironrdp_session::GracefulDisconnectReason) -> bool {
    use ironrdp_pdu::rdp::server_error_info::{ErrorInfo, ProtocolIndependentCode as C};
    use ironrdp_session::GracefulDisconnectReason as R;
    match reason {
        R::UserInitiated | R::ServerInitiated => true,
        R::Other(text) => [C::LogoffByUser, C::RpcInitiatedDisconnectByuser]
            .into_iter()
            .any(|c| *text == ErrorInfo::ProtocolIndependentCode(c).description()),
    }
}

async fn active_session(
    framed: Framed,
    result: ironrdp_connector::ConnectionResult,
    channels: Channels,
    cmd_tx: mpsc::UnboundedSender<Cmd>,
    mut cmd_rx: mpsc::UnboundedReceiver<Cmd>,
    initial_scale: u32,
) -> Result<String, String> {
    log::info!(
        "RDP : canal de messages (mesure du réseau par le serveur) : {}",
        result.message_channel_id.map_or("absent".to_string(), |id| id.to_string())
    );
    let (mut reader, mut writer) = split_tokio_framed(framed);
    let mut image = DecodedImage::new(PixelFormat::RgbA32, result.desktop_size.width, result.desktop_size.height);
    let activation_factory = result.activation_factory;
    let mut stage = ActiveStageBuilder {
        static_channels: result.static_channels,
        user_channel_id: result.user_channel_id,
        io_channel_id: result.io_channel_id,
        message_channel_id: result.message_channel_id,
        share_id: result.share_id,
        compression_type: result.compression_type,
        enable_server_pointer: result.enable_server_pointer,
        pointer_software_rendering: result.pointer_software_rendering,
    }
    .build();

    let mut viewer: Option<Viewer> = None;
    let mut wire = Wire::default();
    // size last announced to the tab (the graphics pipeline resizes the image itself)
    let mut shown = (image.width(), image.height());
    let mut dirty = Dirty::default();
    // frame sent to the tab and not acknowledged yet
    let mut in_flight: Option<std::time::Instant> = None;
    let started = std::time::Instant::now();
    // diagnostics: ORBITERM_RDP_NOACK=1 sends every frame without waiting for the tab
    let paced = std::env::var_os("ORBITERM_RDP_NOACK").is_none();
    let mut flush = tokio::time::interval(Duration::from_millis(10));
    flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut pending_resize: Option<(u16, u16, tokio::time::Instant)> = None;
    // display scale last sent to the server (connection, then resizes)
    let mut scale = initial_scale;
    let mut last_pointer: Option<Vec<u8>> = None;
    // last network measurement of the server (message for the tab)
    let mut link: Option<Vec<u8>> = None;
    let Channels { clip: mut clip_rx, drive, audio: mut audio_rx } = channels;
    // clipboard: our text (CRLF, as sent to the server), last text received from the server
    let mut local_clip: Option<String> = None;
    let mut remote_clip: Option<String> = None;
    // graphics pipeline: frame acknowledgements held since (released once the tab has shown the
    // frames, so the server encodes at the pace of the display), and "the tab just drew a frame"
    let mut acks_held: Option<std::time::Instant> = None;
    let mut tab_drew = false;
    // text typed with "Coller le texte", sent a few characters per tick
    let mut typing: std::collections::VecDeque<FastPathInputEvent> = std::collections::VecDeque::new();

    loop {
        let outputs = tokio::select! {
            // keyboard and mouse first: a busy server connection never delays them
            biased;
            cmd = cmd_rx.recv() => match cmd {
                None | Some(Cmd::Close) => {
                    let out = stage.graceful_shutdown().map_err(|e| e.to_string())?;
                    for o in out {
                        if let ActiveStageOutput::ResponseFrame(f) = o {
                            let _ = writer.write_all(&f).await;
                        }
                    }
                    return Ok("session RDP fermée".into());
                }
                Some(Cmd::Viewer(ws)) => {
                    let (s, mut stream) = (*ws).split();
                    viewer = Some(Viewer::start(s));
                    emit(&mut viewer, Message::binary(size_message(image.width(), image.height())));
                    if let Some(p) = &last_pointer {
                        emit(&mut viewer, Message::binary(p.clone()));
                    }
                    if let Some(m) = &link {
                        emit(&mut viewer, Message::binary(m.clone()));
                    }
                    in_flight = None;
                    dirty.all(image.width(), image.height());
                    // viewer input → this loop (through the command channel)
                    let tx = cmd_tx.clone();
                    tokio::spawn(async move {
                        while let Some(Ok(m)) = stream.next().await {
                            if let Message::Text(t) = m
                                && let Ok(i) = serde_json::from_str::<WsIn>(&t)
                                && tx.send(Cmd::In(i)).is_err()
                            {
                                break;
                            }
                        }
                    });
                    Vec::new()
                }
                Some(Cmd::In(WsIn::Ack)) => {
                    in_flight = None;
                    tab_drew = true;
                    if paced && !dirty.is_empty() && viewer.is_some() {
                        send_frame(&mut viewer, &image, &mut dirty, started, wire);
                        in_flight = Some(std::time::Instant::now());
                    }
                    Vec::new()
                }
                Some(Cmd::In(WsIn::Resize { w, h, s })) => {
                    // debounced: applied 400 ms after the last resize of the tab
                    let rescale = s != 0 && s != scale;
                    if rescale {
                        scale = s;
                    }
                    if w >= 200 && h >= 200 && ((w, h) != (image.width(), image.height()) || rescale) {
                        pending_resize = Some((w, h, tokio::time::Instant::now() + Duration::from_millis(400)));
                    }
                    Vec::new()
                }
                Some(Cmd::In(WsIn::Clip { s })) => {
                    // ignore the echo of what the server just gave us
                    if remote_clip.as_deref() != Some(s.as_str()) && !s.is_empty() {
                        let t = rdp_channels::to_remote(&s);
                        if local_clip.as_deref() != Some(t.as_str()) {
                            local_clip = Some(t);
                            if let Some(cliprdr) = stage.get_svc_processor_mut::<CliprdrClient>()
                                && cliprdr.is_ready()
                            {
                                let msgs = cliprdr
                                    .initiate_copy(&[ClipboardFormat::new(CF_UNICODETEXT)])
                                    .map_err(|e| e.to_string())?;
                                let frame = stage.process_svc_processor_messages(msgs).map_err(|e| e.to_string())?;
                                writer.write_all(&frame).await.map_err(|e| e.to_string())?;
                            }
                        }
                    }
                    Vec::new()
                }
                Some(Cmd::In(m @ WsIn::Text { .. })) => {
                    typing.extend(input_events(m));
                    Vec::new()
                }
                Some(Cmd::In(m)) => {
                    let events = input_events(m);
                    if events.is_empty() {
                        Vec::new()
                    } else {
                        stage.process_fastpath_input(&mut image, &events).map_err(|e| e.to_string())?
                    }
                }
            },
            a = async {
                match audio_rx.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                match a {
                    // no viewer: the sound is dropped (nobody to hear it)
                    Some(frame) => emit(&mut viewer, Message::binary(frame)),
                    None => audio_rx = None,
                }
                continue;
            }
            ev = async {
                match clip_rx.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                let Some(ev) = ev else {
                    clip_rx = None;
                    continue;
                };
                let Some(cliprdr) = stage.get_svc_processor_mut::<CliprdrClient>() else { continue };
                let msgs = match ev {
                    ClipEvent::AdvertiseLocal => {
                        let formats: Vec<ClipboardFormat> =
                            local_clip.iter().map(|_| ClipboardFormat::new(CF_UNICODETEXT)).collect();
                        Some(cliprdr.initiate_copy(&formats))
                    }
                    ClipEvent::RemoteCopy(true) => Some(cliprdr.initiate_paste(CF_UNICODETEXT)),
                    ClipEvent::RemoteCopy(false) => None,
                    ClipEvent::DataRequest(format) => {
                        let resp = match (&local_clip, format == CF_UNICODETEXT) {
                            (Some(t), true) => FormatDataResponse::new_unicode_string(t),
                            _ => FormatDataResponse::new_error(),
                        };
                        Some(cliprdr.submit_format_data(resp))
                    }
                    ClipEvent::DataResponse(text) => {
                        if let Some(t) = text
                            && !t.is_empty()
                        {
                            let m = serde_json::json!({ "t": "clip", "s": t }).to_string();
                            remote_clip = Some(t);
                            emit(&mut viewer, Message::text(m));
                        }
                        None
                    }
                };
                if let Some(msgs) = msgs {
                    let msgs = msgs.map_err(|e| e.to_string())?;
                    let frame = stage.process_svc_processor_messages(msgs).map_err(|e| e.to_string())?;
                    writer.write_all(&frame).await.map_err(|e| e.to_string())?;
                }
                Vec::new()
            }
            frame = reader.read_pdu() => {
                let (action, payload) = match frame {
                    Ok(f) => f,
                    // the server closed the connection cleanly (xrdp logoff, …)
                    Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                        return Ok(ended("session RDP fermée par le serveur"));
                    }
                    Err(e) => return Err(e.to_string()),
                };
                wire.bytes += payload.len() as u64;
                // arrival time: the server's bandwidth measurements are timed with it
                let out = stage.process(&mut image, action, &payload, reader.last_read_at()).map_err(|e| e.to_string())?;
                if (image.width(), image.height()) != shown {
                    shown = (image.width(), image.height());
                    wire.mode = picture_mode(&stage);
                    emit(&mut viewer, Message::binary(size_message(shown.0, shown.1)));
                    in_flight = None;
                    dirty.all(shown.0, shown.1);
                }
                if wire.mode == 0 && wire.bytes < 4 << 20 {
                    wire.mode = picture_mode(&stage);
                }
                out
            }
            _ = flush.tick() => {
                if !typing.is_empty() {
                    let n = typing.len().min(TYPING_EVENTS_PER_TICK);
                    let chunk: Vec<FastPathInputEvent> = typing.drain(..n).collect();
                    for o in stage.process_fastpath_input(&mut image, &chunk).map_err(|e| e.to_string())? {
                        if let ActiveStageOutput::ResponseFrame(f) = o {
                            writer.write_all(&f).await.map_err(|e| e.to_string())?;
                        }
                    }
                }
                // shared folder: I/O completed by the backend's worker threads
                if drive
                    && let Some(rdpdr) = stage.get_svc_processor_mut::<Rdpdr>()
                {
                    let msgs = rdpdr.poll_deferred_messages().map_err(|e| e.to_string())?;
                    if !msgs.is_empty() {
                        let frame = stage.process_svc_messages_by_name(&Rdpdr::NAME, msgs).map_err(|e| e.to_string())?;
                        writer.write_all(&frame).await.map_err(|e| e.to_string())?;
                    }
                }
                if let Some((w, h, at)) = pending_resize
                    && at <= tokio::time::Instant::now()
                {
                    pending_resize = None;
                    let (w, h) = MonitorLayoutEntry::adjust_display_size(u32::from(w), u32::from(h));
                    let (sf, dims) = match scale_factor(scale) {
                        Some(sf) => (Some(sf), Some(physical_mm(w as u16, h as u16, sf))),
                        None => (Some(100), None),
                    };
                    match stage.encode_resize(w, h, sf, dims) {
                        Some(Ok(frame)) => vec![ActiveStageOutput::ResponseFrame(frame)],
                        Some(Err(e)) => return Err(e.to_string()),
                        None => Vec::new(), // server without dynamic resolution: keep the size, the tab scales
                    }
                } else {
                    // without a viewer the changes keep accumulating (as one area at most)
                    // an acknowledgement lost with a stalled tab: go on once its queue is empty
                    let free = match in_flight {
                        None => true,
                        Some(t) => !paced || (t.elapsed() > Duration::from_millis(500) && viewer.as_ref().is_some_and(Viewer::idle)),
                    };
                    if free && !dirty.is_empty() && viewer.is_some() {
                        send_frame(&mut viewer, &image, &mut dirty, started, wire);
                        in_flight = Some(std::time::Instant::now());
                    }
                    Vec::new()
                }
            }
        };

        for out in outputs {
            match out {
                ActiveStageOutput::ResponseFrame(f) => writer.write_all(&f).await.map_err(|e| e.to_string())?,
                ActiveStageOutput::GraphicsUpdate(r) => dirty.add(&r),
                ActiveStageOutput::PointerDefault => {
                    last_pointer = Some(vec![4]);
                    emit(&mut viewer, Message::binary(vec![4u8]));
                }
                ActiveStageOutput::PointerHidden => {
                    last_pointer = Some(vec![5]);
                    emit(&mut viewer, Message::binary(vec![5u8]));
                }
                ActiveStageOutput::PointerPosition { .. } => {}
                ActiveStageOutput::AutoDetect(ironrdp_pdu::rdp::autodetect::AutoDetectRequest::NetworkCharacteristicsResult {
                    bandwidth_kbps,
                    average_rtt_ms,
                    ..
                }) => {
                    // the link as the server measured it: shown in the statistics
                    link = Some(link_message(average_rtt_ms, bandwidth_kbps.unwrap_or(0)));
                    if let Some(m) = &link {
                        emit(&mut viewer, Message::binary(m.clone()));
                    }
                }
                ActiveStageOutput::AutoDetect(other) => log::debug!("RDP : mesure réseau {other:?}"),
                ActiveStageOutput::PointerBitmap(p) => {
                    let mut m = vec![3u8];
                    for v in [p.hotspot_x, p.hotspot_y, p.width, p.height] {
                        m.extend_from_slice(&v.to_le_bytes());
                    }
                    m.extend_from_slice(&p.bitmap_data);
                    last_pointer = Some(m.clone());
                    emit(&mut viewer, Message::binary(m));
                }
                ActiveStageOutput::DeactivateAll => {
                    // Deactivation-Reactivation Sequence (e.g. after a resolution change)
                    let mut activation = activation_factory.create();
                    let mut buf = WriteBuf::new();
                    let mut skipped = 0u32;
                    let mut deferred = Vec::new();
                    loop {
                        let written =
                            reactivation_step(&mut reader, &mut activation, &mut buf, &mut skipped, &mut deferred).await?;
                        if written.size().is_some() {
                            writer.write_all(buf.filled()).await.map_err(|e| e.to_string())?;
                        }
                        if let ConnectionActivationState::Finalized {
                            desktop_size,
                            share_id,
                            enable_server_pointer,
                            pointer_software_rendering,
                            ..
                        } = activation.connection_activation_state()
                        {
                            image = DecodedImage::new(PixelFormat::RgbA32, desktop_size.width, desktop_size.height);
                            stage.set_fastpath_processor(
                                fast_path::ProcessorBuilder {
                                    io_channel_id: activation.io_channel_id(),
                                    user_channel_id: activation.user_channel_id(),
                                    share_id,
                                    enable_server_pointer,
                                    pointer_software_rendering,
                                }
                                .build(),
                            );
                            stage.set_share_id(share_id);
                            stage.set_enable_server_pointer(enable_server_pointer);
                            emit(&mut viewer, Message::binary(size_message(desktop_size.width, desktop_size.height)));
                            shown = (desktop_size.width, desktop_size.height);
                            dirty.clear();
                            break;
                        }
                    }
                    for pdu in deferred {
                        let outs = stage.process(&mut image, ironrdp_pdu::Action::X224, &pdu, None).map_err(|e| e.to_string())?;
                        for o in outs {
                            if let ActiveStageOutput::ResponseFrame(f) = o {
                                writer.write_all(&f).await.map_err(|e| e.to_string())?;
                            }
                        }
                    }
                }
                ActiveStageOutput::Terminate(reason) => {
                    let text = format!("session RDP terminée : {reason}");
                    return Ok(if user_ended(&reason) { ended(text) } else { text });
                }
                _ => {}
            }
        }

        // Frame acknowledgements (graphics pipeline): sent once the tab has drawn the frames, or
        // right away when nothing waits to be drawn (no tab, nothing changed on screen), so the
        // server never encodes pictures faster than they are shown (the Windows client delays
        // its acknowledgements the same way).
        if stage.get_dvc::<GraphicsPipelineClient>().is_some_and(|g| g.processor().held_frame_acks() > 0) {
            let idle = in_flight.is_none() && dirty.is_empty();
            let late = acks_held.is_some_and(|t| t.elapsed() > ACK_HOLD_MAX);
            if viewer.is_none() || !paced || tab_drew || idle || late {
                if let Some(f) = take_frame_acks(&mut stage)? {
                    writer.write_all(&f).await.map_err(|e| e.to_string())?;
                }
                acks_held = None;
            } else if acks_held.is_none() {
                acks_held = Some(std::time::Instant::now());
            }
        }
        tab_drew = false;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn multi_monitor_layout() {
        use ironrdp_pdu::gcc::MonitorFlags;
        assert!(super::monitor_layout(&[]).is_none());
        // a 1280x1024 monitor on the left of the 1920x1080 primary, lower by 100 px
        let (w, h, m) = super::monitor_layout(&[(-1280, 100, 1280, 1024, false), (0, 0, 1920, 1080, true)]).unwrap();
        assert_eq!((w, h), (3200, 1124));
        assert_eq!((m[0].left, m[0].top, m[0].right, m[0].bottom), (-1280, 100, -1, 1123));
        assert_eq!((m[1].left, m[1].top, m[1].right, m[1].bottom), (0, 0, 1919, 1079));
        assert!(m[1].flags.contains(MonitorFlags::PRIMARY) && !m[0].flags.contains(MonitorFlags::PRIMARY));
        // no primary reported: the first monitor becomes the primary, at (0, 0)
        let (_, _, m) = super::monitor_layout(&[(1920, 0, 1920, 1080, false), (3840, 0, 1920, 1080, false)]).unwrap();
        assert_eq!((m[0].left, m[1].left), (0, 1920));
        assert!(m[0].flags.contains(MonitorFlags::PRIMARY));
    }

    use super::*;

    #[test]
    fn readable_errors() {
        assert_eq!(
            super::readable(r"[write all @ C:\x\ironrdp-async-0.12.0\src\framed.rs:314] custom error"),
            "la connexion a été coupée pendant l'ouverture de la session"
        );
        assert_eq!(super::readable("[read frame @ a.rs:1] connection reset"), "connection reset");
        assert_eq!(super::readable("licence [MS-RDPELE] refusée"), "licence [MS-RDPELE] refusée");
    }

    #[test]
    fn slow_link_profile() {
        let ms = |n| Some(Duration::from_millis(n));
        assert!(!slow_link(Quality::Auto, ms(2)));
        assert!(!slow_link(Quality::Auto, ms(25))); // home Wi-Fi
        assert!(slow_link(Quality::Auto, ms(60)));
        assert!(slow_link(Quality::Auto, None)); // RD gateway
        assert!(!slow_link(Quality::Lan, ms(200)));
        assert!(slow_link(Quality::Slow, ms(1)));
        assert_eq!(Quality::parse("slow"), Quality::Slow);
        assert_eq!(Quality::parse(""), Quality::Auto);
    }

    #[test]
    fn dirty_areas() {
        let rect = |l, t, r, b| InclusiveRectangle { left: l, top: t, right: r, bottom: b };
        let mut d = Dirty::default();
        assert!(d.is_empty());
        // a clock in a corner and a cursor far away stay two small areas
        d.add(&rect(1800, 0, 1900, 20));
        d.add(&rect(100, 900, 110, 920));
        assert_eq!(d.0.len(), 2);
        // tiles of the same window merge into one area
        for x in 0..10u16 {
            d.add(&rect(400 + x * 64, 300, 463 + x * 64, 363));
        }
        assert_eq!(d.0.len(), 3);
        assert!(d.0.contains(&(400, 300, 1039, 363)));
        // an area that covers the others swallows them
        d.add(&rect(0, 0, 1919, 1079));
        assert_eq!(d.take(), vec![(0, 0, 1919, 1079)]);
        assert!(d.is_empty());
        // many scattered areas fall back to their bounding box
        for i in 0..30u16 {
            d.add(&rect(i * 300 % 1900, i * 400 % 1000, i * 300 % 1900 + 3, i * 400 % 1000 + 3));
        }
        assert!(d.0.len() <= Dirty::MAX);
        d.all(800, 600);
        assert_eq!(d.take(), vec![(0, 0, 799, 599)]);
    }

    #[test]
    fn rect_is_opaque() {
        let image = DecodedImage::new(PixelFormat::RgbA32, 4, 2);
        let m = rect_message(&image, (1, 0, 2, 1)).unwrap();
        assert_eq!(&m[..9], &[1, 1, 0, 0, 0, 2, 0, 2, 0]);
        assert_eq!(m.len(), 9 + 2 * 2 * 4);
        assert!(m[9..].chunks(4).all(|p| p[3] == 0xff));
        assert!(rect_message(&image, (4, 0, 5, 1)).is_none());
    }

    #[test]
    fn certificates_tofu() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("rdp_known_hosts");
        let fp = fingerprint(b"public key");
        assert!(fp.starts_with("SHA256:"));
        assert_eq!(cert_check(&p, "Srv", 3389, &fp), HostKeyStatus::Unknown);
        cert_learn(&p, "Srv", 3389, &fp).unwrap();
        assert_eq!(cert_check(&p, "srv", 3389, &fp), HostKeyStatus::Known);
        assert_eq!(cert_check(&p, "srv", 3390, &fp), HostKeyStatus::Unknown);
        let other = fingerprint(b"other key");
        assert_eq!(cert_check(&p, "srv", 3389, &other), HostKeyStatus::Changed);
        cert_learn(&p, "srv", 3389, &other).unwrap();
        assert_eq!(cert_check(&p, "srv", 3389, &other), HostKeyStatus::Known);
        assert_eq!(std::fs::read_to_string(&p).unwrap().lines().count(), 1);
    }

    #[test]
    fn input_mapping() {
        let k = |json: &str| input_events(serde_json::from_str(json).unwrap());
        match k(r#"{"t":"k","sc":57427,"up":true}"#).as_slice() {
            [FastPathInputEvent::KeyboardEvent(f, 0x53)] => {
                assert!(f.contains(KeyboardFlags::EXTENDED) && f.contains(KeyboardFlags::RELEASE))
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(k(r#"{"t":"cad"}"#).len(), 6);
        // text: Enter becomes a real key, other characters Unicode events (press + release)
        let ev = k(r#"{"t":"u","s":"é\r\n"}"#);
        assert_eq!(ev.len(), 4);
        assert!(matches!(ev[0], FastPathInputEvent::UnicodeKeyboardEvent(_, 0xe9)));
        assert!(matches!(ev[2], FastPathInputEvent::KeyboardEvent(_, 0x1c)));
        match k(r#"{"t":"w","d":-120,"x":1,"y":2}"#).as_slice() {
            [FastPathInputEvent::MouseEvent(m)] => {
                assert!(m.flags.contains(PointerFlags::VERTICAL_WHEEL));
                assert_eq!(m.number_of_wheel_rotation_units, -120);
            }
            other => panic!("{other:?}"),
        }
    }
}