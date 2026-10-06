//! Vendor-independent, offline analysis of the user/kernel/hardware boundary.
//! Evidence is a dependency, never permission to execute a driver or an IOCTL.
use anyhow::{Context, Result, bail};
use goblin::pe::PE;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path};
use walkdir::WalkDir;

const MAX_IMAGE: u64 = 128 * 1024 * 1024;
const MAX_FILES: usize = 128;
const MAX_TOTAL: u64 = 512 * 1024 * 1024;
const MAX_RESOURCES: usize = 1024;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    DriverLoading,
    DeviceIoctl,
    PhysicalMemory,
    PortIo,
    FirmwareVariables,
    Usb,
    Hid,
}

#[derive(Debug, Serialize)]
pub struct Evidence {
    pub capability: Capability,
    pub imported_symbols: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Image {
    pub source: String,
    pub sha256: String,
    pub size: usize,
    pub machine: u16,
    pub subsystem: Option<u16>,
    pub kernel_image: bool,
    pub imports: Vec<String>,
    pub exports: Vec<String>,
    pub coff_functions: Vec<FunctionSymbol>,
    pub coff_error: Option<String>,
    pub capabilities: Vec<Evidence>,
    pub device_names: Vec<String>,
    pub embedded_images: Vec<Image>,
    pub resource_errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct FunctionSymbol {
    pub name: String,
    pub rva: u32,
    pub file_offset: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct NativeFirmwarePlan {
    pub analysis: HardwareReport,
    pub native: crate::firmware_flash::FlashPlan,
    pub payload_source: String,
    pub package_payload_binding_verified: bool,
}

/// The native equivalent is a trusted fwupd release, never an inferred replay
/// of Windows driver operations or the extracted package's unsigned payload.
pub fn plan_firmware(
    executable: &Path,
    payload_directory: Option<&Path>,
    device: &str,
    version: &str,
) -> Result<NativeFirmwarePlan> {
    let analysis = inspect(executable, payload_directory, None)?;
    let native = crate::firmware_flash::plan(device, version)?;
    Ok(NativeFirmwarePlan {
        analysis,
        native,
        payload_source: "trusted fwupd remote release; the supplied EXE is analyzed only".into(),
        package_payload_binding_verified: false,
    })
}

#[derive(Debug, Serialize)]
pub struct Requirement {
    pub capability: Capability,
    pub backend: String,
    pub status: String,
    pub explanation: String,
}

#[derive(Debug, Serialize)]
pub struct HardwareReport {
    pub schema_version: u32,
    pub images: Vec<Image>,
    pub requirements: Vec<Requirement>,
    pub incomplete: bool,
    pub errors: Vec<String>,
    pub limitations: Vec<String>,
}

/// Only real imports produce capability evidence. Strings alone cannot prove a
/// call, its arguments, the target hardware, or an executable's authenticity.
pub fn imported_capabilities<'a>(symbols: impl IntoIterator<Item = &'a str>) -> Vec<Evidence> {
    let mut groups = std::collections::BTreeMap::<Capability, BTreeSet<String>>::new();
    for symbol in symbols {
        let name = symbol.rsplit('!').next().unwrap_or(symbol);
        let normalized = name
            .trim_start_matches('_')
            .split('@')
            .next()
            .unwrap_or(name)
            .to_ascii_lowercase();
        let capability = match normalized.as_str() {
            "ntloaddriver" | "zwloaddriver" => Capability::DriverLoading,
            "deviceiocontrol" | "ntdeviceiocontrolfile" | "zwdeviceiocontrolfile" => {
                Capability::DeviceIoctl
            }
            "mmmapiospace" | "mmmapiospaceex" | "mmunmapiospace" | "mmgetphysicaladdress" => {
                Capability::PhysicalMemory
            }
            "read_port_uchar" | "read_port_ushort" | "read_port_ulong" | "write_port_uchar"
            | "write_port_ushort" | "write_port_ulong" => Capability::PortIo,
            "getfirmwareenvironmentvariablea"
            | "getfirmwareenvironmentvariablew"
            | "getfirmwareenvironmentvariableexa"
            | "getfirmwareenvironmentvariableexw"
            | "setfirmwareenvironmentvariablea"
            | "setfirmwareenvironmentvariablew"
            | "setfirmwareenvironmentvariableexa"
            | "setfirmwareenvironmentvariableexw"
            | "exgetfirmwareenvironmentvariable"
            | "exsetfirmwareenvironmentvariable" => Capability::FirmwareVariables,
            _ if normalized.starts_with("winusb_") => Capability::Usb,
            _ if normalized.starts_with("hidd_") || normalized.starts_with("hidp_") => {
                Capability::Hid
            }
            _ => continue,
        };
        groups
            .entry(capability)
            .or_default()
            .insert(symbol.to_owned());
    }
    groups
        .into_iter()
        .map(|(capability, imported_symbols)| Evidence {
            capability,
            imported_symbols: imported_symbols.into_iter().collect(),
        })
        .collect()
}

/// Discover embedded kernel imports for the ordinary installation planner too.
/// Malformed resources remain an explicit diagnostic, never a clean bill of health.
pub fn embedded_dependencies(bytes: &[u8], pe: &PE<'_>) -> Result<(bool, Vec<Evidence>)> {
    let mut kernel = false;
    let mut symbols = BTreeSet::new();
    for (_, payload) in resource_payloads(bytes, pe)? {
        if !payload.starts_with(b"MZ") {
            continue;
        }
        let child = PE::parse(payload).context("malformed embedded PE")?;
        kernel |= child
            .header
            .optional_header
            .as_ref()
            .is_some_and(|h| h.windows_fields.subsystem == 1)
            && child.libraries.iter().any(|dll| {
                matches!(
                    dll.to_ascii_lowercase().as_str(),
                    "ntoskrnl.exe" | "hal.dll" | "wdfldr.sys"
                )
            });
        symbols.extend(
            child
                .imports
                .iter()
                .map(|i| format!("{}!{}", i.dll, i.name)),
        );
    }
    Ok((
        kernel,
        imported_capabilities(symbols.iter().map(String::as_str)),
    ))
}

pub fn inspect(
    executable: &Path,
    payload_directory: Option<&Path>,
    export_directory: Option<&Path>,
) -> Result<HardwareReport> {
    let mut images = Vec::new();
    let mut errors = Vec::new();
    let mut files = vec![executable.canonicalize()?];
    if let Some(directory) = payload_directory {
        let directory = directory.canonicalize()?;
        if !directory.is_dir() {
            bail!("payload directory must be a directory");
        }
        for entry in WalkDir::new(directory)
            .follow_links(false)
            .sort_by_file_name()
        {
            let entry = entry?;
            if entry.file_type().is_symlink() {
                errors.push(format!("symlink skipped: {}", entry.path().display()));
                continue;
            }
            if entry.file_type().is_file()
                && entry.path().extension().is_some_and(|e| {
                    ["exe", "dll", "sys"]
                        .iter()
                        .any(|x| e.eq_ignore_ascii_case(x))
                })
            {
                let path = entry.path().canonicalize()?;
                if !files.contains(&path) {
                    files.push(path);
                }
                if files.len() > MAX_FILES {
                    bail!("package exceeds {MAX_FILES} PE images; inspect smaller subsets");
                }
            }
        }
    }
    let mut total = 0u64;
    let mut embedded = Vec::new();
    for path in files {
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file() {
            bail!("image must be a regular file: {}", path.display());
        }
        if metadata.len() > MAX_IMAGE {
            bail!("image exceeds 128 MiB analysis limit: {}", path.display());
        }
        total += metadata.len();
        if total > MAX_TOTAL {
            bail!("package exceeds 512 MiB analysis budget");
        }
        let bytes = fs::read(&path)?;
        if bytes.len() as u64 > MAX_IMAGE {
            bail!("image grew past analysis limit");
        }
        match image(&bytes, path.display().to_string(), true, &mut embedded) {
            Ok(value) => images.push(value),
            Err(error) if images.is_empty() => {
                return Err(error).context("cannot analyze entry executable");
            }
            Err(error) => errors.push(format!("{}: {error:#}", path.display())),
        }
    }
    if let Some(directory) = export_directory {
        // Names are content hashes, never resource-controlled paths. Refuse
        // existing destinations; write files exclusively in a staging folder.
        if directory.exists() {
            bail!("embedded export destination already exists");
        }
        let parent = directory
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let staging = tempfile::Builder::new()
            .prefix(".cognac-embedded-")
            .tempdir_in(parent)?;
        let mut written = BTreeSet::new();
        for (digest, bytes) in embedded {
            if written.insert(digest.clone()) {
                fs::write(staging.path().join(format!("{digest}.pe")), bytes)?;
            }
        }
        // No-clobber publication even if another process creates the target.
        fs::create_dir(directory)?;
        for entry in fs::read_dir(staging.path())? {
            let entry = entry?;
            fs::rename(entry.path(), directory.join(entry.file_name()))?;
        }
    }
    let mut capabilities = BTreeSet::new();
    let mut incomplete = !errors.is_empty();
    for image in &images {
        collect(image, &mut capabilities, &mut incomplete);
    }
    let requirements = capabilities.into_iter().map(requirement).collect();
    Ok(HardwareReport {
        schema_version: 1, images, requirements, incomplete, errors,
        limitations: vec![
            "Static imports identify dependencies, not executed operations; dynamically resolved, packed, compressed or inline hardware access may be missing. Absence of evidence is not proof of compatibility.".into(),
            "Device names and embedded PE resources are untrusted evidence. Exported images are not installed or executed; PE certificates are not authenticated by this analysis.".into(),
            "Windows IOCTL numbers and firmware opcodes require a documented or reverse-engineered protocol. Cognac never translates them into Linux IOCTLs, port writes or arbitrary physical memory access automatically.".into(),
            "Native firmware updates use firmware-devices and flash-firmware with an explicit device/version and trusted fwupd release. The Windows package is not bound to that release by this report.".into(),
        ],
    })
}

fn collect(image: &Image, capabilities: &mut BTreeSet<Capability>, incomplete: &mut bool) {
    capabilities.extend(image.capabilities.iter().map(|e| e.capability));
    *incomplete |= !image.resource_errors.is_empty() || image.coff_error.is_some();
    for child in &image.embedded_images {
        collect(child, capabilities, incomplete);
    }
}

fn requirement(capability: Capability) -> Requirement {
    let (backend, status, explanation) = match capability {
        Capability::FirmwareVariables => (
            "fwupd",
            "device-and-release-validation-required",
            "A firmware dependency can be served by fwupd only when the exact physical device and a trusted release are supported. Arbitrary Windows variable writes are not replayed.",
        ),
        Capability::Usb => (
            "protocol-adapter",
            "not-implemented",
            "WinUSB transport requires a Linux USB adapter plus verified device identity, permissions and transfer semantics; firmware payload validation remains separate.",
        ),
        Capability::Hid => (
            "protocol-adapter",
            "not-implemented",
            "HID transport requires a device-specific report protocol and identity checks. A generic HID API name does not identify that protocol.",
        ),
        Capability::DeviceIoctl => (
            "protocol-adapter",
            "not-implemented",
            "The Windows driver request ABI and its effects must be recovered before selecting a native backend. DeviceIoControl also handles ordinary files; this evidence alone does not imply flashing.",
        ),
        Capability::DriverLoading => (
            "native-driver-or-protocol-adapter",
            "not-implemented",
            "A Windows kernel driver cannot be loaded into the Linux kernel. Reimplement its documented operations through a native backend, or use a supported guest for guest-only hardware.",
        ),
        Capability::PhysicalMemory | Capability::PortIo => (
            "verified-firmware-interface",
            "not-implemented",
            "Raw register/memory access is not a portable firmware interface. A backend needs target identity, ABI, memory bounds, image authentication, recovery and post-update verification.",
        ),
    };
    Requirement {
        capability,
        backend: backend.into(),
        status: status.into(),
        explanation: explanation.into(),
    }
}

fn image(
    bytes: &[u8],
    source: String,
    recurse: bool,
    embedded: &mut Vec<(String, Vec<u8>)>,
) -> Result<Image> {
    let pe = PE::parse(bytes).context("malformed PE image")?;
    let imports: Vec<_> = pe
        .imports
        .iter()
        .map(|i| format!("{}!{}", i.dll.to_ascii_lowercase(), i.name))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let exports = pe
        .exports
        .iter()
        .filter_map(|e| e.name.map(str::to_owned))
        .collect();
    let subsystem = pe
        .header
        .optional_header
        .as_ref()
        .map(|h| h.windows_fields.subsystem);
    let kernel_image = subsystem == Some(1)
        && pe.libraries.iter().any(|dll| {
            matches!(
                dll.to_ascii_lowercase().as_str(),
                "ntoskrnl.exe" | "hal.dll" | "wdfldr.sys"
            )
        });
    let capabilities = imported_capabilities(imports.iter().map(String::as_str));
    let (coff_functions, coff_error) = match coff_functions(bytes, &pe) {
        Ok(functions) => (functions, None),
        Err(error) => (vec![], Some(format!("{error:#}"))),
    };
    let mut result = Image {
        source,
        sha256: hex::encode(Sha256::digest(bytes)),
        size: bytes.len(),
        machine: pe.header.coff_header.machine,
        subsystem,
        kernel_image,
        imports,
        exports,
        coff_functions,
        coff_error,
        capabilities,
        device_names: device_names(bytes),
        embedded_images: vec![],
        resource_errors: vec![],
    };
    if recurse {
        match resource_payloads(bytes, &pe) {
            Ok(resources) => {
                for (offset, payload) in resources {
                    if !payload.starts_with(b"MZ") {
                        continue;
                    }
                    match image(
                        payload,
                        format!("{}#resource@0x{offset:x}", result.source),
                        false,
                        embedded,
                    ) {
                        Ok(child) => {
                            if !embedded.iter().any(|(hash, _)| hash == &child.sha256) {
                                let total: usize =
                                    embedded.iter().map(|(_, bytes)| bytes.len()).sum();
                                if embedded.len() >= MAX_FILES
                                    || total + payload.len() > MAX_TOTAL as usize
                                {
                                    bail!("embedded PE export budget exceeded");
                                }
                                embedded.push((child.sha256.clone(), payload.to_vec()));
                            }
                            result.embedded_images.push(child);
                        }
                        Err(error) => result
                            .resource_errors
                            .push(format!("PE resource at 0x{offset:x}: {error:#}")),
                    }
                }
            }
            Err(error) => result.resource_errors.push(format!("{error:#}")),
        }
    }
    Ok(result)
}

fn coff_functions(bytes: &[u8], pe: &PE<'_>) -> Result<Vec<FunctionSymbol>> {
    let header = &pe.header.coff_header;
    if header.number_of_symbol_table > 65536 {
        bail!("COFF symbol analysis budget exceeded");
    }
    let Some(symbols) = header.symbols(bytes)? else {
        return Ok(vec![]);
    };
    let strings = header.strings(bytes)?;
    let mut result = Vec::new();
    for (_, inline_name, symbol) in symbols.iter() {
        if symbol.derived_type() != 2 || symbol.section_number <= 0 {
            continue;
        }
        let name = if let Some(name) = inline_name {
            name.to_owned()
        } else {
            symbol
                .name(strings.as_ref().context("missing COFF strings")?)?
                .to_owned()
        };
        let section = pe
            .sections
            .get(symbol.section_number as usize - 1)
            .context("COFF function references invalid section")?;
        let rva = section
            .virtual_address
            .checked_add(symbol.value)
            .context("COFF function RVA overflow")?;
        let file_offset = rva_offset(pe, rva, 1, bytes.len()).ok();
        result.push(FunctionSymbol {
            name,
            rva,
            file_offset,
        });
    }
    Ok(result)
}

fn device_names(bytes: &[u8]) -> Vec<String> {
    let mut names = BTreeSet::new();
    // Only bounded printable runs; do not allocate every string in an installer.
    for wide in [false, true] {
        let step = if wide { 2 } else { 1 };
        for alignment in 0..step.min(bytes.len()) {
            let mut run = String::new();
            for chunk in bytes[alignment..].chunks_exact(step) {
                let printable = (32..=126).contains(&chunk[0]) && (!wide || chunk[1] == 0);
                if printable && run.len() < 512 {
                    run.push(chunk[0] as char);
                } else {
                    capture_device_name(&run, &mut names);
                    run.clear();
                }
            }
            capture_device_name(&run, &mut names);
        }
    }
    names.into_iter().collect()
}

fn capture_device_name(run: &str, names: &mut BTreeSet<String>) {
    if names.len() >= 128 {
        return;
    }
    if let Some(start) = [r"\Device\", r"\DosDevices\", r"\\.\", r"\??\"]
        .iter()
        .filter_map(|prefix| run.find(prefix))
        .min()
    {
        names.insert(run[start..].to_owned());
    }
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset.checked_add(4).context("offset overflow")?)
            .context("truncated resource directory")?
            .try_into()?,
    ))
}

