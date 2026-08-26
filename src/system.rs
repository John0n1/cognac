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
        .map(|contents| parse_cpu_virtualization(&contents))
        .unwrap_or(false)
}

fn parse_cpu_virtualization(contents: &str) -> bool {
    contents.lines().any(|line| {
        line.split_once(':')
            .is_some_and(|(key, flags)| {
                key.trim() == "flags" && flags.split_whitespace().any(|flag| matches!(flag, "vmx" | "svm"))
            })
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cpu_virtualization_flags() {
        let sample = "processor\t: 0\nvendor_id\t: GenuineIntel\nflags\t\t: fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 clflush dts acpi mmx fxsr sse sse2 ss ht tm pbe syscall nx rdtscp lm constant_tsc arch_perfmon pebs bts rep_good nopl xtopology nonstop_tsc cpuid aperfmperf pni pclmulqdq dtes64 monitor ds_cpl vmx smx est tm2 ssse3 sdbg fma cx16 xtpr pdcm pcid sse4_1 sse4_2 x2apic movbe popcnt tsc_deadline_timer aes xsave avx f16c rdrand lahf_lm abm 3dnowprefetch cpuid_fault epb ssbd ibrs ibpb stibp tpr_shadow flexpriority ept vpid ept_ad fsgsbase tsc_adjust bmi1 avx2 smep bmi2 erms invpcid rdseed adx smap clflushopt intel_pt xsaveopt xsavec xgetbv1 xsaves dtherm ida arat pln pts hwp hwp_notify hwp_act_window hwp_epp md_clear flush_l1d arch_capabilities\n";
        assert!(parse_cpu_virtualization(sample));

        let sample_amd = "processor\t: 0\nflags\t\t: fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 clflush mmx fxsr sse sse2 ht syscall nx mmxext fxsr_opt pdpe1gb rdtscp lm constant_tsc rep_good nopl nonstop_tsc cpuid extd_apicid aperfmperf rapl pni pclmulqdq monitor ssse3 fma cx16 sse4_1 sse4_2 movbe popcnt aes xsave avx f16c rdrand lahf_lm cmp_legacy svm extapic cr8_legacy abm sse4a misalignsse 3dnowprefetch osvw ibs skinit wdt tce topoext perfctr_core perfctr_nb bpext perfctr_llc mwaitx cpb cat_l3 cdp_l3 hw_pstate ssbd mba ibrs ibpb stibp vmmcall fsgsbase bmi1 avx2 smep bmi2 erms invpcid cqm rdt_a rdseed adx smap clflushopt clwb sha_ni xsaveopt xsavec xgetbv1 xsaves cqm_llc cqm_occup_llc cqm_mbm_total cqm_mbm_local clzero irperf xsaveerptr rdpru wbnoinvd arat npt lbrv svm_lock nrip_save tsc_scale vmcb_clean flushbyasid decodeassists pausefilter pfthreshold avic v_vmsave_vmload vgif v_spec_ctrl umip pku ospke vaes vpclmulqdq rdpid overflow_recov succor smca fsrm\n";
        assert!(parse_cpu_virtualization(sample_amd));

        let sample_none = "processor\t: 0\nflags\t\t: fpu vme de pse tsc msr pae mce cx8\n";
        assert!(!parse_cpu_virtualization(sample_none));
    }

    #[test]
    fn maps_distro_families() {
        assert_eq!(family("arch", None), "arch");
        assert_eq!(family("manjaro", Some("arch")), "arch");
        assert_eq!(family("ubuntu", Some("debian")), "debian");
        assert_eq!(family("fedora", None), "fedora");
        assert_eq!(family("opensuse-tumbleweed", Some("suse")), "suse");
    }
}
