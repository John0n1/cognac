use crate::{
    knowledge::Profile,
    model::{
        ApplicationClass, Architecture, CompatibilityPlan, ExecutableInfo, ExecutionClass,
        ExecutionStrategy, HostInfo, StrategyAvailability,
    },
    strategy::StrategyRecord,
};
use std::collections::BTreeMap;

pub fn build(
    info: &ExecutableInfo,
    host: &HostInfo,
    profile: Option<&Profile>,
    learned: Option<&StrategyRecord>,
) -> CompatibilityPlan {
    let mut components = Vec::new();
    let mut reasons = Vec::new();
    if info
        .frameworks
        .iter()
        .any(|framework| framework == "visual-c++")
    {
        components.push("vcrun2022".into());
        reasons.push("Visual C++ runtime imports detected".into());
    }
    if info
        .frameworks
        .iter()
        .any(|framework| framework == "dotnet")
    {
        components.push("dotnet48".into());
        reasons.push("managed .NET entry point detected".into());
    }
    let has_d3d12 = info.graphics_apis.iter().any(|api| api == "Direct3D 12");
    let has_directx = info
        .graphics_apis
        .iter()
        .any(|api| api.starts_with("Direct3D") || api == "DXGI");
    let mut graphics_backend = if has_d3d12 {
        "vkd3d"
    } else if has_directx {
        "dxvk"
    } else {
        "builtin"
    }
    .to_owned();
    if !host.vulkan_available && matches!(graphics_backend.as_str(), "dxvk" | "vkd3d") {
        graphics_backend = "opengl".into();
        reasons.push("Vulkan is unavailable, so graphics will initially use OpenGL".into());
    }
    if graphics_backend == "dxvk" {
        components.push("dxvk".into());
    }
    if graphics_backend == "vkd3d" {
        components.push("vkd3d".into());
    }

    let mut runner_channel = "staging".to_owned();
    let mut prefix_architecture = Architecture::X64;
    let mut windows_version = "win10".to_owned();
    let mut dll_overrides = BTreeMap::new();
    let mut environment = BTreeMap::new();
    let mut profile_id = None;

    if info.architecture == Architecture::X86 {
        reasons.push("32-bit executable will run inside a unified WoW64 environment".into());
    }
    if let Some(profile) = profile {
        profile_id = Some(profile.id.clone());
        if let Some(value) = &profile.runner {
            runner_channel = value.clone();
        }
        if let Some(value) = profile.architecture {
            prefix_architecture = value;
        }
        if let Some(value) = &profile.windows_version {
            windows_version = value.clone();
        }
        if let Some(value) = &profile.graphics {
            graphics_backend = value.clone();
        }
        dll_overrides.extend(profile.dll_overrides.clone());
        components.extend(profile.components.clone());
        reasons.push(format!("matched compatibility profile {}", profile.id));
    }
    components.sort();
    components.dedup();

    let (execution, execution_fallbacks) =
        select_execution(info, host, profile, learned, &runner_channel);
    if let Some(record) = learned {
        environment.insert("COGNAC_LEARNED_STRATEGY".into(), record.backend.clone());
        reasons.push(format!(
            "a previous installation succeeded with {} ({})",
            record.execution_class, record.backend
        ));
    }

    CompatibilityPlan {
        execution,
        execution_fallbacks,
        runner_channel,
        runner_fallbacks: vec!["stable".into()],
        prefix_architecture,
        windows_version,
        components,
        dll_overrides,
        graphics_backend,
        environment,
        reasons,
        profile_id,
    }
}

