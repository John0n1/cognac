//! Phoenix SCT protocol discovery. This module never maps physical memory,
//! triggers an SMI, changes EFI variables, or submits a capsule.
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{fs, path::Path};

const SMI_TABLE_GUID: [u8; 16] = [
    0x97, 0xb1, 0x9f, 0x0d, 0xfc, 0xce, 0x91, 0x4e, 0xac, 0xb1, 0x25, 0x35, 0xd9, 0xe5, 0xa8, 0x44,
];
const FLASH_SMI_GUID: [u8; 16] = [
    0x58, 0xdc, 0xaf, 0xd8, 0x22, 0x6e, 0xf8, 0x42, 0x99, 0x66, 0x36, 0xff, 0x78, 0x8c, 0x9c, 0xaf,
];
const SHARED_MEMORY_GUID: [u8; 16] = [
    0xe8, 0x63, 0x95, 0xd2, 0xe1, 0xcf, 0x41, 0x4d, 0x8e, 0x54, 0xda, 0x43, 0x22, 0xfe, 0xde, 0x5c,
];
const FLASH_IDENTIFICATION_GUID: [u8; 16] = [
    0xfc, 0x44, 0xde, 0xb1, 0x46, 0x79, 0x82, 0x49, 0x9b, 0x4b, 0x2f, 0x8c, 0xa4, 0x5e, 0xa7, 0x92,
];

#[derive(Debug, Serialize)]
pub struct Discovery {
    pub smi_port: u16,
    pub flash_smi_command: u8,
    pub shared_memory_address: u64,
    pub submission_supported: bool,
    pub limitation: &'static str,
}

fn bytes(data: &[u8], offset: usize, len: usize) -> Result<&[u8]> {
    data.get(offset..offset.checked_add(len).context("ACPI offset overflow")?)
        .context("truncated ACPI table")
}
fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(bytes(data, offset, 4)?.try_into()?))
}
fn table<'a>(data: &'a [u8], signature: &[u8; 4]) -> Result<&'a [u8]> {
    ensure!(bytes(data, 0, 4)? == signature, "wrong ACPI signature");
    let length = u32_at(data, 4)? as usize;
    ensure!(
        length >= 36 && length == data.len(),
        "invalid ACPI declared length"
    );
    ensure!(
        data.iter().fold(0u8, |s, b| s.wrapping_add(*b)) == 0,
        "invalid ACPI checksum"
    );
    Ok(data)
}

/// Decode the layout used by WinFlash64's SmiFindPort and ServiceChannelSmiRequest.
/// Discovery establishes interface presence, never model/payload compatibility.
pub fn decode(uefi: &[u8], batb: &[u8], facp: &[u8]) -> Result<Discovery> {
    let uefi = table(uefi, b"UEFI")?;
    let batb = table(batb, b"BATB")?;
    let facp = table(facp, b"FACP")?;
    ensure!(
        bytes(uefi, 36, 16)? == SMI_TABLE_GUID,
        "unrecognized Phoenix SMI table GUID"
    );
    ensure!(
        bytes(batb, 36, 16)? == SHARED_MEMORY_GUID,
        "unrecognized Phoenix shared-memory GUID"
    );
    // The examined binary uses fixed offsets, not an arbitrary table payload offset.
    ensure!(
        bytes(uefi, 52, 2)? == [0x36, 0],
        "unsupported Phoenix SMI data offset"
    );
    ensure!(
        bytes(batb, 52, 2)? == [0x36, 0],
        "unsupported Phoenix shared-memory data offset"
    );
    let count = u32_at(uefi, 54)? as usize;
    ensure!(count > 0 && count <= 256, "invalid Phoenix SMI entry count");
    let guids = 58 + count * 4;
    bytes(uefi, guids, count * 16)?;
    let mut command = None;
    for i in 0..count {
        if bytes(uefi, guids + i * 16, 16)? == FLASH_SMI_GUID {
            ensure!(command.is_none(), "ambiguous Phoenix flash SMI entries");
            command = Some(
                u8::try_from(u32_at(uefi, 58 + i * 4)?)
                    .context("Phoenix SMI command exceeds port byte width")?,
            );
        }
    }
    let command = command.context("Phoenix flash SMI GUID absent")?;
    let port = u16::try_from(u32_at(facp, 48)?).context("SMI port exceeds x86 port width")?;
    // SmiFindPort reads a DWORD at BATB+0x36, even on x64. Do not guess
    // meanings for the remaining BATB extension fields.
    let address = u64::from(u32_at(batb, 54)?);
    ensure!(
        command != 0 && port != 0 && address != 0,
        "incomplete Phoenix flash interface"
    );
    Ok(Discovery {
        smi_port: port,
        flash_smi_command: command,
        shared_memory_address: address,
        submission_supported: false,
        limitation: "Discovery only. Firmware validation, transport and recovery are not implemented for Phoenix SCT; no writes or SMI are issued.",
    })
}

