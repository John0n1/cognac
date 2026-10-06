# Lenovo FXCN49WW / Phoenix SCT investigation

This is a partial reverse engineering result, not a completed raw-CAP flasher.
The examined installer SHA-256 is
`24386e3f50138483fada6c15da3d72b38befd7b3f96b4b14b21b4b5549be15e9`.
Its CAP is 26,186,240 bytes; SHA-256
`8f5c935fd7711e8cdb087bb34bf5c728ed556e8786652492dbede6714c84dc20`.
Do not distribute the extracted proprietary executable/driver binaries with Cognac.

## Observed process

The Inno Setup install.bat starts SctWinFlash64.exe with
`/file *.cap /sd /sn /cac /cbp 30 /shutdown /silent /bbl /cvar /nodelay`.
SctWinFlash64 has CreateProcessW, process/stdin/stdout progress strings, and an
internal `-phoenix12345` command suffix. It supervises a separate flash-tool
process; it does not itself import DeviceIoControl or the service creation APIs.
The bundled WinFlash64.exe retains COFF symbols, allowing the following native
library flow to be traced without executing the updater:

| Function / VA | Observed behavior |
| --- | --- |
| LibInitial / 0x427bdf | Platform initialization, SMI discovery, service-channel selection |
| PlatformInitial / 0x439870 | Privilege checks and driver initialization |
| DriverOperate / 0x43b6a0 | Load driver or send driver control request |
| SendIoControlCode / 0x4302c8 | Input byte transform, DeviceIoControl, output inverse transform |
| SmiFindPort / 0x439b69 | Read FADT SMI port and vendor ACPI GUID/SMI/shared-memory tables |
| ServiceChannelSmiRequest / 0x427ff8 | Write <=512 request bytes to shared physical memory, outb command to SMI port, read response |
| BiosCapsuleFileUpload / 0x40656b | Flash-write enable, BIOS write/staging, then disable |
| SctWriteSwitch / 0x433410 | Firmware service command 9, 113-byte request; firmware status checked |
| TdkBiosRomWrite / 0x40cbc6 | Select update method from firmware capabilities and capsule properties |
| HddCapsuleUpdateSetup / 0x41ccf0 | Validate/stage capsule on disk and create vendor handoff variables |
| CapsuleValidationByService / 0x42568a | Firmware capsule signature/validation service calls |
| ServiceGetInfo / 0x427550 | 104-byte vendor service-information request |

Four embedded Windows driver resources were found. The newest TdkLib64.sys
resource imports privileged physical-memory allocation/mapping APIs and implements
I/O-port operations. This hardware communication is not supplied by a Wine flag.
The GUI's initialization failure alone does not establish exactly which of the
native initialization steps failed under Cognac.

## Disk handoff branch

For capsules carrying flag 0x10000, TdkBiosRomWrite tries the vendor HDD method
under its supported flash mode, then handles several fallback/error outcomes.
This is a conditional branch, not proof that this exact running BIOS accepts it.
HddCapsuleUpdateSetup requires a >=64-byte service-info structure and additional
library/platform attributes. Validation and flash enable must still succeed.

The ESP branch writes BIOS.cap. SaveBiosImageToEsp selects EFI/UpdateCapsule when
the standard file-capsule OsIndications capability is present; otherwise it uses
the ESP root. There are also custom partition/FAT branches; they are not supported
by this implementation.

The vendor's CapsuleUpdateDataHDD payload is 160 bytes:

| Offset | Width | Observed field |
| --- | --- | --- |
| 0 | 8 | Raw-disk LBA; zero for the ESP branch |
| 8 | 4 | Media/mode field; zero in the examined ESP branch |
| 12 | 4 | Capsule byte count |
| 16 | 16 | GPT unique partition GUID in EFI byte order |
| 32 | 128 | UTF-16 path buffer; zero in the examined ESP branch |

It uses vendor GUID 711c703f-c285-4b10-a3b0-36ecbd3c8be2 and attributes 7.
CapsuleGuidData contains the original CAP GUID under
79dfd2ed-281f-425f-97a8-7a8830f62f6e. Some paths also write CapsuleUpdateData.
TdkVariableSet uses firmware service communication; it must not be replaced with
blind efivar writes based only on these names/layouts.

This alternative disk handoff is worth investigating separately from generic
UpdateCapsule. The earlier firmware query's 4,507,648-byte maximum applies to that
queried capsule route; it does not prove the vendor disk method is unusable.

## Confirmed on the running FXCN28WW machine

Read-only ACPI UEFI/BATB/FACP snapshots match the binary's GUIDs:

