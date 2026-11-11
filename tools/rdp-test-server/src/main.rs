//! Minimal RDP server (IronRDP) to test Orbiterm's embedded RDP client without a Windows host.
//!
//! - TLS + NLA (CredSSP), user `test` / password `test`, self-signed certificate (new at each start)
//! - 1024x768 desktop: four colour bars on top; the key `a` paints the bottom half yellow,
//!   `b` cyan; a left click draws a white square where it happened
//! - logs `auth:ok`-like events on stdout: `key:0x1e`, `click:x,y`, `wheel:n`
//! - sound: a 1.5 s 440 Hz tone once the client accepts audio (`son:<bytes>` logged)
//! - optional Remote Desktop Gateway on another port (`--gateway <port>`, `gw` / `gwpass`)
//!
//! `cargo run --example rdp_test_server -- [port]` (default 3390).

#![allow(dead_code)]

mod gateway;

use std::net::SocketAddr;
use std::num::{NonZeroU16, NonZeroUsize};
use std::sync::Arc;

use ironrdp_server::tokio_rustls::rustls;
use ironrdp_server::tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use ironrdp_server::tokio_rustls::TlsAcceptor;
use ironrdp_server::{
    BitmapUpdate, Credentials, DesktopSize, DisplayUpdate, KeyboardEvent, MouseEvent, PixelFormat, RdpServer,
    RdpServerDisplay, RdpServerDisplayUpdates, RdpServerInputHandler, RdpsndServerHandler, RdpsndServerMessage, ServerEvent,
    ServerEventSender, SoundServerFactory,
};
use tokio::sync::mpsc;

pub const W: u16 = 1024;
pub const H: u16 = 768;

/// Solid BGRA rectangle update.
fn fill(x: u16, y: u16, w: u16, h: u16, rgb: (u8, u8, u8)) -> DisplayUpdate {
    let mut data = Vec::with_capacity(usize::from(w) * usize::from(h) * 4);
    for _ in 0..(usize::from(w) * usize::from(h)) {
        data.extend_from_slice(&[rgb.2, rgb.1, rgb.0, 0xff]);
    }
    DisplayUpdate::Bitmap(BitmapUpdate {
        x,
        y,
        width: NonZeroU16::new(w).unwrap(),
        height: NonZeroU16::new(h).unwrap(),
        format: PixelFormat::BgrA32,
        data: data.into(),
        stride: NonZeroUsize::new(usize::from(w) * 4).unwrap(),
    })
}

struct Display {
    rx: Option<mpsc::UnboundedReceiver<DisplayUpdate>>,
}

struct Updates {
    initial: Vec<DisplayUpdate>,
    rx: mpsc::UnboundedReceiver<DisplayUpdate>,
}

#[async_trait::async_trait]
impl RdpServerDisplayUpdates for Updates {
    async fn next_update(&mut self) -> anyhow::Result<Option<DisplayUpdate>> {
        if !self.initial.is_empty() {
            return Ok(Some(self.initial.remove(0)));
        }
        Ok(self.rx.recv().await)
    }
}

#[async_trait::async_trait]
impl RdpServerDisplay for Display {
    async fn size(&mut self) -> DesktopSize {
        DesktopSize { width: W, height: H }
    }

    async fn updates(&mut self) -> anyhow::Result<Box<dyn RdpServerDisplayUpdates>> {
        let colours = [(220, 30, 30), (30, 180, 60), (40, 80, 220), (240, 240, 240)];
        let bar = W / 4;
        let mut initial: Vec<DisplayUpdate> =
            colours.iter().enumerate().map(|(i, c)| fill(i as u16 * bar, 0, bar, H / 2, *c)).collect();
        initial.push(fill(0, H / 2, W, H / 2, (60, 60, 60)));
        let rx = self.rx.take().unwrap_or_else(|| mpsc::unbounded_channel().1);
        Ok(Box::new(Updates { initial, rx }))
    }
}

struct Input {
    tx: mpsc::UnboundedSender<DisplayUpdate>,
    log: mpsc::UnboundedSender<String>,
    pos: (u16, u16),
}

impl RdpServerInputHandler for Input {
    fn keyboard(&mut self, event: KeyboardEvent) {
        if let KeyboardEvent::Pressed { code, extended } = event {
            let _ = self.log.send(format!("key:0x{code:02x}{}", if extended { " ext" } else { "" }));
            match code {
                0x1e => {
                    let _ = self.tx.send(fill(0, H / 2, W, H / 2, (240, 200, 0)));
                }
                0x30 => {
                    let _ = self.tx.send(fill(0, H / 2, W, H / 2, (0, 200, 220)));
                }
                _ => {}
            }
        }
    }

    fn mouse(&mut self, event: MouseEvent) {
        match event {
            MouseEvent::Move { x, y } => self.pos = (x, y),
            MouseEvent::LeftPressed => {
                let (x, y) = self.pos;
                let _ = self.log.send(format!("click:{x},{y}"));
                let (sx, sy) = (x.saturating_sub(8).min(W - 16), y.saturating_sub(8).min(H - 16));
                let _ = self.tx.send(fill(sx, sy, 16, 16, (255, 255, 255)));
            }
            MouseEvent::VerticalScroll { value } => {
                let _ = self.log.send(format!("wheel:{value}"));
            }
            _ => {}
        }
    }
}

fn tls() -> anyhow::Result<(TlsAcceptor, Vec<u8>)> {
    let ck = rcgen::generate_simple_self_signed(vec!["localhost".into(), "127.0.0.1".into()])?;
    let pub_key = ck.signing_key.public_key_raw().to_vec();
    let cert = CertificateDer::from(ck.cert.der().to_vec());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(ck.signing_key.serialize_der()));
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let cfg = rustls::ServerConfig::builder().with_no_client_auth().with_single_cert(vec![cert], key)?;
    Ok((TlsAcceptor::from(Arc::new(cfg)), pub_key))
}

