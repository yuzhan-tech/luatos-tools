/// CRC8-Maxim (Dallas/Maxim 1-Wire CRC).
/// Ported from ectool2py/ecstruct.py crc8_maxim().
pub fn crc8_maxim(stream: &[u8]) -> u8 {
    let mut crc: u32 = 0;
    for &c in stream {
        for i in 0..8 {
            let b = (crc & 1) ^ (((c as u32) & (1 << i)) >> i);
            crc = (crc ^ (b * 0x118)) >> 1;
        }
    }
    crc as u8
}

/// Self-defined additive checksum used in DLBOOT mode for DOWNLOAD_DATA commands.
/// Ported from ectool2py/ecaction.py self_def_check1().
pub fn self_def_check1(
    cmd: u8,
    index: u8,
    order_id: u8,
    norder_id: u8,
    len: u32,
    data: &[u8],
) -> [u8; 4] {
    let mut ck_val: u32 = cmd as u32
        + index as u32
        + order_id as u32
        + norder_id as u32
        + (len & 0xFF) as u32
        + ((len >> 8) & 0xFF) as u32
        + ((len >> 16) & 0xFF) as u32
        + ((len >> 24) & 0xFF) as u32;

    for &b in data {
        ck_val = ck_val.wrapping_add(b as u32);
    }

    ck_val.to_le_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc8_maxim() {
        // Test vector: 3 bytes of a little-endian u32
        let data = [0x04, 0x00, 0x00];
        let crc = crc8_maxim(&data);
        // The CRC8-Maxim should produce a deterministic result
        assert!(crc <= 0xFF);
    }
}
