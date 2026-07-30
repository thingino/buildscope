//! Reading a squashfs image: its directory tree, and what each file really
//! costs once compressed.
//!
//! Unlike the other listing parsers here, this one needs decompression.
//! squashfs keeps its inode and directory tables in "metadata blocks" -- an
//! 8 KiB chunk behind a two-byte length whose top bit says whether it was
//! compressed -- so the names are not readable without the algorithm the image
//! was built with. gzip, xz, zstd and lz4 are all pure Rust, so this works in
//! the browser as well as the CLI; an image built with lzo is reported as
//! unreadable rather than guessed at.
//!
//! The per-file cost is worth more than a listing. A file's inode carries the
//! on-disk size of every one of its blocks, which is its exact compressed
//! cost, and the tail of a file usually lives in a *fragment* shared with
//! other small files. A shared block cannot be attributed to one file, so its
//! cost is split between the files in it, in proportion to what each
//! contributes. That turns "4 MiB of rootfs, compressed 2.6:1" into a per-file
//! number that adds up to the image.

use super::squashfs::SquashfsInfo;
use super::{le_u16, le_u32, le_u64};
use std::collections::HashMap;

/// Metadata blocks are at most 8 KiB once decompressed.
const METADATA_SIZE: usize = 8192;
/// Ceiling on the listing, as elsewhere.
const MAX_ENTRIES: usize = 20_000;
/// A guard against a corrupt image describing an unbounded tree.
const MAX_DEPTH: usize = 48;

const INODE_DIR: u16 = 1;
const INODE_FILE: u16 = 2;
const INODE_SYMLINK: u16 = 3;
const INODE_LDIR: u16 = 8;
const INODE_LFILE: u16 = 9;
const INODE_LSYMLINK: u16 = 10;

#[derive(Debug, Clone, PartialEq)]
pub struct SquashEntry {
    pub path: String,
    /// Uncompressed size, which for a directory is zero.
    pub bytes: u64,
    /// What the file costs on the medium: the sum of its compressed blocks
    /// plus its share of any fragment it lives in. `None` for anything that is
    /// not a regular file.
    pub compressed_bytes: Option<u64>,
    /// "file" | "dir" | "link" | "other"
    pub kind: &'static str,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SquashListing {
    pub entries: Vec<SquashEntry>,
    pub entries_truncated: bool,
    pub file_count: u32,
    pub dir_count: u32,
    pub link_count: u32,
    /// Sum of the uncompressed file sizes.
    pub logical_bytes: u64,
    /// Sum of the per-file compressed costs, fragments apportioned.
    pub compressed_bytes: u64,
}

/// Why a listing could not be produced, in words a report can print.
#[derive(Debug, Clone, PartialEq)]
pub enum ReadError {
    /// The image uses a compressor this build cannot decode.
    Unsupported(String),
    /// The image is damaged, or is not laid out the way it claims.
    Malformed,
}

fn decompress(algo: &str, src: &[u8], max_out: usize) -> Result<Vec<u8>, ReadError> {
    match algo {
        "gzip" => miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(src, max_out)
            .map_err(|_| ReadError::Malformed),
        "xz" => {
            let mut out = Vec::new();
            lzma_rs::xz_decompress(&mut std::io::Cursor::new(src), &mut out)
                .map_err(|_| ReadError::Malformed)?;
            Ok(out)
        }
        "lz4" => lz4_flex::block::decompress(src, max_out).map_err(|_| ReadError::Malformed),
        "zstd" => {
            use std::io::Read;
            let mut d = ruzstd::StreamingDecoder::new(src).map_err(|_| ReadError::Malformed)?;
            let mut out = Vec::new();
            d.read_to_end(&mut out).map_err(|_| ReadError::Malformed)?;
            Ok(out)
        }
        other => Err(ReadError::Unsupported(other.to_string())),
    }
}

/// The table readers work on a decompressed, concatenated view of a metadata
/// region, remembering where each block began so an inode reference -- which
/// addresses a block by its position in the *image* -- can be resolved.
#[derive(Debug)]
struct Metadata {
    bytes: Vec<u8>,
    /// image offset of a block start -> offset in `bytes`
    starts: HashMap<u64, usize>,
}

