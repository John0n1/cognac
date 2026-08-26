use crate::{model::InstalledApp, paths::CognacPaths};
use anyhow::{Context, Result};
use std::{fs, path::PathBuf};

const ICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">
<defs><linearGradient id="b" x2="0" y2="1"><stop stop-color="#d98b45"/><stop offset="1" stop-color="#6e2819"/></linearGradient></defs>
<path fill="#31211d" d="M72 18h112l-10 37c-3 12-9 20-18 26v31c0 18 11 31 27 39 14 7 24 22 24 39v25c0 13-10 23-23 23H72c-13 0-23-10-23-23v-25c0-17 10-32 24-39 16-8 27-21 27-39V81c-9-6-15-14-18-26z"/>
<path fill="url(#b)" d="M83 153h90c17 8 25 20 25 38v22c0 9-6 15-15 15H73c-9 0-15-6-15-15v-22c0-18 8-30 25-38z"/>
<path fill="#edc889" d="M92 30h72l-6 22c-3 12-15 20-30 20s-27-8-30-20z"/><circle cx="105" cy="188" r="7" fill="#f6dca4" opacity=".75"/>
</svg>"##;

pub fn integrate(paths: &CognacPaths, app: &mut InstalledApp) -> Result<()> {
    let apps = paths.applications_dir()?;
    let icons = paths.icons_dir()?;
    fs::create_dir_all(&apps)?;
    fs::create_dir_all(&icons)?;
    let icon = icons.join(format!("cognac-{}.svg", app.app_id));
    fs::write(&icon, ICON)?;
    let name = sanitize(&app.name);
    let entry = format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName={}\nComment=Windows application managed by Cognac\nExec=cognac run {}\nIcon={}\nTerminal=false\nCategories=Utility;\nStartupNotify=true\nX-Cognac-AppId={}\n",
        name,
        app.app_id,
        icon.display(),
        app.app_id
    );
    let desktop = apps.join(format!("cognac-{}.desktop", app.app_id));
    fs::write(&desktop, entry).with_context(|| format!("cannot create {}", desktop.display()))?;
    app.icon = Some(icon);
    refresh_desktop_database(&apps);
    Ok(())
}

pub fn remove(paths: &CognacPaths, app: &InstalledApp) -> Result<()> {
    let desktop = paths
        .applications_dir()?
        .join(format!("cognac-{}.desktop", app.app_id));
    if desktop.exists() {
        fs::remove_file(desktop)?;
    }
    if let Some(icon) = &app.icon
        && icon.exists()
    {
        fs::remove_file(icon)?;
    }
    refresh_desktop_database(&paths.applications_dir()?);
    Ok(())
}

fn refresh_desktop_database(path: &PathBuf) {
    if crate::util::command_exists("update-desktop-database") {
        let _ = std::process::Command::new("update-desktop-database")
            .arg(path)
            .status();
    }
}

fn sanitize(value: &str) -> String {
    value.replace(['\n', '\r'], " ").trim().to_owned()
}
