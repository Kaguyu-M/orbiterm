//! UDP transport of the remote desktop (multitransport), as the Windows client uses it.
//!
//! The server offers it once the connection is set up (Initiate Multitransport Request, on
//! TCP): the client opens reliable RDP-UDP (MS-RDPEUDP, version 2) to the same address and port, secures
//! it with TLS, then proves with the request's cookie that it is the same client (MS-RDPEMT
//! tunnel). With Soft-Sync (MS-RDPEDYC 3.1.5.1.6) the server then moves the dynamic channels,
//! the graphics pipeline first, onto the tunnel; everything else stays on TCP.
//!
//! Why: on a link that loses packets (VPN, Wi-Fi), TCP holds back everything behind a lost
//! packet for at least one round trip, so the whole picture freezes; RDP-UDP recovers the
//! losses of each stream on its own. Any failure leaves the session on TCP, as before.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use ironrdp_pdu::rdp::multitransport::{MultitransportRequestPdu, MultitransportResponsePdu, RequestedProtocol};
use ironrdp_rdpemt::TunnelConfig;
use ironrdp_rdpeudp_tokio::{connect_udp, UdpTlsConfig, UdpTransport, UdpTransportConfig};
use ironrdp_svc::SvcMessage;
use x509_cert::der::Decode as _;

/// Diagnostics: ORBITERM_RDP_NOUDP=1 keeps every session on TCP.
pub fn allowed() -> bool {
    std::env::var_os("ORBITERM_RDP_NOUDP").is_none()
}

/// The UDP channel must open quickly: the end of the logon waits for it, and a network that
/// drops UDP (firewall) must not delay the session by more than this.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(3);
const TLS_TIMEOUT: Duration = Duration::from_secs(5);
const TUNNEL_TIMEOUT: Duration = Duration::from_secs(5);

/// Whether the certificate `der` holds the same public key as the one checked on the TCP
/// connection (`fingerprint`, see [crate::rdp_client::fingerprint]): the UDP channel ends at the
/// same server, not at someone in between.
pub fn same_key(der: &[u8], fingerprint: &str) -> bool {
    let Ok(cert) = x509_cert::Certificate::from_der(der) else {
        return false;
    };
    ironrdp_tls::extract_tls_server_public_key(&cert)
        .is_some_and(|key| crate::rdp_client::fingerprint(key) == fingerprint)
}

/// Answers one Initiate Multitransport Request: opens the UDP transport when it is the reliable
/// one and Soft-Sync was agreed (without it the server would expect the channels to be created
/// anew on the tunnel, which is not supported). The response goes back on TCP; `None` leaves the
/// session on TCP.
pub async fn open(
    request: &MultitransportRequestPdu,
    soft_sync: bool,
    peer: SocketAddr,
    host: &str,
    fingerprint: &str,
) -> Option<UdpTransport> {
    if request.requested_protocol != RequestedProtocol::UdpFecR || !soft_sync {
        log::info!(
            "RDP : transport UDP {:?} non pris en charge (Soft-Sync : {soft_sync}), la session reste en TCP",
            request.requested_protocol
        );
        return None;
    }
    let mut config = UdpTransportConfig::new(
        peer,
        host.to_string(),
        TunnelConfig { request_id: request.request_id, security_cookie: request.security_cookie },
    );
    // version 2 (MS-RDPEUDP framing): what Windows answers; offered version 3 (MS-RDPEUDP2), a
    // Windows 11 host dropped every SYN without a word ("No UDP Packets Received" on its side)
    config.connection_config.offer_version = ironrdp_rdpeudp::pdu::UdpVersion::V2;
    // receive window: 1024 datagrams (1.2 MB) in flight. The default 64 caps the server near
    // 1.5 MB/s at a 50 ms round trip (VPN), far below what a window drag or a video sends
    config.connection_config.log_window_size = 10;
    // random first sequence number, as the specification asks (2.2.2.5)
    config.connection_config.initial_sequence_number = rand::random();
    config.handshake_timeout = HANDSHAKE_TIMEOUT;
    config.tls_timeout = TLS_TIMEOUT;
    config.tunnel_timeout = TUNNEL_TIMEOUT;
    // the callback decides whenever the system does not trust the certificate (the usual
    // self-signed RDP certificate): same key as on TCP, or no UDP
    let expected = fingerprint.to_string();
    let mut tls = UdpTlsConfig::new(host.to_string());
    tls.certificate_validation = ironrdp_tls::CertificateValidation::Strict;
    tls.certificate_validation_callback = Some(Arc::new(move |der: &[u8], _: &str, _: &str| same_key(der, &expected)));
    config.tls = tls;
    let t0 = std::time::Instant::now();
    match connect_udp(config).await {
        Ok(t) => {
            log::info!("RDP : transport UDP ouvert vers {peer} en {:?}", t0.elapsed());
            Some(t)
        }
        Err(e) => {
            log::info!("RDP : transport UDP impossible vers {peer} ({e}), la session reste en TCP");
            None
        }
    }
}

