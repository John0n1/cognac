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
  Cognac is an intelligent compatibility and installation layer for Windows <code>.exe</code> applications on Linux.
</p>

---

## The idea

The intended experience is simple:

```bash
cognac something.exe
```

Cognac handles the rest.

No manual Wine prefixes.<br>
No hunting for missing DLLs.<br>
No guessing which Wine or Proton build to use.<br>
No Winetricks rabbit hole.<br>
No endless terminal output.

If a Windows application can reasonably be made to run on Linux, Cognac should figure out how.

---

## What Cognac does

Give Cognac an executable:

```bash
cognac VLCSetup.exe
```

Cognac automatically works through the installation process:

```text
Executable
    |
    v
Analyze application
    |
    v
Inspect Linux host
    |
    v
Resolve compatibility requirements
    |
    v
Provision runners and dependencies
    |
    v
Create isolated environment
    |
    v
Install application
    |
    v
Observe and repair failures
    |
    v
Detect installed executable
    |
    v
Create Linux launcher
    |
    v
Done
```

Internally, Cognac can:

- inspect the executable
- detect 32-bit and 64-bit applications
- identify installer types
- inspect application metadata
- detect missing Linux dependencies
- obtain and manage Wine and Proton-compatible runners
- install required Windows runtimes
- configure DXVK and VKD3D where appropriate
- handle architecture requirements
- create isolated application environments
- monitor installation behavior
- detect common compatibility failures
- automatically apply fixes
- retry using alternative strategies
- detect the installed application
- extract its icon
- create desktop integration
- preserve detailed logs without flooding the terminal

The user should rarely need to know any of this happened.

---

## Usage

The primary interface is intentionally small.

```bash
cognac something.exe
```

Installed applications can be managed through Cognac:

```bash
cognac list
cognac run <app>
cognac info <app>
cognac repair <app>
cognac remove <app>
cognac logs <app>
cognac doctor
cognac update
```

Advanced debugging options may expose additional information, but they should not be required during normal use.

---

## Quiet by default

Cognac should avoid turning installation into a wall of Wine output.

The normal interface should stay compact and update in place:

```text
Cognac

Installing VLC

███████████████████░░░░░ 78%

Teaching DirectX to speak Vulkan...
~20 seconds remaining
```

When complete:

```text
VLC installed
Application launcher created
Ready
```

Raw logs are preserved and can be inspected when needed:

```bash
cognac logs VLC
```

Verbose and debugging modes can expose the underlying runner output for troubleshooting.

---

## Cognac makes the decisions

Traditional Wine workflows often require the user to decide:

```text
Which Wine version?
Which prefix architecture?
Wine or Proton?
Install DXVK?
Install .NET?
Install Visual C++?
Which Windows version?
Install Mono?
Which DLL overrides?
```

Cognac treats these as implementation details.

It analyzes the executable and host system, builds an internal compatibility plan, and chooses automatically.

If the first strategy fails, Cognac should attempt another reasonable strategy instead of immediately returning the problem to the user.

---

## Self-bootstrapping

Wine should not need to be installed before Cognac.

A fresh system should eventually be able to go directly from:

```bash
cognac SomeApplication.exe
```

to a running application.

Cognac can obtain and manage compatibility components such as:

- Wine
- Wine Staging
- Wine-GE
- Proton-compatible runners
- UMU-based environments
- DXVK
- VKD3D-Proton
- Wine Mono
- Wine Gecko
- Visual C++ runtimes
- .NET Framework components
- DirectX components
- Media Foundation components
- other commonly required Windows dependencies

Wine and Proton are backends.

They are not the user interface.

---

## Linux dependency resolution

Cognac also resolves Linux-side requirements.

For example:

```text
Windows application
        |
        v
Requires 32-bit Vulkan
        |
        v
Inspect Linux distribution
        |
        v
Resolve equivalent host package
        |
        v
Install or provision dependency
        |
        v
Continue
```

The same capability may correspond to different packages depending on the distribution.

Cognac should reason about capabilities instead of requiring application logic to know every package name directly.

Initial package-manager targets include:

```text
pacman
apt
dnf
zypper
```

Additional backends can be added later.

---

## Architecture handling

Architecture differences should remain invisible to the user.

Cognac should automatically handle scenarios including:

