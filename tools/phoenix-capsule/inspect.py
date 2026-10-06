#!/usr/bin/env python3
"""Offline, exact-image SCT4 signature inspection. Never accesses hardware.

Matching the capsule's embedded public key is an integrity check, not evidence
that the laptop trusts that key. Firmware validation is still required.
"""
import argparse
import hashlib
import json
import struct
import uuid
from pathlib import Path

IMAGE_SHA256 = "8f5c935fd7711e8cdb087bb34bf5c728ed556e8786652492dbede6714c84dc20"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def inspect(data, pin_image=True):
    data = bytes(data)
    require(len(data) == 26186240, "Unexpected image length")
    image_hash = hashlib.sha256(data).hexdigest()
    if pin_image:
        require(image_hash == IMAGE_SHA256, "Image differs from verified FXCN49WW capsule")
    require(data[0x78:0x7c] == b"_FVH", "Missing firmware volume")
    require(struct.unpack_from("<Q", data, 0x70)[0] == len(data) - 0x50,
            "Firmware-volume length mismatch")
    require(struct.unpack_from("<H", data, 0x84)[0] == 0x60,
            "Unexpected authentication extension offset")
    require(uuid.UUID(bytes_le=data[0xe0:0xf0]) ==
            uuid.UUID("a7717414-c616-4977-9420-844712a735bf"),
            "Unsupported certificate format")
    require(uuid.UUID(bytes_le=data[0xf0:0x100]) ==
            uuid.UUID("51aa59de-fdf2-4ea3-bc63-875fb7842ee9"),
            "Unsupported hash algorithm")

    # EVSA TLVs: secure BCP store identified in this exact vendor image.
    start = 0x12a7304
    require(data[start:start+8] == bytes.fromhex("ec8e140045565341"),
            "Unexpected secure BCP store")
    end = start + struct.unpack_from("<I", data, start+12)[0]
    require(end <= 0x1800320, "BCP store exceeds BIOS payload")
    names, flash_maps = {}, []
    pos = start
    while pos < end:
        require(pos+4 <= end, "Truncated EVSA header")
        kind, _, size = struct.unpack_from("<BBH", data, pos)
        require(size >= 4 and pos+size <= end, "Invalid EVSA record length")
        record = data[pos:pos+size]
        # The mutable record-type byte is excluded from the EVSA checksum.
        require(sum(record[1:]) % 256 == 0, "EVSA record checksum mismatch")
        payload = record[4:]
        if kind == 0xee:
            require(len(payload) >= 4 and len(payload) % 2 == 0,
                    "Invalid EVSA variable name")
            ident = int.from_bytes(payload[:2], "little")
            require(ident not in names, "Duplicate EVSA name")
            names[ident] = payload[2:].decode("utf-16-le").rstrip("\0")
        elif kind == 0xef:
            require(len(payload) >= 8, "Truncated EVSA data")
            ident = int.from_bytes(payload[2:4], "little")
            if names.get(ident) == "FlashMap":
                flash_maps.append(payload[8:])
        pos += size
    require(len(flash_maps) == 1, "Expected exactly one secure FlashMap")
    flash_map = flash_maps[0]
    require(flash_map[:10] == b"_FLASH_MAP", "Wrong FlashMap signature")
    count = int.from_bytes(flash_map[10:12], "little")
    require(count == 40 and len(flash_map) == 16+count*36,
            "Unsupported FlashMap layout")
    # Matches CapsuleSignAreaCollectSct4 + AdditionalSignAreaCollect:
    # extended raw FFS header, main/recovery regions, signed subregions,
    # and every byte after the raw BIOS payload (including FFS alignment).
    areas = [(0x300, 32)]
    for index in range(count):
        entry = flash_map[16+index*36:16+(index+1)*36]
        region, area = struct.unpack_from("<HH", entry, 16)
        size, offset = struct.unpack_from("<II", entry, 28)
        require(size > 0 and offset+size <= 0x1800000,
                "FlashMap region exceeds raw BIOS payload")
        if (region == 0 and area in (0, 1)) or (region == 1 and area == 0xf0):
            areas.append((0x320+offset, size))
    areas.append((0x1800320, len(data)-0x1800320))
    areas.sort()
    previous_end = 0
    digest = hashlib.sha256()
    for offset, size in areas:
        require(offset >= previous_end, "Overlapping signed regions")
        previous_end = offset+size
        digest.update(data[offset:previous_end])
    # Phoenix stores the modulus and signature little endian. Exponent 65537
    # yields a standard big-endian EMSA-PKCS1-v1_5 SHA256 encoded message.
    modulus = int.from_bytes(data[0x100:0x200], "little")
    signature = int.from_bytes(data[0x200:0x300], "little")
    require(modulus.bit_length() == 2048 and modulus % 2 == 1,
            "Invalid RSA modulus")
    require(0 < signature < modulus, "Invalid RSA signature integer")
    encoded = pow(signature, 65537, modulus).to_bytes(256, "big")
    digest_info = bytes.fromhex("3031300d060960864801650304020105000420") + digest.digest()
    expected = b"\0\1" + b"\xff"*(256-len(digest_info)-3) + b"\0" + digest_info
    require(encoded == expected, "Computed signed-region digest does not match RSA signature")
    return {
        "image_sha256": image_hash,
        "signed_region_sha256": digest.hexdigest(),
        "algorithm": "RSA-2048 PKCS1-v1_5 SHA-256 (Phoenix algorithm 3)",
        "embedded_key_signature_matches": True,
        "firmware_trust_verified": False,
        "flash_ready": False,
        "signed_regions": [{"offset": offset, "length": size} for offset, size in areas],
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("capsule", type=Path)
    args = parser.parse_args()
    try:
        print(json.dumps(inspect(args.capsule.read_bytes()), indent=2))
    except (ValueError, OSError, UnicodeError, struct.error) as error:
        parser.exit(1, f"Inspection refused: {error}\n")
