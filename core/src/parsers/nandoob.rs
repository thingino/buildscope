//! Raw NAND dumps that still carry their out-of-band bytes.
//!
//! A dump taken straight off the chip interleaves each page with its spare
//! area -- ECC, bad-block marker, whatever the controller puts there -- so the
//! image is page+spare, page+spare, and nothing that expects a filesystem can
//! read it. What gets *flashed* has no spare bytes, which is why every other
//! parser here assumes there are none.
//!
//! The spare layout is controller-specific and nothing in the image declares
//! it, so it is recovered by trying: strip each plausible page/spare pair and
//! see whether the result is a UBI area. UBI is a strong oracle for this,
//! since its eraseblock headers are CRC-checked and evenly spaced, so a wrong
//! guess does not survive.

use super::ubi;

/// Page and spare sizes real SPI-NAND and raw NAND parts use.
const PAGE_SIZES: &[usize] = &[512, 2048, 4096, 8192];
const SPARE_SIZES: &[usize] = &[16, 32, 64, 96, 112, 128, 218, 224, 256, 448, 512];

#[derive(Debug, Clone, PartialEq)]
pub struct OobLayout {
    pub page_bytes: usize,
    pub spare_bytes: usize,
    /// The image with every spare area removed.
    pub stripped: Vec<u8>,
}

/// Remove the spare area that follows each page.
fn strip(data: &[u8], page: usize, spare: usize) -> Vec<u8> {
    let stride = page + spare;
    let mut out = Vec::with_capacity(data.len() / stride * page);
    let mut off = 0;
    while off + page <= data.len() {
        out.extend_from_slice(&data[off..off + page]);
        off += stride;
    }
    out
}

/// Try to read the image as a dump with interleaved spare areas.
///
/// Returns `None` for an image that needs no stripping, which includes every
/// image that already parses as UBI: the point is to rescue the ones that do
/// not, never to second-guess the ones that do.
pub fn detect(data: &[u8]) -> Option<OobLayout> {
    // Only worth trying on something large enough to hold a few eraseblocks,
    // and only on something that does not already read as UBI. The test has
    // to be a full parse, not just finding a header: a dump keeps its
    // eraseblock headers intact inside their pages, so they are still there
    // to be found -- it is everything after them that has moved.
    if data.len() < 128 * 1024 || ubi::parse(data).is_some() {
        return None;
    }
    for &page in PAGE_SIZES {
        for &spare in SPARE_SIZES {
            // A spare area larger than its page is not a real geometry.
            if spare >= page {
                continue;
            }
            let stride = page + spare;
            if data.len() < stride * 4 {
                continue;
            }
            let stripped = strip(data, page, spare);
            let Some(info) = ubi::parse(&stripped) else {
                continue;
            };
            if !info.volumes.iter().any(|v| v.mapped_pebs > 0) {
                continue;
            }
            // Parsing is necessary but not sufficient. Any pair that removes
            // bytes at the same rate de-interleaves identically -- 512+16 and
            // 2048+64 both drop one byte in thirty-three -- and the header
            // CRCs cannot tell those apart, because what they cover survives
            // either way. The image settles it: UBI puts the volume header at
            // the start of the second page, since a page is the smallest unit
            // the chip can write, so the offset it recorded is the page size.
            if info.vid_hdr_offset as usize != page {
                continue;
            }
            return Some(OobLayout {
                page_bytes: page,
                spare_bytes: spare,
                stripped,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Interleave a spare area after every page, the way a dump does.
    fn interleave(data: &[u8], page: usize, spare: usize, fill: u8) -> Vec<u8> {
        let mut out = Vec::new();
        for chunk in data.chunks(page) {
            out.extend_from_slice(chunk);
            // A short final chunk is padded out, as an erased page would be.
            out.extend(std::iter::repeat_n(0xFF, page - chunk.len()));
            out.extend(std::iter::repeat_n(fill, spare));
        }
        out
    }

    /// A UBI image written for a chip with the given page size.
    fn ubi_image(page: usize) -> Vec<u8> {
        crate::parsers::ubi::tests_support::build_ubi_with_page(page)
    }

    #[test]
    fn recovers_a_dump_with_spare_areas() {
        // Real pairings: a 2 KiB page with 64 or 128 spare, 4 KiB with 224.
        for (page, spare) in [(2048usize, 64usize), (2048, 128), (4096, 224)] {
            let clean = ubi_image(page);
            let dumped = interleave(&clean, page, spare, 0xA5);
            let found = detect(&dumped).unwrap_or_else(|| panic!("{page}+{spare} not recovered"));
            assert_eq!(found.page_bytes, page);
            assert_eq!(found.spare_bytes, spare);
            // The stripped image is the original again, padding aside.
            assert_eq!(&found.stripped[..clean.len()], &clean[..]);
            assert!(ubi::parse(&found.stripped).is_some());
        }
    }

    #[test]
    fn an_image_without_spare_areas_is_left_alone() {
        let clean = ubi_image(2048);
        assert!(
            detect(&clean).is_none(),
            "a readable image must not be rewritten"
        );
    }

    #[test]
    fn junk_is_not_forced_into_a_layout() {
        assert!(detect(&vec![0u8; 1 << 20]).is_none());
        assert!(detect(&vec![0xFFu8; 1 << 20]).is_none());
        let noise: Vec<u8> = (0..(1 << 20)).map(|i| (i * 31 % 251) as u8).collect();
        assert!(detect(&noise).is_none());
        // Too small to judge.
        assert!(detect(&vec![0u8; 1024]).is_none());
    }
}