/// The response to send on TCP for a request answered by [open].
pub fn result(opened: bool) -> ironrdp_connector::MultitransportResult {
    if opened {
        ironrdp_connector::MultitransportResult::Success
    } else {
        ironrdp_connector::MultitransportResult::Failure(MultitransportResponsePdu::E_ABORT)
    }
}

/// Network measurements the server makes through the tunnel (MS-RDPBCGR 2.2.14, carried in
/// tunnel sub-headers, MS-RDPEMT 2.2.1.1.1): round trips answered at once, bandwidth measured
/// over the tunnel data received between Start and Stop. Unanswered, the server cannot know the
/// link and Windows sent nothing else (an empty message with only a sub-header came first).
#[derive(Default)]
pub struct Meter {
    started: Option<std::time::Instant>,
    bytes: u64,
}

use ironrdp_pdu::rdp::autodetect::{AutoDetectRequest, AutoDetectResponse};
use ironrdp_rdpemt::{SubHeaderType, TunnelSubHeader};

impl Meter {
    /// Handles one tunnel message: returns the sub-headers to send back, and the server's own
    /// result when it sent one (round trip in ms, bandwidth in kbit/s).
    pub fn on_message(&mut self, m: &ironrdp_rdpeudp_tokio::TunnelMessage) -> (Vec<TunnelSubHeader>, Option<(u32, u32)>) {
        if self.started.is_some() {
            self.bytes += m.data.len() as u64;
        }
        let mut replies = Vec::new();
        let mut result = None;
        for sh in m.sub_headers.iter().filter(|s| s.sub_header_type == SubHeaderType::AutoDetectRequest) {
            // the sub-header's length and type bytes are the request's own first two bytes
            let mut wire = vec![(sh.data.len() + 2) as u8, ironrdp_pdu::rdp::autodetect::TYPE_ID_AUTODETECT_REQUEST];
            wire.extend_from_slice(&sh.data);
            let req = match ironrdp_core::decode::<AutoDetectRequest>(&wire) {
                Ok(r) => r,
                Err(e) => {
                    log::debug!("RDP : mesure réseau UDP illisible ({e})");
                    continue;
                }
            };
            let response = match req {
                AutoDetectRequest::RttRequest { sequence_number, .. } => Some(AutoDetectResponse::RttResponse { sequence_number }),
                AutoDetectRequest::BandwidthMeasureStart { .. } => {
                    self.started = Some(std::time::Instant::now());
                    self.bytes = 0;
                    None
                }
                AutoDetectRequest::BandwidthMeasurePayload { payload, .. } => {
                    self.bytes += payload.len() as u64;
                    None
                }
                AutoDetectRequest::BandwidthMeasureStop { sequence_number, request_type, payload } => {
                    self.bytes += payload.map_or(0, |p| p.len() as u64);
                    let elapsed = self.started.take().map_or(0, |t| t.elapsed().as_millis());
                    Some(AutoDetectResponse::BandwidthMeasureResults {
                        sequence_number,
                        response_type: if request_type == ironrdp_pdu::rdp::autodetect::BW_STOP_CONNECT_TIME {
                            ironrdp_pdu::rdp::autodetect::BW_RESULTS_CONNECT_TIME
                        } else {
                            ironrdp_pdu::rdp::autodetect::BW_RESULTS_CONTINUOUS
                        },
                        time_delta_ms: u32::try_from(elapsed).unwrap_or(u32::MAX),
                        byte_count: u32::try_from(self.bytes).unwrap_or(u32::MAX),
                    })
                }
                AutoDetectRequest::NetworkCharacteristicsResult { bandwidth_kbps, average_rtt_ms, .. } => {
                    result = Some((average_rtt_ms, bandwidth_kbps.unwrap_or(0)));
                    None
                }
            };
            if let Some(r) = response
                && let Ok(bytes) = ironrdp_core::encode_vec(&r)
            {
                replies.push(TunnelSubHeader { sub_header_type: SubHeaderType::AutoDetectResponse, data: bytes[2..].to_vec() });
            }
        }
        (replies, result)
    }
}

