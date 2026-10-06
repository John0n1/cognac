# Exact-platform Phoenix information probe

Experimental Lenovo FXCN28WW-only EFI application. Read docs/phoenix-sct.md first.
The source issues the reverse-engineered GET_INFO service request followed by
capsule-buffer and hash-option queries after a keypress. It writes firmware-owned shared RAM and triggers an SMI; it does not
write flash or update variables. Compiling successfully is not hardware validation.

Run build.sh with GNU-EFI and binutils installed. Copy phoenix-info.efi into
EFI/CognacPhoenix on the ESP. Select it for a one-time boot. Press I to query or
any other key to skip. Results are saved beside it as info.txt. It returns to the
firmware boot manager after 15 seconds. A table/memory-map mismatch skips the SMI.
Do not change the captured platform profile to force a match.

## Exact-image signature validation probe (v6)

The optional PHOENIX_SIGNATURE_DATA_DIR build supplies signature-data.h generated
only after tools/phoenix-capsule/inspect.py accepts the pinned FXCN49WW image.
It adds command 0x13000, traced from ServiceCapsuleSignatureVerify, using a
UEFI-allocated page below 4 GiB for the computed hash and original signature.
It requires unchanged zero hash options. It logs the complete firmware response
and never enables flash, copies a capsule into its firmware buffer, sets an update
variable, or reboots. A successful signature response still requires full image
and platform validation before an update. This variant has been built but has
has returned status zero and result fields 1,1 on this FXCN28WW laptop.

## Full capsule validation probe (v8)

PHOENIX_CAPSULE_PROBE=1 adds the traced CapsulePrepare RAM descriptor and
ServiceCapsuleVerify command 0x11200, gated on successful signature validation.
It reads EFI/CognacPhoenix/FXCN49WW.CAP and computes its pinned complete SHA256
before copying anything into the firmware's reported capsule buffer. The exact
buffer address/size must match; its whole accessed range must be reserved RAM,
ACPI NVS, or runtime-services data in the UEFI map. Runtime-services data is
accepted only for the exact queried buffer address and pinned image length;
the SMI request area still requires reserved RAM or NVS. Full byte-for-byte readback precedes validation. It
clears the temporary descriptor and capsule afterward. No flash-enable, update
variable or reset is included. V7 stopped before any capsule-buffer write because
the actual buffer is type 6 (runtime-services data), within 0x34077000..0x36cca000.
The corrected v8 variant is built but not yet hardware-tested.
The SHA256 implementation passes twelve padding/boundary comparisons against
Python hashlib and a comparison over the full original capsule.

## Authorized conditional update (v10)

PHOENIX_FLASH_ONESHOT=1 additionally enables the vendor RAM-capsule handoff,
only after the same-boot complete image pin, signature-service acceptance, RAM
readback. It follows CapsuleValidationByService's successful signature branch:
the original executable returns success at 0x425955 after ServiceCapsuleSignatureVerify
returns zero (0x425928..0x425932). It calls ServiceCapsuleVerify at 0x42594b only
as an alternative when signature validation is unavailable. V9's additional
requirement for that alternative stopped safely with EFI_UNSUPPORTED, before
flash switch or trigger variables. V10 uses the successful original signature
path; it does not ignore signature rejection or any handoff error.
It detects the flash channel with
the vendor's read-only command 5, enables its flash switch with command 9,
and submits CapsuleUpdateData and CapsuleGuidData through TdkVariableSet
(0x10600). NVS completion (0x13300) and ordinary UEFI-variable readback must
both succeed. It disables the flash switch before warm reset, preserving the
firmware-owned RAM descriptor/image for the vendor updater.

Existing vendor trigger variables are refused. Incomplete handoffs are cleaned
up; uncertain cleanup preserves RAM and blocks automatic reset. No write is
automatically retried. If the final handoff journal cannot be saved, automatic
reset is blocked. This code uses the original capsule and vendor updater; it
does not directly erase or write SPI flash. Success is established only by
post-update firmware version and firmware results, not by submission alone.

The deployment helper additionally requires this exact laptop's old BIOS,
AC power online, adequate battery charge and no existing trigger variables.
The application requires the operator's I key. Native mocks cover successful
handoff, channel rejection before writes, rollback after a rejected second
variable, failed flash-disable, and failed rollback. These tests verify protocol
encoding and control flow. V10 subsequently completed the physical vendor
handoff: channel detection, enabling/disabling the switch, both variable writes,
NVS completion and variable readback all succeeded. After the warm reset, DMI
reports FXCN49WW dated 2024-09-05 and system ESRT version 65585. The trigger
variables were consumed, Debian booted normally, and the temporary ESP files
were archived and removed. This validates this exact FXCN28WW-to-FXCN49WW
transition on this laptop; it does not establish general Phoenix/platform
compatibility. V9 stopped before enabling flash or writing variables.

The offline inspector reconstructs the signed regions from secure BCP EVSA
FlashMap records and checks RSA-2048 PKCS1-v1_5 SHA-256 with the embedded key.
That proves internal consistency, not firmware trust. Its five tests cover the
original signature, payload/signature corruption, truncation, and image-pin
rejection of modifications outside the signed regions.

## Build regression check

Debian's installed GNU-EFI CopyMem and SetMem use EFIAPI (Microsoft x64 ABI).
Compile with GNU_EFI_USE_MS_ABI and reserve outgoing argument space. The older
EFI_FUNCTION_WRAPPER-only build called CopyMem using System V registers and
could fail or hang while reading ACPI fields. build.sh now runs a host-process
copy/compare/set regression test against the actual library before producing the
EFI application. The old flags fail this test; corrected flags pass. This verifies
the library ABI, not the hardware SMI behavior.
