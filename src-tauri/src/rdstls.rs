//! RDSTLS ([MS-RDPBCGR] 2.2.17, 5.4.5.3): the authentication of a client that follows a
//! Server Redirection PDU. Over TLS, the client hands back the redirection GUID and the
//! password cookie of the redirection as it received them (the cookie is encrypted for the
//! target server, the client never sees the password).
//!
//! GNOME Remote Desktop's "remote login" (Ubuntu) uses it: the system service accepts the
//! connection, then redirects the client to the login screen, then to the user's session.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const VERSION_1: u16 = 0x0001;
const TYPE_CAPABILITIES: u16 = 0x0001;
const TYPE_AUTHREQ: u16 = 0x0002;
const TYPE_AUTHRSP: u16 = 0x0004;
const DATA_CAPABILITIES: u16 = 0x0001;
const DATA_PASSWORD_CREDS: u16 = 0x0001;
const DATA_RESULT_CODE: u16 = 0x0001;

/// What the client sends back to the target.
pub struct Credentials<'a> {
    pub redirection_guid: &'a [u8],
    pub username: &'a str,
    pub domain: &'a str,
    pub password: &'a [u8],
}

/// Unicode string with its terminating null, as FreeRDP (GNOME Remote Desktop) reads it.
fn put_string(out: &mut Vec<u8>, s: &str) {
    let units: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
    out.extend_from_slice(&((units.len() * 2) as u16).to_le_bytes());
    for u in units {
        out.extend_from_slice(&u.to_le_bytes());
    }
}

fn put_data(out: &mut Vec<u8>, d: &[u8]) {
    out.extend_from_slice(&(d.len() as u16).to_le_bytes());
    out.extend_from_slice(d);
}

