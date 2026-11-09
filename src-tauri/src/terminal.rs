//! A tab's backend, whatever the protocol.

use std::sync::Arc;

use crate::error::{AppError, AppResult};
use crate::ftp::FtpConnection;
use crate::pty::PtyConnection;
use crate::rdp::ExternalProcess;
use crate::serial::SerialConnection;
use crate::ssh::SshConnection;
use crate::telnet::TcpTermConnection;
use crate::vnc::VncBridge;

#[derive(Clone)]
pub enum Conn {
    Ssh(Arc<SshConnection>),
    Pty(Arc<PtyConnection>),
    Tcp(Arc<TcpTermConnection>),
    Serial(Arc<SerialConnection>),
    /// FTP file session (no terminal)
    Ftp(Arc<FtpConnection>),
    /// program in its own window (mstsc)
    External(Arc<ExternalProcess>),
    /// VNC WebSocket bridge; the SSH gateway (if any) lives inside it
    Vnc(Arc<VncBridge>),
}

impl Conn {
    pub async fn write(&self, data: &[u8]) -> AppResult<()> {
        match self {
            Conn::Ssh(c) => c.write(data).await,
            Conn::Pty(c) => {
                let c = c.clone();
                let d = data.to_vec();
                tokio::task::spawn_blocking(move || c.write(&d)).await.map_err(|e| AppError::Conn(e.to_string()))?
            }
            Conn::Tcp(c) => c.write(data).await,
            Conn::Serial(c) => c.write(data),
            // no keyboard stream for these (MultiExec / macros skip them silently)
            Conn::Ftp(_) | Conn::External(_) | Conn::Vnc(_) => Ok(()),
        }
    }

    pub async fn resize(&self, cols: u32, rows: u32) -> AppResult<()> {
        match self {
            Conn::Ssh(c) => c.resize(cols, rows).await,
            Conn::Pty(c) => c.resize(cols, rows),
            Conn::Tcp(c) => c.resize(cols, rows).await,
            _ => Ok(()),
        }
    }

    pub async fn close(&self) {
        match self {
            Conn::Ssh(c) => c.disconnect().await,
            Conn::Pty(c) => c.kill(),
            Conn::Tcp(c) => c.close().await,
            Conn::Serial(c) => c.close(),
            Conn::Ftp(c) => c.close().await,
            Conn::External(c) => c.kill(),
            Conn::Vnc(c) => c.stop(),
        }
    }

    pub fn ssh(&self) -> AppResult<Arc<SshConnection>> {
        match self {
            Conn::Ssh(c) => Ok(c.clone()),
            _ => Err(AppError::Unsupported("disponible uniquement sur les sessions SSH".into())),
        }
    }
}
