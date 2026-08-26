use crate::{
    model::InstalledApp,
    paths::CognacPaths,
    util::{atomic_json, read_json},
};
use anyhow::{Result, bail};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct AppRegistry {
    apps: BTreeMap<String, InstalledApp>,
}

impl AppRegistry {
    pub fn load(paths: &CognacPaths) -> Result<Self> {
        Ok(Self {
            apps: read_json(&paths.applications_file())?,
        })
    }
    pub fn save(&self, paths: &CognacPaths) -> Result<()> {
        atomic_json(&paths.applications_file(), &self.apps)
    }
    pub fn insert(&mut self, app: InstalledApp) {
        self.apps.insert(app.app_id.clone(), app);
    }
    pub fn get(&self, query: &str) -> Result<&InstalledApp> {
        if let Some(app) = self.apps.get(query) {
            return Ok(app);
        }
        let query = query.to_ascii_lowercase();
        let matches = self
            .apps
            .values()
            .filter(|app| app.name.to_ascii_lowercase().contains(&query))
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [app] => Ok(app),
            [] => bail!("no installed application matches `{query}`"),
            _ => bail!("more than one installed application matches `{query}`; use its id"),
        }
    }
    pub fn remove(&mut self, query: &str) -> Result<InstalledApp> {
        let id = self.get(query)?.app_id.clone();
        Ok(self.apps.remove(&id).expect("resolved registry id"))
    }
    pub fn values(&self) -> impl Iterator<Item = &InstalledApp> {
        self.apps.values()
    }
}