/// RDSTLS Authentication Request PDU with Password Credentials (2.2.17.2).
pub fn authentication_request(c: &Credentials<'_>) -> Result<Vec<u8>, String> {
    for (what, len) in [("GUID", c.redirection_guid.len()), ("mot de passe", c.password.len())] {
        if len > usize::from(u16::MAX) {
            return Err(format!("RDSTLS : {what} de redirection trop long"));
        }
    }
    let mut out = Vec::new();
    for v in [VERSION_1, TYPE_AUTHREQ, DATA_PASSWORD_CREDS] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    put_data(&mut out, c.redirection_guid);
    put_string(&mut out, c.username);
    put_string(&mut out, c.domain);
    put_data(&mut out, c.password);
    Ok(out)
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

/// Checks the RDSTLS Capabilities PDU (2.2.17.1): the server must support version 1.
pub fn check_capabilities(b: &[u8; 8]) -> Result<(), String> {
    if u16_at(b, 2) != TYPE_CAPABILITIES || u16_at(b, 4) != DATA_CAPABILITIES {
        return Err(format!("RDSTLS : réponse inattendue du serveur ({:02x?})", b));
    }
    if u16_at(b, 6) & VERSION_1 == 0 {
        return Err(format!("RDSTLS : version non prise en charge par le serveur ({:#06x})", u16_at(b, 6)));
    }
    Ok(())
}

/// Result code of the RDSTLS Authentication Response PDU (2.2.17.4).
pub fn result_code(b: &[u8; 10]) -> Result<u32, String> {
    if u16_at(b, 2) != TYPE_AUTHRSP || u16_at(b, 4) != DATA_RESULT_CODE {
        return Err(format!("RDSTLS : réponse inattendue du serveur ({:02x?})", b));
    }
    Ok(u32::from_le_bytes([b[6], b[7], b[8], b[9]]))
}

/// The RDSTLS exchange, right after the TLS handshake of a redirected connection.
pub async fn authenticate<S: AsyncRead + AsyncWrite + Unpin>(s: &mut S, c: &Credentials<'_>) -> Result<(), String> {
    let mut caps = [0u8; 8];
    s.read_exact(&mut caps).await.map_err(|e| format!("RDSTLS : {e}"))?;
    check_capabilities(&caps)?;
    s.write_all(&authentication_request(c)?).await.map_err(|e| format!("RDSTLS : {e}"))?;
    s.flush().await.map_err(|e| format!("RDSTLS : {e}"))?;
    let mut rsp = [0u8; 10];
    s.read_exact(&mut rsp).await.map_err(|e| format!("RDSTLS : {e}"))?;
    match result_code(&rsp)? {
        0 => Ok(()),
        // ERROR_ACCESS_DENIED, ERROR_LOGON_FAILURE, ERROR_ACCOUNT_RESTRICTION
        code @ (0x5 | 0x52e | 0x52f) => Err(format!("RDSTLS : le serveur refuse la reconnexion (code {code:#x})")),
        code => Err(format!("RDSTLS : échec de l'authentification (code {code:#x})")),
    }
}

/// Server Redirection PDU read leniently from the user data of an MCS Send Data Indication,
/// for the servers IronRDP's decoder rejects: GNOME Remote Desktop (FreeRDP) writes the target
/// certificate in binary instead of Base64 text. The fields that a reconnection needs are kept,
/// the certificate and the address list are skipped.
pub fn lenient_redirection(user_data: &[u8]) -> Option<ServerRedirectionPdu> {
    let u16_le = |i: usize| user_data.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let start = if u16_le(2)? == 0 && u16_le(0)? & 0x0400 != 0 {
        // Basic Security Header (standard security)
        4
    } else if u16_le(2)? & 0x000F == 0x000A {
        // Share Control Header + pad2Octets (enhanced security)
        8
    } else {
        return None;
    };
    let mut r = Reader { b: user_data, at: start };
    if r.u16()? != 0x0400 {
        return None;
    }
    let _length = r.u16()?;
    let mut pdu = ServerRedirectionPdu { session_id: r.u32()?, ..Default::default() };
    let flags = ServerRedirectionFlags::from_bits_retain(r.u32()?);
    pdu.flags = flags;
    use ServerRedirectionFlags as F;
    if flags.contains(F::TARGET_NET_ADDRESS) {
        pdu.target_net_address = Some(utf16(r.field()?));
    }
    if flags.contains(F::LOAD_BALANCE_INFO) {
        pdu.load_balance_info = Some(r.field()?.to_vec());
    }
    if flags.contains(F::USERNAME) {
        pdu.username = Some(utf16(r.field()?));
    }
    if flags.contains(F::DOMAIN) {
        pdu.domain = Some(utf16(r.field()?));
    }
    if flags.contains(F::PASSWORD) {
        pdu.password = Some(r.field()?.to_vec());
    }
    // the fields after the GUID may be missing or unreadable: a reconnection does not need them
    fn optional<'a>(r: &mut Reader<'a>, flags: ServerRedirectionFlags, f: ServerRedirectionFlags) -> Option<&'a [u8]> {
        if flags.contains(f) { r.field() } else { Some(&[]) }
    }
    let fqdn = optional(&mut r, flags, F::TARGET_FQDN);
    let netbios = fqdn.and_then(|_| optional(&mut r, flags, F::TARGET_NETBIOS_NAME));
    let tsv = netbios.and_then(|_| optional(&mut r, flags, F::CLIENT_TSV_URL));
    if flags.contains(F::TARGET_FQDN) {
        pdu.target_fqdn = fqdn.map(utf16);
    }
    if flags.contains(F::REDIRECTION_GUID) {
        pdu.redirection_guid = Some(tsv.and_then(|_| r.field())?.to_vec());
    }
    Some(pdu)
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    /// length (32 bits) then bytes
    fn field(&mut self) -> Option<&'a [u8]> {
        let n = usize::try_from(self.u32()?).ok()?;
        self.take(n)
    }
}

fn utf16(b: &[u8]) -> String {
    let units: Vec<u16> = b.as_chunks::<2>().0.iter().map(|p| u16::from_le_bytes(*p)).collect();
    String::from_utf16_lossy(&units).trim_end_matches('\0').to_string()
}

