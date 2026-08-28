# Security Policy

Cognac downloads and executes compatibility runtimes, launches untrusted Windows applications, manages isolated environments, and can integrate with host virtualization. Security reports are therefore treated as high priority.

## Supported versions

Security fixes are targeted at:

- the latest tagged release
- the current `master` branch

Older releases may receive fixes when practical, but users should update to the latest release after a security fix is published.

## Reporting a vulnerability

Please do not open a public issue for a vulnerability that could put users, host systems, credentials, application data, or downloaded artifacts at risk.

Preferred reporting order:

1. Use GitHub private vulnerability reporting / a private security advisory for this repository when that option is available.
2. If a private GitHub reporting channel is unavailable, contact the maintainer privately through the contact methods listed on the maintainer's GitHub profile and include `Cognac security` in the subject or first line.

Include enough information to reproduce and assess the issue:

- affected Cognac version or commit
- Linux distribution and architecture
- execution class involved (`Wine`, `Proton / UMU`, `Windows VM`, etc.)
- exact steps to reproduce
- expected and observed behavior
- relevant logs with secrets, tokens, usernames, paths, and personal data removed where possible
- impact assessment
- a minimal proof of concept when safe to provide

Do not include malware samples or sensitive user data unless specifically requested through a private channel.

## Security-sensitive areas

Reports are especially useful for issues involving:

- checksum or signature verification of downloaded runtimes/tools
- archive extraction and path traversal
- command or argument injection
- environment-variable injection
- unsafe handling of executable metadata
- privilege escalation or package-manager invocation
- symlink or filesystem boundary violations
- Cognac deleting or modifying files outside managed directories
- Wine/Proton prefix escape assumptions
- VM guest/host boundary mistakes
- QEMU Guest Agent command handling
- libvirt domain or snapshot handling
- insecure temporary-file handling
- desktop-entry command injection
- unsafe persistence of credentials or secrets
- rollback/snapshot corruption
- update-channel compromise

## Downloaded components

Cognac should verify downloaded components before publishing them into a managed runtime. A verification failure must fail closed: the unverified artifact must not become the active runtime/tool.

If you discover a way to bypass Cognac's checksum or integrity checks, report it privately.

## Untrusted Windows applications

Running an executable through Cognac does not make the executable safe. Wine and Proton are compatibility layers, not security sandboxes. Virtual machines and containers reduce or change attack surfaces but should not be treated as perfect containment.

A report that demonstrates an unsafe assumption in Cognac's isolation model is in scope even when the original Windows application is malicious.

## Security boundaries

Cognac is a compatibility and orchestration project. Security features must not be implemented by silently weakening host security, bypassing application access controls, defeating anti-cheat or DRM systems, falsifying attestation, or concealing security-relevant virtualization state from software that explicitly evaluates it.

Experimental compatibility work should preserve this boundary.

## Disclosure

Please allow reasonable time for triage, remediation, testing, release preparation, and downstream notification before public disclosure. The project will aim to credit reporters unless they prefer to remain anonymous.

## Dependencies

Cognac depends on Rust crates and external compatibility/runtime projects. A vulnerability in an upstream project should generally be reported upstream as well when appropriate. Cognac-specific exposure, insecure configuration, unsafe orchestration, or delayed mitigation remains in scope for this repository.
