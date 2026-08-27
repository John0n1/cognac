use crate::{
    environment::WineEnvironment,
    model::{
        CompatibilityPlan, ExecutionClass, ExecutionStrategy, RunOutcome, StrategyAvailability,
    },
    paths::CognacPaths,
    progress::Progress,
    runner::{RunnerInstallation, RunnerManager},
    umu::UmuEnvironment,
    umu_manager::UmuManager,
    vm::VmEnvironment,
};
use anyhow::{Result, bail};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub enum ExecutionEnvironment<'a> {
    Wine(WineEnvironment<'a>),
    Umu(UmuEnvironment<'a>),
    Vm(VmEnvironment<'a>),
}

impl<'a> ExecutionEnvironment<'a> {
    pub fn provision(
        paths: &'a CognacPaths,
        strategy: &ExecutionStrategy,
        prefix: PathBuf,
        progress: &Progress,
    ) -> Result<Self> {
        if strategy.availability == StrategyAvailability::Blocked {
            bail!(
                "{} is blocked: {}",
                strategy.class,
                strategy.blockers.join("; ")
            );
        }
        match strategy.class {
            ExecutionClass::Wine => {
                let channel = strategy.backend.strip_prefix("wine-").unwrap_or("staging");
                let runner = RunnerManager::new(paths)?.ensure(channel, progress)?;
                Ok(Self::Wine(WineEnvironment::new(paths, prefix, runner)))
            }
            ExecutionClass::ProtonUmu => {
                let installation = UmuManager::new(paths)?.ensure(progress)?;
                Ok(Self::Umu(UmuEnvironment::new(
                    paths,
                    prefix,
                    installation.launcher,
                    "umu-default".into(),
                    (strategy.backend == "umu-ge-proton").then(|| "GE-Proton".into()),
                )))
            }
            ExecutionClass::ContainerizedWine => bail!(
                "containerized Wine passed planning before its backend was enabled; refusing an unsafe partial sandbox"
            ),
            ExecutionClass::VirtualMachine => {
                Ok(Self::Vm(VmEnvironment::provision(paths, prefix, progress)?))
            }
            ExecutionClass::Restricted => bail!(
                "Cognac restricted this application: {}",
                strategy.reasons.join("; ")
            ),
        }
    }

    pub fn from_installed(
        paths: &'a CognacPaths,
        execution_class: ExecutionClass,
        prefix: PathBuf,
        launcher: PathBuf,
        launch_environment: &BTreeMap<String, String>,
    ) -> Result<Self> {
        match execution_class {
            ExecutionClass::Wine => {
                let root = launcher
                    .parent()
                    .and_then(Path::parent)
                    .unwrap_or(Path::new("/"))
                    .to_path_buf();
                Ok(Self::Wine(WineEnvironment::new(
                    paths,
                    prefix,
                    RunnerInstallation {
                        channel: "installed".into(),
                        version: "installed".into(),
                        root,
                        wine: launcher,
                    },
                )))
            }
            ExecutionClass::ProtonUmu => Ok(Self::Umu(UmuEnvironment::new(
                paths,
                prefix,
                launcher,
                launch_environment
                    .get("GAMEID")
                    .cloned()
                    .unwrap_or_else(|| "umu-default".into()),
                launch_environment.get("PROTONPATH").cloned(),
            ))),
            ExecutionClass::ContainerizedWine => {
                bail!("this application uses a container backend not supported by this build")
            }
            ExecutionClass::VirtualMachine => {
                Ok(Self::Vm(VmEnvironment::from_installed(paths, prefix)?))
            }
            ExecutionClass::Restricted => bail!("restricted applications cannot be launched"),
        }
    }

    pub fn class(&self) -> ExecutionClass {
        match self {
            Self::Wine(_) => ExecutionClass::Wine,
            Self::Umu(_) => ExecutionClass::ProtonUmu,
            Self::Vm(_) => ExecutionClass::VirtualMachine,
        }
    }

    pub fn backend(&self) -> &'static str {
        match self {
            Self::Wine(_) => "wine",
            Self::Umu(_) => "umu",
            Self::Vm(_) => "libvirt-qemu",
        }
    }

    pub fn prefix(&self) -> &Path {
        match self {
            Self::Wine(environment) => &environment.prefix,
            Self::Umu(environment) => &environment.prefix,
            Self::Vm(environment) => environment.prefix(),
        }
    }

    pub fn launcher(&self) -> PathBuf {
        match self {
            Self::Wine(environment) => environment.runner.wine.clone(),
            Self::Umu(environment) => environment.launcher.clone(),
            Self::Vm(environment) => environment.launcher(),
        }
    }

    pub fn base_environment(&self) -> BTreeMap<String, String> {
        match self {
            Self::Wine(environment) => environment.base_environment(),
            Self::Umu(environment) => environment.base_environment(),
            Self::Vm(environment) => environment.base_environment(),
        }
    }

    pub fn initialize(
        &self,
        plan: &CompatibilityPlan,
        progress: &Progress,
        log: &Path,
    ) -> Result<()> {
        match self {
            Self::Wine(environment) => environment.initialize(plan, progress, log),
            Self::Umu(environment) => environment.initialize(plan, progress, log),
            Self::Vm(environment) => environment.initialize(plan, progress, log),
        }
    }

    pub fn run(
        &self,
        executable: &Path,
        args: &[String],
        extra: &BTreeMap<String, String>,
        log: &Path,
    ) -> Result<RunOutcome> {
        match self {
            Self::Wine(environment) => environment.run(executable, args, extra, log),
            Self::Umu(environment) => environment.run(executable, args, extra, log),
            Self::Vm(environment) => environment.run(executable, args, extra, log),
        }
    }

    pub fn launch(
        &self,
        executable: &Path,
        args: &[String],
        extra: &BTreeMap<String, String>,
        log: &Path,
    ) -> Result<()> {
        match self {
            Self::Wine(environment) => environment.launch(executable, args, extra, log),
            Self::Umu(environment) => environment.launch(executable, args, extra, log),
            Self::Vm(environment) => environment.launch(executable, args, extra, log),
        }
    }

    pub fn snapshot(&self, app_id: &str, attempt: usize) -> Result<PathBuf> {
        match self {
            Self::Wine(environment) => environment.snapshot(app_id, attempt),
            Self::Umu(environment) => environment.snapshot(app_id, attempt),
            Self::Vm(environment) => environment.snapshot(app_id, attempt),
        }
    }

    pub fn restore(&self, snapshot: &Path) -> Result<()> {
        match self {
            Self::Wine(environment) => environment.restore(snapshot),
            Self::Umu(environment) => environment.restore(snapshot),
            Self::Vm(environment) => environment.restore(snapshot),
        }
    }

    pub fn install_component(&self, component: &str, log: &Path) -> Result<()> {
        match self {
            Self::Wine(environment) => environment.install_component(component, log),
            Self::Umu(environment) => environment.install_component(component, log),
            Self::Vm(environment) => environment.install_component(component, log),
        }
    }

    pub fn update_prefix(&self, log: &Path) -> Result<RunOutcome> {
        match self {
            Self::Wine(environment) => {
                environment.tool("wineboot", ["--update"], &BTreeMap::new(), log)
            }
            Self::Umu(environment) => environment.update_prefix(log),
            Self::Vm(environment) => environment.update(log),
        }
    }

    pub fn switch_wine_runner(&mut self, runner: RunnerInstallation) -> Result<()> {
        match self {
            Self::Wine(environment) => {
                environment.runner = runner;
                Ok(())
            }
            Self::Umu(_) => bail!("UMU runner changes require an execution-class fallback"),
            Self::Vm(_) => bail!("a Windows VM does not use a Wine runner"),
        }
    }
}
