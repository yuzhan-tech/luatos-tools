use anyhow::{Context, Result};
use serialport::SerialPort;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PortType {
    Usb,
    Uart,
}

impl PortType {
    pub fn baudrate(&self) -> u32 {
        match self {
            PortType::Usb => 921600,
            PortType::Uart => 115200,
        }
    }
}

/// Open a serial port with the appropriate settings for the given port type.
pub fn open_port(path: &str, port_type: PortType) -> Result<Box<dyn SerialPort>> {
    let baudrate = port_type.baudrate();
    log::info!(
        "Opening port {} at {} baud ({:?})",
        path,
        baudrate,
        port_type
    );

    let mut port = serialport::new(path, baudrate)
        .timeout(Duration::from_millis(800))
        .open()
        .with_context(|| format!("Failed to open serial port {}", path))?;

    port.write_data_terminal_ready(true)
        .context("Failed to set DTR")?;

    Ok(port)
}

/// Write data to serial port, chunking on non-Windows to avoid USB CDC issues.
pub fn com_write(port: &mut dyn SerialPort, data: &[u8]) -> Result<()> {
    log::debug!("COM WRITE: {} bytes", data.len());

    if cfg!(target_os = "windows") || data.len() <= 64 {
        port.write_all(data).context("Serial write failed")?;
    } else {
        for chunk in data.chunks(64) {
            port.write_all(chunk).context("Serial write failed")?;
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    Ok(())
}

/// Read up to `len` bytes from serial port.
///
/// Returns whatever bytes are available within the timeout, which may be fewer
/// than `len`. Returns `None` only if zero bytes were read (timeout with no data).
pub fn com_read(port: &mut dyn SerialPort, len: usize) -> Result<Option<Vec<u8>>> {
    if len == 0 {
        return Ok(None);
    }

    let mut buf = vec![0u8; len];
    let mut total = 0;

    while total < len {
        match port.read(&mut buf[total..]) {
            Ok(n) => {
                total += n;
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                break;
            }
            Err(e) => return Err(e).context("Serial read failed"),
        }
    }

    if total == 0 {
        log::debug!("COM READ timeout");
        Ok(None)
    } else {
        buf.truncate(total);
        log::debug!("COM READ: {} bytes (requested {})", total, len);
        Ok(Some(buf))
    }
}
