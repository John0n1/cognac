use crate::{
    analyzer, desktop,
    detector::{choose_installed, executable_inventory},
    diagnostics,
    environment::WineEnvironment,
    knowledge::KnowledgeBase,
    model::{CompatibilityPlan, InstalledApp, Repair, ResultQuality},
    paths::CognacPaths,
    planner,
    progress::Progress,
    registry::AppRegistry,
    runner::RunnerManager,
    system,
    util::slugify,
};
use anyhow::{Context, Result, bail};
use chrono::Utc;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub struct PreparedInstall {
    pub info: crate::model::ExecutableInfo,
    pub host: crate::model::HostInfo,
    pub plan: CompatibilityPlan,
}

pub fn prepare(paths: &CognacPaths, executable: &Path) -> Result<PreparedInstall> {
    let info = analyzer::analyze(executable)?;
    let host = system::detect()?;
    let knowledge = KnowledgeBase::load(&paths.config.join("profiles.json"))?;
    let plan = planner::build(&info, &host, knowledge.identify(&info));
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
        if let Some(candidate) =
            choose_installed(&BTreeSet::new(), &executable_inventory(&app.prefix))
            && candidate != app.executable
        {
            app.executable = candidate;
            app.name = app
                .executable
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(clean_app_name)
                .filter(|value| !value.is_empty())
                .unwrap_or(app.name);
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
    progress.update("Inspecting the vintage...", Some(6));
    let managers = RunnerManager::new(paths)?;
    let runner = managers.ensure(&prepared.plan.runner_channel, &progress)?;
    let prefix = paths.prefixes().join(&app_id);
    if prefix.exists()
        && let Some(installed) = choose_installed(&BTreeSet::new(), &executable_inventory(&prefix))
    {
        progress.update("Recovering a completed installation...", Some(92));
        let app_name = installed
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(clean_app_name)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| name.clone());
        let app = InstalledApp {
            app_id,
            name: app_name,
            executable: installed,
            prefix,
            runner: runner.wine,
            architecture: prepared.info.architecture,
            installed_at: Utc::now().to_rfc3339(),
            icon: None,
            launch_arguments: vec![],
            quality: ResultQuality::FunctionalWithLimitations,
            limitations: vec![
                "Recovered after the original installation process was interrupted".into(),
            ],
            source_sha256: Some(prepared.info.sha256),
        };
        return register(paths, app, &progress);
    }
    let log = paths.logs().join(format!("{app_id}.log"));
    if log.exists() {
        fs::rename(
            &log,
            log.with_extension(format!("{}.log", Utc::now().timestamp())),
        )?;
    }
    let mut environment = WineEnvironment::new(paths, prefix.clone(), runner);
    environment.initialize(&prepared.plan, &progress, &log)?;
    let before = executable_inventory(&prefix);
    let mut plan = prepared.plan;
    let mut limitations = Vec::new();
    let mut last_failure = None;

    for attempt in 1..=3 {
        let snapshot = environment.snapshot(&app_id, attempt)?;
        progress.update(
            if attempt == 1 {
                "Pouring the installer..."
            } else {
                "Trying another barrel..."
            },
            Some(55 + attempt as u8 * 8),
        );
        let runtime_environment = runtime_environment(&plan);
        let outcome = environment.run(executable, &[], &runtime_environment, &log)?;
        if matches!(outcome.status, Some(0 | 194)) {
            let after = executable_inventory(&prefix);
            let installed =
                choose_installed(&before, &after).unwrap_or_else(|| executable.to_path_buf());
            if installed == executable {
                limitations.push("No installed executable was detected; Cognac will launch the supplied executable directly".into());
            }
            let app_name = if installed != executable {
                installed
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(clean_app_name)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| name.clone())
            } else {
                name.clone()
            };
            let app = InstalledApp {
                app_id,
                name: app_name,
                executable: installed,
                prefix,
                runner: environment.runner.wine.clone(),
                architecture: prepared.info.architecture,
                installed_at: Utc::now().to_rfc3339(),
                icon: None,
                launch_arguments: vec![],
                quality: if limitations.is_empty() {
                    ResultQuality::Unverified
                } else {
                    ResultQuality::FunctionalWithLimitations
                },
                limitations,
                source_sha256: Some(prepared.info.sha256),
            };
            progress.update("Corking the bottle...", Some(92));
            return register(paths, app, &progress);
        }
        let failure = diagnostics::classify(&outcome.output, outcome.status);
        append_diagnostic(&log, attempt, &failure)?;
        last_failure = Some(failure.summary.clone());
        if !failure.retryable || attempt == 3 {
            break;
        }
        environment.restore(&snapshot)?;
        apply_repair(
            paths,
            &managers,
            &mut environment,
            &mut plan,
            failure.repair,
            &log,
            &progress,
        )?;
    }
    bail!(
        "installation failed after safe retries: {} (log: {})",
        last_failure.unwrap_or_else(|| "unknown failure".into()),
        log.display()
    )
}

fn runtime_environment(plan: &CompatibilityPlan) -> BTreeMap<String, String> {
    let mut env = plan.environment.clone();
    let mut overrides = plan
        .dll_overrides
        .iter()
        .map(|(dll, mode)| format!("{dll}={mode}"))
        .collect::<Vec<_>>();
    match plan.graphics_backend.as_str() {
        "opengl" | "builtin" => {
            overrides.push("dxgi,d3d11,d3d10core,d3d9=b".into());
        }
        _ => {}
    }
    if !overrides.is_empty() {
        env.insert("WINEDLLOVERRIDES".into(), overrides.join(";"));
    }
    env
}

fn apply_repair(
    paths: &CognacPaths,
    manager: &RunnerManager<'_>,
    environment: &mut WineEnvironment<'_>,
    plan: &mut CompatibilityPlan,
    repair: Option<Repair>,
    log: &Path,
    progress: &Progress,
) -> Result<()> {
    match repair.unwrap_or(Repair::FallbackRunner) {
        Repair::AddVcrun => environment.install_component("vcrun2022", log),
        Repair::AddDotNet => environment.install_component("dotnet48", log),
        Repair::DisableDxvk | Repair::UseOpenGl => {
            plan.graphics_backend = "opengl".into();
            Ok(())
        }
        Repair::ChangeWindowsVersion => {
            plan.windows_version = if plan.windows_version == "win10" {
                "win7".into()
            } else {
                "win10".into()
            };
            Ok(())
        }
        Repair::FallbackRunner => {
            let fallback = plan
                .runner_fallbacks
                .first()
                .cloned()
                .unwrap_or_else(|| "stable".into());
            progress.update("Selecting another compatibility runner...", None);
            environment.runner = manager.ensure(&fallback, progress)?;
            Ok(())
        }
    }
    .with_context(|| {
        format!(
            "automatic repair could not be applied in {}",
            paths.data.display()
        )
    })
}

fn append_diagnostic(log: &Path, attempt: usize, failure: &crate::model::Failure) -> Result<()> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new().create(true).append(true).open(log)?;
    writeln!(
        file,
        "\n[cognac] attempt {attempt}: {} ({})",
        failure.summary, failure.category
    )?;
    Ok(())
}

fn clean_app_name(value: &str) -> String {
    value
        .replace(['_', '-'], " ")
        .split_whitespace()
        .filter(|word| {
            !["launcher", "client"]
                .iter()
                .any(|x| word.eq_ignore_ascii_case(x))
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
