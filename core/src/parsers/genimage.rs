//! Minimal genimage.cfg reader: enough to recover a partition layout from
//! the standard Buildroot image-assembly config. Handles the common shape:
//!
//!   image sdcard.img {
//!     hdimage { }
//!     partition boot { offset = 8K; image = "boot.vfat" }
//!     partition rootfs { size = 512M; image = "rootfs.ext4" }
//!   }
//!
//! Both `key = value` and `key = "value"` forms, `#` comments, nested
//! blocks. Offsets default to "after the previous partition" like hdimage.

#[derive(Debug, Clone, PartialEq)]
pub struct GenimagePartition {
    pub name: String,
    pub offset: Option<u64>,
    pub size: Option<u64>,
    pub image: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenimageImage {
    pub name: String,
    pub partitions: Vec<GenimagePartition>,
}

fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim().trim_matches('"');
    if s.is_empty() {
        return None;
    }
    let (num, mult) = match s.as_bytes().last()? {
        b'k' | b'K' => (&s[..s.len() - 1], 1024u64),
        b'm' | b'M' => (&s[..s.len() - 1], 1024 * 1024),
        b'g' | b'G' => (&s[..s.len() - 1], 1024 * 1024 * 1024),
        _ => (s, 1),
    };
    if let Some(hex) = num.strip_prefix("0x") {
        return u64::from_str_radix(hex, 16).ok()?.checked_mul(mult);
    }
    num.parse::<u64>().ok()?.checked_mul(mult)
}

/// Strip comments, split into brace/token stream.
fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for raw_line in text.lines() {
        let line = raw_line.split('#').next().unwrap_or("");
        let mut cur = String::new();
        let mut in_str = false;
        for c in line.chars() {
            match c {
                '"' => {
                    in_str = !in_str;
                    cur.push(c);
                }
                '{' | '}' | '=' if !in_str => {
                    if !cur.trim().is_empty() {
                        tokens.push(cur.trim().to_string());
                    }
                    cur.clear();
                    tokens.push(c.to_string());
                }
                c if c.is_whitespace() && !in_str => {
                    if !cur.trim().is_empty() {
                        tokens.push(cur.trim().to_string());
                    }
                    cur.clear();
                }
                _ => cur.push(c),
            }
        }
        if !cur.trim().is_empty() {
            tokens.push(cur.trim().to_string());
        }
    }
    tokens
}

pub fn parse(text: &str) -> Option<Vec<GenimageImage>> {
    let tokens = tokenize(text);
    let mut images = Vec::new();
    let mut i = 0;

    // Walk `image <name> { ... }` blocks at any nesting level.
    while i < tokens.len() {
        if tokens[i] == "image" && i + 2 < tokens.len() && tokens[i + 2] == "{" {
            let img_name = tokens[i + 1].trim_matches('"').to_string();
            let mut partitions = Vec::new();
            let mut depth = 1;
            i += 3;
            while i < tokens.len() && depth > 0 {
                match tokens[i].as_str() {
                    "{" => depth += 1,
                    "}" => depth -= 1,
                    "partition" if i + 2 < tokens.len() && tokens[i + 2] == "{" => {
                        let pname = tokens[i + 1].trim_matches('"').to_string();
                        let mut pdepth = 1;
                        let mut part = GenimagePartition {
                            name: pname,
                            offset: None,
                            size: None,
                            image: None,
                        };
                        i += 3;
                        while i < tokens.len() && pdepth > 0 {
                            match tokens[i].as_str() {
                                "{" => pdepth += 1,
                                "}" => pdepth -= 1,
                                key if i + 2 < tokens.len() && tokens[i + 1] == "=" => {
                                    let val = tokens[i + 2].clone();
                                    match key {
                                        "offset" => part.offset = parse_size(&val),
                                        "size" => part.size = parse_size(&val),
                                        "image" => {
                                            part.image = Some(val.trim_matches('"').to_string())
                                        }
                                        _ => {}
                                    }
                                    i += 2;
                                }
                                _ => {}
                            }
                            i += 1;
                        }
                        partitions.push(part);
                        continue;
                    }
                    _ => {}
                }
                i += 1;
            }
            images.push(GenimageImage {
                name: img_name,
                partitions,
            });
            continue;
        }
        i += 1;
    }

    if images.iter().all(|im| im.partitions.is_empty()) {
        return None;
    }
    Some(images)
}

/// Resolve cumulative offsets (hdimage semantics: next partition follows the
/// previous one when no explicit offset is given).
pub fn resolve_offsets(
    parts: &[GenimagePartition],
) -> Vec<(String, u64, Option<u64>, Option<String>)> {
    let mut out = Vec::new();
    let mut cursor = 0u64;
    for p in parts {
        let offset = p.offset.unwrap_or(cursor);
        if let Some(sz) = p.size {
            cursor = offset + sz;
        }
        out.push((p.name.clone(), offset, p.size, p.image.clone()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
# comment
image sdcard.img {
  hdimage {
  }

  partition boot {
    partition-type = 0xC
    bootable = "true"
    offset = 8K
    image = "boot.vfat"
    size = 32M
  }

  partition rootfs {
    partition-type = 0x83
    image = "rootfs.ext4"
    size = 512M
  }
}
"#;

    #[test]
    fn parses_sample() {
        let images = parse(SAMPLE).unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].name, "sdcard.img");
        let parts = &images[0].partitions;
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].offset, Some(8 * 1024));
        assert_eq!(parts[0].size, Some(32 * 1024 * 1024));
        assert_eq!(parts[1].image.as_deref(), Some("rootfs.ext4"));

        let resolved = resolve_offsets(parts);
        assert_eq!(resolved[1].1, 8 * 1024 + 32 * 1024 * 1024);
    }

    #[test]
    fn rejects_no_partitions() {
        assert!(parse("image foo.img { hdimage { } }").is_none());
        assert!(parse("not a config").is_none());
    }
}
