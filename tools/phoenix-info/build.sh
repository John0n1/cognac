#!/bin/sh
set -eu
cd "$(dirname "$0")"
# Test the installed library ABI in a host process before producing boot code.
abi_test=$(mktemp)
trap 'rm -f "$abi_test"' EXIT
gcc -I/usr/include/efi -I/usr/include/efi/x86_64 -fshort-wchar -DGNU_EFI_USE_MS_ABI -maccumulate-outgoing-args -Wall -Wextra -Werror tests/abi-smoke.c -lefi -lgnuefi -o "$abi_test"
"$abi_test"
if [ -n "${PHOENIX_SIGNATURE_DATA_DIR:-}" ]; then
    set -- -DPHOENIX_SIGNATURE_PROBE -I"$PHOENIX_SIGNATURE_DATA_DIR"
else
    set --
fi
if [ "${PHOENIX_CAPSULE_PROBE:-0}" = 1 ]; then
    test -n "${PHOENIX_SIGNATURE_DATA_DIR:-}"
    python3 tests/sha256-check.py
    set -- "$@" -DPHOENIX_CAPSULE_PROBE
fi
if [ "${PHOENIX_FLASH_ONESHOT:-0}" = 1 ]; then
    test "${PHOENIX_CAPSULE_PROBE:-0}" = 1
    gcc -I/usr/include/efi -I/usr/include/efi/x86_64 -fshort-wchar -DGNU_EFI_USE_MS_ABI -maccumulate-outgoing-args -Wall -Wextra -Werror tests/flash-stage-test.c -lefi -lgnuefi -o "$abi_test"
    "$abi_test"
    set -- "$@" -DPHOENIX_FLASH_ONESHOT
fi
gcc "$@" -I/usr/include/efi -I/usr/include/efi/x86_64 -fpic -fshort-wchar -mno-red-zone -fno-stack-protector -DGNU_EFI_USE_MS_ABI -maccumulate-outgoing-args -Wall -Wextra -Werror -c main.c -o main.o
ld -nostdlib -znocombreloc -T /usr/lib/elf_x86_64_efi.lds -shared -Bsymbolic /usr/lib/crt0-efi-x86_64.o main.o -L/usr/lib -lefi -lgnuefi -o main.so
objcopy -j .text -j .sdata -j .data -j .rodata -j .dynamic -j .dynsym -j .rel -j .rela -j .reloc --target=efi-app-x86_64 main.so phoenix-info.efi
