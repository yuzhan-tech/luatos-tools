use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

use super::binpkg::{parse_binpkg, rehash_entry, serialize_binpkg, BinpkgEntry, BinpkgResult};
use super::info::parse_info_json;

/// Parse a SOC file (7z archive) containing binpkg, info.json, and optional script.bin.
///
/// If `keep_data` is true, image data is kept in memory for burning.
pub fn parse_soc(path: &Path, keep_data: bool) -> Result<BinpkgResult> {
    let tmpdir = tempfile::tempdir().context("Failed to create temp directory")?;
    let tmppath = tmpdir.path();

    sevenz_rust2::decompress_file(path, tmppath)
        .with_context(|| format!("Failed to extract SOC file: {}", path.display()))?;

    let mut binpkg_data: Option<Vec<u8>> = None;
    let mut info_json_data: Option<Vec<u8>> = None;
    let mut script_data: Option<(String, Vec<u8>)> = None;

    for entry in fs::read_dir(tmppath).context("Failed to read temp directory")? {
        let entry = entry?;
        let fname = entry.file_name().to_string_lossy().to_string();
        let fpath = entry.path();

        if fname.ends_with(".binpkg") {
            binpkg_data = Some(fs::read(&fpath)?);
        } else if fname.ends_with("script.bin") {
            let data = fs::read(&fpath)?;
            script_data = Some((fname, data));
        } else if fname == "info.json" {
            info_json_data = Some(fs::read(&fpath)?);
        }
    }

    let binpkg_bytes =
        binpkg_data.ok_or_else(|| anyhow::anyhow!("No .binpkg file found in SOC archive"))?;
    let mut result = parse_binpkg(&binpkg_bytes, keep_data)?;

    // Handle script.bin
    if let Some((_fname, sdata)) = script_data {
        let hash = hex::encode(Sha256::digest(&sdata));
        let mut script_entry = BinpkgEntry {
            name: "script".to_string(),
            addr: 0,
            flash_size: 0,
            offset: 0,
            image_size: sdata.len() as u32,
            hash,
            image_type: "AP".to_string(),
            vt: 0,
            vtsize: 0,
            rsvd: 0,
            pdata: 0,
            data: if keep_data { Some(sdata) } else { None },
        };

        // Get burn_addr from info.json
        if let Some(ref info_bytes) = info_json_data {
            if let Ok(info) = parse_info_json(info_bytes) {
                if let Some(download) = &info.download {
                    if let Some(ref script_addr) = download.script_addr {
                        if let Ok(addr) = u32::from_str_radix(script_addr, 16) {
                            script_entry.addr = addr;
                        }
                    }
                }
            }
        }

        result.entries.push(script_entry);
    }

    // Read force_br from info.json
    if let Some(ref info_bytes) = info_json_data {
        if let Ok(info) = parse_info_json(info_bytes) {
            if let Some(download) = &info.download {
                if let Some(ref br) = download.force_br {
                    if let Ok(v) = br.parse::<u32>() {
                        result.force_br = Some(v);
                    }
                }
            }
        }
    }

    Ok(result)
}

/// Metadata read from a SOC file.
pub struct SocMetadata {
    pub script_addr: u32,
    pub chip: String,
    pub force_br: Option<u32>,
    pub script_bitw: u32,
}

