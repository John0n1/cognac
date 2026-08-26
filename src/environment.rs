use crate::{
    model::{Architecture, CompatibilityPlan, RunOutcome},
    paths::CognacPaths,
    progress::Progress,
    runner::RunnerInstallation,
};
use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

pub struct WineEnvironment<'a> {
    pub prefix: PathBuf,
    pub runner: RunnerInstallation,
    paths: &'a CognacPaths,
}

impl<'a> WineEnvironment<'a> {
    pub fn new(paths: &'a CognacPaths, prefix: PathBuf, runner: RunnerInstallation) -> Self {
        Self {
            paths,
            prefix,
            runner,
        }
    }

    pub fn initialize(
        &self,
        plan: &CompatibilityPlan,
        progress: &Progress,
        log: &Path,
    ) -> Result<()> {
        fs::create_dir_all(&self.prefix)?;
        progress.update("Convincing Windows it's totally at home...", Some(38));
        let mut initialization = BTreeMap::new();
        initialization.insert(
            "WINEARCH".into(),
            match plan.prefix_architecture {
                Architecture::X86 => "win32",
                _ => "win64",
            }
            .into(),
        );
        initialization.insert("WINEDLLOVERRIDES".into(), "mscoree,mshtml=".into());
        let outcome = self.tool("wineboot", ["--init"], &initialization, log)?;
        if outcome.status != Some(0) {
            bail!("Wine could not initialize the isolated environment");
        }
        let version = format!("-v{}", plan.windows_version);
        let _ = self.tool("winecfg", [version.as_str()], &BTreeMap::new(), log);
        if !plan.components.is_empty() {
            let winetricks = ensure_winetricks(self.paths)?;
            progress.update("Collecting tiny pieces of Windows...", Some(46));
            let mut command = Command::new("sh");
            command
                .arg(winetricks)
                .arg("-q")
                .args(&plan.components)
                .envs(self.base_environment());
            let code = append_command_output(&mut command, log)?;
            if code != Some(0) {
                bail!(
                    "could not install required Windows components: {}",
                    plan.components.join(", ")
                );
            }
        }
        Ok(())
    }

    pub fn install_component(&self, component: &str, log: &Path) -> Result<()> {
        let winetricks = ensure_winetricks(self.paths)?;
        let mut command = Command::new("sh");
        command
            .arg(winetricks)
            .arg("-q")
            .arg(component)
            .envs(self.base_environment());
        let code = append_command_output(&mut command, log)?;
        if code != Some(0) {
            bail!("could not install Windows component {component}");
        }
        Ok(())
    }

    pub fn run(
        &self,
        executable: &Path,
        args: &[String],
        extra: &BTreeMap<String, String>,
        log: &Path,
    ) -> Result<RunOutcome> {
        let mut command = Command::new(&self.runner.wine);
        command
            .arg(executable)
            .args(args)
            .envs(self.base_environment())
            .envs(extra);
        run_logged(&mut command, log)
            .with_context(|| format!("could not start {}", self.runner.wine.display()))
    }

    pub fn launch(
        &self,
        executable: &Path,
        args: &[String],
        extra: &BTreeMap<String, String>,
        log: &Path,
    ) -> Result<()> {
        let file = OpenOptions::new().create(true).append(true).open(log)?;
        let error_file = file.try_clone()?;
        let mut command = Command::new(&self.runner.wine);
        command
            .arg(executable)
            .args(args)
            .envs(self.base_environment())
            .envs(extra)
            .stdin(Stdio::null())
            .stdout(Stdio::from(file))
            .stderr(Stdio::from(error_file));
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .spawn()
            .with_context(|| format!("could not start {}", self.runner.wine.display()))?;
        std::thread::sleep(Duration::from_millis(250));
        if let Some(status) = child.try_wait()?
            && !status.success()
        {
            bail!(
                "{} exited during startup with {status}",
                executable.display()
            );
        }
        Ok(())
    }

    pub fn snapshot(&self, app_id: &str, attempt: usize) -> Result<PathBuf> {
        snapshot_prefix(self.paths, &self.prefix, app_id, attempt)
    }

    pub fn restore(&self, snapshot: &Path) -> Result<()> {
        restore_prefix(&self.prefix, snapshot)
    }