pub fn discover(directory: &Path) -> Result<Discovery> {
    let read = |name| {
        fs::read(directory.join(name))
        .with_context(|| format!("cannot read ACPI {name}; supply a readable table snapshot or run this read-only probe with administrator privileges"))
    };
    decode(&read("UEFI")?, &read("BATB")?, &read("FACP")?)
}

/// Offline encoder for ServiceGetInfo (WinFlash64 0x427550).
/// This produces bytes only. There is deliberately no hardware transport.
pub fn service_get_info_request() -> [u8; 104] {
    let mut request = [0; 104];
    request[0..8].copy_from_slice(&0x10000u64.to_le_bytes());
    request[8..16].copy_from_slice(&0xffu64.to_le_bytes());
    request[16..24].copy_from_slice(&104u64.to_le_bytes());
    request[24..40].copy_from_slice(&FLASH_IDENTIFICATION_GUID);
    request
}

/// Decode only the legacy 64-byte or observed FXCN28WW 72-byte information
/// format. A zeroed response GUID is observed firmware behavior, not a model
/// identity assertion. This decoder never authenticates a capsule or submits it.
pub fn service_get_info_response(data: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        matches!(data.len(), 104 | 112),
        "unsupported ServiceGetInfo response length"
    );
    let field = |offset: usize| -> u64 {
        u64::from_le_bytes(data[offset..offset + 8].try_into().expect("bounded header"))
    };
    ensure!(field(0) == 0x10000, "ServiceGetInfo command mismatch");
    ensure!(
        field(16) == data.len() as u64,
        "ServiceGetInfo packet size mismatch"
    );
    ensure!(
        data[24..40] == FLASH_IDENTIFICATION_GUID || data[24..40] == [0; 16],
        "unrecognized ServiceGetInfo response GUID"
    );
    let status = field(8);
    if status != 0 {
        bail!("Phoenix ServiceGetInfo firmware status {status:#x}");
    }
    ensure!(
        field(40) == (data.len() - 40) as u64,
        "ServiceGetInfo payload size mismatch"
    );
    Ok(data[40..].to_vec())
}

/// Only information queries whose layouts were traced in the vendor binary.
#[derive(Debug, Clone, Copy)]
pub enum CapabilityQuery {
    CapsuleBuffer,
    HashOptions,
}
impl CapabilityQuery {
    fn layout(self) -> (u64, usize) {
        match self {
            Self::CapsuleBuffer => (0x10800, 56),
            Self::HashOptions => (0x12900, 48),
        }
    }
}
pub fn capability_request(query: CapabilityQuery) -> Vec<u8> {
    let (command, length) = query.layout();
    let mut packet = vec![0; length];
    packet[..8].copy_from_slice(&command.to_le_bytes());
    packet[8..16].copy_from_slice(&0xffu64.to_le_bytes());
    packet[16..24].copy_from_slice(&(length as u64).to_le_bytes());
    packet[24..40].copy_from_slice(&FLASH_IDENTIFICATION_GUID);
    packet
}
#[derive(Debug, Serialize, PartialEq)]
pub enum CapabilityResponse {
    CapsuleBuffer { address: u64, bytes: u64 },
    HashOptions { first: u32, second: u32 },
}
pub fn capability_response(query: CapabilityQuery, packet: &[u8]) -> Result<CapabilityResponse> {
    let (command, length) = query.layout();
    ensure!(
        packet.len() == length,
        "capability response length mismatch"
    );
    let qword = |offset: usize| {
        u64::from_le_bytes(
            packet[offset..offset + 8]
                .try_into()
                .expect("bounded packet"),
        )
    };
    ensure!(
        qword(0) == command && qword(16) == length as u64,
        "capability command/size mismatch"
    );
    ensure!(
        packet[24..40] == [0; 16] || packet[24..40] == FLASH_IDENTIFICATION_GUID,
        "capability response GUID mismatch"
    );
    ensure!(qword(8) == 0, "firmware capability status {:#x}", qword(8));
    match query {
        CapabilityQuery::CapsuleBuffer => {
            let address = qword(40);
            let size = qword(48);
            ensure!(
                address != 0 && size != 0 && address.checked_add(size).is_some(),
                "invalid capsule buffer range"
            );
            Ok(CapabilityResponse::CapsuleBuffer {
                address,
                bytes: size,
            })
        }
        CapabilityQuery::HashOptions => Ok(CapabilityResponse::HashOptions {
            first: u32_at(packet, 40)?,
            second: u32_at(packet, 44)?,
        }),
    }
}

