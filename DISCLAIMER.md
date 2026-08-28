# Disclaimer

Cognac is an independent open-source compatibility and orchestration project for running Windows applications on Linux.

## No affiliation

Cognac is not affiliated with, endorsed by, sponsored by, or supported by Microsoft, Valve, CodeWeavers, WineHQ, the UMU project, Winetricks, QEMU, libvirt, game publishers, anti-cheat vendors, or other third-party projects and vendors referenced by Cognac.

All product names, trademarks, service names, logos, and brands belong to their respective owners.

## Compatibility is not guaranteed

Windows applications vary widely in their dependencies, installers, drivers, DRM, anti-cheat systems, hardware assumptions, and licensing requirements. Cognac can analyze, configure, retry, and select alternative execution strategies, but it cannot guarantee that a particular application will install, launch, remain functional, or continue to work after an application, runtime, driver, operating-system, or vendor update.

A successful installation does not imply that every application feature is functional or supported.

## Third-party software

Cognac may download, invoke, configure, or interoperate with third-party software such as Wine builds, Winetricks, UMU, Proton-family runtimes, QEMU, and libvirt. Those projects remain governed by their own licenses, terms, security policies, and support policies.

Cognac does not grant rights to third-party software, Windows, or applications installed through Cognac. Users are responsible for complying with all applicable licenses and terms.

See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for additional information.

## Windows licensing

Use of a Windows virtual machine requires a valid Windows license where one is required by Microsoft or applicable law. Cognac does not provide, activate, bypass activation for, or grant a license to Windows.

## Privileged and system changes

Some current or future Cognac functionality may require host-level changes such as installing packages, configuring virtualization, creating virtual machines, or modifying managed compatibility environments. These operations can affect system stability, storage usage, networking, graphics configuration, or application data.

Cognac should only modify resources it explicitly manages, but users should maintain backups of important data and review privileged operations before authorizing them.

## Untrusted executables

Cognac can execute Windows binaries supplied by the user. Running an executable through Wine, Proton, a container, or a virtual machine does not make that executable trustworthy.

Do not run software you would not otherwise trust. Compatibility environments and virtual machines reduce or change attack surfaces; they are not a guarantee of containment.

## Security and anti-cheat systems

Cognac may detect security-sensitive application requirements and choose an appropriate execution class. It does not promise compatibility with DRM, anti-cheat, attestation, kernel-security, or virtualization-detection systems.

Cognac is intended to provide compatibility and orchestration, not to defeat access controls, licensing systems, anti-cheat protections, or security boundaries.

## No warranty

Cognac is distributed under the MIT License and is provided "AS IS", without warranty of any kind. The terms of the repository's [LICENSE](LICENSE) govern distribution of Cognac itself.

This document explains project expectations and does not replace the license or constitute legal advice.
