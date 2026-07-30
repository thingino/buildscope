//! FAT12/16/32 boot sector, and the allocation table behind it.
//!
//! The boot partition of essentially every card image. Geometry alone would
//! only give the partition's size, which the layout already knows, so this
//! also counts allocated clusters in the FAT: that is uncompressed, cheap to
//! read, and turns "32 MiB partition" into "3.2 MiB of it is in use".
//!
//! Field offsets follow Microsoft's FAT specification, little endian.

use super::{le_u16, le_u32};

#[derive(Debug, Clone, PartialEq)]
pub struct FatInfo {
    /// "FAT12" | "FAT16" | "FAT32"
    pub kind: &'static str,
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub cluster_bytes: u32,
    pub total_sectors: u64,
    pub cluster_count: u32,
    /// Clusters with a non-zero entry in the allocation table.
    pub used_clusters: u32,
    pub total_bytes: u64,
    /// Space the volume occupies: allocated clusters plus the reserved
    /// sectors, the allocation tables and any fixed root directory, so used
    /// and free add up to the total the way `df` reports them.
    pub used_bytes: u64,
    pub free_bytes: u64,
    /// The metadata share of `used_bytes`.
    pub overhead_bytes: u64,
    pub label: String,
    pub oem: String,
}

fn trimmed(b: &[u8]) -> String {
    String::from_utf8_lossy(b)
        .trim_end()
        .trim_end_matches('\0')
        .to_string()
}

/// One entry from the first allocation table.
fn fat_entry(fat: &[u8], kind: &str, n: u32) -> Option<u32> {
    let i = n as usize;
    match kind {
        "FAT12" => {
            // Twelve bits per entry, so every other one straddles a byte.
            let off = i + i / 2;
            let pair = u16::from_le_bytes([*fat.get(off)?, *fat.get(off + 1)?]);
            Some(if i.is_multiple_of(2) {
                pair & 0x0FFF
            } else {
                pair >> 4
            } as u32)
        }
        "FAT16" => le_u16(fat, i * 2).map(|v| v as u32),
        _ => le_u32(fat, i * 4).map(|v| v & 0x0FFF_FFFF),
    }
}