/// Where the messages of one dynamic channel go: Soft-Sync moved the channel onto the tunnel,
/// or not.
pub enum Out {
    Nothing,
    /// one frame for the TCP connection
    Tcp(Vec<u8>),
    /// DRDYNVC PDUs, each in its own tunnel message
    Udp(Vec<Vec<u8>>),
}

/// Encodes the messages of dynamic channel `channel_id` for the transport it uses.
pub fn route(
    stage: &mut ironrdp_session::ActiveStage,
    channel_id: u32,
    messages: Vec<SvcMessage>,
) -> Result<Out, String> {
    if messages.is_empty() {
        return Ok(Out::Nothing);
    }
    if stage.dvc_tunnel_for_channel(channel_id).is_some() {
        let pdus = messages.iter().map(|m| m.encode_unframed_pdu().map_err(|e| e.to_string())).collect::<Result<_, _>>()?;
        Ok(Out::Udp(pdus))
    } else {
        stage.encode_dvc_messages(messages).map(Out::Tcp).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ironrdp_rdpeudp_tokio::TunnelMessage;

    /// A request as the server puts it in a sub-header: without its length and type bytes.
    fn request(r: AutoDetectRequest) -> TunnelSubHeader {
        let wire = ironrdp_core::encode_vec(&r).unwrap();
        TunnelSubHeader { sub_header_type: SubHeaderType::AutoDetectRequest, data: wire[2..].to_vec() }
    }

    fn response(sh: &TunnelSubHeader) -> AutoDetectResponse {
        assert_eq!(sh.sub_header_type, SubHeaderType::AutoDetectResponse);
        let mut wire = vec![(sh.data.len() + 2) as u8, ironrdp_pdu::rdp::autodetect::TYPE_ID_AUTODETECT_RESPONSE];
        wire.extend_from_slice(&sh.data);
        ironrdp_core::decode(&wire).unwrap()
    }

    #[test]
    fn round_trip_requests_are_answered() {
        let mut m = Meter::default();
        let msg = TunnelMessage { sub_headers: vec![request(AutoDetectRequest::rtt_continuous(7))], data: Vec::new() };
        let (replies, result) = m.on_message(&msg);
        assert_eq!(replies.len(), 1);
        assert_eq!(response(&replies[0]), AutoDetectResponse::RttResponse { sequence_number: 7 });
        assert!(result.is_none());
    }

    #[test]
    fn bandwidth_counts_the_tunnel_data_between_start_and_stop() {
        let mut m = Meter::default();
        let start = TunnelMessage { sub_headers: vec![request(AutoDetectRequest::BandwidthMeasureStart { sequence_number: 1, request_type: ironrdp_pdu::rdp::autodetect::BW_START_RELIABLE_UDP })], data: Vec::new() };
        assert!(m.on_message(&start).0.is_empty());
        m.on_message(&TunnelMessage { sub_headers: Vec::new(), data: vec![0; 1000] });
        m.on_message(&TunnelMessage { sub_headers: Vec::new(), data: vec![0; 500] });
        let stop = TunnelMessage { sub_headers: vec![request(AutoDetectRequest::BandwidthMeasureStop { sequence_number: 2, request_type: ironrdp_pdu::rdp::autodetect::BW_STOP_RELIABLE_UDP, payload: None })], data: vec![0; 10] };
        let (replies, _) = m.on_message(&stop);
        match response(&replies[0]) {
            AutoDetectResponse::BandwidthMeasureResults { sequence_number, response_type, byte_count, .. } => {
                assert_eq!(sequence_number, 2);
                assert_eq!(response_type, ironrdp_pdu::rdp::autodetect::BW_RESULTS_CONTINUOUS);
                assert_eq!(byte_count, 1510);
            }
            other => panic!("{other:?}"),
        }
        // nothing counted any more after the stop
        m.on_message(&TunnelMessage { sub_headers: Vec::new(), data: vec![0; 99] });
        assert_eq!(m.bytes, 1510);
    }

    #[test]
    fn server_result_is_reported() {
        let mut m = Meter::default();
        let msg = TunnelMessage { sub_headers: vec![request(AutoDetectRequest::netchar_result(3, 20, 50_000, 42))], data: Vec::new() };
        let (replies, result) = m.on_message(&msg);
        assert!(replies.is_empty());
        assert_eq!(result, Some((42, 50_000)));
    }

    #[test]
    fn a_certificate_that_does_not_parse_is_refused() {
        assert!(!same_key(b"not a certificate", "aa:bb"));
    }
}