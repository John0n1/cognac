use crate::model::{
    ApplicationClass, Architecture, ExecutableInfo, InstallerType, TrustRequirements,
};
use anyhow::{Context, Result, bail};
use goblin::pe::{PE, header};
use memmap2::MmapOptions;
use sha2::{Digest, Sha256};
use std::{fs, fs::File, path::Path};

const MAX_EXECUTABLE_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const STRING_WINDOW_BYTES: usize = 64 * 1024 * 1024;

pub fn analyze(path: &Path) -> Result<ExecutableInfo> {
    let metadata =
        fs::metadata(path).with_context(|| format!("cannot access {}", path.display()))?;
    if !metadata.is_file() {
        bail!("{} is not a regular file", path.display());
    }
    if metadata.len() > MAX_EXECUTABLE_BYTES {
        bail!(
            "{} is too large to analyze safely (limit: 16 GiB)",
            path.display()
        );
    }
    let file = File::open(path).with_context(|| format!("cannot read {}", path.display()))?;
    // SAFETY: this is a private, read-only mapping of a regular file whose
    // metadata was checked immediately above. Cognac never writes through the
    // mapping or keeps it after analysis.
    let bytes = unsafe { MmapOptions::new().map(&file) }
        .with_context(|| format!("cannot map {} for analysis", path.display()))?;
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
    let mut ascii = Vec::new();
    let mut utf16 = Vec::new();
    for window in string_windows(&bytes) {
        ascii.extend(printable_strings(window, false));
        utf16.extend(printable_strings(window, true));
    }
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
    let strong_game_marker = lower.contains("unityplayer.dll")
        || lower.contains("unreal engine")
        || imports.iter().any(|value| value == "xinput1_3.dll");
    let mut graphics_apis = Vec::new();
    for (needle, label) in [
        ("d3d12", "Direct3D 12"),
        ("d3d11", "Direct3D 11"),
        ("d3d9", "Direct3D 9"),
        ("dxgi", "DXGI"),
        ("vulkan-1", "Vulkan"),
        ("opengl32", "OpenGL"),
    ] {
        if imports.iter().any(|value| value.contains(needle))
            || strong_game_marker && lower.contains(needle)
        {
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
    let trust = analyze_trust(&lower, &imports);
    if trust.elevation_likely {
        indicators.push("driver-or-elevation".into());
    }
    if trust.kernel_driver_likely {
        indicators.push("kernel-driver".into());
    }
    if trust.windows_service_likely {
        indicators.push("windows-service".into());
    }
    if !trust.anti_cheat.is_empty() {
        indicators.push("anti-cheat".into());
    }

    let fallback_name = clean_filename(path);
    let product_name = metadata_value(&utf16, "ProductName").or(Some(fallback_name));
    let publisher = metadata_value(&utf16, "CompanyName");
    let sha256 = hex::encode(Sha256::digest(&bytes));
    let application_class = classify_application(&lower, &frameworks, &indicators, &trust);
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
        application_class,
        trust,
    })
}

fn string_windows(bytes: &[u8]) -> Vec<&[u8]> {
    if bytes.len() <= STRING_WINDOW_BYTES * 2 {
        return vec![bytes];
    }
    vec![
        &bytes[..STRING_WINDOW_BYTES],
        &bytes[bytes.len() - STRING_WINDOW_BYTES..],
    ]
}

