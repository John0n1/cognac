# Cognac

<p align="center">
  <strong>Run Windows applications on Linux without babysitting Wine.</strong>
</p>

<p align="center">
  <img alt="Platform" src="https://img.shields.io/badge/platform-Linux-111111">
  <img alt="Language" src="https://img.shields.io/badge/language-Rust-111111?color=orange">
  <img alt="License" src="https://img.shields.io/badge/license-MIT-111111?color=blue">
  <img alt="Architecture" src="https://img.shields.io/badge/arch-x86__64-111111?color=white">
</p>

<p align="center">
  <a href="https://github.com/John0n1/cognac/releases/download/v0.1.0/cognac_0.1.0_amd64.deb">
    <img alt="DEB" src="https://img.shields.io/badge/download-DEB-A81D33?logo=debian&logoColor=white">
  </a>
  <a href="https://github.com/John0n1/cognac/releases/download/v0.1.0/cognac-0.1.0-1.x86_64.rpm">
    <img alt="RPM" src="https://img.shields.io/badge/download-RPM-294172?logo=fedora&logoColor=white">
  </a>
  <a href="https://github.com/John0n1/cognac/releases/download/v0.1.0/cognac-bin-0.1.0-1-x86_64.pkg.tar.zst">
    <img alt="Arch Linux" src="https://img.shields.io/badge/download-Arch-1793D1?logo=archlinux&logoColor=white">
  </a>
  <a href="https://github.com/John0n1/cognac/releases/download/v0.1.0/Cognac-0.1.0-x86_64.AppImage">
    <img alt="AppImage" src="https://img.shields.io/badge/download-AppImage-2EA3F2?logo=appimage&logoColor=white">
  </a>
</p>

<p align="center">
  <img
    width="400"
    alt="Cognac"
    src="https://github.com/user-attachments/assets/c3cfff59-6b8d-4dd6-a1d6-8f70f58642dd"
  />
</p>

<p align="center">
  Cognac is an intelligent compatibility and installation layer for Windows <code>.exe</code> applications on Linux.
</p>

## The idea

Running a Windows application on Linux often means figuring out Wine versions, prefixes, Winetricks components, DXVK, Vulkan libraries, architecture mismatches, installer quirks, and a long list of application-specific workarounds.

Cognac is meant to make that somebody else's problem.

```bash
cognac something.exe
```

Cognac inspects the executable and the Linux host, plans a compatible environment, downloads or selects the required compatibility runner, installs Windows components, runs the installer, watches what happens, retries known repairs when something fails, discovers the installed application, and creates a Linux desktop launcher.

The user should not need to understand Wine prefixes to install a Windows application.

## What Cognac does

A typical Cognac install looks like this internally:

```text
random.exe
    ↓
analyze executable
    ↓
inspect Linux host
    ↓
select execution class
    ↓
provision compatibility runtime
    ↓
resolve Linux + Windows dependencies
    ↓
create isolated environment
    ↓
install
    ↓
observe installer and child processes
    ↓
classify failures
    ↓
repair / rollback / retry
    ↓
discover installed application
    ↓
create Linux launcher
```

Cognac currently understands multiple execution classes:

```text
Wine
Proton / UMU
Containerized Wine
Windows VM
Unsupported / restricted
```

The selected execution strategy depends on the executable and on the capabilities of the current Linux host.

## Usage

Install or launch an executable directly:

```bash
cognac setup.exe
```

Analyze without making changes:

```bash
cognac --dry-run setup.exe
```

Machine-readable planning output:

```bash
cognac --dry-run --json setup.exe
```

List installed applications:

```bash
cognac list
```

Launch one later:

```bash
cognac run my-app
```

Inspect its state:

```bash
cognac info my-app
```

Show logs:

```bash
cognac logs my-app
```

Repair its managed environment:

```bash
cognac repair my-app
```

Remove it:

```bash
cognac remove my-app
```

Check the host:

```bash
cognac doctor
```

Update managed runners:

```bash
cognac update
```

## Quiet by default

Cognac is designed around one coherent progress interface rather than dumping raw Wine output into the terminal.

Typical status messages include:

```text
Aging a fresh Windows vintage...
Fetching grapes...
Convincing Windows it's totally at home...
Collecting tiny pieces of Windows...
Trying another barrel...
Corking the bottle...
```

Detailed diagnostics are written to managed log files and can be inspected later with:

```bash
cognac logs <app>
```

## Cognac makes decisions

Cognac tries to infer what an application needs instead of asking the user to choose every compatibility detail manually.

The executable analyzer currently examines information such as:

