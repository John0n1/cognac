//! Read-only package inspection. No firmware-writing service is called here.
use crate::{analyzer, model::ExecutableInfo, util::find_command};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};
use walkdir::WalkDir;

#[derive(Debug, Serialize)]
pub struct FirmwareInspection {
    pub executable: ExecutableInfo,
    pub extraction_directory: PathBuf,
    pub extraction_output: String,
    pub host: FirmwareHost,
    pub payload_executables: Vec<ExecutableInfo>,
    pub executable_errors: Vec<String>,
    pub capsules: Vec<CapsuleInspection>,
    pub capsule_errors: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct FirmwareHost {
    pub vendor: Option<String>,
    pub product: Option<String>,
    pub bios_version: Option<String>,
    pub bios_date: Option<String>,
    pub esrt: Vec<FirmwareResource>,
    pub esrt_status: String,
}

#[derive(Debug, Serialize)]
pub struct FirmwareResource {
    pub guid: String,
    pub firmware_type: Option<u32>,
    pub version: Option<u32>,
    pub last_attempt_version: Option<u32>,
    pub last_attempt_status: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct CapsuleInspection {
    pub path: PathBuf,
    pub sha256: String,
    pub size: u64,
    pub capsule_guid: String,
    pub header_size: u32,
    pub flags: u32,
    pub declared_size: u32,
    pub header_bounds_valid: bool,
    pub matching_esrt_guids: Vec<String>,
}

pub fn inspect(executable: &Path, destination: &Path) -> Result<FirmwareInspection> {
    let info = analyzer::analyze(executable)?;
    let extractor = find_command("innoextract")
        .context("innoextract is required for read-only package extraction; install it using your distribution's package manager")?;
    if destination.exists() {
        bail!(
            "extraction destination already exists; choose a new directory to preserve existing files"
        );
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let staging = tempfile::Builder::new()
        .prefix(".cognac-firmware-")
        .tempdir_in(parent)
        .context("cannot create extraction staging directory beside the destination")?;
    let output = Command::new(extractor)
        .arg("--extract")
        .arg("--output-dir")
        .arg(staging.path())
        .arg(&info.path)
        .output()
        .context("could not start innoextract")?;
    let extraction_output = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        bail!(
            "package extraction failed; no executable was run: {}",
            extraction_output.trim()
        );
    }
    // Create the final directory exclusively; never overwrite an existing package.
    fs::create_dir(destination).context("cannot create a new extraction destination")?;
    for entry in fs::read_dir(staging.path())? {
        let entry = entry?;
        fs::rename(entry.path(), destination.join(entry.file_name()))?;
    }
    let host = detect_host(Path::new("/sys"));
    let mut capsules = Vec::new();
    let mut capsule_errors = Vec::new();
    let mut payload_executables = Vec::new();
    let mut executable_errors = Vec::new();
    for entry in WalkDir::new(destination).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        if entry
            .path()
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        {
            match analyzer::analyze(entry.path()) {
                Ok(info) => payload_executables.push(info),
                Err(error) => {
                    executable_errors.push(format!("{}: {error:#}", entry.path().display()))
                }
            }
        }
        if !entry
            .path()
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("cap"))
        {
            continue;
        }
        match inspect_capsule(entry.path(), &host.esrt) {
            Ok(capsule) => capsules.push(capsule),
            Err(error) => capsule_errors.push(format!("{}: {error:#}", entry.path().display())),
        }
    }
    capsules.sort_by(|a, b| a.path.cmp(&b.path));
    payload_executables.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(FirmwareInspection {
        executable: info,
        extraction_directory: destination.canonicalize()?,
        extraction_output,
        host, capsules, capsule_errors, payload_executables, executable_errors,
        limitations: vec![
            "Extraction and header checks do not authenticate the capsule or prove compatibility with this computer".into(),
            "A capsule GUID matching ESRT is identification evidence only; a different GUID may be a vendor container".into(),
            "The firmware's maximum capsule size must be queried in UEFI before submitting an update; this inspection does not call UpdateCapsule or schedule a reboot".into(),
            "Wine, Proton, and Windows guest snapshots do not provide a supported physical motherboard flash or recovery path".into(),
        ],
    })
}

