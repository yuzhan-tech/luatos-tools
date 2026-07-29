//! LuatOS integration for ectool's generic EigenComm UniLog implementation.

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::serial::detect::resolve_unilog_port;

pub struct UnilogArgs {
    pub port: String,
    pub comdb: Option<PathBuf>,
    pub base_image: Option<PathBuf>,
    pub raw: bool,
    pub phy: bool,
    pub owner: Vec<String>,
    pub module: Vec<String>,
    pub sub: Vec<String>,
    pub level: Vec<String>,
    pub file: Option<PathBuf>,
    pub out: Option<PathBuf>,
    pub append: bool,
    pub version_check: bool,
}

#[derive(Debug)]
struct ResolvedComdb {
    path: Option<PathBuf>,
    // Keep an extracted SOC alive while ectool reads and uses its comdb.
    _extracted_soc: Option<tempfile::TempDir>,
}

pub fn run(args: UnilogArgs) -> Result<()> {
    let UnilogArgs {
        port,
        comdb,
        base_image,
        raw,
        phy,
        owner,
        module,
        sub,
        level,
        file,
        out,
        append,
        version_check,
    } = args;

    let resolved_comdb = resolve_comdb(raw, comdb, base_image)?;
    let live_port = if file.is_some() {
        None
    } else {
        Some(resolve_unilog_port(&port)?)
    };

    ectool_core::unilog::run(ectool_core::unilog::UnilogArgs {
        port: live_port,
        comdb: resolved_comdb.path.clone(),
        raw,
        phy,
        owner,
        module,
        sub,
        level,
        file,
        out,
        append,
        version_check,
    })
}

fn resolve_comdb(
    raw: bool,
    comdb: Option<PathBuf>,
    base_image: Option<PathBuf>,
) -> Result<ResolvedComdb> {
    if raw {
        return Ok(ResolvedComdb {
            path: None,
            _extracted_soc: None,
        });
    }

    if let Some(path) = comdb {
        return Ok(ResolvedComdb {
            path: Some(path),
            _extracted_soc: None,
        });
    }

    let base_image = base_image.ok_or_else(|| {
        anyhow::anyhow!(
            "UniLog decoding requires --comdb <comdb.txt>, --base-image <soc>, or --raw"
        )
    })?;

    if base_image.is_dir() {
        let path = find_comdb_file(&base_image)?
            .ok_or_else(|| anyhow::anyhow!("No comdb.txt found under {}", base_image.display()))?;
        return Ok(ResolvedComdb {
            path: Some(path),
            _extracted_soc: None,
        });
    }

    if !base_image.is_file() {
        bail!("Base image does not exist: {}", base_image.display());
    }

    let extracted = tempfile::tempdir().context("Failed to create SOC extraction directory")?;
    sevenz_rust2::decompress_file(&base_image, extracted.path())
        .with_context(|| format!("Failed to extract base image {}", base_image.display()))?;
    let path = find_comdb_file(extracted.path())?
        .ok_or_else(|| anyhow::anyhow!("No comdb.txt found in {}", base_image.display()))?;

    Ok(ResolvedComdb {
        path: Some(path),
        _extracted_soc: Some(extracted),
    })
}

fn find_comdb_file(root: &Path) -> Result<Option<PathBuf>> {
    let mut entries = fs::read_dir(root)
        .with_context(|| format!("Failed to read directory {}", root.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("Failed to read directory entry under {}", root.display()))?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_comdb_file(&path)? {
                return Ok(Some(found));
            }
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.eq_ignore_ascii_case("comdb.txt"))
            .unwrap_or(false)
        {
            return Ok(Some(path));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_comdb_wins_over_base_image() {
        let selected = resolve_comdb(
            false,
            Some(PathBuf::from("explicit-comdb.txt")),
            Some(PathBuf::from("ignored.soc")),
        )
        .unwrap();

        assert_eq!(selected.path, Some(PathBuf::from("explicit-comdb.txt")));
        assert!(selected._extracted_soc.is_none());
    }

    #[test]
    fn raw_mode_does_not_require_or_extract_a_dictionary() {
        let selected = resolve_comdb(true, None, Some(PathBuf::from("missing.soc"))).unwrap();

        assert!(selected.path.is_none());
        assert!(selected._extracted_soc.is_none());
    }

    #[test]
    fn finds_comdb_recursively_in_extracted_directory() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("nested").join("db");
        fs::create_dir_all(&nested).unwrap();
        let comdb = nested.join("ComDb.TxT");
        fs::write(&comdb, "fixture").unwrap();

        let selected = resolve_comdb(false, None, Some(root.path().to_path_buf())).unwrap();

        assert_eq!(selected.path, Some(comdb));
        assert!(selected._extracted_soc.is_none());
    }

    #[test]
    fn decoded_mode_requires_a_dictionary_source() {
        let error = resolve_comdb(false, None, None).unwrap_err();

        assert!(error.to_string().contains("--base-image"));
        assert!(error.to_string().contains("--comdb"));
        assert!(error.to_string().contains("--raw"));
    }
}
