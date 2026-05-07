/// A file entry to pack into the luadb container.
pub struct LuadbEntry {
    pub filename: String,
    pub data: Vec<u8>,
}

/// Luadb entry magic: tag=0x01 len=0x04 value=0x5AA55AA5.
const LUADB_MAGIC: &[u8] = &[0x01, 0x04, 0x5A, 0xA5, 0x5A, 0xA5];
const LUADB_ALL_CRC_NAME: &str = ".airm2m_all_crc#.bin";

fn sum_check(data: &[u8]) -> u16 {
    data.iter()
        .fold(0u16, |acc, byte| acc.wrapping_add(*byte as u16))
}

fn append_checked_header(out: &mut Vec<u8>, header_without_crc: &[u8]) {
    out.extend_from_slice(header_without_crc);
    out.extend_from_slice(&sum_check(header_without_crc).to_le_bytes());
}

fn entry_header(filename: &str, data_len: usize) -> Vec<u8> {
    let filename_len = filename.len();
    assert!(
        filename_len <= u8::MAX as usize,
        "luadb filename is too long: {}",
        filename
    );

    let mut header = Vec::with_capacity(6 + 2 + filename_len + 6 + 2);
    header.extend_from_slice(LUADB_MAGIC);
    header.push(0x02);
    header.push(filename_len as u8);
    header.extend_from_slice(filename.as_bytes());
    header.extend_from_slice(&[0x03, 0x04]);
    header.extend_from_slice(&(data_len as u32).to_le_bytes());
    header.extend_from_slice(&[0xFE, 0x02]);
    header
}

/// Pack file entries into luadb format (script.bin).
///
/// Format reference: https://wiki.luatos.com/develop/contribute/luadb.html
pub fn pack_luadb(entries: &[LuadbEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    let file_count = (entries.len() + 1) as u16;

    let mut global_header = Vec::with_capacity(22);
    global_header.extend_from_slice(LUADB_MAGIC);
    global_header.extend_from_slice(&[0x02, 0x02]);
    global_header.extend_from_slice(&2u16.to_le_bytes());
    global_header.extend_from_slice(&[0x03, 0x04]);
    global_header.extend_from_slice(&0x18u32.to_le_bytes());
    global_header.extend_from_slice(&[0x04, 0x02]);
    global_header.extend_from_slice(&file_count.to_le_bytes());
    global_header.extend_from_slice(&[0xFE, 0x02]);
    append_checked_header(&mut out, &global_header);

    for entry in entries {
        append_checked_header(&mut out, &entry_header(&entry.filename, entry.data.len()));
        out.extend_from_slice(&entry.data);
    }

    append_checked_header(&mut out, &entry_header(LUADB_ALL_CRC_NAME, 16));
    let digest = md5::compute(&out);
    out.extend_from_slice(&digest.0);

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_u16(data: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap())
    }

    fn read_u32(data: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
    }

    #[test]
    fn writes_official_v2_header_checksums() {
        let packed = pack_luadb(&[LuadbEntry {
            filename: "main.luac".to_string(),
            data: vec![1, 2, 3],
        }]);

        assert_eq!(&packed[0..6], LUADB_MAGIC);
        assert_eq!(read_u16(&packed, 8), 2);
        assert_eq!(read_u32(&packed, 12), 0x18);
        assert_eq!(read_u16(&packed, 18), 2);
        assert_eq!(read_u16(&packed, 22), sum_check(&packed[0..22]));

        let first_entry = 24;
        let first_header_len = 6 + 2 + "main.luac".len() + 6 + 2;
        assert_eq!(
            read_u16(&packed, first_entry + first_header_len),
            sum_check(&packed[first_entry..first_entry + first_header_len])
        );
    }

    #[test]
    fn appends_all_crc_md5_entry() {
        let packed = pack_luadb(&[LuadbEntry {
            filename: "main.luac".to_string(),
            data: vec![1, 2, 3],
        }]);

        let crc_name = LUADB_ALL_CRC_NAME.as_bytes();
        let name_pos = packed
            .windows(crc_name.len())
            .position(|window| window == crc_name)
            .expect("all-crc entry name");
        let entry_start = name_pos - 8;
        let header_len = 6 + 2 + crc_name.len() + 6 + 2;
        let checksum_pos = entry_start + header_len;
        let md5_pos = checksum_pos + 2;

        assert_eq!(read_u32(&packed, name_pos + crc_name.len() + 2), 16);
        assert_eq!(
            read_u16(&packed, checksum_pos),
            sum_check(&packed[entry_start..checksum_pos])
        );
        assert_eq!(packed.len(), md5_pos + 16);
        assert_eq!(&packed[md5_pos..], &md5::compute(&packed[..md5_pos]).0);
    }
}
