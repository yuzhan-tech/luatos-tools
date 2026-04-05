mod cli;
mod flash;
mod logs;
mod lua;
mod luadb;
mod package;
mod serial;
mod util;

use anyhow::{bail, Context, Result};
use clap::Parser;
use std::fs;
use std::path::Path;

use cli::{Cli, Commands};
use flash::burn::{burn_agboot, burn_img, load_agentboot, sys_reset};
use flash::consts::*;
use flash::sync::burn_sync;
use lua::compiler::{compile_lua, init_lua_helper_cache};
use luadb::pack::{pack_luadb, LuadbEntry};
use serial::detect::{
    resolve_port, BOOT_PID, BOOT_VID, LOG_COMM_INTERFACE, LOG_DATA_INTERFACE, LOG_PID, LOG_VID,
};
use serial::port::{open_port, PortType};

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

fn normalize_lua_bitw(lua_bitw: u32) -> Result<u32> {
    match lua_bitw {
        32 | 64 => Ok(lua_bitw),
        _ => bail!("Unsupported Lua bitness: {} (expected 32 or 64)", lua_bitw),
    }
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
        let meta = base_meta
            .ok_or_else(|| anyhow::anyhow!("--base-image is required when using --burn"))?;

        let mut burn_addr = meta.script_addr;
        if burn_addr < 0x800000 {
            burn_addr += 0x800000;
        }
        let agent_br = meta.force_br.unwrap_or(921600);
        log::info!(
            "Burn addr=0x{:X}, chip={}, agent_br={}",
            burn_addr,
            meta.chip,
            agent_br
        );

        let port_type = parse_port_type(port_type_str);
        let port_name = resolve_port(port, BOOT_VID, BOOT_PID, &[])?;
        let mut burncom = open_port(&port_name, port_type)?;
        let port = burncom.as_mut();

        log::info!("Go   Sync");
        burn_sync(port, SyncType::DlBoot, 2)?;
        log::info!("Done Sync");

        let ag = load_agentboot(&meta.chip, port_type)?;
        burn_agboot(port, ag, agent_br)?;

        log::info!("Go   Script download");
        let ret = burn_img(
            port,
            &bin,
            BurnImageType::FlexFile,
            STYPE_AP_FLASH,
            burn_addr,
            "SCRIPT",
            None,
        )?;

        let reset_ret = sys_reset(port)?;
        log::info!("sys reset {}", reset_ret);

        if ret == 0 {
            log::info!("burn script ok");
        } else {
            bail!("burn script failed ({})", ret);
        }
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
    let jdata = package::soc::parse_package(file, true)?;
    let chip_name = chip
        .as_deref()
        .or_else(|| {
            if jdata.chip == package::binpkg::UNKNOWN_CHIP {
                None
            } else {
                Some(jdata.chip.as_str())
            }
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Unable to determine chip type from package; specify it manually with -c <chip>"
            )
        })?;
    let is_ec7xx = chip_name.to_uppercase().contains("EC7");

    log::info!(
        "Files: {:?}",
        jdata.entries.iter().map(|e| &e.name).collect::<Vec<_>>()
    );
    log::info!("Chip: {}", chip_name);

    let port_type = parse_port_type(port_type_str);
    let port_name = resolve_port(port, BOOT_VID, BOOT_PID, &[])?;
    log::info!("Select {}", port_name);

    let mut burncom = open_port(&port_name, port_type)?;
    let port = burncom.as_mut();

    // Initial DLBOOT sync
    log::info!("Go   Sync");
    burn_sync(port, SyncType::DlBoot, 2)?;
    log::info!("Done Sync");

    // Agent boot is required for all burn operations (BL/AP/CP/script all use LPC via agentboot)
    let agent_br = jdata.force_br.unwrap_or(921600);
    log::info!("Go   AgentBoot download (baud={})", agent_br);
    let ag = load_agentboot(chip_name, port_type)?;
    burn_agboot(port, ag, agent_br)?;
    log::info!("Done AgentBoot download");

    // Identify partitions by image_type
    let bl_idx = jdata
        .entries
        .iter()
        .position(|e| e.image_type == "BL" && e.data.is_some());
    let ap_idx = jdata
        .entries
        .iter()
        .position(|e| e.image_type == "AP" && e.name != "script" && e.data.is_some());
    let cp_idx = jdata
        .entries
        .iter()
        .position(|e| e.image_type == "CP" && e.data.is_some());

    let mut ret = 0;

    // Burn BL
    if let Some(idx) = bl_idx {
        if do_burn_bl {
            let entry = &jdata.entries[idx];
            log::info!("Go   BL download");
            ret = burn_img(
                port,
                entry.data.as_ref().unwrap(),
                BurnImageType::Bootloader,
                STYPE_AP_FLASH,
                0,
                "BL",
                None,
            )?;
            if ret != 0 {
                bail!("burn_img BootLoader failed");
            }
            log::info!("Done BL download");
        }
    }

    // Burn AP
    if let Some(idx) = ap_idx {
        if do_burn_ap {
            let entry = &jdata.entries[idx];
            log::info!("Go   AP download");
            let mut ap_addr = entry.addr;
            if ap_addr >= 0x800000 {
                ap_addr -= 0x800000;
            }
            ret = burn_img(
                port,
                entry.data.as_ref().unwrap(),
                BurnImageType::Ap,
                STYPE_AP_FLASH,
                ap_addr,
                "AP",
                None,
            )?;
            if ret != 0 {
                bail!("burn_img AP failed");
            }
            log::info!("Done AP download");
        }
    }

    // Burn CP
    if let Some(idx) = cp_idx {
        if do_burn_cp {
            let entry = &jdata.entries[idx];
            log::info!("Go   CP download");
            if is_ec7xx {
                let mut cp_addr = entry.addr;
                if cp_addr >= 0x800000 {
                    cp_addr -= 0x800000;
                }
                ret = burn_img(
                    port,
                    entry.data.as_ref().unwrap(),
                    BurnImageType::Cp,
                    STYPE_AP_FLASH,
                    cp_addr,
                    "CP",
                    None,
                )?;
            } else {
                ret = burn_img(
                    port,
                    entry.data.as_ref().unwrap(),
                    BurnImageType::Cp,
                    STYPE_CP_FLASH,
                    0,
                    "CP",
                    None,
                )?;
            }
            if ret != 0 {
                bail!("burn_img CP failed");
            }
            log::info!("Done CP download");
        }
    }

    // Burn Script
    if let Some(script_entry) = jdata.find_entry("script") {
        if do_burn_script {
            if let Some(ref data) = script_entry.data {
                log::info!("Go   Script download");
                let mut burn_addr = script_entry.addr;
                if burn_addr < 0x800000 {
                    burn_addr += 0x800000;
                }
                ret = burn_img(
                    port,
                    data,
                    BurnImageType::FlexFile,
                    STYPE_AP_FLASH,
                    burn_addr,
                    "SCRIPT",
                    None,
                )?;
                if ret != 0 {
                    bail!("burn_img SCRIPT failed");
                }
                log::info!("Done Script download");
            }
        }
    }

    let reset_ret = sys_reset(port)?;
    log::info!("sys reset {}", reset_ret);

    if ret == 0 {
        log::info!("burn ok");
    } else {
        log::info!("burn fail {}", ret);
    }

    Ok(())
}

