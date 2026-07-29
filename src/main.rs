mod agentboot;
mod cli;
mod logs;
mod lua;
mod luadb;
mod package;
mod serial;

use anyhow::{bail, Context, Result};
use clap::Parser;
use ectool_core::{
    open_port, plan_binpkg_images, resolve_transfer_config, AgentBootConfig, BinpkgResult,
    FlashSession, FlashStorage, ImageKind, ImageTarget, PackageSelection, PortType,
    TransferOverrides,
};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::Path;

use agentboot::load_agentboot;
use cli::{Cli, Commands};
use lua::compiler::{compile_lua, init_lua_helper_cache};
use luadb::pack::{pack_luadb, LuadbEntry};
use serial::detect::{resolve_boot_port, resolve_log_port};

/// Add a single file to the entry list, compiling .lua files.
fn add_file_entry(
    path: &Path,
    entries: &mut Vec<LuadbEntry>,
    strip: bool,
    lua_bitw: u32,
) -> Result<()> {
    let filename = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("No filename for {}", path.display()))?
        .to_string_lossy()
        .to_string();
    let raw = fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;

    if filename.ends_with(".lua") {
        let chunk_name = format!("@{}", filename);
        match compile_lua(&raw, &chunk_name, strip, lua_bitw) {
            Ok(bytecode) => {
                let luac_name = filename.replace(".lua", ".luac");
                log::info!(
                    "Compiled {} -> {} ({} bytes, {}-bit)",
                    filename,
                    luac_name,
                    bytecode.len(),
                    lua_bitw
                );
                entries.push(LuadbEntry {
                    filename: luac_name,
                    data: bytecode,
                });
            }
            Err(e) => {
                bail!("Error compiling {}: {}", filename, e);
            }
        }
    } else {
        log::info!("Including {} ({} bytes)", filename, raw.len());
        entries.push(LuadbEntry {
            filename,
            data: raw,
        });
    }
    Ok(())
}

/// Recursively collect files from a directory, sorted by path.
fn collect_dir_recursive(
    dir: &Path,
    entries: &mut Vec<LuadbEntry>,
    strip: bool,
    lua_bitw: u32,
) -> Result<()> {
    let mut file_paths: Vec<std::path::PathBuf> = Vec::new();
    collect_files(dir, &mut file_paths)?;
    file_paths.sort();
    for path in file_paths {
        add_file_entry(&path, entries, strip, lua_bitw)?;
    }
    Ok(())
}

fn collect_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<()> {
    let mut children: Vec<_> = fs::read_dir(dir)
        .with_context(|| format!("Failed to read directory {}", dir.display()))?
        .filter_map(|e| e.ok())
        .collect();
    children.sort_by_key(|e| e.file_name());
    for entry in children {
        let ft = entry
            .file_type()
            .unwrap_or_else(|_| fs::metadata(entry.path()).unwrap().file_type());
        if ft.is_dir() {
            collect_files(&entry.path(), out)?;
        } else if ft.is_file() {
            out.push(entry.path());
        }
    }
    Ok(())
}

/// Compile Lua files and pack into script.bin bytes.
/// Accepts a list of files and/or directories.
fn generate_script_bin(
    paths: &[std::path::PathBuf],
    strip: bool,
    lua_bitw: u32,
) -> Result<Vec<u8>> {
    if paths.is_empty() {
        bail!("No input paths specified");
    }

    let mut entries: Vec<LuadbEntry> = Vec::new();

    for input in paths {
        if input.is_dir() {
            collect_dir_recursive(input, &mut entries, strip, lua_bitw)?;
        } else if input.is_file() {
            add_file_entry(input, &mut entries, strip, lua_bitw)?;
        } else {
            bail!("{} is not a file or directory", input.display());
        }
    }

    if entries.is_empty() {
        bail!("No files found in {:?}", paths);
    }

    let bin = pack_luadb(&entries);
    log::info!("Packed {} files, {} bytes", entries.len(), bin.len());
    Ok(bin)
}

/// Strip control characters from a string, keeping newlines and tabs.
fn strip_control(s: &str) -> String {
    s.chars()
        .filter(|&c| !c.is_control() || c == '\n' || c == '\t')
        .collect()
}

/// Parse a log message into (level_char, module, body).
/// Expected format: "D/user.module message body" or "I/module body".
/// Strips "user." prefix from module names.
fn parse_log_parts(msg: &str) -> Option<(char, &str, &str)> {
    // Must start with a level letter followed by '/'
    let level = msg.chars().next()?;
    if !matches!(level, 'D' | 'I' | 'W' | 'E') {
        return None;
    }
    if msg.as_bytes().get(1) != Some(&b'/') {
        return None;
    }
    let after_slash = &msg[2..];
    // Module name ends at first space
    let space_pos = after_slash.find(' ')?;
    let module = &after_slash[..space_pos];
    let body = &after_slash[space_pos + 1..];
    Some((level, module, body))
}

/// Format a device tick (milliseconds) as `[SSSSSSSSS.mmm]`.
fn format_tick(tick_ms: u32) -> String {
    format!("[{:09}.{:03}]", tick_ms / 1000, tick_ms % 1000)
}