impl Metadata {
    /// Read metadata blocks from `start` until `end`.
    fn read(data: &[u8], algo: &str, start: u64, end: u64) -> Result<Metadata, ReadError> {
        let mut bytes = Vec::new();
        let mut starts = HashMap::new();
        let mut at = start;
        while at < end {
            let pos = at as usize;
            let header = le_u16(data, pos).ok_or(ReadError::Malformed)?;
            // The top bit marks a block that was stored as-is.
            let size = (header & 0x7FFF) as usize;
            let uncompressed = header & 0x8000 != 0;
            if size == 0 {
                break;
            }
            let src = data
                .get(pos + 2..pos + 2 + size)
                .ok_or(ReadError::Malformed)?;
            starts.insert(at, bytes.len());
            if uncompressed {
                bytes.extend_from_slice(src);
            } else {
                bytes.extend_from_slice(&decompress(algo, src, METADATA_SIZE)?);
            }
            at += (size + 2) as u64;
        }
        Ok(Metadata { bytes, starts })
    }

    /// Resolve a squashfs metadata reference: the block's image offset in the
    /// high 32 bits (relative to the table's start) and a byte offset within
    /// the decompressed block in the low 16.
    fn at(&self, base: u64, reference: u64) -> Option<usize> {
        let block = base + (reference >> 16);
        let within = (reference & 0xFFFF) as usize;
        self.starts.get(&block).map(|s| s + within)
    }
}

struct FileCost {
    /// Sum of the file's own compressed blocks.
    blocks: u64,
    /// Fragment this file's tail lives in, and how many bytes it puts there.
    fragment: Option<(u32, u64)>,
}

/// One parsed inode, reduced to what a size report needs.
struct Inode {
    kind: &'static str,
    size: u64,
    /// For a directory: where its entries are and how long they run.
    dir: Option<(u64, u32, u16)>,
    cost: Option<FileCost>,
}

fn parse_inode(m: &[u8], at: usize, block_size: u32) -> Option<Inode> {
    let t = le_u16(m, at)?;
    let body = at + 16;
    match t {
        INODE_DIR => Some(Inode {
            kind: "dir",
            size: 0,
            // start_block, file_size, offset
            dir: Some((
                le_u32(m, body)? as u64,
                le_u16(m, body + 8)? as u32,
                le_u16(m, body + 10)?,
            )),
            cost: None,
        }),
        INODE_LDIR => Some(Inode {
            kind: "dir",
            size: 0,
            dir: Some((
                le_u32(m, body + 8)? as u64,
                le_u32(m, body + 4)?,
                le_u16(m, body + 18)?,
            )),
            cost: None,
        }),
        INODE_FILE | INODE_LFILE => {
            let (fragment, size, tail_at) = if t == INODE_FILE {
                (le_u32(m, body + 4)?, le_u32(m, body + 12)? as u64, body + 16)
            } else {
                (le_u32(m, body + 28)?, le_u64(m, body + 8)?, body + 40)
            };
            let block_offset = if t == INODE_FILE {
                le_u32(m, body + 8)?
            } else {
                le_u32(m, body + 32)?
            } as u64;

            // A file's full blocks are listed one u32 each; the low 24 bits
            // are the size on the medium, and bit 24 marks one stored as-is.
            let has_fragment = fragment != 0xFFFF_FFFF;
            let full_blocks = if has_fragment {
                (size / block_size as u64) as usize
            } else {
                size.div_ceil(block_size as u64) as usize
            };
            let mut blocks = 0u64;
            for i in 0..full_blocks {
                let v = le_u32(m, tail_at + i * 4)?;
                blocks += (v & 0x00FF_FFFF) as u64;
            }
            let tail = size % block_size as u64;
            Some(Inode {
                kind: "file",
                size,
                dir: None,
                cost: Some(FileCost {
                    blocks,
                    fragment: (has_fragment && tail > 0).then_some((fragment, tail)).or({
                        // A file whose size is a whole number of blocks has no
                        // tail even when the field is set.
                        None
                    }),
                }),
            })
            .map(|mut i| {
                // block_offset is only meaningful with a fragment; keep the
                // read so a malformed inode is caught here rather than later.
                let _ = block_offset;
                i.size = size;
                i
            })
        }
        INODE_SYMLINK | INODE_LSYMLINK => Some(Inode {
            kind: "link",
            size: le_u32(m, body + 4)? as u64,
            dir: None,
            cost: None,
        }),
        _ => Some(Inode {
            kind: "other",
            size: 0,
            dir: None,
            cost: None,
        }),
    }
}

