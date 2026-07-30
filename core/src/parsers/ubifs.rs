//! UBIFS superblock, and a listing of what the volume holds.
//!
//! The overlay volume of a NAND build is UBIFS. Its superblock sits at the
//! start of the first logical block and answers what a size report asks: how
//! large the filesystem is, how many blocks it reserves, what compression its
//! data uses, and how much it can grow into.
//!
//! The contents are listed by scanning for nodes rather than by walking the
//! index. UBIFS compresses *data* nodes only, so directory entries and inodes
//! are plain structures on the medium: every name and every size can be read
//! without a decompressor and without the wandering B-tree. Scanning sees
//! superseded and deleted entries too, which the sequence number in each node
//! resolves -- the newest node for a given name wins, and a directory entry
//! pointing at inode zero is a deletion.
//!
//! Layouts follow `fs/ubifs/ubifs-media.h`, all little endian. Every node's
//! CRC covers it from byte 8 onward, in the same convention UBI uses: seeded
//! with all ones and not inverted.

use super::{le_u16, le_u32, le_u64};
use crate::crc::crc32_raw;
use std::collections::HashMap;

pub const MAGIC: u32 = 0x0610_1831;
/// Node types from the on-medium enum.
const INO_NODE_TYPE: u8 = 0;
const DENT_NODE_TYPE: u8 = 2;
/// `UBIFS_SB_NODE` in the node-type enum.
const SB_NODE_TYPE: u8 = 6;
/// The superblock node is padded to a fixed 4 KiB.
const SB_NODE_SIZE: u32 = 4096;
/// Common header length, and the fixed part of the two nodes read here.
const CH_SZ: usize = 24;
const INO_NODE_SZ: usize = 160;
const DENT_NODE_SZ: usize = 56;
/// Nodes are aligned to 8 bytes, so nothing finer needs scanning.
const SCAN_STEP: usize = 8;
/// Same ceiling the other listing parsers use.
const MAX_ENTRIES: usize = 4000;

#[derive(Debug, Clone, PartialEq)]
pub struct UbifsEntry {
    pub path: String,
    pub bytes: u64,
    /// "file" | "dir" | "link" | "other"
    pub kind: &'static str,
}

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

/// A node header that passed its CRC.
struct Node {
    node_type: u8,
    sqnum: u64,
    len: usize,
}

fn node_at(data: &[u8], off: usize) -> Option<Node> {
    let head = data.get(off..off + CH_SZ)?;
    if le_u32(head, 0)? != MAGIC {
        return None;
    }
    let len = le_u32(head, 16)? as usize;
    if !(CH_SZ..=1 << 20).contains(&len) {
        return None;
    }
    let node = data.get(off..off + len)?;
    if crc32_raw(0xFFFF_FFFF, &node[8..]) != le_u32(node, 4)? {
        return None;
    }
    Some(Node {
        node_type: head[20],
        sqnum: le_u64(head, 8)?,
        len,
    })
}

fn kind_of_dent(t: u8) -> &'static str {
    match t {
        0 => "file",
        1 => "dir",
        2 => "link",
        _ => "other",
    }
}

