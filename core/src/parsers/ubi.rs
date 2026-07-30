//! UBI reader: enough of the on-flash format to turn a NAND image into named
//! volumes with real usage.
//!
//! A NAND build carries no `mtdparts` list, so unlike NOR its layout is not
//! written down in the environment. It does not need to be: UBI is
//! self-describing. Every physical eraseblock opens with an erase-counter
//! header (`UBI#`), a volume-id header (`UBI!`) says which volume and which
//! logical block the eraseblock holds, and one reserved volume carries the
//! volume table that names everything. None of that is compressed, so a bare
//! image is enough to recover the layout and the real occupancy.
//!
//! Field order and structure sizes follow Linux's `include/mtd/ubi-media.h`.
//! Every multi-byte field is big endian, and both header CRCs cover the first
//! 60 bytes in UBI's convention: reflected CRC-32 seeded with all ones and
//! *no* final inversion.

use super::be_u32;
use crate::crc::crc32_raw;
use std::collections::BTreeMap;

const EC_MAGIC: u32 = 0x5542_4923; // "UBI#"
const VID_MAGIC: u32 = 0x5542_4921; // "UBI!"
const EC_HDR_SIZE: usize = 64;
const VID_HDR_SIZE: usize = 64;
const HDR_SIZE_CRC: usize = 60;
const VTBL_RECORD_SIZE: usize = 172;
const VTBL_RECORD_SIZE_CRC: usize = VTBL_RECORD_SIZE - 4;
const LAYOUT_VOLUME_ID: u32 = 0x7FFF_EFFF;
const VOL_NAME_MAX: usize = 127;
const VOL_TYPE_DYNAMIC: u8 = 1;
const VOL_TYPE_STATIC: u8 = 2;
/// `UBI_VTBL_AUTORESIZE_FLG`: the kernel grows this volume into whatever the
/// chip has spare the first time it attaches, so a generated image reserves
/// only a token amount.
const VTBL_AUTORESIZE: u8 = 0x01;

/// Eraseblocks start on an erase boundary; 512 is finer than any real one.
const SCAN_STEP: usize = 512;
/// Two layout-volume copies mean a real UBI image always has at least two.
const MIN_HEADERS: usize = 2;

fn be_u16(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]))
}

/// UBI seeds with all ones and does not invert the result.
fn ubi_crc(data: &[u8]) -> u32 {
    crc32_raw(0xFFFF_FFFF, data)
}

fn is_erased(d: &[u8]) -> bool {
    d.iter().all(|&b| b == 0xFF)
}

#[derive(Debug, Clone, PartialEq)]
struct EcHeader {
    vid_hdr_offset: u32,
    data_offset: u32,
    image_seq: u32,
}

fn parse_ec(d: &[u8]) -> Option<EcHeader> {
    let head = d.get(..EC_HDR_SIZE)?;
    if be_u32(head, 0)? != EC_MAGIC {
        return None;
    }
    if ubi_crc(&head[..HDR_SIZE_CRC]) != be_u32(head, 60)? {
        return None;
    }
    let h = EcHeader {
        vid_hdr_offset: be_u32(head, 16)?,
        data_offset: be_u32(head, 20)?,
        image_seq: be_u32(head, 24)?,
    };
    // The header area must precede the payload and leave room for the VID
    // header; anything else is a CRC coincidence, not an eraseblock. These are
    // file-supplied offsets, so the arithmetic is widened rather than trusted.
    if (h.vid_hdr_offset as u64) < EC_HDR_SIZE as u64
        || h.vid_hdr_offset as u64 + VID_HDR_SIZE as u64 > h.data_offset as u64
    {
        return None;
    }
    Some(h)
}

#[derive(Debug, Clone, PartialEq)]
struct VidHeader {
    vol_id: u32,
    lnum: u32,
    vol_type: u8,
    data_size: u32,
    data_pad: u32,
}

