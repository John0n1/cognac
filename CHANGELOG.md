# Changelog

All notable changes to Cognac will be documented in this file.

## Unreleased

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