```text
32-bit Windows executable
64-bit Windows executable
32-bit installer -> 64-bit application
WoW64
multilib requirements
mixed architecture dependencies
```

If the host requires additional architecture support, Cognac should detect and provision it where possible.

---

## Application analysis

Before executing an application, Cognac can inspect information such as:

- PE architecture
- product metadata
- application name
- version
- publisher
- installer technology
- embedded manifests
- imported DLLs
- .NET metadata
- likely graphics requirements
- known executable fingerprints
- compatibility profiles

This information helps Cognac determine an initial installation strategy before blindly launching the executable.

---

## Compatibility knowledge

Known applications can use reusable compatibility profiles containing information such as:

```text
preferred runner
required runtimes
Windows version
graphics backend
known fixes
DLL overrides
installer quirks
known limitations
```

Unknown applications should still work through generic analysis, probing, and runtime observation.

Application identification should rely on stronger signals than filenames alone.

Useful identifiers can include:

```text
SHA-256
publisher
product metadata
version
installer metadata
executable characteristics
```

Compatibility knowledge should remain separate from the core where practical so new application profiles can be added without rebuilding Cognac itself.

---

## Automatic repair

Failure should not immediately mean installation failure.

Cognac should recognize and react to common problems such as:

- missing DLLs
- missing Visual C++ runtimes
- .NET initialization failures
- Wine Mono conflicts
- graphics initialization errors
- missing Vulkan support
- missing 32-bit host libraries
- incompatible runner versions
- unsuitable Windows version settings
- installer-specific behavior
- missing optional Windows components

The normal recovery flow is:

```text
Failure detected
      |
      v
Classify cause
      |
      v
Apply repair
      |
      v
Retry
```

If necessary:

```text
Repair failed
      |
      v
Restore previous state
      |
      v
Try another strategy
```

This can include switching runners, changing runtime combinations, modifying compatibility settings, or starting from a clean environment.

---

## Graceful degradation

Not every missing feature should stop an installation.

Cognac should distinguish between:

```text
Required
Recommended
Optional
```

and between:

```text
Fatal
Retryable
Degraded
Ignorable
```

A Windows application may depend on a vendor-specific hardware driver for one feature while the rest of the application remains completely usable.

In such a case, Cognac should generally continue rather than treating the entire application as unusable.

Internally, installations can be classified as:

```text
Fully functional
Functional
Functional with limitations
Unverified
Failed
```

The normal user-facing result should remain concise.

---

## Safe retries

Compatibility changes can sometimes make an environment worse.

Cognac should preserve application state before major changes where practical.

```text
Current state
     |
     v
Try compatibility change
     |
     +---- success ----> keep it
     |
     +---- failure ----> rollback
                           |
                           v
                    try alternative
```

This allows the resolver to experiment without permanently destroying a promising configuration.

---

## Installer and process tracking

Many installers do not simply start and exit.

They may:

- spawn child installers
- restart themselves
- launch an updater
- continue setup in another process
- install background components
- relaunch the application
- update immediately after installation

Cognac should track the installation lifecycle rather than assuming that the original `.exe` process represents the entire installation.

This is especially important for bootstrap installers and self-updating applications.

---

## Installed application detection

After installation, Cognac should determine what was actually installed.

Useful signals include:

- new filesystem entries
- Program Files changes
- Start Menu shortcuts
- Desktop shortcuts
- registry uninstall entries
- newly created executables
- installer child processes

From this, Cognac should determine:

```text
Application name
Main executable
Launch arguments
Icon
Environment
Runner
Prefix
```

The result becomes a managed Cognac application.

---

## Desktop integration

Installed Windows applications should feel like normal Linux applications.

Cognac should automatically create:

- desktop entries
- application-menu entries
- icons
- launcher commands
- application metadata

Once installed, the user should be able to launch the application from environments such as:

```text
GNOME
KDE Plasma
XFCE
Cinnamon
other freedesktop-compatible desktops
```

Launching the application should not require knowing anything about its Wine prefix or runner.

---

## Managed environments

Cognac keeps applications and compatibility components organized and isolated.

A Cognac installation may use a structure similar to:

```text
cognac/
├── apps/
├── prefixes/
├── runners/
├── components/
├── compatibility/
├── cache/
├── icons/
├── logs/
└── cognac.db
```

Applications should not depend on manually maintained Wine prefixes scattered throughout the user's home directory.

---

## Progress and ETA

