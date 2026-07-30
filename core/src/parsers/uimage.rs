//! Legacy U-Boot uImage header: 64 bytes, big endian, magic 0x27051956.

use super::be_u32;
use crate::crc::crc32_ieee;

pub const MAGIC: u32 = 0x2705_1956;
pub const HEADER_LEN: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub struct UimageInfo {
    pub header_crc_ok: bool,
    pub data_crc: u32,
    pub timestamp: u32,
    /// Payload size declared by the header (image file = 64 + this, plus any padding).
    pub declared_size: u32,
    pub load_addr: u32,
    pub entry_point: u32,
    pub os: u8,
    pub arch: u8,
    pub image_type: u8,
    pub type_name: String,
    pub compression: u8,
    pub compression_name: String,
    pub name: String,
}

fn comp_name(c: u8) -> &'static str {
    match c {
        0 => "none",
        1 => "gzip",
        2 => "bzip2",
        3 => "lzma",
        4 => "lzo",
        5 => "lz4",
        6 => "zstd",
        _ => "unknown",
    }
}

fn type_name(t: u8) -> &'static str {
    match t {
        1 => "standalone",
        2 => "kernel",
        3 => "ramdisk",
        4 => "multi",
        5 => "firmware",
        6 => "script",
        7 => "filesystem",
        _ => "other",
    }
}

pub fn parse(data: &[u8]) -> Option<UimageInfo> {
    if data.len() < HEADER_LEN || be_u32(data, 0)? != MAGIC {
        return None;
    }
    let stored_hcrc = be_u32(data, 4)?;
    let timestamp = be_u32(data, 8)?;
    let declared_size = be_u32(data, 12)?;
    let load_addr = be_u32(data, 16)?;
    let entry_point = be_u32(data, 20)?;
    let data_crc = be_u32(data, 24)?;
    let os = data[28];
    let arch = data[29];
    let image_type = data[30];
    let compression = data[31];
    let name = String::from_utf8_lossy(&data[32..64])
        .trim_end_matches('\0')
        .to_string();

    let mut header = [0u8; HEADER_LEN];
    header.copy_from_slice(&data[..HEADER_LEN]);
    header[4..8].fill(0);
    let header_crc_ok = crc32_ieee(&header) == stored_hcrc;

    // A header whose declared size wildly exceeds the file is not a uImage
    // (or is truncated); either way, refuse rather than report nonsense.
    if (declared_size as u64) > data.len() as u64 {
        return None;
    }

    Some(UimageInfo {
        header_crc_ok,
        data_crc,
        timestamp,
        declared_size,
        load_addr,
        entry_point,
        os,
        arch,
        image_type,
        type_name: type_name(image_type).to_string(),
        compression,
        compression_name: comp_name(compression).to_string(),
        name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic(payload: &[u8]) -> Vec<u8> {
        let mut h = vec![0u8; HEADER_LEN];
        h[0..4].copy_from_slice(&MAGIC.to_be_bytes());
        h[8..12].copy_from_slice(&1_700_000_000u32.to_be_bytes());
        h[12..16].copy_from_slice(&(payload.len() as u32).to_be_bytes());
        h[16..20].copy_from_slice(&0x8060_0000u32.to_be_bytes());
        h[20..24].copy_from_slice(&0x8060_0000u32.to_be_bytes());
        h[24..28].copy_from_slice(&crc32_ieee(payload).to_be_bytes());
        h[28] = 5; // linux
        h[29] = 5; // mips
        h[30] = 2; // kernel
        h[31] = 3; // lzma
        h[32..38].copy_from_slice(b"kernel");
        let hcrc = crc32_ieee(&h);
        h[4..8].copy_from_slice(&hcrc.to_be_bytes());
        h.extend_from_slice(payload);
        h
    }

    #[test]
    fn parses_synthetic() {
        let img = synthetic(b"payload bytes here");
        let info = parse(&img).unwrap();
        assert!(info.header_crc_ok);
        assert_eq!(info.declared_size, 18);
        assert_eq!(info.compression_name, "lzma");
        assert_eq!(info.type_name, "kernel");
        assert_eq!(info.name, "kernel");
    }

    #[test]
    fn bad_hcrc_flagged() {
        let mut img = synthetic(b"x");
        img[40] ^= 0xFF; // corrupt name inside CRC coverage
        let info = parse(&img).unwrap();
        assert!(!info.header_crc_ok);
    }

    #[test]
    fn rejects_oversized_declaration() {
        let mut img = synthetic(b"x");
        img[12..16].copy_from_slice(&0xFFFF_FFu32.to_be_bytes());
        assert!(parse(&img).is_none());
    }
}