fn format_hex_bytes(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() * 2);
    for byte in data {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{:02X}", byte);
    }
    out
}

/// Print a log line with timestamp in a separate column.
/// Parses log level (D/I/W/E) and module name for colored output.
/// Continuation lines are indented to align with the content column.
fn print_log(timestamp: &str, msg: &str) {
    let clean = strip_control(msg);

    if let Some((level, module, body)) = parse_log_parts(&clean) {
        let color = match level {
            'D' => "\x1b[2m",  // dim
            'I' => "",         // normal
            'W' => "\x1b[33m", // yellow
            'E' => "\x1b[31m", // red
            _ => "",
        };
        let module_display = module;
        let plain_pad = timestamp.len() + 1 + 2 + module_display.len() + 1;

        let mut first = true;
        for line in body.split('\n') {
            if first {
                println!(
                    "{}{} {}/{} {}\x1b[0m",
                    color, timestamp, level, module_display, line
                );
                first = false;
            } else {
                println!("{}{:>pad$}{}\x1b[0m", color, "", line, pad = plain_pad);
            }
        }
        if first {
            println!("{}{} {}/{}\x1b[0m", color, timestamp, level, module_display);
        }
    } else {
        // Unparseable log line — print as-is
        let pad = timestamp.len() + 1;
        let mut first = true;
        for line in clean.split('\n') {
            if first {
                println!("\x1b[2m{} {}\x1b[0m", timestamp, line);
                first = false;
            } else {
                println!("\x1b[2m{:>pad$}{}\x1b[0m", "", line, pad = pad);
            }
        }
        if first {
            println!("{}", timestamp);
        }
    }
}

fn parse_port_type(s: &str) -> PortType {
    if s == "uart" {
        PortType::Uart
    } else {
        PortType::Usb
    }
}

fn progress_bar(total: u64, label: &str) -> ProgressBar {
    let progress = ProgressBar::new(total);
    progress.set_style(
        ProgressStyle::default_bar()
            .template(&format!(
                "  {{bar:40.cyan/blue}} {{percent:>3}}% {{pos:>7}}/{{len:7}} {label}"
            ))
            .expect("static progress template is valid")
            .progress_chars("##-"),
    );
    progress
}

fn start_flash_session(
    port_name: &str,
    port_type: PortType,
    product_name: &str,
    package: Option<&BinpkgResult>,
    force_br: Option<u32>,
) -> Result<FlashSession> {
    let resolved = resolve_transfer_config(
        package,
        port_type,
        TransferOverrides {
            // LuatOS info.json metadata is an application-owned override.
            agent_baud: force_br,
            ..TransferOverrides::default()
        },
    )?;
    let agent = load_agentboot(product_name, port_type)?;
    let port = open_port(port_name, port_type)?;

    log::info!(
        "Loading AgentBoot at {} baud (pullup_qspi={}, dribble_download={})",
        resolved.agent_baud,
        resolved.pullup_qspi,
        resolved.transfer.dribble_download
    );
    FlashSession::start(
        port,
        AgentBootConfig {
            data: agent,
            baud: resolved.agent_baud,
            pullup_qspi: resolved.pullup_qspi,
        },
        resolved.transfer,
    )
}

fn flash_with_progress(
    session: &mut FlashSession,
    target: ImageTarget<'_>,
    data: &[u8],
) -> Result<()> {
    let progress = progress_bar(data.len() as u64, target.tag);
    let mut update = |completed, _total| progress.set_position(completed);
    if let Err(error) = session.flash_image(target, data, Some(&mut update)) {
        progress.abandon_with_message(format!("{} FAILED", target.tag));
        return Err(error);
    }
    progress.finish_with_message(format!("{} done", target.tag));
    Ok(())
}

fn luatos_script_target(address: u32) -> Result<ImageTarget<'static>> {
    // Preserve the LuatOS FlexFile convention used by the existing tool: the
    // script address is sent in AP-flash XIP form.
    let address = if address < 0x0080_0000 {
        address
            .checked_add(0x0080_0000)
            .context("LuatOS script address overflow while applying XIP bias")?
    } else {
        address
    };
    Ok(ImageTarget {
        image_type: ImageKind::FlexFile,
        storage: FlashStorage::ApFlash,
        address,
        tag: "SCRIPT",
    })
}

fn normalize_lua_bitw(lua_bitw: u32) -> Result<u32> {
    match lua_bitw {
        32 | 64 => Ok(lua_bitw),
        _ => bail!("Unsupported Lua bitness: {} (expected 32 or 64)", lua_bitw),
    }
}

fn parse_u32_arg(value: &str, label: &str) -> Result<u32> {
    let trimmed = value.trim();
    let (digits, radix) = if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        (hex, 16)
    } else if trimmed.chars().any(|c| matches!(c, 'a'..='f' | 'A'..='F')) {
        (trimmed, 16)
    } else {
        (trimmed, 10)
    };

    u32::from_str_radix(digits, radix)
        .with_context(|| format!("Invalid {} value: {}", label, value))
}

