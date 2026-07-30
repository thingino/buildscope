//! Minimal MBR partition table reader for composite disk images
//! (sdcard.img and friends). Offsets and sizes in bytes (512-byte sectors).

use super::le_u32;

#[derive(Debug, Clone, PartialEq)]
pub struct MbrPartition {
    pub index: u8,
    pub part_type: u8,
    pub bootable: bool,
    pub offset: u64,
    pub size: u64,
}

pub fn parse(data: &[u8]) -> Option<Vec<MbrPartition>> {
    if data.len() < 512 || data[510] != 0x55 || data[511] != 0xAA {
        return None;
    }
    let mut parts = Vec::new();
    for i in 0..4u8 {
        let base = 446 + 16 * i as usize;
        let status = data[base];
        if status != 0x00 && status != 0x80 {
            return None; // not a valid MBR entry table
        }
        let part_type = data[base + 4];
        let lba = le_u32(data, base + 8)? as u64;
        let sectors = le_u32(data, base + 12)? as u64;
        if part_type == 0 || sectors == 0 {
            continue;
        }
        parts.push(MbrPartition {
            index: i + 1,
            part_type,
            bootable: status == 0x80,
            offset: lba * 512,
            size: sectors * 512,
        });
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sector_with(entries: &[(u8, u8, u32, u32)]) -> Vec<u8> {
        let mut d = vec![0u8; 512];
        d[510] = 0x55;
        d[511] = 0xAA;
        for (i, (status, ptype, lba, num)) in entries.iter().enumerate() {
            let base = 446 + 16 * i;
            d[base] = *status;
            d[base + 4] = *ptype;
            d[base + 8..base + 12].copy_from_slice(&lba.to_le_bytes());
            d[base + 12..base + 16].copy_from_slice(&num.to_le_bytes());
        }
        d
    }

    #[test]
    fn parses_two_partitions() {
        let d = sector_with(&[(0x80, 0x0C, 2048, 65536), (0x00, 0x83, 67584, 131072)]);
        let parts = parse(&d).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].offset, 2048 * 512);
        assert!(parts[0].bootable);
        assert_eq!(parts[1].size, 131072 * 512);
    }

    #[test]
    fn rejects_bad_signature_or_status() {
        assert!(parse(&[0u8; 512]).is_none());
        let mut d = sector_with(&[(0x80, 0x0C, 2048, 65536)]);
        d[446] = 0x42;
        assert!(parse(&d).is_none());
    }
}