fn read_text(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

fn read_number(path: &Path) -> Option<u32> {
    read_text(path)?.parse().ok()
}

fn detect_host(sys: &Path) -> FirmwareHost {
    let dmi = sys.join("class/dmi/id");
    let mut esrt = Vec::new();
    let entries = fs::read_dir(sys.join("firmware/efi/esrt/entries"));
    let mut esrt_status = match &entries {
        Ok(_) => "ESRT directory readable; inaccessible entry fields are reported as null".into(),
        Err(error) => format!("ESRT unavailable: {error}"),
    };
    if let Ok(entries) = entries {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(guid) = read_text(&path.join("fw_class")) {
                esrt.push(FirmwareResource {
                    guid,
                    firmware_type: read_number(&path.join("fw_type")),
                    version: read_number(&path.join("fw_version")),
                    last_attempt_version: read_number(&path.join("last_attempt_version")),
                    last_attempt_status: read_number(&path.join("last_attempt_status")),
                });
            } else {
                esrt_status.push_str(&format!(
                    "; cannot read firmware GUID at {}",
                    path.display()
                ));
            }
        }
    }
    esrt.sort_by(|a, b| a.guid.cmp(&b.guid));
    FirmwareHost {
        vendor: read_text(&dmi.join("sys_vendor")),
        product: read_text(&dmi.join("product_name")),
        bios_version: read_text(&dmi.join("bios_version")),
        bios_date: read_text(&dmi.join("bios_date")),
        esrt,
        esrt_status,
    }
}

fn inspect_capsule(path: &Path, resources: &[FirmwareResource]) -> Result<CapsuleInspection> {
    let mut file = fs::File::open(path)?;
    let size = file.metadata()?.len();
    let mut header = [0u8; 28];
    file.read_exact(&mut header)
        .context("file is shorter than a UEFI capsule header")?;
    let mut hash = Sha256::new();
    hash.update(header);
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    let guid = capsule_guid(&header);
    let number = |i| u32::from_le_bytes(header[i..i + 4].try_into().expect("fixed header slice"));
    let header_size = number(16);
    let flags = number(20);
    let declared_size = number(24);
    Ok(CapsuleInspection {
        path: path.canonicalize()?,
        sha256: hex::encode(hash.finalize()),
        size,
        capsule_guid: guid.clone(),
        header_size,
        flags,
        declared_size,
        header_bounds_valid: header_size >= 28
            && u64::from(header_size) <= size
            && u64::from(declared_size) == size,
        matching_esrt_guids: resources
            .iter()
            .filter(|r| r.guid.eq_ignore_ascii_case(&guid))
            .map(|r| r.guid.clone())
            .collect(),
    })
}

fn capsule_guid(h: &[u8; 28]) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        u32::from_le_bytes(h[0..4].try_into().unwrap()),
        u16::from_le_bytes(h[4..6].try_into().unwrap()),
        u16::from_le_bytes(h[6..8].try_into().unwrap()),
        h[8],
        h[9],
        h[10],
        h[11],
        h[12],
        h[13],
        h[14],
        h[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnoses_truncated_and_misdeclared_capsules() {
        let file = tempfile::NamedTempFile::new().unwrap();
        fs::write(file.path(), [0u8; 27]).unwrap();
        assert!(inspect_capsule(file.path(), &[]).is_err());
        let mut h = [0u8; 28];
        h[16..20].copy_from_slice(&28u32.to_le_bytes());
        h[24..28].copy_from_slice(&29u32.to_le_bytes());
        fs::write(file.path(), h).unwrap();
        assert!(
            !inspect_capsule(file.path(), &[])
                .unwrap()
                .header_bounds_valid
        );
        h[24..28].copy_from_slice(&28u32.to_le_bytes());
        fs::write(file.path(), h).unwrap();
        assert!(
            inspect_capsule(file.path(), &[])
                .unwrap()
                .header_bounds_valid
        );
    }
    #[test]
    fn reads_mixed_endian_guid_without_claiming_compatibility() {
        let mut h = [0u8; 28];
        h[..16].copy_from_slice(&[
            0xd3, 0xaf, 0x0b, 0xe2, 0x14, 0x99, 0x4f, 0x4f, 0x95, 0x37, 0x31, 0x29, 0xe0, 0x90,
            0xeb, 0x3c,
        ]);
        assert_eq!(capsule_guid(&h), "e20bafd3-9914-4f4f-9537-3129e090eb3c");
    }
    #[test]
    fn non_uefi_hosts_remain_inspectable() {
        let sys = tempfile::tempdir().unwrap();
        let host = detect_host(sys.path());
        assert!(host.esrt.is_empty());
        assert!(host.bios_version.is_none());
    }
}
