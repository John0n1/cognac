# Changelog

All notable changes to Cognac will be documented in this file.

## 0.2.0 - 2026-10-06

### Hardware analysis and native firmware operations

- Add manufacturer-independent `inspect-hardware` analysis of EXE, DLL and SYS
  imports, embedded PE resources, kernel dependencies and ASCII/UTF-16 device hints.
- Export embedded driver resources under SHA-256 filenames without executing them;
  bound resource traversal, detect cycles and preserve analysis errors.
- Index retained COFF function names, RVAs and file offsets for reverse engineering.
- Add `decode-ioctl` for the documented Windows CTL_CODE fields; private command
  semantics are not inferred or replayed.
- Feed embedded physical-memory and port-I/O dependencies into installation
  planning, preventing unsupported Wine/VM fallback across manufacturers.
- Add read-only `hardware-plan` review of an explicitly selected trusted fwupd
  alternative, keeping the analyzed EXE separate from the native release payload.
- Add `firmware-devices`, journaled `flash-firmware` and post-update
  `firmware-status`, with exact device/version targeting and no automatic reboot.
- Add nonexecuting Inno firmware extraction, capsule-header/hash inspection and
  read-only Phoenix ACPI interface discovery.
- Include an experimental Phoenix SCT EFI implementation and offline signature
  inspection tools. One exact platform/image BIOS update was physically verified;
  this remains a restricted proof of concept, not universal raw flashing support.
- Document unsupported protocols, static-analysis coverage and native adapter
  requirements. Verify with 64 Rust/CLI tests, Clippy, architecture fixtures and
  read-only inspection of the real updater package.

### Execution and installation improvements

- Replace Wine-only runner selection with execution-class planning.
- Add managed UMU-Proton and GE-Proton strategies with verified self-bootstrap.
- Route detected and observed kernel requirements toward VM-backed execution.
- Treat anti-cheat compatibility as runtime evidence instead of a vendor blacklist.
- Detect KVM, QEMU, libvirt, OVMF, swtpm, IOMMU, VFIO, container, audio, and graphics capabilities.
- Isolate runner-family fallbacks in separate environments and remember successful strategies.
- Observe updater/file/process activity and launch-test installed applications.
- Discover launch targets through Wine uninstall-registry metadata.
- Version and safely migrate the installed-application registry.
- Analyze large installers through bounded, read-only memory mapping.

## 0.1.0 - 2026-08-26

- Analyze Windows executables and produce automatic compatibility plans.
- Download and manage isolated Wine runners and application environments.
- Install common Windows components and retry classified failures safely.
- Detect installed applications and create Linux desktop integration.
- Provide quiet progress reporting plus management, repair, log, and doctor commands.
- Add Debian, RPM, Arch Linux, AppImage, and generic binary release formats.
