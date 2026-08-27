use crate::{
    model::{CompatibilityPlan, ExecutableInfo, HostInfo, RunOutcome},
    paths::CognacPaths,
    progress::Progress,
    util::find_command,
};
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

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

#[derive(Debug, Clone)]
pub struct PreparedVm {
    pub backend: String,
}

pub struct VmEnvironment<'a> {
    paths: &'a CognacPaths,
    state_dir: PathBuf,
    config: VmConfig,
    virsh: PathBuf,
    active_snapshot: RefCell<Option<String>>,
}

impl<'a> VmEnvironment<'a> {
    pub fn provision(paths: &'a CognacPaths, state_dir: PathBuf, progress: &Progress) -> Result<Self> {
        let environment = Self::load(paths, state_dir)?;
        progress.update("Starting the Windows VM bridge...", None);
        environment.ensure_ready()?;
        fs::create_dir_all(&environment.state_dir)?;
        Ok(environment)
    }

    pub fn from_installed(paths: &'a CognacPaths, state_dir: PathBuf) -> Result<Self> {
        let environment = Self::load(paths, state_dir)?;
        environment.ensure_ready()?;
        Ok(environment)
    }

    fn load(paths: &'a CognacPaths, state_dir: PathBuf) -> Result<Self> {
        let config = load_config(paths)?;
        let virsh = find_command("virsh").context("virsh/libvirt is not installed")?;
        Ok(Self {
            paths,
            state_dir,
            config,
            virsh,
            active_snapshot: RefCell::new(None),
        })
    }

    pub fn prefix(&self) -> &Path {
        &self.state_dir
    }

    pub fn launcher(&self) -> PathBuf {
        self.virsh.clone()
    }

