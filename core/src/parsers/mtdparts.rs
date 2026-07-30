//! Parser for the kernel `mtdparts=` command-line partition syntax:
//!
//!   mtdparts=<mtd-id>:<size>[@<offset>](<name>)[ro],...
//!
//! Sizes take k/m/g binary suffixes; `-` means "remainder of the device"
//! and is resolvable only once a device total is known. Offsets default to
//! "immediately after the previous partition".

#[derive(Debug, Clone, PartialEq)]
pub struct MtdPartition {
    pub name: String,
    pub offset: u64,
    /// None for an unresolved remainder (`-`) entry.
    pub size: Option<u64>,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MtdParts {
    pub mtd_id: String,
    pub partitions: Vec<MtdPartition>,
    /// Highest offset+size among sized partitions.
    pub declared_end: u64,
}

impl MtdParts {
    /// Fill remainder (`-`) sizes given the device total.
    pub fn resolve_remainders(&mut self, total: u64) {
        for p in &mut self.partitions {
            if p.size.is_none() && total > p.offset {
                p.size = Some(total - p.offset);
            }
        }
        self.declared_end = self
            .partitions
            .iter()
            .filter_map(|p| p.size.map(|s| p.offset + s))
            .max()
            .unwrap_or(self.declared_end);
    }
}

fn parse_size(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let (num, mult) = match s.as_bytes().last()? {
        b'k' | b'K' => (&s[..s.len() - 1], 1024u64),
        b'm' | b'M' => (&s[..s.len() - 1], 1024 * 1024),
        b'g' | b'G' => (&s[..s.len() - 1], 1024 * 1024 * 1024),
        _ => (s, 1),
    };
    let n: u64 = num.parse().ok()?;
    n.checked_mul(mult)
}

/// Parse one partition definition like `4096k@0x1c0000(rootfs)ro`.
fn parse_part(s: &str, cursor: &mut u64) -> Option<MtdPartition> {
    let open = s.find('(')?;
    let close = s.find(')')?;
    if close < open || open == 0 {
        return None;
    }
    let name = s[open + 1..close].to_string();
    if name.is_empty() {
        return None;
    }
    let read_only = s[close + 1..].eq_ignore_ascii_case("ro");
    if !read_only && !s[close + 1..].is_empty() {
        return None;
    }

    let sizeofs = &s[..open];
    let (size_str, offset) = match sizeofs.split_once('@') {
        Some((sz, ofs)) => {
            let o = if let Some(hex) = ofs.strip_prefix("0x") {
                u64::from_str_radix(hex, 16).ok()?
            } else {
                parse_size(ofs)?
            };
            (sz, Some(o))
        }
        None => (sizeofs, None),
    };

    let size = if size_str == "-" {
        None
    } else {
        Some(parse_size(size_str)?)
    };

    let offset = offset.unwrap_or(*cursor);
    if let Some(sz) = size {
        *cursor = offset + sz;
    } else {
        // Remainder runs to the end; nothing may follow without an
        // explicit offset, so leave the cursor where it is.
    }

    Some(MtdPartition {
        name,
        offset,
        size,
        read_only,
    })
}

/// Parse a full spec. Accepts with or without the leading `mtdparts=`.
/// Multi-device specs (`;`-separated) yield the first device.
pub fn parse(spec: &str) -> Option<MtdParts> {
    let s = spec.trim();
    let s = s.strip_prefix("mtdparts=").unwrap_or(s);
    let s = s.split(';').next()?;
    let (id, defs) = s.split_once(':')?;
    if id.is_empty() || id.contains(['$', '{', ' ']) {
        return None;
    }
    let mut cursor = 0u64;
    let mut partitions = Vec::new();
    for part in defs.split(',') {
        partitions.push(parse_part(part.trim(), &mut cursor)?);
    }
    if partitions.is_empty() {
        return None;
    }
    let declared_end = partitions
        .iter()
        .filter_map(|p| p.size.map(|sz| p.offset + sz))
        .max()
        .unwrap_or(0);
    Some(MtdParts {
        mtd_id: id.to_string(),
        partitions,
        declared_end,
    })
}

/// Find the first parseable `mtdparts=` occurrence in free-form text
/// (an environment source file, a boot log, a .config).
pub fn find_in_text(text: &str) -> Option<MtdParts> {
    for (idx, _) in text.match_indices("mtdparts=") {
        let tail = &text[idx..];
        let end = tail
            .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
            .unwrap_or(tail.len());
        if let Some(parsed) = parse(&tail[..end]) {
            return Some(parsed);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &str = "mtdparts=jz_sfc:320k(boot),64k(env),1408k(kernel),4096k(rootfs),10496k(data),16384k@0(all)";

    #[test]
    fn parses_cumulative_offsets() {
        let p = parse(SPEC).unwrap();
        assert_eq!(p.mtd_id, "jz_sfc");
        assert_eq!(p.partitions.len(), 6);
        let by_name = |n: &str| p.partitions.iter().find(|x| x.name == n).unwrap();
        assert_eq!(by_name("boot").offset, 0);
        assert_eq!(by_name("env").offset, 327_680);
        assert_eq!(by_name("kernel").offset, 393_216);
        assert_eq!(by_name("rootfs").offset, 1_835_008);
        assert_eq!(by_name("data").offset, 6_029_312);
        assert_eq!(by_name("data").size, Some(10_747_904));
        assert_eq!(by_name("all").offset, 0);
        assert_eq!(p.declared_end, 16 * 1024 * 1024);
    }

    #[test]
    fn remainder_and_resolution() {
        let mut p = parse("spi0.0:1m(u-boot),256k(env),-(rootfs)").unwrap();
        assert_eq!(p.partitions[2].size, None);
        p.resolve_remainders(8 * 1024 * 1024);
        assert_eq!(p.partitions[2].size, Some(8 * 1024 * 1024 - 1_310_720));
        assert_eq!(p.declared_end, 8 * 1024 * 1024);
    }

    #[test]
    fn finds_in_text_skipping_variable_refs() {
        let text = "bootargs=console=ttyS1 mtdparts=${mtdparts}\nmtdparts=nor0:64k(a),-(b)\n";
        let p = find_in_text(text).unwrap();
        assert_eq!(p.mtd_id, "nor0");
        assert_eq!(p.partitions.len(), 2);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("noparts").is_none());
        assert!(parse("id:").is_none());
        assert!(parse("id:64q(a)").is_none());
        assert!(find_in_text("nothing here").is_none());
    }

    #[test]
    fn hex_offset_and_ro() {
        let p = parse("nand0:2m@0x100000(kernel)ro,-(ubi)").unwrap();
        assert_eq!(p.partitions[0].offset, 0x10_0000);
        assert!(p.partitions[0].read_only);
        assert!(!p.partitions[1].read_only);
    }
}
