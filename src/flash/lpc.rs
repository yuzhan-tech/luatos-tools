use anyhow::Result;
use serialport::SerialPort;

use super::commands::send_recv_lpc_cmd;
use super::consts::*;
use super::protocol::LpcCmd;

/// LPC burn one: tell the agent to prepare for a specific image type.
pub fn lpc_burn_one(
    port: &mut dyn SerialPort,
    img_type: BurnImageType,
    stor_type: u8,
) -> Result<i32> {
    let mut cmd = LpcCmd::new(LPC_BURN_ONE);
    let img_id = img_type.identifier();

    let data = if stor_type == STYPE_CP_FLASH {
        cmd.len = 6;
        let mut d = img_id.to_le_bytes().to_vec();
        d.extend_from_slice(&CP_FLASH_MARKER.to_le_bytes());
        d
    } else {
        cmd.len = 4;
        img_id.to_le_bytes().to_vec()
    };

    log::debug!("lpc burn one {:?} len={}", img_type, cmd.len);
    let (ret, _) = send_recv_lpc_cmd(port, &mut cmd, &data)?;
    log::debug!("lpc_burn_one {}", ret);
    Ok(ret)
}

/// LPC get burn status: check if the previous burn completed successfully.
/// Expects response data of b'\0\0\0\0'.
pub fn lpc_get_burn_status(port: &mut dyn SerialPort) -> Result<i32> {
    let mut cmd = LpcCmd::new(LPC_GET_BURN_STATUS);
    let (ret, data) = send_recv_lpc_cmd(port, &mut cmd, &[])?;
    log::debug!("lpc_get_burn_status {}", ret);
    if ret == 0 {
        if let Some(ref d) = data {
            if d == LPC_BURN_STATUS_OK {
                return Ok(0);
            }
        }
    }
    Ok(-1)
}

/// LPC flash erase at a given address and size.
pub fn lpc_flash_erase(port: &mut dyn SerialPort, addr: u32, size: u32) -> Result<i32> {
    let mut cmd = LpcCmd::new(LPC_FLASH_ERASE);
    cmd.len = 8;
    let mut data = size.to_le_bytes().to_vec();
    data.extend_from_slice(&addr.to_le_bytes());
    let (ret, _) = send_recv_lpc_cmd(port, &mut cmd, &data)?;
    Ok(ret)
}

/// LPC system reset. Expects response data of b'ZzZzZzZz'.
pub fn lpc_sys_reset(port: &mut dyn SerialPort) -> Result<i32> {
    let mut cmd = LpcCmd::new(LPC_SYS_RST);
    let (ret, data) = send_recv_lpc_cmd(port, &mut cmd, &[])?;
    log::debug!("lpc_sys_reset {}", ret);
    if ret == 0 {
        if let Some(ref d) = data {
            if d == LPC_SYS_RESET_ACK {
                return Ok(0);
            }
        }
    }
    Ok(-1)
}
