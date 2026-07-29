use anyhow::{bail, Result};
use ectool_core::PortType;

// Curated AgentBoot binaries distributed by luatos-tools. The generic ectool
// library deliberately requires callers to provide revision/transport-matched
// bytes instead of embedding or guessing them.
const AGENTBOOT_EC618_USB: &[u8] = include_bytes!("../agentboot/ec618_usb.bin");
const AGENTBOOT_EC618_UART: &[u8] = include_bytes!("../agentboot/ec618_uart.bin");
const AGENTBOOT_EC716_USB: &[u8] = include_bytes!("../agentboot/ec716_usb.bin");
const AGENTBOOT_EC716_UART: &[u8] = include_bytes!("../agentboot/ec716_uart.bin");
const AGENTBOOT_EC718_USB: &[u8] = include_bytes!("../agentboot/ec718_usb.bin");
const AGENTBOOT_EC718_UART: &[u8] = include_bytes!("../agentboot/ec718_uart.bin");
const AGENTBOOT_EC718M_USB: &[u8] = include_bytes!("../agentboot/ec718m_usb.bin");
const AGENTBOOT_EC718M_UART: &[u8] = include_bytes!("../agentboot/ec718m_uart.bin");
const AGENTBOOT_EC217_USB: &[u8] = include_bytes!("../agentboot/ec217_usb.bin");
const AGENTBOOT_EC217_UART: &[u8] = include_bytes!("../agentboot/ec217_uart.bin");

/// Map a LuatOS package/product name to the curated AgentBoot family.
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
        bail!(
            "Unable to determine AgentBoot family from package product: {}",
            chip_name
        )
    }
}

/// Return the curated AgentBoot bytes for a package product and transport.
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
        _ => bail!("Unknown AgentBoot family: {}", family),
    };

    log::info!(
        "Package product {} -> AgentBoot {}_{:?}",
        chip_name,
        family,
        port_type
    );
    Ok(data)
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
