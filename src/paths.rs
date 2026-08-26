use anyhow::{Context, Result};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CognacPaths {
    pub data: PathBuf,
    pub cache: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
}

impl CognacPaths {
    pub fn discover() -> Result<Self> {
        let home = dirs::home_dir().context("cannot determine the home directory")?;
        let data = dirs::data_dir()
            .unwrap_or_else(|| home.join(".local/share"))
            .join("cognac");
        let cache = dirs::cache_dir()
            .unwrap_or_else(|| home.join(".cache"))
            .join("cognac");
        let config = dirs::config_dir()
            .unwrap_or_else(|| home.join(".config"))
            .join("cognac");
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/state"))
            .join("cognac");
        Ok(Self {
            data,
            cache,
            config,
            state,
        })
    }

    pub fn runners(&self) -> PathBuf {
        self.data.join("runners")
    }
    pub fn prefixes(&self) -> PathBuf {
        self.data.join("prefixes")
    }
    pub fn environments(&self) -> PathBuf {
        self.data.join("environments")
    }
    pub fn applications_file(&self) -> PathBuf {
        self.data.join("applications.json")
    }
    pub fn strategies_file(&self) -> PathBuf {
        self.data.join("strategies.json")
    }
    pub fn logs(&self) -> PathBuf {
        self.state.join("logs")
    }
    pub fn snapshots(&self) -> PathBuf {
        self.state.join("snapshots")
    }
    pub fn applications_dir(&self) -> Result<PathBuf> {
        Ok(dirs::home_dir()
            .context("cannot determine home")?
            .join(".local/share/applications"))
    }
    pub fn icons_dir(&self) -> Result<PathBuf> {
        Ok(dirs::home_dir()
            .context("cannot determine home")?
            .join(".local/share/icons/hicolor/scalable/apps"))
    }

    pub fn ensure(&self) -> Result<()> {
        for path in [
            &self.data,
            &self.cache,
            &self.config,
            &self.state,
            &self.runners(),
            &self.prefixes(),
            &self.environments(),
            &self.logs(),
            &self.snapshots(),
        ] {
            std::fs::create_dir_all(path)
                .with_context(|| format!("cannot create {}", path.display()))?;
        }
        Ok(())
    }
}
