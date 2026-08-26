use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_exposes_the_small_command_surface() {
    Command::cargo_bin("cognac")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("cognac [OPTIONS] [EXECUTABLE]"))
        .stdout(predicate::str::contains("doctor"));
}

#[test]
fn a_non_pe_file_fails_cleanly() {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), b"not a Windows program").unwrap();
    Command::cargo_bin("cognac")
        .unwrap()
        .arg(file.path())
        .arg("--dry-run")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not a Windows PE executable"));
}
