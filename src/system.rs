use crate::{
    model::{HostCapabilities, HostInfo},
    util::command_exists,
};
use anyhow::{Context, Result};
use std::{collections::HashMap, fs, fs::OpenOptions, path::Path};

pub fn detect() -> Result<HostInfo> {
    let release = parse_os_release(Path::new("/etc/os-release")).unwrap_or_default();
    let distro_id = release
        .get("ID")
        .cloned()
        .unwrap_or_else(|| "unknown".into());
    let distro_family = family(&distro_id, release.get("ID_LIKE").map(String::as_str));
    let package_manager = ["pacman", "apt-get", "dnf", "zypper"]
        .into_iter()
        .find(|name| command_exists(name))
        .map(str::to_owned);
    let vulkan_available = command_exists("vulkaninfo")
        || library_exists(&[
            "/usr/lib/libvulkan.so.1",
            "/usr/lib64/libvulkan.so.1",
            "/usr/lib/x86_64-linux-gnu/libvulkan.so.1",
        ]);
    let vulkan_32bit_likely = library_exists(&[
        "/usr/lib32/libvulkan.so.1",
        "/usr/lib/i386-linux-gnu/libvulkan.so.1",
    ]);
    let capabilities = detect_capabilities();
    let mut issues = Vec::new();
    if !vulkan_available {
        issues.push("Vulkan loader not found; DirectX translation may fall back to OpenGL".into());
    }
    if package_manager.is_none() {
        issues.push("No supported host package manager detected".into());
    }
    if std::env::consts::ARCH != "x86_64" {
        issues.push(format!(
            "host architecture {} is not yet automatically provisioned",
            std::env::consts::ARCH
        ));
    }
    Ok(HostInfo {
        distro_id,
        distro_family,
        version: release.get("VERSION_ID").cloned().unwrap_or_default(),
        architecture: std::env::consts::ARCH.into(),
        package_manager,
        vulkan_available,
        vulkan_32bit_likely,
        desktop_environment: std::env::var("XDG_CURRENT_DESKTOP").ok(),
        capabilities,
        issues,
    })
}

fn detect_capabilities() -> HostCapabilities {
    let kvm_device = Path::new("/dev/kvm").exists();
    let kvm_usable = kvm_device
        && OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/kvm")
            .is_ok();
    HostCapabilities {
        python3: command_exists("python3"),
        umu_launcher: command_exists("umu-run"),
        proton_installation: proton_installation_exists(),
        bubblewrap: command_exists("bwrap"),
        podman: command_exists("podman"),
        cpu_virtualization: cpu_virtualization_available(),
        kvm_device,
        kvm_usable,
        qemu: command_exists("qemu-system-x86_64"),
        libvirt: command_exists("virsh"),
        windows_vm_configured: dirs::config_dir()
            .is_some_and(|path| path.join("cognac/windows-vm.json").is_file()),
        ovmf: library_exists(&[
            "/usr/share/OVMF/OVMF_CODE.fd",
            "/usr/share/edk2/x64/OVMF_CODE.fd",
            "/usr/share/edk2/ovmf/OVMF_CODE.fd",
        ]),
        swtpm: command_exists("swtpm"),
        iommu: directory_has_entries(Path::new("/sys/kernel/iommu_groups")),
        vfio: Path::new("/dev/vfio/vfio").exists(),
        render_node: directory_has_prefix(Path::new("/dev/dri"), "renderD"),
        tpm: Path::new("/dev/tpmrm0").exists() || Path::new("/dev/tpm0").exists(),
        pipewire: command_exists("pipewire")
            || std::env::var_os("XDG_RUNTIME_DIR")
                .is_some_and(|path| Path::new(&path).join("pipewire-0").exists()),
    }
}

fn cpu_virtualization_available() -> bool {
    fs::read_to_string("/proc/cpuinfo")
        .map(|contents| {
            contents.lines().any(|line| {
                line.strip_prefix("flags")
                    .and_then(|line| line.split_once(':').map(|(_, flags)| flags))
                    .is_some_and(|flags| {
                        flags
                            .split_whitespace()
                            .any(|flag| matches!(flag, "vmx" | "svm"))
                    })
            })
        })
        .unwrap_or(false)
}

fn proton_installation_exists() -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    [
        home.join(".local/share/Steam/steamapps/common"),
        home.join(".steam/root/steamapps/common"),
        home.join(".local/share/Steam/compatibilitytools.d"),
        home.join(".steam/root/compatibilitytools.d"),
    ]
    .iter()
    .any(|directory| {
        fs::read_dir(directory).ok().is_some_and(|entries| {
            entries.flatten().any(|entry| {
                let path = entry.path();
                path.join("proton").is_file()
                    || path.join("files/bin/wine").is_file()
                    || path.join("proton").join("proton").is_file()
            })
        })
    })
}

fn directory_has_entries(path: &Path) -> bool {
    fs::read_dir(path)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
}

fn directory_has_prefix(path: &Path, prefix: &str) -> bool {
    fs::read_dir(path)
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with(prefix))
            })
        })
        .unwrap_or(false)
}

fn parse_os_release(path: &Path) -> Result<HashMap<String, String>> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    Ok(contents
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((key.into(), value.trim_matches(['\'', '"']).into()))
        })
        .collect())
}

fn family(id: &str, like: Option<&str>) -> String {
    let value = format!("{id} {}", like.unwrap_or_default());
    for (needle, result) in [
        ("arch", "arch"),
        ("debian", "debian"),
        ("ubuntu", "debian"),
        ("fedora", "fedora"),
        ("rhel", "fedora"),
        ("suse", "suse"),
    ] {
        if value.split_whitespace().any(|part| part == needle) {
            return result.into();
        }
    }
    id.into()
}

fn library_exists(paths: &[&str]) -> bool {
    paths.iter().any(|path| Path::new(path).exists())
}

pub fn dependency_hint(host: &HostInfo) -> Option<String> {
    if host.vulkan_available && host.vulkan_32bit_likely {
        return None;
    }
    let packages = match host.distro_family.as_str() {
        "arch" => "vulkan-icd-loader lib32-vulkan-icd-loader",
        "debian" => "libvulkan1 libvulkan1:i386",
        "fedora" => "vulkan-loader vulkan-loader.i686",
        "suse" => "libvulkan1 libvulkan1-32bit",
        _ => {
            return Some(
                "install the 64-bit and 32-bit Vulkan loaders for this distribution".into(),
            );
        }
    };
    Some(format!("install host packages: {packages}"))
}