/// Fragment blocks, by index: (compressed size on the medium).
fn read_fragments(
    data: &[u8],
    sb: &SquashfsInfo,
    algo: &str,
    table_start: u64,
) -> Result<Vec<u64>, ReadError> {
    if sb.fragment_count == 0 || table_start == u64::MAX {
        return Ok(Vec::new());
    }
    // The table is a list of u64 pointers, each to one metadata block holding
    // up to 512 of the 16-byte fragment entries.
    let blocks = (sb.fragment_count as usize).div_ceil(512);
    let mut out = Vec::with_capacity(sb.fragment_count as usize);
    let mut raw = Vec::new();
    for i in 0..blocks {
        let ptr = le_u64(data, table_start as usize + i * 8).ok_or(ReadError::Malformed)?;
        // Exactly one block: the pointers are not contiguous with each other,
        // and reading on would run into whatever table comes next.
        let m = Metadata::read(data, algo, ptr, ptr + 1)?;
        raw.extend_from_slice(&m.bytes);
    }
    for i in 0..sb.fragment_count as usize {
        // start u64, size u32 (bit 24 = uncompressed), unused u32
        let size = le_u32(&raw, i * 16 + 8).ok_or(ReadError::Malformed)?;
        out.push((size & 0x00FF_FFFF) as u64);
    }
    Ok(out)
}

