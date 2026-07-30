//! JFFS2 node-level scan. A jffs2 image is a sequence of 4-byte-aligned
//! nodes, each starting with magic 0x1985, plus 0xFF free space. Because
//! images are habitually padded to the full partition size, the file size
//! says nothing about content; walking nodes yields the real numbers.
//!
//! Node header (12 bytes): magic u16, nodetype u16, totlen u32, hdr_crc u32
//! where hdr_crc covers the first 8 bytes with the JFFS2 CRC convention
//! (seed 0, no inversion).

use crate::crc::crc32_jffs2;
use std::collections::HashMap;

pub const MAGIC: u16 = 0x1985;

const NODETYPE_DIRENT: u16 = 0xE001;
const NODETYPE_INODE: u16 = 0xE002;
const NODETYPE_CLEANMARKER: u16 = 0x2003;
const NODETYPE_PADDING: u16 = 0x2004;
const NODETYPE_SUMMARY: u16 = 0x2006;

const DT_DIR: u8 = 4;
const DT_REG: u8 = 8;

#[derive(Debug, Clone, PartialEq)]
pub struct Jffs2Info {
    pub total_bytes: u64,
    /// Bytes occupied by valid nodes (aligned), i.e. real content + fs metadata.
    pub used_bytes: u64,
    /// Bytes in 0xFF runs: genuinely free space.
    pub free_bytes: u64,
    /// Bytes in regions that are neither valid nodes nor 0xFF.
    pub dirty_bytes: u64,
    pub node_count: u32,
    pub inode_nodes: u32,
    pub dirent_nodes: u32,
    pub clean_markers: u32,
    pub summary_nodes: u32,
    pub crc_errors: u32,
    /// Live directory entries by latest version: regular files.
    pub live_files: u32,
    /// Live directory entries by latest version: directories.
    pub live_dirs: u32,
    /// Live directory entries of other types (symlinks, devices, ...).
    pub live_other: u32,
    /// Sum of the latest known logical file size (isize) per inode.
    pub logical_content_bytes: u64,
    pub endianness: &'static str,
}

fn align4(x: u64) -> u64 {
    (x + 3) & !3
}

struct Reader {
    big_endian: bool,
}

impl Reader {
    fn u16(&self, d: &[u8], o: usize) -> Option<u16> {
        let b = d.get(o..o + 2)?;
        Some(if self.big_endian {
            u16::from_be_bytes([b[0], b[1]])
        } else {
            u16::from_le_bytes([b[0], b[1]])
        })
    }
    fn u32(&self, d: &[u8], o: usize) -> Option<u32> {
        let b = d.get(o..o + 4)?;
        Some(if self.big_endian {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        })
    }
}

