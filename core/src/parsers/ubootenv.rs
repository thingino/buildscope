//! U-Boot environment image: CRC-32 (IEEE) over the payload, then
//! NUL-separated `key=value` records terminated by an empty record.
//! Redundant-environment images carry one extra flags byte after the CRC.

use super::le_u32;
use crate::crc::crc32_ieee;

#[derive(Debug, Clone, PartialEq)]
pub struct UbootEnvInfo {
    pub crc_ok: bool,
    pub redundant: bool,
    pub total_bytes: u64,
    /// Header + records + terminating NUL: the bytes that actually matter.
    pub used_bytes: u64,
    pub vars: Vec<(String, String)>,
}

fn parse_vars(payload: &[u8]) -> Option<(Vec<(String, String)>, u64)> {
    let mut vars = Vec::new();
    let mut pos = 0usize;
    loop {
        if pos >= payload.len() {
            // No explicit terminator: accept only if we got something.
            return if vars.is_empty() {
                None
            } else {
                Some((vars, payload.len() as u64))
            };
        }
        if payload[pos] == 0 {
            // Empty record: end of environment.
            return Some((vars, (pos + 1) as u64));
        }
        let end = payload[pos..].iter().position(|&b| b == 0)? + pos;
        let rec = &payload[pos..end];
        let eq = rec.iter().position(|&b| b == b'=')?;
        let key = std::str::from_utf8(&rec[..eq]).ok()?;
        if key.is_empty()
            || !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-.,+#".contains(&b))
        {
            return None;
        }
        let val = String::from_utf8_lossy(&rec[eq + 1..]).to_string();
        vars.push((key.to_string(), val));
        pos = end + 1;
    }
}

pub fn parse(data: &[u8]) -> Option<UbootEnvInfo> {
    if data.len() < 8 {
        return None;
    }
    let stored = le_u32(data, 0)?;

    let plain_ok = crc32_ieee(&data[4..]) == stored;
    let redundant_ok = !plain_ok && crc32_ieee(&data[5..]) == stored;

    let (offset, crc_ok, redundant) = if plain_ok {
        (4usize, true, false)
    } else if redundant_ok {
        (5usize, true, true)
    } else {
        (4usize, false, false)
    };

    let (vars, payload_used) = parse_vars(&data[offset..])?;
    if !crc_ok && vars.len() < 2 {
        // Without a valid CRC, demand more evidence before calling it an env.
        return None;
    }
    Some(UbootEnvInfo {
        crc_ok,
        redundant,
        total_bytes: data.len() as u64,
        used_bytes: offset as u64 + payload_used,
        vars,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic(pairs: &[(&str, &str)], size: usize) -> Vec<u8> {
        let mut payload = Vec::new();
        for (k, v) in pairs {
            payload.extend_from_slice(k.as_bytes());
            payload.push(b'=');
            payload.extend_from_slice(v.as_bytes());
            payload.push(0);
        }
        payload.push(0);
        payload.resize(size - 4, 0);
        let mut out = crc32_ieee(&payload).to_le_bytes().to_vec();
        out.extend_from_slice(&payload);
        out
    }

    #[test]
    fn parses_synthetic() {
        let img = synthetic(
            &[("bootcmd", "sf probe"), ("mtdparts", "spi:1m(a),-(b)")],
            65536,
        );
        let info = parse(&img).unwrap();
        assert!(info.crc_ok);
        assert!(!info.redundant);
        assert_eq!(info.vars.len(), 2);
        assert_eq!(info.vars[1].0, "mtdparts");
        // 4 (crc) + records + terminator
        let expected = 4 + "bootcmd=sf probe".len() + 1 + "mtdparts=spi:1m(a),-(b)".len() + 1 + 1;
        assert_eq!(info.used_bytes, expected as u64);
    }

    #[test]
    fn bad_crc_needs_evidence() {
        let mut img = synthetic(&[("a", "b"), ("c", "d")], 4096);
        img[0] ^= 0xFF;
        let info = parse(&img).unwrap();
        assert!(!info.crc_ok);
        assert_eq!(info.vars.len(), 2);

        let mut single = synthetic(&[("a", "b")], 4096);
        single[0] ^= 0xFF;
        assert!(parse(&single).is_none());
    }

    #[test]
    fn rejects_binary_noise() {
        let noise: Vec<u8> = (0..4096u32).map(|i| (i * 7 + 1) as u8).collect();
        assert!(parse(&noise).is_none());
    }
}
