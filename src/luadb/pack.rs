/// A file entry to pack into the luadb container.
pub struct LuadbEntry {
    pub filename: String,
    pub data: Vec<u8>,
}

/// Luadb entry magic: tag=0x01 len=0x04 value=0x5AA55AA5.
const LUADB_MAGIC: &[u8] = &[0x01, 0x04, 0x5A, 0xA5, 0x5A, 0xA5];

/// Pack file entries into luadb format (script.bin).
///
/// Format reference: https://wiki.luatos.com/develop/contribute/luadb.html
pub fn pack_luadb(entries: &[LuadbEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    let file_count = entries.len() as u16;

    // === Global Header ===
    // Magic: tag=0x01 len=0x04 value=0x5AA55AA5
    out.extend_from_slice(LUADB_MAGIC);
    // Version: tag=0x02 len=0x02 value=0x0002
    out.extend_from_slice(&[0x02, 0x02, 0x02, 0x00]);
    // Head length: tag=0x03 len=0x04 value=18 (0x12)
    out.extend_from_slice(&[0x03, 0x04, 0x12, 0x00, 0x00, 0x00]);
    // File count: tag=0x04 len=0x02 value=LE u16
    out.extend_from_slice(&[0x04, 0x02, file_count as u8, (file_count >> 8) as u8]);
    // CRC (unused): tag=0xFE len=0x02 value=0xFFFF
    out.extend_from_slice(&[0xFE, 0x02, 0xFF, 0xFF]);

    // === Per-file entries ===
    for entry in entries {
        // Magic
        out.extend_from_slice(&[0x01, 0x04, 0x5A, 0xA5, 0x5A, 0xA5]);
        // Filename length: tag=0x02 len=N
        out.push(0x02);
        out.push(entry.filename.len() as u8);
        out.extend_from_slice(entry.filename.as_bytes());
        // File size: tag=0x03 len=0x04 value=LE u32
        let size = entry.data.len() as u32;
        out.extend_from_slice(&[
            0x03,
            0x04,
            size as u8,
            (size >> 8) as u8,
            (size >> 16) as u8,
            (size >> 24) as u8,
        ]);
        // CRC (unused)
        out.extend_from_slice(&[0xFE, 0x02, 0xFF, 0xFF]);
        // File contents
        out.extend_from_slice(&entry.data);
    }

    out
}