- Phoenix SMI table GUID: 0d9fb197-cefc-4e91-acb1-2535d9e5a844.
- Flash SMI GUID: d8afdc58-6e22-42f8-9966-36ff788c9caf, command **0xe9**.
- FADT SMI command port: **0xb2**.
- Shared-memory GUID: d29563e8-cfe1-4d41-8e54-da4322fede5c.
- Shared-memory base as decoded by the vendor: **0x44cd4000**.

Cognac's read-only parser checks signatures, declared lengths, checksums, GUIDs,
entry bounds/ambiguity, port width and nonzero interface values. It does not infer
flash support from discovery.

ServiceGetInfo's request is 104 bytes: command u64=0x10000 at offset 0,
status u64=0xff at 8, length u64=104 at 16, identification GUID
b1de44fc-7946-4982-9b4b-2f8ca45ea792 at 24, and 64 output bytes at 40.
The request uses that GUID and length. Observed FXCN28WW responses return a
112-byte packet, clear the GUID area, and declare a 72-byte information payload.
The decoder accepts only consistent 104/64 or 112/72 packet/payload sizes and
requires the original command and status zero; this is not capsule authentication.
A returned status of 0xff is unprocessed, not success.

## Implemented next experiment

`tools/phoenix-info` is an x86-64 EFI application containing ONLY this GET_INFO
request. It checks the exact captured ACPI snapshots and the UEFI memory map;
the shared region must be reserved or ACPI NVS. It requires an `I` keypress before
writing the 104-byte RAM request and issuing the SMI. There is no flash enable,
flash write/erase, EFI variable update, capsule submission, or reset code.
It saves the response to EFI/CognacPhoenix/info.txt and returns to the boot manager.
It is experimental and has not yet been run against the physical firmware.

Build on Debian with GNU-EFI headers/libraries and binutils:

```sh
tools/phoenix-info/build.sh
```

The exact-table binding is intentionally specific to this captured boot/platform.
If addresses/tables change at reboot, the probe skips the SMI. It must not be
relaxed by dropping checks or substituting addresses by guesswork.

Next: capture the firmware's actual service-info response, decode its supported
methods, then trace and reproduce authentication/validation and staging calls.
Only after those are validated can a model-bound Phoenix write backend be enabled.
Offline tests cover discovery corruption/ambiguity and packet/ESP handoff encoding;
they do not verify physical firmware behavior or establish a recovery method.

## Physical query result

The corrected v3 EFI probe reached its SMI request after all platform checks passed.
Its initial response guard rejected an extended response format. A read-only
postboot capture of the same reserved buffer recovered command 0x10000, status 0,
packet size 0x70, zero response GUID, and information size 0x48. This is evidence
of a completed information request, with the limitation that the capture was
made after boot rather than logged immediately by the EFI probe.

The Rust decoder now handles this observed response, with regression tests for
wrong size, truncation, command, and status. Remaining information fields are kept
raw until their meanings are traced; pointer-looking values and sizes alone do
not establish capsule-buffer ownership or flash readiness. ServiceGetCapsuleBuffer
uses a separate vendor request, command 0x10800.

## Capability queries following GET_INFO

FillGlobalServiceSettings (0x410735) maps returned information offsets 16 and 24
to gCapsuleBufferAddress and gCapsuleBufferSize. The recovered packet reports
0x34cab018 and 0x1e00000 (30 MiB). This describes the vendor route and is not the
standard UpdateCapsule size limit. Do not access that buffer based on this capture.

The v5 query-only probe obtains a fresh GET_INFO response, then sends these fixed
queries only after success, saving progress between requests:

| Vendor function | Command | Request/response bytes | Output fields |
| --- | --- | --- | --- |
| ServiceGetCapsuleBuffer, 0x427630 | 0x10800 | 56 | u64 address at 40; u64 size at 48 |
| ServiceCapsuleGetHashOptions, 0x427ed0 | 0x12900 | 48 | u32 fields at 40 and 44 |

Both use the same 40-byte request header. The Rust encoder exposes an enum of
these two queries, not arbitrary opcodes. Response parsing checks command, size,
status, recognized GUID representation, and address overflow. Reported capsule
memory is not accessed by the diagnostic. Hash option meanings remain subject to
tracing their consumers; they must not be treated as permission to skip validation.

The subsequent validation functions are ServiceCapsuleSignatureVerify (0x13000,
96-byte request) and ServiceCapsuleVerify (0x11200, 41-byte request). The former
consumes data in a separate firmware communication buffer; the latter inspects a
firmware validation result. They are not included in the query probe. No flash
enable, firmware variable update, or capsule staging is included.