    pub fn tool<const N: usize>(
        &self,
        tool: &str,
        args: [&str; N],
        extra: &BTreeMap<String, String>,
        log: &Path,
    ) -> Result<RunOutcome> {
        let candidate = self.runner.root.join("bin").join(tool);
        let executable = if candidate.is_file() {
            candidate
        } else {
            PathBuf::from(tool)
        };
        let mut command = Command::new(executable);
        command.args(args).envs(self.base_environment()).envs(extra);
        run_logged(&mut command, log)
    }

    pub fn base_environment(&self) -> BTreeMap<String, String> {
        let mut env = BTreeMap::new();
        env.insert("WINEPREFIX".into(), self.prefix.display().to_string());
        env.insert("WINEDEBUG".into(), "-all".into());
        env
    }
}

fn ensure_winetricks(paths: &CognacPaths) -> Result<PathBuf> {
    const WINETRICKS_URL: &str =
        "https://raw.githubusercontent.com/Winetricks/winetricks/20260125/src/winetricks";
    const WINETRICKS_SHA256: &str =
        "431f82fc74000e6c864409f1d8fb495d696c03928808e3e8acffc45179312a7b";
    let path = paths.data.join("tools/winetricks");
    if path.is_file() {
        return Ok(path);
    }
    fs::create_dir_all(path.parent().context("tool path has no parent")?)?;
    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(format!("cognac/{}", crate::VERSION))
        .build()?;
    let bytes = client
        .get(WINETRICKS_URL)
        .send()?
        .error_for_status()?
        .bytes()?;
    if !bytes.starts_with(b"#!/bin/sh") {
        bail!("downloaded winetricks script was invalid");
    }
    if hex::encode(Sha256::digest(&bytes)) != WINETRICKS_SHA256 {
        bail!("winetricks checksum mismatch; the script was not installed");
    }
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temp.write_all(&bytes)?;
    temp.persist(&path).map_err(|e| e.error)?;
    Ok(path)
}

fn append_command_output(command: &mut Command, log: &Path) -> Result<Option<i32>> {
    let file = OpenOptions::new().create(true).append(true).open(log)?;
    let error_file = file.try_clone()?;
    let status = command
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(error_file))
        .status()?;
    Ok(status.code())
}

/// Run without pipes. Installers often start the installed application before
/// exiting; descendants inherit pipes and would keep `Command::output` waiting
/// forever for EOF even though the installer itself has finished.
pub(crate) fn run_logged(command: &mut Command, log: &Path) -> Result<RunOutcome> {
    let start = fs::metadata(log)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let file = OpenOptions::new().create(true).append(true).open(log)?;
    let error_file = file.try_clone()?;
    let status = command
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(error_file))
        .status()?;

    let mut reader = OpenOptions::new().read(true).open(log)?;
    reader.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    reader.take(2 * 1024 * 1024).read_to_end(&mut bytes)?;
    Ok(RunOutcome {
        status: status.code(),
        log_path: log.to_path_buf(),
        output: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

pub(crate) fn snapshot_prefix(
    paths: &CognacPaths,
    prefix: &Path,
    app_id: &str,
    attempt: usize,
) -> Result<PathBuf> {
    let destination = paths.snapshots().join(format!(
        "{app_id}-{}-attempt-{attempt}",
        chrono::Utc::now().timestamp_millis()
    ));
    if destination.exists() {
        bail!("snapshot {} already exists", destination.display());
    }
    let status = Command::new("cp")
        .args(["--archive", "--reflink=auto"])
        .arg(prefix)
        .arg(&destination)
        .status()?;
    if !status.success() {
        bail!("could not snapshot environment before retry");
    }
    Ok(destination)
}

pub(crate) fn restore_prefix(prefix: &Path, snapshot: &Path) -> Result<()> {
    let failed = prefix.with_extension(format!("failed-{}", chrono::Utc::now().timestamp()));
    fs::rename(prefix, &failed)?;
    if let Err(error) = fs::rename(snapshot, prefix) {
        let _ = fs::rename(&failed, prefix);
        return Err(error).context("could not restore compatibility snapshot");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn descendants_do_not_hold_execution_open() {
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join("run.log");
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "(sleep 2; echo descendant-finished) & echo installer-finished",
        ]);
        let started = Instant::now();
        let outcome = run_logged(&mut command, &log).unwrap();
        assert_eq!(outcome.status, Some(0));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(outcome.output.contains("installer-finished"));
    }
}
