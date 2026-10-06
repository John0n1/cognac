# Hardware-aware package analysis and native operations

Cognac can analyze hardware dependencies across manufacturers. It does not need
a Lenovo filename, BIOS version, firmware GUID or fixed resource offset. The
reusable workflow is: inspect a Windows package, recover its driver/interface
boundary, identify the physical device, then use a verified native implementation
of the intended operation. Translating an arbitrary Windows kernel driver into
Linux code or replaying unknown register writes is not implemented.

## Commands

```sh
# Read-only; reports JSON. No supplied code runs.
cognac inspect-hardware updater.exe --json

# Include files extracted by an external extractor or inspect-firmware.
cognac inspect-hardware updater.exe --payload-dir ./extracted --json

# Export PE resources under SHA-256 filenames into a new directory.
cognac inspect-hardware flash-tool.exe --export-embedded ./new-drivers

# Decode a number already recovered from disassembly or documentation.
cognac decode-ioctl 0x8337EAF2

# Review a native firmware operation for an explicitly selected device/version.
cognac firmware-devices --json
cognac hardware-plan updater.exe --payload-dir ./extracted \
  --device EXACT_DEVICE_ID --version VERSION

# Submit the reviewed trusted native release and verify afterwards.
cognac flash-firmware --device EXACT_DEVICE_ID --version VERSION --yes
cognac firmware-status --device EXACT_DEVICE_ID
```

`hardware-plan` only queries the native fwupd backend. Its `payload_source` and
`package_payload_binding_verified: false` state that the selected remote release
is independent of the analyzed EXE. It never schedules a flash. Actual submission
uses the existing journaled, explicitly targeted `flash-firmware` command; fwupd
validates the release/device and performs the privileged operation. This works
across firmware/device manufacturers supported by fwupd. It cannot invent an
available release for unsupported hardware.

`inspect-firmware` currently extracts Inno Setup packages using `innoextract`.
For other package formats, supply a previously extracted directory. The new
analysis handles EXE, DLL and SYS images independently of the extractor.

## Evidence and planning

Each image records its hash, machine/subsystem, exact imported symbols, exports,
embedded PE images and resource offsets. Native-subsystem images importing a
Windows kernel library are identified as kernel images. If COFF function symbols
are retained, the report includes names, RVAs and file offsets to guide subsequent
disassembly. Device-name hints are recovered from bounded ASCII/UTF-16 strings;
these hints are not proof of a device match.

The capability catalog distinguishes driver loading, Windows IOCTL transport,
physical-memory mapping, port I/O, firmware variables, WinUSB and HID. Evidence
comes from imports, including decorated 32-bit symbols. A service API or generic
file API alone does not prove a hardware dependency. DeviceIoControl alone does
not prove firmware writing; it also serves ordinary file/device operations.

The ordinary installation analyzer now examines embedded kernel imports too.
A package importing physical-memory or port-I/O operations is restricted until a
verified native adapter exists. Learned Wine/VM preferences cannot override that
restriction. Ordinary guest driver packages retain their existing VM route when
they do not expose these dependencies. This conservative rule may block packages
that bundle an unused driver; inspect the relevant user-space component separately.

The reported requirements describe which backend could serve an operation and
what remains unsupported. They are not automatically generated executable
adapters. USB/HID transports and Windows driver IOCTL translation still require
protocol-specific implementations.

## Bounds and uncertainty

The resource walker checks file-backed RVA ranges, lengths, directory cycles,
maximum depth, entry counts and cumulative payload size. It exports whole resource
payloads, not guessed fragments from a byte-pattern scan. Duplicate resource
ranges/hashes are deduplicated. Export filenames are content hashes, not paths
controlled by an executable, and existing destinations are refused. Analysis
limits are 128 MiB per PE, 128 external/unique exported PE images, 512 MiB of
external images or resource payload work, 1,024 resource entries and depth eight.
Symlink payload entries are skipped and reported. No driver, installer script or
supplied executable runs during analysis.

`incomplete` reports analysis errors, malformed resources or skipped symlinks.
Even when false, coverage is limited to static imports, retained symbols and
uncompressed PE resource payloads. Dynamically resolved APIs, inlined port
instructions, packed installers, compressed driver resources and indirect child
processes can evade this analysis. PE certificate presence and hashes do not
establish vendor authenticity. This is a reverse-engineering aid, not a sandbox
or proof that an unknown executable is safe to run.

## Adding an actual protocol adapter

A new adapter must identify supported devices and firmware interfaces using
validated physical identity and ABI data, specify authenticated payload/layout
and rollback rules, check memory/transfer bounds and power conditions, keep an
exclusive persistent update journal, avoid retries after uncertain writes and
verify device state after completion/reboot. Mock ABI/error-path tests and real
hardware validation are both needed before claiming support. Unknown IOCTLs,
SMI ports and opcodes must remain unsupported. A Linux adapter handles verified
operations; merely returning success to a Windows updater cannot do that work.

The Phoenix SCT EFI experiment in `tools/phoenix-info` demonstrates this process
on one exact validated platform and capsule. Its physical-memory tables, image
pin and command ABI must not be reused on unrelated firmware. The portable
analysis, fwupd backend and transaction machinery are the reusable parts today.

References: [Microsoft IOCTL definition](https://learn.microsoft.com/en-us/windows-hardware/drivers/kernel/defining-i-o-control-codes),
[DeviceIoControl](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-deviceiocontrol),
[fwupd UEFI capsule backend](https://github.com/fwupd/fwupd/blob/main/plugins/uefi-capsule/README.md).