fn read_base_image_metadata(base_image: &Path) -> Result<package::soc::SocMetadata> {
    let meta = package::soc::read_soc_metadata(base_image)?;
    normalize_lua_bitw(meta.script_bitw)?;
    Ok(meta)
}

fn resolve_script_lua_bitw(
    base_meta: Option<&package::soc::SocMetadata>,
    cli_lua_bitw: Option<u32>,
) -> Result<u32> {
    let cli_lua_bitw = cli_lua_bitw.map(normalize_lua_bitw).transpose()?;

    if let Some(meta) = base_meta {
        if let Some(cli_lua_bitw) = cli_lua_bitw {
            if cli_lua_bitw != meta.script_bitw {
                bail!(
                    "Requested Lua bitness {} does not match base image script.bitw {}",
                    cli_lua_bitw,
                    meta.script_bitw
                );
            }
        }
        Ok(meta.script_bitw)
    } else {
        Ok(cli_lua_bitw.unwrap_or(32))
    }
}

fn cmd_script(
    paths: &[std::path::PathBuf],
    output: &Option<std::path::PathBuf>,
    burn: bool,
    production: bool,
    lua_bitw: Option<u32>,
    base_image: &Option<std::path::PathBuf>,
    port: &str,
    port_type_str: &str,
) -> Result<()> {
    if output.is_none() && !burn {
        bail!("Either -o/--output or -b/--burn must be specified");
    }

    let base_meta = match base_image {
        Some(base_path) => Some(read_base_image_metadata(base_path)?),
        None => None,
    };
    let lua_bitw = resolve_script_lua_bitw(base_meta.as_ref(), lua_bitw)?;

    let bin = generate_script_bin(paths, production, lua_bitw)?;

    if let Some(ref out) = output {
        fs::write(out, &bin).with_context(|| format!("Failed to write {}", out.display()))?;
        log::info!("Generated {} ({} bytes)", out.display(), bin.len());
    }

    if burn {
        let base_path = base_image
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--base-image is required when using --burn"))?;
        let meta = base_meta
            .ok_or_else(|| anyhow::anyhow!("--base-image is required when using --burn"))?;
        let base_package = package::soc::parse_package(base_path, false)?;

        log::info!(
            "Burn script addr=0x{:X}, package product={}",
            meta.script_addr,
            meta.chip
        );

        let port_type = parse_port_type(port_type_str);
        let port_name = resolve_boot_port(port)?;
        let mut session = start_flash_session(
            &port_name,
            port_type,
            &meta.chip,
            Some(&base_package.binpkg),
            meta.force_br,
        )?;
        flash_with_progress(&mut session, luatos_script_target(meta.script_addr)?, &bin)?;
        session
            .finish_reset()
            .context("Script was written, but the final device reset failed")?;
        log::info!("burn script ok");
    }

    Ok(())
}

fn cmd_pack(
    paths: &[std::path::PathBuf],
    base_image: &Path,
    output: &Path,
    production: bool,
) -> Result<()> {
    let meta = read_base_image_metadata(base_image)?;
    let bin = generate_script_bin(paths, production, meta.script_bitw)?;

    if production {
        package::soc::gen_production_binpkg(base_image, &bin, output)?;
        log::info!("Generated production binpkg: {}", output.display());
    } else {
        package::soc::gen_soc(base_image, &bin, output)?;
        log::info!("Generated SOC: {}", output.display());
    }

    Ok(())
}

fn cmd_burn(
    file: &Path,
    port: &str,
    port_type_str: &str,
    chip: &Option<String>,
    do_burn_bl: bool,
    do_burn_ap: bool,
    do_burn_cp: bool,
    do_burn_script: bool,
) -> Result<()> {
    let mut package = package::soc::parse_package(file, true)?;
    if package.product_name().is_none() {
        if let Some(chip) = chip.as_deref() {
            // Legacy binpkg headers may not carry a product. Give ectool's
            // generic planner the same explicit product selected for AgentBoot.
            package.binpkg.product_name = chip.to_string();
        }
    }
    let chip_name = chip
        .as_deref()
        .or_else(|| package.product_name())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Unable to determine chip type from package; specify it manually with -c <chip>"
            )
        })?;

    let mut files = package
        .binpkg
        .entries
        .iter()
        .map(|entry| entry.name.clone())
        .collect::<Vec<_>>();
    if package.script.is_some() {
        files.push("script".to_string());
    }
    log::info!("Files: {:?}", files);
    log::info!("Package product: {}", chip_name);

    let generic_selection = PackageSelection {
        bootloader: do_burn_bl,
        ap: do_burn_ap,
        cp: do_burn_cp,
    };
    let generic_plan = if generic_selection.is_empty() {
        Vec::new()
    } else {
        plan_binpkg_images(&package.binpkg, generic_selection)?
    };
    let script = if do_burn_script {
        package.script.as_ref()
    } else {
        None
    };
    if generic_plan.is_empty() && script.is_none() {
        bail!("Package contains no selected generic or LuatOS script images");
    }

    let port_type = parse_port_type(port_type_str);
    let port_name = resolve_boot_port(port)?;
    log::info!("Select {}", port_name);
    let mut session = start_flash_session(
        &port_name,
        port_type,
        chip_name,
        Some(&package.binpkg),
        package.force_br,
    )?;

    for image in generic_plan {
        let data = image
            .entry
            .data
            .as_deref()
            .expect("ectool package planning validates retained image data");
        flash_with_progress(&mut session, image.target, data)
            .with_context(|| format!("Failed to flash {}", image.entry.name))?;
    }

    if let Some(script) = script {
        let data = script
            .data
            .as_deref()
            .context("LuatOS script data was not retained")?;
        flash_with_progress(&mut session, luatos_script_target(script.address)?, data)?;
    }

    session
        .finish_reset()
        .context("Images were written, but the final device reset failed")?;
    log::info!("burn ok");
    Ok(())
}