fn select_execution(
    info: &ExecutableInfo,
    host: &HostInfo,
    profile: Option<&Profile>,
    learned: Option<&StrategyRecord>,
    wine_channel: &str,
) -> (ExecutionStrategy, Vec<ExecutionStrategy>) {
    let requires_kernel = info.trust.requires_windows_kernel();
    let virtualization_sensitive = info.trust.anti_cheat.iter().any(|anti_cheat| {
        matches!(
            anti_cheat.as_str(),
            "Riot Vanguard" | "FACEIT Anti-Cheat" | "Ricochet" | "HoYoProtect"
        )
    });
    let is_game = info.application_class == ApplicationClass::Game;
    let supported_host_arch = host.architecture == "x86_64";

    let mut wine = ExecutionStrategy {
        class: ExecutionClass::Wine,
        backend: format!("wine-{wine_channel}"),
        availability: if supported_host_arch && !requires_kernel {
            StrategyAvailability::Provisionable
        } else {
            StrategyAvailability::Blocked
        },
        score: if is_game { 75 } else { 120 },
        reasons: vec![if is_game {
            "Wine remains a broad userspace fallback for games".into()
        } else {
            "ordinary Windows applications usually need only userspace compatibility".into()
        }],
        blockers: Vec::new(),
    };
    if !supported_host_arch {
        wine.blockers
            .push("managed Wine runners currently require an x86_64 host".into());
    }
    if requires_kernel {
        wine.blockers
            .push("the application appears to require real Windows kernel behavior".into());
    }

    let mut proton = ExecutionStrategy {
        class: ExecutionClass::ProtonUmu,
        backend: "umu".into(),
        availability: if host.capabilities.umu_launcher && supported_host_arch && !requires_kernel {
            StrategyAvailability::Ready
        } else if host.capabilities.python3 && supported_host_arch && !requires_kernel {
            StrategyAvailability::Provisionable
        } else {
            StrategyAvailability::Blocked
        },
        score: if is_game { 140 } else { 55 },
        reasons: vec![if is_game {
            "game indicators favor Proton's game-focused compatibility stack".into()
        } else {
            "Proton/UMU is available as a secondary userspace strategy".into()
        }],
        blockers: Vec::new(),
    };
    if !host.capabilities.umu_launcher && host.capabilities.python3 {
        proton
            .reasons
            .push("Cognac can download and verify the official UMU zipapp".into());
    } else if !host.capabilities.umu_launcher {
        proton
            .blockers
            .push("managed UMU requires Python 3.10 or newer".into());
    }
    if requires_kernel {
        proton
            .blockers
            .push("Proton cannot provide Windows kernel-mode drivers".into());
    }
    if !info.trust.anti_cheat.is_empty() && !requires_kernel {
        proton.reasons.push(format!(
            "anti-cheat support is vendor- and game-specific ({})",
            info.trust.anti_cheat.join(", ")
        ));
    }

    let mut proton_ge = proton.clone();
    proton_ge.backend = "umu-ge-proton".into();
    proton_ge.score -= 5;
    proton_ge.reasons = vec![
        "GE-Proton through UMU is a second game-focused runner family with additional compatibility patches"
            .into(),
    ];

    let container_available = host.capabilities.bubblewrap || host.capabilities.podman;
    let mut container = ExecutionStrategy {
        class: ExecutionClass::ContainerizedWine,
        backend: if host.capabilities.bubblewrap {
            "bubblewrap-wine".into()
        } else {
            "podman-wine".into()
        },
        availability: StrategyAvailability::Blocked,
        score: if is_game { 45 } else { 70 },
        reasons: vec![
            "filesystem isolation can reduce the host surface exposed to an application".into(),
        ],
        blockers: vec!["the container execution backend is not enabled in this release".into()],
    };
    if !container_available {
        container
            .blockers
            .push("neither Bubblewrap nor rootless Podman was detected".into());
    }
    if requires_kernel {
        container
            .blockers
            .push("a Linux container cannot supply a Windows kernel".into());
    }

    let vm_prerequisites = host.capabilities.cpu_virtualization
        && host.capabilities.kvm_usable
        && host.capabilities.qemu
        && host.capabilities.libvirt;
    let vm_ready = vm_prerequisites && host.capabilities.windows_vm_configured;
    let mut vm = ExecutionStrategy {
        class: ExecutionClass::VirtualMachine,
        backend: "libvirt-qemu".into(),
        availability: if vm_ready {
            StrategyAvailability::Ready
        } else {
            StrategyAvailability::Provisionable
        },
        score: if requires_kernel { 180 } else { 25 },
        reasons: vec![if requires_kernel {
            "real Windows kernel behavior appears to be required".into()
        } else {
            "a Windows VM is a high-cost isolation fallback".into()
        }],
        blockers: Vec::new(),
    };
    if !host.capabilities.cpu_virtualization {
        vm.blockers
            .push("CPU virtualization extensions were not detected".into());
    }
    if !host.capabilities.kvm_usable {
        vm.blockers
            .push("KVM is unavailable or the current user cannot open /dev/kvm".into());
    }
    if !host.capabilities.qemu || !host.capabilities.libvirt {
        vm.blockers
            .push("QEMU and libvirt are both required for VM execution".into());
    }
    if !host.capabilities.windows_vm_configured {
        vm.blockers
            .push("no licensed Cognac Windows VM has completed one-time provisioning".into());
    }
    if info.trust.secure_boot_likely && !host.capabilities.ovmf {
        vm.blockers
            .push("Secure Boot needs libvirt-compatible OVMF firmware".into());
    }
    if info.trust.tpm_likely && !host.capabilities.swtpm {
        vm.blockers
            .push("TPM-backed execution needs a managed swtpm 2.0 device".into());
    }
    if is_game && (!host.capabilities.iommu || !host.capabilities.vfio) {
        vm.reasons
            .push("no ready VFIO/IOMMU path was detected, so VM graphics may be degraded".into());
    }
    if virtualization_sensitive {
        vm.reasons.push(
            "the detected trust system may reject virtualization; Cognac will test honestly without concealing the hypervisor"
                .into(),
        );
    }

    let restricted = ExecutionStrategy {
        class: ExecutionClass::Restricted,
        backend: "policy".into(),
        availability: StrategyAvailability::Ready,
        score: -100,
        reasons: vec![
            "reserved for an observed hard incompatibility or an explicit safety boundary".into(),
        ],
        blockers: Vec::new(),
    };

    let mut wine_stable = wine.clone();
    wine_stable.backend = "wine-stable".into();
    wine_stable.score -= 10;
    wine_stable.reasons = vec![
        "stable Wine is isolated from the staging prefix and avoids staging-specific regressions"
            .into(),
    ];

    let mut candidates = vec![
        wine,
        wine_stable,
        proton,
        proton_ge,
        container,
        vm,
        restricted,
    ];
    if let Some(profile) = profile
        && let Some(preferred) = profile.execution_class
    {
        for candidate in &mut candidates {
            let backend_matches = profile
                .execution_backend
                .as_ref()
                .is_none_or(|backend| candidate.backend == *backend);
            if candidate.class == preferred && backend_matches {
                candidate.score += 300;
                candidate.reasons.push(format!(
                    "compatibility profile {} prefers this class",
                    profile.id
                ));
            }
        }
    }
    if let Some(learned) = learned {
        for candidate in &mut candidates {
            if candidate.class == learned.execution_class && candidate.backend == learned.backend {
                candidate.score += 1_000;
                candidate.reasons.push(
                    "this strategy succeeded previously for the same application identity".into(),
                );
            }
        }
    }

    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.class.cmp(&right.class))
    });
    let selected_index = candidates
        .iter()
        .position(|candidate| candidate.availability != StrategyAvailability::Blocked)
        .expect("the restricted strategy is always selectable");
    let selected = candidates.remove(selected_index);
    (selected, candidates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{HostCapabilities, InstallerType, TrustRequirements};
    use std::path::PathBuf;

    fn executable(class: ApplicationClass) -> ExecutableInfo {
        ExecutableInfo {
            path: PathBuf::from("a.exe"),
            sha256: "0".into(),
            size: 1,
            architecture: Architecture::X86,
            installer_type: InstallerType::PortableOrUnknown,
            product_name: None,
            publisher: None,
            imports: vec![],
            graphics_apis: vec!["Direct3D 11".into()],
            frameworks: vec!["visual-c++".into()],
            indicators: vec![],
            application_class: class,
            trust: TrustRequirements::default(),
        }
    }

    fn host() -> HostInfo {
        HostInfo {
            distro_id: "x".into(),
            distro_family: "x".into(),
            version: "".into(),
            architecture: "x86_64".into(),
            package_manager: None,
            vulkan_available: true,
            vulkan_32bit_likely: true,
            desktop_environment: None,
            capabilities: HostCapabilities::default(),
            issues: vec![],
        }
    }

    #[test]
    fn plans_x86_in_unified_wow64() {
        let plan = build(&executable(ApplicationClass::General), &host(), None, None);
        assert_eq!(plan.prefix_architecture, Architecture::X64);
        assert_eq!(plan.graphics_backend, "dxvk");
        assert!(plan.components.contains(&"vcrun2022".into()));
        assert_eq!(plan.execution.class, ExecutionClass::Wine);
    }

    #[test]
    fn games_prefer_umu_when_it_is_ready() {
        let mut host = host();
        host.capabilities.umu_launcher = true;
        let plan = build(&executable(ApplicationClass::Game), &host, None, None);
        assert_eq!(plan.execution.class, ExecutionClass::ProtonUmu);
    }

    #[test]
    fn kernel_drivers_select_a_ready_vm() {
        let mut info = executable(ApplicationClass::DriverPackage);
        info.trust.kernel_driver_likely = true;
        let mut host = host();
        host.capabilities.cpu_virtualization = true;
        host.capabilities.kvm_usable = true;
        host.capabilities.qemu = true;
        host.capabilities.libvirt = true;
        host.capabilities.windows_vm_configured = true;
        let plan = build(&info, &host, None, None);
        assert_eq!(plan.execution.class, ExecutionClass::VirtualMachine);
    }

    #[test]
    fn kernel_drivers_request_vm_provisioning_instead_of_restriction() {
        let mut info = executable(ApplicationClass::DriverPackage);
        info.trust.kernel_driver_likely = true;
        let plan = build(&info, &host(), None, None);
        assert_eq!(plan.execution.class, ExecutionClass::VirtualMachine);
        assert_eq!(
            plan.execution.availability,
            StrategyAvailability::Provisionable
        );
    }

    #[test]
    fn virtualization_sensitive_anti_cheat_is_attempted_in_a_vm() {
        let mut info = executable(ApplicationClass::Game);
        info.trust.kernel_driver_likely = true;
        info.trust.anti_cheat.push("Riot Vanguard".into());
        let mut host = host();
        host.capabilities.cpu_virtualization = true;
        host.capabilities.kvm_usable = true;
        host.capabilities.qemu = true;
        host.capabilities.libvirt = true;
        host.capabilities.windows_vm_configured = true;
        let plan = build(&info, &host, None, None);
        assert_eq!(plan.execution.class, ExecutionClass::VirtualMachine);
        assert!(
            plan.execution
                .reasons
                .iter()
                .any(|reason| reason.contains("test honestly"))
        );
    }
}
