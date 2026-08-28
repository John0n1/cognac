# Contributing to Cognac

Contributions are welcome. Cognac sits at the boundary between Linux host management, Windows compatibility, process orchestration, and virtualization, so changes should be small enough to review and explicit about the assumptions they introduce.

## Ways to contribute

Useful contributions include:

- compatibility fixes and new diagnostics
- executable-analysis improvements
- host capability detection
- Wine/Proton/UMU strategy improvements
- safe automatic repair rules
- VM backend and guest-integration improvements
- distro/package-manager support
- installer and child-process detection
- desktop integration
- tests and reproducible compatibility reports
- documentation and packaging

## Development setup

Cognac is written in Rust.

```bash
cargo build
cargo test
```

Before submitting a pull request, run:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --release
```

If a change depends on tools or hardware that CI cannot provide, state exactly what you tested manually.

## Design principles

Changes should preserve the core Cognac model:

```text
observe -> diagnose -> mutate -> retry
```

Prefer capability-based decisions over distribution-specific assumptions. Prefer execution-class selection over hard-coding Wine as the answer. Prefer isolated, reversible changes over mutating an existing environment in place.

Cognac should degrade gracefully when an optional capability is missing and fail clearly when continuing would be unsafe or misleading.

## Execution classes

Current execution classes include:

- Wine
- Proton / UMU
- containerized Wine
- Windows VM
- restricted / unsupported

A new backend should fit the common execution abstraction where possible rather than bypassing the planner and installer orchestration.

## Compatibility rules

Avoid application-name-only hacks when a generic capability or failure signature can solve the same class of problem.

Good:

```text
missing vcruntime DLL -> install VC++ runtime -> retry
```

Less desirable:

```text
if application name == X -> run arbitrary shell command
```

Known-application profiles are appropriate when the behavior genuinely cannot be inferred generically.

## Host changes and privilege

Host-level changes must be explicit, minimal, and auditable.

Do not:

- silently disable host security features
- edit unrelated system configuration
- remove user files outside Cognac-managed paths
- invoke a privileged package manager through an unescaped shell string
- assume `sudo` is available or appropriate

Where privileged configuration is required, separate planning from execution and show what Cognac intends to change.

## Downloads and supply-chain safety

Downloaded executables, scripts, runtimes, archives, and metadata must be treated as untrusted until verified.

When possible:

- pin a version or resolve from a trusted upstream release API
- verify a published checksum or signature
- download to a temporary path
- validate expected file/layout properties
- publish atomically only after verification
- retain the currently working runtime if an update fails

Do not add an unverified `curl | sh` style path.

## Filesystem safety

Cognac must not delete or overwrite arbitrary user paths.

Before destructive operations, verify that the target is within a Cognac-managed directory. Be careful with symlinks, canonicalization, archive extraction, temporary directories, and path components supplied by Windows metadata.

## VM and security-sensitive compatibility

VM work should focus on legitimate compatibility: provisioning, Secure Boot/TPM configuration, guest tooling, graphics, audio, input, snapshots, reboot/resume, and application integration.

Do not submit features whose purpose is to defeat anti-cheat, DRM, attestation, licensing, access controls, or virtualization-detection security boundaries.

## Tests

Add regression tests whenever practical.

Particularly valuable tests cover:

- analyzer classification
- planner strategy selection
- failure classification
- rollback behavior
- application discovery
- path validation
- malformed input
- interrupted installs
- strategy memory
- security boundaries

For compatibility reports, include the application version, installer source/hash when appropriate, distro, GPU/driver, selected execution class, Cognac version, and sanitized logs.

## Pull requests

Keep pull requests focused. A good PR description explains:

- what problem it solves
- why the chosen layer is the right place to solve it
- how behavior changes
- how it was tested
- any security or rollback implications

Large architectural changes should include tests or a reproducible validation plan.

## Commit messages

Use concise imperative messages, for example:

```text
analyzer: detect Burn bootstrapper metadata
planner: prefer UMU for game-class executables
vm: validate guest-agent channel before launch
```

## Documentation

Update documentation when user-visible behavior, supported execution classes, configuration, security boundaries, or packaging changes.

## License

By contributing, you agree that your contributions may be distributed under the repository's MIT License unless explicitly agreed otherwise before submission.
