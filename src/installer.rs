use crate::{
    analyzer, desktop,
    detector::{discover_installed, executable_inventory},
    diagnostics,
    execution::ExecutionEnvironment,
    knowledge::KnowledgeBase,
    model::{
        CompatibilityPlan, ExecutionClass, ExecutionClassification, ExecutionStrategy,
        InstalledApp, Repair, ResultQuality, StrategyAvailability,
    },
    observer,
    paths::CognacPaths,
    planner,
    progress::Progress,
    registry::AppRegistry,
    strategy::StrategyMemory,
    system,
    util::slugify,
    vm,
};
use anyhow::{Context, Result, bail};
use chrono::Utc;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

const ATTEMPTS_PER_STRATEGY: usize = 3;

pub struct PreparedInstall {
    pub info: crate::model::ExecutableInfo,
    pub host: crate::model::HostInfo,
    pub plan: CompatibilityPlan,
}

pub fn prepare(paths: &CognacPaths, executable: &Path) -> Result<PreparedInstall> {
    let info = analyzer::analyze(executable)?;
    let mut host = system::detect()?;
    if let Ok(report) = vm::probe(paths) {
        host.capabilities.windows_vm_configured = report.configured;
    }
    let knowledge = KnowledgeBase::load(&paths.config.join("profiles.json"))?;
    let memory = StrategyMemory::load(paths)?;
    let plan = planner::build(
        &info,
        &host,
        knowledge.identify(&info),
        memory.preferred(&info),
    );
    Ok(PreparedInstall { info, host, plan })
}

