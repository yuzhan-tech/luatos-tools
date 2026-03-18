use anyhow::{bail, Result};
use serialport::SerialPort;
use sha2::{Digest, Sha256};

use super::consts::*;
use super::protocol::*;
use crate::serial::port::{com_read, com_write};
use crate::util::checksum::{crc8_maxim, self_def_check1};

/// Send a DL command and receive response.
///
/// Two modes:
/// - `dlboot=true`: DLBOOT mode, DOWNLOAD_DATA uses self_def_check1 checksum
/// - `dlboot=false`: AGBOOT mode, uses CRC32 suffix + CRC8-Maxim length encoding
pub fn send_recv_cmd(
    port: &mut dyn SerialPort,
    cmd: &mut Cmd,
    data: &[u8],
    dlboot: bool,
) -> Result<(i32, Option<Vec<u8>>)> {
    let mut tmpdata = cmd.pack();
    log::debug!("CMD {}", hex::encode(&tmpdata));
    tmpdata.extend_from_slice(data);

    if !dlboot {
        // AGBOOT mode: compute CRC32 first, then encode length with CRC8
        let ck_val = crc32fast::hash(&tmpdata);
        if cmd.len > 0 {
            let tmp_len = cmd.len & 0x00FFFFFF;
            let len_bytes = tmp_len.to_le_bytes();
            let crc8 = crc8_maxim(&len_bytes[..3]);
            cmd.len = ((crc8 as u32) << 24) | tmp_len;
            tmpdata = cmd.pack();
            tmpdata.extend_from_slice(data);
        }
        tmpdata.extend_from_slice(&ck_val.to_le_bytes());
    } else if cmd.cmd == CMD_DOWNLOAD_DATA {
        // DLBOOT mode with DOWNLOAD_DATA: use self_def_check1
        let ck = self_def_check1(
            cmd.cmd,
            cmd.index,
            cmd.order_id,
            cmd.norder_id,
            cmd.len,
            data,
        );
        tmpdata.extend_from_slice(&ck);
    }

    com_write(port, &tmpdata)?;
    std::thread::sleep(std::time::Duration::from_millis(2));

    // Read 6-byte response header
    let recv_buf = match com_read(port, FIXED_PROTOCOL_RSP_LEN)? {
        Some(buf) if buf.len() == FIXED_PROTOCOL_RSP_LEN => buf,
        Some(buf) => {
            log::warn!(
                "Read response incomplete: got {} bytes, expected {}",
                buf.len(),
                FIXED_PROTOCOL_RSP_LEN
            );
            return Ok((-1, None));
        }
        None => {
            log::warn!("Read response timeout");
            return Ok((-1, None));
        }
    };

    log::debug!("rsp buf {}", hex::encode(&recv_buf));
    let rsp = Rsp::unpack(&recv_buf);
    log::debug!("rsp.len {}", rsp.len);

    // Read response data
    let rsp_data = if rsp.len > 0 {
        com_read(port, rsp.len as usize)?
    } else {
        None
    };

    // In AGBOOT mode, read trailing CRC32
    if !dlboot {
        if let Some(crc_buf) = com_read(port, 4)? {
            log::debug!("read CRC32 {}", hex::encode(&crc_buf));
        }
    }

    if rsp.state != 0 {
        log::warn!("Response not ACK: state={}", rsp.state);
        if let Some(ref d) = rsp_data {
            log::warn!("Response data: {}", hex::encode(d));
        }
        return Ok((-2, None));
    }

    if let Some(ref d) = rsp_data {
        log::debug!("send_recv_cmd ok data={}", hex::encode(d));
    }

    Ok((0, rsp_data))
}

