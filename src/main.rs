use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use cognac::{
    desktop, execution::ExecutionEnvironment, installer, paths::CognacPaths, progress::Progress,
    registry::AppRegistry, runner::RunnerManager, system, umu_manager::UmuManager, vm,
};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(
    name = "cognac",
    version,
    about = "Windows applications, served properly on Linux",
    subcommand_precedence_over_arg = true
)]
struct Cli {
    /// A Windows .exe to install or launch
    #[arg(value_name = "EXECUTABLE")]
    executable: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Commands>,
    /// Analyze and plan without downloading or changing anything
    #[arg(long, global = true)]
    dry_run: bool,
    /// Emit machine-readable output where supported
    #[arg(long, global = true)]
    json: bool,
    /// Suppress progress output
    #[arg(long, global = true)]
    quiet: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// List applications managed by Cognac
    List,
    /// Launch an installed application
    Run {
        app: String,
        #[arg(last = true)]
        arguments: Vec<String>,
    },
    /// Remove an application and its isolated environment
    Remove { app: String },
    /// Rebuild configuration for an installed application
    Repair { app: String },
    /// Show application details
    Info { app: String },
    /// Show or locate an application's diagnostic log
    Logs {
        app: String,
        #[arg(long)]
        path: bool,
        #[arg(long, default_value_t = 80)]
        lines: usize,
    },
    /// Check whether this Linux system is ready
    Doctor,
    /// Update Cognac-managed compatibility runners
    Update,
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cognac: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = CognacPaths::discover()?;
    match (cli.command.as_ref(), cli.executable.as_ref()) {
        (None, Some(executable)) => install_command(&paths, executable, &cli),
        (Some(Commands::List), None) => list_command(&paths, cli.json),
        (Some(Commands::Run { app, arguments }), None) => {
            run_command(&paths, app, arguments, cli.quiet)
        }
        (Some(Commands::Remove { app }), None) => remove_command(&paths, app),
        (Some(Commands::Repair { app }), None) => repair_command(&paths, app, cli.quiet),
        (Some(Commands::Info { app }), None) => info_command(&paths, app, cli.json),
        (Some(Commands::Logs { app, path, lines }), None) => {
            logs_command(&paths, app, *path, *lines)
        }
        (Some(Commands::Doctor), None) => doctor_command(&paths, cli.json),
        (Some(Commands::Update), None) => update_command(&paths, cli.quiet),
        (None, None) => {
            Cli::parse_from(["cognac", "--help"]);
            Ok(())
        }
        _ => bail!("provide either an executable or a command, not both"),
    }
}

fn install_command(paths: &CognacPaths, executable: &Path, cli: &Cli) -> Result<()> {
    let prepared = installer::prepare(paths, executable)?;
    if cli.dry_run {
        if cli.json {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({ "executable": prepared.info, "host": prepared.host, "plan": prepared.plan })
                )?
            );
        } else {
            println!(
                "{} ({}, {:?})",
                prepared
                    .info
                    .product_name
                    .as_deref()
                    .unwrap_or("Windows application"),
                prepared.info.architecture,
                prepared.info.installer_type
            );
            let fallbacks = prepared
                .plan
                .execution_fallbacks
                .iter()
                .filter(|strategy| {
                    strategy.availability != cognac::model::StrategyAvailability::Blocked
                        && strategy.class != cognac::model::ExecutionClass::Restricted
                })
                .map(|strategy| format!("{} ({})", strategy.class, strategy.backend))
                .collect::<Vec<_>>();
            println!("Application class: {}", prepared.info.application_class);
            println!(
                "Execution: {} ({}){}",
                prepared.plan.execution.class,
                prepared.plan.execution.backend,
                if fallbacks.is_empty() {
                    String::new()
                } else {
                    format!(" → {}", fallbacks.join(" → "))
                }
            );
            println!(
                "Environment: {}, Windows {}",
                prepared.plan.prefix_architecture,
                prepared.plan.windows_version.trim_start_matches("win")
            );
            println!("Graphics: {}", prepared.plan.graphics_backend);
            println!(
                "Components: {}",
                if prepared.plan.components.is_empty() {
                    "none".into()
                } else {
                    prepared.plan.components.join(", ")
                }
            );
            for reason in prepared.plan.reasons {
                println!("  • {reason}");
            }
            for reason in prepared.plan.execution.reasons {
                println!("  • {reason}");
            }
            for strategy in prepared
                .plan
                .execution_fallbacks
                .iter()
                .filter(|strategy| !strategy.blockers.is_empty())
            {
                println!(
                    "  {} {}: {}",
                    if strategy.availability == cognac::model::StrategyAvailability::Blocked {
                        "×"
                    } else {
                        "○"
                    },
                    strategy.class,
                    strategy.blockers.join("; ")
                );
            }
        }
        return Ok(());
    }
    let app = installer::install(paths, executable, cli.quiet)?;
    println!(
        "✓ {} is installed and available in your application menu.",
        app.name
    );
    if !app.limitations.is_empty() {
        println!(
            "  Installed with limitations: {}",
            app.limitations.join("; ")
        );
    }
    Ok(())
}

