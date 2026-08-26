use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

pub fn executable_inventory(prefix: &Path) -> BTreeSet<PathBuf> {
    let drive = prefix.join("drive_c");
    if !drive.exists() {
        return BTreeSet::new();
    }
    WalkDir::new(drive)
        .follow_links(false)
        .max_depth(8)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        })
        .map(|entry| entry.into_path())
        .collect()
}

pub fn choose_installed(before: &BTreeSet<PathBuf>, after: &BTreeSet<PathBuf>) -> Option<PathBuf> {
    after
        .difference(before)
        .filter(|path| score(path) > 0)
        .max_by_key(|path| score(path))
        .cloned()
}

fn score(path: &Path) -> i32 {
    let lower = path.to_string_lossy().to_ascii_lowercase();
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut score = 0;
    if lower.contains("program files") {
        score += 30;
    }
    if path
        .parent()
        .and_then(Path::file_name)
        .and_then(|parent| parent.to_str())
        .is_some_and(|parent| parent.eq_ignore_ascii_case(&name))
    {
        score += 40;
    }
    if lower.contains("start menu") {
        score += 10;
    }
    if [
        "unins",
        "uninstall",
        "update",
        "updater",
        "crash",
        "report",
        "helper",
        "service",
        "streaming",
        "monitor",
        "overlay",
        "driver",
        "query",
        "dump",
        "capture",
        "setup",
        "install",
        "redist",
    ]
    .iter()
    .any(|word| name.contains(word))
    {
        score -= 45;
    }
    if lower.contains("windows/system32") || lower.contains("windows\\system32") {
        score -= 100;
    }
    if name.len() > 3 {
        score += 5;
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn main_program_beats_uninstaller() {
        let before = BTreeSet::new();
        let after = [
            PathBuf::from("drive_c/Program Files/Foo/Foo.exe"),
            PathBuf::from("drive_c/Program Files/Foo/uninstall.exe"),
        ]
        .into_iter()
        .collect();
        assert!(
            choose_installed(&before, &after)
                .unwrap()
                .ends_with("Foo.exe")
        );
    }

    #[test]
    fn builtin_windows_programs_are_not_applications() {
        let before = BTreeSet::new();
        let after = [PathBuf::from("drive_c/windows/system32/notepad.exe")]
            .into_iter()
            .collect();
        assert!(choose_installed(&before, &after).is_none());
    }

    #[test]
    fn product_named_executable_beats_sibling_tools() {
        let before = BTreeSet::new();
        let after = [
            PathBuf::from("drive_c/Program Files/Example/Example.exe"),
            PathBuf::from("drive_c/Program Files/Example/streaming_client.exe"),
            PathBuf::from("drive_c/Program Files/Example/bin/helper.exe"),
        ]
        .into_iter()
        .collect();
        assert!(
            choose_installed(&before, &after)
                .unwrap()
                .ends_with("Example.exe")
        );
    }
}