struct EraseTarget {
    name: String,
    addr: u32,
    size: u32,
}

fn resolve_erase_targets(
    base_image: &Option<std::path::PathBuf>,
    partitions: &[String],
    addr: &Option<String>,
    size: &Option<String>,
) -> Result<Vec<EraseTarget>> {
    let mut targets = Vec::new();

    if addr.is_some() || size.is_some() {
        let addr = addr
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--addr and --size must be provided together"))?;
        let size = size
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--addr and --size must be provided together"))?;
        targets.push(EraseTarget {
            name: "custom".to_string(),
            addr: parse_u32_arg(addr, "addr")?,
            size: parse_u32_arg(size, "size")?,
        });
    }

    if !partitions.is_empty() {
        let base_path = base_image
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--base-image is required with --partition"))?;
        let part_map = package::soc::read_package_partitions(base_path)?;
        if part_map.partitions.is_empty() {
            bail!(
                "No erasable partitions found in {}; use --addr and --size instead",
                base_path.display()
            );
        }

        for name in partitions {
            let part = part_map.find(name).ok_or_else(|| {
                anyhow::anyhow!(
                    "Unknown partition '{}'. Available: {}",
                    name,
                    part_map.available_names().join(", ")
                )
            })?;
            targets.push(EraseTarget {
                name: part.name.clone(),
                addr: part.addr,
                size: part.size,
            });
        }
    }

    if targets.is_empty() {
        bail!("Nothing to erase; specify --partition or --addr/--size");
    }

    Ok(targets)
}

fn cmd_erase(
    base_image: &Option<std::path::PathBuf>,
    partitions: &[String],
    addr: &Option<String>,
    size: &Option<String>,
    port: &str,
    port_type_str: &str,
    chip: &Option<String>,
) -> Result<()> {
    let targets = resolve_erase_targets(base_image, partitions, addr, size)?;

    let package_info = base_image
        .as_deref()
        .map(|path| package::soc::parse_package(path, false))
        .transpose()?;
    let chip_name = chip
        .as_deref()
        .or_else(|| package_info.as_ref().and_then(|pkg| pkg.product_name()))
        .ok_or_else(|| {
            anyhow::anyhow!("Unable to determine chip type; specify --chip or pass --base-image")
        })?;

    for target in &targets {
        log::info!(
            "Erase target {} addr=0x{:X} size=0x{:X}",
            target.name,
            target.addr,
            target.size
        );
    }

    let port_type = parse_port_type(port_type_str);
    let port_name = resolve_boot_port(port)?;
    log::info!("Select {}", port_name);
    let mut session = start_flash_session(
        &port_name,
        port_type,
        chip_name,
        package_info.as_ref().map(|package| &package.binpkg),
        package_info.as_ref().and_then(|package| package.force_br),
    )?;

    for target in &targets {
        log::info!("Go   Erase {}", target.name);
        let progress = progress_bar(target.size as u64, &format!("erase {}", target.name));
        let mut update = |completed, _total| progress.set_position(completed);
        if let Err(error) = session.erase_with_progress(target.addr, target.size, Some(&mut update))
        {
            progress.abandon_with_message(format!("erase {} FAILED", target.name));
            return Err(error).with_context(|| format!("erase {} failed", target.name));
        }
        progress.finish_with_message(format!("erase {} done", target.name));
        log::info!("Done Erase {}", target.name);
    }

    session
        .finish_reset()
        .context("Erase completed, but the final device reset failed")?;
    log::info!("erase ok");

    Ok(())
}

fn cmd_logs(port: &str, baud: u32) -> Result<()> {
    use chrono::Local;
    use std::io::Read;
    use std::time::Duration;

    let port_name = resolve_log_port(port)?;
    log::info!("Select {}", port_name);

    let mut logcom = serialport::new(&port_name, baud)
        .timeout(Duration::from_millis(100))
        .open()
        .with_context(|| format!("Failed to open log port {}", port_name))?;

    logcom.write_data_terminal_ready(true)?;

    // Send init sequence
    logcom.write_all(&[0x7E, 0x00, 0x00, 0x7E])?;

    let mut ctx = logs::capture::LogContext::new();
    let mut buf = [0u8; 512];

    loop {
        match logcom.read(&mut buf) {
            Ok(n) if n > 0 => {
                let msgs = logs::capture::log_parse(&mut ctx, &buf[..n]);
                for msg in msgs {
                    if !logs::status::StatusParser::is_status(&msg.text) {
                        let ts = format!(
                            "{}{}",
                            Local::now().format("%H:%M:%S%.3f"),
                            format_tick(msg.tick_ms)
                        );
                        print_log(&ts, &msg.text);
                    }
                }
            }
            Ok(_) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => return Err(e.into()),
        }
    }
}

