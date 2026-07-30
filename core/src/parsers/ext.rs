//! ext2/ext3/ext4 superblock.
//!
//! Buildroot's most common rootfs, and the one place a size report most wants
//! to look: the superblock counts free blocks directly, so used-vs-free is
//! exact and needs no decompression and no walk of the filesystem.
//!
//! Layout is `struct ext4_super_block` from Linux's `fs/ext4/ext4.h`, little
//! endian throughout, always 1024 bytes into the filesystem.

use super::{le_u16, le_u32};

pub const MAGIC: u16 = 0xEF53;
/// The superblock never moves: 1024 bytes in, whatever the block size.
const SB_OFFSET: usize = 1024;

// Feature bits, enough to tell the three generations apart.
const COMPAT_HAS_JOURNAL: u32 = 0x0004;
const INCOMPAT_EXTENTS: u32 = 0x0040;
const INCOMPAT_64BIT: u32 = 0x0080;
const INCOMPAT_FLEX_BG: u32 = 0x0200;

#[derive(Debug, Clone, PartialEq)]
pub struct ExtInfo {
    /// "ext2" | "ext3" | "ext4"
    pub version: &'static str,
    pub block_size: u32,
    pub block_count: u64,
    pub free_blocks: u64,
    /// Blocks set aside for root, which are free but not available.
    pub reserved_blocks: u64,
    pub inode_count: u32,
    pub free_inodes: u32,
    pub inode_size: u16,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub label: String,
    pub uuid: String,
    /// True when the filesystem was unmounted cleanly.
    pub clean: bool,
}