fn rva_offset(pe: &PE<'_>, rva: u32, size: u32, length: usize) -> Result<usize> {
    for section in &pe.sections {
        if let Some(relative) = rva.checked_sub(section.virtual_address)
            && relative
                .checked_add(size)
                .is_some_and(|end| end <= section.size_of_raw_data)
        {
            let offset = section.pointer_to_raw_data as usize + relative as usize;
            if offset
                .checked_add(size as usize)
                .is_some_and(|end| end <= length)
            {
                return Ok(offset);
            }
        }
    }
    bail!("resource RVA/length outside file-backed sections")
}

fn resource_payloads<'a>(bytes: &'a [u8], pe: &PE<'_>) -> Result<Vec<(usize, &'a [u8])>> {
    let Some(directory) = pe
        .header
        .optional_header
        .as_ref()
        .and_then(|h| h.data_directories.get_resource_table())
    else {
        return Ok(vec![]);
    };
    let base = rva_offset(pe, directory.virtual_address, directory.size, bytes.len())?;
    let table = &bytes[base..base + directory.size as usize];
    let mut pending = vec![(0usize, 0usize)];
    let mut visited = BTreeSet::new();
    let mut payloads = Vec::new();
    let mut budget = MAX_RESOURCES;
    let mut payload_budget = MAX_TOTAL;
    let mut seen_payloads = BTreeSet::new();
    while let Some((offset, depth)) = pending.pop() {
        if depth > 8 || !visited.insert(offset) {
            bail!("resource directory cycle or excessive depth");
        }
        let counts = u32_at(table, offset.checked_add(12).context("offset overflow")?)?;
        let count = (counts & 0xffff) as usize + (counts >> 16) as usize;
        budget = budget
            .checked_sub(count)
            .context("resource entry budget exceeded")?;
        for index in 0..count {
            let entry = offset
                .checked_add(16 + index * 8)
                .context("offset overflow")?;
            let target = u32_at(table, entry + 4)?;
            let target_offset = (target & 0x7fff_ffff) as usize;
            if target & 0x8000_0000 != 0 {
                pending.push((target_offset, depth + 1));
            } else {
                let rva = u32_at(table, target_offset)?;
                let size = u32_at(table, target_offset + 4)?;
                let file_offset = rva_offset(pe, rva, size, bytes.len())?;
                if seen_payloads.insert((file_offset, size)) {
                    payload_budget = payload_budget
                        .checked_sub(size as u64)
                        .context("resource payload analysis budget exceeded")?;
                    payloads.push((
                        file_offset,
                        &bytes[file_offset..file_offset + size as usize],
                    ));
                }
            }
        }
    }
    Ok(payloads)
}

