//! Serial port sessions (COMx on Windows, /dev/ttyUSB* on Linux).

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serialport::{DataBits, FlowControl, Parity, SerialPort, StopBits};

use crate::config::Session;
use crate::error::{AppError, AppResult};
use crate::ssh::{CloseSink, DataSink};

pub struct SerialConnection {
    port: Mutex<Box<dyn SerialPort>>,
    stop: Arc<AtomicBool>,
}

pub fn list_ports() -> Vec<String> {
    serialport::available_ports().map(|v| v.into_iter().map(|p| p.port_name).collect()).unwrap_or_default()
}

impl SerialConnection {
    pub fn open(s: &Session, on_data: DataSink, on_close: CloseSink) -> AppResult<Arc<SerialConnection>> {
        let name = if s.opt("serial_port").is_empty() { s.host.as_str() } else { s.opt("serial_port") };
        if name.is_empty() {
            return Err(AppError::Config("aucun port série choisi".into()));
        }
        let baud: u32 = s.opt("baud").parse().unwrap_or(115_200);
        let data_bits = match s.opt("data_bits") {
            "5" => DataBits::Five,
            "6" => DataBits::Six,
            "7" => DataBits::Seven,
            _ => DataBits::Eight,
        };
        let parity = match s.opt("parity") {
            "odd" => Parity::Odd,
            "even" => Parity::Even,
            _ => Parity::None,
        };
        let stop_bits = if s.opt("stop_bits") == "2" { StopBits::Two } else { StopBits::One };
        let flow = match s.opt("flow") {
            "hardware" => FlowControl::Hardware,
            "software" => FlowControl::Software,
            _ => FlowControl::None,
        };
        let port = serialport::new(name, baud)
            .data_bits(data_bits)
            .parity(parity)
            .stop_bits(stop_bits)
            .flow_control(flow)
            .timeout(Duration::from_millis(100))
            .open()
            .map_err(|e| AppError::Conn(format!("{name} : {e}")))?;
        let mut reader = port.try_clone().map_err(|e| AppError::Conn(e.to_string()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let reason = loop {
                if stop2.load(Ordering::SeqCst) {
                    break "Port fermé".to_string();
                }
                match reader.read(&mut buf) {
                    Ok(0) => {}
                    Ok(n) => on_data(buf[..n].to_vec()),
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(e) => break format!("Port série : {e}"),
                }
            };
            on_close(reason);
        });
        Ok(Arc::new(SerialConnection { port: Mutex::new(port), stop }))
    }

    pub fn write(&self, data: &[u8]) -> AppResult<()> {
        self.port.lock().unwrap().write_all(data)?;
        Ok(())
    }

    pub fn close(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}