/// Send an LPC command and receive response.
pub fn send_recv_lpc_cmd(
    port: &mut dyn SerialPort,
    cmd: &mut LpcCmd,
    data: &[u8],
) -> Result<(i32, Option<Vec<u8>>)> {
    log::debug!("CMD lpc {}", hex::encode(cmd.pack()));

    // Compute CRC32 of original cmd + data
    let mut orig = cmd.pack();
    orig.extend_from_slice(data);
    let ck_val = crc32fast::hash(&orig);

    // Encode length with CRC8-Maxim
    cmd.len = data.len() as u32;
    if cmd.len > 0 {
        let tmp_len = cmd.len & 0x00FFFFFF;
        let len_bytes = tmp_len.to_le_bytes();
        let crc8 = crc8_maxim(&len_bytes[..3]);
        cmd.len = ((crc8 as u32) << 24) | tmp_len;
    }

    let mut tmpdata = cmd.pack();
    tmpdata.extend_from_slice(data);
    tmpdata.extend_from_slice(&ck_val.to_le_bytes());

    log::debug!("CMD lpc FULL {}", hex::encode(&tmpdata));
    com_write(port, &tmpdata)?;

    // Read 6-byte response
    if let Some(recv_buf) = com_read(port, 6)? {
        if recv_buf.len() < 6 {
            log::warn!("LPC response incomplete: got {} bytes", recv_buf.len());
            return Ok((-1, None));
        }
        log::debug!("rsp buf {}", hex::encode(&recv_buf));
        let rsp = Rsp::unpack(&recv_buf);
        log::debug!("lpc rsp state={} len={}", rsp.state, rsp.len);

        let rsp_data = if rsp.len > 0 {
            com_read(port, rsp.len as usize)?
        } else {
            None
        };

        // Read trailing CRC32
        if let Some(crc_buf) = com_read(port, 4)? {
            log::debug!("lpc rsp CRC32 {}", hex::encode(&crc_buf));
        }

        if rsp.state != 0 {
            log::warn!("LPC response not ACK: state={}", rsp.state);
            return Ok((-2, rsp_data));
        }

        return Ok((0, rsp_data));
    }

    Ok((-1, None))
}

/// Get firmware version from device.
pub fn package_get_version(
    port: &mut dyn SerialPort,
    dlboot: bool,
) -> Result<(i32, Option<VersionInfo>)> {
    log::debug!("package_get_version");
    let mut cmd = Cmd::new(CMD_GET_VERSION);
    let (ok, data) = send_recv_cmd(port, &mut cmd, &[], dlboot)?;
    if ok == 0 {
        if let Some(ref d) = data {
            if d.len() >= 16 {
                let ver = VersionInfo::unpack(d);
                log::info!(
                    "version: vVal=0x{:08X} id=0x{:08X} dtm=0x{:08X}",
                    ver.v_val,
                    ver.id,
                    ver.dtm
                );
                return Ok((0, Some(ver)));
            }
        }
    }
    Ok((ok, None))
}

/// Select image type on device.
pub fn package_sel_image(port: &mut dyn SerialPort, img_type: u32, dlboot: bool) -> Result<i32> {
    log::debug!("package_sel_image");
    let mut cmd = Cmd::new(CMD_SEL_IMAGE);
    let (ok, data) = send_recv_cmd(port, &mut cmd, &[], dlboot)?;
    if ok == 0 {
        if let Some(ref d) = data {
            if d.len() >= 4 {
                let ck_img = u32::from_le_bytes(d[0..4].try_into().unwrap());
                log::debug!("sel_image {:08X} vs {:08X}", img_type, ck_img);
                if img_type == ck_img {
                    return Ok(0);
                }
                log::error!("package_sel_image NOT match");
            }
        }
    }
    Ok(-1)
}

/// Verify image on device.
pub fn package_verify_image(port: &mut dyn SerialPort, dlboot: bool) -> Result<i32> {
    log::debug!("package_verify_image");
    let mut cmd = Cmd::new(CMD_VERIFY_IMAGE);
    let (ok, data) = send_recv_cmd(port, &mut cmd, &[], dlboot)?;
    if ok == 0 {
        if let Some(ref d) = data {
            log::debug!("verify_image {}", hex::encode(d));
        }
    }
    Ok(ok)
}

/// Execute the three base info steps in sequence: get_version, sel_image, verify_image.
pub fn package_base_info(
    port: &mut dyn SerialPort,
    img_type: u32,
    dlboot: bool,
) -> Result<(i32, Option<VersionInfo>)> {
    let (ok, ver) = package_get_version(port, dlboot)?;
    if ok != 0 {
        return Ok((-1, None));
    }
    if package_sel_image(port, img_type, dlboot)? != 0 {
        return Ok((-1, None));
    }
    if package_verify_image(port, dlboot)? != 0 {
        return Ok((-1, None));
    }
    Ok((0, ver))
}

