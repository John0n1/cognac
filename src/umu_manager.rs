use crate::{
    paths::CognacPaths,
    progress::Progress,
    util::{atomic_json, find_command, read_json},
};
use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const RELEASE_API: &str =
    "https://api.github.com/repos/Open-Wine-Components/umu-launcher/releases/latest";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UmuInstallation {
    pub version: String,
    pub launcher: PathBuf,
    pub managed: bool,
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
    size: u64,
}

pub struct UmuManager<'a> {
    paths: &'a CognacPaths,
    client: Client,
}

impl<'a> UmuManager<'a> {
    pub fn new(paths: &'a CognacPaths) -> Result<Self> {
        Ok(Self {
            paths,
            client: Client::builder()
                .timeout(Duration::from_secs(300))
                .user_agent(format!("cognac/{}", crate::VERSION))
                .build()?,
        })
    }

    pub fn installed(&self) -> Option<UmuInstallation> {
        if let Some(launcher) = find_command("umu-run") {
            return Some(UmuInstallation {
                version: "system".into(),
                launcher,
                managed: false,
            });
        }
        let metadata: UmuInstallation = read_json(&self.paths.data.join("tools/umu.json")).ok()?;
        metadata.launcher.is_file().then_some(metadata)
    }

    pub fn ensure(&self, progress: &Progress) -> Result<UmuInstallation> {
        if let Some(installation) = self.installed() {
            return Ok(installation);
        }
        ensure_python()?;
        progress.update("Fetching a game-ready barrel...", Some(18));
        let release: Release = self
            .client
            .get(RELEASE_API)
            .send()?
            .error_for_status()?
            .json()?;
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name.ends_with("-zipapp.tar"))
            .context("the latest UMU release has no zipapp asset")?;
        let expected = asset
            .digest
            .as_deref()
            .and_then(|digest| digest.strip_prefix("sha256:"))
            .context("GitHub did not publish a SHA-256 digest for the UMU zipapp")?;

        let downloads = self.paths.cache.join("downloads");
        let tools = self.paths.data.join("tools");
        fs::create_dir_all(&downloads)?;
        fs::create_dir_all(&tools)?;
        let archive = downloads.join(&asset.name);
        download(&self.client, asset, &archive)?;
        let actual = sha256_file(&archive)?;
        if !actual.eq_ignore_ascii_case(expected) {
            bail!("UMU checksum mismatch; the downloaded launcher was not installed");
        }

        progress.update("Unpacking Proton's concierge...", Some(25));
        let staging = tempfile::Builder::new().prefix("umu-").tempdir_in(&tools)?;
        let status = Command::new("tar")
            .args(["--extract", "--no-same-owner", "--file"])
            .arg(&archive)
            .arg("--directory")
            .arg(staging.path())
            .status()
            .context("tar is required to unpack the UMU launcher")?;
        if !status.success() {
            bail!("could not unpack the verified UMU launcher");
        }
        let extracted = staging.path().join("umu");
        let launcher = extracted.join("umu-run");
        if !launcher.is_file() {
            bail!("the verified UMU archive contains no umu/umu-run");
        }
        fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755))?;
        let destination = tools.join(format!("umu-{}", release.tag_name));
        if !destination.exists() {
            fs::rename(&extracted, &destination)?;
        }
        let installation = UmuInstallation {
            version: release.tag_name,
            launcher: destination.join("umu-run"),
            managed: true,
        };
        if !installation.launcher.is_file() {
            bail!("managed UMU installation is incomplete");
        }
        atomic_json(&tools.join("umu.json"), &installation)?;
        Ok(installation)
    }
}

fn ensure_python() -> Result<()> {
    let python = find_command("python3")
        .context("managed UMU requires Python 3.10 or newer, but python3 was not found")?;
    let status = Command::new(python)
        .args([
            "-c",
            "import sys; raise SystemExit(0 if sys.version_info >= (3, 10) else 1)",
        ])
        .status()?;
    if !status.success() {
        bail!("managed UMU requires Python 3.10 or newer");
    }
    Ok(())
}

fn download(client: &Client, asset: &Asset, path: &Path) -> Result<()> {
    if path.is_file() && fs::metadata(path)?.len() == asset.size {
        return Ok(());
    }
    let response = client
        .get(&asset.browser_download_url)
        .send()?
        .error_for_status()?
        .bytes()?;
    if response.len() as u64 != asset.size {
        bail!("UMU download was incomplete");
    }
    let mut temporary =
        tempfile::NamedTempFile::new_in(path.parent().context("UMU download path has no parent")?)?;
    temporary.write_all(&response)?;
    temporary.as_file_mut().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zipapp_assets_are_selected_without_distro_guessing() {
        let release = Release {
            tag_name: "1.2.3".into(),
            assets: vec![
                Asset {
                    name: "umu.rpm".into(),
                    browser_download_url: "rpm".into(),
                    digest: None,
                    size: 1,
                },
                Asset {
                    name: "umu-launcher-1.2.3-zipapp.tar".into(),
                    browser_download_url: "zipapp".into(),
                    digest: Some("sha256:abc".into()),
                    size: 2,
                },
            ],
        };
        let selected = release
            .assets
            .iter()
            .find(|asset| asset.name.ends_with("-zipapp.tar"))
            .unwrap();
        assert_eq!(selected.browser_download_url, "zipapp");
    }
}