- PE architecture
- installer family
- imported DLLs
- graphics APIs
- common runtime/framework indicators
- product and publisher metadata
- game indicators
- service installation indicators
- kernel driver indicators
- anti-cheat markers
- likely Secure Boot or TPM requirements

The host analyzer checks capabilities such as:

- distribution family
- package manager
- Vulkan
- 32-bit Vulkan support
- Python
- UMU
- Proton installations
- Bubblewrap / Podman
- CPU virtualization
- KVM
- QEMU
- libvirt
- OVMF
- swtpm
- IOMMU / VFIO
- render nodes
- TPM devices
- PipeWire

The planner then scores available execution strategies.

Ordinary desktop applications generally prefer Wine. Games prefer Proton/UMU when available. Applications requiring genuine Windows kernel behavior can be routed toward a configured Windows VM instead of repeatedly attempting an impossible Wine setup.

## Self-bootstrapping

Cognac tries to carry its own compatibility tooling where practical.

Managed Wine runners are downloaded on demand and verified before installation. If a managed Wine download cannot be used but a system Wine is available, Cognac can fall back to it.

For game-focused execution Cognac can use the UMU launcher and can install a verified managed UMU zipapp when needed.

Winetricks is also downloaded as a pinned, checksum-verified tool when Cognac needs Windows compatibility components.

The long-term goal is that a fresh Linux install should need as little manual preparation as possible.

## Linux dependency resolution

Cognac reasons about host capabilities rather than hard-coding a single distribution.

Current package-manager detection includes:

- pacman
- apt
- dnf
- zypper

For example, a missing 32-bit Vulkan loader is the same compatibility requirement everywhere, but the package name differs by distribution.

Cognac currently detects and explains these missing host capabilities. Automatic privileged host-package installation is an area of active development.

## Architecture handling

32-bit Windows applications run inside a unified WoW64-style 64-bit prefix by default, avoiding the fragility of maintaining separate legacy 32-bit environments where possible.

Current managed Wine runners target x86_64 Linux hosts.

Windows ARM64 executables are detected but are not automatically supported yet.

## Application analysis

Cognac parses Windows PE executables directly in Rust.

It identifies common installer technologies including:

- MSI
- Inno Setup
- NSIS
- InstallShield
- WiX Burn
- Squirrel
- portable or unknown executables

It also detects common runtime needs such as:

- Visual C++ runtime
- .NET Framework
- Windows Media Foundation
- Direct3D 9/11/12
- DXGI
- Vulkan
- OpenGL

These hints feed into the compatibility plan before the installer is run.

## Compatibility knowledge

Cognac combines generic analysis with an optional compatibility profile database.

A profile can override or extend:

- preferred execution class
- runner
- Windows version
- graphics backend
- prefix architecture
- required components
- DLL overrides

This makes known-good application recipes possible without turning Cognac into a static database-only installer.

The generic analyzer remains the fallback for unknown executables.

## Automatic repair

When an installation fails, Cognac classifies the output and can apply known repairs before retrying.

Current repair categories include:

- missing Visual C++ runtime
- missing .NET runtime
- missing Media Foundation
- missing DirectX shader compiler
- missing XACT / XAudio runtime
- Vulkan/DXVK initialization failures
- VKD3D failures
- unsupported Windows version
- synchronization backend failures
- Windows kernel driver requirements
- virtualization-sensitive failures

Repairs can install components, change graphics paths, change Windows versions, disable unsupported synchronization mechanisms, or advance to another runner/execution class.

## Graceful degradation

Not every warning means an application is unusable.

Cognac records installation quality separately from simple process exit status:

```text
Fully functional
Functional
Functional with limitations
Unverified
Failed
```

This allows Cognac to accept a usable installation while clearly recording missing optional functionality.

An application can therefore be installed successfully even when some non-critical feature cannot be supported.

## Safe retries

Cognac snapshots an application's environment before risky installation attempts.

If a repair is needed, Cognac restores the previous environment before retrying rather than repeatedly mutating an already-broken prefix.

On supported filesystems normal compatibility environments use copy-on-write/reflink snapshots where available.

The Windows VM backend uses libvirt snapshots.

## Installer and process tracking

Windows installers frequently launch child processes, updaters, launchers, or the installed application before the original installer exits.

Cognac avoids treating the parent installer process as the entire installation lifecycle.

After installation it watches the environment for:

- filesystem changes
- active Wine-prefix processes
- service activity
- Windows kernel-driver activity
- reboot requests
- background updater activity

This helps Cognac distinguish a finished installer from an installation that is still settling.

