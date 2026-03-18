use anyhow::{bail, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serialport::SerialPort;

use super::commands::*;
use super::consts::*;
use super::lpc::*;
use super::protocol::Cmd;
use super::sync::burn_sync;
use crate::serial::port::PortType;

// Embedded agent boot binaries
const AGENTBOOT_EC618_USB: &[u8] = include_bytes!("../../agentboot/ec618_usb.bin");
const AGENTBOOT_EC618_UART: &[u8] = include_bytes!("../../agentboot/ec618_uart.bin");
const AGENTBOOT_EC716_USB: &[u8] = include_bytes!("../../agentboot/ec716_usb.bin");
const AGENTBOOT_EC716_UART: &[u8] = include_bytes!("../../agentboot/ec716_uart.bin");
const AGENTBOOT_EC718_USB: &[u8] = include_bytes!("../../agentboot/ec718_usb.bin");
const AGENTBOOT_EC718_UART: &[u8] = include_bytes!("../../agentboot/ec718_uart.bin");
const AGENTBOOT_EC718M_USB: &[u8] = include_bytes!("../../agentboot/ec718m_usb.bin");
const AGENTBOOT_EC718M_UART: &[u8] = include_bytes!("../../agentboot/ec718m_uart.bin");
const AGENTBOOT_EC217_USB: &[u8] = include_bytes!("../../agentboot/ec217_usb.bin");
const AGENTBOOT_EC217_UART: &[u8] = include_bytes!("../../agentboot/ec217_uart.bin");

/// Determine chip family from chip name string.
/// Returns "ec618", "ec716", "ec718", "ec718m", or "ec217".
pub fn chip_family(chip_name: &str) -> Result<&'static str> {
    let chip = chip_name.trim().to_ascii_uppercase();

    if chip.starts_with("EC718HM")
        || chip.starts_with("EC718UM")
        || chip.starts_with("EC718PM")
        || chip.starts_with("EC718SM")
    {
        Ok("ec718m")
    } else if chip.starts_with("EC718") {
        Ok("ec718")
    } else if chip.starts_with("EC716") {
        Ok("ec716")
    } else if chip.starts_with("QCX217") || chip.starts_with("EC217") {
        Ok("ec217")
    } else if chip.starts_with("EC618") {
        Ok("ec618")
    } else {
        bail!("Unable to determine chip family from chip name: {}", chip_name)
    }
}

/// Load the appropriate agent boot binary for the given chip and port type.
pub fn load_agentboot(chip_name: &str, port_type: PortType) -> Result<&'static [u8]> {
    let family = chip_family(chip_name)?;

    let data = match (family, port_type) {
        ("ec618", PortType::Usb) => AGENTBOOT_EC618_USB,
        ("ec618", PortType::Uart) => AGENTBOOT_EC618_UART,
        ("ec716", PortType::Usb) => AGENTBOOT_EC716_USB,
        ("ec716", PortType::Uart) => AGENTBOOT_EC716_UART,
        ("ec718", PortType::Usb) => AGENTBOOT_EC718_USB,
        ("ec718", PortType::Uart) => AGENTBOOT_EC718_UART,
        ("ec718m", PortType::Usb) => AGENTBOOT_EC718M_USB,
        ("ec718m", PortType::Uart) => AGENTBOOT_EC718M_UART,
        ("ec217", PortType::Usb) => AGENTBOOT_EC217_USB,
        ("ec217", PortType::Uart) => AGENTBOOT_EC217_UART,
        _ => bail!("Unknown chip family: {}", family),
    };

    log::info!(
        "Chip: {} -> agent boot: {}_{:?}",
        chip_name,
        family,
        port_type
    );
    Ok(data)
}

/// Download agent boot to device.
///
/// Sequence: base_info(HEAD) -> image_head(AGBOOT) -> DLBOOT sync -> base_info(BL) -> download_data
pub fn burn_agboot(port: &mut dyn SerialPort, agent_data: &[u8], baud: u32) -> Result<i32> {
    log::info!("Burn agent boot start");

    let (ret, _ver) = package_base_info(port, BurnImageType::Head.identifier(), true)?;
    if ret != 0 {
        bail!("Agent boot base_info(HEAD) failed");
    }

    log::debug!("agentboot file size {}", agent_data.len());
    let ret = package_image_head(port, agent_data, BurnImageType::AgBoot, 0, baud, true, 1)?;
    if ret != 0 {
        bail!("Agent boot image_head failed");
    }

    burn_sync(port, SyncType::DlBoot, 2)?;

    let (ret, _ver) = package_base_info(port, BurnImageType::Bootloader.identifier(), true)?;
    if ret != 0 {
        bail!("Agent boot base_info(BL) failed");
    }

    let mut cmd = Cmd::new(CMD_DOWNLOAD_DATA);
    cmd.len = agent_data.len() as u32;
    let ret = package_data(port, &mut cmd, agent_data, true)?;
    if ret != 0 {
        bail!("Agent boot data download failed");
    }

    log::info!("Agent boot download complete");
    Ok(0)
}