Where practical, Cognac should estimate remaining installation time.

When uncertainty is high, ranges are preferable to fake precision:

```text
~30-45 seconds remaining
```

Estimates can improve as Cognac learns more about:

- download size
- current transfer speed
- prefix initialization
- dependency installation
- compatibility preparation
- installer progress

---

## Status messages

Cognac can occasionally display subtle rotating status messages while working.

Examples:

```text
Aging a fresh Windows vintage...
Fetching grapes...
Teaching DirectX to speak Vulkan...
Convincing Windows it's totally at home...
Polishing the registry...
Collecting tiny pieces of Windows...
Microsoft requires additional Microsoft...
Trying another barrel...
Leaving some Windows baggage behind...
Corking the bottle...
Nobody mention this to Microsoft.
```

They should remain secondary to useful progress information and should never turn the interface into terminal spam.

---

## Built with Rust

Cognac's core is built in **Rust**.

Rust provides a strong foundation for:

- native Linux integration
- process management
- concurrency
- downloads and caching
- filesystem operations
- executable analysis
- structured error handling
- package-manager integration
- long-running compatibility orchestration

Cognac should remain a native Linux application without requiring Python, Node.js, or another scripting runtime.

---

## Distribution

Release formats:

| Format | Target |
|---|---|
| `.deb` | Debian, Ubuntu, Mint, Pop!_OS and derivatives |
| `.rpm` | Fedora, openSUSE, RHEL-family distributions |
| AUR / PKGBUILD | Arch Linux and derivatives |
| AppImage | Portable Linux distribution |
| `.tar.gz` | Generic binary release |

The first release targets:

```text
x86_64
```

Download a package from the [latest GitHub release](https://github.com/John0n1/cognac/releases/latest), then install it with the matching command:

```bash
# Debian, Ubuntu, Mint, Pop!_OS
sudo apt install ./cognac_0.1.0_amd64.deb

# Fedora, openSUSE, RHEL-family
sudo dnf install ./cognac-0.1.0-1.x86_64.rpm

# Arch Linux and derivatives
sudo pacman -U ./cognac-bin-0.1.0-1-x86_64.pkg.tar.zst

# Portable AppImage
chmod +x Cognac-0.1.0-x86_64.AppImage
./Cognac-0.1.0-x86_64.AppImage --help
```

The release also includes an AUR-ready `PKGBUILD`, `.SRCINFO`, a generic binary
archive, and `SHA256SUMS`. To build from source instead:

```bash
cargo build --release --locked
sudo install -Dm755 target/release/cognac /usr/local/bin/cognac
```

---

## Testing

Cognac should be tested against increasingly difficult real-world applications.

A useful progression is:

| Stage | Application type | Purpose |
|---|---|---|
| 1 | VLC | Basic installer and desktop integration |
| 2 | Steam | Bootstrap installer, updater, process tracking |
| 3 | Discord | Chromium/Electron application behavior |
| 4 | WinSCP | Networking and integration |
| 5 | .NET application | Runtime provisioning |
| 6 | Legacy 32-bit application | Architecture and multilib |
| 7 | DirectX application | DXVK/VKD3D and graphics |
| 8 | Games | Input, graphics, audio and runtime complexity |
| 9 | Hardware-integrated software | Graceful degradation and driver handling |

A successful test should mean more than:

```text
installer exited successfully
```

Cognac should verify that the resulting application can be identified, launched, closed, and launched again using the Linux desktop entry it created.

---

## Project status

Cognac is under active development.

Windows applications vary enormously, and perfect compatibility with every application is not realistic.

The goal is not to claim universal compatibility.

The goal is to automatically solve as much compatibility work as reasonably possible and avoid exposing that complexity to the user.

---

## Philosophy

Cognac should not feel like a Wine frontend.

It should feel like a Windows application installer for Linux.

Users should not need to understand:

```text
Wine
Proton
prefixes
Winetricks
DXVK
VKD3D
DLL overrides
Wine architecture
Windows runtimes
runner versions
```

Those are Cognac's problems.

The intended experience remains:

```text
cognac something.exe
        |
        v
     install
        |
        v
     launch
        |
        v
      done
```

> **If a Windows application can reasonably be made to run on Linux, Cognac should figure out how.**

<p align="center">
  <strong>Cognac — Windows software. Linux machine. No babysitting required.</strong>
</p>