fn cmd_logs_hex(port: &str, baud: u32) -> Result<()> {
    use std::io::Read;
    use std::time::Duration;

    let port_name = resolve_log_port(port)?;
    log::info!("Select {}", port_name);

    let mut logcom = serialport::new(&port_name, baud)
        .timeout(Duration::from_millis(100))
        .open()
        .with_context(|| format!("Failed to open log port {}", port_name))?;

    logcom.write_data_terminal_ready(true)?;

    // Send init sequence
    logcom.write_all(&[0x7E, 0x00, 0x00, 0x7E])?;

    let mut frame = Vec::new();
    let mut in_frame = false;
    let mut buf = [0u8; 512];

    loop {
        match logcom.read(&mut buf) {
            Ok(n) if n > 0 => {
                for &byte in &buf[..n] {
                    if byte == 0x7E {
                        if in_frame {
                            frame.push(byte);
                            println!("{}", format_hex_bytes(&frame));
                            frame.clear();
                            in_frame = false;
                        } else {
                            frame.clear();
                            frame.push(byte);
                            in_frame = true;
                        }
                    } else if in_frame {
                        frame.push(byte);
                    }
                }
            }
            Ok(_) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => return Err(e.into()),
        }
    }
}

fn cmd_monitor(port: &str, baud: u32, stream: bool, debug: bool) -> Result<()> {
    use chrono::Local;
    use std::io::Read;
    use std::time::Duration;

    let port_name = resolve_log_port(port)?;
    log::info!("Select {}", port_name);

    let mut logcom = serialport::new(&port_name, baud)
        .timeout(Duration::from_millis(100))
        .open()
        .with_context(|| format!("Failed to open log port {}", port_name))?;

    logcom.write_data_terminal_ready(true)?;

    // Send init sequence
    logcom.write_all(&[0x7E, 0x00, 0x00, 0x7E])?;

    let mut ctx = logs::capture::LogContext::new();
    let mut parser = logs::status::StatusParser::new();
    let mut buf = [0u8; 512];
    let mut needs_display = true;

    loop {
        match logcom.read(&mut buf) {
            Ok(n) if n > 0 => {
                let msgs = logs::capture::log_parse(&mut ctx, &buf[..n]);
                for msg in msgs {
                    let tick = format_tick(msg.tick_ms);
                    if stream {
                        let matched = parser.feed(&msg.text);
                        if matched {
                            print_log(
                                &format!(
                                    "{}{} [STATUS]",
                                    Local::now().format("%H:%M:%S%.3f"),
                                    tick
                                ),
                                &msg.text,
                            );
                        } else if debug {
                            print_log(
                                &format!("{}{} [LOG]", Local::now().format("%H:%M:%S%.3f"), tick),
                                &msg.text,
                            );
                        }
                    } else {
                        if parser.feed(&msg.text) {
                            needs_display = true;
                        }
                    }
                }
                if !stream && needs_display {
                    parser.status.display();
                    needs_display = false;
                }
            }
            Ok(_) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => return Err(e.into()),
        }
    }
}

/// Print a line to stdout with \r\n (for use in raw terminal mode).
fn raw_println(s: &str) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(s.as_bytes());
    let _ = out.write_all(b"\r\n");
    let _ = out.flush();
}

/// Print a log line in raw mode with timestamp column alignment and colored levels.
fn raw_print_log(timestamp: &str, msg: &str) {
    let clean = strip_control(msg);

    if let Some((level, module, body)) = parse_log_parts(&clean) {
        let color = match level {
            'D' => "\x1b[2m",
            'I' => "",
            'W' => "\x1b[33m",
            'E' => "\x1b[31m",
            _ => "",
        };
        let module_display = module;
        let plain_pad = timestamp.len() + 1 + 2 + module_display.len() + 1;

        let mut first = true;
        for line in body.split('\n') {
            if first {
                raw_println(&format!(
                    "{}{} {}/{} {}\x1b[0m",
                    color, timestamp, level, module_display, line
                ));
                first = false;
            } else {
                raw_println(&format!(
                    "{}{:>pad$}{}\x1b[0m",
                    color,
                    "",
                    line,
                    pad = plain_pad
                ));
            }
        }
        if first {
            raw_println(&format!(
                "{}{} {}/{}\x1b[0m",
                color, timestamp, level, module_display
            ));
        }
    } else {
        let pad = timestamp.len() + 1;
        let mut first = true;
        for line in clean.split('\n') {
            if first {
                raw_println(&format!("\x1b[2m{} {}\x1b[0m", timestamp, line));
                first = false;
            } else {
                raw_println(&format!("\x1b[2m{:>pad$}{}\x1b[0m", "", line, pad = pad));
            }
        }
        if first {
            raw_println(timestamp);
        }
    }
}