fn list_command(paths: &CognacPaths, json: bool) -> Result<()> {
    let registry = AppRegistry::load(paths)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&registry.values().collect::<Vec<_>>())?
        );
        return Ok(());
    }
    let apps = registry.values().collect::<Vec<_>>();
    if apps.is_empty() {
        println!("No applications installed yet. Try: cognac something.exe");
    } else {
        for app in apps {
            println!(
                "{:<28} {:<24} {}",
                app.app_id, app.execution_class, app.name
            );
        }
    }
    Ok(())
}

fn run_command(paths: &CognacPaths, query: &str, arguments: &[String], quiet: bool) -> Result<()> {
    let registry = AppRegistry::load(paths)?;
    let app = registry.get(query)?;
    let environment = ExecutionEnvironment::from_installed(
        paths,
        app.execution_class,
        app.prefix.clone(),
        app.runner.clone(),
        &app.launch_environment,
    )?;
    let mut args = app.launch_arguments.clone();
    args.extend_from_slice(arguments);
    let progress = Progress::new(format!("Opening {}", app.name), quiet);
    progress.update("Uncorking...", None);
    let log = paths.logs().join(format!("{}.log", app.app_id));
    environment.launch(&app.executable, &args, &app.launch_environment, &log)?;
    Ok(())
}

fn remove_command(paths: &CognacPaths, query: &str) -> Result<()> {
    let mut registry = AppRegistry::load(paths)?;
    let app = registry.remove(query)?;
    desktop::remove(paths, &app)?;
    let prefixes = paths.prefixes();
    let legacy_environment = app.prefix.starts_with(&prefixes)
        && app
            .prefix
            .file_name()
            .is_some_and(|n| n == app.app_id.as_str())
        && app.prefix.exists();
    let environment_root = paths.environments().join(&app.app_id);
    let current_environment =
        app.prefix.starts_with(&environment_root) && environment_root.exists();
    if legacy_environment {
        fs::remove_dir_all(&app.prefix)
            .with_context(|| format!("cannot remove {}", app.prefix.display()))?;
    } else if current_environment {
        fs::remove_dir_all(&environment_root)
            .with_context(|| format!("cannot remove {}", environment_root.display()))?;
    } else if app.prefix.exists() {
        bail!("refusing to remove an environment outside Cognac's managed directories");
    }
    registry.save(paths)?;
    println!("Removed {} and its isolated environment.", app.name);
    Ok(())
}

fn repair_command(paths: &CognacPaths, query: &str, quiet: bool) -> Result<()> {
    let registry = AppRegistry::load(paths)?;
    let app = registry.get(query)?;
    let environment = ExecutionEnvironment::from_installed(
        paths,
        app.execution_class,
        app.prefix.clone(),
        app.runner.clone(),
        &app.launch_environment,
    )?;
    let progress = Progress::new(format!("Repairing {}", app.name), quiet);
    progress.update("Polishing the registry...", None);
    let log = paths.logs().join(format!("{}.log", app.app_id));
    let outcome = environment.update_prefix(&log)?;
    if outcome.status != Some(0) {
        bail!("repair failed (log: {})", log.display());
    }
    println!("✓ {} has been repaired.", app.name);
    Ok(())
}

