use crate::{model::HostInfo, util::command_exists};
use anyhow::{Context, Result};
use std::{collections::HashMap, fs, path::Path};

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
        "/usr/lib/libvulkan.so.1",
    ]);
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
        issues,
    })
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
