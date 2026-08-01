//! Fleet snapshot: the two artifacts a CI run publishes for a whole matrix of
//! builds. A small index, which a viewer loads first to fill its picker, and
//! one gzipped tar of every report, fetched only when a build is opened.
//!
//! The archive is written here rather than shelled out to tar(1) and gzip(1)
//! so the command behaves the same on any host, and so the index and the
//! archive can never disagree about which member holds which build.

use buildscope_core::crc::crc32_ieee;
use buildscope_core::report::Report;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

const BLOCK: usize = 512;

/// A gzip container around raw deflate. miniz_oxide offers the zlib and raw
/// wrappers but not this one, so the 10-byte header and 8-byte trailer are
/// written by hand. mtime and OS are fixed rather than sampled, so two runs
/// over the same reports produce byte-identical output.
fn gzip(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255];
    out.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(data, 9));
    out.extend_from_slice(&crc32_ieee(data).to_le_bytes());
    // ISIZE is the uncompressed length mod 2^32, which is what the truncating
    // cast gives; no report set comes close to 4 GiB anyway.
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out
}

/// One ustar entry: a 512-byte header, the data, and padding out to the next
/// block boundary.
fn tar_entry(out: &mut Vec<u8>, name: &str, data: &[u8]) -> io::Result<()> {
    if name.len() >= 100 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("tar member name too long: {name}"),
        ));
    }
    let start = out.len();
    out.resize(start + BLOCK, 0);
    let put = |out: &mut Vec<u8>, off: usize, s: &str| {
        out[start + off..start + off + s.len()].copy_from_slice(s.as_bytes());
    };
    put(out, 0, name);
    put(out, 100, "0000644\0"); // mode
    put(out, 108, "0000000\0"); // uid
    put(out, 116, "0000000\0"); // gid
    put(out, 124, &format!("{:011o}\0", data.len()));
    put(out, 136, "00000000000\0"); // mtime 0, for reproducible output
    put(out, 148, "        "); // checksum reads as spaces while it is summed
    out[start + 156] = b'0'; // typeflag: regular file
    put(out, 257, "ustar\0");
    put(out, 263, "00");
    let sum: u32 = out[start..start + BLOCK].iter().map(|&b| b as u32).sum();
    put(out, 148, &format!("{sum:06o}\0 "));

    out.extend_from_slice(data);
    out.resize(out.len() + (BLOCK - data.len() % BLOCK) % BLOCK, 0);
    Ok(())
}

/// A build name reduced to something that cannot be a path.
///
/// Separators and traversal are the point, but control characters go too: the
/// name is printed to a terminal elsewhere, and an archive listing is a
/// terminal too.
fn safe_member(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+') {
                c
            } else {
                '_'
            }
        })
        .collect();
    // A leading dot would still hide the file, and "." or ".." are paths even
    // with no separator left in them.
    let trimmed = cleaned.trim_matches('.');
    if trimmed.is_empty() {
        "build".to_string()
    } else {
        trimmed.chars().take(80).collect()
    }
}

