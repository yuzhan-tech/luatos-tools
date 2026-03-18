use anyhow::{bail, Context, Result};
use serialport::SerialPort;
use std::time::Duration;

/// USB Boot mode: VID=0x17D1, PID=0x0001
pub const BOOT_VID: u16 = 0x17D1;
pub const BOOT_PID: u16 = 0x0001;

/// Log port: VID=0x19D1, PID=0x0001
/// CDC ACM comm interface 2 / data interface 3.
/// macOS reports data interface, Linux/Windows report comm interface.
pub const LOG_VID: u16 = 0x19D1;
pub const LOG_PID: u16 = 0x0001;
pub const LOG_COMM_INTERFACE: u8 = 2;
pub const LOG_DATA_INTERFACE: u8 = 3;
const LOG_INTERFACES: &[u8] = &[LOG_COMM_INTERFACE, LOG_DATA_INTERFACE];

/// Auto-detect a serial port by USB VID/PID, optionally filtering by USB interface number.
/// For CDC ACM devices, pass both comm and data interface numbers since the reported
/// interface differs by platform (macOS reports data, Linux/Windows report comm).
pub fn auto_detect_port(vid: u16, pid: u16, interfaces: &[u8]) -> Option<String> {
    let ports = serialport::available_ports().ok()?;
    for port in ports {
        if let serialport::SerialPortType::UsbPort(usb_info) = &port.port_type {
            if usb_info.vid == vid && usb_info.pid == pid {
                if !interfaces.is_empty() {
                    match usb_info.interface {
                        Some(iface) if interfaces.contains(&iface) => {}
                        Some(_) => continue,
                        // If the platform doesn't report interface, accept the match
                        None => {}
                    }
                }
                return Some(port.port_name);
            }
        }
    }
    None
}

/// Wait for a serial port with the given VID/PID to appear, polling every 100ms.
/// If `timeout_secs` is 0, wait indefinitely.
pub fn wait_for_port(vid: u16, pid: u16, interfaces: &[u8], timeout_secs: u32) -> Result<String> {
    let infinite = timeout_secs == 0;
    let max_iterations = if infinite {
        u32::MAX
    } else {
        timeout_secs * 10
    };
    for _ in 0..max_iterations {
        if let Some(port) = auto_detect_port(vid, pid, interfaces) {
            return Ok(port);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    bail!(
        "Timeout waiting for USB device {:04X}:{:04X} ({} seconds)",
        vid,
        pid,
        timeout_secs
    );
}

/// Send a diag command to the log port. Returns true if the command was sent.
fn send_diag_frame(frame: &[u8]) -> bool {
    let log_port = auto_detect_port(LOG_VID, LOG_PID, LOG_INTERFACES);
    let log_port = match log_port {
        Some(p) => p,
        None => return false,
    };

    log::info!("Found log port {}, sending diag frame", log_port);

    let port = serialport::new(&log_port, 115200)
        .timeout(Duration::from_millis(500))
        .open();

    let mut port = match port {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Failed to open log port: {}", e);
            return false;
        }
    };

    let _ = port.write_all(frame);
    std::thread::sleep(Duration::from_millis(200));

    drop(port);
    true
}

fn send_diag_frame_on_port(port: &mut dyn SerialPort, frame: &[u8]) -> Result<()> {
    port.write_all(frame)
        .context("Failed to write diag reboot frame")?;
    port.flush().context("Failed to flush diag reboot frame")?;
    std::thread::sleep(Duration::from_millis(200));
    Ok(())
}

/// Reboot the module normally via diag command.
pub fn try_reboot() -> bool {
    send_diag_frame(b"\x7e\x00\x01\x7e")
}

/// Reboot the module normally using an already-open log port.
pub fn reboot_on_port(port: &mut dyn SerialPort) -> Result<()> {
    send_diag_frame_on_port(port, b"\x7e\x00\x01\x7e")
}

/// Reboot the module into download mode using an already-open log port.
pub fn reboot_to_download_on_port(port: &mut dyn SerialPort) -> Result<()> {
    port.write_all(b"AT+ECRST=delay,799\r\n")
        .context("Failed to write AT reboot command")?;
    port.flush().context("Failed to flush AT reboot command")?;
    std::thread::sleep(Duration::from_millis(200));

    send_diag_frame_on_port(port, b"\x7e\x00\x02\x7e")
}

/// Reboot the module into download mode via AT+ECRST + diag command.
pub fn try_reboot_to_download() -> bool {
    let log_port = auto_detect_port(LOG_VID, LOG_PID, LOG_INTERFACES);
    let log_port = match log_port {
        Some(p) => p,
        None => return false,
    };

    log::info!("Found log port {}, sending reboot-to-download", log_port);

    let port = serialport::new(&log_port, 115200)
        .timeout(Duration::from_millis(500))
        .open();

    let mut port = match port {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Failed to open log port: {}", e);
            return false;
        }
    };

    let ok = reboot_to_download_on_port(port.as_mut()).is_ok();
    drop(port);
    ok
}

/// Resolve a port string: "auto" triggers auto-detection, otherwise returns as-is.
pub fn resolve_port(port: &str, vid: u16, pid: u16, interfaces: &[u8]) -> Result<String> {
    if port == "auto" {
        if vid == BOOT_VID && pid == BOOT_PID {
            // For boot port: first try to reboot the device automatically
            if auto_detect_port(BOOT_VID, BOOT_PID, &[]).is_none() {
                if try_reboot_to_download() {
                    log::info!("Reboot command sent, waiting for boot port...");
                } else {
                    log::info!("No running device found, please press BOOT button and power on/reset the module");
                }
            }
            let found = wait_for_port(vid, pid, interfaces, 120)?;
            log::info!("Found {}", found);
            Ok(found)
        } else {
            log::info!("Searching for SoC Log COM, max wait 120s");
            let found = wait_for_port(vid, pid, interfaces, 120)?;
            log::info!("Found {}", found);
            Ok(found)
        }
    } else {
        Ok(port.to_string())
    }
}
