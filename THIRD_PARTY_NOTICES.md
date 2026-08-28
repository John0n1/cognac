# Third-Party Notices

Cognac is MIT-licensed software, but it interoperates with and may download, invoke, or manage third-party compatibility/runtime components. Those projects are not relicensed by Cognac and remain subject to their own licenses and notices.

This document is informational and is not a substitute for the license files shipped by an upstream project or runtime.

## Wine

Cognac can download managed Wine builds and can fall back to a system Wine installation.

- Project: Wine
- Upstream: https://www.winehq.org/
- Source: https://gitlab.winehq.org/wine/wine
- Primary project license: GNU Lesser General Public License v2.1 or later

Managed binary builds used by Cognac may be obtained from the `Kron4ek/Wine-Builds` release project. Those builds package Wine and related runtime files; the applicable upstream component licenses continue to apply.

- Build project: https://github.com/Kron4ek/Wine-Builds

Cognac does not claim ownership of Wine or the managed Wine builds.

## Winetricks

Cognac can download a pinned Winetricks script when Windows compatibility components are required.

- Project: Winetricks
- Source: https://github.com/Winetricks/winetricks
- License: GNU Lesser General Public License v2.1

Cognac verifies the pinned Winetricks artifact before making it available as a managed tool.

## UMU Launcher

Cognac can use a system `umu-run` installation or download a managed UMU zipapp for Proton-oriented execution.

- Project: umu-launcher
- Source: https://github.com/Open-Wine-Components/umu-launcher
- License: GNU General Public License v3.0

UMU may itself use or download the Steam Runtime, Proton-family runtimes, and additional third-party components. Those components have their own licenses and notices. Consult the UMU and runtime distributions for the complete license set.

## Proton and Proton-family runtimes

Cognac can interoperate with Proton-family runtimes through UMU.

- Project: Proton
- Source: https://github.com/ValveSoftware/Proton
- Proton-specific top-level code: BSD 3-Clause
- Distribution: contains numerous components under separate licenses

A complete Proton runtime is not covered by a single license. Wine, DXVK, VKD3D-Proton, runtime libraries, and other included projects retain their respective licenses. Refer to the `LICENSE`, `LICENSE.proton`, `dist.LICENSE`, and component license files distributed by the selected Proton build.

Third-party Proton variants may contain additional patches or components and may have additional attribution requirements.

## QEMU

Cognac's Windows VM backend can invoke a host-provided QEMU installation through libvirt.

- Project: QEMU
- Source: https://gitlab.com/qemu-project/qemu
- Emulator as a whole: GNU General Public License v2
- Individual components/files may use compatible licenses as documented upstream

QEMU is normally supplied by the user's Linux distribution; Cognac does not currently bundle QEMU.

## libvirt

Cognac uses `virsh`/libvirt to inspect, start, snapshot, restore, and communicate with configured Windows virtual machines.

- Project: libvirt
- Upstream: https://libvirt.org/
- Source: https://gitlab.com/libvirt/libvirt
- Core library: GNU Lesser General Public License v2.1 or later
- Some non-library components use GNU General Public License v2 or later

libvirt is normally supplied by the user's Linux distribution; Cognac does not currently bundle libvirt.

## OVMF / edk2

Cognac detects OVMF/edk2 firmware for EFI and Secure Boot-capable virtual machines.

- Project: TianoCore EDK II / OVMF
- Source: https://github.com/tianocore/edk2

EDK II contains files under multiple permissive open-source licenses. Refer to the license and notice files shipped by the user's firmware package.

Cognac does not currently bundle OVMF firmware.

## swtpm

Cognac detects `swtpm` for virtual TPM 2.0 support in managed Windows VM configurations.

- Project: swtpm
- Source: https://github.com/stefanberger/swtpm

`swtpm` and its dependencies retain their upstream licenses and are normally supplied by the Linux distribution. Cognac does not currently bundle swtpm.

## Rust dependencies

Cognac is built using third-party Rust crates listed in `Cargo.toml` and locked in `Cargo.lock`.

Each crate retains its own copyright and license. The exact dependency graph can vary by Cognac version. Packagers and redistributors should review the dependency metadata for the release being distributed and preserve any notices required by those licenses.

Useful tooling for release audits includes `cargo metadata`, `cargo tree`, and license-audit tools such as `cargo-about` or `cargo-deny`.

## Linux distribution packages

When Cognac detects or later installs host packages through a Linux package manager, those packages remain governed by the distribution and upstream project licenses. Installing or invoking a system package does not make it part of Cognac's MIT-licensed source code.

## Windows and Windows applications

Microsoft Windows and applications installed through Cognac are third-party proprietary or open-source products governed by their respective licenses and terms.

Cognac does not provide a Windows license, application license, or rights to third-party software merely because it can install or launch that software.

## Trademarks

Wine, Proton, Steam, Windows, Microsoft, QEMU, libvirt, and other names referenced here may be trademarks of their respective owners. Their appearance in Cognac documentation identifies compatibility or integration targets and does not imply endorsement or affiliation.

## Redistribution

If a Cognac package or release begins bundling a third-party runtime rather than downloading or invoking it separately, the packager must review that runtime's redistribution requirements and include the corresponding license text, source offer/source availability information, copyright notices, and other required materials.

If this document conflicts with an upstream license, the upstream license controls for that upstream software.