/// Read script_addr, chip, force_br, and script bitness from a SOC file.
/// Chip name comes from binpkg header (specific, e.g. "ec718m"),
/// script_addr, force_br, and script.bitw come from info.json.
pub fn read_soc_metadata(path: &Path) -> Result<SocMetadata> {
    let tmpdir = tempfile::tempdir().context("Failed to create temp directory")?;
    let tmppath = tmpdir.path();

    sevenz_rust2::decompress_file(path, tmppath)
        .with_context(|| format!("Failed to extract SOC file: {}", path.display()))?;

    // Read chip name from binpkg header (more specific than info.json's chip.type)
    let mut chip: Option<String> = None;
    for entry in fs::read_dir(tmppath)? {
        let entry = entry?;
        let fname = entry.file_name().to_string_lossy().to_string();
        if fname.ends_with(".binpkg") {
            let fdata = fs::read(entry.path())?;
            let result = parse_binpkg(&fdata, false)?;
            chip = Some(result.chip);
            break;
        }
    }
    let chip = chip.ok_or_else(|| anyhow::anyhow!("No chip found in SOC binpkg header"))?;
    if chip == super::binpkg::UNKNOWN_CHIP {
        bail!("Unable to determine chip type from SOC binpkg header");
    }

    // Read script_addr and force_br from info.json
    let info_path = tmppath.join("info.json");
    let info_bytes = fs::read(&info_path).context("No info.json in SOC")?;
    let info = parse_info_json(&info_bytes)?;

    let download = info
        .download
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No download section in info.json"))?;

    let script_addr_str = download
        .script_addr
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No script_addr in info.json"))?;
    let script_addr = u32::from_str_radix(script_addr_str, 16)
        .with_context(|| format!("Invalid script_addr: {}", script_addr_str))?;

    let force_br = download
        .force_br
        .as_ref()
        .and_then(|s| s.parse::<u32>().ok());
    let script_bitw = info
        .script
        .as_ref()
        .and_then(|s| s.bitw)
        .ok_or_else(|| anyhow::anyhow!("No script.bitw in info.json"))?;

    Ok(SocMetadata {
        script_addr,
        chip,
        force_br,
        script_bitw,
    })
}

/// Parse either a .soc or .binpkg file based on extension.
pub fn parse_package(path: &Path, keep_data: bool) -> Result<BinpkgResult> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    match ext {
        "soc" => parse_soc(path, keep_data),
        "binpkg" => {
            let fdata = fs::read(path)
                .with_context(|| format!("Failed to read binpkg: {}", path.display()))?;
            parse_binpkg(&fdata, keep_data)
        }
        _ => bail!("Unknown package format: {} (expected .soc or .binpkg)", ext),
    }
}

/// Generate a .soc file by replacing script.bin in the base .soc archive.
///
/// Streams entries from the base SOC directly into the output archive,
/// only keeping essential files (binpkg, info.json) and replacing script.bin.
/// Skips large debug artifacts (.elf, .map, etc.) without decompressing them to disk.
pub fn gen_soc(base_soc: &Path, script_bin: &[u8], output: &Path) -> Result<()> {
    use std::io::Cursor;

    log::info!("Packing SOC...");
    let mut reader = sevenz_rust2::ArchiveReader::open(base_soc, sevenz_rust2::Password::empty())
        .with_context(|| format!("Failed to open base SOC: {}", base_soc.display()))?;

    let mut writer = sevenz_rust2::ArchiveWriter::create(output)
        .with_context(|| format!("Failed to create SOC: {}", output.display()))?;
    writer.set_content_methods(vec![
        sevenz_rust2::encoder_options::Lzma2Options::from_level(1).into(),
    ]);

    let mut found_script = false;

    reader
        .for_each_entries(|entry, rd| {
            let name = entry.name().to_string();

            if name.ends_with("script.bin") {
                // Replace with new script.bin
                let mut archive_entry = sevenz_rust2::ArchiveEntry::default();
                archive_entry.name = name.clone();
                writer
                    .push_archive_entry(archive_entry, Some(Cursor::new(script_bin)))
                    .map_err(|e| {
                        sevenz_rust2::Error::from(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            e.to_string(),
                        ))
                    })?;
                found_script = true;
                log::info!("  {} (replaced, {} bytes)", name, script_bin.len());
            } else if !entry.is_directory() {
                // Copy entry as-is
                let mut data = Vec::new();
                rd.read_to_end(&mut data)?;
                let mut archive_entry = sevenz_rust2::ArchiveEntry::default();
                archive_entry.name = name.clone();
                writer
                    .push_archive_entry(archive_entry, Some(Cursor::new(data)))
                    .map_err(|e| {
                        sevenz_rust2::Error::from(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            e.to_string(),
                        ))
                    })?;
                log::info!("  {}", name);
            }

            Ok(true)
        })
        .with_context(|| "Failed to read base SOC entries")?;

    if !found_script {
        let mut archive_entry = sevenz_rust2::ArchiveEntry::default();
        archive_entry.name = "script.bin".to_string();
        writer
            .push_archive_entry(archive_entry, Some(Cursor::new(script_bin)))
            .with_context(|| "Failed to add script.bin")?;
        log::info!("  script.bin (added, {} bytes)", script_bin.len());
    }

    writer
        .finish()
        .with_context(|| "Failed to finish SOC archive")?;

    Ok(())
}