/// Set up terminal scroll region to reserve the bottom line for the dev banner.
fn dev_setup_banner() {
    use std::io::Write;
    let (_cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let mut out = std::io::stderr().lock();
    // Set scroll region excluding bottom line and hide cursor.
    let _ = write!(out, "\x1b[?25l\x1b[1;{}r\x1b[1;1H", rows - 1);
    let _ = out.flush();
}

/// Fully reset the dev screen on initial entry.
fn dev_reset_screen() {
    use std::io::Write;
    let mut out = std::io::stderr().lock();
    let _ = write!(out, "\x1b[3J\x1b[2J");
    let _ = out.flush();
    drop(out);
    dev_setup_banner();
}

/// Clear the visible dev log output without triggering a full-screen repaint.
fn dev_clear_output() {
    use std::io::Write;
    let (_cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let mut out = std::io::stderr().lock();
    let _ = write!(out, "\x1b[1;1H\x1b[0J\x1b[{};1H\x1b[2K", rows);
    let _ = out.flush();
    drop(out);
    dev_setup_banner();
}

/// Draw a sticky status banner on the reserved bottom line with inverted colors.
fn dev_draw_banner(status: &str, device: &logs::status::DeviceStatus) {
    use std::io::Write;
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let cols = cols as usize;

    let hints = format!(" ^B burn | ^R restart | ^C exit  {}", status);

    let mut info_parts: Vec<String> = Vec::new();
    if let Some(total) = device.lua_total {
        let used = device.lua_used.unwrap_or(0);
        info_parts.push(format!("{}/{}KB", used / 1024, total / 1024));
    }
    if let Some(ref ver) = device.version {
        info_parts.push(ver.clone());
    }
    let info = if info_parts.is_empty() {
        String::new()
    } else {
        format!("{} ", info_parts.join(" "))
    };

    let gap = cols.saturating_sub(hints.len() + info.len());
    let line = format!("{}{}{}", hints, " ".repeat(gap), info);
    let display: String = format!("{:<width$}", line, width = cols)
        .chars()
        .take(cols)
        .collect();

    let mut out = std::io::stderr().lock();
    let _ = write!(out, "\x1b7\x1b[{};1H\x1b[7m{}\x1b[0m\x1b8", rows, display);
    let _ = out.flush();
}

/// Switch to alternate screen buffer with scroll region and banner (burn modal).
fn dev_enter_alt_screen(status: &str, device: &logs::status::DeviceStatus) {
    use std::io::Write;
    let (_cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let mut out = std::io::stderr().lock();
    // Enter alt screen, clear it, set scroll region, position cursor at top
    let _ = write!(out, "\x1b[?1049h\x1b[2J\x1b[1;{}r\x1b[1;1H", rows - 1);
    let _ = out.flush();
    drop(out);
    dev_draw_banner(status, device);
}

/// Leave alternate screen buffer, restoring the main screen and its scroll region.
fn dev_leave_alt_screen() {
    use std::io::Write;
    let (_cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let mut out = std::io::stderr().lock();
    // Leave alt screen, then re-establish scroll region for main screen
    let _ = write!(out, "\x1b[?1049l\x1b[1;{}r", rows - 1);
    let _ = out.flush();
}

/// Clean up terminal state when leaving dev mode.
fn dev_cleanup() {
    use std::io::Write;
    let _ = crossterm::terminal::disable_raw_mode();
    let mut out = std::io::stderr().lock();
    let _ = write!(out, "\x1b[r\x1b[?25h");
    let _ = out.flush();
}

/// Wait for a serial port, polling keyboard events so Ctrl+C works in raw mode.
/// Returns Ok(Some(port)) on success, Ok(None) on Ctrl+C, Err on timeout.
fn wait_for_port_interruptible(
    mut find_port: impl FnMut() -> Result<Option<String>>,
    description: &str,
    timeout_secs: u32,
) -> Result<Option<String>> {
    use crossterm::event::{self, Event, KeyCode, KeyModifiers};
    use std::time::Duration;

    let infinite = timeout_secs == 0;
    let max_iterations = if infinite {
        u32::MAX
    } else {
        timeout_secs * 10
    };
    for _ in 0..max_iterations {
        if let Some(port) = find_port()? {
            return Ok(Some(port));
        }
        // Poll keyboard for 100ms (same interval as wait_for_port's sleep)
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if matches!(
                    (key.code, key.modifiers),
                    (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL)
                ) {
                    return Ok(None);
                }
            }
        }
    }
    anyhow::bail!(
        "Timeout waiting for {} ({} seconds)",
        description,
        timeout_secs
    );
}

fn cmd_dev(
    paths: &[std::path::PathBuf],
    base_image: &Path,
    port: &str,
    port_type_str: &str,
    baud: u32,
) -> Result<()> {
    use crossterm::event::{self, Event, KeyCode, KeyModifiers};
    use std::io::Read as IoRead;
    use std::time::Duration;

    let meta = read_base_image_metadata(base_image)?;
    let base_package = package::soc::parse_package(base_image, false)?;
    let port_type = parse_port_type(port_type_str);
    let mut status_parser = logs::status::StatusParser::new();

    crossterm::terminal::enable_raw_mode()?;
    dev_reset_screen();

    loop {
        dev_draw_banner("Connecting...", &status_parser.status);

        let log_port_name = if port == "auto" {
            match wait_for_port_interruptible(
                serial::detect::find_log_port_now,
                "LuatOS log port",
                0,
            )? {
                Some(p) => p,
                None => {
                    dev_cleanup();
                    std::process::exit(0);
                }
            }
        } else {
            port.to_string()
        };

        let mut logcom = serialport::new(&log_port_name, baud)
            .timeout(Duration::from_millis(10))
            .open()
            .with_context(|| format!("Failed to open log port {}", log_port_name))?;

        logcom.write_data_terminal_ready(true)?;
        logcom.write_all(&[0x7E, 0x00, 0x00, 0x7E])?;

        let mut ctx = logs::capture::LogContext::new();
        let mut buf = [0u8; 512];

        dev_clear_output();
        dev_draw_banner("", &status_parser.status);

        enum DevAction {
            Burn,
            Restart,
            Disconnected,
        }

        let action = loop {
            // Poll for keyboard events (non-blocking)
            if event::poll(Duration::ZERO)? {
                match event::read()? {
                    Event::Key(key) => match (key.code, key.modifiers) {
                        (KeyCode::Char('b'), m) if m.contains(KeyModifiers::CONTROL) => {
                            break DevAction::Burn;
                        }
                        (KeyCode::Char('r'), m) if m.contains(KeyModifiers::CONTROL) => {
                            break DevAction::Restart;
                        }
                        (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                            dev_cleanup();
                            std::process::exit(0);
                        }
                        (KeyCode::Char('l'), m) if m.contains(KeyModifiers::CONTROL) => {
                            // Clear screen + scrollback, re-establish scroll region and banner
                            dev_clear_output();
                            dev_draw_banner("", &status_parser.status);
                        }
                        _ => {}
                    },
                    Event::Resize(_, _) => {
                        dev_setup_banner();
                        dev_draw_banner("", &status_parser.status);
                    }
                    _ => {}
                }
            }

            match logcom.read(&mut buf) {
                Ok(n) if n > 0 => {
                    let msgs = logs::capture::log_parse(&mut ctx, &buf[..n]);
                    let mut status_changed = false;
                    for msg in msgs {
                        if logs::status::StatusParser::is_status(&msg.text) {
                            if status_parser.feed(&msg.text) {
                                status_changed = true;
                            }
                        } else {
                            let ts = format!(
                                "{}{}",
                                chrono::Local::now().format("%H:%M:%S%.3f"),
                                format_tick(msg.tick_ms)
                            );
                            raw_print_log(&ts, &msg.text);
                        }
                    }
                    if status_changed {
                        dev_draw_banner("", &status_parser.status);
                    }
                }
                Ok(_) => {}
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => break DevAction::Disconnected,
            }
        };

        match action {
            DevAction::Disconnected => {
                drop(logcom);
                dev_draw_banner("Disconnected", &status_parser.status);
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
            DevAction::Restart => {
                dev_draw_banner("Restarting...", &status_parser.status);
                let _ = serial::detect::reboot_on_port(logcom.as_mut());
                std::thread::sleep(Duration::from_millis(200));
                drop(logcom);
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
            DevAction::Burn => {}
        }

        // Drain any queued key events
        while event::poll(Duration::ZERO).unwrap_or(false) {
            let _ = event::read();
        }

        // Leave raw mode and switch to alt screen for burn modal
        let _ = crossterm::terminal::disable_raw_mode();
        dev_enter_alt_screen(">> Compiling...", &status_parser.status);

        // --- Compile ---
        let bin = match generate_script_bin(paths, false, meta.script_bitw) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("\x1b[31mCompile error: {}\x1b[0m", e);
                dev_draw_banner("Compile error", &status_parser.status);
                std::thread::sleep(Duration::from_secs(2));
                dev_leave_alt_screen();
                crossterm::terminal::enable_raw_mode()?;
                continue;
            }
        };

        // --- Reboot to download mode ---
        dev_draw_banner(">> Rebooting...", &status_parser.status);
        let _ = serial::detect::reboot_to_download_on_port(logcom.as_mut());
        drop(logcom);

        dev_draw_banner(">> Waiting for boot port...", &status_parser.status);
        crossterm::terminal::enable_raw_mode()?;
        let boot_port_result = wait_for_port_interruptible(
            || ectool_core::find_download_port_now("auto").map(|port| port.map(|port| port.name)),
            "EigenComm download port",
            30,
        );
        let _ = crossterm::terminal::disable_raw_mode();
        let boot_port_name = match boot_port_result {
            Ok(Some(p)) => p,
            Ok(None) => {
                dev_leave_alt_screen();
                dev_cleanup();
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!(
                    "\x1b[31mBoot port not found: {}. Reset device manually.\x1b[0m",
                    e
                );
                dev_draw_banner("Boot port error", &status_parser.status);
                std::thread::sleep(Duration::from_secs(2));
                dev_leave_alt_screen();
                crossterm::terminal::enable_raw_mode()?;
                continue;
            }
        };

        // --- Burn ---
        dev_draw_banner(">> Downloading...", &status_parser.status);
        let burn_result = (|| -> Result<()> {
            let mut session = start_flash_session(
                &boot_port_name,
                port_type,
                &meta.chip,
                Some(&base_package.binpkg),
                meta.force_br,
            )?;
            flash_with_progress(&mut session, luatos_script_target(meta.script_addr)?, &bin)?;
            session
                .finish_reset()
                .context("Script was written, but the final device reset failed")?;
            Ok(())
        })();

        match &burn_result {
            Ok(()) => {
                // Success — leave alt screen immediately, show result in main banner
                dev_leave_alt_screen();
                dev_draw_banner("Burn OK", &status_parser.status);
            }
            Err(e) => {
                // Failure — show error on alt screen so user can read burn logs
                eprintln!("\x1b[31mBurn failed: {}\x1b[0m", e);
                dev_draw_banner("Burn failed", &status_parser.status);
                std::thread::sleep(Duration::from_secs(3));
                dev_leave_alt_screen();
                dev_draw_banner("Burn failed", &status_parser.status);
            }
        }

        // Re-enable raw mode before returning to log loop
        crossterm::terminal::enable_raw_mode()?;

        std::thread::sleep(Duration::from_secs(2));
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let log_level = if cli.debug { "debug" } else { "info" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level))
        .format_target(false)
        .format_timestamp(None)
        .init();

    match &cli.command {
        Commands::Script { .. } | Commands::Pack { .. } | Commands::Dev { .. } => {
            init_lua_helper_cache().map_err(anyhow::Error::msg)?;
        }
        _ => {}
    }

    match &cli.command {
        Commands::Script {
            paths,
            output,
            burn,
            production,
            lua_bitw,
            base_image,
            port,
            port_type,
        } => {
            cmd_script(
                paths,
                output,
                *burn,
                *production,
                *lua_bitw,
                base_image,
                port,
                port_type,
            )?;
        }
        Commands::Pack {
            paths,
            base_image,
            output,
            production,
        } => {
            cmd_pack(paths, base_image, output, *production)?;
        }
        Commands::Burn {
            file,
            port,
            port_type,
            chip,
            only,
        } => {
            let (do_bl, do_ap, do_cp, do_script) = if only.is_empty() {
                (true, true, true, true)
            } else {
                (
                    only.iter().any(|s| s == "bl"),
                    only.iter().any(|s| s == "ap"),
                    only.iter().any(|s| s == "cp"),
                    only.iter().any(|s| s == "script"),
                )
            };
            cmd_burn(file, port, port_type, chip, do_bl, do_ap, do_cp, do_script)?;
        }
        Commands::Erase {
            base_image,
            partition,
            addr,
            size,
            port,
            port_type,
            chip,
        } => {
            cmd_erase(base_image, partition, addr, size, port, port_type, chip)?;
        }
        Commands::Dev {
            paths,
            base_image,
            port,
            port_type,
            baud,
        } => {
            cmd_dev(paths, base_image, port, port_type, *baud)?;
        }
        Commands::Logs { port, baud, hex } => {
            if *hex {
                cmd_logs_hex(port, *baud)?;
            } else {
                cmd_logs(port, *baud)?;
            }
        }
        Commands::Monitor { port, baud, stream } => {
            cmd_monitor(port, *baud, *stream, cli.debug)?;
        }
        Commands::Reboot { port } => {
            cmd_reboot(port)?;
        }
    }

    Ok(())
}

/// Reboot the module normally by sending the diag reboot frame over the log COM
/// (same effect as Dev mode's Ctrl-R). Lets the device restart without a full
/// reflash, so a fresh boot can be observed.
fn cmd_reboot(port: &str) -> Result<()> {
    use std::time::Duration;

    let port_name = resolve_log_port(port)?;
    log::info!("Select {}", port_name);

    let mut logcom = serialport::new(&port_name, 115200)
        .timeout(Duration::from_millis(500))
        .open()
        .with_context(|| format!("Failed to open log port {}", port_name))?;

    serial::detect::reboot_on_port(logcom.as_mut())?;
    log::info!("reboot sent on {}", port_name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luatos_script_target_preserves_flexfile_xip_convention() {
        let target = luatos_script_target(0x003A_0000).unwrap();
        assert_eq!(target.image_type, ImageKind::FlexFile);
        assert_eq!(target.storage, FlashStorage::ApFlash);
        assert_eq!(target.address, 0x00BA_0000);
        assert_eq!(target.tag, "SCRIPT");

        let already_biased = luatos_script_target(0x00BA_0000).unwrap();
        assert_eq!(already_biased.address, 0x00BA_0000);
    }
}
