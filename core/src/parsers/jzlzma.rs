//! JZLZMA: Ingenic's hardware LZ77, which their bootloaders decompress with an
//! engine in the SoC rather than in software.
//!
//! It borrows LZMA's *distance model* -- posSlot, the rep[4] history, the same
//! constants -- but the bits are packed plainly rather than range-coded, which
//! is what makes it decodable in hardware and what makes it not LZMA. Nothing
//! else reads it: it is neither lzma-alone nor any lz4 shape, so an image
//! carrying one is opaque to every standard tool.
//!
//! Ingenic uImages routinely declare `comp=5` (lz4) for these, so the header
//! cannot be trusted; the payload layout is the only reliable tell.
//!
//! Grammar and container shapes follow CapnRon/un-jzlzma.

/// Where a stream was found and what it says about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub dict_size: u32,
    /// Size the container claims the output will be.
    pub uncompressed: u32,
    /// Offset of the first byte of the bit stream.
    pub stream_at: usize,
    /// Which container shape matched, for the report.
    pub container: &'static str,
}

const MAGIC: u32 = 0x2705_1956;
const K_START_POS_MODEL: u32 = 4;
const K_END_POS_MODEL: u32 = 14;
const K_NUM_ALIGN_BITS: u32 = 4;

/// A dictionary outside this range means the bytes are not a header at all.
const DICT_MIN: u32 = 0x1000;
const DICT_MAX: u32 = 0x400_0000;

fn le32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

/// Identify the container, if these bytes are one.
///
/// Three shapes occur in the wild, and the wrapper's magic sits at a different
/// offset in two of them -- a detector that only checks one silently falls
/// through and mis-parses the other as a raw stream.
pub fn parse(d: &[u8]) -> Option<Header> {
    // mark_rootfs, magic third: [uncompressed][compressed][magic] then a raw
    // container of its own.
    if le32(d, 8) == Some(MAGIC) {
        let dict_size = le32(d, 12)?;
        if (DICT_MIN..=DICT_MAX).contains(&dict_size) {
            return Some(Header {
                dict_size,
                uncompressed: le32(d, 16)?,
                stream_at: 20,
                container: "mark_rootfs+8",
            });
        }
    }
    // mark_rootfs, magic second: [compressed][magic][dict][uncompressed]
    if le32(d, 4) == Some(MAGIC) {
        let dict_size = le32(d, 8)?;
        if (DICT_MIN..=DICT_MAX).contains(&dict_size) {
            return Some(Header {
                dict_size,
                uncompressed: le32(d, 12)?,
                stream_at: 16,
                container: "mark_rootfs+4",
            });
        }
    }
    // Bare jz_lzma_out.bin: [dict][uncompressed]
    let dict_size = le32(d, 0)?;
    if (DICT_MIN..=DICT_MAX).contains(&dict_size) {
        let uncompressed = le32(d, 4)?;
        // A plausible dictionary alone is weak evidence, so require the size to
        // be sane too: two little-endian words can be anything.
        if uncompressed > 0 && uncompressed <= MAX_OUTPUT as u32 {
            return Some(Header {
                dict_size,
                uncompressed,
                stream_at: 8,
                container: "raw",
            });
        }
    }
    None
}

/// Ceiling on a decode, so a corrupt stream cannot exhaust memory. Generous
/// against real firmware: the largest of these seen is about 6 MiB.
const MAX_OUTPUT: usize = 64 << 20;

/// Detect the container and decode. None when these are not JZLZMA bytes.
pub fn decompress(d: &[u8]) -> Option<Vec<u8>> {
    let h = parse(d)?;
    let out = decode(d.get(h.stream_at..)?, h.uncompressed as usize)?;
    // The container states the size; a decode that falls short of it is a
    // wrong guess about the format rather than a truncated file.
    (out.len() == h.uncompressed as usize).then_some(out)
}