fn parse_vid(d: &[u8]) -> Option<VidHeader> {
    let head = d.get(..VID_HDR_SIZE)?;
    if be_u32(head, 0)? != VID_MAGIC {
        return None;
    }
    if ubi_crc(&head[..HDR_SIZE_CRC]) != be_u32(head, 60)? {
        return None;
    }
    Some(VidHeader {
        vol_type: head[5],
        vol_id: be_u32(head, 8)?,
        lnum: be_u32(head, 12)?,
        data_size: be_u32(head, 20)?,
        data_pad: be_u32(head, 28)?,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct UbiVolume {
    pub id: u32,
    /// Name from the volume table; empty when no table named this id.
    pub name: String,
    /// "static" | "dynamic"
    pub vol_type: &'static str,
    /// Eraseblocks the volume table reserved for it.
    pub reserved_pebs: u32,
    /// Eraseblocks actually mapped in this image.
    pub mapped_pebs: u32,
    /// Payload bytes present: what the headers of a static volume declare, or
    /// the usable capacity of the mapped blocks of a dynamic one.
    pub bytes: u64,
    /// Flash the mapped blocks occupy, headers and padding included.
    pub flash_bytes: u64,
    /// The kernel will grow this volume to fill the chip on first attach.
    pub autoresize: bool,
    /// Offset of the volume's first mapped eraseblock, or `None` when the
    /// table reserved the volume but the image holds nothing for it.
    pub peb_offset: Option<u64>,
    /// Distance from the first to the last mapped eraseblock's end.
    pub peb_span: u64,
    /// True when the mapped blocks are adjacent and in logical order, which
    /// is what a freshly generated image looks like.
    pub contiguous: bool,
    /// True when a logical block below the highest one present is missing, so
    /// the payload has a hole and cannot be read as one run of bytes.
    pub has_holes: bool,
    /// Payload with the logical blocks concatenated in order, ready to be
    /// identified by the same parsers used on a partition.
    pub content: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UbiInfo {
    /// Offset of the first eraseblock header in the image.
    pub start_offset: u64,
    pub peb_size: u32,
    pub leb_size: u32,
    pub vid_hdr_offset: u32,
    pub data_offset: u32,
    pub image_seq: u32,
    /// Eraseblocks between the first header and the end of the image.
    pub total_pebs: u32,
    /// Eraseblocks carrying a volume's logical block.
    pub mapped_pebs: u32,
    /// Eraseblocks with a header but no volume mapping: UBI's spare pool.
    pub free_pebs: u32,
    /// Eraseblocks left erased, which a generated image simply omits.
    pub erased_pebs: u32,
    /// Eraseblocks holding neither a valid header nor erased flash.
    pub bad_pebs: u32,
    pub volumes: Vec<UbiVolume>,
    /// False when no volume table was found, so the names are unknown.
    pub layout_found: bool,
}

impl UbiInfo {
    /// Flash the mapped eraseblocks occupy, the volume table included. This is
    /// what the UBI area actually costs, as opposed to what it spans.
    pub fn used_bytes(&self) -> u64 {
        self.mapped_pebs as u64 * self.peb_size as u64
    }
}

/// Offsets of every valid eraseblock header, in order.
fn header_offsets(data: &[u8]) -> Vec<usize> {
    let magic = EC_MAGIC.to_be_bytes();
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + EC_HDR_SIZE <= data.len() {
        if data[off..off + 4] == magic && parse_ec(&data[off..]).is_some() {
            out.push(off);
        }
        off += SCAN_STEP;
    }
    out
}

/// Offset of the first eraseblock header, which need not be 0: a NAND image
/// usually opens with a raw bootloader region.
pub fn find_start(data: &[u8]) -> Option<usize> {
    let hits = header_offsets(data);
    (hits.len() >= MIN_HEADERS).then(|| hits[0])
}

/// Whether a valid eraseblock header sits exactly here. Cheap enough to use as
/// a guard, and on its own it is the right check for "does this partition begin
/// with a UBI area".
pub fn has_header_at(data: &[u8], off: usize) -> bool {
    data.get(off..).and_then(parse_ec).is_some()
}

/// Eraseblock size from the spacing of the headers. Free or erased blocks
/// leave gaps that are whole multiples, so the smallest gap is the size.
fn peb_size_from(hits: &[usize]) -> Option<usize> {
    let mut peb = usize::MAX;
    for w in hits.windows(2) {
        peb = peb.min(w[1] - w[0]);
    }
    if peb == usize::MAX || peb < EC_HDR_SIZE {
        return None;
    }
    // Every other header must land on a multiple of it.
    if hits.windows(2).any(|w| (w[1] - w[0]) % peb != 0) {
        return None;
    }
    Some(peb)
}

#[derive(Debug, Clone, PartialEq)]
struct VtblRecord {
    name: String,
    reserved_pebs: u32,
    vol_type: u8,
    autoresize: bool,
}

/// Parse a volume table logical block: an array of fixed records, each with
/// its own CRC, indexed by volume id.
fn parse_vtbl(leb: &[u8]) -> BTreeMap<u32, VtblRecord> {
    let mut out = BTreeMap::new();
    for i in 0..(leb.len() / VTBL_RECORD_SIZE) {
        let rec = &leb[i * VTBL_RECORD_SIZE..(i + 1) * VTBL_RECORD_SIZE];
        let Some(stored) = be_u32(rec, VTBL_RECORD_SIZE_CRC) else {
            break;
        };
        if ubi_crc(&rec[..VTBL_RECORD_SIZE_CRC]) != stored {
            continue;
        }
        // An unused record is all zeroes; its CRC is valid, so filter on the
        // reservation instead.
        let reserved_pebs = be_u32(rec, 0).unwrap_or(0);
        if reserved_pebs == 0 {
            continue;
        }
        let name_len = (be_u16(rec, 14).unwrap_or(0) as usize).min(VOL_NAME_MAX);
        out.insert(
            i as u32,
            VtblRecord {
                name: String::from_utf8_lossy(&rec[16..16 + name_len]).into_owned(),
                reserved_pebs,
                vol_type: rec[12],
                autoresize: rec[144] & VTBL_AUTORESIZE != 0,
            },
        );
    }
    out
}

/// Parse a UBI area that begins exactly at `start`.
pub fn parse_at(data: &[u8], start: usize) -> Option<UbiInfo> {
    // Reject before scanning: this runs against every image in a build, and
    // almost none of them are UBI.
    if !has_header_at(data, start) {
        return None;
    }
    let hits: Vec<usize> = header_offsets(data.get(start..)?)
        .into_iter()
        .map(|o| o + start)
        .collect();
    if hits.first() != Some(&start) || hits.len() < MIN_HEADERS {
        return None;
    }
    let peb_size = peb_size_from(&hits)?;
    let first = parse_ec(&data[start..])?;
    let data_offset = first.data_offset as usize;
    if data_offset >= peb_size {
        return None;
    }
    let leb_size = peb_size - data_offset;

    // Walk the area one eraseblock at a time: mapped blocks by volume and
    // logical number, everything else counted.
    let mut blocks: BTreeMap<u32, BTreeMap<u32, (usize, VidHeader)>> = BTreeMap::new();
    let (mut total, mut free, mut erased, mut bad) = (0u32, 0u32, 0u32, 0u32);
    let mut off = start;
    while off < data.len() {
        total += 1;
        let peb = &data[off..(off + peb_size).min(data.len())];
        match parse_ec(peb) {
            Some(ec) => match peb.get(ec.vid_hdr_offset as usize..).and_then(parse_vid) {
                Some(vid) => {
                    blocks
                        .entry(vid.vol_id)
                        .or_default()
                        .insert(vid.lnum, (off, vid));
                }
                None => free += 1,
            },
            None if is_erased(&peb[..EC_HDR_SIZE.min(peb.len())]) => erased += 1,
            None => bad += 1,
        }
        off += peb_size;
    }
    if blocks.is_empty() {
        return None;
    }
    let mapped_pebs = blocks.values().map(|v| v.len() as u32).sum();

    // The layout volume names every other volume.
    let mut names: BTreeMap<u32, VtblRecord> = BTreeMap::new();
    let mut layout_found = false;
    if let Some(layout) = blocks.get(&LAYOUT_VOLUME_ID) {
        for (peb_off, _) in layout.values() {
            let from = peb_off + data_offset;
            let to = (from + leb_size).min(data.len());
            if from >= to {
                continue;
            }
            let table = parse_vtbl(&data[from..to]);
            if !table.is_empty() {
                names = table;
                layout_found = true;
                break;
            }
        }
    }

    // A volume the table reserved but the image never wrote is still real
    // space, so report the union of both sides rather than only what is
    // mapped: `ubinize` leaves an autoresize volume empty on purpose.
    let empty: BTreeMap<u32, (usize, VidHeader)> = BTreeMap::new();
    let mut ids: Vec<u32> = blocks.keys().copied().chain(names.keys().copied()).collect();
    ids.sort_unstable();
    ids.dedup();

    let mut volumes = Vec::new();
    for vol_id in ids {
        if vol_id == LAYOUT_VOLUME_ID {
            continue;
        }
        let lebs = blocks.get(&vol_id).unwrap_or(&empty);
        let named = names.get(&vol_id);
        // The volume table is authoritative; a block header settles it when
        // no table was found.
        let type_byte = named
            .map(|n| n.vol_type)
            .or_else(|| lebs.values().next().map(|(_, v)| v.vol_type))
            .unwrap_or(VOL_TYPE_DYNAMIC);
        let is_static = type_byte == VOL_TYPE_STATIC;

        let mut content = Vec::new();
        for (peb_off, vid) in lebs.values() {
            let from = peb_off + data_offset;
            if from >= data.len() {
                continue;
            }
            // A static volume declares its payload per block; a dynamic one
            // uses the whole block less any alignment padding.
            // Both come from the block header, so neither is trusted to be
            // sane before it is clamped to what the image actually holds.
            let want = if is_static {
                vid.data_size as usize
            } else {
                leb_size.saturating_sub(vid.data_pad as usize)
            };
            let to = from.saturating_add(want).min(data.len());
            content.extend_from_slice(&data[from..to]);
        }

        let first_peb = lebs.values().map(|(o, _)| *o).min();
        let last_peb = lebs.values().map(|(o, _)| *o).max();
        let span = match (first_peb, last_peb) {
            (Some(f), Some(l)) => (l + peb_size).min(data.len()) as u64 - f as u64,
            _ => 0,
        };
        let ordered_and_dense = first_peb.is_some_and(|f| {
            lebs.values()
                .enumerate()
                .all(|(i, (o, _))| *o == f + i * peb_size)
        });
        let highest = *lebs.keys().max().unwrap_or(&0);

        volumes.push(UbiVolume {
            id: vol_id,
            name: named.map(|n| n.name.clone()).unwrap_or_default(),
            vol_type: if is_static { "static" } else { "dynamic" },
            reserved_pebs: named.map(|n| n.reserved_pebs).unwrap_or(lebs.len() as u32),
            mapped_pebs: lebs.len() as u32,
            bytes: content.len() as u64,
            flash_bytes: lebs.len() as u64 * peb_size as u64,
            autoresize: named.is_some_and(|n| n.autoresize),
            peb_offset: first_peb.map(|o| o as u64),
            peb_span: span,
            contiguous: ordered_and_dense,
            has_holes: !lebs.is_empty() && highest as usize + 1 != lebs.len(),
            content,
        });
    }
    // Placed volumes in flash order, then whatever the table only reserved.
    volumes.sort_by_key(|v| (v.peb_offset.is_none(), v.peb_offset, v.id));

    Some(UbiInfo {
        start_offset: start as u64,
        peb_size: peb_size as u32,
        leb_size: leb_size as u32,
        vid_hdr_offset: first.vid_hdr_offset,
        data_offset: first.data_offset,
        image_seq: first.image_seq,
        total_pebs: total,
        mapped_pebs,
        free_pebs: free,
        erased_pebs: erased,
        bad_pebs: bad,
        volumes,
        layout_found,
    })
}

/// Find and parse a UBI area anywhere in the image.
pub fn parse(data: &[u8]) -> Option<UbiInfo> {
    parse_at(data, find_start(data)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEB: usize = 64 * 1024;
    const VID_OFF: usize = 2048;
    const DATA_OFF: usize = 4096;
    const LEB: usize = PEB - DATA_OFF;
    const SEQ: u32 = 0x1234_5678;

    fn ec_header() -> Vec<u8> {
        let mut h = vec![0u8; EC_HDR_SIZE];
        h[0..4].copy_from_slice(&EC_MAGIC.to_be_bytes());
        h[4] = 1;
        h[8..16].copy_from_slice(&7u64.to_be_bytes()); // erase counter
        h[16..20].copy_from_slice(&(VID_OFF as u32).to_be_bytes());
        h[20..24].copy_from_slice(&(DATA_OFF as u32).to_be_bytes());
        h[24..28].copy_from_slice(&SEQ.to_be_bytes());
        let crc = ubi_crc(&h[..HDR_SIZE_CRC]);
        h[60..64].copy_from_slice(&crc.to_be_bytes());
        h
    }

    fn vid_header(vol_id: u32, lnum: u32, vol_type: u8, data_size: u32) -> Vec<u8> {
        let mut h = vec![0u8; VID_HDR_SIZE];
        h[0..4].copy_from_slice(&VID_MAGIC.to_be_bytes());
        h[4] = 1;
        h[5] = vol_type;
        h[8..12].copy_from_slice(&vol_id.to_be_bytes());
        h[12..16].copy_from_slice(&lnum.to_be_bytes());
        h[20..24].copy_from_slice(&data_size.to_be_bytes());
        let crc = ubi_crc(&h[..HDR_SIZE_CRC]);
        h[60..64].copy_from_slice(&crc.to_be_bytes());
        h
    }

    fn vtbl_record(reserved_pebs: u32, vol_type: u8, name: &str) -> Vec<u8> {
        vtbl_record_flags(reserved_pebs, vol_type, name, 0)
    }

    fn vtbl_record_flags(reserved_pebs: u32, vol_type: u8, name: &str, flags: u8) -> Vec<u8> {
        let mut r = vec![0u8; VTBL_RECORD_SIZE];
        r[0..4].copy_from_slice(&reserved_pebs.to_be_bytes());
        r[4..8].copy_from_slice(&1u32.to_be_bytes()); // alignment
        r[12] = vol_type;
        r[14..16].copy_from_slice(&(name.len() as u16).to_be_bytes());
        r[16..16 + name.len()].copy_from_slice(name.as_bytes());
        r[144] = flags;
        let crc = ubi_crc(&r[..VTBL_RECORD_SIZE_CRC]);
        r[VTBL_RECORD_SIZE_CRC..].copy_from_slice(&crc.to_be_bytes());
        r
    }

    /// Append one eraseblock: headers, then payload, tail left erased.
    fn push_peb(img: &mut Vec<u8>, vid: Option<Vec<u8>>, payload: &[u8]) {
        let base = img.len();
        img.resize(base + PEB, 0xFF);
        img[base..base + EC_HDR_SIZE].copy_from_slice(&ec_header());
        if let Some(v) = vid {
            img[base + VID_OFF..base + VID_OFF + VID_HDR_SIZE].copy_from_slice(&v);
            let end = base + DATA_OFF + payload.len();
            img[base + DATA_OFF..end].copy_from_slice(payload);
        }
    }

    /// A raw boot region, then a UBI area with a layout volume naming two
    /// volumes, a two-block static one and a one-block dynamic one, then a
    /// spare block.
    fn synthetic(boot_bytes: usize) -> Vec<u8> {
        let mut img = vec![0xAAu8; boot_bytes];

        let mut table = Vec::new();
        table.extend_from_slice(&vtbl_record(2, VOL_TYPE_STATIC, "kernel"));
        table.extend_from_slice(&vtbl_record(4, VOL_TYPE_DYNAMIC, "overlay"));
        let lay = |l| Some(vid_header(LAYOUT_VOLUME_ID, l, VOL_TYPE_DYNAMIC, 0));
        push_peb(&mut img, lay(0), &table);
        push_peb(&mut img, lay(1), &table);

        push_peb(&mut img, Some(vid_header(0, 0, VOL_TYPE_STATIC, LEB as u32)), &vec![0x11; LEB]);
        push_peb(&mut img, Some(vid_header(0, 1, VOL_TYPE_STATIC, 100)), &vec![0x22; 100]);
        push_peb(&mut img, Some(vid_header(1, 0, VOL_TYPE_DYNAMIC, 0)), &vec![0x33; 512]);
        push_peb(&mut img, None, &[]); // spare: header, no mapping
        img
    }

    #[test]
    fn reads_geometry_volumes_and_names() {
        let img = synthetic(0x100000);
        let info = parse(&img).expect("ubi area");
        assert_eq!(info.start_offset, 0x100000);
        assert_eq!(info.peb_size, PEB as u32);
        assert_eq!(info.leb_size, LEB as u32);
        assert_eq!(info.vid_hdr_offset, VID_OFF as u32);
        assert_eq!(info.image_seq, SEQ);
        assert!(info.layout_found);
        assert_eq!(info.total_pebs, 6);
        assert_eq!(info.mapped_pebs, 5);
        assert_eq!(info.free_pebs, 1);
        assert_eq!(info.erased_pebs, 0);
        assert_eq!(info.bad_pebs, 0);
        assert_eq!(info.used_bytes(), 5 * PEB as u64);

        let names: Vec<&str> = info.volumes.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["kernel", "overlay"]);

        let kernel = &info.volumes[0];
        assert_eq!(kernel.vol_type, "static");
        assert_eq!(kernel.reserved_pebs, 2);
        assert_eq!(kernel.mapped_pebs, 2);
        // A static volume reports exactly what its block headers declare.
        assert_eq!(kernel.bytes, LEB as u64 + 100);
        assert_eq!(kernel.content.len(), kernel.bytes as usize);
        assert_eq!(kernel.content[0], 0x11);
        assert_eq!(*kernel.content.last().unwrap(), 0x22);
        assert_eq!(kernel.flash_bytes, 2 * PEB as u64);
        assert_eq!(kernel.peb_offset, Some(0x100000 + 2 * PEB as u64));
        assert_eq!(kernel.peb_span, 2 * PEB as u64);
        assert!(kernel.contiguous && !kernel.has_holes);
        assert!(!kernel.autoresize);

        let overlay = &info.volumes[1];
        assert_eq!(overlay.vol_type, "dynamic");
        assert_eq!(overlay.reserved_pebs, 4); // reserved, not mapped
        assert_eq!(overlay.mapped_pebs, 1);
        assert_eq!(overlay.bytes, LEB as u64);
    }

    #[test]
    fn finds_the_area_after_a_boot_region() {
        for boot in [0usize, 0x40000, 0x100000] {
            let info = parse(&synthetic(boot)).expect("ubi area");
            assert_eq!(info.start_offset, boot as u64);
            assert_eq!(info.peb_size, PEB as u32);
        }
    }

    /// A gap of erased blocks must not be mistaken for a smaller eraseblock,
    /// and must be counted rather than silently dropped.
    #[test]
    fn erased_gap_keeps_the_eraseblock_size() {
        let mut img = synthetic(0);
        // Two erased blocks, then one more mapped block of volume 1.
        img.resize(img.len() + 2 * PEB, 0xFF);
        push_peb(&mut img, Some(vid_header(1, 1, VOL_TYPE_DYNAMIC, 0)), &vec![0x44; 32]);
        let info = parse(&img).expect("ubi area");
        assert_eq!(info.peb_size, PEB as u32);
        assert_eq!(info.erased_pebs, 2);
        let overlay = info.volumes.iter().find(|v| v.name == "overlay").unwrap();
        assert_eq!(overlay.mapped_pebs, 2);
        // Two logical blocks, not adjacent on flash: block 4 and block 8, so
        // the span covers the three unrelated blocks between them.
        assert!(!overlay.contiguous);
        assert!(!overlay.has_holes);
        assert_eq!(overlay.peb_offset, Some(4 * PEB as u64));
        assert_eq!(overlay.peb_span, 5 * PEB as u64);
        assert_eq!(overlay.flash_bytes, 2 * PEB as u64);
    }

    /// A missing logical block leaves a hole the payload cannot hide.
    #[test]
    fn missing_logical_block_is_flagged() {
        let mut img = Vec::new();
        let table = vtbl_record(3, VOL_TYPE_DYNAMIC, "overlay");
        let lay = |l| Some(vid_header(LAYOUT_VOLUME_ID, l, VOL_TYPE_DYNAMIC, 0));
        push_peb(&mut img, lay(0), &table);
        push_peb(&mut img, lay(1), &table);
        push_peb(&mut img, Some(vid_header(0, 0, VOL_TYPE_DYNAMIC, 0)), &[1]);
        push_peb(&mut img, Some(vid_header(0, 2, VOL_TYPE_DYNAMIC, 0)), &[3]);
        let info = parse(&img).expect("ubi area");
        let v = &info.volumes[0];
        assert_eq!(v.mapped_pebs, 2);
        assert!(v.has_holes);
    }

    /// `ubinize` writes no eraseblock for a volume with no image, which is how
    /// an autoresize overlay is generated. It is still reserved space and must
    /// be reported, without a location it does not have.
    #[test]
    fn reserved_but_empty_volume_is_reported() {
        let mut img = Vec::new();
        let mut table = Vec::new();
        table.extend_from_slice(&vtbl_record(2, VOL_TYPE_STATIC, "rootfs"));
        table.extend_from_slice(&vtbl_record_flags(5, VOL_TYPE_DYNAMIC, "overlay", 1));
        let lay = |l| Some(vid_header(LAYOUT_VOLUME_ID, l, VOL_TYPE_DYNAMIC, 0));
        push_peb(&mut img, lay(0), &table);
        push_peb(&mut img, lay(1), &table);
        push_peb(&mut img, Some(vid_header(0, 0, VOL_TYPE_STATIC, 64)), &vec![0x77; 64]);

        let info = parse(&img).expect("ubi area");
        assert_eq!(info.volumes.len(), 2);
        // Placed volumes first, reserved-only last.
        assert_eq!(info.volumes[0].name, "rootfs");
        let overlay = &info.volumes[1];
        assert_eq!(overlay.name, "overlay");
        assert_eq!(overlay.mapped_pebs, 0);
        assert_eq!(overlay.reserved_pebs, 5);
        assert_eq!(overlay.bytes, 0);
        assert_eq!(overlay.flash_bytes, 0);
        assert_eq!(overlay.peb_offset, None);
        assert_eq!(overlay.peb_span, 0);
        assert!(overlay.autoresize);
        assert!(!overlay.has_holes);
        assert!(overlay.content.is_empty());
        // Only the written blocks count as used flash.
        assert_eq!(info.mapped_pebs, 3);
    }

    /// Without a volume table the blocks still parse, unnamed.
    #[test]
    fn no_layout_volume_still_reports_blocks() {
        let mut img = Vec::new();
        push_peb(&mut img, Some(vid_header(0, 0, VOL_TYPE_STATIC, 64)), &vec![0x55; 64]);
        push_peb(&mut img, Some(vid_header(0, 1, VOL_TYPE_STATIC, 64)), &vec![0x55; 64]);
        let info = parse(&img).expect("ubi area");
        assert!(!info.layout_found);
        assert_eq!(info.volumes.len(), 1);
        assert_eq!(info.volumes[0].name, "");
        assert_eq!(info.volumes[0].reserved_pebs, 2); // mapped count stands in
        assert_eq!(info.volumes[0].bytes, 128);
    }

    #[test]
    fn rejects_non_ubi() {
        assert!(parse(&[0u8; 4096]).is_none());
        assert!(parse(&vec![0xFFu8; 1 << 20]).is_none());
        assert!(parse(&vec![0x5Au8; 1 << 20]).is_none());
        // A lone eraseblock is not enough to fix the geometry.
        let mut one = Vec::new();
        push_peb(&mut one, Some(vid_header(0, 0, VOL_TYPE_DYNAMIC, 0)), &[1]);
        assert!(parse(&one).is_none());
    }

    #[test]
    fn corrupt_header_crc_is_not_an_eraseblock() {
        let mut img = synthetic(0);
        img[60] ^= 0xFF; // first EC header CRC
        let info = parse(&img).expect("later blocks still parse");
        assert_eq!(info.start_offset, PEB as u64); // area now starts at PEB 1
        assert_eq!(info.peb_size, PEB as u32);
    }

    #[test]
    fn parse_at_requires_the_area_to_start_there() {
        let img = synthetic(0x40000);
        assert!(parse_at(&img, 0).is_none());
        assert!(parse_at(&img, 0x40000).is_some());
    }
}