pub fn install(paths: &CognacPaths, executable: &Path, quiet: bool) -> Result<InstalledApp> {
    paths.ensure()?;
    let prepared = prepare(paths, executable)?;
    let mut registry = AppRegistry::load(paths)?;
    let existing = registry
        .values()
        .find(|app| app.source_sha256.as_deref() == Some(&prepared.info.sha256))
        .cloned();
    if let Some(mut app) = existing {
        if let Some(candidate) = discover_installed(
            &app.prefix,
            &BTreeSet::new(),
            &executable_inventory(&app.prefix),
        ) && candidate != app.executable
        {
            app.executable = candidate;
            app.name = detected_name(&app.executable, &app.name);
            app.quality = ResultQuality::Unverified;
            app.limitations
                .retain(|value| !value.contains("Recovered after"));
            desktop::integrate(paths, &mut app)?;
            registry.insert(app.clone());
            registry.save(paths)?;
        }
        return Ok(app);
    }
    if prepared.info.architecture == crate::model::Architecture::Arm64 {
        bail!("Windows ARM64 executables are not supported on this host yet");
    }

    let name = prepared
        .info
        .product_name
        .clone()
        .unwrap_or_else(|| "Windows application".into());
    let app_id = format!("{}-{}", slugify(&name), &prepared.info.sha256[..8]);
    let progress = Progress::new(format!("Installing {name}"), quiet);
    progress.update("Inspecting execution requirements...", Some(6));

    if prepared.plan.execution.class == ExecutionClass::Restricted {
        bail!(
            "Cognac cannot safely run this application: {}",
            prepared.plan.execution.reasons.join("; ")
        );
    }

    rotate_log(paths, &app_id)?;
    let mut strategies = vec![prepared.plan.execution.clone()];
    strategies.extend(prepared.plan.execution_fallbacks.clone());
    strategies.retain(|strategy| {
        strategy.availability != StrategyAvailability::Blocked
            && strategy.class != ExecutionClass::Restricted
    });
    let mut memory = StrategyMemory::load(paths)?;
    let mut failures = Vec::new();
    let mut kernel_required = prepared.info.trust.requires_windows_kernel();

    for (strategy_index, strategy) in strategies.iter().enumerate() {
        if kernel_required && strategy.class != ExecutionClass::VirtualMachine {
            continue;
        }
        progress.update(
            format!("Preparing {}...", strategy.class),
            Some(12 + (strategy_index as u8).saturating_mul(8)),
        );
        let prefix = strategy_prefix(paths, &app_id, strategy);
        let log = paths.logs().join(format!("{app_id}.log"));
        let mut environment =
            match ExecutionEnvironment::provision(paths, strategy, prefix.clone(), &progress) {
                Ok(environment) => environment,
                Err(error) => {
                    failures.push(format!("{}: {error:#}", strategy.class));
                    memory.record_failure(&prepared.info, strategy.class, &strategy.backend);
                    memory.save(paths)?;
                    continue;
                }
            };

        if let Some(app) = recover_interrupted(
            paths,
            &prepared,
            &name,
            &app_id,
            strategy,
            &environment,
            &progress,
        )? {
            memory.record_success(
                &prepared.info,
                strategy.class,
                &strategy.backend,
                app.quality.clone(),
            );
            memory.save(paths)?;
            return Ok(app);
        }

        let mut plan = prepared.plan.clone();
        if let Err(error) = environment.initialize(&plan, &progress, &log) {
            failures.push(format!("{} initialization: {error:#}", strategy.class));
            memory.record_failure(&prepared.info, strategy.class, &strategy.backend);
            memory.save(paths)?;
            continue;
        }
        let before = executable_inventory(environment.prefix());
        let mut last_failure = None;

        for attempt in 1..=ATTEMPTS_PER_STRATEGY {
            let snapshot = environment.snapshot(
                &format!("{}-{}", app_id, slugify(&strategy.backend)),
                attempt,
            )?;
            progress.update(
                if attempt == 1 && strategy_index == 0 {
                    "Pouring the installer..."
                } else if attempt == 1 {
                    "Trying another execution class..."
                } else {
                    "Trying another barrel..."
                },
                Some((52 + (strategy_index as u8 * 8) + attempt as u8 * 4).min(84)),
            );
            let runtime_env = runtime_environment(&plan, strategy.class);
            let outcome = match environment.run(executable, &[], &runtime_env, &log) {
                Ok(outcome) => outcome,
                Err(error) => {
                    environment.restore(&snapshot)?;
                    last_failure = Some(format!("could not start backend: {error:#}"));
                    break;
                }
            };
            if matches!(outcome.status, Some(0 | 194)) {
                let observation = observer::observe_install(environment.prefix(), &log, &progress)?;
                append_observation(&log, strategy, &observation)?;
                if observation.kernel_driver_activity
                    && matches!(
                        strategy.class,
                        ExecutionClass::Wine | ExecutionClass::ProtonUmu
                    )
                {
                    kernel_required = true;
                    last_failure = Some(
                        "installer behavior revealed a Windows kernel component; advancing to VM-backed execution"
                            .into(),
                    );
                    break;
                }
                let after = executable_inventory(environment.prefix());
                let installed = discover_installed(environment.prefix(), &before, &after)
                    .unwrap_or_else(|| executable.to_path_buf());
                let mut limitations = Vec::new();
                if installed == executable {
                    limitations.push("No installed executable was detected; Cognac will launch the supplied executable directly".into());
                }
                if !prepared.info.trust.anti_cheat.is_empty() {
                    limitations.push(format!(
                        "{} support depends on the application publisher enabling this execution path",
                        prepared.info.trust.anti_cheat.join(", ")
                    ));
                }
                if !observation.quiescent {
                    limitations.push(
                        "Background installation activity did not become quiet before Cognac's observation limit"
                            .into(),
                    );
                }
                if observation.reboot_requested {
                    limitations.push(
                        "The installer requested a Windows reboot; the environment may need repair before every feature works"
                            .into(),
                    );
                }
                let launch_environment =
                    persisted_environment(&environment, runtime_environment(&plan, strategy.class));
                let launch_verified = if observation.active_processes > 0 || installed == executable
                {
                    true
                } else {
                    progress.update("Giving the application a first sip...", Some(86));
                    match environment.launch(&installed, &[], &launch_environment, &log) {
                        Ok(()) => true,
                        Err(error) => {
                            limitations.push(format!(
                                "Installation completed, but the first launch probe failed: {error:#}"
                            ));
                            false
                        }
                    }
                };
                let quality = if limitations.is_empty() && launch_verified {
                    ResultQuality::Functional
                } else {
                    ResultQuality::FunctionalWithLimitations
                };
                let app = InstalledApp {
                    app_id: app_id.clone(),
                    name: detected_name(&installed, &name),
                    executable: installed,
                    prefix: environment.prefix().to_path_buf(),
                    runner: environment.launcher(),
                    architecture: prepared.info.architecture,
                    installed_at: Utc::now().to_rfc3339(),
                    icon: None,
                    launch_arguments: vec![],
                    launch_environment,
                    quality: quality.clone(),
                    limitations,
                    source_sha256: Some(prepared.info.sha256.clone()),
                    execution_class: strategy.class,
                    execution_backend: strategy.backend.clone(),
                    execution_classification: classification(strategy.class, &quality),
                };
                if snapshot.exists() {
                    let _ = fs::remove_dir_all(&snapshot);
                }
                memory.record_success(&prepared.info, strategy.class, &strategy.backend, quality);
                memory.save(paths)?;
                progress.update("Corking the bottle...", Some(92));
                return register(paths, app, &progress);
            }

            let failure = diagnostics::classify(&outcome.output, outcome.status);
            append_diagnostic(&log, attempt, strategy, &failure)?;
            last_failure = Some(failure.summary.clone());
            if !failure.retryable || attempt == ATTEMPTS_PER_STRATEGY {
                break;
            }
            environment.restore(&snapshot)?;
            if apply_repair(
                paths,
                &mut environment,
                &mut plan,
                failure.repair,
                &log,
                &progress,
            )? == RepairDisposition::NextStrategy
            {
                break;
            }
        }

        let summary = last_failure.unwrap_or_else(|| "unknown backend failure".into());
        failures.push(format!("{}: {summary}", strategy.class));
        memory.record_failure(&prepared.info, strategy.class, &strategy.backend);
        memory.save(paths)?;
    }
    # create windows image if none of the strategies worked
     if kernel_required {
        progress.update("Preparing a Windows virtual machine...", Some(90));
        let vm = vm::prepare(paths, &prepared.info, &prepared.host, &progress)?;
        let strategy = ExecutionStrategy {
            class: ExecutionClass::VirtualMachine,
            backend: vm.backend.clone(),
            availability: StrategyAvailability::Available,
        };
        let prefix = strategy_prefix(paths, &app_id, &strategy);
        let log = paths.logs().join(format!("{app_id}.log"));
        let mut environment = ExecutionEnvironment::provision(paths, &strategy, prefix.clone(), &progress)
            .context("failed to provision a virtual machine environment")?;
        environment.initialize(&prepared.plan, &progress, &log)
            .context("failed to initialize a virtual machine environment")?;
        let before = executable_inventory(environment.prefix());
        let snapshot = environment.snapshot(
            &format!("{}-{}", app_id, slugify(&strategy.backend)),
            1,
        )?;
        progress.update("Pouring the installer into the virtual machine...", Some(92));
        let runtime_env = runtime_environment(&prepared.plan, strategy.class);
        let outcome = environment.run(executable, &[], &runtime_env, &log)
            .context("failed to run the installer in the virtual machine")?;
        if !matches!(outcome.status, Some(0 | 194)) {
            let failure = diagnostics::classify(&outcome.output, outcome.status);
        append_diagnostic(&log, 1, &strategy, &failure) 
            .context("failed to log the virtual machine installation failure")?;
            bail!("installer failed in the virtual machine: {}", failure.summary);
        }
        let after = executable_inventory(environment.prefix());
        let installed = discover_installed(environment.prefix(), &before, &after)
            .unwrap_or_else(|| executable.to_path_buf());
        let launch_environment = persisted_environment(&environment, runtime_environment(&prepared.plan, strategy.class));
        let quality = ResultQuality::Functional;
        let app = InstalledApp {
            app_id: app_id.clone(),
            name: detected_name(&installed, &name),
            executable: installed,
            prefix: environment.prefix().to_path_buf(),
            runner: environment.launcher(),
            architecture: prepared.info.architecture,
            installed_at: Utc::now().to_rfc3339(),
            icon: None,
            launch_arguments: vec![],
            launch_environment,
            quality: quality.clone(),
            limitations: vec![],    
        source_sha256: Some(prepared.info.sha256.clone()),
            execution_class: strategy.class,
            execution_backend: strategy.backend.clone(),
            execution_classification: classification(strategy.class, &quality),
        };
        memory.record_success(&prepared.info, strategy.class, &strategy.backend, quality);
        memory.save(paths)?;
        return register(paths, app, &progress); 

}

fn recover_interrupted(
    paths: &CognacPaths,
    prepared: &PreparedInstall,
    fallback_name: &str,
    app_id: &str,
    strategy: &ExecutionStrategy,
    environment: &ExecutionEnvironment<'_>,
    progress: &Progress,
) -> Result<Option<InstalledApp>> {
    let Some(installed) = discover_installed(
        environment.prefix(),
        &BTreeSet::new(),
        &executable_inventory(environment.prefix()),
    ) else {
        return Ok(None);
    };
    progress.update("Recovering a completed installation...", Some(90));
    let quality = ResultQuality::FunctionalWithLimitations;
    let app = InstalledApp {
        app_id: app_id.into(),
        name: detected_name(&installed, fallback_name),
        executable: installed,
        prefix: environment.prefix().to_path_buf(),
        runner: environment.launcher(),
        architecture: prepared.info.architecture,
        installed_at: Utc::now().to_rfc3339(),
        icon: None,
        launch_arguments: vec![],
        launch_environment: persisted_environment(
            environment,
            runtime_environment(&prepared.plan, strategy.class),
        ),
        quality: quality.clone(),
        limitations: vec![
            "Recovered after the original installation process was interrupted".into(),
        ],
        source_sha256: Some(prepared.info.sha256.clone()),
        execution_class: strategy.class,
        execution_backend: strategy.backend.clone(),
        execution_classification: classification(strategy.class, &quality),
    };
    Ok(Some(register(paths, app, progress)?))
}

fn strategy_prefix(paths: &CognacPaths, app_id: &str, strategy: &ExecutionStrategy) -> PathBuf {
    paths
        .environments()
        .join(app_id)
        .join(slugify(&strategy.id()))
}

fn runtime_environment(
    plan: &CompatibilityPlan,
    execution_class: ExecutionClass,
) -> BTreeMap<String, String> {
    let mut environment = plan
        .environment
        .iter()
        .filter(|(key, _)| !key.starts_with("COGNAC_"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    if execution_class == ExecutionClass::ProtonUmu {
        if matches!(plan.graphics_backend.as_str(), "opengl" | "builtin") {
            environment.insert("PROTON_USE_WINED3D".into(), "1".into());
        }
        return environment;
    }

    let mut overrides = plan
        .dll_overrides
        .iter()
        .map(|(dll, mode)| format!("{dll}={mode}"))
        .collect::<Vec<_>>();
    if matches!(plan.graphics_backend.as_str(), "opengl" | "builtin") {
        overrides.push("dxgi,d3d11,d3d10core,d3d9=b".into());
    }
    if !overrides.is_empty() {
        environment.insert("WINEDLLOVERRIDES".into(), overrides.join(";"));
    }
    environment
}

fn persisted_environment(
    environment: &ExecutionEnvironment<'_>,
    mut runtime: BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut base = environment.base_environment();
    base.remove("WINEPREFIX");
    runtime.extend(base);
    runtime
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepairDisposition {
    Retry,
    NextStrategy,
}

fn apply_repair(
    paths: &CognacPaths,
    environment: &mut ExecutionEnvironment<'_>,
    plan: &mut CompatibilityPlan,
    repair: Option<Repair>,
    log: &Path,
    progress: &Progress,
) -> Result<RepairDisposition> {
    let result = match repair.unwrap_or(Repair::FallbackExecutionClass) {
        Repair::AddVcrun => environment
            .install_component("vcrun2022", log)
            .map(|()| RepairDisposition::Retry),
        Repair::AddDotNet => environment
            .install_component("dotnet48", log)
            .map(|()| RepairDisposition::Retry),
        Repair::AddMediaFoundation => environment
            .install_component("mf", log)
            .map(|()| RepairDisposition::Retry),
        Repair::AddDirectXCompiler => environment
            .install_component("d3dcompiler_47", log)
            .map(|()| RepairDisposition::Retry),
        Repair::AddXact => environment
            .install_component("xact", log)
            .map(|()| RepairDisposition::Retry),
        Repair::DisableDxvk | Repair::UseOpenGl => {
            plan.graphics_backend = "opengl".into();
            Ok(RepairDisposition::Retry)
        }
        Repair::DisableSync => {
            plan.environment
                .insert("PROTON_NO_FSYNC".into(), "1".into());
            plan.environment
                .insert("PROTON_NO_NTSYNC".into(), "1".into());
            Ok(RepairDisposition::Retry)
        }
        Repair::ChangeWindowsVersion => {
            plan.windows_version = if plan.windows_version == "win10" {
                "win7".into()
            } else {
                "win10".into()
            };
            Ok(RepairDisposition::Retry)
        }
        Repair::FallbackRunner | Repair::FallbackExecutionClass => {
            progress.update("Selecting another execution strategy...", None);
            Ok(RepairDisposition::NextStrategy)
        }
    };
    result.with_context(|| {
        format!(
            "automatic repair could not be applied in {}",
            paths.data.display()
        )
    })
}

fn append_diagnostic(
    log: &Path,
    attempt: usize,
    strategy: &ExecutionStrategy,
    failure: &crate::model::Failure,
) -> Result<()> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new().create(true).append(true).open(log)?;
    writeln!(
        file,
        "\n[cognac] {} attempt {attempt}: {} ({})",
        strategy.class, failure.summary, failure.category
    )?;
    Ok(())
}

fn append_observation(
    log: &Path,
    strategy: &ExecutionStrategy,
    observation: &observer::ExecutionObservation,
) -> Result<()> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new().create(true).append(true).open(log)?;
    writeln!(
        file,
        "\n[cognac] {} observation: {}",
        strategy.class,
        serde_json::to_string(observation)?
    )?;
    Ok(())
}

fn classification(
    execution_class: ExecutionClass,
    quality: &ResultQuality,
) -> ExecutionClassification {
    if *quality == ResultQuality::FunctionalWithLimitations {
        return ExecutionClassification::Degraded;
    }
    match execution_class {
        ExecutionClass::Wine | ExecutionClass::ProtonUmu | ExecutionClass::ContainerizedWine => {
            ExecutionClassification::CompatibilityLayer
        }
        ExecutionClass::VirtualMachine => ExecutionClassification::Virtualized,
        ExecutionClass::Restricted => ExecutionClassification::Unsupported,
    }
}

fn rotate_log(paths: &CognacPaths, app_id: &str) -> Result<()> {
    let log = paths.logs().join(format!("{app_id}.log"));
    if log.exists() {
        fs::rename(
            &log,
            log.with_extension(format!("{}.log", Utc::now().timestamp())),
        )?;
    }
    Ok(())
}

fn detected_name(path: &Path, fallback: &str) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(clean_app_name)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.into())
}

fn clean_app_name(value: &str) -> String {
    value
        .replace(['_', '-'], " ")
        .split_whitespace()
        .filter(|word| {
            !["launcher", "client"]
                .iter()
                .any(|candidate| word.eq_ignore_ascii_case(candidate))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn register(
    paths: &CognacPaths,
    mut app: InstalledApp,
    progress: &Progress,
) -> Result<InstalledApp> {
    desktop::integrate(paths, &mut app)?;
    let mut registry = AppRegistry::load(paths)?;
    registry.insert(app.clone());
    registry.save(paths)?;
    progress.update("Ready to serve.", Some(100));
    Ok(app)
}