/// Offline ESP handoff layout from HddCapsuleUpdateSetup. The ESP branch
/// leaves LBA, media mode and path zero and sets capsule byte count and the
/// GPT unique partition GUID. This is NOT a staging or authentication API.
pub fn esp_handoff_data(capsule_size: u64, partition_guid_le: [u8; 16]) -> Result<[u8; 160]> {
    ensure!(capsule_size >= 28, "capsule too small");
    let size = u32::try_from(capsule_size).context("capsule exceeds vendor DWORD size field")?;
    ensure!(
        partition_guid_le != [0; 16],
        "missing GPT unique partition GUID"
    );
    let mut data = [0; 160];
    data[12..16].copy_from_slice(&size.to_le_bytes());
    data[16..32].copy_from_slice(&partition_guid_le);
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn checksum(data: &mut [u8]) {
        data[9] = 0;
        data[9] = 0u8.wrapping_sub(data.iter().fold(0u8, |s, b| s.wrapping_add(*b)));
    }
    fn acpi(signature: &[u8; 4], len: usize) -> Vec<u8> {
        let mut data = vec![0; len];
        data[..4].copy_from_slice(signature);
        data[4..8].copy_from_slice(&(len as u32).to_le_bytes());
        data
    }
    fn fixture() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let mut u = acpi(b"UEFI", 78);
        u[36..52].copy_from_slice(&SMI_TABLE_GUID);
        u[52] = 54;
        u[54..58].copy_from_slice(&1u32.to_le_bytes());
        u[58] = 0xe9;
        u[62..78].copy_from_slice(&FLASH_SMI_GUID);
        let mut b = acpi(b"BATB", 74);
        b[36..52].copy_from_slice(&SHARED_MEMORY_GUID);
        b[52] = 54;
        b[54..58].copy_from_slice(&0x44cd4000u32.to_le_bytes());
        let mut f = acpi(b"FACP", 276);
        f[48] = 0xb2;
        for t in [&mut u, &mut b, &mut f] {
            checksum(t);
        }
        (u, b, f)
    }
    #[test]
    fn decodes_discovery_without_claiming_flash_support() {
        let (u, b, f) = fixture();
        let d = decode(&u, &b, &f).unwrap();
        assert_eq!(d.smi_port, 0xb2);
        assert_eq!(d.flash_smi_command, 0xe9);
        assert_eq!(d.shared_memory_address, 0x44cd4000);
        assert!(!d.submission_supported);
    }
    #[test]
    fn rejects_corruption_truncation_wrong_guids_and_large_counts() {
        let (u, b, f) = fixture();
        for len in 0..u.len() {
            assert!(decode(&u[..len], &b, &f).is_err());
        }
        let mut broken = u.clone();
        broken[9] ^= 1;
        assert!(decode(&broken, &b, &f).is_err());
        let mut broken = u.clone();
        broken[36] ^= 1;
        checksum(&mut broken);
        assert!(decode(&broken, &b, &f).is_err());
        let mut broken = u;
        broken[54..58].copy_from_slice(&u32::MAX.to_le_bytes());
        checksum(&mut broken);
        assert!(decode(&broken, &b, &f).is_err());
    }
    #[test]
    fn rejects_ambiguous_and_non_byte_commands() {
        let (u, b, f) = fixture();
        let mut duplicate = acpi(b"UEFI", 98);
        duplicate[..58].copy_from_slice(&u[..58]);
        duplicate[4..8].copy_from_slice(&98u32.to_le_bytes());
        duplicate[54..58].copy_from_slice(&2u32.to_le_bytes());
        duplicate[58] = 0xe9;
        duplicate[62] = 0xe9;
        duplicate[66..82].copy_from_slice(&FLASH_SMI_GUID);
        duplicate[82..98].copy_from_slice(&FLASH_SMI_GUID);
        checksum(&mut duplicate);
        assert!(decode(&duplicate, &b, &f).is_err());
        let mut wide = u;
        wide[58..62].copy_from_slice(&256u32.to_le_bytes());
        checksum(&mut wide);
        assert!(decode(&wide, &b, &f).is_err());
    }
    #[test]
    fn esp_handoff_preserves_exact_vendor_offsets_without_writing() {
        let guid = [7; 16];
        let data = esp_handoff_data(26186240, guid).unwrap();
        assert_eq!(&data[..12], &[0; 12]);
        assert_eq!(&data[12..16], &26186240u32.to_le_bytes());
        assert_eq!(&data[16..32], &guid);
        assert_eq!(&data[32..], &[0; 128]);
        assert!(esp_handoff_data(1, guid).is_err());
        assert!(esp_handoff_data(u64::MAX, guid).is_err());
        assert!(esp_handoff_data(26186240, [0; 16]).is_err());
    }
    #[test]
    fn info_packet_requires_firmware_completion_and_identity() {
        let mut p = service_get_info_request();
        assert!(service_get_info_response(&p).is_err());
        p[8..16].fill(0);
        p[40] = 64;
        p[48] = 42;
        assert_eq!(service_get_info_response(&p).unwrap()[8], 42);
        p[24] ^= 1;
        assert!(service_get_info_response(&p).is_err());
        assert!(service_get_info_response(&p[..103]).is_err());
    }
    #[test]
    fn decodes_observed_fxcn28ww_response_and_rejects_bad_lengths_status() {
        let mut packet=hex::decode("000001000000000000000000000000007000000000000000000000000000000000000000000000004800000000000000010000000000000018b0ca34000000000000e001000000000100000000000000010000000000000000100000000000000101000000000000ff03000000000000").unwrap();
        assert_eq!(service_get_info_response(&packet).unwrap().len(), 72);
        assert!(service_get_info_response(&packet[..104]).is_err());
        packet[8] = 0xff;
        assert!(service_get_info_response(&packet).is_err());
        packet[8] = 0;
        packet[40] = 64;
        assert!(service_get_info_response(&packet).is_err());
        packet[40] = 72;
        packet[0] ^= 1;
        assert!(service_get_info_response(&packet).is_err());
    }
    #[test]
    fn capability_queries_reject_unprocessed_wrong_command_and_overflow() {
        for query in [CapabilityQuery::CapsuleBuffer, CapabilityQuery::HashOptions] {
            let mut packet = capability_request(query);
            assert!(capability_response(query, &packet).is_err());
            packet[8..16].fill(0);
            packet[24..40].fill(0);
            packet[40..48].copy_from_slice(&1u64.to_le_bytes());
            if packet.len() == 56 {
                packet[48..56].copy_from_slice(&4096u64.to_le_bytes());
            }
            assert!(capability_response(query, &packet).is_ok());
            packet[0] ^= 1;
            assert!(capability_response(query, &packet).is_err());
            packet[0] ^= 1;
            if packet.len() == 56 {
                packet[40..48].copy_from_slice(&u64::MAX.to_le_bytes());
                assert!(capability_response(query, &packet).is_err());
            }
            assert!(capability_response(query, &packet[..packet.len() - 1]).is_err());
        }
    }
}