/// Send DATA_HEAD command, returns the transfer block size.
fn package_data_head(
    port: &mut dyn SerialPort,
    remain_size: u32,
    dlboot: bool,
) -> Result<(i32, u32)> {
    log::debug!("package_data_head remainSize {:08X}", remain_size);
    let mut cmd = Cmd::new(CMD_DATA_HEAD);
    cmd.len = 4;
    let data = remain_size.to_le_bytes();
    let (ok, recv) = send_recv_cmd(port, &mut cmd, &data, dlboot)?;
    log::debug!("package_data_head {}", ok);
    if ok == 0 {
        if let Some(ref d) = recv {
            if d.len() >= 4 {
                let tb_size = u32::from_le_bytes(d[0..4].try_into().unwrap());
                log::debug!(
                    "package_data_head tbsize {:08X} remainSize {:08X}",
                    tb_size,
                    remain_size
                );
                return Ok((0, tb_size));
            }
        }
    }
    Ok((ok, 0))
}

/// Send a single data packet.
fn package_data_single(
    port: &mut dyn SerialPort,
    cmd: &mut Cmd,
    data: &[u8],
    dlboot: bool,
) -> Result<i32> {
    log::debug!("package_data_single data {}", data.len());
    cmd.len = data.len() as u32;
    let (ok, _) = send_recv_cmd(port, cmd, data, dlboot)?;
    log::debug!("package_data_single {}", ok);
    Ok(ok)
}

/// Send DONE command.
fn package_done(port: &mut dyn SerialPort, dlboot: bool) -> Result<i32> {
    let mut cmd = Cmd::new(CMD_DONE);
    let (ok, _) = send_recv_cmd(port, &mut cmd, &[], dlboot)?;
    log::debug!("package_done {}", ok);
    Ok(ok)
}

/// Transfer data using the data_head + data_single loop + done protocol.
pub fn package_data(
    port: &mut dyn SerialPort,
    cmd: &mut Cmd,
    data: &[u8],
    dlboot: bool,
) -> Result<i32> {
    log::debug!("package_data ====================");
    let mut data_offset: usize = 0;
    let mut remain_size = data.len() as u32;
    let mut counter: u8 = 0;
    let mut ret = 0;

    while remain_size > 0 {
        let (ok, tb_size) = package_data_head(port, remain_size, dlboot)?;
        if ok != 0 {
            return Ok(-1);
        }

        cmd.index = counter;
        cmd.len = tb_size;

        if tb_size >= remain_size {
            log::debug!("final data packet");
            ret = package_data_single(port, cmd, &data[data_offset..], dlboot)?;
            break;
        }

        let end = data_offset + tb_size as usize;
        ret = package_data_single(port, cmd, &data[data_offset..end], dlboot)?;
        if ret != 0 {
            break;
        }

        counter = counter.wrapping_add(1);
        data_offset += tb_size as usize;
        remain_size -= tb_size;
    }

    log::debug!("package_data almost end {}", ret);
    if ret == 0 {
        ret = package_done(port, dlboot)?;
    }
    Ok(ret)
}

/// Build and send image header.
pub fn package_image_head(
    port: &mut dyn SerialPort,
    fdata: &[u8],
    img_type: BurnImageType,
    addr: u32,
    baud: u32,
    dlboot: bool,
    pullup_qspi: u32,
) -> Result<i32> {
    log::debug!("package_image_head");

    let fhash: [u8; 32] = Sha256::digest(fdata).into();

    let mut img_hd = ImgHead::new();
    img_hd.set_body_id(img_type.identifier());
    img_hd.set_img_size(fdata.len() as u32);
    img_hd.set_burn_addr(addr);
    img_hd.set_hashv(&fhash);
    img_hd.set_baudrate_ctrl(baud);
    img_hd.set_hashtype(0xee);
    img_hd.set_rsvd0(pullup_qspi);
    img_hd.finalize_hash();

    let mut cmd = Cmd::new(CMD_DOWNLOAD_DATA);
    let hd_data = img_hd.pack().to_vec();
    cmd.len = hd_data.len() as u32;

    if package_data(port, &mut cmd, &hd_data, dlboot)? != 0 {
        bail!("package_image_head failed");
    }
    Ok(0)
}
