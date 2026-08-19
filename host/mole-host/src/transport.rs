//! Transporte del host — TR-07: `Transport: Read + Write + Send`.
//! Agregar TR-05 (vendor bulk) no debe tocar el decoder ni el store.

use std::io::{Read, Write};

/// VID conocidos: Espressif nativo y puentes USB-serial habituales (DX-03).
pub fn known_vid(vid: u16) -> Option<&'static str> {
    match vid {
        0x303A => Some("Espressif (USB nativo)"),
        0x10C4 => Some("Silicon Labs CP210x"),
        0x1A86 => Some("WCH CH340"),
        0x0403 => Some("FTDI"),
        _ => None,
    }
}

/// Lista de puertos con metadata USB, lista para la UI (DX-03).
pub fn list_ports() -> Vec<serde_json::Value> {
    let Ok(ports) = serialport::available_ports() else {
        return Vec::new();
    };
    ports
        .into_iter()
        .map(|p| {
            let (vid, pid, tag) = match &p.port_type {
                serialport::SerialPortType::UsbPort(u) => {
                    (Some(u.vid), Some(u.pid), known_vid(u.vid))
                }
                _ => (None, None, None),
            };
            serde_json::json!({
                "name": p.port_name,
                "vid": vid,
                "pid": pid,
                "known": tag,
            })
        })
        .collect()
}

pub trait Transport: Send {
    /// Lee lo que haya. Ok(0) = nada por ahora (no es EOF salvo `finished`).
    fn read_some(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;
    /// Escribe un frame de downlink completo.
    fn write_all_frame(&mut self, data: &[u8]) -> std::io::Result<()>;
    /// true cuando la fuente se agotó (solo replay).
    fn finished(&self) -> bool {
        false
    }
}

/// Puerto serie real (UART bridge o CDC). TR-09: la reconexión con backoff
/// vive en el dueño del pipeline, no acá.
pub struct SerialTransport {
    port: Box<dyn serialport::SerialPort>,
}

impl SerialTransport {
    pub fn open(port_name: &str, baud: u32) -> Result<Self, serialport::Error> {
        let port = serialport::new(port_name, baud)
            .timeout(std::time::Duration::from_millis(50))
            .open()?;
        Ok(SerialTransport { port })
    }
}

impl Transport for SerialTransport {
    fn read_some(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.port.read(buf) {
            Ok(n) => Ok(n),
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => Ok(0),
            Err(e) => Err(e),
        }
    }

    fn write_all_frame(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.port.write_all(data)
    }
}

/// Replay de un stream crudo desde archivo (TR-06/SES-03: el pipeline no
/// distingue replay de hardware). A velocidad máxima; el pacing 1× llega
/// con F7.
pub struct FileReplay {
    data: Vec<u8>,
    pos: usize,
    chunk: usize,
}

impl FileReplay {
    pub fn open(path: &str) -> std::io::Result<Self> {
        Ok(FileReplay {
            data: std::fs::read(path)?,
            pos: 0,
            chunk: 4096,
        })
    }

    pub fn from_bytes(data: Vec<u8>) -> Self {
        FileReplay { data, pos: 0, chunk: 4096 }
    }
}

impl Transport for FileReplay {
    fn read_some(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = buf.len().min(self.chunk).min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }

    fn write_all_frame(&mut self, _data: &[u8]) -> std::io::Result<()> {
        Ok(()) // el downlink de un replay va a la nada
    }

    fn finished(&self) -> bool {
        self.pos >= self.data.len()
    }
}
