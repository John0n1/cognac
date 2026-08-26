use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum ApplicationClass {
    Game,
    Productivity,
    Media,
    Legacy,
    SystemUtility,
    DriverPackage,
    #[default]
    General,
}

impl std::fmt::Display for ApplicationClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Game => "game",
            Self::Productivity => "productivity",
            Self::Media => "media",
            Self::Legacy => "legacy",
            Self::SystemUtility => "system utility",
            Self::DriverPackage => "driver package",
            Self::General => "general application",
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustRequirements {
    pub elevation_likely: bool,
    pub windows_service_likely: bool,
    pub kernel_driver_likely: bool,
    pub anti_cheat: Vec<String>,
    pub tpm_likely: bool,
    pub secure_boot_likely: bool,
    pub direct_hardware_access_likely: bool,
    pub evidence: Vec<String>,
}

impl TrustRequirements {
    pub fn requires_windows_kernel(&self) -> bool {
        self.kernel_driver_likely
            || self.tpm_likely
            || self.secure_boot_likely
            || self.direct_hardware_access_likely
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionClass {
    #[default]
    Wine,
    ProtonUmu,
    ContainerizedWine,
    VirtualMachine,
    Restricted,
}

impl std::fmt::Display for ExecutionClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Wine => "Wine",
            Self::ProtonUmu => "Proton / UMU",
            Self::ContainerizedWine => "containerized Wine",
            Self::VirtualMachine => "Windows VM",
            Self::Restricted => "unsupported / restricted",
        })
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StrategyAvailability {
    Ready,
    #[default]
    Provisionable,
    Blocked,
}

impl std::fmt::Display for StrategyAvailability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Ready => "ready",
            Self::Provisionable => "provisionable",
            Self::Blocked => "blocked",
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionStrategy {
    pub class: ExecutionClass,
    pub backend: String,
    pub availability: StrategyAvailability,
    pub score: i32,
    pub reasons: Vec<String>,
    pub blockers: Vec<String>,
}

impl ExecutionStrategy {
    pub fn id(&self) -> String {
        format!("{}:{}", self.class, self.backend)
            .to_ascii_lowercase()
            .replace([' ', '/'], "-")
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostCapabilities {
    pub python3: bool,
    pub umu_launcher: bool,
    pub proton_installation: bool,
    pub bubblewrap: bool,
    pub podman: bool,
    pub cpu_virtualization: bool,
    pub kvm_device: bool,
    pub kvm_usable: bool,
    pub qemu: bool,
    pub libvirt: bool,
    pub windows_vm_configured: bool,
    pub ovmf: bool,
    pub swtpm: bool,
    pub iommu: bool,
    pub vfio: bool,
    pub render_node: bool,
    pub tpm: bool,
    pub pipewire: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Architecture {
    X86,
    #[serde(rename = "x86_64")]
    X64,
    Arm64,
    Unknown,
}

impl std::fmt::Display for Architecture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::X86 => "x86",
            Self::X64 => "x86_64",
            Self::Arm64 => "arm64",
            Self::Unknown => "unknown",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum InstallerType {
    Msi,
    Inno,
    Nsis,
    InstallShield,
    Burn,
    Squirrel,
    PortableOrUnknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutableInfo {
    pub path: PathBuf,
    pub sha256: String,
    pub size: u64,
    pub architecture: Architecture,
    pub installer_type: InstallerType,
    pub product_name: Option<String>,
    pub publisher: Option<String>,
    pub imports: Vec<String>,
    pub graphics_apis: Vec<String>,
    pub frameworks: Vec<String>,
    pub indicators: Vec<String>,
    #[serde(default)]
    pub application_class: ApplicationClass,
    #[serde(default)]
    pub trust: TrustRequirements,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    pub distro_id: String,
    pub distro_family: String,
    pub version: String,
    pub architecture: String,
    pub package_manager: Option<String>,
    pub vulkan_available: bool,
    pub vulkan_32bit_likely: bool,
    pub desktop_environment: Option<String>,
    #[serde(default)]
    pub capabilities: HostCapabilities,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatibilityPlan {
    pub execution: ExecutionStrategy,
    pub execution_fallbacks: Vec<ExecutionStrategy>,
    pub runner_channel: String,
    pub runner_fallbacks: Vec<String>,
    pub prefix_architecture: Architecture,
    pub windows_version: String,
    pub components: Vec<String>,
    pub dll_overrides: BTreeMap<String, String>,
    pub graphics_backend: String,
    pub environment: BTreeMap<String, String>,
    pub reasons: Vec<String>,
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ResultQuality {
    FullyFunctional,
    Functional,
    FunctionalWithLimitations,
    Unverified,
    Failed,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionClassification {
    NativeCompatible,
    CompatibilityLayer,
    Virtualized,
    Degraded,
    Unsupported,
    #[default]
    Unverified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledApp {
    pub app_id: String,
    pub name: String,
    pub executable: PathBuf,
    pub prefix: PathBuf,
    pub runner: PathBuf,
    pub architecture: Architecture,
    pub installed_at: String,
    pub icon: Option<PathBuf>,
    pub launch_arguments: Vec<String>,
    #[serde(default)]
    pub launch_environment: BTreeMap<String, String>,
    pub quality: ResultQuality,
    pub limitations: Vec<String>,
    pub source_sha256: Option<String>,
    #[serde(default)]
    pub execution_class: ExecutionClass,
    #[serde(default)]
    pub execution_backend: String,
    #[serde(default)]
    pub execution_classification: ExecutionClassification,
}

#[derive(Debug, Clone)]
pub struct Failure {
    pub category: &'static str,
    pub summary: String,
    pub retryable: bool,
    pub repair: Option<Repair>,
}

#[derive(Debug, Clone, Copy)]
pub enum Repair {
    AddVcrun,
    AddDotNet,
    AddMediaFoundation,
    AddDirectXCompiler,
    AddXact,
    DisableDxvk,
    DisableSync,
    UseOpenGl,
    ChangeWindowsVersion,
    FallbackRunner,
    FallbackExecutionClass,
}

#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub status: Option<i32>,
    pub log_path: PathBuf,
    pub output: String,
}