/// Write `fleet-index.json` and `fleet-reports.tar.gz` into `out_dir`.
pub fn build_fleet(reports: &[Report], out_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(out_dir)?;

    // Sorted, so a rerun over the same builds produces the same bytes and the
    // picker is in a predictable order regardless of matrix completion order.
    let mut ordered: Vec<&Report> = reports.iter().collect();
    ordered.sort_by(|a, b| a.build.name.cmp(&b.build.name));

    let mut seen: HashMap<&str, u32> = HashMap::new();
    let mut tar = Vec::new();
    let mut entries = Vec::new();
    for r in ordered {
        // Two builds can share a name -- a matrix over near-identical
        // defconfigs will do it -- and each still needs its own member.
        let base = r.build.name.as_str();
        let nth = {
            let n = seen.entry(base).or_insert(0);
            *n += 1;
            *n
        };
        // The build name reaches a tar member name, and a report is an input
        // like any other: one naming itself ../../etc/something would write
        // outside whatever directory the archive is unpacked into. Reduced to
        // a leaf with a known-safe alphabet, so nothing in it can be a path.
        let safe = safe_member(base);
        let file = if nth == 1 {
            format!("{safe}.json")
        } else {
            format!("{safe}-{nth}.json")
        };

        let body = serde_json::to_string(r).expect("serialize report");
        tar_entry(&mut tar, &file, body.as_bytes())?;

        let mut entry = crate::export::index_entry(r);
        entry.insert("file".into(), serde_json::Value::String(file));
        entries.push(serde_json::Value::Object(entry));
    }
    // Two zero blocks end an archive.
    tar.resize(tar.len() + 2 * BLOCK, 0);

    fs::write(
        out_dir.join("fleet-index.json"),
        serde_json::json!({ "reports": entries }).to_string(),
    )?;
    fs::write(out_dir.join("fleet-reports.tar.gz"), gzip(&tar))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gzip_framing_and_roundtrip() {
        let body = b"buildscope fleet snapshot, with enough text to actually deflate";
        let gz = gzip(body);
        assert_eq!(
            &gz[..4],
            &[0x1f, 0x8b, 8, 0],
            "gzip magic + deflate + flags"
        );
        let n = gz.len();
        assert_eq!(
            u32::from_le_bytes(gz[n - 4..].try_into().unwrap()),
            body.len() as u32,
            "ISIZE trailer"
        );
        assert_eq!(
            u32::from_le_bytes(gz[n - 8..n - 4].try_into().unwrap()),
            crc32_ieee(body),
        );
        let back = miniz_oxide::inflate::decompress_to_vec(&gz[10..n - 8]).unwrap();
        assert_eq!(back, body);
    }

    #[test]
    fn tar_header_fields_and_checksum() {
        let mut tar = Vec::new();
        tar_entry(&mut tar, "cam.json", b"{}").unwrap();
        assert_eq!(
            tar.len(),
            2 * BLOCK,
            "one header block, one padded data block"
        );
        assert_eq!(&tar[..8], b"cam.json");
        assert_eq!(&tar[257..263], b"ustar\0");
        assert_eq!(tar[156], b'0', "regular file");
        assert_eq!(&tar[124..136], b"00000000002\0", "size in octal");
        assert_eq!(&tar[BLOCK..BLOCK + 2], b"{}");

        // The stored checksum must equal the sum with its own field blanked.
        let stored = u32::from_str_radix(
            std::str::from_utf8(&tar[148..154])
                .unwrap()
                .trim_matches('\0'),
            8,
        )
        .unwrap();
        let mut blanked = tar[..BLOCK].to_vec();
        blanked[148..156].fill(b' ');
        let sum: u32 = blanked.iter().map(|&b| b as u32).sum();
        assert_eq!(stored, sum);
    }

    #[test]
    fn member_names_cannot_be_paths() {
        // The property that matters: whatever a report calls itself, the
        // member is one path component and not a relative one.
        for name in [
            "../../../etc/cron.d/evil",
            "/absolute/path",
            "..",
            "...",
            "",
            ".hidden",
            "a\u{1b}]0;x\u{7}b",
            "C:\\windows\\system32",
            "with spaces and \u{0}nul",
        ] {
            let m = safe_member(name);
            assert!(!m.is_empty(), "{name:?} produced an empty member");
            assert!(!m.contains('/'), "{name:?} kept a separator: {m}");
            assert!(!m.contains('\\'), "{name:?} kept a separator: {m}");
            assert!(m != "." && m != "..", "{name:?} is still a path: {m}");
            assert!(!m.starts_with('.'), "{name:?} still hides the file: {m}");
            assert!(
                m.chars().all(|c| !c.is_control()),
                "{name:?} kept a control character: {m:?}"
            );
        }
        // An ordinary name is left alone.
        assert_eq!(
            safe_member("teacup_t31x-3.10.14-uclibc"),
            "teacup_t31x-3.10.14-uclibc"
        );
    }

    #[test]
    fn tar_rejects_an_overlong_name() {
        let mut tar = Vec::new();
        assert!(tar_entry(&mut tar, &"x".repeat(100), b"{}").is_err());
    }
}
