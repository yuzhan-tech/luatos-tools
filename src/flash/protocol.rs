use super::consts::*;

/// DL command structure (8 bytes).
#[derive(Debug, Clone)]
pub struct Cmd {
    pub cmd: u8,
    pub index: u8,
    pub order_id: u8,
    pub norder_id: u8,
    pub len: u32,
}

impl Cmd {
    pub fn new(cmd_id: u8) -> Self {
        Cmd {
            cmd: cmd_id,
            index: 0,
            order_id: DL_COMMAND_ID,
            norder_id: DL_COMMAND_ID_INV,
            len: 0,
        }
    }

    pub fn pack(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(8);
        buf.push(self.cmd);
        buf.push(self.index);
        buf.push(self.order_id);
        buf.push(self.norder_id);
        buf.extend_from_slice(&self.len.to_le_bytes());
        buf
    }
}

/// DL response structure (6 bytes).
#[derive(Debug)]
pub struct Rsp {
    pub cmd: u8,
    pub index: u8,
    pub order_id: u8,
    pub norder_id: u8,
    pub state: u8,
    pub len: u8,
}

impl Rsp {
    pub fn unpack(data: &[u8]) -> Self {
        Rsp {
            cmd: data[0],
            index: data[1],
            order_id: data[2],
            norder_id: data[3],
            state: data[4],
            len: data[5],
        }
    }
}

/// LPC command structure (8 bytes, same layout as Cmd but different IDs).
#[derive(Debug, Clone)]
pub struct LpcCmd {
    pub cmd: u8,
    pub index: u8,
    pub order_id: u8,
    pub norder_id: u8,
    pub len: u32,
}

impl LpcCmd {
    pub fn new(cmd_id: u8) -> Self {
        LpcCmd {
            cmd: cmd_id,
            index: 0,
            order_id: LPC_COMMAND_ID,
            norder_id: LPC_COMMAND_ID_INV,
            len: 0,
        }
    }

    pub fn pack(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(8);
        buf.push(self.cmd);
        buf.push(self.index);
        buf.push(self.order_id);
        buf.push(self.norder_id);
        buf.extend_from_slice(&self.len.to_le_bytes());
        buf
    }
}

/// Version info structure (16 bytes).
#[derive(Debug)]
pub struct VersionInfo {
    pub v_val: u32,
    pub id: u32,
    pub dtm: u32,
    pub rsvd: u32,
}

impl VersionInfo {
    pub fn unpack(data: &[u8]) -> Self {
        VersionInfo {
            v_val: u32::from_le_bytes(data[0..4].try_into().unwrap()),
            id: u32::from_le_bytes(data[4..8].try_into().unwrap()),
            dtm: u32::from_le_bytes(data[8..12].try_into().unwrap()),
            rsvd: u32::from_le_bytes(data[12..16].try_into().unwrap()),
        }
    }
}

/// Image header structure for firmware download.
pub struct ImgHead {
    data: Vec<u8>,
}

impl ImgHead {
    /// Size of the packed image header.
    // VersionInfo(16) + imgnum(4) + CtlInfo(4) + rsvd0(4) + rsvd1(4) +
    // hashih(32) + ImgBody(4+4+4+4+16+32+64+64=192) + ReservedArea(4+4+8=16) = 272
    pub const SIZE: usize = 272;

    /// Create a new image header with default values.
    pub fn new() -> Self {
        let mut data = vec![0u8; Self::SIZE];

        // VersionInfo.vVal = 0x10000001
        data[0..4].copy_from_slice(&0x10000001u32.to_le_bytes());
        // VersionInfo.id = IMGH_IDENTIFIER
        data[4..8].copy_from_slice(&IMGH_IDENTIFIER.to_le_bytes());
        // VersionInfo.dtm = 0x20180507
        data[8..12].copy_from_slice(&0x20180507u32.to_le_bytes());
        // imgnum = 1
        data[16..20].copy_from_slice(&1u32.to_le_bytes());
        // ctlinfo.hashtype = 0xee
        data[20] = 0xee;
        // ImgBody.id = AGBT_IDENTIFIER (at offset 64: after verinfo+imgnum+ctlinfo+rsvd0+rsvd1+hashih)
        let body_offset = 16 + 4 + 4 + 4 + 4 + 32; // = 64
        data[body_offset..body_offset + 4].copy_from_slice(&AGBT_IDENTIFIER.to_le_bytes());
        // ImgBody.ldloc = 0x04010000
        data[body_offset + 8..body_offset + 12].copy_from_slice(&0x04010000u32.to_le_bytes());

        ImgHead { data }
    }

    // Field offsets
    const CTLINFO_OFFSET: usize = 20; // After VersionInfo(16) + imgnum(4)
    const RSVD0_OFFSET: usize = 24; // After CtlInfo(4)
    const HASHIH_OFFSET: usize = 32; // After rsvd0(4) + rsvd1(4)
    const BODY_OFFSET: usize = 64; // After hashih(32)

    pub fn set_body_id(&mut self, id: u32) {
        self.data[Self::BODY_OFFSET..Self::BODY_OFFSET + 4].copy_from_slice(&id.to_le_bytes());
    }

    pub fn set_burn_addr(&mut self, addr: u32) {
        let off = Self::BODY_OFFSET + 4;
        self.data[off..off + 4].copy_from_slice(&addr.to_le_bytes());
    }

    pub fn set_img_size(&mut self, size: u32) {
        let off = Self::BODY_OFFSET + 12;
        self.data[off..off + 4].copy_from_slice(&size.to_le_bytes());
    }

    pub fn set_hashv(&mut self, hash: &[u8; 32]) {
        let off = Self::BODY_OFFSET + 32; // After id(4) + burnaddr(4) + ldloc(4) + img_size(4) + reserve(16)
        self.data[off..off + 32].copy_from_slice(hash);
    }

    pub fn set_baudrate_ctrl(&mut self, baud: u32) {
        let ctrl = if baud != 0 {
            ((baud / 100) + 0x8000) as u16
        } else {
            0
        };
        let off = Self::CTLINFO_OFFSET + 2; // baudratectrl is at offset 2 within CtlInfo
        self.data[off..off + 2].copy_from_slice(&ctrl.to_le_bytes());
    }

    pub fn set_hashtype(&mut self, hashtype: u8) {
        self.data[Self::CTLINFO_OFFSET] = hashtype;
    }

    pub fn set_rsvd0(&mut self, val: u32) {
        self.data[Self::RSVD0_OFFSET..Self::RSVD0_OFFSET + 4].copy_from_slice(&val.to_le_bytes());
    }

    pub fn set_hashih(&mut self, hash: &[u8; 32]) {
        self.data[Self::HASHIH_OFFSET..Self::HASHIH_OFFSET + 32].copy_from_slice(hash);
    }

    /// Compute and set the header's own hash (hashih field).
    pub fn finalize_hash(&mut self) {
        use sha2::{Digest, Sha256};
        let hash: [u8; 32] = Sha256::digest(&self.data).into();
        self.set_hashih(&hash);
    }

    pub fn pack(&self) -> &[u8] {
        &self.data
    }
}
