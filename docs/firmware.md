# Host firmware support

## Current behavior

BIOS/UEFI update utilities can install their Windows wrapper successfully while
their firmware component fails. Cognac identifies host firmware updaters from
product metadata and specific Phoenix/Insyde/AMI flash-tool markers. Detection is
heuristic: an unrecognized or opaque package can evade it, and it is not a security
sandbox. Firmware is a separate application class, with no Wine, Proton, or VM
fallback. User profiles and learned runner preferences cannot override that route.
Detected flashers are also rejected when launching registered apps, recovering
interrupted installations, or probing newly discovered application payloads.

`cognac inspect-firmware update.exe --extract-to ./new-directory --json` invokes
`innoextract`, never the supplied executable. It stages extraction in a temporary
directory, refuses to overwrite an existing destination, then reports:

- Original executable metadata, SHA-256, and firmware classification.
- Nested EXE metadata and classifications; errors are retained in the report.
- CAP files' SHA-256, byte size, mixed-endian capsule GUID, flags, header size,
  declared size, and whether header bounds agree with the file size.
- Linux DMI vendor, model, BIOS version/date and accessible ESRT information.

Missing ESRT access is reported explicitly. Header checks and hashes do not verify
the vendor's signature or establish model compatibility. A matching ESRT GUID is
identification evidence; a different GUID can describe a vendor container.
Extraction currently supports Inno Setup packages, not arbitrary installer formats.

The exact message `TDK library initialization failed` is classified as terminal
when present in captured output, including when the wrapper returns zero. The
message does not establish which library, service, or hardware operation failed.
GUI-only errors are not captured by the current observer; dialog/accessibility
telemetry would be a separate improvement. Existing process and exit-code checks
do not prove that a BIOS changed. Installed-app quality records are not firmware
update verification.

## What executing a vendor updater would require

A generic Windows guest runs against virtual firmware. Its snapshot does not
back up the physical laptop's flash chip. PCI or USB passthrough alone does not
provide the host platform's firmware-update protocol.

Keeping the Windows EXE would require understanding its specific interfaces:
library dependencies, Windows service/driver loading, DeviceIoControl commands,
and any privileged physical-memory, I/O, SMI, or firmware-service operations it
actually uses. Cognac would need a tested Linux implementation and translation
bridge for those operations on supported hardware. Stubbing successful return
values would hide failures and cannot supply the required hardware semantics.
Running the EXE as root is not an implementation of these interfaces.

A more maintainable direction is to extract the vendor package and hand off to a
model-compatible native backend, such as fwupd where that exact device and payload
are supported, or an OEM bootable EFI updater. A production backend would need:

1. Explicit model/board and version matching, authenticated payloads, and
   anti-rollback policy; hashes alone are not vendor authentication.
2. Capability checks for the actual capsule submission method, firmware-reported
   maximum capsule size, and reset requirements. Do not infer support merely from
   an ESRT entry or a successful capability query status.
3. A narrowly privileged service, exclusive update coordination, power checks,
   and a persistent transaction recording staging and reboot state.
4. Postboot verification using physical BIOS version and firmware result records.
   Wrapper exit status is not sufficient.
5. A tested recovery method appropriate to that motherboard. User-space files and
   VM snapshots do not restore a damaged physical flash chip.

## Native fwupd backend

Cognac now supports physical-device updates through the system fwupd service:

```sh
cognac firmware-devices --json
cognac flash-firmware --device EXACT_DEVICE_ID --version VERSION --dry-run
cognac flash-firmware --device EXACT_DEVICE_ID --version VERSION --yes
cognac firmware-status --device EXACT_DEVICE_ID
```

The selected version must be an available release for that exact device, with
trusted metadata, a remote identifier and a SHA-256 checksum. Blocked releases,
raw files and ambiguous matches are rejected. fwupd performs the privileged
submission and its own payload/power/security checks. Cognac never uses force,
rollback, raw capsule submission, or automatic reboot options. This requires a
system fwupdmgr supporting JSON output and `install DEVICE VERSION`.

An exclusive process lock and persistent journal record intent before submission.
An uncertain submission is never automatically retried. Postboot checks require
matching host/device identity and target version; failed fwupd update state
prevents verification. Pending reboot is not reported as update success.

This portable backend does not submit arbitrary Phoenix SCT CAP files. The
investigated Lenovo device had no available system-firmware release through
fwupd. Its BIOS was subsequently updated and verified using the separate,
exact-platform Phoenix EFI experiment. That success does not establish support
for other firmware versions, platforms or payloads. See
[the Phoenix investigation](phoenix-sct.md) and
[the vendor-independent hardware workflow](hardware.md).

`cognac probe-phoenix-firmware --tables-dir DIRECTORY --json` validates readable
UEFI/BATB/FACP ACPI snapshots and reports the advertised Phoenix SMI interface.
The default directory is `/sys/firmware/acpi/tables`, which may require root to
read. It never maps memory, writes EFI variables, or issues an SMI. Interface
presence does not establish model/capsule compatibility or update readiness.

## Standards reference

Microsoft's [UEFI firmware update platform](https://learn.microsoft.com/en-us/windows-hardware/drivers/bringup/windows-uefi-firmware-update-platform)
documents the platform firmware update route using UEFI UpdateCapsule.
Its [processing updates documentation](https://learn.microsoft.com/en-us/windows-hardware/drivers/bringup/processing-updates)
explains the Windows loader's ESRT-based capsule construction. These describe one
supported architecture; they do not prove that a particular vendor EXE uses it.
