use crate::model::{Architecture, ExecutableInfo};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KnowledgeBase {
    pub profiles: Vec<Profile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    #[serde(default)]
    pub sha256: Vec<String>,
    #[serde(default)]
    pub product_contains: Vec<String>,
    #[serde(default)]
    pub indicators: Vec<String>,
    pub runner: Option<String>,
    #[serde(default)]
    pub components: Vec<String>,
    pub windows_version: Option<String>,
    pub architecture: Option<Architecture>,
    pub graphics: Option<String>,
    #[serde(default)]
    pub dll_overrides: BTreeMap<String, String>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl KnowledgeBase {
    pub fn load(user_path: &Path) -> Result<Self> {
        let mut built_in: Self = serde_json::from_str(include_str!("data/profiles.json"))
            .context("built-in compatibility profiles are invalid")?;
        if user_path.exists() {
            let bytes = fs::read(user_path)?;
            let mut user: Self = serde_json::from_slice(&bytes)
                .context("user compatibility profiles are invalid")?;
            user.profiles.append(&mut built_in.profiles);
            Ok(user)
        } else {
            Ok(built_in)
        }
    }

    pub fn identify(&self, info: &ExecutableInfo) -> Option<&Profile> {
        self.profiles
            .iter()
            .map(|profile| {
                let mut score = 0;
                if profile
                    .sha256
                    .iter()
                    .any(|hash| hash.eq_ignore_ascii_case(&info.sha256))
                {
                    score += 100;
                }
                if let Some(name) = &info.product_name {
                    let lower = name.to_ascii_lowercase();
                    if profile
                        .product_contains
                        .iter()
                        .any(|part| lower.contains(&part.to_ascii_lowercase()))
                    {
                        score += 20;
                    }
                }
                score += profile
                    .indicators
                    .iter()
                    .filter(|i| info.indicators.contains(i))
                    .count()
                    * 3;
                (score, profile)
            })
            .filter(|(score, _)| *score >= 10)
            .max_by_key(|(score, _)| *score)
            .map(|(_, profile)| profile)
    }
}
