use crate::{paths::CognacPaths, util::find_command};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{fs, process::Command};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VmConfig {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    #[serde(default = "default_uri")]
    pub connection_uri: String,
    pub domain: String,
    #[serde(default)]
    pub licensed_windows: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VmCapabilityReport {
    pub configured: bool,
    pub domain_available: bool,
    pub guest_agent_channel: bool,
    pub efi: bool,
    pub secure_boot: bool,
    pub virtual_tpm: bool,
    pub blockers: Vec<String>,
}

pub fn probe(paths: &CognacPaths) -> Result<VmCapabilityReport> {
    let config_path = paths.config.join("windows-vm.json");
    if !config_path.exists() {
        return Ok(VmCapabilityReport {
            blockers: vec![format!(
                "one-time Windows VM configuration is absent ({})",
                config_path.display()
            )],
            ..VmCapabilityReport::default()
        });
    }
    let config: VmConfig = serde_json::from_slice(&fs::read(&config_path)?)
        .with_context(|| format!("invalid VM configuration in {}", config_path.display()))?;
    validate_config(&config)?;
    let Some(virsh) = find_command("virsh") else {
        return Ok(VmCapabilityReport {
            blockers: vec!["virsh/libvirt is not installed".into()],
            ..VmCapabilityReport::default()
        });
    };
    let mut report = VmCapabilityReport::default();
    if !config.licensed_windows {
        report.blockers.push(
            "the configured Windows environment has no recorded license acknowledgement".into(),
        );
    }
    let output = Command::new(&virsh)
        .args([
            "--connect",
            &config.connection_uri,
            "dumpxml",
            &config.domain,
        ])
        .output()
        .context("could not query the configured libvirt domain")?;
    if !output.status.success() {
        report.blockers.push(format!(
            "libvirt domain `{}` is unavailable: {}",
            config.domain,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
        return Ok(report);
    }
    report.domain_available = true;
    let xml = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    report.guest_agent_channel = xml.contains("org.qemu.guest_agent.0");
    report.efi = xml.contains("firmware='efi'")
        || xml.contains("firmware=\"efi\"")
        || xml.contains("<loader");
    report.secure_boot = (xml.contains("secure-boot") && xml.contains("enabled='yes'"))
        || xml.contains("secure='yes'")
        || xml.contains("secure=\"yes\"");
    report.virtual_tpm = xml.contains("<tpm") && xml.contains("version='2.0'")
        || xml.contains("<tpm") && xml.contains("version=\"2.0\"");
    if !report.guest_agent_channel {
        report
            .blockers
            .push("the VM has no private QEMU guest-agent channel".into());
    }
    report.configured = config.licensed_windows
        && report.domain_available
        && report.guest_agent_channel
        && report.blockers.is_empty();
    Ok(report)
}

fn validate_config(config: &VmConfig) -> Result<()> {
    if config.schema_version != 1 {
        bail!("unsupported Windows VM configuration schema");
    }
    if config.domain.is_empty()
        || config.domain.len() > 128
        || !config
            .domain
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character))
    {
        bail!("invalid libvirt domain name in Windows VM configuration");
    }
    if !matches!(
        config.connection_uri.as_str(),
        "qemu:///system" | "qemu:///session"
    ) {
        bail!("Cognac VM connections must use qemu:///system or qemu:///session");
    }
    Ok(())
}

fn schema_version() -> u32 {
    1
}

fn default_uri() -> String {
    "qemu:///system".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_remote_or_unsafe_libvirt_configuration() {
        let remote = VmConfig {
            schema_version: 1,
            connection_uri: "qemu+ssh://host/system".into(),
            domain: "windows".into(),
            licensed_windows: true,
        };
        assert!(validate_config(&remote).is_err());
        let unsafe_domain = VmConfig {
            connection_uri: "qemu:///system".into(),
            domain: "windows;shutdown".into(),
            ..remote
        };
        assert!(validate_config(&unsafe_domain).is_err());
    }

    #[test]
    fn missing_configuration_is_a_capability_result() {
        let directory = tempfile::tempdir().unwrap();
        let paths = CognacPaths {
            data: directory.path().join("data"),
            cache: directory.path().join("cache"),
            config: directory.path().join("config"),
            state: directory.path().join("state"),
        };
        let report = probe(&paths).unwrap();
        assert!(!report.configured);
        assert!(!report.blockers.is_empty());
    }
}