/// Decode the documented CTL_CODE bit layout, without guessing vendor semantics.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct IoctlCode {
    pub value: u32,
    pub device_type: u16,
    pub function: u16,
    pub access: u8,
    pub method: u8,
    pub vendor_device_type: bool,
    pub vendor_function: bool,
}
pub fn decode_ioctl(value: u32) -> IoctlCode {
    IoctlCode {
        value,
        device_type: (value >> 16) as u16,
        function: ((value >> 2) & 0xfff) as u16,
        access: ((value >> 14) & 3) as u8,
        method: (value & 3) as u8,
        vendor_device_type: value & 0x8000_0000 != 0,
        vendor_function: value & 0x2000 != 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dependencies_are_vendor_independent_and_deduplicated() {
        let evidence = imported_capabilities([
            "kernel32.dll!DeviceIoControl",
            "kernel32.dll!DeviceIoControl",
            "hal.dll!_WRITE_PORT_UCHAR@8",
            "ntoskrnl.exe!MmMapIoSpace",
            "hid.dll!HidD_GetFeature",
            "winusb.dll!WinUsb_ControlTransfer",
        ]);
        assert_eq!(evidence.len(), 5);
        assert_eq!(evidence[0].imported_symbols.len(), 1);
        assert!(evidence.iter().any(|e| e.capability == Capability::PortIo));
    }
    #[test]
    fn generic_service_and_file_apis_do_not_prove_a_hardware_dependency() {
        assert!(
            imported_capabilities([
                "CreateServiceW",
                "CreateFileW",
                "ReadFile",
                "SetupDiGetClassDevsW"
            ])
            .is_empty()
        );
        assert_eq!(
            requirement(Capability::DeviceIoctl).status,
            "not-implemented"
        );
    }
    #[test]
    fn ioctl_decode_preserves_access_method_and_vendor_bits() {
        let code = decode_ioctl((0x8337 << 16) | (3 << 14) | (0xabc << 2) | 2);
        assert_eq!(
            (code.device_type, code.function, code.access, code.method),
            (0x8337, 0xabc, 3, 2)
        );
        assert!(code.vendor_device_type && code.vendor_function);
    }
    #[test]
    fn device_paths_are_captured_in_both_encodings() {
        let mut bytes = b"\\\\.\\ExampleA\0".to_vec();
        for unit in "\\Device\\ExampleB\0".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(
            device_names(&bytes),
            vec![r"\Device\ExampleB", r"\\.\ExampleA"]
        );
    }
    fn put16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn fixture(machine: u16, dll: &str, api: &str) -> Vec<u8> {
        let wide = machine != 0x14c;
        let optional = if wide { 240 } else { 224 };
        let directories = if wide { 264 } else { 248 };
        let section = 152 + optional;
        let mut bytes = vec![0; 1024];
        bytes[..2].copy_from_slice(b"MZ");
        put32(&mut bytes, 60, 128);
        bytes[128..132].copy_from_slice(b"PE\0\0");
        put16(&mut bytes, 132, machine);
        put16(&mut bytes, 134, 1);
        put16(&mut bytes, 148, optional as u16);
        put16(&mut bytes, 150, 0x102);
        put16(&mut bytes, 152, if wide { 0x20b } else { 0x10b });
        put32(&mut bytes, 184, 4096);
        put32(&mut bytes, 188, 512);
        put32(&mut bytes, 208, 8192);
        put32(&mut bytes, 212, 512);
        put16(&mut bytes, 220, 1);
        put32(&mut bytes, directories - 4, 16);
        bytes[section..section + 5].copy_from_slice(b".data");
        put32(&mut bytes, section + 8, 512);
        put32(&mut bytes, section + 12, 4096);
        put32(&mut bytes, section + 16, 512);
        put32(&mut bytes, section + 20, 512);
        put32(&mut bytes, directories + 8, 4096);
        put32(&mut bytes, directories + 12, 40);
        put32(&mut bytes, 512, 0x1030);
        put32(&mut bytes, 524, 0x1060);
        put32(&mut bytes, 528, 0x1040);
        put32(&mut bytes, 560, 0x1080);
        bytes[608..608 + dll.len()].copy_from_slice(dll.as_bytes());
        bytes[642..642 + api.len()].copy_from_slice(api.as_bytes());
        bytes
    }
    fn wrapper(payload: &[u8]) -> Vec<u8> {
        let mut bytes = fixture(0x14c, "kernel32.dll", "CreateFileW");
        bytes.resize(2560, 0);
        // Replace import directory with one RT_RCDATA leaf; its bytes are a PE.
        put32(&mut bytes, 256, 0);
        put32(&mut bytes, 260, 0);
        put32(&mut bytes, 264, 4096);
        put32(&mut bytes, 268, 2048);
        put32(&mut bytes, 384, 2048);
        put32(&mut bytes, 392, 2048);
        bytes[512..768].fill(0);
        put16(&mut bytes, 526, 1);
        put32(&mut bytes, 528, 10);
        put32(&mut bytes, 532, 24);
        put32(&mut bytes, 536, 0x1100);
        put32(&mut bytes, 540, payload.len() as u32);
        bytes[768..768 + payload.len()].copy_from_slice(payload);
        bytes
    }
    #[test]
    fn real_pe_imports_work_across_cpu_architectures_and_generic_package_names() {
        for machine in [0x14c, 0x8664, 0xaa64] {
            let bytes = wrapper(&fixture(machine, "ntoskrnl.exe", "MmMapIoSpace"));
            let report = image(&bytes, "unrelated-vendor.exe".into(), true, &mut vec![]).unwrap();
            assert!(report.capabilities.is_empty());
            assert_eq!(report.embedded_images.len(), 1);
            let driver = &report.embedded_images[0];
            assert_eq!(driver.machine, machine);
            assert!(driver.kernel_image);
            assert_eq!(
                driver.capabilities[0].capability,
                Capability::PhysicalMemory
            );
            let (kernel, dependencies) =
                embedded_dependencies(&bytes, &PE::parse(&bytes).unwrap()).unwrap();
            assert!(kernel);
            assert_eq!(dependencies[0].capability, Capability::PhysicalMemory);
        }
    }
    #[test]
    fn resource_cycles_truncation_and_unbacked_rvas_are_rejected() {
        let base = wrapper(&fixture(0x14c, "ntoskrnl.exe", "MmMapIoSpace"));
        for (offset, value) in [
            (532, 0x8000_0000),
            (532, 2046),
            (536, 0xffff_ffff),
            (540, 0xffff_ffff),
            (524, 0xffff_ffff),
        ] {
            let mut bytes = base.clone();
            put32(&mut bytes, offset, value);
            // Use a PE with its resource directory disabled to isolate the
            // bounded walker from goblin's independent resource validation.
            let mut header = bytes.clone();
            put32(&mut header, 264, 0);
            put32(&mut header, 268, 0);
            let mut pe = PE::parse(&header).unwrap();
            let dd = goblin::pe::data_directories::DataDirectory {
                virtual_address: 4096,
                size: 2048,
            };
            pe.header
                .optional_header
                .as_mut()
                .unwrap()
                .data_directories
                .data_directories[2] = Some((0, dd));
            assert!(
                resource_payloads(&bytes, &pe).is_err(),
                "mutation at {offset}"
            );
        }
    }
    #[test]
    fn export_is_content_addressed_and_never_clobbers() {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("vendor.exe");
        let payload = fixture(0x8664, "ntoskrnl.exe", "MmMapIoSpace");
        fs::write(&entry, wrapper(&payload)).unwrap();
        let destination = dir.path().join("drivers");
        let report = inspect(&entry, None, Some(&destination)).unwrap();
        assert!(!report.incomplete);
        let hash = hex::encode(Sha256::digest(&payload));
        assert_eq!(
            fs::read(destination.join(format!("{hash}.pe"))).unwrap(),
            payload
        );
        assert!(inspect(&entry, None, Some(&destination)).is_err());
        let info = crate::analyzer::analyze(&entry).unwrap();
        assert!(info.trust.kernel_driver_likely);
        assert!(info.trust.direct_hardware_access_likely);
        assert!(crate::analyzer::ensure_application_execution(&info).is_err());
    }
    #[test]
    fn extracted_driver_dependencies_are_aggregated_without_execution() {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("package.exe");
        fs::write(&entry, fixture(0x14c, "kernel32.dll", "CreateFileW")).unwrap();
        fs::write(
            dir.path().join("bus.sys"),
            fixture(0x8664, "hal.dll", "WRITE_PORT_UCHAR"),
        )
        .unwrap();
        fs::write(dir.path().join("broken.dll"), b"not a PE").unwrap();
        let report = inspect(&entry, Some(dir.path()), None).unwrap();
        assert_eq!(report.images.len(), 2);
        assert!(report.incomplete);
        assert_eq!(report.requirements[0].capability, Capability::PortIo);
        assert_eq!(report.requirements[0].status, "not-implemented");
    }
    #[test]
    fn coff_function_index_reports_locations_and_rejects_corruption() {
        let mut bytes = fixture(0x8664, "kernel32.dll", "DeviceIoControl");
        put32(&mut bytes, 140, 1024);
        put32(&mut bytes, 144, 1);
        bytes.resize(1046, 0);
        bytes[1024..1032].copy_from_slice(b"QueryAbi");
        put32(&mut bytes, 1032, 16);
        put16(&mut bytes, 1036, 1);
        put16(&mut bytes, 1038, 32);
        bytes[1040] = 2;
        put32(&mut bytes, 1042, 4);
        let report = image(&bytes, "tool.exe".into(), true, &mut vec![]).unwrap();
        assert_eq!(report.coff_functions[0].name, "QueryAbi");
        assert_eq!(report.coff_functions[0].rva, 0x1010);
        assert_eq!(report.coff_functions[0].file_offset, Some(528));
        assert!(report.coff_error.is_none());
        put16(&mut bytes, 1036, 99);
        let report = image(&bytes, "tool.exe".into(), true, &mut vec![]).unwrap();
        assert!(report.coff_error.is_some());
    }
}
