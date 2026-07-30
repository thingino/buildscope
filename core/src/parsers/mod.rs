//! Binary and text format parsers. Each parser is pure: bytes in, facts out,
//! `None` when the input is not that format. No parser ever guesses.

pub mod cpio;
pub mod ext;
pub mod fat;
pub mod fdt;
pub mod fit;
pub mod genimage;
pub mod gpt;
pub mod jffs2;
pub mod mbr;
pub mod mtdparts;
pub mod nandoob;
pub mod padding;
pub mod squashfs;
pub mod ubi;
pub mod ubifs;
pub mod ubootenv;
pub mod uimage;

pub(crate) fn le_u16(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

pub(crate) fn le_u32(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

pub(crate) fn le_u64(d: &[u8], o: usize) -> Option<u64> {
    d.get(o..o + 8).map(|b| {
        u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
    })
}

pub(crate) fn be_u32(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}