/// One directory entry as found on the medium.
struct Dent {
    parent: u32,
    name: String,
    inum: u32,
    kind: &'static str,
    sqnum: u64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct UbifsListing {
    pub entries: Vec<UbifsEntry>,
    pub entries_truncated: bool,
    pub file_count: u32,
    pub dir_count: u32,
    pub link_count: u32,
    /// Sum of the sizes of the files that are still linked.
    pub logical_bytes: u64,
    /// Nodes whose CRC held, which is a measure of how much was readable.
    pub node_count: u32,
}

/// Reconstruct the volume's contents by scanning its nodes.
pub fn listing(data: &[u8]) -> UbifsListing {
    let mut dents: Vec<Dent> = Vec::new();
    // Newest inode wins, so keep the size that came with the highest sqnum.
    let mut inodes: HashMap<u32, (u64, u64)> = HashMap::new();
    let mut node_count = 0u32;

    let mut off = 0usize;
    while off + CH_SZ <= data.len() {
        let Some(node) = node_at(data, off) else {
            off += SCAN_STEP;
            continue;
        };
        node_count += 1;
        match node.node_type {
            DENT_NODE_TYPE if node.len >= DENT_NODE_SZ => {
                let d = &data[off..off + node.len];
                // The key's first word is the parent directory's inode.
                let parent = le_u32(d, CH_SZ).unwrap_or(0);
                let inum = le_u64(d, 40).unwrap_or(0) as u32;
                let nlen = le_u16(d, 50).unwrap_or(0) as usize;
                if let Some(raw) = d.get(DENT_NODE_SZ..DENT_NODE_SZ + nlen) {
                    dents.push(Dent {
                        parent,
                        name: String::from_utf8_lossy(raw).into_owned(),
                        inum,
                        kind: kind_of_dent(d[49]),
                        sqnum: node.sqnum,
                    });
                }
            }
            INO_NODE_TYPE if node.len >= INO_NODE_SZ => {
                let d = &data[off..off + node.len];
                let inum = le_u32(d, CH_SZ).unwrap_or(0);
                let size = le_u64(d, 48).unwrap_or(0);
                let e = inodes.entry(inum).or_insert((0, 0));
                if node.sqnum >= e.1 {
                    *e = (size, node.sqnum);
                }
            }
            _ => {}
        }
        // A node's own length is the only safe stride; anything else risks
        // finding a "node" inside a payload.
        off += node.len.max(SCAN_STEP).next_multiple_of(SCAN_STEP);
    }

    // The newest entry for a name wins, and a link to inode 0 is a deletion.
    let mut live: HashMap<(u32, String), &Dent> = HashMap::new();
    for d in &dents {
        let key = (d.parent, d.name.clone());
        match live.get(&key) {
            Some(prev) if prev.sqnum > d.sqnum => {}
            _ => {
                live.insert(key, d);
            }
        }
    }
    let live: Vec<&Dent> = live.into_values().filter(|d| d.inum != 0).collect();

    // Children by parent, so paths can be built from the root down.
    let mut children: HashMap<u32, Vec<&Dent>> = HashMap::new();
    for d in &live {
        children.entry(d.parent).or_default().push(d);
    }
    for v in children.values_mut() {
        v.sort_by(|a, b| a.name.cmp(&b.name));
    }

    let mut out = UbifsListing {
        node_count,
        ..Default::default()
    };
    // UBIFS numbers the root inode 1, the same as jffs2.
    let mut stack = vec![(1u32, String::new())];
    let mut seen: Vec<u32> = vec![1];
    while let Some((inum, prefix)) = stack.pop() {
        let Some(kids) = children.get(&inum) else {
            continue;
        };
        for d in kids {
            let path = format!("{prefix}/{}", d.name);
            let bytes = inodes.get(&d.inum).map(|(s, _)| *s).unwrap_or(0);
            match d.kind {
                "dir" => out.dir_count += 1,
                "link" => out.link_count += 1,
                "file" => {
                    out.file_count += 1;
                    out.logical_bytes += bytes;
                }
                _ => {}
            }
            if out.entries.len() < MAX_ENTRIES {
                out.entries.push(UbifsEntry {
                    path: path.clone(),
                    bytes: if d.kind == "dir" { 0 } else { bytes },
                    kind: d.kind,
                });
            } else {
                out.entries_truncated = true;
            }
            // A hard link cycle would otherwise walk forever.
            if d.kind == "dir" && !seen.contains(&d.inum) {
                seen.push(d.inum);
                stack.push((d.inum, path));
            }
        }
    }
    out.entries.sort_by(|a, b| a.path.cmp(&b.path));
    out
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

    /// Build the two node kinds a listing is made of.
    fn ch(node_type: u8, sqnum: u64, len: usize) -> Vec<u8> {
        let mut n = vec![0u8; len];
        n[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        n[8..16].copy_from_slice(&sqnum.to_le_bytes());
        n[16..20].copy_from_slice(&(len as u32).to_le_bytes());
        n[20] = node_type;
        n
    }

    fn seal(mut n: Vec<u8>) -> Vec<u8> {
        let crc = crc32_raw(0xFFFF_FFFF, &n[8..]);
        n[4..8].copy_from_slice(&crc.to_le_bytes());
        n
    }

    fn dent(parent: u32, name: &str, inum: u32, t: u8, sqnum: u64) -> Vec<u8> {
        let mut n = ch(DENT_NODE_TYPE, sqnum, DENT_NODE_SZ + name.len());
        n[CH_SZ..CH_SZ + 4].copy_from_slice(&parent.to_le_bytes());
        n[40..48].copy_from_slice(&(inum as u64).to_le_bytes());
        n[49] = t;
        n[50..52].copy_from_slice(&(name.len() as u16).to_le_bytes());
        n[DENT_NODE_SZ..].copy_from_slice(name.as_bytes());
        seal(n)
    }

    fn ino(inum: u32, size: u64, sqnum: u64) -> Vec<u8> {
        let mut n = ch(INO_NODE_TYPE, sqnum, INO_NODE_SZ);
        n[CH_SZ..CH_SZ + 4].copy_from_slice(&inum.to_le_bytes());
        n[48..56].copy_from_slice(&size.to_le_bytes());
        seal(n)
    }

    fn volume(nodes: Vec<Vec<u8>>) -> Vec<u8> {
        let mut v = Vec::new();
        for n in nodes {
            v.extend_from_slice(&n);
            while v.len() % 8 != 0 {
                v.push(0xFF);
            }
        }
        v
    }

    #[test]
    fn reconstructs_paths_and_sizes_from_nodes() {
        let img = volume(vec![
            ino(1, 0, 1),                       // root
            dent(1, "etc", 2, 1, 2),            // /etc
            ino(2, 0, 3),
            dent(2, "passwd", 3, 0, 4),         // /etc/passwd
            ino(3, 1234, 5),
            dent(1, "bin", 4, 1, 6),            // /bin
            ino(4, 0, 7),
            dent(4, "sh", 5, 2, 8),             // /bin/sh -> symlink
            ino(5, 7, 9),
        ]);
        let l = listing(&img);
        let paths: Vec<&str> = l.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["/bin", "/bin/sh", "/etc", "/etc/passwd"]);
        assert_eq!(l.dir_count, 2);
        assert_eq!(l.file_count, 1);
        assert_eq!(l.link_count, 1);
        assert_eq!(l.logical_bytes, 1234);
        let by = |p: &str| l.entries.iter().find(|e| e.path == p).unwrap();
        assert_eq!(by("/etc/passwd").bytes, 1234);
        assert_eq!(by("/etc/passwd").kind, "file");
        assert_eq!(by("/bin/sh").kind, "link");
        assert_eq!(by("/etc").bytes, 0);
    }

    /// Scanning sees history, not just the current state: a rewritten file and
    /// a deleted one both leave their old nodes behind.
    #[test]
    fn newer_nodes_supersede_older_ones() {
        let img = volume(vec![
            ino(1, 0, 1),
            dent(1, "a", 2, 0, 2),
            ino(2, 100, 3),
            ino(2, 5000, 9), // a grew
            dent(1, "gone", 3, 0, 4),
            ino(3, 42, 5),
            dent(1, "gone", 0, 0, 10), // ...then was unlinked
        ]);
        let l = listing(&img);
        let paths: Vec<&str> = l.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["/a"], "the deleted name must not be listed");
        assert_eq!(l.entries[0].bytes, 5000, "the newest inode size wins");
        assert_eq!(l.file_count, 1);
    }

    #[test]
    fn a_volume_with_no_nodes_lists_nothing() {
        let l = listing(&vec![0xFFu8; 8192]);
        assert!(l.entries.is_empty());
        assert_eq!(l.node_count, 0);
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
