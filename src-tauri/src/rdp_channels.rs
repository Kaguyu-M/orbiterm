//! RDP static virtual channels of the embedded client: shared clipboard (CLIPRDR, text)
//! and local folder sharing (RDPDR drive redirection).
//!
//! The clipboard backend does no I/O itself: IronRDP calls it synchronously while processing
//! PDUs; it only forwards the events to the session loop, which answers through the
//! `Cliprdr` processor and talks to the tab (the tab owns the local clipboard).

use std::path::Path;

use ironrdp_cliprdr::backend::CliprdrBackend;
use ironrdp_cliprdr::pdu::{
    ClipboardFormat, ClipboardFormatId, ClipboardGeneralCapabilityFlags, FileContentsRequest, FileContentsResponse,
    FormatDataRequest, FormatDataResponse, LockDataId,
};
use ironrdp_cliprdr::{Cliprdr, CliprdrClient};
use ironrdp_pdu::PduResult;
use ironrdp_rdpdr::Rdpdr;
use ironrdp_rdpdr::backend::{RdpdrBackend, RdpdrBackendFactory as _};
use ironrdp_rdpdr::pdu::efs::{
    AnyIoCtlCode, DecodedDeviceControlRequest, DeviceControlRequest, DeviceIoRequest, FileInformationClass,
    PrinterIoRequest, ServerDeviceAnnounceResponse, ServerDriveIoRequest,
};
use ironrdp_rdpdr::pdu::esc::{ScardCall, ScardIoCtlCode};
use ironrdp_rdpdr::pdu::esp::PortIoRequest;
use ironrdp_svc::SvcMessage;
use tokio::sync::mpsc;

pub const CF_UNICODETEXT: ClipboardFormatId = ClipboardFormatId::CF_UNICODETEXT;
/// Biggest clipboard text exchanged (characters), to keep the bridge responsive.
pub const MAX_CLIP_CHARS: usize = 4 * 1024 * 1024;

/// Clipboard events raised by IronRDP, handled by the session loop.
#[derive(Debug)]
pub enum ClipEvent {
    /// the server is ready: advertise our clipboard
    AdvertiseLocal,
    /// the remote clipboard changed; `true` when it holds text
    RemoteCopy(bool),
    /// the server pastes: it wants our text
    DataRequest(ClipboardFormatId),
    /// text received from the server (`None`: error / not text)
    DataResponse(Option<String>),
}

#[derive(Debug)]
pub struct ClipBackend {
    tx: mpsc::UnboundedSender<ClipEvent>,
    tmp: String,
}

ironrdp_core::impl_as_any!(ClipBackend);

impl CliprdrBackend for ClipBackend {
    fn temporary_directory(&self) -> &str {
        &self.tmp
    }

    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        // text only: no file transfer through the clipboard
        ClipboardGeneralCapabilityFlags::USE_LONG_FORMAT_NAMES
    }

    fn on_ready(&mut self) {}

    fn on_request_format_list(&mut self) {
        let _ = self.tx.send(ClipEvent::AdvertiseLocal);
    }

    fn on_process_negotiated_capabilities(&mut self, _capabilities: ClipboardGeneralCapabilityFlags) {}

    fn on_remote_copy(&mut self, available_formats: &[ClipboardFormat]) {
        let text = available_formats.iter().any(|f| f.id() == CF_UNICODETEXT);
        let _ = self.tx.send(ClipEvent::RemoteCopy(text));
    }

    fn on_format_data_request(&mut self, request: FormatDataRequest) {
        let _ = self.tx.send(ClipEvent::DataRequest(request.format));
    }

    fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        let text = if response.is_error() { None } else { response.to_unicode_string().ok() };
        let _ = self.tx.send(ClipEvent::DataResponse(text.map(|t| normalize_remote(&t))));
    }

    fn on_file_contents_request(&mut self, _request: FileContentsRequest) {}

    fn on_file_contents_response(&mut self, _response: FileContentsResponse<'_>) {}

    fn on_lock(&mut self, _data_id: LockDataId) {}

    fn on_unlock(&mut self, _data_id: LockDataId) {}
}

/// Clipboard channel + the receiver of its events.
pub fn clipboard() -> (CliprdrClient, mpsc::UnboundedReceiver<ClipEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let tmp = std::env::temp_dir().to_string_lossy().into_owned();
    (Cliprdr::new(Box::new(ClipBackend { tx, tmp })), rx)
}

/// Remote (Windows) text → local: CRLF to LF on Linux, trailing NULs removed.
fn normalize_remote(t: &str) -> String {
    let t = t.trim_end_matches('\0');
    let t: String = t.chars().take(MAX_CLIP_CHARS).collect();
    if cfg!(windows) { t } else { t.replace("\r\n", "\n") }
}

/// Local text → remote: Windows expects CRLF line ends.
pub fn to_remote(t: &str) -> String {
    let t: String = t.chars().take(MAX_CLIP_CHARS).collect();
    t.replace("\r\n", "\n").replace('\n', "\r\n")
}

// ---------------------------------------------------------------- folder sharing

/// RDPDR device ID of the shared folder (any non-zero ID outside the drive letters works).
const SHARE_DEVICE_ID: u32 = 0x100;

/// Name shown on the remote side ("<name> on <computer>"): letters, digits, `-` and `_`.
pub fn share_name(dir: &Path) -> String {
    let base = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let name: String = base
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(7)
        .collect();
    if name.trim_matches('_').is_empty() { "Orbiterm".into() } else { name }
}

