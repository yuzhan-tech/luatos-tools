use anyhow::{bail, Result};
use serialport::SerialPort;

use super::consts::SyncType;
use crate::serial::port::{com_read, com_write};

/// Perform handshake sync with the device.
///
/// Sends the handshake value repeatedly until the device echoes it back.
/// For DLBOOT sync, also reads and checks an extra byte (must be 0).
pub fn burn_sync(port: &mut dyn SerialPort, sync_type: SyncType, counter: u32) -> Result<()> {
    log::debug!("burn_sync {:?} counter={}", sync_type, counter);

    let handshake = sync_type.handshake_value();
    let send_buf = handshake.to_le_bytes();

    for _i in 0..50 {
        for _j in 0..counter {
            com_write(port, &send_buf)?;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        if let Some(recv_buf) = com_read(port, 4)? {
            log::debug!(
                "sync recv: {} (expect {})",
                hex::encode(&recv_buf),
                hex::encode(&send_buf)
            );
            if recv_buf.len() < 4 {
                continue;
            }

            if matches!(sync_type, SyncType::DlBoot) {
                // For DLBOOT, read an extra byte and check it is non-zero to reject.
                // If timeout (None), treat as OK.
                if let Some(extra) = com_read(port, 1)? {
                    log::debug!("sync extra byte: {:02x}", extra[0]);
                    if extra[0] != 0 {
                        continue;
                    }
                }
            }

            if recv_buf == send_buf {
                log::debug!("sync done");
                return Ok(());
            }
        }
    }

    bail!("Sync failed for {:?}", sync_type);
}