pub fn parse(data: &[u8]) -> Option<FatInfo> {
    let boot = data.get(..512)?;
    // A boot sector opens with a jump instruction and ends with 0x55AA.
    if !(boot[0] == 0xEB || boot[0] == 0xE9) {
        return None;
    }
    if boot[510] != 0x55 || boot[511] != 0xAA {
        return None;
    }

    let bytes_per_sector = le_u16(boot, 11)?;
    if !matches!(bytes_per_sector, 512 | 1024 | 2048 | 4096) {
        return None;
    }
    let sectors_per_cluster = boot[13];
    if sectors_per_cluster == 0 || !sectors_per_cluster.is_power_of_two() {
        return None;
    }
    let reserved = le_u16(boot, 14)? as u64;
    let num_fats = boot[16];
    if reserved == 0 || !(1..=4).contains(&num_fats) {
        return None;
    }
    let root_entries = le_u16(boot, 17)? as u64;
    let total_16 = le_u16(boot, 19)? as u64;
    let fat_16 = le_u16(boot, 22)? as u64;
    let total_32 = le_u32(boot, 32)? as u64;
    let fat_32 = le_u32(boot, 36)? as u64;

    let total_sectors = if total_16 != 0 { total_16 } else { total_32 };
    let fat_sectors = if fat_16 != 0 { fat_16 } else { fat_32 };
    if total_sectors == 0 || fat_sectors == 0 {
        return None;
    }

    let bps = bytes_per_sector as u64;
    // The fixed root directory exists on FAT12/16 only; on FAT32 it is a
    // normal cluster chain and root_entries is zero.
    let root_sectors = (root_entries * 32).div_ceil(bps);
    let meta_sectors = reserved + num_fats as u64 * fat_sectors + root_sectors;
    if meta_sectors >= total_sectors {
        return None;
    }
    let data_sectors = total_sectors - meta_sectors;
    let cluster_count = (data_sectors / sectors_per_cluster as u64) as u32;

    // A zero 16-bit FAT length with a 32-bit one set says FAT32 outright, and
    // that beats counting clusters: `mkfs.vfat -F 32` will happily build a
    // FAT32 with fewer than the 65525 clusters the specification asks for, and
    // reading its 32-bit table as 16-bit entries would produce nonsense. The
    // cluster count only settles FAT12 against FAT16.
    let kind = if fat_16 == 0 && fat_32 != 0 {
        "FAT32"
    } else if cluster_count < 4085 {
        "FAT12"
    } else if cluster_count < 65525 {
        "FAT16"
    } else {
        "FAT32"
    };

    // Clusters are numbered from 2, and the first two entries are reserved.
    let fat_at = (reserved * bps) as usize;
    let fat = data.get(fat_at..fat_at + (fat_sectors * bps) as usize);
    let mut used_clusters = 0u32;
    if let Some(fat) = fat {
        for n in 2..cluster_count + 2 {
            match fat_entry(fat, kind, n) {
                // A bad-cluster mark is not free space, so it counts as used.
                Some(v) if v != 0 => used_clusters += 1,
                Some(_) => {}
                None => break,
            }
        }
    }

    let label_at = if kind == "FAT32" { 71 } else { 43 };
    let cluster_bytes = bps as u32 * sectors_per_cluster as u32;
    let total_bytes = total_sectors * bps;
    let overhead_bytes = meta_sectors * bps;
    let used_bytes = overhead_bytes + used_clusters as u64 * cluster_bytes as u64;
    Some(FatInfo {
        kind,
        bytes_per_sector,
        sectors_per_cluster,
        cluster_bytes,
        total_sectors,
        cluster_count,
        used_clusters,
        total_bytes,
        used_bytes,
        free_bytes: total_bytes.saturating_sub(used_bytes),
        overhead_bytes,
        label: trimmed(boot.get(label_at..label_at + 11)?),
        oem: trimmed(boot.get(3..11)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A FAT16 volume: 16 MiB of 512-byte sectors, 4 per cluster. The cluster
    /// count is what makes it FAT16 rather than FAT12, so the geometry has to
    /// be large enough to land above the 4085 threshold.
    fn synth(clusters_used: u32) -> Vec<u8> {
        let (bps, spc, reserved, fats, root_entries, fat_sectors, total) =
            (512u64, 4u8, 1u64, 2u8, 512u64, 32u64, 32768u64);
        let mut img = vec![0u8; (total * bps) as usize];
        let b = &mut img[..512];
        b[0] = 0xEB;
        b[3..11].copy_from_slice(b"mkfs.fat");
        b[11..13].copy_from_slice(&(bps as u16).to_le_bytes());
        b[13] = spc;
        b[14..16].copy_from_slice(&(reserved as u16).to_le_bytes());
        b[16] = fats;
        b[17..19].copy_from_slice(&(root_entries as u16).to_le_bytes());
        b[19..21].copy_from_slice(&(total as u16).to_le_bytes());
        b[22..24].copy_from_slice(&(fat_sectors as u16).to_le_bytes());
        b[43..54].copy_from_slice(b"BOOT       ");
        b[510] = 0x55;
        b[511] = 0xAA;

        // Mark the first `clusters_used` data clusters allocated.
        let fat_at = (reserved * bps) as usize;
        for n in 0..clusters_used {
            let off = fat_at + (n as usize + 2) * 2;
            img[off..off + 2].copy_from_slice(&0xFFFFu16.to_le_bytes());
        }
        img
    }

    #[test]
    fn reads_geometry_label_and_real_usage() {
        let img = synth(100);
        let f = parse(&img).expect("fat");
        assert_eq!(f.kind, "FAT16");
        assert_eq!(f.bytes_per_sector, 512);
        assert_eq!(f.sectors_per_cluster, 4);
        assert_eq!(f.cluster_bytes, 2048);
        assert_eq!(f.total_sectors, 32768);
        assert_eq!(f.label, "BOOT");
        assert_eq!(f.oem, "mkfs.fat");
        assert_eq!(f.total_bytes, 32768 * 512);
        // reserved 1 + 2 FATs of 32 + 32 root-directory sectors
        assert_eq!(f.overhead_bytes, (1 + 64 + 32) * 512);
        assert_eq!(f.cluster_count, (32768 - 97) / 4);
        assert_eq!(f.used_clusters, 100);
        assert_eq!(f.used_bytes, 97 * 512 + 100 * 2048);
        // Used and free account for the whole volume between them.
        assert_eq!(f.used_bytes + f.free_bytes, f.total_bytes);
    }

    #[test]
    fn an_empty_volume_costs_only_its_metadata() {
        let f = parse(&synth(0)).expect("fat");
        assert_eq!(f.used_clusters, 0);
        assert_eq!(f.used_bytes, f.overhead_bytes);
        assert_eq!(f.used_bytes + f.free_bytes, f.total_bytes);
    }

    /// `mkfs.vfat -F 32` on a small volume builds a FAT32 with fewer clusters
    /// than the specification's own threshold, so the cluster count alone
    /// would read its 32-bit table as 16-bit entries.
    #[test]
    fn an_undersized_fat32_is_still_fat32() {
        let mut img = synth(0);
        img[22..24].copy_from_slice(&0u16.to_le_bytes()); // no 16-bit FAT
        img[36..40].copy_from_slice(&32u32.to_le_bytes()); // a 32-bit one
        img[17..19].copy_from_slice(&0u16.to_le_bytes()); // no fixed root dir
        let f = parse(&img).expect("fat");
        assert_eq!(f.kind, "FAT32");
        assert!(f.cluster_count < 65525, "the point of the case");
    }

    #[test]
    fn twelve_bit_entries_unpack_from_both_halves() {
        // 0x123 at cluster 2, 0x456 at cluster 3, sharing a middle byte.
        let fat = [0x23u8, 0x61, 0x45];
        assert_eq!(fat_entry(&fat, "FAT12", 0), Some(0x123));
        assert_eq!(fat_entry(&fat, "FAT12", 1), Some(0x456));
    }

    #[test]
    fn rejects_non_fat() {
        assert!(parse(&[0u8; 4096]).is_none());
        assert!(parse(&vec![0xFFu8; 4096]).is_none());
        // right signature, impossible geometry
        let mut bad = synth(0);
        bad[13] = 3; // clusters must be a power of two of sectors
        assert!(parse(&bad).is_none());
        let mut bad2 = synth(0);
        bad2[11..13].copy_from_slice(&999u16.to_le_bytes());
        assert!(parse(&bad2).is_none());
    }
}
