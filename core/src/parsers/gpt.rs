//! GUID Partition Table.
//!
//! The partition table on EFI systems and on most single-board-computer card
//! images, so the layout of a Buildroot `sdcard.img` for anything that is not
//! raw flash usually lives here rather than in an MBR.
//!
//! Both the header and the entry array carry a CRC-32, which makes the table
//! self-verifying: a table that checksums is a table, not a coincidence.

use super::{le_u32, le_u64};
use crate::crc::crc32_ieee;

const SIGNATURE: &[u8; 8] = b"EFI PART";
/// Sector sizes worth trying. A card image is 512; some disks are 4096.
const SECTOR_SIZES: &[usize] = &[512, 4096];
const MIN_HEADER: usize = 92;
/// Guards a corrupt count from allocating without bound.
const MAX_ENTRIES: u32 = 512;

#[derive(Debug, Clone, PartialEq)]
pub struct GptPartition {
    /// 1-based, the way partition devices are numbered.
    pub index: u32,
    pub type_guid: String,
    /// A human name for well-known type GUIDs, else empty.
    pub type_name: &'static str,
    pub unique_guid: String,
    pub offset: u64,
    pub size: u64,
    /// UTF-16 name from the entry, usually set by the image builder.
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GptInfo {
    pub sector_size: u32,
    pub disk_guid: String,
    pub first_usable: u64,
    pub last_usable: u64,
    pub header_crc_ok: bool,
    pub entries_crc_ok: bool,
    pub partitions: Vec<GptPartition>,
}

/// A GUID is stored with its first three fields little endian and the rest
/// big endian, which is why it cannot simply be hex-dumped.
fn format_guid(b: &[u8]) -> Option<String> {
    let g = b.get(..16)?;
    Some(format!(
        "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{}",
        u32::from_le_bytes([g[0], g[1], g[2], g[3]]),
        u16::from_le_bytes([g[4], g[5]]),
        u16::from_le_bytes([g[6], g[7]]),
        g[8],
        g[9],
        g[10..16].iter().map(|x| format!("{x:02X}")).collect::<String>()
    ))
}

/// The type GUIDs a Buildroot image actually produces, plus the neighbours
/// they sit next to.
fn type_name(guid: &str) -> &'static str {
    match guid {
        "C12A7328-F81F-11D2-BA4B-00A0C93EC93B" => "EFI system",
        "21686148-6449-6E6F-744E-656564454649" => "BIOS boot",
        "0FC63DAF-8483-4772-8E79-3D69D8477DE4" => "Linux filesystem",
        "44479540-F297-41B2-9AF7-D131D5F0458A" => "Linux root (x86)",
        "4F68BCE3-E8CD-4DB1-96E7-FBCAF984B709" => "Linux root (x86-64)",
        "69DAD710-2CE4-4E3C-B16C-21A1D49ABED3" => "Linux root (arm)",
        "B921B045-1DF0-41C3-AF44-4C6F280D3FAE" => "Linux root (arm64)",
        "933AC7E1-2EB4-4F13-B844-0E14E2AEF915" => "Linux /home",
        "0657FD6D-A4AB-43C4-84E5-0933C84B4F4F" => "Linux swap",
        "E6D6D379-F507-44C2-A23C-238F2A3DF928" => "Linux LVM",
        "EBD0A0A2-B9E5-4433-87C0-68B6B72699C7" => "Microsoft basic data",
        _ => "",
    }
}

