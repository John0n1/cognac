use anyhow::{Context, Result};
use serde::{Serialize, de::DeserializeOwned};
use std::{fs, io::Write, path::Path};

pub fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in value.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "windows-app".into()
    } else {
        trimmed.chars().take(48).collect()
    }
}

pub fn read_json<T: DeserializeOwned + Default>(path: &Path) -> Result<T> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .with_context(|| format!("invalid JSON in {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
    }
}

pub fn atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().context("JSON path has no parent")?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut temp, value)?;
    writeln!(temp)?;
    temp.as_file_mut().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub fn command_exists(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|directory| directory.join(name).is_file())
    })
}
