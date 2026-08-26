use crate::{model::InstalledApp, paths::CognacPaths, util::atomic_json};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs};

const REGISTRY_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Serialize, Deserialize)]
struct RegistryFile {
    schema_version: u32,
    apps: BTreeMap<String, InstalledApp>,
}

#[derive(Default)]
pub struct AppRegistry {
    apps: BTreeMap<String, InstalledApp>,
    migrated_legacy: bool,
}

impl AppRegistry {
    pub fn load(paths: &CognacPaths) -> Result<Self> {
        let path = paths.applications_file();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(error).with_context(|| format!("cannot read {}", path.display()));
            }
        };
        if let Ok(file) = serde_json::from_slice::<RegistryFile>(&bytes) {
            if file.schema_version > REGISTRY_SCHEMA_VERSION {
                bail!(
                    "application registry schema {} is newer than Cognac supports",
                    file.schema_version
                );
            }
            return Ok(Self {
                apps: file.apps,
                migrated_legacy: false,
            });
        }
        let mut apps = serde_json::from_slice::<BTreeMap<String, InstalledApp>>(&bytes)
            .with_context(|| format!("invalid application registry in {}", path.display()))?;
        for app in apps.values_mut() {
            if app.execution_backend.is_empty() {
                app.execution_backend = "wine-legacy".into();
            }
        }
        Ok(Self {
            apps,
            migrated_legacy: true,
        })
    }

    pub fn save(&self, paths: &CognacPaths) -> Result<()> {
        let path = paths.applications_file();
        if self.migrated_legacy && path.exists() {
            let backup = path.with_extension("v1.json");
            if !backup.exists() {
                fs::copy(&path, &backup).with_context(|| {
                    format!(
                        "cannot preserve legacy registry backup {}",
                        backup.display()
                    )
                })?;
            }
        }
        atomic_json(
            &path,
            &RegistryFile {
                schema_version: REGISTRY_SCHEMA_VERSION,
                apps: self.apps.clone(),
            },
        )
    }

    pub fn insert(&mut self, app: InstalledApp) {
        self.apps.insert(app.app_id.clone(), app);
    }

    pub fn get(&self, query: &str) -> Result<&InstalledApp> {
        if let Some(app) = self.apps.get(query) {
            return Ok(app);
        }
        let query = query.to_ascii_lowercase();
        let lower = query.to_ascii_lowercase();
        if let Some(app) = self
            .apps
            .values()
            .find(|app| app.app_id.to_ascii_lowercase() == lower || app.name.to_ascii_lowercase() == lower)
        {
            return Ok(app);
        }
        let matches = self
            .apps
            .values()
            .filter(|app| app.name.to_ascii_lowercase().contains(&query))
            .filter(|app| {
                app.name.to_ascii_lowercase().contains(&lower)
                    || app.app_id.to_ascii_lowercase().contains(&lower)
            })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_registry_is_backed_up_and_versioned() {
        let directory = tempfile::tempdir().unwrap();
        let paths = CognacPaths {
            data: directory.path().join("data"),
            cache: directory.path().join("cache"),
            config: directory.path().join("config"),
            state: directory.path().join("state"),
        };
        paths.ensure().unwrap();
        fs::write(paths.applications_file(), "{}").unwrap();
        let registry = AppRegistry::load(&paths).unwrap();
        registry.save(&paths).unwrap();
        assert!(
            paths
                .applications_file()
                .with_extension("v1.json")
                .is_file()
        );
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(paths.applications_file()).unwrap()).unwrap();
        assert_eq!(value["schema_version"], 2);
        assert!(value["apps"].is_object());
    }

    #[test]
    fn legacy_wine_app_records_receive_execution_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let paths = CognacPaths {
            data: directory.path().join("data"),
            cache: directory.path().join("cache"),
            config: directory.path().join("config"),
            state: directory.path().join("state"),
        };
        paths.ensure().unwrap();
        fs::write(
            paths.applications_file(),
            r#"{
              "old-app": {
                "app_id": "old-app",
                "name": "Old App",
                "executable": "/tmp/prefix/drive_c/Old.exe",
                "prefix": "/tmp/prefix",
                "runner": "/tmp/wine/bin/wine",
                "architecture": "x86_64",
                "installed_at": "2026-01-01T00:00:00Z",
                "icon": null,
                "launch_arguments": [],
                "quality": "unverified",
                "limitations": [],
                "source_sha256": null
              }
            }"#,
        )
        .unwrap();
        let registry = AppRegistry::load(&paths).unwrap();
        let app = registry.get("old-app").unwrap();
        assert_eq!(app.execution_class, crate::model::ExecutionClass::Wine);
        assert_eq!(app.execution_backend, "wine-legacy");
        assert!(app.launch_environment.is_empty());
    }

    #[test]
    fn exact_match_resolves_substring_ambiguity() {
        let mut registry = AppRegistry::default();
        let doom = InstalledApp {
            app_id: "doom-123".into(),
            name: "Doom".into(),
            executable: "/tmp/doom.exe".into(),
            prefix: "/tmp/prefix".into(),
            runner: "/tmp/wine".into(),
            architecture: crate::model::Architecture::X64,
            installed_at: "2026-01-01T00:00:00Z".into(),
            icon: None,
            launch_arguments: vec![],
            launch_environment: Default::default(),
            quality: crate::model::ResultQuality::Functional,
            limitations: vec![],
            source_sha256: None,
            execution_class: crate::model::ExecutionClass::Wine,
            execution_backend: "wine-staging".into(),
            execution_classification: crate::model::ExecutionClassification::CompatibilityLayer,
        };
        let doom_eternal = InstalledApp {
            app_id: "doom-eternal-456".into(),
            name: "Doom Eternal".into(),
            ..doom.clone()
        };
        registry.insert(doom);
        registry.insert(doom_eternal);
        let found = registry.get("Doom").unwrap();
        assert_eq!(found.app_id, "doom-123");
        let found_lower = registry.get("doom").unwrap();
        assert_eq!(found_lower.app_id, "doom-123");
    }
}