fn info_command(paths: &CognacPaths, query: &str, json: bool) -> Result<()> {
    let registry = AppRegistry::load(paths)?;
    let app = registry.get(query)?;
    if json {
        println!("{}", serde_json::to_string_pretty(app)?);
    } else {
        println!("{} ({})", app.name, app.app_id);
        println!("Executable: {}", app.executable.display());
        println!("Environment: {}", app.prefix.display());
        println!("Runner: {}", app.runner.display());
        println!(
            "Execution: {} ({})",
            app.execution_class, app.execution_backend
        );
        println!("Classification: {:?}", app.execution_classification);
        println!("Architecture: {}", app.architecture);
        println!("Status: {:?}", app.quality);
        for limitation in &app.limitations {
            println!("  • {limitation}");
        }
    }
    Ok(())
}

fn logs_command(paths: &CognacPaths, query: &str, only_path: bool, lines: usize) -> Result<()> {
    let registry = AppRegistry::load(paths)?;
    let app = registry.get(query)?;
    let log = paths.logs().join(format!("{}.log", app.app_id));
    if only_path {
        println!("{}", log.display());
        return Ok(());
    }
    if !log.exists() {
        bail!("no log exists for {}", app.name);
    }
    let mut file = fs::File::open(&log)?;
    let length = file.metadata()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(128 * 1024)))?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let selected = text.lines().rev().take(lines).collect::<Vec<_>>();
    for line in selected.into_iter().rev() {
        println!("{line}");
    }
    Ok(())
}

fn doctor_command(paths: &CognacPaths, json: bool) -> Result<()> {
    let mut host = system::detect()?;
    let vm_report = vm::probe(paths)?;
    host.capabilities.windows_vm_configured = vm_report.configured;
    if json {
        println!("{}", serde_json::to_string_pretty(&host)?);
        return Ok(());
    }
    println!(
        "Cognac {} on {} {} ({})",
        cognac::VERSION,
        host.distro_id,
        host.version,
        host.architecture
    );
    println!(
        "Package manager: {}",
        host.package_manager.as_deref().unwrap_or("not detected")
    );
    println!(
        "Vulkan: {}",
        if host.vulkan_available {
            "available"
        } else {
            "not detected"
        }
    );
    println!(
        "32-bit Vulkan: {}",
        if host.vulkan_32bit_likely {
            "available"
        } else {
            "not detected"
        }
    );
    println!(
        "Proton / UMU: {}",
        if let Some(umu) = UmuManager::new(paths)?.installed() {
            if umu.managed {
                "managed launcher ready"
            } else {
                "system launcher ready"
            }
        } else if host.capabilities.python3 {
            "will be downloaded when a game needs it"
        } else {
            "Python 3.10+ is required"
        }
    );
    println!(
        "Container isolation: {}",
        if host.capabilities.bubblewrap || host.capabilities.podman {
            "host support detected"
        } else {
            "not detected"
        }
    );
    println!(
        "Windows VM: {}",
        if host.capabilities.windows_vm_configured
            && host.capabilities.kvm_usable
            && host.capabilities.qemu
            && host.capabilities.libvirt
        {
            "ready"
        } else if host.capabilities.cpu_virtualization {
            "provisioning required"
        } else {
            "unavailable"
        }
    );
    println!(
        "VM trust stack: OVMF {}, swtpm {}, VFIO {}",
        availability(host.capabilities.ovmf),
        availability(host.capabilities.swtpm),
        availability(host.capabilities.iommu && host.capabilities.vfio)
    );
    for blocker in &vm_report.blockers {
        println!("  ○ VM setup: {blocker}");
    }
    let managed = RunnerManager::new(paths)?.installed("staging");
    println!(
        "Managed runner: {}",
        managed
            .map(|r| r.version)
            .as_deref()
            .unwrap_or("will be downloaded when needed")
    );
    for issue in &host.issues {
        println!("! {issue}");
    }
    if let Some(hint) = system::dependency_hint(&host) {
        println!("→ {hint}");
    }
    Ok(())
}

fn update_command(paths: &CognacPaths, quiet: bool) -> Result<()> {
    paths.ensure()?;
    let progress = Progress::new("Updating Cognac runners", quiet);
    let runner = RunnerManager::new(paths)?.update("staging", &progress)?;
    let umu = UmuManager::new(paths)?.ensure(&progress)?;
    println!(
        "✓ Managed Wine runner is {} and UMU launcher is {}.",
        runner.version, umu.version
    );
    Ok(())
}

fn availability(available: bool) -> &'static str {
    if available { "yes" } else { "no" }
}