fn scan(data: &[u8], big_endian: bool) -> Jffs2Info {
    let r = Reader { big_endian };
    let len = data.len();
    let mut info = Jffs2Info {
        total_bytes: len as u64,
        used_bytes: 0,
        free_bytes: 0,
        dirty_bytes: 0,
        node_count: 0,
        inode_nodes: 0,
        dirent_nodes: 0,
        clean_markers: 0,
        summary_nodes: 0,
        crc_errors: 0,
        live_files: 0,
        live_dirs: 0,
        live_other: 0,
        logical_content_bytes: 0,
        endianness: if big_endian { "big" } else { "little" },
    };

    // (pino, name) -> (version, ino, dtype); highest version wins.
    let mut dirents: HashMap<(u32, Vec<u8>), (u32, u32, u8)> = HashMap::new();
    // ino -> (version, isize); highest version wins.
    let mut isizes: HashMap<u32, (u32, u32)> = HashMap::new();

    let mut pos: usize = 0;
    while pos < len {
        if data[pos] == 0xFF {
            let mut end = pos;
            while end < len && data[end] == 0xFF {
                end += 1;
            }
            let run = (end - pos) as u64;
            if run >= 8 {
                info.free_bytes += run;
            } else {
                info.dirty_bytes += run;
            }
            // Nodes are 4-aligned; consume stray unaligned bytes as dirty.
            let aligned = ((end + 3) & !3).min(len);
            info.dirty_bytes += (aligned - end) as u64;
            pos = aligned;
            continue;
        }

        let header_ok = (|| {
            let magic = r.u16(data, pos)?;
            if magic != MAGIC {
                return None;
            }
            let nodetype = r.u16(data, pos + 2)?;
            let totlen = r.u32(data, pos + 4)? as u64;
            let hdr_crc = r.u32(data, pos + 8)?;
            if totlen < 12 || pos as u64 + totlen > len as u64 {
                return None;
            }
            let computed = crc32_jffs2(&data[pos..pos + 8]);
            if computed != hdr_crc {
                info.crc_errors += 1;
                return None;
            }
            Some((nodetype, totlen))
        })();

        match header_ok {
            Some((nodetype, totlen)) => {
                info.node_count += 1;
                info.used_bytes += align4(totlen).min(len as u64 - pos as u64);
                match nodetype {
                    NODETYPE_INODE => {
                        info.inode_nodes += 1;
                        // ino u32 @12, version u32 @16, isize u32 @28
                        if totlen >= 32 {
                            if let (Some(ino), Some(version), Some(isize)) = (
                                r.u32(data, pos + 12),
                                r.u32(data, pos + 16),
                                r.u32(data, pos + 28),
                            ) {
                                let e = isizes.entry(ino).or_insert((0, 0));
                                if version >= e.0 {
                                    *e = (version, isize);
                                }
                            }
                        }
                    }
                    NODETYPE_DIRENT => {
                        info.dirent_nodes += 1;
                        // pino @12, version @16, ino @20, nsize u8 @28,
                        // type u8 @29, name @40.
                        if let (Some(pino), Some(version), Some(ino)) = (
                            r.u32(data, pos + 12),
                            r.u32(data, pos + 16),
                            r.u32(data, pos + 20),
                        ) {
                            let nsize =
                                data.get(pos + 28).copied().unwrap_or(0) as u64;
                            let dtype = data.get(pos + 29).copied().unwrap_or(0);
                            if totlen >= 40 + nsize {
                                if let Some(name) =
                                    data.get(pos + 40..pos + 40 + nsize as usize)
                                {
                                    let e = dirents
                                        .entry((pino, name.to_vec()))
                                        .or_insert((0, 0, 0));
                                    if version >= e.0 {
                                        *e = (version, ino, dtype);
                                    }
                                }
                            }
                        }
                    }
                    NODETYPE_CLEANMARKER => info.clean_markers += 1,
                    NODETYPE_SUMMARY => info.summary_nodes += 1,
                    NODETYPE_PADDING => {}
                    _ => {}
                }
                pos += align4(totlen) as usize;
            }
            None => {
                let step = 4.min(len - pos);
                info.dirty_bytes += step as u64;
                pos += step;
            }
        }
    }

    for (_, (_, ino, dtype)) in &dirents {
        if *ino == 0 {
            continue; // deletion dirent
        }
        match *dtype {
            DT_REG => info.live_files += 1,
            DT_DIR => info.live_dirs += 1,
            _ => info.live_other += 1,
        }
    }
    for (_, (_, isize)) in &isizes {
        info.logical_content_bytes += *isize as u64;
    }
    info
}