fn utf16_name(b: &[u8]) -> String {
    let units: Vec<u16> = b
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

fn parse_at(data: &[u8], sector: usize) -> Option<GptInfo> {
    let head = data.get(sector..sector + sector.max(MIN_HEADER))?;
    if head.get(..8)? != SIGNATURE {
        return None;
    }
    let header_size = le_u32(head, 12)? as usize;
    if !(MIN_HEADER..=sector).contains(&header_size) {
        return None;
    }

    // The header CRC covers the header with its own CRC field zeroed.
    let stored_crc = le_u32(head, 16)?;
    let mut copy = head.get(..header_size)?.to_vec();
    copy[16..20].fill(0);
    let header_crc_ok = crc32_ieee(&copy) == stored_crc;

    let entry_lba = le_u64(head, 72)?;
    let count = le_u32(head, 80)?.min(MAX_ENTRIES);
    let entry_size = le_u32(head, 84)? as usize;
    if entry_size < 128 || entry_size > 4096 || count == 0 {
        return None;
    }

    let table_at = (entry_lba as usize).checked_mul(sector)?;
    let table_len = (count as usize).checked_mul(entry_size)?;
    let table = data.get(table_at..table_at + table_len)?;
    let entries_crc_ok = crc32_ieee(table) == le_u32(head, 88)?;

    // A table whose checksums both fail is almost certainly not a table.
    if !header_crc_ok && !entries_crc_ok {
        return None;
    }

    let mut partitions = Vec::new();
    for i in 0..count as usize {
        let e = &table[i * entry_size..(i + 1) * entry_size];
        // An unused entry is a zero type GUID.
        if e[..16].iter().all(|&b| b == 0) {
            continue;
        }
        let first = le_u64(e, 32)?;
        let last = le_u64(e, 40)?;
        if last < first {
            continue;
        }
        let type_guid = format_guid(&e[..16])?;
        partitions.push(GptPartition {
            index: i as u32 + 1,
            type_name: type_name(&type_guid),
            type_guid,
            unique_guid: format_guid(&e[16..32])?,
            offset: first * sector as u64,
            // Both bounds are inclusive.
            size: (last - first + 1) * sector as u64,
            name: utf16_name(e.get(56..56 + 72).unwrap_or(&[])),
        });
    }
    if partitions.is_empty() {
        return None;
    }

    Some(GptInfo {
        sector_size: sector as u32,
        disk_guid: format_guid(head.get(56..72)?)?,
        first_usable: le_u64(head, 40)?,
        last_usable: le_u64(head, 48)?,
        header_crc_ok,
        entries_crc_ok,
        partitions,
    })
}

pub fn parse(data: &[u8]) -> Option<GptInfo> {
    SECTOR_SIZES.iter().find_map(|&s| parse_at(data, s))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECTOR: usize = 512;

    fn guid_bytes(s: &str) -> Vec<u8> {
        let h: Vec<u8> = s
            .chars()
            .filter(|c| c.is_ascii_hexdigit())
            .collect::<String>()
            .as_bytes()
            .chunks(2)
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect();
        let mut out = Vec::with_capacity(16);
        out.extend_from_slice(&u32::from_be_bytes([h[0], h[1], h[2], h[3]]).to_le_bytes());
        out.extend_from_slice(&u16::from_be_bytes([h[4], h[5]]).to_le_bytes());
        out.extend_from_slice(&u16::from_be_bytes([h[6], h[7]]).to_le_bytes());
        out.extend_from_slice(&h[8..16]);
        out
    }

    fn entry(type_guid: &str, first: u64, last: u64, name: &str) -> Vec<u8> {
        let mut e = vec![0u8; 128];
        e[..16].copy_from_slice(&guid_bytes(type_guid));
        e[16..32].copy_from_slice(&guid_bytes("11111111-2222-3333-4444-555555555555"));
        e[32..40].copy_from_slice(&first.to_le_bytes());
        e[40..48].copy_from_slice(&last.to_le_bytes());
        for (i, u) in name.encode_utf16().enumerate() {
            e[56 + i * 2..58 + i * 2].copy_from_slice(&u.to_le_bytes());
        }
        e
    }

    fn disk(entries: Vec<Vec<u8>>, break_header_crc: bool) -> Vec<u8> {
        let mut img = vec![0u8; 64 * SECTOR];
        let count = 128u32;
        let mut table = vec![0u8; count as usize * 128];
        for (i, e) in entries.iter().enumerate() {
            table[i * 128..(i + 1) * 128].copy_from_slice(e);
        }
        img[2 * SECTOR..2 * SECTOR + table.len()].copy_from_slice(&table);

        let mut h = vec![0u8; 92];
        h[..8].copy_from_slice(SIGNATURE);
        h[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        h[12..16].copy_from_slice(&92u32.to_le_bytes());
        h[24..32].copy_from_slice(&1u64.to_le_bytes()); // my_lba
        h[32..40].copy_from_slice(&63u64.to_le_bytes()); // alternate
        h[40..48].copy_from_slice(&34u64.to_le_bytes()); // first usable
        h[48..56].copy_from_slice(&30u64.to_le_bytes()); // last usable
        h[56..72].copy_from_slice(&guid_bytes("AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE"));
        h[72..80].copy_from_slice(&2u64.to_le_bytes()); // entry array LBA
        h[80..84].copy_from_slice(&count.to_le_bytes());
        h[84..88].copy_from_slice(&128u32.to_le_bytes());
        h[88..92].copy_from_slice(&crc32_ieee(&table).to_le_bytes());
        let crc = crc32_ieee(&h);
        h[16..20].copy_from_slice(&(if break_header_crc { !crc } else { crc }).to_le_bytes());
        img[SECTOR..SECTOR + h.len()].copy_from_slice(&h);
        img
    }

    #[test]
    fn reads_a_two_partition_table() {
        let img = disk(
            vec![
                entry("C12A7328-F81F-11D2-BA4B-00A0C93EC93B", 4, 7, "boot"),
                entry("0FC63DAF-8483-4772-8E79-3D69D8477DE4", 8, 29, "rootfs"),
            ],
            false,
        );
        let g = parse(&img).expect("gpt");
        assert_eq!(g.sector_size, 512);
        assert!(g.header_crc_ok && g.entries_crc_ok);
        assert_eq!(g.disk_guid, "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE");
        assert_eq!(g.partitions.len(), 2);

        let boot = &g.partitions[0];
        assert_eq!(boot.index, 1);
        assert_eq!(boot.name, "boot");
        assert_eq!(boot.type_name, "EFI system");
        assert_eq!(boot.offset, 4 * 512);
        assert_eq!(boot.size, 4 * 512); // LBA 4..7 inclusive

        let root = &g.partitions[1];
        assert_eq!(root.index, 2);
        assert_eq!(root.name, "rootfs");
        assert_eq!(root.type_name, "Linux filesystem");
        assert_eq!(root.offset, 8 * 512);
        assert_eq!(root.size, 22 * 512);
    }

    /// A damaged header is still usable when the entry array checksums: that
    /// is exactly the case worth reporting rather than refusing.
    #[test]
    fn a_bad_header_crc_is_reported_not_fatal() {
        let img = disk(vec![entry("0FC63DAF-8483-4772-8E79-3D69D8477DE4", 4, 9, "x")], true);
        let g = parse(&img).expect("gpt");
        assert!(!g.header_crc_ok);
        assert!(g.entries_crc_ok);
        assert_eq!(g.partitions.len(), 1);
    }

    #[test]
    fn rejects_non_gpt() {
        assert!(parse(&[0u8; 64 * SECTOR]).is_none());
        assert!(parse(&vec![0xFFu8; 64 * SECTOR]).is_none());
        // signature only, nothing behind it
        let mut fake = vec![0u8; 64 * SECTOR];
        fake[SECTOR..SECTOR + 8].copy_from_slice(SIGNATURE);
        assert!(parse(&fake).is_none());
        // no used entries
        assert!(parse(&disk(vec![], false)).is_none());
    }
}
