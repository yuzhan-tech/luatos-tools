use anyhow::{bail, Result};
use sha2::{Digest, Sha256};

pub const UNKNOWN_CHIP: &str = "unknown";

/// A single entry parsed from a binpkg file.
#[derive(Debug, Clone)]
pub struct BinpkgEntry {
    pub name: String,
    pub addr: u32,
    pub flash_size: u32,
    pub offset: u32,
    pub image_size: u32,
    pub hash: String,
    pub image_type: String,
    pub vt: u16,
    pub vtsize: u16,
    pub rsvd: u32,
    pub pdata: u32,
    pub data: Option<Vec<u8>>,
}

/// Result of parsing a binpkg file.
#[derive(Debug)]
pub struct BinpkgResult {
    pub chip: String,
    /// Raw header bytes before the first entry (preserved for serialization).
    pub raw_header: Vec<u8>,
    /// Entries in order as they appear in the binpkg.
    pub entries: Vec<BinpkgEntry>,
    /// Forced baud rate from info.json (download.force_br), if available.
    pub force_br: Option<u32>,
}

impl BinpkgResult {
    /// Find an entry by name.
    pub fn find_entry(&self, name: &str) -> Option<&BinpkgEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Find a mutable entry by name.
    pub fn find_entry_mut(&mut self, name: &str) -> Option<&mut BinpkgEntry> {
        self.entries.iter_mut().find(|e| e.name == name)
    }
}

/// Magic string at offset 0x38 identifying the new pkgmode binpkg format.
const PKGMODE_MAGIC: &[u8] = b"pkgmode";

/// Entry metadata size in the binpkg format.
/// struct.unpack("64sIIII256s16sHHII") = 64+4+4+4+4+256+16+2+2+4+4 = 364
const ENTRY_META_SIZE: usize = 364;

/// Parse a binpkg binary blob.
///
/// If `keep_data` is true, each entry's `data` field will contain the image bytes.
pub fn parse_binpkg(fdata: &[u8], keep_data: bool) -> Result<BinpkgResult> {
    let fsize = fdata.len();
    if fsize < 0x34 {
        bail!("binpkg data too small ({} bytes)", fsize);
    }

    let foffset: usize;
    let chip_name: String;

    // Detect format: pkgmode vs legacy
    if fsize > 0x3F && &fdata[0x38..0x3F] == PKGMODE_MAGIC {
        // New pkgmode format
        foffset = 0x1D8;
        let raw = &fdata[0x190..std::cmp::min(0x1A0, fsize)];
        chip_name = raw
            .split(|&b| b == 0)
            .next()
            .map(|s| String::from_utf8_lossy(s).to_string())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| UNKNOWN_CHIP.to_string());
    } else {
        // Legacy format: 52-byte header
        foffset = 0x34;
        chip_name = UNKNOWN_CHIP.to_string();
    }

    let raw_header = fdata[..foffset].to_vec();
    let mut entries = Vec::new();
    let mut cursor = foffset;

    while cursor + ENTRY_META_SIZE <= fsize {
        let meta = &fdata[cursor..cursor + ENTRY_META_SIZE];

        let name_raw = &meta[0..64];
        let name = name_raw
            .split(|&b| b == 0)
            .next()
            .map(|s| String::from_utf8_lossy(s).to_string())
            .unwrap_or_default();

        let addr = u32::from_le_bytes(meta[64..68].try_into().unwrap());
        let flash_size = u32::from_le_bytes(meta[68..72].try_into().unwrap());
        let offset = u32::from_le_bytes(meta[72..76].try_into().unwrap());
        let img_size = u32::from_le_bytes(meta[76..80].try_into().unwrap());

        let hash_raw = &meta[80..336];
        let hash = hash_raw
            .split(|&b| b == 0)
            .next()
            .map(|s| String::from_utf8_lossy(s).to_string().to_lowercase())
            .unwrap_or_default();

        let img_type_raw = &meta[336..352];
        let image_type = img_type_raw
            .split(|&b| b == 0)
            .next()
            .map(|s| String::from_utf8_lossy(s).to_string())
            .unwrap_or_default();

        let vt = u16::from_le_bytes(meta[352..354].try_into().unwrap());
        let vtsize = u16::from_le_bytes(meta[354..356].try_into().unwrap());
        let rsvd = u32::from_le_bytes(meta[356..360].try_into().unwrap());
        let pdata = u32::from_le_bytes(meta[360..364].try_into().unwrap());

        cursor += ENTRY_META_SIZE;

        let data = if keep_data && cursor + (img_size as usize) <= fsize {
            Some(fdata[cursor..cursor + img_size as usize].to_vec())
        } else {
            None
        };

        log::debug!("{}", name);

        entries.push(BinpkgEntry {
            name,
            addr,
            flash_size,
            offset,
            image_size: img_size,
            hash,
            image_type,
            vt,
            vtsize,
            rsvd,
            pdata,
            data,
        });

        cursor += img_size as usize;
    }

    Ok(BinpkgResult {
        chip: chip_name,
        raw_header,
        entries,
        force_br: None,
    })
}

/// Serialize a BinpkgResult back to binary format.
pub fn serialize_binpkg(result: &BinpkgResult) -> Vec<u8> {
    let mut out = result.raw_header.clone();

    for entry in &result.entries {
        // 64-byte name
        let mut name_buf = [0u8; 64];
        let name_bytes = entry.name.as_bytes();
        let copy_len = name_bytes.len().min(63);
        name_buf[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
        out.extend_from_slice(&name_buf);

        out.extend_from_slice(&entry.addr.to_le_bytes());
        out.extend_from_slice(&entry.flash_size.to_le_bytes());
        out.extend_from_slice(&entry.offset.to_le_bytes());
        out.extend_from_slice(&entry.image_size.to_le_bytes());

        // 256-byte hash
        let mut hash_buf = [0u8; 256];
        let hash_bytes = entry.hash.as_bytes();
        let copy_len = hash_bytes.len().min(255);
        hash_buf[..copy_len].copy_from_slice(&hash_bytes[..copy_len]);
        out.extend_from_slice(&hash_buf);

        // 16-byte image_type
        let mut type_buf = [0u8; 16];
        let type_bytes = entry.image_type.as_bytes();
        let copy_len = type_bytes.len().min(15);
        type_buf[..copy_len].copy_from_slice(&type_bytes[..copy_len]);
        out.extend_from_slice(&type_buf);

        out.extend_from_slice(&entry.vt.to_le_bytes());
        out.extend_from_slice(&entry.vtsize.to_le_bytes());
        out.extend_from_slice(&entry.rsvd.to_le_bytes());
        out.extend_from_slice(&entry.pdata.to_le_bytes());

        // Image data
        if let Some(ref data) = entry.data {
            out.extend_from_slice(data);
        }
    }

    out
}

/// Recalculate the SHA256 hash for an entry's data.
pub fn rehash_entry(entry: &mut BinpkgEntry) {
    if let Some(ref data) = entry.data {
        entry.hash = hex::encode(Sha256::digest(data));
        entry.image_size = data.len() as u32;
    }
}
