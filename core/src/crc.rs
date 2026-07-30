//! Reflected CRC-32 (polynomial 0xEDB88320) in the two conventions this
//! problem space needs:
//!
//! - `crc32_ieee`: init 0xFFFFFFFF, final xor 0xFFFFFFFF. Used by U-Boot
//!   environment blocks and uImage header/data CRCs (zlib convention).
//! - `crc32_raw` with seed 0 and no final xor: the JFFS2 node convention.

const fn build_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

static TABLE: [u32; 256] = build_table();

/// Table-driven reflected CRC-32 with an explicit seed and no final xor.
pub fn crc32_raw(seed: u32, data: &[u8]) -> u32 {
    let mut crc = seed;
    for &b in data {
        crc = TABLE[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc
}

/// Standard (zlib/IEEE) CRC-32: pre and post inversion.
pub fn crc32_ieee(data: &[u8]) -> u32 {
    !crc32_raw(0xFFFF_FFFF, data)
}

/// JFFS2 node CRC: seed 0, no inversion.
pub fn crc32_jffs2(data: &[u8]) -> u32 {
    crc32_raw(0, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ieee_known_vector() {
        // "123456789" is the classic check value 0xCBF43926.
        assert_eq!(crc32_ieee(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn raw_zero_seed_differs() {
        assert_ne!(crc32_jffs2(b"123456789"), crc32_ieee(b"123456789"));
        assert_eq!(crc32_jffs2(b""), 0);
    }
}
