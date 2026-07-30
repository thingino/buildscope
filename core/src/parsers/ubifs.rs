//! UBIFS superblock reader.
//!
//! The overlay volume of a NAND build is UBIFS, whose contents live in a
//! wandering B-tree with compressed nodes: listing files means implementing
//! the index and a decompressor. The superblock alone is uncompressed, sits at
//! the start of the first logical block, and answers the questions a size
//! report asks: how large the filesystem is, how many blocks it reserves, what
//! compression its data uses, and how much it can grow into.
//!
//! Layout is `struct ubifs_sb_node` from Linux's `fs/ubifs/ubifs-media.h`, all
//! little endian. The common header CRC covers everything after the magic and
//! the CRC field itself, in the same convention UBI uses: seeded with all ones
//! and not inverted.

use super::{le_u16, le_u32, le_u64};
use crate::crc::crc32_raw;

pub const MAGIC: u32 = 0x0610_1831;
/// `UBIFS_SB_NODE` in the node-type enum.
const SB_NODE_TYPE: u8 = 6;
/// The superblock node is padded to a fixed 4 KiB.
const SB_NODE_SIZE: u32 = 4096;

fn compression_name(id: u16) -> &'static str {
    match id {
        0 => "none",
        1 => "lzo",
        2 => "zlib",
        3 => "zstd",
        _ => "unknown",
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UbifsInfo {
    pub leb_size: u32,
    pub leb_count: u32,
    pub max_leb_count: u32,
    pub min_io_size: u32,
    pub log_lebs: u32,
    pub lpt_lebs: u32,
    pub orph_lebs: u32,
    pub format_version: u32,
    pub default_compression: &'static str,
    pub uuid: String,
    /// Space the filesystem claims: every logical block it was formatted for.
    pub total_bytes: u64,
    /// What it would claim after growing into its volume, when autoresize is
    /// set: `mkfs.ubifs` writes the smaller size and the kernel expands on
    /// first mount.
    pub max_bytes: u64,
    pub crc_ok: bool,
}

fn format_uuid(b: &[u8]) -> String {
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    if hex.len() != 32 {
        return hex;
    }
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

pub fn parse(data: &[u8]) -> Option<UbifsInfo> {
    if le_u32(data, 0)? != MAGIC {
        return None;
    }
    if *data.get(20)? != SB_NODE_TYPE {
        return None;
    }
    let len = le_u32(data, 16)?;
    if len != SB_NODE_SIZE {
        return None;
    }
    let node = data.get(..len as usize)?;
    let crc_ok = crc32_raw(0xFFFF_FFFF, &node[8..]) == le_u32(node, 4)?;

    let leb_size = le_u32(node, 36)?;
    let leb_count = le_u32(node, 40)?;
    let max_leb_count = le_u32(node, 44)?;
    // A plausible geometry: UBIFS needs a handful of blocks to exist at all,
    // and its block size is the containing volume's.
    if leb_size < 4096 || leb_count < 3 {
        return None;
    }

    Some(UbifsInfo {
        leb_size,
        leb_count,
        max_leb_count,
        min_io_size: le_u32(node, 32)?,
        log_lebs: le_u32(node, 56)?,
        lpt_lebs: le_u32(node, 60)?,
        orph_lebs: le_u32(node, 64)?,
        format_version: le_u32(node, 80)?,
        default_compression: compression_name(le_u16(node, 84)?),
        uuid: format_uuid(node.get(108..124)?),
        total_bytes: leb_size as u64 * leb_count as u64,
        max_bytes: leb_size as u64 * max_leb_count as u64,
        crc_ok,
    })
}

/// Reserved-space fields the report does not surface but the parser reads, kept
/// so a future consumer does not have to rediscover the offsets.
pub fn max_bud_bytes(data: &[u8]) -> Option<u64> {
    (le_u32(data, 0)? == MAGIC).then(|| le_u64(data, 48)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sb_node(leb_size: u32, leb_cnt: u32, max_leb_cnt: u32, compr: u16) -> Vec<u8> {
        let mut n = vec![0u8; SB_NODE_SIZE as usize];
        n[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        n[8..16].copy_from_slice(&1u64.to_le_bytes()); // sqnum
        n[16..20].copy_from_slice(&SB_NODE_SIZE.to_le_bytes());
        n[20] = SB_NODE_TYPE;
        n[26] = 0; // key_hash
        n[32..36].copy_from_slice(&2048u32.to_le_bytes()); // min_io_size
        n[36..40].copy_from_slice(&leb_size.to_le_bytes());
        n[40..44].copy_from_slice(&leb_cnt.to_le_bytes());
        n[44..48].copy_from_slice(&max_leb_cnt.to_le_bytes());
        n[48..56].copy_from_slice(&(8u64 << 20).to_le_bytes()); // max_bud_bytes
        n[56..60].copy_from_slice(&4u32.to_le_bytes()); // log_lebs
        n[60..64].copy_from_slice(&2u32.to_le_bytes()); // lpt_lebs
        n[64..68].copy_from_slice(&1u32.to_le_bytes()); // orph_lebs
        n[80..84].copy_from_slice(&4u32.to_le_bytes()); // fmt_version
        n[84..86].copy_from_slice(&compr.to_le_bytes());
        for (i, b) in n[108..124].iter_mut().enumerate() {
            *b = i as u8;
        }
        let crc = crc32_raw(0xFFFF_FFFF, &n[8..]);
        n[4..8].copy_from_slice(&crc.to_le_bytes());
        n
    }

    #[test]
    fn reads_geometry_and_compression() {
        let n = sb_node(126_976, 40, 200, 1);
        let info = parse(&n).expect("superblock");
        assert!(info.crc_ok);
        assert_eq!(info.leb_size, 126_976);
        assert_eq!(info.leb_count, 40);
        assert_eq!(info.max_leb_count, 200);
        assert_eq!(info.min_io_size, 2048);
        assert_eq!(info.format_version, 4);
        assert_eq!(info.default_compression, "lzo");
        assert_eq!(info.total_bytes, 126_976 * 40);
        assert_eq!(info.max_bytes, 126_976 * 200);
        assert_eq!(info.uuid, "00010203-0405-0607-0809-0a0b0c0d0e0f");
        assert_eq!(max_bud_bytes(&n), Some(8 << 20));
    }

    #[test]
    fn a_corrupt_node_still_parses_but_says_so() {
        let mut n = sb_node(126_976, 40, 40, 2);
        n[2000] ^= 0xFF;
        let info = parse(&n).expect("superblock");
        assert!(!info.crc_ok);
        assert_eq!(info.default_compression, "zlib");
    }

    #[test]
    fn rejects_other_node_types_and_junk() {
        let mut mst = sb_node(126_976, 40, 40, 0);
        mst[20] = 7; // master node
        assert!(parse(&mst).is_none());

        let mut short = sb_node(126_976, 40, 40, 0);
        short[16..20].copy_from_slice(&512u32.to_le_bytes());
        assert!(parse(&short).is_none());

        assert!(parse(&[0u8; 4096]).is_none());
        assert!(parse(&vec![0xFFu8; 8192]).is_none());
        // Magic and type right, geometry impossible.
        assert!(parse(&sb_node(512, 40, 40, 0)).is_none());
        assert!(parse(&sb_node(126_976, 1, 40, 0)).is_none());
    }
}