pub fn parse(data: &[u8]) -> Option<Jffs2Info> {
    if data.len() < 12 {
        return None;
    }
    // Cheap pre-check: a jffs2 image starts with a node or 0xFF padding.
    let le_hit = data[0] == 0x85 && data[1] == 0x19;
    let be_hit = data[0] == 0x19 && data[1] == 0x85;
    if !le_hit && !be_hit && data[0] != 0xFF {
        return None;
    }
    let first = scan(data, be_hit && !le_hit);
    let info = if first.node_count == 0 && !be_hit {
        let second = scan(data, true);
        if second.node_count > 0 {
            second
        } else {
            first
        }
    } else {
        first
    };
    if info.node_count == 0 {
        return None;
    }
    // Refuse the label if most of the image is unexplained garbage.
    if info.dirty_bytes > info.total_bytes / 2 {
        return None;
    }
    Some(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crc::crc32_jffs2;

    fn push_header(buf: &mut Vec<u8>, nodetype: u16, totlen: u32) {
        let start = buf.len();
        buf.extend_from_slice(&MAGIC.to_le_bytes());
        buf.extend_from_slice(&nodetype.to_le_bytes());
        buf.extend_from_slice(&totlen.to_le_bytes());
        let crc = crc32_jffs2(&buf[start..start + 8]);
        buf.extend_from_slice(&crc.to_le_bytes());
    }

    fn synthetic_image() -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();
        // Cleanmarker: totlen 12.
        push_header(&mut buf, NODETYPE_CLEANMARKER, 12);

        // Dirent: "hello", ino 2, pino 1, version 1, DT_REG. totlen 45.
        push_header(&mut buf, NODETYPE_DIRENT, 45);
        buf.extend_from_slice(&1u32.to_le_bytes()); // pino
        buf.extend_from_slice(&1u32.to_le_bytes()); // version
        buf.extend_from_slice(&2u32.to_le_bytes()); // ino
        buf.extend_from_slice(&0u32.to_le_bytes()); // mctime
        buf.push(5); // nsize
        buf.push(DT_REG); // type
        buf.extend_from_slice(&[0, 0]); // unused
        buf.extend_from_slice(&0u32.to_le_bytes()); // node_crc (unchecked)
        buf.extend_from_slice(&0u32.to_le_bytes()); // name_crc (unchecked)
        buf.extend_from_slice(b"hello");
        while buf.len() % 4 != 0 {
            buf.push(0xFF);
        }

        // Inode: ino 2, version 1, isize 123. totlen 68 (no payload).
        push_header(&mut buf, NODETYPE_INODE, 68);
        buf.extend_from_slice(&2u32.to_le_bytes()); // ino
        buf.extend_from_slice(&1u32.to_le_bytes()); // version
        buf.extend_from_slice(&0o100644u32.to_le_bytes()); // mode
        buf.extend_from_slice(&[0; 4]); // uid+gid
        buf.extend_from_slice(&123u32.to_le_bytes()); // isize @28
        buf.resize(buf.len() + 68 - 32, 0); // rest of the raw inode

        // Pad to a 64 KiB erase block with 0xFF.
        buf.resize(65536, 0xFF);
        buf
    }

    #[test]
    fn parses_synthetic() {
        let info = parse(&synthetic_image()).unwrap();
        assert_eq!(info.node_count, 3);
        assert_eq!(info.clean_markers, 1);
        assert_eq!(info.dirent_nodes, 1);
        assert_eq!(info.inode_nodes, 1);
        assert_eq!(info.live_files, 1);
        assert_eq!(info.live_dirs, 0);
        assert_eq!(info.logical_content_bytes, 123);
        assert_eq!(info.used_bytes, 12 + 48 + 68);
        assert_eq!(info.crc_errors, 0);
        assert_eq!(
            info.used_bytes + info.free_bytes + info.dirty_bytes,
            65536
        );
        assert_eq!(info.endianness, "little");
    }

    #[test]
    fn deletion_dirent_not_live() {
        let mut buf: Vec<u8> = Vec::new();
        push_header(&mut buf, NODETYPE_DIRENT, 45);
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&2u32.to_le_bytes()); // version 2
        buf.extend_from_slice(&0u32.to_le_bytes()); // ino 0: deletion
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.push(5);
        buf.push(DT_REG);
        buf.extend_from_slice(&[0, 0]);
        buf.extend_from_slice(&[0; 8]);
        buf.extend_from_slice(b"hello");
        buf.resize(4096, 0xFF);
        let info = parse(&buf).unwrap();
        assert_eq!(info.live_files, 0);
    }

    #[test]
    fn rejects_non_jffs2() {
        assert!(parse(&[0u8; 4096]).is_none());
        let mut d = vec![0xFFu8; 4096];
        assert!(parse(&d).is_none()); // all free, zero nodes: unprovable
        d[0] = 0x85;
        d[1] = 0x19;
        // Magic but garbage CRC everywhere: dirty-dominated, rejected.
        assert!(parse(&d).is_none());
    }
}
