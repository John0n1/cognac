use std::{
    collections::BTreeSet,
    fs,
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

pub fn discover_installed(
    prefix: &Path,
    before: &BTreeSet<PathBuf>,
    after: &BTreeSet<PathBuf>,
) -> Option<PathBuf> {
    registry_display_icons(prefix)
        .into_iter()
        .filter(|path| after.contains(path) && !before.contains(path))
        .max_by_key(|path| score(path) + 100)
        .or_else(|| choose_installed(before, after))
}

fn registry_display_icons(prefix: &Path) -> Vec<PathBuf> {
    [prefix.join("user.reg"), prefix.join("system.reg")]
        .into_iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .flat_map(|registry| {
            let mut in_uninstall_key = false;
            registry
                .lines()
                .filter_map(|line| {
                    if line.starts_with('[') {
                        in_uninstall_key = line.to_ascii_lowercase().contains("\\uninstall\\");
                        return None;
                    }
                    if !in_uninstall_key || !line.starts_with("\"DisplayIcon\"=") {
                        return None;
                    }
                    let value = line
                        .split_once('=')?
                        .1
                        .trim()
                        .trim_matches('"')
                        .split(',')
                        .next()?
                        .trim_matches('"');
                    windows_path(prefix, value)
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn windows_path(prefix: &Path, value: &str) -> Option<PathBuf> {
    let normalized = value
        .replace("\\\\", "/")
        .replace('\\', "/")
        .trim_start_matches("C:/")
        .trim_start_matches("c:/")
        .to_owned();
    if normalized.is_empty()
        || normalized
            .split('/')
            .any(|component| matches!(component, "" | "." | ".."))
    {
    let normalized = value.replace('\\', "/");
    let trimmed = normalized.trim().trim_matches('"');
    let stripped = trimmed
        .strip_prefix("C:/")
        .or_else(|| trimmed.strip_prefix("c:/"))
        .unwrap_or(trimmed);
    let mut relative = PathBuf::new();
    for part in stripped.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        relative.push(part);
    }
    if relative.as_os_str().is_empty() {
        return None;
    }
    Some(prefix.join("drive_c").join(normalized))
    Some(prefix.join("drive_c").join(relative))
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

    #[test]
    fn uninstall_registry_display_icon_identifies_the_launch_target() {
        let directory = tempfile::tempdir().unwrap();
        let prefix = directory.path();
        let expected = prefix.join("drive_c/Program Files/Example/Example.exe");
        let helper = prefix.join("drive_c/Program Files/Example/helper.exe");
        fs::create_dir_all(expected.parent().unwrap()).unwrap();
        fs::write(&expected, b"MZ").unwrap();
        fs::write(&helper, b"MZ").unwrap();
        fs::write(
            prefix.join("system.reg"),
            "[Software\\\\Microsoft\\\\Windows\\\\CurrentVersion\\\\Uninstall\\\\Example]\n\"DisplayIcon\"=\"C:\\\\Program Files\\\\Example\\\\Example.exe,0\"\n",
        )
        .unwrap();
        let after = [expected.clone(), helper].into_iter().collect();
        assert_eq!(
            discover_installed(prefix, &BTreeSet::new(), &after),
            Some(expected)
        );
    }

    #[test]
    fn windows_path_normalization_handles_various_formats() {
        let prefix = Path::new("/tmp/test_prefix");
        assert_eq!(
            windows_path(prefix, r#"C:\Program Files\App\app.exe"#),
            Some(PathBuf::from("/tmp/test_prefix/drive_c/Program Files/App/app.exe"))
        );
        assert_eq!(
            windows_path(prefix, r#"c:\\Program Files\\App\\app.exe"#),
            Some(PathBuf::from("/tmp/test_prefix/drive_c/Program Files/App/app.exe"))
        );
        assert_eq!(
            windows_path(prefix, r#""C:\App\app.exe""#),
            Some(PathBuf::from("/tmp/test_prefix/drive_c/App/app.exe"))
        );
        assert_eq!(
            windows_path(prefix, r#"\App\app.exe"#),
            Some(PathBuf::from("/tmp/test_prefix/drive_c/App/app.exe"))
        );
        assert_eq!(
            windows_path(prefix, r#"..\..\etc\passwd"#),
            None
        );
    }
}