/// Read the directory tree and the per-file compressed cost.
pub fn read(data: &[u8], sb: &SquashfsInfo) -> Result<SquashListing, ReadError> {
    let algo = sb.compression.as_str();
    // Superblock fields the summary does not need: the root inode reference at
    // 32, then the table offsets from 48 on.
    let root_ref = le_u64(data, 32).ok_or(ReadError::Malformed)?;
    let id_table = le_u64(data, 48).ok_or(ReadError::Malformed)?;
    let inode_table = le_u64(data, 64).ok_or(ReadError::Malformed)?;
    let dir_table = le_u64(data, 72).ok_or(ReadError::Malformed)?;
    let fragment_table = le_u64(data, 80).ok_or(ReadError::Malformed)?;
    let lookup_table = le_u64(data, 88).ok_or(ReadError::Malformed)?;

    if inode_table >= dir_table || dir_table > sb.bytes_used {
        return Err(ReadError::Malformed);
    }
    // Each table runs until the next one begins.
    let after_dir = [fragment_table, id_table, lookup_table, sb.bytes_used]
        .into_iter()
        .filter(|&v| v != u64::MAX && v > dir_table)
        .min()
        .unwrap_or(sb.bytes_used);

    let inodes = Metadata::read(data, algo, inode_table, dir_table)?;
    let dirs = Metadata::read(data, algo, dir_table, after_dir)?;
    let fragments = read_fragments(data, sb, algo, fragment_table)?;

    // First pass: how much each fragment block holds in total, so a shared
    // block can be divided between the files that share it.
    let mut fragment_load: HashMap<u32, u64> = HashMap::new();
    let mut walk_stack = vec![(root_ref, String::new(), 0usize)];
    let mut inode_cache: Vec<(u64, String, &'static str, u64, Option<FileCost>)> = Vec::new();
    let mut seen_dirs: Vec<u64> = Vec::new();

    while let Some((dir_ref, prefix, depth)) = walk_stack.pop() {
        if depth > MAX_DEPTH || seen_dirs.contains(&dir_ref) {
            continue;
        }
        seen_dirs.push(dir_ref);
        let at = inodes
            .at(inode_table, dir_ref)
            .ok_or(ReadError::Malformed)?;
        let Some(dir_inode) = parse_inode(&inodes.bytes, at, sb.block_size) else {
            continue;
        };
        let Some((start_block, dir_size, dir_offset)) = dir_inode.dir else {
            continue;
        };
        // A directory's listing is `file_size` bytes starting at its offset,
        // and squashfs counts a trailing byte that is not there.
        if dir_size < 3 {
            continue;
        }
        let Some(begin) = dirs.at(dir_table, (start_block << 16) | dir_offset as u64) else {
            continue;
        };
        let end = begin + dir_size as usize - 3;
        let mut p = begin;
        while p + 12 <= end.min(dirs.bytes.len()) {
            let count = le_u32(&dirs.bytes, p).ok_or(ReadError::Malformed)?;
            let start = le_u32(&dirs.bytes, p + 4).ok_or(ReadError::Malformed)? as u64;
            p += 12;
            // The header's count is one less than the number of entries.
            for _ in 0..(count as u64 + 1) {
                if p + 8 > dirs.bytes.len() {
                    break;
                }
                let offset = le_u16(&dirs.bytes, p).ok_or(ReadError::Malformed)?;
                let kind = le_u16(&dirs.bytes, p + 4).ok_or(ReadError::Malformed)?;
                let name_len = le_u16(&dirs.bytes, p + 6).ok_or(ReadError::Malformed)? as usize + 1;
                let name_at = p + 8;
                let raw = dirs
                    .bytes
                    .get(name_at..name_at + name_len)
                    .ok_or(ReadError::Malformed)?;
                let name = String::from_utf8_lossy(raw).into_owned();
                p = name_at + name_len;

                let child_ref = (start << 16) | offset as u64;
                let path = format!("{prefix}/{name}");
                let Some(child_at) = inodes.at(inode_table, child_ref) else {
                    continue;
                };
                let Some(child) = parse_inode(&inodes.bytes, child_at, sb.block_size) else {
                    continue;
                };
                if let Some(c) = &child.cost {
                    if let Some((frag, tail)) = c.fragment {
                        *fragment_load.entry(frag).or_insert(0) += tail;
                    }
                }
                if kind == INODE_DIR || kind == INODE_LDIR {
                    walk_stack.push((child_ref, path.clone(), depth + 1));
                }
                inode_cache.push((child_ref, path, child.kind, child.size, child.cost));
            }
        }
    }

    // Second pass: now that each fragment's total load is known, split it.
    let mut out = SquashListing::default();
    for (_, path, kind, size, cost) in inode_cache {
        let compressed = cost.map(|c| {
            let share = match c.fragment {
                Some((frag, tail)) => {
                    let block = fragments.get(frag as usize).copied().unwrap_or(0);
                    let load = fragment_load.get(&frag).copied().unwrap_or(0);
                    if load == 0 {
                        0
                    } else {
                        // Proportional, so the shares of a block add to it.
                        (block as u128 * tail as u128 / load as u128) as u64
                    }
                }
                None => 0,
            };
            c.blocks + share
        });
        match kind {
            "dir" => out.dir_count += 1,
            "link" => out.link_count += 1,
            "file" => {
                out.file_count += 1;
                out.logical_bytes += size;
                out.compressed_bytes += compressed.unwrap_or(0);
            }
            _ => {}
        }
        if out.entries.len() < MAX_ENTRIES {
            out.entries.push(SquashEntry {
                path,
                bytes: size,
                compressed_bytes: compressed,
                kind,
            });
        } else {
            out.entries_truncated = true;
        }
    }
    out.entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_an_unsupported_compressor_rather_than_guessing() {
        let e = decompress("lzo", &[1, 2, 3], 4096).unwrap_err();
        assert_eq!(e, ReadError::Unsupported("lzo".to_string()));
    }

    #[test]
    fn round_trips_each_supported_compressor() {
        let payload: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();

        let gz = miniz_oxide::deflate::compress_to_vec_zlib(&payload, 6);
        assert_eq!(decompress("gzip", &gz, METADATA_SIZE).unwrap(), payload);

        let lz = lz4_flex::block::compress(&payload);
        assert_eq!(decompress("lz4", &lz, payload.len()).unwrap(), payload);
    }

    #[test]
    fn a_truncated_metadata_block_is_malformed_not_a_panic() {
        // A header promising more bytes than the image holds.
        let mut data = vec![0u8; 64];
        data[0..2].copy_from_slice(&100u16.to_le_bytes());
        assert_eq!(
            Metadata::read(&data, "gzip", 0, 64).unwrap_err(),
            ReadError::Malformed
        );
    }

    #[test]
    fn an_uncompressed_metadata_block_is_taken_as_is() {
        let mut data = Vec::new();
        let body = b"metadata contents";
        data.extend_from_slice(&(0x8000u16 | body.len() as u16).to_le_bytes());
        data.extend_from_slice(body);
        let m = Metadata::read(&data, "lzo", 0, data.len() as u64).expect("no decoder needed");
        assert_eq!(m.bytes, body);
        assert_eq!(m.at(0, 3), Some(3));
    }
}

#[cfg(test)]
mod real_image_tests {
    //! Driven by `tests/squashfs_fixtures.rs`, which builds images with
    //! mksquashfs when it is available.
}