fn analyze_trust(lower: &str, imports: &[String]) -> TrustRequirements {
    let mut trust = TrustRequirements::default();
    let mut evidence = Vec::new();

    trust.elevation_likely = contains_any(
        lower,
        &[
            "requireadministrator",
            "requires administrator",
            "runasadministrator",
            "requestedexecutionlevel level=\"requireadministrator\"",
        ],
    );
    if trust.elevation_likely {
        evidence.push("administrator manifest or elevation marker detected".into());
    }

    trust.windows_service_likely = contains_any(
        lower,
        &[
            "createservicew",
            "createservicea",
            "startservicew",
            "service_control_manager",
            "service_win32_own_process",
        ],
    );
    if trust.windows_service_likely {
        evidence.push("Windows service installation APIs detected".into());
    }

    let imports_setup_api = imports
        .iter()
        .any(|dll| matches!(dll.as_str(), "setupapi.dll" | "newdev.dll" | "cfgmgr32.dll"));
    trust.kernel_driver_likely = contains_any(
        lower,
        &[
            "service_kernel_driver",
            "ntloaddriver",
            "zwloaddriver",
            "\\systemroot\\system32\\drivers",
            "kernel-mode driver",
            "kernel mode driver",
            "minifilter driver",
            "ndis filter driver",
        ],
    ) || (imports_setup_api
        && contains_any(lower, &[".sys", "difxapi", "dpinst", "setupcopyoeminf"]));
    if trust.kernel_driver_likely {
        evidence.push("kernel driver installation markers detected".into());
    }

    for (needle, name) in [
        ("easyanticheat", "Easy Anti-Cheat"),
        ("easy anti-cheat", "Easy Anti-Cheat"),
        ("battleye", "BattlEye"),
        ("vgk.sys", "Riot Vanguard"),
        ("riot vanguard", "Riot Vanguard"),
        ("faceit anti-cheat", "FACEIT Anti-Cheat"),
        ("xigncode", "XIGNCODE"),
        ("equ8", "EQU8"),
        ("ricochet anti-cheat", "Ricochet"),
        ("mhyprot", "HoYoProtect"),
        ("nprotect gameguard", "nProtect GameGuard"),
    ] {
        if lower.contains(needle) && !trust.anti_cheat.iter().any(|value| value == name) {
            trust.anti_cheat.push(name.into());
        }
    }
    if trust.anti_cheat.iter().any(|name| {
        matches!(
            name.as_str(),
            "Riot Vanguard"
                | "FACEIT Anti-Cheat"
                | "XIGNCODE"
                | "Ricochet"
                | "HoYoProtect"
                | "nProtect GameGuard"
        )
    }) {
        if !trust.kernel_driver_likely {
            evidence.push("kernel-level anti-cheat behavior is likely".into());
        }
        trust.kernel_driver_likely = true;
    }
    if !trust.anti_cheat.is_empty() {
        evidence.push(format!(
            "anti-cheat markers detected: {}",
            trust.anti_cheat.join(", ")
        ));
    }

    trust.tpm_likely = contains_any(lower, &["tpm 2.0", "tbsip_submit_command", "tbs.dll"]);
    trust.secure_boot_likely = contains_any(
        lower,
        &[
            "secure boot required",
            "secureboot_required",
            "confirm-securebootuefi",
        ],
    );
    trust.direct_hardware_access_likely = contains_any(
        lower,
        &[
            "winusb.dll",
            "setupdigetclassdevs",
            "device interface guid",
            "usb kernel driver",
            "pci device driver",
        ],
    ) && trust.kernel_driver_likely;
    if trust.tpm_likely {
        evidence.push("TPM-backed trust markers detected".into());
    }
    if trust.secure_boot_likely {
        evidence.push("Secure Boot requirement markers detected".into());
    }
    if trust.direct_hardware_access_likely {
        evidence.push("direct Windows hardware access appears to be required".into());
    }
    evidence.sort();
    evidence.dedup();
    trust.evidence = evidence;
    trust
}

fn classify_application(
    lower: &str,
    frameworks: &[String],
    indicators: &[String],
    trust: &TrustRequirements,
) -> ApplicationClass {
    if indicators.iter().any(|value| value == "game") || !trust.anti_cheat.is_empty() {
        return ApplicationClass::Game;
    }
    if trust.kernel_driver_likely || trust.direct_hardware_access_likely {
        return ApplicationClass::DriverPackage;
    }
    if indicators.iter().any(|value| value == "windows-service")
        || contains_any(
            lower,
            &["system utility", "system optimizer", "registry cleaner"],
        )
    {
        return ApplicationClass::SystemUtility;
    }
    if frameworks.iter().any(|value| value == "media-foundation")
        || contains_any(lower, &["media player", "video player", "audio player"])
    {
        return ApplicationClass::Media;
    }
    if contains_any(
        lower,
        &[
            "word processor",
            "spreadsheet",
            "office suite",
            "pdf editor",
            "productivity",
        ],
    ) {
        return ApplicationClass::Productivity;
    }
    if contains_any(
        lower,
        &["windows xp", "windows 2000", "windows 98", "16-bit"],
    ) {
        return ApplicationClass::Legacy;
    }
    ApplicationClass::General
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
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

    #[test]
    fn detects_kernel_anti_cheat_requirements() {
        let trust = analyze_trust(
            "easyanticheat service_kernel_driver ntloaddriver secure boot required",
            &["setupapi.dll".into()],
        );
        assert!(trust.kernel_driver_likely);
        assert!(trust.secure_boot_likely);
        assert_eq!(trust.anti_cheat, ["Easy Anti-Cheat"]);
        assert!(trust.requires_windows_kernel());
    }

    #[test]
    fn classifies_userspace_games_without_forcing_vm() {
        let trust = TrustRequirements::default();
        assert_eq!(
            classify_application("game", &[], &["game".into()], &trust),
            ApplicationClass::Game
        );
    }
}
