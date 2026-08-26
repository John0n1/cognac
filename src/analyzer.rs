use crate::model::{Architecture, ExecutableInfo, InstallerType};
use anyhow::{Context, Result, bail};
use goblin::pe::{PE, header};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

const MAX_ANALYSIS_BYTES: usize = 256 * 1024 * 1024;

pub fn analyze(path: &Path) -> Result<ExecutableInfo> {
    let metadata =
        fs::metadata(path).with_context(|| format!("cannot access {}", path.display()))?;
    if !metadata.is_file() {
        bail!("{} is not a regular file", path.display());
    }
    if metadata.len() as usize > MAX_ANALYSIS_BYTES {
        bail!(
            "{} is too large to analyze safely (limit: 256 MiB)",
            path.display()
        );
    }
    let bytes = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    if !bytes.starts_with(b"MZ") {
        bail!("{} is not a Windows PE executable", path.display());
    }
    let pe = PE::parse(&bytes).context("malformed or unsupported Windows PE executable")?;

    let architecture = match pe.header.coff_header.machine {
        header::COFF_MACHINE_X86 => Architecture::X86,
        header::COFF_MACHINE_X86_64 => Architecture::X64,
        header::COFF_MACHINE_ARM64 => Architecture::Arm64,
        _ => Architecture::Unknown,
    };
    let ascii = printable_strings(&bytes, false);
    let utf16 = printable_strings(&bytes, true);
    let searchable = format!("{}\n{}", ascii.join("\n"), utf16.join("\n"));
    let lower = searchable.to_ascii_lowercase();

    let installer_type = if lower.contains("inno setup") {
        InstallerType::Inno
    } else if lower.contains("nullsoft") || lower.contains("nsis") {
        InstallerType::Nsis
    } else if lower.contains("installshield") {
        InstallerType::InstallShield
    } else if lower.contains("wixburn") || lower.contains("burnengine") {
        InstallerType::Burn
    } else if lower.contains("squirrel") || lower.contains("update.exe") {
        InstallerType::Squirrel
    } else if lower.contains("windows installer")
        || path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("msi"))
    {
        InstallerType::Msi
    } else {
        InstallerType::PortableOrUnknown
    };

    let imports = pe
        .imports
        .iter()
        .map(|import| import.dll.to_ascii_lowercase())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut graphics_apis = Vec::new();
    for (needle, label) in [
        ("d3d12", "Direct3D 12"),
        ("d3d11", "Direct3D 11"),
        ("d3d9", "Direct3D 9"),
        ("dxgi", "DXGI"),
        ("vulkan-1", "Vulkan"),
        ("opengl32", "OpenGL"),
    ] {
        if imports.iter().any(|v| v.contains(needle)) || lower.contains(needle) {
            graphics_apis.push(label.into());
        }
    }
    let mut frameworks = Vec::new();
    if imports.iter().any(|v| v == "mscoree.dll") || lower.contains(".netframework") {
        frameworks.push("dotnet".into());
    }
    if imports
        .iter()
        .any(|v| v.starts_with("vcruntime") || v.starts_with("msvcp1"))
    {
        frameworks.push("visual-c++".into());
    }
    if lower.contains("windows media foundation") || imports.iter().any(|v| v == "mfplat.dll") {
        frameworks.push("media-foundation".into());
    }

    let mut indicators = Vec::new();
    if lower.contains("unityplayer.dll") {
        indicators.push("unity".into());
    }
    if lower.contains("unreal engine") || imports.iter().any(|v| v == "xinput1_3.dll") {
        indicators.push("game".into());
    }
    if lower.contains("electron") || lower.contains("chrome_elf.dll") {
        indicators.push("electron".into());
    }
    if lower.contains("kernel driver") || lower.contains("requires administrator") {
        indicators.push("driver-or-elevation".into());
    }

    let fallback_name = clean_filename(path);
    let product_name = metadata_value(&utf16, "ProductName").or(Some(fallback_name));
    let publisher = metadata_value(&utf16, "CompanyName");
    let sha256 = hex::encode(Sha256::digest(&bytes));
    Ok(ExecutableInfo {
        path: path.canonicalize().unwrap_or_else(|_| path.to_path_buf()),
        sha256,
        size: metadata.len(),
        architecture,
        installer_type,
        product_name,
        publisher,
        imports,
        graphics_apis,
        frameworks,
        indicators,
    })
}

fn clean_filename(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Windows application");
    let mut words = stem.replace(['_', '-', '.'], " ");
    for token in [
        "setup",
        "installer",
        "install",
        "win64",
        "win32",
        "x64",
        "x86",
    ] {
        words = words
            .split_whitespace()
            .filter(|word| !word.eq_ignore_ascii_case(token))
            .collect::<Vec<_>>()
            .join(" ");
    }
    if words.is_empty() { stem.into() } else { words }
}

fn metadata_value(strings: &[String], key: &str) -> Option<String> {
    strings.windows(2).find_map(|pair| {
        (pair[0].eq_ignore_ascii_case(key) && pair[1].len() > 1 && pair[1].len() < 160)
            .then(|| pair[1].clone())
    })
}

fn printable_strings(bytes: &[u8], wide: bool) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = Vec::new();
    let step = if wide { 2 } else { 1 };
    let mut index = 0;
    while index + step - 1 < bytes.len() {
        let valid_wide = !wide || bytes[index + 1] == 0;
        let byte = bytes[index];
        if valid_wide && (byte.is_ascii_graphic() || byte == b' ') {
            current.push(byte);
        } else {
            if current.len() >= 4 {
                result.push(String::from_utf8_lossy(&current).into_owned());
            }
            current.clear();
        }
        index += step;
    }
    if current.len() >= 4 {
        result.push(String::from_utf8_lossy(&current).into_owned());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_are_cleaned() {
        assert_eq!(
            clean_filename(Path::new("Some_App_Setup_x64.exe")),
            "Some App"
        );
    }
    #[test]
    fn rejects_non_pe() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"hello").unwrap();
        assert!(
            analyze(file.path())
                .unwrap_err()
                .to_string()
                .contains("not a Windows PE")
        );
    }
}
