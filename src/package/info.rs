use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct InfoJson {
    pub version: Option<u32>,
    pub chip: Option<ChipInfo>,
    pub rom: Option<RomInfo>,
    pub script: Option<ScriptInfo>,
    pub download: Option<DownloadInfo>,
    pub fota: Option<FotaInfo>,
    pub fs: Option<FsInfo>,
    pub user: Option<UserInfo>,
}

#[derive(Debug, Deserialize)]
pub struct ChipInfo {
    #[serde(rename = "type")]
    pub chip_type: Option<String>,
    pub ram: Option<RamInfo>,
}

#[derive(Debug, Deserialize)]
pub struct RamInfo {
    pub total: Option<u32>,
    pub sys: Option<u32>,
    pub lua: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct RomInfo {
    pub file: Option<String>,
    pub fs: Option<serde_json::Value>,
    #[serde(rename = "version-core")]
    pub version_core: Option<String>,
    #[serde(rename = "version-bsp")]
    pub version_bsp: Option<String>,
    pub mark: Option<String>,
    pub build: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ScriptInfo {
    pub file: Option<String>,
    pub lua: Option<String>,
    pub bitw: Option<u32>,
    #[serde(rename = "use-luac")]
    pub use_luac: Option<bool>,
    #[serde(rename = "use-debug")]
    pub use_debug: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct DownloadInfo {
    pub bl_addr: Option<String>,
    pub partition_addr: Option<String>,
    pub core_addr: Option<String>,
    pub app_addr: Option<String>,
    pub script_addr: Option<String>,
    pub nvm_addr: Option<String>,
    pub fs_addr: Option<String>,
    pub force_br: Option<String>,
    pub extra_param: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FotaInfo {
    pub magic_num: Option<String>,
    pub block_len: Option<String>,
    pub core_type: Option<String>,
    pub ap_type: Option<String>,
    pub cp_type: Option<String>,
    pub full_addr: Option<String>,
    pub fota_len: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FsInfo {
    pub total_len: Option<u32>,
    pub format_len: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UserInfo {
    pub project: Option<String>,
    pub version: Option<String>,
    pub log_br: Option<String>,
}

pub fn parse_info_json(data: &[u8]) -> anyhow::Result<InfoJson> {
    let info: InfoJson = serde_json::from_slice(data)?;
    Ok(info)
}
