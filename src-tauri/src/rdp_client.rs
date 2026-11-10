//! Embedded RDP client (IronRDP): the remote desktop is drawn inside the Orbiterm tab.
//!
//! The RDP connection (TCP → TLS → CredSSP/NLA → active session) runs in the backend.
//! The tab's canvas talks to it through a WebSocket bound to 127.0.0.1 with a random token
//! (like the VNC bridge):
//! - backend → tab (binary): `1` rect (x, y, w, h: u16 LE, then RGBA), `2` desktop size (w, h),
//!   `3` cursor (hot x, hot y, w, h, RGBA), `4` default cursor, `5` hidden cursor;
//! - backend → tab (JSON text): `{"t":"clip","s":…}` text copied on the remote computer;
//! - tab → backend (JSON text): keyboard scancodes, mouse, wheel, resize, Ctrl+Alt+Del,
//!   local clipboard text.
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
use crate::ssh::{CloseSink, HostKeyPrompter, HostKeyQuestion};

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
    #[serde(rename = "r")]
    Resize { w: u16, h: u16 },
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

fn connector_config(p: &RdpParams) -> AppResult<ironrdp_connector::Config> {
    let codecs = client_codecs_capabilities(&[]).map_err(|e| AppError::Conn(format!("codecs RDP : {e}")))?;
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
        desktop_size: DesktopSize { width: p.width, height: p.height },
        desktop_scale_factor: 0,
        monitor_layout: None,
        desktop_physical_size: None,
        desktop_orientation: None,
        device_scale_factor: None,
        monitor_extended_layout: None,
        relative_mouse_input: false,
        enable_standard_rdp_security: false,
        connection_type: ironrdp_pdu::gcc::ConnectionType::Lan,
        remote_application_mode: false,
        rail_support_level: ironrdp_pdu::rdp::capability_sets::RailSupportLevel::empty(),
        support_dyn_vc_gfx_protocol: false,
        enable_audio_capture: false,
        bitmap: Some(BitmapConfig { color_depth: 32, lossy_compression: true, codecs }),
        client_build: 0,
        client_name: client_name(),
        client_dir: String::new(),
        platform: if cfg!(windows) { MajorPlatformType::WINDOWS } else { MajorPlatformType::UNIX },
        hardware_id: None,
        license_cache: None,
        enable_server_pointer: true,
        // without NLA (TLS only), log on with the same credentials instead of the logon screen
        autologon: !p.password.is_empty(),
        enable_audio_playback: false,
        request_data: None,
        pointer_software_rendering: false,
        multitransport_flags: None,
        // no bulk compression: xrdp then compresses even the share-data PDUs of the
        // Deactivation-Reactivation sequence, which IronRDP cannot decode there (the
        // session dropped right after the logon on Kali / xrdp)
        compression_type: None,
        performance_flags: PerformanceFlags::default(),
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
        _ => AppError::Conn(format!("RDP : {msg}")),
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
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ready_tx.send(Err(AppError::Conn(e.to_string())));
                    return;
                }
            };
            rt.block_on(async move {
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
                let reason = match active_session(framed, result, channels, loop_tx, cmd_rx).await {
                    Ok(r) => r,
                    Err(e) => format!("connexion RDP interrompue : {e}"),
                };
                on_close(reason);
            });
        })
        .map_err(|e| AppError::Conn(e.to_string()))?;
    let (port, token, desktop) = ready_rx.await.map_err(|_| AppError::Conn("session RDP interrompue".into()))??;
    Ok(RdpSession { port, token, cmd: cmd_tx, desktop })
}

struct Channels {
    clip: Option<mpsc::UnboundedReceiver<ClipEvent>>,
    drive: bool,
}

async fn establish(
    p: RdpParams,
    prompter: HostKeyPrompter,
) -> AppResult<(Framed, ironrdp_connector::ConnectionResult, Channels)> {
    if p.username.trim().is_empty() {
        return Err(AppError::Config("RDP : indique le nom d'utilisateur dans les paramètres de la session".into()));
    }
    let stream = tokio::time::timeout(Duration::from_secs(15), TcpStream::connect((p.host.as_str(), p.port)))
        .await
        .map_err(|_| AppError::Conn(format!("{}:{} ne répond pas", p.host, p.port)))?
        .map_err(|e| AppError::Conn(format!("{}:{} : {e}", p.host, p.port)))?;
    let _ = stream.set_nodelay(true);
    let client_addr: SocketAddr = stream.local_addr()?;
    let mut framed = ironrdp_tokio::TokioFramed::new(stream);
    let mut connector = ClientConnector::new(connector_config(&p)?, client_addr).with_static_channel(
        DrdynvcClient::new().with_dynamic_channel(DisplayControlClient::new(|_| Ok(Vec::new()))),
    );
    let mut channels = Channels { clip: None, drive: false };
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

    let should_upgrade = ironrdp_tokio::connect_begin(&mut framed, &mut connector).await.map_err(map_connect_err)?;
    let (initial, leftover) = framed.into_inner();
    let (tls, cert) = ironrdp_tls::upgrade(initial, &p.host)
        .await
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

#[derive(Default, Clone, Copy)]
struct Dirty(Option<(u16, u16, u16, u16)>); // inclusive left, top, right, bottom

impl Dirty {
    fn add(&mut self, r: &InclusiveRectangle) {
        self.0 = Some(match self.0 {
            None => (r.left, r.top, r.right, r.bottom),
            Some((l, t, rr, b)) => (l.min(r.left), t.min(r.top), rr.max(r.right), b.max(r.bottom)),
        });
    }
    fn all(&mut self, w: u16, h: u16) {
        if w > 0 && h > 0 {
            self.0 = Some((0, 0, w - 1, h - 1));
        }
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
        let row = &data[start..start + usize::from(w) * 4];
        for px in row.as_chunks::<4>().0 {
            out.extend_from_slice(&[px[0], px[1], px[2], 0xff]);
        }
    }
    Some(out)
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
        WsIn::Resize { .. } | WsIn::Clip { .. } => vec![],
    }
}

