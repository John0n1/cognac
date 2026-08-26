use crate::{
    paths::CognacPaths,
    progress::Progress,
    util::{atomic_json, command_exists, read_json},
};
use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const RELEASE_API: &str = "https://api.github.com/repos/Kron4ek/Wine-Builds/releases/latest";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunnerInstallation {
    pub channel: String,
    pub version: String,
    pub root: PathBuf,
    pub wine: PathBuf,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
}

pub struct RunnerManager<'a> {
    paths: &'a CognacPaths,
    client: Client,
}

impl<'a> RunnerManager<'a> {
    pub fn new(paths: &'a CognacPaths) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(600))
            .user_agent(format!("cognac/{}", crate::VERSION))
            .build()?;
        Ok(Self { paths, client })
    }

    pub fn installed(&self, channel: &str) -> Option<RunnerInstallation> {
        let metadata = self.paths.runners().join(format!("{channel}.json"));
        let runner: RunnerInstallation = read_json(&metadata).ok()?;
        runner.wine.is_file().then_some(runner)
    }

    pub fn ensure(&self, channel: &str, progress: &Progress) -> Result<RunnerInstallation> {
        if let Some(runner) = self.installed(channel) {
            return Ok(runner);
        }
        match self.download(channel, progress) {
            Ok(runner) => Ok(runner),
            Err(_download_error) if command_exists("wine") => {
                progress.update("Using the system Wine as a fallback...", None);
                let wine = find_command("wine").context("Wine disappeared from PATH")?;
                Ok(RunnerInstallation {
                    channel: "system".into(),
                    version: "system".into(),
                    root: wine.parent().unwrap_or(Path::new("/")).to_path_buf(),
                    wine,
                })
            }
            Err(error) => Err(error),
        }
    }

    pub fn update(&self, channel: &str, progress: &Progress) -> Result<RunnerInstallation> {
        let metadata = self.paths.runners().join(format!("{channel}.json"));
        if metadata.exists() {
            fs::remove_file(metadata)?;
        }
        self.ensure(channel, progress)
    }

    fn download(&self, channel: &str, progress: &Progress) -> Result<RunnerInstallation> {
        if std::env::consts::ARCH != "x86_64" {
            bail!("managed Wine runners currently require an x86_64 Linux host");
        }
        progress.update("Selecting a fresh compatibility runner...", Some(12));
        let release: Release = self
            .client
            .get(RELEASE_API)
            .send()?
            .error_for_status()?
            .json()?;
        let wanted = if channel == "stable" {
            format!("wine-{}-amd64-wow64.tar.xz", release.tag_name)
        } else {
            format!("wine-{}-staging-amd64-wow64.tar.xz", release.tag_name)
        };
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == wanted)
            .with_context(|| {
                format!(
                    "release {} has no supported {channel} runner",
                    release.tag_name
                )
            })?;
        let sums = release
            .assets
            .iter()
            .find(|asset| asset.name == "sha256sums.txt")
            .context("runner release has no checksum file")?;
        let checksums = self
            .client
            .get(&sums.browser_download_url)
            .send()?
            .error_for_status()?
            .text()?;
        let expected = checksums
            .lines()
            .find_map(|line| {
                let mut fields = line.split_whitespace();
                let sum = fields.next()?;
                let name = fields.next()?.trim_start_matches('*');
                (name == asset.name).then(|| sum.to_ascii_lowercase())
            })
            .context("runner archive is absent from the published release checksum list")?;

        self.paths.cache.join("downloads").create_dir_all()?;
        let archive = self.paths.cache.join("downloads").join(&asset.name);
        progress.update("Fetching grapes...", Some(18));
        download(&self.client, asset, &archive, progress)?;
        let actual = sha256_file(&archive)?;
        if actual != expected {
            bail!("runner checksum mismatch; the download was not installed");
        }

        progress.update("Aging a fresh Windows vintage...", Some(30));
        let destination = self
            .paths
            .runners()
            .join(format!("{}-{}", channel, release.tag_name));
        if !destination.exists() {
            let staging = tempfile::Builder::new()
                .prefix("runner-")
                .tempdir_in(self.paths.runners())?;
            let status = Command::new("tar")
                .args(["--extract", "--xz", "--no-same-owner", "--file"])
                .arg(&archive)
                .arg("--directory")
                .arg(staging.path())
                .status()
                .context("the `tar` utility is required to unpack a runner")?;
            if !status.success() {
                bail!("could not unpack the managed Wine runner");
            }
            let root = locate_wine_root(staging.path())
                .context("downloaded runner contains no bin/wine")?;
            if root == staging.path() {
                fs::rename(staging.keep(), &destination)?;
            } else {
                fs::rename(root, &destination)?;
            }
        }
        let wine = destination.join("bin/wine");
        if !wine.is_file() {
            bail!(
                "managed runner is incomplete: {} is missing",
                wine.display()
            );
        }
        let runner = RunnerInstallation {
            channel: channel.into(),
            version: release.tag_name,
            root: destination,
            wine,
        };
        atomic_json(
            &self.paths.runners().join(format!("{channel}.json")),
            &runner,
        )?;
        Ok(runner)
    }
}

trait CreateDir {
    fn create_dir_all(&self) -> Result<()>;
}
impl CreateDir for PathBuf {
    fn create_dir_all(&self) -> Result<()> {
        fs::create_dir_all(self)?;
        Ok(())
    }
}

fn download(client: &Client, asset: &Asset, path: &Path, progress: &Progress) -> Result<()> {
    if path.is_file() && fs::metadata(path)?.len() == asset.size {
        return Ok(());
    }
    let mut response = client
        .get(&asset.browser_download_url)
        .send()?
        .error_for_status()?;
    let mut temp =
        tempfile::NamedTempFile::new_in(path.parent().context("download has no parent")?)?;
    let mut buffer = [0u8; 64 * 1024];
    let mut received = 0u64;
    loop {
        let count = response.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        temp.write_all(&buffer[..count])?;
        received += count as u64;
        let percent = 18 + ((received.saturating_mul(10) / asset.size.max(1)) as u8).min(10);
        progress.update("Fetching grapes...", Some(percent));
    }
    temp.as_file_mut().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    std::io::copy(&mut file, &mut hash)?;
    Ok(hex::encode(hash.finalize()))
}

fn locate_wine_root(path: &Path) -> Option<PathBuf> {
    if path.join("bin/wine").is_file() {
        return Some(path.to_path_buf());
    }
    fs::read_dir(path).ok()?.flatten().find_map(|entry| {
        entry
            .path()
            .join("bin/wine")
            .is_file()
            .then(|| entry.path())
    })
}

fn find_command(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|path| path.join(name))
            .find(|path| path.is_file())
    })
}
