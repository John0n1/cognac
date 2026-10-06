//! Native firmware updates through the system fwupd daemon, never a Wine runner.
use crate::{paths::CognacPaths, util::atomic_json};
use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, process::Command};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    #[serde(rename = "DeviceId")]
    pub id: String,
    #[serde(rename = "Name", default)]
    pub name: String,
    #[serde(rename = "Version", default)]
    pub version: Option<String>,
    #[serde(rename = "Guid", default)]
    pub guids: Vec<String>,
    #[serde(rename = "Flags", default)]
    pub flags: Vec<String>,
    #[serde(rename = "UpdateState", default)]
    pub update_state: Option<u32>,
    #[serde(rename = "UpdateError", default)]
    pub update_error: Option<String>,
    #[serde(rename = "Releases", default)]
    pub releases: Vec<Release>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    #[serde(rename = "Version")]
    pub version: String,
    #[serde(rename = "RemoteId", default)]
    pub remote: Option<String>,
    #[serde(rename = "Checksum", default)]
    pub checksums: Vec<String>,
    #[serde(rename = "Flags", default)]
    pub flags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FlashPlan {
    pub device: Device,
    pub release: Release,
    pub command: Vec<String>,
    pub reboot_automatically: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Transaction {
    pub plan: FlashPlan,
    pub started_at: String,
    pub host_id: String,
    pub boot_id: String,
    pub phase: Phase,
    pub exit_code: Option<i32>,
    pub output: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Submitting,
    SubmissionUnknown,
    Submitted,
    PendingReboot,
    Unverified,
    Verified,
}

#[derive(Debug, Serialize)]
pub struct Verification {
    pub phase: Phase,
    pub expected_version: String,
    pub observed_version: Option<String>,
    pub firmware_update_state: Option<u32>,
    pub firmware_update_error: Option<String>,
    pub reboot_observed: bool,
    pub explanation: String,
}

pub struct Response {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

// Injectable command boundary lets tests exercise real transaction behavior
// without giving the test process access to physical firmware writes.
trait Backend {
    fn invoke(&self, arguments: &[String]) -> Result<Response>;
}

struct Fwupd {
    executable: PathBuf,
}

impl Fwupd {
    fn discover() -> Result<Self> {
        let executable = ["/usr/bin/fwupdmgr", "/usr/local/bin/fwupdmgr"]
            .iter()
            .map(PathBuf::from)
            .find(|path| path.is_file())
            .context(
                "fwupdmgr is required; install fwupd using the distribution package manager",
            )?;
        Ok(Self { executable })
    }
}

impl Backend for Fwupd {
    fn invoke(&self, arguments: &[String]) -> Result<Response> {
        let output = Command::new(&self.executable)
            .args(arguments)
            .output()
            .context("could not invoke the system fwupdmgr client")?;
        Ok(Response {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[derive(Deserialize)]
struct Inventory {
    #[serde(rename = "Devices")]
    devices: Vec<Device>,
}

fn query_devices(backend: &impl Backend, verb: &str, id: Option<&str>) -> Result<Vec<Device>> {
    let mut args = vec![verb.into(), "--json".into()];
    if let Some(id) = id {
        args.push(id.into());
    }
    let response = backend.invoke(&args)?;
    // fwupdmgr uses status 2 for a valid query with no actions available.
    if !matches!(response.code, Some(0 | 2)) {
        bail!(
            "fwupd {verb} failed: {} {}",
            response.stdout.trim(),
            response.stderr.trim()
        );
    }
    let inventory: Inventory = serde_json::from_str(&response.stdout).with_context(|| {
        format!(
            "fwupd {verb} did not return a Devices JSON inventory: {}",
            response.stdout.trim()
        )
    })?;
    Ok(inventory.devices)
}

fn validate_id(id: &str) -> Result<()> {
    if id.len() != 40 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!(
            "select an exact 40-character fwupd DeviceId from `cognac firmware-devices`; EXE and raw CAP files are not supported flash inputs"
        );
    }
    Ok(())
}

fn selected(mut devices: Vec<Device>, id: &str) -> Result<Device> {
    devices.retain(|device| device.id == id);
    if devices.len() != 1 {
        bail!("fwupd did not uniquely identify device {id}");
    }
    Ok(devices.remove(0))
}

fn has(flags: &[String], flag: &str) -> bool {
    flags.iter().any(|value| value == flag)
}

fn install_arguments(id: &str, version: &str) -> Vec<String> {
    // No force, downgrade, reinstall, requirement bypass, raw-blob write, or reboot.
    [
        "install",
        id,
        version,
        "--json",
        "--assume-yes",
        "--no-reboot-check",
        "--no-unreported-check",
        "--no-security-fix",
        "--no-remote-check",
        "--filter-release",
        "trusted-metadata",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn plan_with(backend: &impl Backend, id: &str, version: &str) -> Result<FlashPlan> {
    validate_id(id)?;
    if version.is_empty()
        || version.starts_with('-')
        || version.len() > 128
        || version.chars().any(char::is_control)
    {
        bail!("provide a literal firmware version from the device's available releases");
    }
    let device = selected(query_devices(backend, "get-devices", None)?, id)?;
    if !has(&device.flags, "updatable") || device.guids.is_empty() || device.version.is_none() {
        bail!("device does not expose an identified, versioned fwupd update interface");
    }
    if device.version.as_deref() == Some(version) {
        bail!("requested version is already installed; reinstallation is not supported");
    }
    let updates = query_devices(backend, "get-updates", Some(id))?;
    let available = updates.into_iter().find(|value| value.id == id).context(
        "fwupd offers no upgrade for this device; a raw vendor EXE/CAP cannot be substituted",
    )?;
    if available.guids != device.guids || available.version != device.version {
        bail!("device identity or version changed during planning; query it again");
    }
    let mut releases = available
        .releases
        .into_iter()
        .filter(|release| release.version == version)
        .collect::<Vec<_>>();
    if releases.len() != 1 {
        bail!("requested version is not a unique fwupd upgrade for the selected device");
    }
    let release = releases.remove(0);
    if !has(&release.flags, "trusted-metadata")
        || release.remote.as_deref().is_none_or(str::is_empty)
        || !release
            .checksums
            .iter()
            .any(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        || release
            .flags
            .iter()
            .any(|flag| flag.starts_with("blocked-"))
    {
        bail!(
            "upgrade lacks trusted remote metadata and a SHA-256 checksum, or is blocked by fwupd"
        );
    }
    Ok(FlashPlan {
        device,
        release,
        command: install_arguments(id, version),
        reboot_automatically: false,
    })
}

pub fn devices() -> Result<Vec<Device>> {
    query_devices(&Fwupd::discover()?, "get-devices", None)
}

pub fn plan(id: &str, version: &str) -> Result<FlashPlan> {
    plan_with(&Fwupd::discover()?, id, version)
}

fn state_directory(paths: &CognacPaths) -> PathBuf {
    paths.state.join("firmware")
}

fn journal_path(paths: &CognacPaths, id: &str) -> PathBuf {
    state_directory(paths).join(format!("{id}.json"))
}

fn identity(path: &str) -> Result<String> {
    let value = fs::read_to_string(path).with_context(|| format!("cannot read {path}"))?;
    if value.trim().is_empty() {
        bail!("empty identity at {path}");
    }
    Ok(value.trim().into())
}

fn verification(
    transaction: &Transaction,
    current: &Device,
    host_id: &str,
    boot_id: &str,
) -> Result<Verification> {
    if transaction.host_id != host_id
        || transaction.plan.device.id != current.id
        || transaction.plan.device.guids != current.guids
    {
        bail!("transaction host or device identity does not match this machine");
    }
    let reboot_observed = transaction.boot_id != boot_id;
    let same_version = current.version.as_deref() == Some(&transaction.plan.release.version);
    let state_failed = matches!(current.update_state, Some(3 | 5));
    let deferred = has(&transaction.plan.device.flags, "needs-reboot")
        || has(&transaction.plan.device.flags, "needs-shutdown");
    let (phase, explanation) = if same_version && !state_failed && (!deferred || reboot_observed) {
        (
            Phase::Verified,
            "Selected device reports the requested firmware version; deferred updates also require a new boot. This verifies version, not every firmware function.",
        )
    } else if deferred && !reboot_observed && transaction.phase != Phase::SubmissionUnknown {
        (
            Phase::PendingReboot,
            "Firmware submission is not proof of flashing. Save work, follow fwupd's reboot or shutdown instructions, then run firmware-status.",
        )
    } else {
        (
            Phase::Unverified,
            "Requested firmware version is not verified. Inspect fwupd history/results and any required physical power cycle before considering another submission.",
        )
    };
    Ok(Verification {
        phase,
        expected_version: transaction.plan.release.version.clone(),
        observed_version: current.version.clone(),
        firmware_update_state: current.update_state,
        firmware_update_error: current.update_error.clone(),
        reboot_observed,
        explanation: explanation.into(),
    })
}

fn submit_with(
    backend: &impl Backend,
    paths: &CognacPaths,
    id: &str,
    version: &str,
    host_id: &str,
    boot_id: &str,
) -> Result<Transaction> {
    validate_id(id)?;
    fs::create_dir_all(state_directory(paths))?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(state_directory(paths).join("update.lock"))?;
    lock.try_lock()
        .context("another Cognac firmware operation is in progress")?;
    let journal = journal_path(paths, id);
    if journal.exists() {
        let previous: Transaction = serde_json::from_slice(&fs::read(&journal)?)?;
        let current = selected(query_devices(backend, "get-devices", None)?, id)?;
        if verification(&previous, &current, host_id, boot_id)?.phase != Phase::Verified {
            bail!(
                "a previous firmware transaction is unresolved; inspect `cognac firmware-status --device {id}` and fwupd results instead of automatically resubmitting"
            );
        }
        // Preserve the previous successful transaction before recording a new one.
        let archived = tempfile::Builder::new()
            .prefix(&format!("{id}-history-"))
            .suffix(".json")
            .tempfile_in(state_directory(paths))?;
        atomic_json(archived.path(), &previous)?;
        archived.keep()?;
    }
    let plan = plan_with(backend, id, version)?;
    let mut transaction = Transaction {
        plan,
        started_at: Utc::now().to_rfc3339(),
        host_id: host_id.into(),
        boot_id: boot_id.into(),
        phase: Phase::Submitting,
        exit_code: None,
        output: String::new(),
    };
    // Commit intent before issuing a potentially irreversible update request.
    atomic_json(&journal, &transaction)?;
    match backend.invoke(&transaction.plan.command) {
        Ok(response) => {
            transaction.exit_code = response.code;
            transaction.output = format!("{}{}", response.stdout, response.stderr);
            transaction.phase = if response.code == Some(0) {
                Phase::Submitted
            } else {
                Phase::SubmissionUnknown
            };
        }
        Err(error) => {
            transaction.phase = Phase::SubmissionUnknown;
            transaction.output = format!("fwupd invocation error: {error:#}");
        }
    }
    atomic_json(&journal, &transaction)?;
    if transaction.phase == Phase::SubmissionUnknown {
        bail!(
            "firmware submission failed or has an unknown result; transaction retained at {}: {}",
            journal.display(),
            transaction.output.trim()
        );
    }
    // Query failure must retain Submitted, not invent a successful flash.
    if let Ok(devices) = query_devices(backend, "get-devices", None)
        && let Ok(current) = selected(devices, id)
    {
        transaction.phase = verification(&transaction, &current, host_id, boot_id)?.phase;
        atomic_json(&journal, &transaction)?;
    }
    Ok(transaction)
}

pub fn flash(paths: &CognacPaths, id: &str, version: &str) -> Result<Transaction> {
    submit_with(
        &Fwupd::discover()?,
        paths,
        id,
        version,
        &identity("/etc/machine-id")?,
        &identity("/proc/sys/kernel/random/boot_id")?,
    )
}

pub fn status(paths: &CognacPaths, id: &str) -> Result<Verification> {
    validate_id(id)?;
    let transaction: Transaction = serde_json::from_slice(
        &fs::read(journal_path(paths, id))
            .context("no readable Cognac firmware transaction exists for this device")?,
    )?;
    let current = selected(devices()?, id)?;
    verification(
        &transaction,
        &current,
        &identity("/etc/machine-id")?,
        &identity("/proc/sys/kernel/random/boot_id")?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::VecDeque};
    const ID: &str = "0123456789abcdef0123456789abcdef01234567";

    struct Mock {
        replies: RefCell<VecDeque<Response>>,
        calls: RefCell<Vec<Vec<String>>>,
        journal: Option<PathBuf>,
    }
    impl Backend for Mock {
        fn invoke(&self, args: &[String]) -> Result<Response> {
            self.calls.borrow_mut().push(args.to_vec());
            if args[0] == "install"
                && let Some(path) = &self.journal
            {
                let txn: Transaction = serde_json::from_slice(&fs::read(path)?)?;
                assert_eq!(txn.phase, Phase::Submitting);
                assert_eq!(txn.plan.device.id, ID);
            }
            self.replies
                .borrow_mut()
                .pop_front()
                .context("unexpected backend call")
        }
    }
    fn response(value: serde_json::Value) -> Response {
        Response {
            code: Some(0),
            stdout: value.to_string(),
            stderr: String::new(),
        }
    }
    fn device(version: &str) -> Device {
        serde_json::from_value(serde_json::json!({
            "DeviceId": ID, "Name": "Test System Firmware", "Version": version,
            "Guid": ["test-guid"], "Flags": ["updatable", "needs-reboot"],
            "Releases": [{"Version": "2", "RemoteId": "lvfs", "Checksum": ["a".repeat(64)], "Flags": ["trusted-metadata"]}]
        })).unwrap()
    }
    fn inventory(version: &str) -> Response {
        response(serde_json::json!({"Devices": [device(version)]}))
    }
    fn mock(replies: Vec<Response>) -> Mock {
        Mock {
            replies: RefCell::new(replies.into()),
            calls: RefCell::new(Vec::new()),
            journal: None,
        }
    }
    fn paths(directory: &std::path::Path) -> CognacPaths {
        CognacPaths {
            data: directory.join("data"),
            cache: directory.join("cache"),
            config: directory.join("config"),
            state: directory.join("state"),
        }
    }

    #[test]
    fn raw_files_options_and_no_available_release_do_not_submit() {
        let backend = mock(vec![]);
        assert!(plan_with(&backend, "FXCN49WW.CAP", "49").is_err());
        assert!(plan_with(&backend, ID, "--force").is_err());
        assert!(backend.calls.borrow().is_empty());
        let backend = mock(vec![
            inventory("1"),
            response(serde_json::json!({"Devices": []})),
        ]);
        assert!(
            plan_with(&backend, ID, "2")
                .unwrap_err()
                .to_string()
                .contains("no upgrade")
        );
        assert!(
            !backend
                .calls
                .borrow()
                .iter()
                .any(|call| call[0] == "install")
        );
    }

    #[test]
    fn unauthenticated_blocked_or_ambiguous_metadata_is_rejected() {
        for mode in ["untrusted", "blocked", "duplicate", "checksum"] {
            let mut dev = device("1");
            match mode {
                "untrusted" => dev.releases[0].flags.clear(),
                "blocked" => dev.releases[0].flags.push("blocked-approval".into()),
                "duplicate" => dev.releases.push(dev.releases[0].clone()),
                _ => dev.releases[0].checksums = vec!["not-a-checksum".into()],
            }
            let backend = mock(vec![
                inventory("1"),
                response(serde_json::json!({"Devices": [dev]})),
            ]);
            assert!(plan_with(&backend, ID, "2").is_err(), "mode {mode}");
        }
    }

    #[test]
    fn journals_before_submission_and_verifies_only_after_reboot() {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(directory.path());
        let mut backend = mock(vec![
            inventory("1"),
            inventory("1"),
            response(serde_json::json!({})),
            inventory("2"),
        ]);
        backend.journal = Some(journal_path(&paths, ID));
        let txn = submit_with(&backend, &paths, ID, "2", "host", "boot-one").unwrap();
        assert_eq!(txn.phase, Phase::PendingReboot);
        let calls = backend.calls.borrow();
        let command = calls.iter().find(|call| call[0] == "install").unwrap();
        assert_eq!(&command[..3], &["install", ID, "2"]);
        assert!(command.contains(&"--no-reboot-check".into()));
        assert!(!command.iter().any(|arg| {
            [
                "--force",
                "--allow-older",
                "--allow-reinstall",
                "--no-safety-check",
            ]
            .contains(&arg.as_str())
        }));
        let result = verification(&txn, &device("2"), "host", "boot-two").unwrap();
        assert_eq!(result.phase, Phase::Verified);
        assert!(result.reboot_observed);
        assert!(verification(&txn, &device("2"), "other-host", "boot-two").is_err());
    }

    #[test]
    fn command_failure_and_unresolved_transaction_never_retry() {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(directory.path());
        let mut backend = mock(vec![
            inventory("1"),
            inventory("1"),
            Response {
                code: Some(1),
                stdout: "write failed".into(),
                stderr: String::new(),
            },
        ]);
        backend.journal = Some(journal_path(&paths, ID));
        assert!(submit_with(&backend, &paths, ID, "2", "host", "boot-one").is_err());
        let txn: Transaction =
            serde_json::from_slice(&fs::read(journal_path(&paths, ID)).unwrap()).unwrap();
        assert_eq!(txn.phase, Phase::SubmissionUnknown);
        let backend = mock(vec![inventory("1")]);
        assert!(submit_with(&backend, &paths, ID, "2", "host", "boot-two").is_err());
        assert!(
            !backend
                .calls
                .borrow()
                .iter()
                .any(|call| call[0] == "install")
        );
    }

    #[test]
    fn changed_boot_or_zero_exit_does_not_verify_unchanged_firmware() {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(directory.path());
        let backend = mock(vec![
            inventory("1"),
            inventory("1"),
            response(serde_json::json!({})),
            inventory("1"),
        ]);
        let txn = submit_with(&backend, &paths, ID, "2", "host", "boot-one").unwrap();
        assert_eq!(
            verification(&txn, &device("1"), "host", "boot-two")
                .unwrap()
                .phase,
            Phase::Unverified
        );
        let mut changed = device("2");
        changed.update_state = Some(3);
        assert_eq!(
            verification(&txn, &changed, "host", "boot-two")
                .unwrap()
                .phase,
            Phase::Unverified
        );
    }
}