fn cmd_logs(port: &str, baud: u32) -> Result<()> {
    use chrono::Local;
    use std::io::Read;
    use std::time::Duration;

    let port_name = resolve_port(
        port,
        LOG_VID,
        LOG_PID,
        &[LOG_COMM_INTERFACE, LOG_DATA_INTERFACE],
    )?;
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

    let port_name = resolve_port(
        port,
        LOG_VID,
        LOG_PID,
        &[LOG_COMM_INTERFACE, LOG_DATA_INTERFACE],
    )?;
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

    let port_name = resolve_port(
        port,
        LOG_VID,
        LOG_PID,
        &[LOG_COMM_INTERFACE, LOG_DATA_INTERFACE],
    )?;
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
    vid: u16,
    pid: u16,
    interfaces: &[u8],
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
        if let Some(port) = serial::detect::auto_detect_port(vid, pid, interfaces) {
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
        "Timeout waiting for USB device {:04X}:{:04X} ({} seconds)",
        vid,
        pid,
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
    let mut burn_addr = meta.script_addr;
    if burn_addr < 0x800000 {
        burn_addr += 0x800000;
    }
    let agent_br = meta.force_br.unwrap_or(921600);
    let port_type = parse_port_type(port_type_str);
    let mut status_parser = logs::status::StatusParser::new();

    crossterm::terminal::enable_raw_mode()?;
    dev_reset_screen();

    let mut first_connect = true;
    loop {
        if first_connect {
            first_connect = false;
        } else {
            dev_clear_output();
        }
        dev_draw_banner("Connecting...", &status_parser.status);

        let log_port_name = if port == "auto" {
            match wait_for_port_interruptible(
                LOG_VID,
                LOG_PID,
                &[LOG_COMM_INTERFACE, LOG_DATA_INTERFACE],
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
        let boot_port_result = wait_for_port_interruptible(BOOT_VID, BOOT_PID, &[], 30);
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
            let mut burncom = open_port(&boot_port_name, port_type)?;
            let port = burncom.as_mut();

            burn_sync(port, SyncType::DlBoot, 2)?;

            let ag = load_agentboot(&meta.chip, port_type)?;
            burn_agboot(port, ag, agent_br)?;

            let ret = burn_img(
                port,
                &bin,
                BurnImageType::FlexFile,
                STYPE_AP_FLASH,
                burn_addr,
                "SCRIPT",
                None,
            )?;

            sys_reset(port)?;

            if ret != 0 {
                bail!("Burn failed ({})", ret);
            }
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
    }

    Ok(())
}
