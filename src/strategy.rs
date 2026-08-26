use crate::{
    model::{ExecutableInfo, ExecutionClass, ResultQuality},
    paths::CognacPaths,
    util::{atomic_json, read_json},
};
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StrategyRecord {
    pub sha256: String,
    pub identity: String,
    pub execution_class: ExecutionClass,
    pub backend: String,
    pub successes: u32,
    pub failures: u32,
    pub last_result: ResultQuality,
    pub last_used: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct StrategyMemory {
    #[serde(default)]
    records: Vec<StrategyRecord>,
}

impl StrategyMemory {
    pub fn load(paths: &CognacPaths) -> Result<Self> {
        read_json(&paths.strategies_file())
    }

    pub fn save(&self, paths: &CognacPaths) -> Result<()> {
        atomic_json(&paths.strategies_file(), self)
    }

    pub fn preferred(&self, info: &ExecutableInfo) -> Option<&StrategyRecord> {
        let identity = identity(info);
        self.records
            .iter()
            .filter(|record| record.sha256 == info.sha256 || record.identity == identity)
            .filter(|record| record.successes > 0)
            .max_by_key(|record| {
                (
                    u8::from(record.sha256 == info.sha256),
                    record.successes.saturating_sub(record.failures),
                    &record.last_used,
                )
            })
    }

    pub fn record_success(
        &mut self,
        info: &ExecutableInfo,
        execution_class: ExecutionClass,
        backend: &str,
        quality: ResultQuality,
    ) {
        let record = self.entry(info, execution_class, backend);
        record.successes = record.successes.saturating_add(1);
        record.last_result = quality;
        record.last_used = Utc::now().to_rfc3339();
    }

    pub fn record_failure(
        &mut self,
        info: &ExecutableInfo,
        execution_class: ExecutionClass,
        backend: &str,
    ) {
        let record = self.entry(info, execution_class, backend);
        record.failures = record.failures.saturating_add(1);
        record.last_result = ResultQuality::Failed;
        record.last_used = Utc::now().to_rfc3339();
    }

    fn entry(
        &mut self,
        info: &ExecutableInfo,
        execution_class: ExecutionClass,
        backend: &str,
    ) -> &mut StrategyRecord {
        let identity = identity(info);
        if let Some(index) = self.records.iter().position(|record| {
            record.sha256 == info.sha256
                && record.execution_class == execution_class
                && record.backend == backend
        }) {
            return &mut self.records[index];
        }
        self.records.push(StrategyRecord {
            sha256: info.sha256.clone(),
            identity,
            execution_class,
            backend: backend.into(),
            successes: 0,
            failures: 0,
            last_result: ResultQuality::Unverified,
            last_used: Utc::now().to_rfc3339(),
        });
        self.records.last_mut().expect("record was just inserted")
    }
}

fn identity(info: &ExecutableInfo) -> String {
    let publisher = info.publisher.as_deref().unwrap_or("unknown-publisher");
    let product = info
        .product_name
        .as_deref()
        .unwrap_or("unknown-application");
    format!("{publisher}::{product}")
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ApplicationClass, Architecture, InstallerType, TrustRequirements};
    use std::path::PathBuf;

    fn executable(hash: &str) -> ExecutableInfo {
        ExecutableInfo {
            path: PathBuf::from("setup.exe"),
            sha256: hash.into(),
            size: 1,
            architecture: Architecture::X64,
            installer_type: InstallerType::PortableOrUnknown,
            product_name: Some("Some App".into()),
            publisher: Some("Some Company".into()),
            imports: vec![],
            graphics_apis: vec![],
            frameworks: vec![],
            indicators: vec![],
            application_class: ApplicationClass::General,
            trust: TrustRequirements::default(),
        }
    }

    #[test]
    fn remembers_a_success_across_installer_hashes() {
        let mut memory = StrategyMemory::default();
        memory.record_success(
            &executable("old"),
            ExecutionClass::Wine,
            "wine-staging",
            ResultQuality::Functional,
        );
        let learned = memory.preferred(&executable("new")).unwrap();
        assert_eq!(learned.backend, "wine-staging");
        assert_eq!(learned.successes, 1);
    }
}