## Installed application detection

Cognac compares the environment before and after installation and searches for newly installed executables.

The selected executable is persisted in Cognac's application registry so future launches do not require the user to know the Wine prefix or installed Windows path.

VM-backed installations synchronize the executable inventory from the Windows guest through the QEMU Guest Agent.

## Desktop integration

Successful applications get a freedesktop desktop entry automatically.

The generated launcher calls Cognac rather than exposing the compatibility runtime directly:

```text
Exec=cognac run <app-id>
```

That means Cognac remains responsible for reconstructing the correct runner, environment variables, prefix, and execution backend when the application is opened later.

## Managed environments

Cognac keeps its state under the user's standard XDG directories.

Conceptually:

```text
~/.local/share/cognac/
  environments/
  runners/
  tools/
  icons/
  logs/
  snapshots/
  registry.json
  strategies.json

~/.config/cognac/
  profiles.json
  windows-vm.json
```

Each execution strategy gets an isolated environment.

A failed Wine attempt therefore does not contaminate a Proton attempt, and a stable Wine fallback does not reuse a staging prefix.

## Strategy memory

Cognac remembers execution strategies that have worked previously.

Success is keyed both by executable hash and by application identity, allowing a later version of the same installer to benefit from a strategy that worked before.

A known-good strategy receives a strong preference during future planning.

The objective is for Cognac to become less experimental as it learns which execution paths consistently work for an application.

## Windows VM execution

Some Windows applications depend on behavior that Wine and Proton fundamentally cannot provide, such as genuine Windows kernel-mode drivers.

Cognac can detect this class of requirement and route the application toward a Windows VM backend.

The current VM backend integrates with libvirt/QEMU and uses the QEMU Guest Agent to:

- start the managed Windows guest
- stage installer files inside Windows
- execute Windows processes
- capture process results
- synchronize installed executable inventory
- create and restore libvirt snapshots

Cognac also probes the VM for features such as:

- EFI firmware
- Secure Boot
- virtual TPM 2.0
- QEMU Guest Agent

The user is responsible for a valid Windows license.

Automatic creation and provisioning of the Windows guest is still under development.

Cognac does not attempt to disguise virtualization from software that explicitly rejects virtual machines.

## Progress and ETA

Progress reporting is intentionally coarse.

Compatibility operations can involve downloads, Wine initialization, installers, updater processes, and runtime repair. Exact completion times are often unknowable.

Cognac therefore prefers meaningful stages and approximate progress over fake precision.

## Built with Rust

Cognac's core is written in Rust.

The project uses Rust for:

- PE parsing
- compatibility planning
- host detection
- runner management
- process execution
- diagnostics
- rollback orchestration
- application registry management
- desktop integration

External compatibility tools are invoked only where reimplementing them would provide no meaningful benefit.

## Distribution

Release artifacts currently include:

- Debian/Ubuntu `.deb`
- Fedora/RHEL-style `.rpm`
- Arch Linux package
- AppImage
- generic tarball

Release files are published with SHA-256 checksums.

## Testing

Useful compatibility tests range from simple applications to increasingly difficult installers:

```text
7-Zip
Notepad++
SumatraPDF
VLC
WinSCP
32-bit legacy applications
.NET-heavy applications
DirectX games
multi-stage launchers/updaters
```

A clean installation should ideally satisfy all of the following:

```text
cognac installer.exe
→ installer runs
→ dependencies are supplied automatically
→ background updater completes
→ installed application is detected
→ launcher is created
→ application opens from the Linux desktop
→ subsequent launches need no Wine knowledge
```

## Project status

Cognac is under active development.

The core architecture is usable, but compatibility coverage and automatic host provisioning are still evolving rapidly.

Expect edge cases, especially with:

- unusual DRM
- kernel-dependent security software
- hardware-specific Windows utilities
- applications requiring complex Windows services
- vendor-specific launchers
- applications that explicitly reject virtualization

Diagnostic reports and reproducible compatibility failures are useful contributions.

## Philosophy

Cognac is built around one rule:

> If a Windows application can reasonably be made to run on Linux, Cognac should figure out how.

The user should not have to become a Wine expert first.

---

**Cognac — Windows software. Linux machine. No babysitting required.**

## Project documents

- [Contributing](CONTRIBUTING.md)
- [Contributors](CONTRIBUTORS.md)
- [Security policy](SECURITY.md)
- [Third-party notices](THIRD_PARTY_NOTICES.md)
- [Disclaimer](DISCLAIMER.md)
- [Code of Conduct](CODE_OF_CONDUCT.md)
