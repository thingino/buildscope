//! squashfs v4 superblock. 96 bytes at offset 0, little endian. This gives
//! the truth a bare file size cannot: real bytes used (images are commonly
//! padded), compression algorithm, block size, inode count.

use super::{le_u16, le_u32, le_u64};

pub const MAGIC: u32 = 0x7371_7368; // "hsqs" read little-endian

#[derive(Debug, Clone, PartialEq)]
pub struct SquashfsInfo {
    pub bytes_used: u64,
    pub inode_count: u32,
    pub fragment_count: u32,
    pub block_size: u32,
    pub compression: String,
    pub version_major: u16,
    pub version_minor: u16,
    pub mod_time: u32,
}

fn compressor_name(id: u16) -> Option<&'static str> {
    Some(match id {
        1 => "gzip",
        2 => "lzma",
        3 => "lzo",
        4 => "xz",
        5 => "lz4",
        6 => "zstd",
        _ => return None,
    })
}

pub fn parse(data: &[u8]) -> Option<SquashfsInfo> {
    if le_u32(data, 0)? != MAGIC {
        return None;
    }
    let inode_count = le_u32(data, 4)?;
    let mod_time = le_u32(data, 8)?;
    let block_size = le_u32(data, 12)?;
    let fragment_count = le_u32(data, 16)?;
    let compression_id = le_u16(data, 20)?;
    let block_log = le_u16(data, 22)?;
    let version_major = le_u16(data, 28)?;
    let version_minor = le_u16(data, 30)?;
    let bytes_used = le_u64(data, 40)?;

    // Sanity: v4 only, and the superblock's own consistency invariant.
    if version_major != 4 {
        return None;
    }
    if block_log >= 32 || (1u32 << block_log) != block_size {
        return None;
    }
    if bytes_used as usize > data.len() {
        return None;
    }
    let compression = match compressor_name(compression_id) {
        Some(name) => name.to_string(),
        None => format!("unknown({compression_id})"),
    };

    Some(SquashfsInfo {
        bytes_used,
        inode_count,
        fragment_count,
        block_size,
        compression,
        version_major,
        version_minor,
        mod_time,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_superblock() -> Vec<u8> {
        let mut d = vec![0u8; 4096];
        d[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        d[4..8].copy_from_slice(&123u32.to_le_bytes()); // inodes
        d[8..12].copy_from_slice(&1_700_000_000u32.to_le_bytes()); // mod_time
        d[12..16].copy_from_slice(&131_072u32.to_le_bytes()); // block_size
        d[16..20].copy_from_slice(&7u32.to_le_bytes()); // fragments
        d[20..22].copy_from_slice(&4u16.to_le_bytes()); // xz
        d[22..24].copy_from_slice(&17u16.to_le_bytes()); // block_log
        d[28..30].copy_from_slice(&4u16.to_le_bytes()); // major
        d[30..32].copy_from_slice(&0u16.to_le_bytes()); // minor
        d[40..48].copy_from_slice(&3000u64.to_le_bytes()); // bytes_used
        d
    }

    #[test]
    fn parses_synthetic() {
        let info = parse(&synthetic_superblock()).unwrap();
        assert_eq!(info.bytes_used, 3000);
        assert_eq!(info.inode_count, 123);
        assert_eq!(info.compression, "xz");
        assert_eq!(info.block_size, 131_072);
    }

    #[test]
    fn rejects_wrong_magic() {
        let mut d = synthetic_superblock();
        d[0] = 0;
        assert!(parse(&d).is_none());
    }

    #[test]
    fn rejects_inconsistent_block_log() {
        let mut d = synthetic_superblock();
        d[22..24].copy_from_slice(&12u16.to_le_bytes()); // 4096 != 131072
        assert!(parse(&d).is_none());
    }
}
