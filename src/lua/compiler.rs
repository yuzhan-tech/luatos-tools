use std::io::Write;
use std::process::{Command, Stdio};

use super::embedded_helpers::{ensure_embedded_helper, init_helper_cache};

pub fn init_lua_helper_cache() -> Result<(), String> {
    init_helper_cache()
}

/// Compile Lua source to bytecode for the requested Lua bitness.
/// When `strip` is true, debug info is removed for smaller output.
/// When false, debug info is preserved for stack traces.
pub fn compile_lua(
    source: &[u8],
    chunk_name: &str,
    strip: bool,
    bitw: u32,
) -> Result<Vec<u8>, String> {
    let helper = ensure_embedded_helper(bitw)?;

    let mut child = Command::new(&helper)
        .arg(chunk_name)
        .arg(if strip { "1" } else { "0" })
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "failed to spawn Lua {}-bit compiler from {}: {}",
                bitw,
                helper.display(),
                e
            )
        })?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(source)
            .map_err(|e| format!("failed to write source to Lua compiler: {}", e))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("failed to wait for Lua compiler: {}", e))?;

    if output.status.success() {
        Ok(output.stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            Err(format!("Lua {}-bit compiler failed", bitw))
        } else {
            Err(stderr)
        }
    }
}
