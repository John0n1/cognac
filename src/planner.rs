use crate::{
    knowledge::Profile,
    model::{Architecture, CompatibilityPlan, ExecutableInfo, HostInfo},
};
use std::collections::BTreeMap;

pub fn build(
    info: &ExecutableInfo,
    host: &HostInfo,
    profile: Option<&Profile>,
) -> CompatibilityPlan {
    let mut components = Vec::new();
    let mut reasons = Vec::new();
    if info.frameworks.iter().any(|f| f == "visual-c++") {
        components.push("vcrun2022".into());
        reasons.push("Visual C++ runtime imports detected".into());
    }
    if info.frameworks.iter().any(|f| f == "dotnet") {
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
    if !host.vulkan_available && (graphics_backend == "dxvk" || graphics_backend == "vkd3d") {
        graphics_backend = "opengl".into();
        reasons.push("Vulkan is unavailable, so graphics will initially use OpenGL".into());
    }
    if graphics_backend == "dxvk" {
        components.push("dxvk".into());
    }
    if graphics_backend == "vkd3d" {
        components.push("vkd3d".into());
    }
    let mut plan = CompatibilityPlan {
        runner_channel: "staging".into(),
        runner_fallbacks: vec!["stable".into()],
        prefix_architecture: Architecture::X64,
        windows_version: "win10".into(),
        components,
        dll_overrides: BTreeMap::new(),
        graphics_backend,
        environment: BTreeMap::new(),
        reasons,
        profile_id: None,
    };
    if info.architecture == Architecture::X86 {
        plan.reasons
            .push("32-bit executable will run inside a unified WoW64 environment".into());
    }
    if let Some(profile) = profile {
        plan.profile_id = Some(profile.id.clone());
        if let Some(value) = &profile.runner {
            plan.runner_channel = value.clone();
        }
        if let Some(value) = profile.architecture {
            plan.prefix_architecture = value;
        }
        if let Some(value) = &profile.windows_version {
            plan.windows_version = value.clone();
        }
        if let Some(value) = &profile.graphics {
            plan.graphics_backend = value.clone();
        }
        plan.dll_overrides.extend(profile.dll_overrides.clone());
        plan.components.extend(profile.components.clone());
        plan.reasons
            .push(format!("matched compatibility profile {}", profile.id));
    }
    plan.components.sort();
    plan.components.dedup();
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ExecutableInfo, InstallerType};
    use std::path::PathBuf;
    fn fixture() -> ExecutableInfo {
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
        }
    }
    #[test]
    fn plans_x86_in_unified_wow64() {
        let host = HostInfo {
            distro_id: "x".into(),
            distro_family: "x".into(),
            version: "".into(),
            architecture: "x86_64".into(),
            package_manager: None,
            vulkan_available: true,
            vulkan_32bit_likely: true,
            desktop_environment: None,
            issues: vec![],
        };
        let plan = build(&fixture(), &host, None);
        assert_eq!(plan.prefix_architecture, Architecture::X64);
        assert_eq!(plan.graphics_backend, "dxvk");
        assert!(plan.components.contains(&"vcrun2022".into()));
    }
}