/// LSB-first bit reader over the packed stream.
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
    buf: u64,
    have: u32,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Bits {
            data,
            at: 0,
            buf: 0,
            have: 0,
        }
    }

    fn fill(&mut self, want: u32) -> Option<()> {
        while self.have < want {
            let byte = *self.data.get(self.at)?;
            self.buf |= (byte as u64) << self.have;
            self.at += 1;
            self.have += 8;
        }
        Some(())
    }

    /// `k` bits in stream order.
    fn raw(&mut self, k: u32) -> Option<u32> {
        if k == 0 {
            return Some(0);
        }
        self.fill(k)?;
        let v = (self.buf & ((1u64 << k) - 1)) as u32;
        self.buf >>= k;
        self.have -= k;
        Some(v)
    }

    /// `k` bits, most significant first.
    ///
    /// Two fields in the distance decoder are read with `raw` instead: the
    /// reference reverses them a second time, and the two reversals cancel.
    /// Folding that into the reader looks like a tidy-up and is not -- get it
    /// wrong and literals still decode perfectly while match distances are
    /// subtly off, so the output is the right length and nearly plausible.
    fn num(&mut self, k: u32) -> Option<u32> {
        if k == 0 {
            return Some(0);
        }
        Some(self.raw(k)?.reverse_bits() >> (32 - k))
    }

    fn bit(&mut self) -> Option<u32> {
        self.raw(1)
    }

    fn length(&mut self) -> Option<u32> {
        if self.bit()? == 0 {
            return Some(self.num(3)? + 2);
        }
        if self.bit()? == 0 {
            return Some(self.num(3)? + 10);
        }
        Some(self.num(8)? + 18)
    }

    fn dist(&mut self) -> Option<u32> {
        let pos_slot = self.num(6)?;
        if pos_slot < K_START_POS_MODEL {
            return Some(pos_slot);
        }
        let direct = (pos_slot >> 1) - 1;
        let mut pos = (2 | (pos_slot & 1)) << direct;
        if pos_slot < K_END_POS_MODEL {
            pos += self.raw(direct)?;
        } else {
            pos += self.num(direct - K_NUM_ALIGN_BITS)? << K_NUM_ALIGN_BITS;
            pos += self.raw(K_NUM_ALIGN_BITS)?;
        }
        Some(pos)
    }
}