use ironrdp_pdu::rdp::server_redirection::{ServerRedirectionFlags, ServerRedirectionPdu};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freerdp_redirection_with_binary_certificate() {
        let field = |out: &mut Vec<u8>, d: &[u8]| {
            out.extend_from_slice(&(d.len() as u32).to_le_bytes());
            out.extend_from_slice(d);
        };
        let wide = |s: &str| s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect::<Vec<u8>>();
        let flags = 0x2 | 0x4 | 0x10 | 0x4000 | 0x8000 | 0x1_0000u32;
        let mut p = vec![0x00, 0x04, 0, 0];
        p.extend_from_slice(&7u32.to_le_bytes());
        p.extend_from_slice(&flags.to_le_bytes());
        field(&mut p, b"Cookie: msts=abc\r\n");
        field(&mut p, &wide("Raph"));
        field(&mut p, &[9; 300]);
        field(&mut p, &wide("GUID"));
        // binary certificate container (FreeRDP), not Base64 text
        field(&mut p, &[32, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 0x30, 0x82]);
        let len = p.len() as u16;
        p[2..4].copy_from_slice(&len.to_le_bytes());
        let mut ud = vec![0, 0, 0x1A, 0, 0xEA, 0x03, 0, 0];
        ud.extend_from_slice(&p);

        let r = lenient_redirection(&ud).unwrap();
        assert_eq!(r.session_id, 7);
        assert_eq!(r.load_balance_info.as_deref(), Some(&b"Cookie: msts=abc\r\n"[..]));
        assert_eq!(r.username.as_deref(), Some("Raph"));
        assert_eq!(r.password.as_ref().map(Vec::len), Some(300));
        assert_eq!(r.redirection_guid, Some(wide("GUID")));
        assert!(r.flags.contains(ServerRedirectionFlags::PASSWORD_IS_PK_ENCRYPTED));
        assert!(lenient_redirection(&[0, 0, 0x17, 0, 0, 0]).is_none());
    }

    #[test]
    fn request_layout() {
        let c = Credentials { redirection_guid: b"GU", username: "Raph", domain: "", password: &[1, 2, 3] };
        let r = authentication_request(&c).unwrap();
        #[rustfmt::skip]
        let want: Vec<u8> = vec![
            1, 0, 2, 0, 1, 0,                         // version, AUTHREQ, PASSWORD_CREDS
            2, 0, b'G', b'U',                         // redirection GUID
            10, 0, b'R', 0, b'a', 0, b'p', 0, b'h', 0, 0, 0, // "Raph\0"
            2, 0, 0, 0,                               // empty domain "\0"
            3, 0, 1, 2, 3,                            // password cookie
        ];
        assert_eq!(r, want);
    }

    #[test]
    fn server_pdus() {
        assert!(check_capabilities(&[1, 0, 1, 0, 1, 0, 1, 0]).is_ok());
        assert!(check_capabilities(&[1, 0, 2, 0, 1, 0, 1, 0]).is_err());
        assert_eq!(result_code(&[1, 0, 4, 0, 1, 0, 0, 0, 0, 0]), Ok(0));
        assert_eq!(result_code(&[1, 0, 4, 0, 1, 0, 0x2e, 5, 0, 0]), Ok(0x52e));
    }

    #[tokio::test]
    async fn exchange() {
        let (mut client, mut server) = tokio::io::duplex(256);
        let srv = tokio::spawn(async move {
            server.write_all(&[1, 0, 1, 0, 1, 0, 1, 0]).await.unwrap();
            let mut req = vec![0u8; 6 + 4 + 12 + 4 + 5];
            server.read_exact(&mut req).await.unwrap();
            server.write_all(&[1, 0, 4, 0, 1, 0, 0, 0, 0, 0]).await.unwrap();
            req
        });
        let c = Credentials { redirection_guid: b"GU", username: "Raph", domain: "", password: &[1, 2, 3] };
        authenticate(&mut client, &c).await.unwrap();
        assert_eq!(srv.await.unwrap(), authentication_request(&c).unwrap());
    }
}