fn c_string(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
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

pub fn parse(data: &[u8]) -> Option<ExtInfo> {
    let sb = data.get(SB_OFFSET..SB_OFFSET + 1024)?;
    if le_u16(sb, 56)? != MAGIC {
        return None;
    }

    // 1024 << s_log_block_size, and ext tops out at 64 KiB blocks.
    let log_block_size = le_u32(sb, 24)?;
    if log_block_size > 6 {
        return None;
    }
    let block_size = 1024u32 << log_block_size;

    let compat = le_u32(sb, 92)?;
    let incompat = le_u32(sb, 96)?;

    // The high halves only exist once the 64bit feature is set; reading them
    // unconditionally would pick up whatever those bytes held on an ext2.
    let wide = incompat & INCOMPAT_64BIT != 0;
    let hi = |off: usize| -> u64 {
        if wide {
            (le_u32(sb, off).unwrap_or(0) as u64) << 32
        } else {
            0
        }
    };
    let block_count = le_u32(sb, 4)? as u64 | hi(0x150);
    let reserved_blocks = le_u32(sb, 8)? as u64 | hi(0x154);
    let free_blocks = le_u32(sb, 12)? as u64 | hi(0x158);

    if block_count == 0 || free_blocks > block_count {
        return None;
    }

    let version = if incompat & (INCOMPAT_EXTENTS | INCOMPAT_64BIT | INCOMPAT_FLEX_BG) != 0 {
        "ext4"
    } else if compat & COMPAT_HAS_JOURNAL != 0 {
        "ext3"
    } else {
        "ext2"
    };

    let bs = block_size as u64;
    Some(ExtInfo {
        version,
        block_size,
        block_count,
        free_blocks,
        reserved_blocks,
        inode_count: le_u32(sb, 0)?,
        free_inodes: le_u32(sb, 16)?,
        inode_size: le_u16(sb, 88).filter(|&s| s >= 128).unwrap_or(128),
        total_bytes: block_count * bs,
        used_bytes: (block_count - free_blocks) * bs,
        free_bytes: free_blocks * bs,
        label: c_string(sb.get(120..136)?),
        uuid: format_uuid(sb.get(104..120)?),
        // s_state bit 0 is EXT2_VALID_FS.
        clean: le_u16(sb, 58)? & 1 != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sb(mut f: impl FnMut(&mut [u8])) -> Vec<u8> {
        let mut img = vec![0u8; 4096];
        let s = &mut img[SB_OFFSET..SB_OFFSET + 1024];
        s[56..58].copy_from_slice(&MAGIC.to_le_bytes());
        s[0..4].copy_from_slice(&2048u32.to_le_bytes()); // inodes
        s[4..8].copy_from_slice(&8192u32.to_le_bytes()); // blocks
        s[8..12].copy_from_slice(&409u32.to_le_bytes()); // reserved
        s[12..16].copy_from_slice(&7000u32.to_le_bytes()); // free blocks
        s[16..20].copy_from_slice(&2037u32.to_le_bytes()); // free inodes
        s[24..28].copy_from_slice(&0u32.to_le_bytes()); // 1024-byte blocks
        s[58..60].copy_from_slice(&1u16.to_le_bytes()); // clean
        s[88..90].copy_from_slice(&256u16.to_le_bytes()); // inode size
        s[120..126].copy_from_slice(b"rootfs");
        for (i, b) in s[104..120].iter_mut().enumerate() {
            *b = i as u8;
        }
        f(s);
        img
    }

    #[test]
    fn reads_geometry_and_usage() {
        let img = sb(|_| {});
        let i = parse(&img).expect("superblock");
        assert_eq!(i.version, "ext2");
        assert_eq!(i.block_size, 1024);
        assert_eq!(i.block_count, 8192);
        assert_eq!(i.free_blocks, 7000);
        assert_eq!(i.total_bytes, 8192 * 1024);
        assert_eq!(i.used_bytes, (8192 - 7000) * 1024);
        assert_eq!(i.free_bytes, 7000 * 1024);
        assert_eq!(i.reserved_blocks, 409);
        assert_eq!(i.inode_count, 2048);
        assert_eq!(i.free_inodes, 2037);
        assert_eq!(i.inode_size, 256);
        assert_eq!(i.label, "rootfs"); // NUL-terminated within its 16 bytes
        assert_eq!(i.uuid, "00010203-0405-0607-0809-0a0b0c0d0e0f");
        assert!(i.clean);
    }

    #[test]
    fn tells_the_three_generations_apart() {
        let ext3 = sb(|s| s[92..96].copy_from_slice(&COMPAT_HAS_JOURNAL.to_le_bytes()));
        assert_eq!(parse(&ext3).unwrap().version, "ext3");

        let ext4 = sb(|s| {
            s[92..96].copy_from_slice(&COMPAT_HAS_JOURNAL.to_le_bytes());
            s[96..100].copy_from_slice(&INCOMPAT_EXTENTS.to_le_bytes());
        });
        assert_eq!(parse(&ext4).unwrap().version, "ext4");

        // A journal-less ext4 is still ext4 if it uses ext4-only layout.
        let flex = sb(|s| s[96..100].copy_from_slice(&INCOMPAT_FLEX_BG.to_le_bytes()));
        assert_eq!(parse(&flex).unwrap().version, "ext4");
    }

    /// The high halves of the block counts are only real with the 64bit
    /// feature; on an ext2 those bytes hold something else entirely.
    #[test]
    fn high_block_counts_need_the_64bit_feature() {
        let junk = sb(|s| s[0x150..0x154].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes()));
        assert_eq!(parse(&junk).unwrap().block_count, 8192);

        let wide = sb(|s| {
            s[96..100].copy_from_slice(&INCOMPAT_64BIT.to_le_bytes());
            s[0x150..0x154].copy_from_slice(&1u32.to_le_bytes());
        });
        let i = parse(&wide).unwrap();
        assert_eq!(i.block_count, (1u64 << 32) | 8192);
        assert_eq!(i.version, "ext4");
    }

    #[test]
    fn rejects_non_ext() {
        assert!(parse(&[0u8; 4096]).is_none());
        assert!(parse(&vec![0xFFu8; 8192]).is_none());
        assert!(parse(&[0u8; 512]).is_none()); // too short to hold one
                                               // magic right, geometry impossible
        assert!(parse(&sb(|s| s[24..28].copy_from_slice(&9u32.to_le_bytes()))).is_none());
        assert!(parse(&sb(|s| s[12..16].copy_from_slice(&99999u32.to_le_bytes()))).is_none());
    }
}