/// Decode a bare stream, stopping at `expected` bytes.
pub fn decode(stream: &[u8], expected: usize) -> Option<Vec<u8>> {
    if expected > MAX_OUTPUT {
        return None;
    }
    let mut bits = Bits::new(stream);
    let mut out: Vec<u8> = Vec::with_capacity(expected.min(MAX_OUTPUT));
    let mut reps = [0u32; 4];

    // Running out of bits is how a stream ends; every other failure is a
    // malformed one, and both leave through the same None.
    loop {
        if out.len() >= expected {
            break;
        }
        let Some(marker) = bits.bit() else { break };
        if marker == 0 {
            let Some(byte) = bits.num(8) else { break };
            out.push(byte as u8);
            continue;
        }

        let mut size = 0u32;
        let Some(b1) = bits.bit() else { break };
        if b1 == 0 {
            let Some(len) = bits.length() else { break };
            let Some(d) = bits.dist() else { break };
            size = len;
            reps.rotate_right(1);
            reps[0] = d;
        } else {
            let Some(b2) = bits.bit() else { break };
            if b2 == 0 {
                let Some(b3) = bits.bit() else { break };
                if b3 == 0 {
                    size = 1;
                }
            } else {
                let Some(b3) = bits.bit() else { break };
                let take = if b3 == 0 {
                    1
                } else {
                    let Some(b4) = bits.bit() else { break };
                    if b4 == 0 {
                        2
                    } else {
                        3
                    }
                };
                let d = reps[take];
                reps.copy_within(0..take, 1);
                reps[0] = d;
            }
        }
        if size == 0 {
            let Some(len) = bits.length() else { break };
            size = len;
        }

        let have = out.len();
        let Some(start) = have.checked_sub(reps[0] as usize + 1) else {
            return None; // a back-reference before the start of the output
        };
        let mut remaining = size as usize;
        let mut from = start;
        while remaining > 0 {
            if out.len() >= MAX_OUTPUT {
                return None;
            }
            let end = (from + remaining).min(out.len());
            if end <= from {
                return None;
            }
            out.extend_from_within(from..end);
            remaining -= end - from;
            from = end;
        }
    }

    out.truncate(expected);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_a_bare_container() {
        let mut d = Vec::new();
        d.extend_from_slice(&0x0001_0000u32.to_le_bytes()); // dict
        d.extend_from_slice(&1234u32.to_le_bytes()); // uncompressed
        d.extend_from_slice(&[0; 8]);
        let h = parse(&d).unwrap();
        assert_eq!(h.container, "raw");
        assert_eq!(h.dict_size, 0x10000);
        assert_eq!(h.uncompressed, 1234);
        assert_eq!(h.stream_at, 8);
    }

    #[test]
    fn finds_the_wrapper_at_either_offset() {
        // magic second
        let mut a = Vec::new();
        a.extend_from_slice(&999u32.to_le_bytes());
        a.extend_from_slice(&MAGIC.to_le_bytes());
        a.extend_from_slice(&0x0001_0000u32.to_le_bytes());
        a.extend_from_slice(&4321u32.to_le_bytes());
        a.extend_from_slice(&[0; 8]);
        let h = parse(&a).unwrap();
        assert_eq!(h.container, "mark_rootfs+4");
        assert_eq!(h.stream_at, 16);
        assert_eq!(h.uncompressed, 4321);

        // magic third
        let mut b = Vec::new();
        b.extend_from_slice(&4321u32.to_le_bytes());
        b.extend_from_slice(&999u32.to_le_bytes());
        b.extend_from_slice(&MAGIC.to_le_bytes());
        b.extend_from_slice(&0x0001_0000u32.to_le_bytes());
        b.extend_from_slice(&4321u32.to_le_bytes());
        b.extend_from_slice(&[0; 8]);
        let h = parse(&b).unwrap();
        assert_eq!(h.container, "mark_rootfs+8");
        assert_eq!(h.stream_at, 20);
        assert_eq!(h.uncompressed, 4321);
    }

    #[test]
    fn rejects_bytes_that_are_not_a_container() {
        assert!(parse(&[0; 32]).is_none()); // dict 0 is not plausible
        assert!(parse(b"hsqs____________________").is_none()); // a squashfs
        assert!(parse(&[0xFF; 32]).is_none()); // dict far too large
        assert!(parse(&[]).is_none());
    }

    #[test]
    fn a_literal_only_stream_round_trips() {
        // marker bit 0 then eight bits MSB-first, per literal.
        let mut bits: Vec<u8> = Vec::new();
        let mut acc = 0u32;
        let mut n = 0u32;
        let push = |v: u32, k: u32, bits: &mut Vec<u8>, acc: &mut u32, n: &mut u32| {
            for i in 0..k {
                if v >> i & 1 != 0 {
                    *acc |= 1 << *n;
                }
                *n += 1;
                if *n == 8 {
                    bits.push(*acc as u8);
                    *acc = 0;
                    *n = 0;
                }
            }
        };
        for byte in b"hello jzlzma" {
            push(0, 1, &mut bits, &mut acc, &mut n);
            // stored MSB-first, so reverse before writing it LSB-first
            push(
                (*byte as u32).reverse_bits() >> 24,
                8,
                &mut bits,
                &mut acc,
                &mut n,
            );
        }
        if n > 0 {
            bits.push(acc as u8);
        }
        assert_eq!(decode(&bits, 12).unwrap(), b"hello jzlzma");
    }

    #[test]
    fn a_short_decode_is_not_passed_off_as_success() {
        // Header promises far more than the stream can deliver.
        let mut d = Vec::new();
        d.extend_from_slice(&0x0001_0000u32.to_le_bytes());
        d.extend_from_slice(&100_000u32.to_le_bytes());
        d.extend_from_slice(&[0x00, 0x01, 0x02]);
        assert!(decompress(&d).is_none());
    }
}
