use crate::progress::Progress;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    hash::{Hash, Hasher},
    path::Path,
    thread,
    time::{Duration, Instant, UNIX_EPOCH},
};
use walkdir::WalkDir;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionObservation {
    pub quiescent: bool,
    pub elapsed_millis: u128,
    pub filesystem_changes: u32,
    pub active_processes: usize,
    pub service_activity: bool,
    pub kernel_driver_activity: bool,
    pub reboot_requested: bool,
}

pub fn observe_install(
    prefix: &Path,
    log: &Path,
    progress: &Progress,
) -> Result<ExecutionObservation> {
    observe_with_limits(
        prefix,
        log,
        progress,
        Duration::from_secs(30),
        Duration::from_secs(2),
        Duration::from_millis(500),
    )
}

fn observe_with_limits(
    prefix: &Path,
    log: &Path,
    progress: &Progress,
    timeout: Duration,
    quiet_period: Duration,
    poll_interval: Duration,
) -> Result<ExecutionObservation> {
    let started = Instant::now();
    let mut last_change = started;
    let mut previous = prefix_fingerprint(prefix);
    let mut filesystem_changes = 0u32;
    let mut active_processes = active_prefix_processes(prefix);
    let mut quiescent = false;

    loop {
        let elapsed = started.elapsed();
        if elapsed >= timeout {
            break;
        }
        thread::sleep(poll_interval);
        let current = prefix_fingerprint(prefix);
        if current != previous {
            previous = current;
            filesystem_changes = filesystem_changes.saturating_add(1);
            last_change = Instant::now();
        }
        active_processes = active_prefix_processes(prefix);
        if active_processes > 0 {
            last_change = Instant::now();
        }
        progress.update(
            if active_processes > 0 {
                "Letting the updater finish its pour..."
            } else {
                "Checking that everything has settled..."
            },
            Some(78),
        );
        if last_change.elapsed() >= quiet_period {
        if active_processes == 0 && last_change.elapsed() >= quiet_period {
            quiescent = true;
            break;
        }
    }

    let log_tail = fs::read(log)
        .map(|bytes| {
            let start = bytes.len().saturating_sub(2 * 1024 * 1024);
            String::from_utf8_lossy(&bytes[start..]).to_ascii_lowercase()
        })
        .unwrap_or_default();
    Ok(ExecutionObservation {
        quiescent,
        elapsed_millis: started.elapsed().as_millis(),
        filesystem_changes,
        active_processes,
        service_activity: contains_any(
            &log_tail,
            &["createservice", "startservice", "service control manager"],
        ),
        kernel_driver_activity: contains_any(
            &log_tail,
            &[
                "service_kernel_driver",
                "ntloaddriver",
                "failed to load driver",
                "kernel driver",
            ],
        ),
        reboot_requested: contains_any(
            &log_tail,
            &[
                "restart required",
                "reboot required",
                "exit code 3010",
                "exit code 1641",
            ],
        ),
    })
}

fn prefix_fingerprint(prefix: &Path) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for entry in WalkDir::new(prefix)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        entry.path().hash(&mut hasher);
        if let Ok(metadata) = entry.metadata() {
            metadata.len().hash(&mut hasher);
            metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos())
                .hash(&mut hasher);
        }
    }
    hasher.finish()
}

fn active_prefix_processes(prefix: &Path) -> usize {
    let marker = format!("WINEPREFIX={}", prefix.display());
    fs::read_dir("/proc")
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.bytes().all(|byte| byte.is_ascii_digit()))
        })
        .filter(|entry| {
            fs::read(entry.path().join("environ"))
                .ok()
                .is_some_and(|environment| {
                    environment
                        .split(|byte| *byte == 0)
                        .any(|variable| variable == marker.as_bytes())
                })
        })
        .count()
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_changes_when_installed_files_change() {
        let directory = tempfile::tempdir().unwrap();
        let before = prefix_fingerprint(directory.path());
        fs::write(directory.path().join("app.exe"), b"one").unwrap();
        let after = prefix_fingerprint(directory.path());
        assert_ne!(before, after);
    }

    #[test]
    fn recognizes_behavioral_kernel_signals() {
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join("install.log");
        fs::write(&log, "CreateService SERVICE_KERNEL_DRIVER restart required").unwrap();
        let progress = Progress::new("test", true);
        let observation = observe_with_limits(
            directory.path(),
            &log,
            &progress,
            Duration::from_millis(2),
            Duration::from_millis(1),
            Duration::from_millis(1),
        )
        .unwrap();
        assert!(observation.service_activity);
        assert!(observation.kernel_driver_activity);
        assert!(observation.reboot_requested);
    }
}
