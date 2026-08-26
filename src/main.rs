use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use cognac::{
    desktop,
    environment::WineEnvironment,
    installer,
    model::InstalledApp,
    paths::CognacPaths,
    progress::Progress,
    registry::AppRegistry,
    runner::{RunnerInstallation, RunnerManager},
    system,
};
use std::{
    collections::BTreeMap,
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
            println!(
                "Runner: {} → {}",
                prepared.plan.runner_channel,
                prepared.plan.runner_fallbacks.join(" → ")
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
            println!("{:<28} {}", app.app_id, app.name);
        }
    }
    Ok(())
}

fn run_command(paths: &CognacPaths, query: &str, arguments: &[String], quiet: bool) -> Result<()> {
    let registry = AppRegistry::load(paths)?;
    let app = registry.get(query)?;
    let runner = runner_from_app(app);
    let environment = WineEnvironment::new(paths, app.prefix.clone(), runner);
    let mut args = app.launch_arguments.clone();
    args.extend_from_slice(arguments);
    let progress = Progress::new(format!("Opening {}", app.name), quiet);
    progress.update("Uncorking...", None);
    let log = paths.logs().join(format!("{}.log", app.app_id));
    environment.launch(&app.executable, &args, &BTreeMap::new(), &log)?;
    Ok(())
}

fn remove_command(paths: &CognacPaths, query: &str) -> Result<()> {
    let mut registry = AppRegistry::load(paths)?;
    let app = registry.remove(query)?;
    desktop::remove(paths, &app)?;
    let prefixes = paths.prefixes();
    if app.prefix.starts_with(&prefixes)
        && app
            .prefix
            .file_name()
            .is_some_and(|n| n == app.app_id.as_str())
        && app.prefix.exists()
    {
        fs::remove_dir_all(&app.prefix)
            .with_context(|| format!("cannot remove {}", app.prefix.display()))?;
    } else if app.prefix.exists() {
        bail!("refusing to remove an environment outside Cognac's prefix directory");
    }
    registry.save(paths)?;
    println!("Removed {} and its isolated environment.", app.name);
    Ok(())
}

fn repair_command(paths: &CognacPaths, query: &str, quiet: bool) -> Result<()> {
    let registry = AppRegistry::load(paths)?;
    let app = registry.get(query)?;
    let environment = WineEnvironment::new(paths, app.prefix.clone(), runner_from_app(app));
    let progress = Progress::new(format!("Repairing {}", app.name), quiet);
    progress.update("Polishing the registry...", None);
    let log = paths.logs().join(format!("{}.log", app.app_id));
    let outcome = environment.tool("wineboot", ["--update"], &BTreeMap::new(), &log)?;
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
    let selected = text.lines().rev().take(lines).collect::<Vec<_>>();
    for line in selected.into_iter().rev() {
        println!("{line}");
    }
    Ok(())
}

fn doctor_command(paths: &CognacPaths, json: bool) -> Result<()> {
    let host = system::detect()?;
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
    println!("✓ Managed runner updated to {}.", runner.version);
    Ok(())
}

fn runner_from_app(app: &InstalledApp) -> RunnerInstallation {
    RunnerInstallation {
        channel: "installed".into(),
        version: "installed".into(),
        root: app
            .runner
            .parent()
            .and_then(Path::parent)
            .unwrap_or(Path::new("/"))
            .into(),
        wine: app.runner.clone(),
    }
}
