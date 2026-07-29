use anyhow::{bail, Context, Result};
use ectool_core::{find_download_port_now, wait_for_download_port};
use serialport::SerialPort;
use std::time::Duration;

/// Log port: VID=0x19D1, PID=0x0001
/// CDC ACM comm interface 2 / data interface 3.
/// macOS reports data interface, Linux/Windows report comm interface.
pub const LOG_VID: u16 = 0x19D1;
pub const LOG_PID: u16 = 0x0001;
pub const LOG_COMM_INTERFACE: u8 = 2;
pub const LOG_DATA_INTERFACE: u8 = 3;
const LOG_INTERFACES: &[u8] = &[LOG_COMM_INTERFACE, LOG_DATA_INTERFACE];

/// Find the LuatOS formatted-log port without waiting.
pub fn find_log_port_now() -> Result<Option<String>> {
    let mut matches = Vec::new();
    for port in serialport::available_ports().context("Failed to list serial ports")? {
        if let serialport::SerialPortType::UsbPort(usb_info) = &port.port_type {
            if usb_info.vid == LOG_VID && usb_info.pid == LOG_PID {
                match usb_info.interface {
                    Some(interface) if LOG_INTERFACES.contains(&interface) => {}
                    Some(_) => continue,
                    // Some platforms do not report a USB interface number.
                    None => {}
                }
                matches.push(port.port_name);
            }
        }
    }

    // Treat the macOS callout/dial-in aliases as one physical port.
    let callout_suffixes = matches
        .iter()
        .filter_map(|name| name.strip_prefix("/dev/cu."))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    matches.retain(|name| {
        name.strip_prefix("/dev/tty.")
            .map(|suffix| !callout_suffixes.iter().any(|callout| callout == suffix))
            .unwrap_or(true)
    });

    match matches.as_slice() {
        [] => Ok(None),
        [name] => Ok(Some(name.clone())),
        _ => bail!(
            "Multiple LuatOS log ports ({:04X}:{:04X}) found: {}. Specify one with --port",
            LOG_VID,
            LOG_PID,
            matches.join(", ")
        ),
    }
}

fn wait_for_log_port(timeout: Duration) -> Result<String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(port) = find_log_port_now()? {
            return Ok(port);
        }
        if std::time::Instant::now() >= deadline {
            bail!(
                "Timeout waiting for LuatOS log port {:04X}:{:04X}",
                LOG_VID,
                LOG_PID
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn send_diag_frame_on_port(port: &mut dyn SerialPort, frame: &[u8]) -> Result<()> {
    port.write_all(frame)
        .context("Failed to write diag reboot frame")?;
    port.flush().context("Failed to flush diag reboot frame")?;
    std::thread::sleep(Duration::from_millis(200));
    Ok(())
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
    let log_port = match find_log_port_now() {
        Ok(Some(port)) => port,
        Ok(None) => return false,
        Err(error) => {
            log::warn!("Unable to select LuatOS log port: {error}");
            return false;
        }
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

    reboot_to_download_on_port(port.as_mut()).is_ok()
}

/// Resolve the conservative EigenComm download port, using the LuatOS runtime
/// reboot sequence only when no download port is already present.
pub fn resolve_boot_port(requested: &str) -> Result<String> {
    if requested != "auto" {
        return Ok(requested.to_string());
    }

    if find_download_port_now("auto")?.is_none() {
        if try_reboot_to_download() {
            log::info!("Reboot command sent, waiting for download port...");
        } else {
            log::info!("No running device found; press BOOT and power on or reset the module");
        }
    }

    let found = wait_for_download_port("auto", Duration::from_secs(120))?;
    log::info!("Found {}", found.name);
    Ok(found.name)
}

/// Resolve the LuatOS formatted-log port.
pub fn resolve_log_port(requested: &str) -> Result<String> {
    if requested != "auto" {
        return Ok(requested.to_string());
    }

    log::info!("Searching for LuatOS log port, max wait 120s");
    let found = wait_for_log_port(Duration::from_secs(120))?;
    log::info!("Found {}", found);
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_ports_are_preserved() {
        assert_eq!(
            resolve_boot_port("/dev/cu.explicit").unwrap(),
            "/dev/cu.explicit"
        );
        assert_eq!(
            resolve_log_port("/dev/cu.explicit").unwrap(),
            "/dev/cu.explicit"
        );
    }
}