/// Serves RDP connections one after another on `addr`; events are sent to `log`.
pub async fn serve(addr: SocketAddr, log: mpsc::UnboundedSender<String>) -> anyhow::Result<()> {
    let (acceptor, pub_key) = tls()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    loop {
        let (stream, _) = listener.accept().await?;
        let (tx, rx) = mpsc::unbounded_channel();
        let mut server = RdpServer::builder()
            .with_addr(addr)
            .with_hybrid(acceptor.clone(), pub_key.clone())
            .with_input_handler(Input { tx, log: log.clone(), pos: (0, 0) })
            .with_display_handler(Display { rx: Some(rx) })
            .with_sound_factory(Some(Box::new(Sound { sender: None, log: Some(log.clone()) })))
            .build();
        server.set_credentials(Some(Credentials { username: "test".into(), password: "test".into(), domain: None }));
        let _ = log.send("connexion".into());
        match server.run_connection(stream).await {
            Ok(()) => {
                let _ = log.send("fin".into());
            }
            Err(e) => {
                let _ = log.send(format!("erreur:{e:#}"));
            }
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port: u16 = std::env::args().nth(1).and_then(|p| p.parse().ok()).unwrap_or(3390);
    let (log, mut rx) = mpsc::unbounded_channel();
    println!("Serveur RDP de test prêt sur 127.0.0.1:{port}  (utilisateur test, mot de passe test)");
    tokio::spawn(async move {
        while let Some(l) = rx.recv().await {
            println!("{l}");
        }
    });
    let args: Vec<String> = std::env::args().collect();
    if let Some(gw) = args.iter().position(|a| a == "--gateway").and_then(|i| args.get(i + 1)).and_then(|p| p.parse::<u16>().ok()) {
        println!("Passerelle RD de test sur 127.0.0.1:{gw}  (utilisateur {}, mot de passe {})", gateway::USER, gateway::PASSWORD);
        let log = log.clone();
        tokio::spawn(async move {
            if let Err(e) = gateway::serve(SocketAddr::from(([127, 0, 0, 1], gw)), log.clone()).await {
                let _ = log.send(format!("gw:erreur {e:#}"));
            }
        });
    }
    serve(SocketAddr::from(([127, 0, 0, 1], port)), log).await
}

// ------------------------------------------------------------------ sound

/// Plays a 1.5 s 440 Hz tone (PCM 44.1 kHz stereo) as soon as the client accepts the
/// sound channel; logs `son:<octets>` when done.
#[derive(Debug, Default)]
struct Sound {
    sender: Option<mpsc::UnboundedSender<ServerEvent>>,
    log: Option<mpsc::UnboundedSender<String>>,
}

impl ServerEventSender for Sound {
    fn set_sender(&mut self, sender: mpsc::UnboundedSender<ServerEvent>) {
        self.sender = Some(sender);
    }
}

impl SoundServerFactory for Sound {
    fn build_backend(&self) -> Box<dyn RdpsndServerHandler> {
        Box::new(Tone { sender: self.sender.clone(), log: self.log.clone(), formats: vec![pcm44()] })
    }
}

fn pcm44() -> ironrdp_rdpsnd::pdu::AudioFormat {
    ironrdp_rdpsnd::pdu::AudioFormat {
        format: ironrdp_rdpsnd::pdu::WaveFormat::PCM,
        n_channels: 2,
        n_samples_per_sec: 44_100,
        n_avg_bytes_per_sec: 44_100 * 4,
        n_block_align: 4,
        bits_per_sample: 16,
        data: None,
    }
}

#[derive(Debug)]
struct Tone {
    sender: Option<mpsc::UnboundedSender<ServerEvent>>,
    log: Option<mpsc::UnboundedSender<String>>,
    formats: Vec<ironrdp_rdpsnd::pdu::AudioFormat>,
}

impl RdpsndServerHandler for Tone {
    fn get_formats(&self) -> &[ironrdp_rdpsnd::pdu::AudioFormat] {
        &self.formats
    }

    fn choose_format<'a>(
        &mut self,
        common: &'a [ironrdp_rdpsnd::server::NegotiatedFormat],
    ) -> Option<&'a ironrdp_rdpsnd::server::NegotiatedFormat> {
        common.first()
    }

    fn start(
        &mut self,
        _format: &ironrdp_rdpsnd::server::NegotiatedFormat,
    ) -> Result<(), Box<dyn ironrdp_rdpsnd::server::RdpsndError>> {
        let (Some(tx), log) = (self.sender.clone(), self.log.clone()) else { return Ok(()) };
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let (rate, block_ms) = (44_100u32, 50u32);
            let frames = rate * block_ms / 1000;
            let mut sent = 0usize;
            for b in 0..30u32 {
                let mut data = Vec::with_capacity(frames as usize * 4);
                for i in 0..frames {
                    let t = f64::from(b * frames + i) / f64::from(rate);
                    let v = ((t * 440.0 * std::f64::consts::TAU).sin() * 8000.0) as i16;
                    data.extend_from_slice(&v.to_le_bytes());
                    data.extend_from_slice(&v.to_le_bytes());
                }
                sent += data.len();
                if tx.send(ServerEvent::Rdpsnd(RdpsndServerMessage::Wave(data, b * block_ms))).is_err() {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(u64::from(block_ms))).await;
            }
            if let Some(l) = log {
                let _ = l.send(format!("son:{sent}"));
            }
        });
        Ok(())
    }

    fn stop(&mut self) {}
}
