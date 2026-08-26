use crate::{
    environment::{restore_prefix, run_logged, snapshot_prefix},
    model::{CompatibilityPlan, RunOutcome},
    paths::CognacPaths,
    progress::Progress,
};
use anyhow::{Context, Result, bail};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

pub struct UmuEnvironment<'a> {
    pub prefix: PathBuf,
    pub launcher: PathBuf,
    pub game_id: String,
    pub proton_path: Option<String>,
    paths: &'a CognacPaths,
}

impl<'a> UmuEnvironment<'a> {
    pub fn new(
        paths: &'a CognacPaths,
        prefix: PathBuf,
        launcher: PathBuf,
        game_id: String,
        proton_path: Option<String>,
    ) -> Self {
        Self {
            prefix,
            launcher,
            game_id,
            proton_path,
            paths,
        }
    }

    pub fn initialize(
        &self,
        plan: &CompatibilityPlan,
        progress: &Progress,
        log: &Path,
    ) -> Result<()> {
        fs::create_dir_all(&self.prefix)?;
        if !self.prefix.is_absolute() {
            bail!("UMU requires an absolute Cognac environment path");
        }
        if plan.components.is_empty() {
            return Ok(());
        }
        progress.update("Preparing Proton's traveling case...", Some(42));
        let mut command = self.command();
        command.arg("winetricks").args(&plan.components);
        let outcome = run_logged(&mut command, log)?;
        let already_installed = outcome.status == Some(1)
            && outcome
                .output
                .to_ascii_lowercase()
                .contains("already installed");
        if outcome.status != Some(0) && !already_installed {
            bail!(
                "UMU could not install required components: {}",
                plan.components.join(", ")
            );
        }
        Ok(())
    }

    pub fn install_component(&self, component: &str, log: &Path) -> Result<()> {
        let mut command = self.command();
        command.arg("winetricks").arg(component);
        let outcome = run_logged(&mut command, log)?;
        let already_installed = outcome.status == Some(1)
            && outcome
                .output
                .to_ascii_lowercase()
                .contains("already installed");
        if outcome.status != Some(0) && !already_installed {
            bail!("UMU could not install Windows component {component}");
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
        let executable = absolute(executable)?;
        let mut command = self.command();
        command.arg(&executable).args(args).envs(extra);
        run_logged(&mut command, log)
            .with_context(|| format!("could not start UMU for {}", executable.display()))
    }

    pub fn launch(
        &self,
        executable: &Path,
        args: &[String],
        extra: &BTreeMap<String, String>,
        log: &Path,
    ) -> Result<()> {
        let executable = absolute(executable)?;
        let file = OpenOptions::new().create(true).append(true).open(log)?;
        let error_file = file.try_clone()?;
        let mut command = self.command();
        command
            .arg(&executable)
            .args(args)
            .envs(extra)
            .stdin(Stdio::null())
            .stdout(Stdio::from(file))
            .stderr(Stdio::from(error_file));
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().with_context(|| {
            format!(
                "could not start {} for {}",
                self.launcher.display(),
                executable.display()
            )
        })?;
        std::thread::sleep(Duration::from_millis(300));
        if let Some(status) = child.try_wait()?
            && !status.success()
        {
            bail!("UMU exited during startup with {status}");
        }
        Ok(())
    }

    pub fn update_prefix(&self, log: &Path) -> Result<RunOutcome> {
        let mut command = self.command();
        command.args(["wineboot", "--update"]);
        run_logged(&mut command, log)
    }

    pub fn snapshot(&self, app_id: &str, attempt: usize) -> Result<PathBuf> {
        snapshot_prefix(self.paths, &self.prefix, app_id, attempt)
    }

    pub fn restore(&self, snapshot: &Path) -> Result<()> {
        restore_prefix(&self.prefix, snapshot)
    }

    pub fn base_environment(&self) -> BTreeMap<String, String> {
        let mut environment = BTreeMap::from([
            ("WINEPREFIX".into(), self.prefix.display().to_string()),
            ("GAMEID".into(), self.game_id.clone()),
            ("STORE".into(), "none".into()),
        ]);
        if let Some(proton_path) = &self.proton_path {
            environment.insert("PROTONPATH".into(), proton_path.clone());
        }
        environment
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.launcher);
        for variable in [
            "WINEPREFIX",
            "WINEARCH",
            "WINEDLLOVERRIDES",
            "PROTONPATH",
            "PROTON_USE_WINED3D",
            "PROTON_NO_D3D11",
            "PROTON_NO_D3D10",
            "PROTON_NO_FSYNC",
            "PROTON_NO_NTSYNC",
            "PROTONFIXES_DISABLE",
            "STEAM_COMPAT_DATA_PATH",
        ] {
            command.env_remove(variable);
        }
        command.envs(self.base_environment());
        command
    }
}

fn absolute(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn default_umu_contract_removes_protonpath() {
        let directory = tempfile::tempdir().unwrap();
        let paths = CognacPaths {
            data: directory.path().join("data"),
            cache: directory.path().join("cache"),
            config: directory.path().join("config"),
            state: directory.path().join("state"),
        };
        let environment = UmuEnvironment::new(
            &paths,
            directory.path().join("prefix"),
            PathBuf::from("umu-run"),
            "umu-default".into(),
            None,
        );
        let command = environment.command();
        let values = command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(values.get("PROTONPATH"), Some(&None));
        assert_eq!(
            values.get("WINEPREFIX").and_then(Clone::clone),
            Some(directory.path().join("prefix").display().to_string())
        );
        assert_eq!(
            values.get("GAMEID").and_then(Clone::clone),
            Some("umu-default".into())
        );
    }

    #[test]
    fn executable_arguments_remain_separate() {
        let directory = tempfile::tempdir().unwrap();
        let launcher = directory.path().join("umu-run");
        fs::write(
            &launcher,
            "#!/bin/sh\nprintf 'prefix=%s\\n' \"$WINEPREFIX\"\nprintf 'game=%s\\n' \"$GAMEID\"\nfor arg in \"$@\"; do printf 'arg=<%s>\\n' \"$arg\"; done\n",
        )
        .unwrap();
        fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).unwrap();
        let paths = CognacPaths {
            data: directory.path().join("data"),
            cache: directory.path().join("cache"),
            config: directory.path().join("config"),
            state: directory.path().join("state"),
        };
        paths.ensure().unwrap();
        let environment = UmuEnvironment::new(
            &paths,
            directory.path().join("prefix with spaces"),
            launcher,
            "umu-default".into(),
            None,
        );
        fs::create_dir_all(&environment.prefix).unwrap();
        let executable = directory.path().join("game with spaces.exe");
        let log = paths.logs().join("umu.log");
        let outcome = environment
            .run(
                &executable,
                &["argument with spaces".into(), "--flag".into()],
                &BTreeMap::new(),
                &log,
            )
            .unwrap();
        assert_eq!(outcome.status, Some(0));
        assert!(outcome.output.contains("game=umu-default"));
        assert!(
            outcome
                .output
                .contains(&format!("arg=<{}>", executable.display()))
        );
        assert!(outcome.output.contains("arg=<argument with spaces>"));
        assert!(outcome.output.contains("arg=<--flag>"));
    }
}