/// RDPDR channel sharing `dir` (read/write) with the remote computer.
pub fn drive_channel(dir: &Path) -> Result<Rdpdr, String> {
    let dir = dir.canonicalize().map_err(|e| format!("{} : {e}", dir.display()))?;
    if !dir.is_dir() {
        return Err(format!("{} n'est pas un dossier", dir.display()));
    }
    let name = share_name(&dir);
    let computer = hostname();

    #[cfg(windows)]
    let factory = {
        use ironrdp_rdpdr_native::{RedirectedDrive, WindowsRdpdrBackendFactory};
        // canonicalize() gives a verbatim path (\\?\C:\…): the backend wants C:\…
        let plain = dir.to_string_lossy().trim_start_matches(r"\\?\").to_string();
        let drive = RedirectedDrive::new(SHARE_DEVICE_ID, name.clone(), plain, false).map_err(|e| e.to_string())?;
        WindowsRdpdrBackendFactory::new(drive)
    };
    #[cfg(not(windows))]
    let factory = ironrdp_rdpdr_native::backend::NixRdpdrBackendFactory::new(dir.to_string_lossy().into_owned())
        .with_drive(SHARE_DEVICE_ID, name.clone());

    let product = factory.build_rdpdr_backend().map_err(|e| e.to_string())?;
    let (backend, drives) = product.into_parts();
    let drives = drives.into_iter().map(|d| d.into_parts()).collect();
    Ok(Rdpdr::new(Box::new(SlashFix(backend)), computer).with_drives(Some(drives)))
}

/// xrdp sends some paths with "/" separators ("/file.txt") where RDPDR uses "\": the native
/// backend would reject them as rooted. "/" cannot appear in a file name, so it is rewritten
/// before the request reaches the backend.
#[derive(Debug)]
struct SlashFix(Box<dyn RdpdrBackend>);

ironrdp_core::impl_as_any!(SlashFix);

fn fix_path(p: &mut String) {
    if p.contains('/') {
        *p = p.replace('/', "\\");
    }
}

impl RdpdrBackend for SlashFix {
    fn supports_drive_security(&self) -> bool {
        self.0.supports_drive_security()
    }
    fn reset(&mut self) -> PduResult<()> {
        self.0.reset()
    }
    fn restore_drive(&mut self, device_id: u32) -> PduResult<()> {
        self.0.restore_drive(device_id)
    }
    fn add_drive(&mut self, device_id: u32) -> PduResult<()> {
        self.0.add_drive(device_id)
    }
    fn remove_drive(&mut self, device_id: u32) -> PduResult<Vec<SvcMessage>> {
        self.0.remove_drive(device_id)
    }
    fn handle_server_device_announce_response(&mut self, pdu: ServerDeviceAnnounceResponse) -> PduResult<()> {
        self.0.handle_server_device_announce_response(pdu)
    }
    fn handle_scard_call(
        &mut self,
        req: DeviceControlRequest<ScardIoCtlCode>,
        call: ScardCall,
    ) -> PduResult<Vec<SvcMessage>> {
        self.0.handle_scard_call(req, call)
    }
    fn handle_drive_io_request(&mut self, mut req: ServerDriveIoRequest) -> PduResult<Vec<SvcMessage>> {
        match &mut req {
            ServerDriveIoRequest::ServerCreateDriveRequest(r) => fix_path(&mut r.path),
            ServerDriveIoRequest::ServerDriveQueryDirectoryRequest(r) => fix_path(&mut r.path),
            ServerDriveIoRequest::ServerDriveSetInformationRequest(r) => {
                if let FileInformationClass::Rename(i) = &mut r.set_buffer {
                    fix_path(&mut i.file_name);
                }
            }
            _ => {}
        }
        self.0.handle_drive_io_request(req)
    }
    fn handle_drive_device_control(
        &mut self,
        req: DecodedDeviceControlRequest<AnyIoCtlCode>,
    ) -> PduResult<Vec<SvcMessage>> {
        self.0.handle_drive_device_control(req)
    }
    fn poll_deferred_messages(&mut self) -> PduResult<Vec<SvcMessage>> {
        self.0.poll_deferred_messages()
    }
    fn handle_user_logged_on(&mut self, rdpdr: &mut Rdpdr) -> PduResult<Vec<SvcMessage>> {
        self.0.handle_user_logged_on(rdpdr)
    }
    fn handle_printer_io_request(&mut self, req: PrinterIoRequest) -> PduResult<Vec<SvcMessage>> {
        self.0.handle_printer_io_request(req)
    }
    fn handle_port_io_request(&mut self, req: PortIoRequest) -> PduResult<Vec<SvcMessage>> {
        self.0.handle_port_io_request(req)
    }
    fn reject_printer_write(&mut self, req: DeviceIoRequest) -> PduResult<Vec<SvcMessage>> {
        self.0.reject_printer_write(req)
    }
}

fn hostname() -> String {
    let h = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .unwrap_or_default();
    let h = h.trim();
    if h.is_empty() { "orbiterm".into() } else { h.chars().take(15).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_line_ends() {
        assert_eq!(share_name(Path::new("/home/u/Mes Documents")), "Mes_Doc");
        assert_eq!(share_name(Path::new("/")), "Orbiterm");
        assert_eq!(to_remote("a\nb\r\nc"), "a\r\nb\r\nc");
        let back = normalize_remote("a\r\nb\0");
        assert_eq!(back, if cfg!(windows) { "a\r\nb" } else { "a\nb" });
        let mut p = String::from("/dir/f.txt");
        fix_path(&mut p);
        assert_eq!(p, r"\dir\f.txt");
    }

    #[test]
    fn drive_channel_builds() {
        let dir = tempfile::tempdir().unwrap();
        let ch = drive_channel(dir.path());
        assert!(ch.is_ok(), "{:?}", ch.err());
        assert!(drive_channel(&dir.path().join("absent")).is_err());
    }
}