async fn send_text(sink: &mut Option<Sink>, text: String) {
    if let Some(s) = sink
        && s.send(Message::text(text)).await.is_err()
    {
        *sink = None;
    }
}

async fn send(sink: &mut Option<Sink>, data: Vec<u8>) {
    if let Some(s) = sink
        && s.send(Message::binary(data)).await.is_err()
    {
        *sink = None;
    }
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

async fn active_session(
    framed: Framed,
    result: ironrdp_connector::ConnectionResult,
    channels: Channels,
    cmd_tx: mpsc::UnboundedSender<Cmd>,
    mut cmd_rx: mpsc::UnboundedReceiver<Cmd>,
) -> Result<String, String> {
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

    let mut sink: Option<Sink> = None;
    let mut dirty = Dirty::default();
    let mut flush = tokio::time::interval(Duration::from_millis(16));
    flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut pending_resize: Option<(u16, u16, tokio::time::Instant)> = None;
    let mut last_pointer: Option<Vec<u8>> = None;
    let Channels { clip: mut clip_rx, drive } = channels;
    // clipboard: our text (CRLF, as sent to the server), last text received from the server
    let mut local_clip: Option<String> = None;
    let mut remote_clip: Option<String> = None;

    loop {
        let outputs = tokio::select! {
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
                            send_text(&mut sink, m).await;
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
                let (action, payload) = frame.map_err(|e| e.to_string())?;
                stage.process(&mut image, action, &payload, None).map_err(|e| e.to_string())?
            }
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
                    let (mut s, mut stream) = (*ws).split();
                    let _ = s.send(Message::binary(size_message(image.width(), image.height()))).await;
                    if let Some(p) = &last_pointer {
                        let _ = s.send(Message::binary(p.clone())).await;
                    }
                    sink = Some(s);
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
                Some(Cmd::In(WsIn::Resize { w, h })) => {
                    // debounced: applied 400 ms after the last resize of the tab
                    if w >= 200 && h >= 200 && (w, h) != (image.width(), image.height()) {
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
                Some(Cmd::In(m)) => {
                    let events = input_events(m);
                    if events.is_empty() {
                        Vec::new()
                    } else {
                        stage.process_fastpath_input(&mut image, &events).map_err(|e| e.to_string())?
                    }
                }
            },
            _ = flush.tick() => {
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
                    match stage.encode_resize(w, h, None, None) {
                        Some(Ok(frame)) => vec![ActiveStageOutput::ResponseFrame(frame)],
                        Some(Err(e)) => return Err(e.to_string()),
                        None => Vec::new(), // server without dynamic resolution: keep the size, the tab scales
                    }
                } else if let Some(r) = dirty.0.take() {
                    if let Some(m) = rect_message(&image, r) {
                        send(&mut sink, m).await;
                    }
                    Vec::new()
                } else {
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
                    send(&mut sink, vec![4]).await;
                }
                ActiveStageOutput::PointerHidden => {
                    last_pointer = Some(vec![5]);
                    send(&mut sink, vec![5]).await;
                }
                ActiveStageOutput::PointerPosition { .. } => {}
                ActiveStageOutput::PointerBitmap(p) => {
                    let mut m = vec![3u8];
                    for v in [p.hotspot_x, p.hotspot_y, p.width, p.height] {
                        m.extend_from_slice(&v.to_le_bytes());
                    }
                    m.extend_from_slice(&p.bitmap_data);
                    last_pointer = Some(m.clone());
                    send(&mut sink, m).await;
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
                            send(&mut sink, size_message(desktop_size.width, desktop_size.height)).await;
                            dirty.0 = None;
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
                ActiveStageOutput::Terminate(reason) => return Ok(format!("session RDP terminée : {reason}")),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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