/// Burn a single image partition.
///
/// Sequence: LPC sync -> lpc_burn_one -> AGBOOT sync x2 -> base_info ->
///           image_head -> (loop: AGBOOT sync + 64KB chunks) -> lpc_get_burn_status
pub fn burn_img(
    port: &mut dyn SerialPort,
    data: &[u8],
    img_type: BurnImageType,
    stor_type: u8,
    addr: u32,
    tag: &str,
    mut progress: Option<&mut dyn FnMut(u64, u64)>,
) -> Result<i32> {
    log::info!(
        "burn image {} {:?} stor={} addr={:08X}",
        tag,
        img_type,
        stor_type,
        addr
    );

    // 1. LPC Sync
    burn_sync(port, SyncType::Lpc, 2)?;

    // 2. LPC burn one
    let ret = lpc_burn_one(port, img_type, stor_type)?;
    if ret != 0 {
        bail!("lpc_burn_one failed for {}", tag);
    }

    // 3. AGBOOT Sync x2
    burn_sync(port, SyncType::AgBoot, 2)?;
    burn_sync(port, SyncType::AgBoot, 2)?;

    // 4. Base info
    let (ret, _ver) = package_base_info(port, BurnImageType::Head.identifier(), false)?;
    if ret != 0 {
        bail!("package_base_info failed for {}", tag);
    }

    // 5. Image header
    let ret = package_image_head(port, data, img_type, addr, 0, false, 0)?;
    if ret != 0 {
        bail!("package_image_head failed for {}", tag);
    }

    // 6. Data transfer in 64KB blocks
    let mut remain = data.len();
    let mut data_offset: usize = 0;
    let mut ret = 0;

    let total = data.len() as u64;
    let pb = if progress.is_none() {
        let pb = ProgressBar::new(total);
        pb.set_style(
            ProgressStyle::default_bar()
                .template(&format!(
                    "  {{bar:40.cyan/blue}} {{pos:>7}}/{{len:7}} {}",
                    tag
                ))
                .unwrap()
                .progress_chars("##-"),
        );
        Some(pb)
    } else {
        None
    };

    log::debug!("start send file data ...");
    while remain > 0 {
        burn_sync(port, SyncType::AgBoot, 2)?;

        let data_len = if remain > MAX_DATA_BLOCK_SIZE {
            MAX_DATA_BLOCK_SIZE
        } else {
            remain
        };

        let mut cmd = Cmd::new(CMD_DOWNLOAD_DATA);
        cmd.len = data_len as u32;
        ret = package_data(
            port,
            &mut cmd,
            &data[data_offset..data_offset + data_len],
            false,
        )?;
        if ret != 0 {
            if let Some(ref pb) = pb {
                pb.abandon_with_message(format!("{} FAILED", tag));
            }
            log::error!("package_data failed for {}", tag);
            break;
        }

        data_offset += data_len;
        remain -= data_len;
        if let Some(ref mut cb) = progress {
            cb(data_offset as u64, total);
        }
        if let Some(ref pb) = pb {
            pb.set_position(data_offset as u64);
        }
    }

    log::debug!("almost done burn_img");
    if ret == 0 {
        ret = lpc_get_burn_status(port)?;
    }
    if let Some(pb) = pb {
        pb.finish_with_message(format!("{} done", tag));
    }

    Ok(ret)
}

/// Reset the device via LPC command.
pub fn sys_reset(port: &mut dyn SerialPort) -> Result<i32> {
    burn_sync(port, SyncType::Lpc, 2)?;
    let ret = lpc_sys_reset(port)?;
    Ok(ret)
}

#[cfg(test)]
mod tests {
    use super::chip_family;

    #[test]
    fn maps_ec718_variants_from_product_config() {
        assert_eq!(chip_family("EC718U_PRD").unwrap(), "ec718");
        assert_eq!(chip_family("EC718H_PRD").unwrap(), "ec718");
        assert_eq!(chip_family("EC718P_PRD").unwrap(), "ec718");
        assert_eq!(chip_family("EC718S_PRD").unwrap(), "ec718");
        assert_eq!(chip_family("EC718SEF_PRD").unwrap(), "ec718");
        assert_eq!(chip_family("EC718PEF_PRD").unwrap(), "ec718");
    }

    #[test]
    fn maps_ec718m_variants_from_product_config() {
        assert_eq!(chip_family("EC718HM_PRD").unwrap(), "ec718m");
        assert_eq!(chip_family("EC718UM_PRD").unwrap(), "ec718m");
        assert_eq!(chip_family("EC718PM_PRD").unwrap(), "ec718m");
        assert_eq!(chip_family("EC718SM_PRD").unwrap(), "ec718m");
    }

    #[test]
    fn maps_other_known_families() {
        assert_eq!(chip_family("EC618_CUSTOM_TEST").unwrap(), "ec618");
        assert_eq!(chip_family("EC716S_PRD").unwrap(), "ec716");
        assert_eq!(chip_family("EC716E_PRD").unwrap(), "ec716");
        assert_eq!(chip_family("QCX217_PRD").unwrap(), "ec217");
        assert_eq!(chip_family("EC217_PRD").unwrap(), "ec217");
    }

    #[test]
    fn rejects_unknown_families() {
        assert!(chip_family("UNKNOWN_CHIP").is_err());
        assert!(chip_family("ABCD1234").is_err());
    }
}