/// Generate a production .binpkg by patching script.bin into the AP image.
pub fn gen_production_binpkg(base_soc: &Path, script_bin: &[u8], output: &Path) -> Result<()> {
    let tmpdir = tempfile::tempdir().context("Failed to create temp directory")?;
    let tmppath = tmpdir.path();

    log::info!("Extracting base SOC...");
    sevenz_rust2::decompress_file(base_soc, tmppath)
        .with_context(|| format!("Failed to extract base SOC: {}", base_soc.display()))?;

    let mut binpkg_data: Option<Vec<u8>> = None;
    let mut info_json_data: Option<Vec<u8>> = None;

    for entry in fs::read_dir(tmppath)? {
        let entry = entry?;
        let fname = entry.file_name().to_string_lossy().to_string();
        let fpath = entry.path();

        if fname.ends_with(".binpkg") {
            binpkg_data = Some(fs::read(&fpath)?);
        } else if fname == "info.json" {
            info_json_data = Some(fs::read(&fpath)?);
        }
    }

    let binpkg_bytes = binpkg_data.ok_or_else(|| anyhow::anyhow!("No .binpkg in base SOC"))?;
    let info_bytes = info_json_data.ok_or_else(|| anyhow::anyhow!("No info.json in base SOC"))?;

    let mut result = parse_binpkg(&binpkg_bytes, true)?;
    let info = parse_info_json(&info_bytes)?;

    let download = info
        .download
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No download section in info.json"))?;

    let script_addr = download
        .script_addr
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No script_addr in info.json"))?;
    let script_addr = u32::from_str_radix(script_addr, 16)
        .with_context(|| format!("Invalid script_addr: {}", script_addr))?;

    // Find AP entry and calculate patch offset.
    // The binpkg AP addr includes a 0x800000 bias for flash addressing.
    // The script_addr from info.json is the raw flash address.
    // Offset within AP image = script_addr - (ap_binpkg_addr - 0x800000)
    let ap_entry = result
        .entries
        .iter()
        .find(|e| e.image_type == "AP" && e.name != "script")
        .ok_or_else(|| anyhow::anyhow!("No AP entry in binpkg"))?;
    let mut ap_flash_addr = ap_entry.addr;
    if ap_flash_addr >= 0x800000 {
        ap_flash_addr -= 0x800000;
    }

    let patch_offset = (script_addr - ap_flash_addr) as usize;
    let required_size = patch_offset + script_bin.len();

    log::info!(
        "Production binpkg: AP flash=0x{:X}, script=0x{:X}, offset=0x{:X}, required={}",
        ap_flash_addr,
        script_addr,
        patch_offset,
        required_size
    );

    // Get mutable AP entry and patch. Extend with 0xFF if needed.
    let ap_entry = result
        .entries
        .iter_mut()
        .find(|e| e.image_type == "AP" && e.name != "script")
        .unwrap();

    let ap_data = ap_entry
        .data
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("AP entry has no data"))?;

    if required_size > ap_data.len() {
        ap_data.resize(required_size, 0xFF);
    }

    ap_data[patch_offset..patch_offset + script_bin.len()].copy_from_slice(script_bin);

    // Recalculate hash and size
    let ap_entry = result
        .entries
        .iter_mut()
        .find(|e| e.image_type == "AP" && e.name != "script")
        .unwrap();
    rehash_entry(ap_entry);

    log::info!("Writing production binpkg...");
    let out_data = serialize_binpkg(&result);
    fs::write(output, &out_data)
        .with_context(|| format!("Failed to write binpkg: {}", output.display()))?;

    Ok(())
}
