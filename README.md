<p align="center">
  <img width="320" alt="Cognac" src="https://github.com/user-attachments/assets/c3cfff59-6b8d-4dd6-a1d6-8f70f58642dd" />
</p>

<h3 align="center">Windows apps on Linux. Less setup.</h3>

<p align="center">
  Cognac is a Rust-powered CLI that installs and manages Windows applications on Linux, handling much of the compatibility work for you.
</p>

<p align="center">
  <a href="https://github.com/John0n1/cognac/releases/download/v0.2.0/cognac_0.2.0_amd64.deb"><img alt="DEB" src="https://img.shields.io/badge/download-DEB-A81D33?logo=debian&logoColor=white"></a>
  <a href="https://github.com/John0n1/cognac/releases/download/v0.2.0/cognac-0.2.0-1.x86_64.rpm"><img alt="RPM" src="https://img.shields.io/badge/download-RPM-294172?logo=fedora&logoColor=white"></a>
  <a href="https://github.com/John0n1/cognac/releases/download/v0.2.0/cognac-bin-0.2.0-1-x86_64.pkg.tar.zst"><img alt="Arch Linux" src="https://img.shields.io/badge/download-Arch-1793D1?logo=archlinux&logoColor=white"></a>
  <a href="https://github.com/John0n1/cognac/releases/download/v0.2.0/Cognac-0.2.0-x86_64.AppImage"><img alt="AppImage" src="https://img.shields.io/badge/download-AppImage-2EA3F2?logo=appimage&logoColor=white"></a>
</p>

## Quick start

Download a package from the [latest release](https://github.com/John0n1/cognac/releases/latest), install it using your distribution's package manager, then run:

```bash
cognac ~/Downloads/Setup.exe
```

Cognac analyzes the executable, selects an available compatibility strategy, prepares an isolated environment, and attempts the installation. It can also detect installed applications and create Linux launchers.

## Commands

```bash
cognac app.exe       # Install an application
cognac list          # Show managed applications
cognac run <app>     # Launch an installed app
cognac info <app>    # Show application details
cognac repair <app>  # Troubleshoot an installation
cognac logs <app>    # View detailed logs
cognac remove <app>  # Remove an application
cognac doctor        # Check the Linux host
cognac update        # Check for updates
```

Normal output stays concise; diagnostic logs remain available when something goes wrong.

## What it handles

- **Application analysis:** PE architecture, installer type, metadata, and compatibility clues.
- **Runner selection:** Managed Wine and Proton via UMU, with isolated environments for cross-runner retries.
- **Dependencies:** Common Windows runtimes, graphics components, and Linux-side requirements where supported.
- **Recovery:** Failure detection, targeted repairs, and alternative strategies.
- **Desktop integration:** Installed executable discovery, icons, and application launchers.

The aim is to make `cognac app.exe` the normal workflow—not manual prefix management, DLL hunting, or runner configuration.

## Execution support

| Backend | Status |
| --- | --- |
| Managed Wine | Available |
| UMU / Proton | Available |
| Containerized Wine | Detection and planning; execution in development |
| Windows VM | Host checks and planning; managed guest provisioning in development |

Some software depends on Windows kernel drivers, anti-cheat systems, or hardware features that Wine cannot provide. Cognac can identify these cases and plan for a VM, but VM support is **not yet an automatic fallback**. A Windows VM also requires suitable hardware and a licensed Windows environment, and it does not guarantee that protected software will run.

## Install from source

Requires the Rust toolchain and the appropriate Linux build dependencies.

```bash
cargo build --release --locked
sudo install -Dm755 target/release/cognac /usr/local/bin/cognac
```

Prebuilt v0.2.0 packages target **x86_64**: DEB, RPM, Arch, and AppImage. The release also includes a generic archive and checksums.

## Project status

Cognac is **under active development**. Managed Wine and UMU/Proton execution, runner fallback isolation, launch probing, and compatibility recovery are implemented. Container execution and the complete managed Windows VM lifecycle remain in progress.

Windows compatibility is application-specific. Cognac aims to automate the practical fixes, **not promise that every `.exe` will work**.

---

<p align="center"><strong>Cognac — less configuration, more application.</strong></p>
