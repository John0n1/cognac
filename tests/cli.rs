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
fn firmware_inspection_requires_an_explicit_extraction_destination() {
    Command::cargo_bin("cognac")
        .unwrap()
        .args(["inspect-firmware", "update.exe"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--extract-to"));
}

#[test]
fn identified_firmware_is_rejected_before_runner_provisioning() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("update.exe");
    // Minimal parseable PE, with no executable code; only product metadata.
    let mut bytes = vec![0u8; 512];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[60..64].copy_from_slice(&128u32.to_le_bytes());
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[132..134].copy_from_slice(&0x14cu16.to_le_bytes());
    bytes[148..150].copy_from_slice(&224u16.to_le_bytes());
    bytes[150..152].copy_from_slice(&0x102u16.to_le_bytes());
    bytes[152..154].copy_from_slice(&0x10bu16.to_le_bytes());
    for unit in "ProductName\0Lenovo BIOS Update Utility\0".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    std::fs::write(&executable, bytes).unwrap();
    let mut command = Command::cargo_bin("cognac").unwrap();
    for variable in [
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_CONFIG_HOME",
        "XDG_STATE_HOME",
    ] {
        command.env(variable, directory.path());
    }
    command
        .arg(&executable)
        .arg("--quiet")
        .assert()
        .failure()
        .stderr(predicate::str::contains("physical computer's BIOS/UEFI"));
    for name in ["runners", "environments"] {
        let path = directory.path().join("cognac").join(name);
        assert_eq!(std::fs::read_dir(path).unwrap().count(), 0);
    }
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

#[test]
fn hardware_analysis_export_cannot_be_requested_in_dry_run() {
    Command::cargo_bin("cognac")
        .unwrap()
        .args([
            "inspect-hardware",
            "unopened.exe",
            "--export-embedded",
            "uncreated-directory",
            "--dry-run",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("omit it for read-only analysis"));
}

#[test]
fn ioctl_cli_decodes_fields_and_rejects_overflow_and_shell_text() {
    Command::cargo_bin("cognac")
        .unwrap()
        .args(["decode-ioctl", "0x8337EAF2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"function\": 2748"))
        .stdout(predicate::str::contains("\"method\": 2"));
    for value in ["0x100000000", "$(touch should-not-exist)", "-1"] {
        Command::cargo_bin("cognac")
            .unwrap()
            .args(["decode-ioctl", value])
            .assert()
            .failure();
    }
}

#[test]
fn native_hardware_plan_requires_explicit_device_and_version() {
    Command::cargo_bin("cognac")
        .unwrap()
        .args(["hardware-plan", "unopened.exe"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--device"))
        .stderr(predicate::str::contains("--version"));
}
