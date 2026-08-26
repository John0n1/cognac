use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

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
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatibilityPlan {
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
    pub quality: ResultQuality,
    pub limitations: Vec<String>,
    pub source_sha256: Option<String>,
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
    DisableDxvk,
    UseOpenGl,
    ChangeWindowsVersion,
    FallbackRunner,
}

#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub status: Option<i32>,
    pub log_path: PathBuf,
    pub output: String,
}