    pub fn base_environment(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("COGNAC_VM_DOMAIN".into(), self.config.domain.clone()),
            (
                "COGNAC_VM_URI".into(),
                self.config.connection_uri.clone(),
            ),
        ])
    }

    pub fn initialize(
        &self,
        _plan: &CompatibilityPlan,
        progress: &Progress,
        _log: &Path,
    ) -> Result<()> {
        progress.update("Synchronizing the Windows VM...", None);
        self.ensure_ready()?;
        self.sync_inventory()?;
        Ok(())
    }

    pub fn run(
        &self,
        executable: &Path,
        args: &[String],
        _extra: &BTreeMap<String, String>,
        log: &Path,
    ) -> Result<RunOutcome> {
        self.ensure_ready()?;
        let guest_executable = self.resolve_guest_executable(executable)?;
        let outcome = self.guest_exec_wait(&guest_executable, args)?;
        append_log(log, &format!("[cognac-vm] {guest_executable}\n{}", outcome.output))?;
        self.sync_inventory()?;
        if matches!(outcome.status, Some(0 | 194)) {
            self.discard_active_snapshot()?;
        }
        Ok(RunOutcome {
            status: outcome.status,
            log_path: log.to_path_buf(),
            output: outcome.output,
        })
    }

    pub fn launch(
        &self,
        executable: &Path,
        args: &[String],
        _extra: &BTreeMap<String, String>,
        log: &Path,
    ) -> Result<()> {
        self.ensure_ready()?;
        let guest_executable = self.resolve_guest_executable(executable)?;
        let pid = self.guest_exec_start(&guest_executable, args, false)?;
        append_log(
            log,
            &format!("[cognac-vm] launched {guest_executable} through QEMU Guest Agent (pid {pid})"),
        )?;
        Ok(())
    }

    pub fn snapshot(&self, app_id: &str, attempt: usize) -> Result<PathBuf> {
        self.ensure_ready()?;
        if self.active_snapshot.borrow().is_some() {
            bail!("a Cognac VM snapshot is already active");
        }
        let safe = app_id
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                    character
                } else {
                    '-'
                }
            })
            .collect::<String>();
        let name = format!("cognac-{safe}-{attempt}-{}", unix_millis());
        let output = Command::new(&self.virsh)
            .args([
                "--connect",
                &self.config.connection_uri,
                "snapshot-create-as",
                &self.config.domain,
                &name,
                "--atomic",
                "--quiesce",
            ])
            .output()
            .context("failed to create the libvirt rollback snapshot")?;
        if !output.status.success() {
            bail!(
                "the configured VM storage cannot provide a safe Cognac rollback snapshot: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        *self.active_snapshot.borrow_mut() = Some(name.clone());
        let marker = self
            .state_dir
            .join(".snapshots")
            .join(format!("{safe}-{attempt}"));
        fs::create_dir_all(&marker)?;
        fs::write(marker.join("libvirt-name"), name)?;
        Ok(marker)
    }

    pub fn restore(&self, snapshot: &Path) -> Result<()> {
        let name = fs::read_to_string(snapshot.join("libvirt-name"))
            .context("invalid Cognac VM snapshot marker")?;
        let name = name.trim();
        let output = Command::new(&self.virsh)
            .args([
                "--connect",
                &self.config.connection_uri,
                "snapshot-revert",
                &self.config.domain,
                name,
                "--running",
            ])
            .output()
            .context("failed to revert the Windows VM snapshot")?;
        if !output.status.success() {
            bail!(
                "failed to roll back the Windows VM: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        self.delete_snapshot(name)?;
        *self.active_snapshot.borrow_mut() = None;
        self.ensure_ready()?;
        self.sync_inventory()?;
        Ok(())
    }

    pub fn install_component(&self, component: &str, _log: &Path) -> Result<()> {
        bail!(
            "automatic compatibility-layer component `{component}` is not applicable inside a native Windows VM"
        )
    }

    pub fn update(&self, log: &Path) -> Result<RunOutcome> {
        self.ensure_ready()?;
        self.sync_inventory()?;
        let output = "Windows VM and QEMU Guest Agent are responsive".to_string();
        append_log(log, &format!("[cognac-vm] {output}"))?;
        Ok(RunOutcome {
            status: Some(0),
            log_path: log.to_path_buf(),
            output,
        })
    }

    fn ensure_ready(&self) -> Result<()> {
        validate_config(&self.config)?;
        let state = Command::new(&self.virsh)
            .args([
                "--connect",
                &self.config.connection_uri,
                "domstate",
                &self.config.domain,
            ])
            .output()
            .context("could not query the Windows VM state")?;
        if !state.status.success() {
            bail!(
                "configured Windows VM `{}` is unavailable: {}",
                self.config.domain,
                String::from_utf8_lossy(&state.stderr).trim()
            );
        }
        if !String::from_utf8_lossy(&state.stdout)
            .to_ascii_lowercase()
            .contains("running")
        {
            let start = Command::new(&self.virsh)
                .args([
                    "--connect",
                    &self.config.connection_uri,
                    "start",
                    &self.config.domain,
                ])
                .output()
                .context("could not start the configured Windows VM")?;
            if !start.status.success() {
                bail!(
                    "failed to start Windows VM `{}`: {}",
                    self.config.domain,
                    String::from_utf8_lossy(&start.stderr).trim()
                );
            }
        }
        for _ in 0..60 {
            if self
                .agent_command(&json!({"execute": "guest-ping"}))
                .is_ok()
            {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(500));
        }
        bail!(
            "Windows VM `{}` is running but its QEMU Guest Agent did not become ready",
            self.config.domain
        )
    }

    fn resolve_guest_executable(&self, executable: &Path) -> Result<String> {
        if let Some(guest) = self.mirror_to_guest(executable) {
            return Ok(guest);
        }
        let text = executable.to_string_lossy();
        if looks_like_windows_path(&text) {
            return Ok(text.replace('/', "\\"));
        }
        if !executable.exists() {
            bail!("cannot stage missing executable {}", executable.display());
        }
        self.upload_file(executable)
    }

    fn mirror_to_guest(&self, path: &Path) -> Option<String> {
        let drive = self.state_dir.join("drive_c");
        let relative = path.strip_prefix(&drive).ok()?;
        let mut parts = Vec::new();
        for component in relative.components() {
            match component {
                Component::Normal(value) => parts.push(value.to_string_lossy().to_string()),
                Component::CurDir => {}
                _ => return None,
            }
        }
        (!parts.is_empty()).then(|| format!("C:\\{}", parts.join("\\")))
    }

    fn upload_file(&self, source: &Path) -> Result<String> {
        let name = source
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| {
                !value.is_empty()
                    && value
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || "-_. ".contains(character))
            })
            .unwrap_or("installer.exe");
        let staging = r"C:\ProgramData\Cognac\staging";
        let destination = format!(r"{staging}\{}-{name}", unix_millis());
        self.powershell_wait(&format!(
            "New-Item -ItemType Directory -Force -Path '{}' | Out-Null",
            staging.replace(''', "''")
        ))?;
        let handle = self
            .agent_command(&json!({
                "execute": "guest-file-open",
                "arguments": {"path": destination, "mode": "wb"}
            }))?
            .as_i64()
            .context("QEMU Guest Agent returned no file handle")?;
        let result = (|| -> Result<()> {
            let bytes = fs::read(source)
                .with_context(|| format!("cannot read {} for VM staging", source.display()))?;
            for chunk in bytes.chunks(48 * 1024) {
                let encoded = BASE64.encode(chunk);
                let response = self.agent_command(&json!({
                    "execute": "guest-file-write",
                    "arguments": {"handle": handle, "buf-b64": encoded}
                }))?;
                let written = response
                    .get("count")
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
                if written != chunk.len() as u64 {
                    bail!("QEMU Guest Agent wrote only {written} of {} bytes", chunk.len());
                }
            }
            Ok(())
        })();
        let close = self.agent_command(&json!({
            "execute": "guest-file-close",
            "arguments": {"handle": handle}
        }));
        result?;
        close.context("failed to close the staged VM file")?;
        Ok(destination)
    }

    fn sync_inventory(&self) -> Result<()> {
        let command = r#"$roots=@($env:ProgramFiles,${env:ProgramFiles(x86)}); foreach($root in $roots){ if($root -and (Test-Path -LiteralPath $root)){ Get-ChildItem -LiteralPath $root -Filter *.exe -File -Recurse -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName } } }"#;
        let inventory = self.powershell_wait(command)?.output;
        let drive = self.state_dir.join("drive_c");
        if drive.exists() {
            fs::remove_dir_all(&drive)?;
        }
        fs::create_dir_all(&drive)?;
        for line in inventory.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let Some(relative) = windows_c_relative(line) else {
                continue;
            };
            let target = drive.join(relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            if !target.exists() {
                fs::write(target, [])?;
            }
        }
        Ok(())
    }

    fn powershell_wait(&self, command: &str) -> Result<GuestOutcome> {
        self.guest_exec_wait(
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
            &[
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-Command".into(),
                command.into(),
            ],
        )
    }

    fn guest_exec_start(&self, executable: &str, args: &[String], capture: bool) -> Result<i64> {
        let response = self.agent_command(&json!({
            "execute": "guest-exec",
            "arguments": {
                "path": executable,
                "arg": args,
                "capture-output": capture
            }
        }))?;
        response
            .get("pid")
            .and_then(Value::as_i64)
            .context("QEMU Guest Agent returned no process id")
    }

    fn guest_exec_wait(&self, executable: &str, args: &[String]) -> Result<GuestOutcome> {
        let pid = self.guest_exec_start(executable, args, true)?;
        for _ in 0..14_400 {
            let status = self.agent_command(&json!({
                "execute": "guest-exec-status",
                "arguments": {"pid": pid}
            }))?;
            if status
                .get("exited")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                let stdout = decode_agent_output(status.get("out-data"))?;
                let stderr = decode_agent_output(status.get("err-data"))?;
                let output = match (stdout.is_empty(), stderr.is_empty()) {
                    (false, false) => format!("{stdout}\n{stderr}"),
                    (false, true) => stdout,
                    (true, false) => stderr,
                    (true, true) => String::new(),
                };
                let code = status
                    .get("exitcode")
                    .and_then(Value::as_i64)
                    .map(|value| value as i32);
                return Ok(GuestOutcome {
                    status: code,
                    output,
                });
            }
            thread::sleep(Duration::from_millis(250));
        }
        bail!("Windows guest process {pid} exceeded Cognac's one-hour execution limit")
    }

    fn agent_command(&self, payload: &Value) -> Result<Value> {
        let payload = serde_json::to_string(payload)?;
        let output = Command::new(&self.virsh)
            .args([
                "--connect",
                &self.config.connection_uri,
                "qemu-agent-command",
                &self.config.domain,
                &payload,
            ])
            .output()
            .context("failed to contact QEMU Guest Agent")?;
        if !output.status.success() {
            bail!(
                "QEMU Guest Agent command failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let response: Value = serde_json::from_slice(&output.stdout)
            .context("QEMU Guest Agent returned invalid JSON")?;
        if let Some(error) = response.get("error") {
            bail!("QEMU Guest Agent error: {error}");
        }
        Ok(response.get("return").cloned().unwrap_or(Value::Null))
    }

    fn discard_active_snapshot(&self) -> Result<()> {
        let Some(name) = self.active_snapshot.borrow_mut().take() else {
            return Ok(());
        };
        self.delete_snapshot(&name)
    }

    fn delete_snapshot(&self, name: &str) -> Result<()> {
        let output = Command::new(&self.virsh)
            .args([
                "--connect",
                &self.config.connection_uri,
                "snapshot-delete",
                &self.config.domain,
                name,
            ])
            .output()
            .context("failed to discard the Cognac VM snapshot")?;
        if !output.status.success() {
            bail!(
                "failed to discard VM snapshot `{name}`: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }
}

struct GuestOutcome {
    status: Option<i32>,
    output: String,
}

pub fn prepare(
    paths: &CognacPaths,
    _info: &ExecutableInfo,
    host: &HostInfo,
    progress: &Progress,
) -> Result<PreparedVm> {
    if !host.capabilities.cpu_virtualization || !host.capabilities.kvm_usable {
        bail!("the host does not currently provide usable KVM virtualization");
    }
    if !host.capabilities.qemu || !host.capabilities.libvirt {
        bail!("QEMU and libvirt are required for VM execution");
    }
    let report = probe(paths)?;
    if !report.configured {
        bail!(
            "the Windows VM is not ready: {}",
            report.blockers.join("; ")
        );
    }
    let config = load_config(paths)?;
    let environment = VmEnvironment::provision(paths, paths.state.join("vm-probe"), progress)?;
    environment.ensure_ready()?;
    Ok(PreparedVm {
        backend: format!("libvirt-qemu:{}", config.domain),
    })
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
    let config = load_config(paths)?;
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

fn load_config(paths: &CognacPaths) -> Result<VmConfig> {
    let config_path = paths.config.join("windows-vm.json");
    let config: VmConfig = serde_json::from_slice(
        &fs::read(&config_path)
            .with_context(|| format!("cannot read {}", config_path.display()))?,
    )
    .with_context(|| format!("invalid VM configuration in {}", config_path.display()))?;
    validate_config(&config)?;
    Ok(config)
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

fn decode_agent_output(value: Option<&Value>) -> Result<String> {
    let Some(encoded) = value.and_then(Value::as_str) else {
        return Ok(String::new());
    };
    let bytes = BASE64
        .decode(encoded)
        .context("QEMU Guest Agent returned invalid base64 output")?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn windows_c_relative(value: &str) -> Option<PathBuf> {
    let normalized = value.trim().trim_matches('"').replace('/', "\\");
    let relative = normalized
        .strip_prefix("C:\\")
        .or_else(|| normalized.strip_prefix("c:\\"))?;
    let mut path = PathBuf::new();
    for part in relative.split('\\') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        path.push(part);
    }
    (!path.as_os_str().is_empty()).then_some(path)
}

fn looks_like_windows_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}

fn append_log(path: &Path, message: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{message}")?;
    Ok(())
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
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

    #[test]
    fn maps_windows_inventory_paths_without_traversal() {
        assert_eq!(
            windows_c_relative(r"C:\Program Files\Foo\Foo.exe"),
            Some(PathBuf::from("Program Files/Foo/Foo.exe"))
        );
        assert!(windows_c_relative(r"C:\Program Files\..\evil.exe").is_none());
    }
}
