//! cpio archives in the `newc` and `crc` formats, which is what Buildroot
//! writes for an initramfs rootfs.
//!
//! Everything is plain ASCII hex headers with the file data laid out between
//! them, so an archive yields a complete listing -- names, sizes and kinds --
//! with no decompression at all, the same way jffs2 does. A `.cpio.gz` or
//! `.cpio.xz` is a compressed stream and stays opaque; the uncompressed
//! `rootfs.cpio` beside it is the one to read.

const MAGIC_NEWC: &[u8; 6] = b"070701";
const MAGIC_CRC: &[u8; 6] = b"070702";
const HEADER_LEN: usize = 110;
const TRAILER: &str = "TRAILER!!!";
/// Same ceiling the other listing parsers use.
const MAX_ENTRIES: usize = 4000;

const S_IFMT: u32 = 0o170000;
const S_IFREG: u32 = 0o100000;
const S_IFDIR: u32 = 0o040000;
const S_IFLNK: u32 = 0o120000;

#[derive(Debug, Clone, PartialEq)]
pub struct CpioEntry {
    pub path: String,
    pub bytes: u64,
    /// "file" | "dir" | "link" | "other"
    pub kind: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CpioInfo {
    /// "newc" | "crc"
    pub format: &'static str,
    pub entry_count: u32,
    pub file_count: u32,
    pub dir_count: u32,
    pub link_count: u32,
    /// Sum of the file data, headers and padding excluded.
    pub content_bytes: u64,
    /// Where the archive ends: everything past this is padding.
    pub archive_bytes: u64,
    pub entries: Vec<CpioEntry>,
    pub entries_truncated: bool,
}

/// Header fields are eight ASCII hex digits, no prefix.
fn hex8(d: &[u8], off: usize) -> Option<u64> {
    let s = std::str::from_utf8(d.get(off..off + 8)?).ok()?;
    u64::from_str_radix(s, 16).ok()
}

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

/// Archive names are relative, usually written as `./etc/passwd`, and the root
/// of the tree is stored as a bare `.`. Render them as absolute paths so a
/// listing reads the way the running filesystem will.
fn normalise(name: &str) -> String {
    let t = name.trim_start_matches("./").trim_start_matches('/');
    if t.is_empty() || t == "." {
        "/".to_string()
    } else {
        format!("/{t}")
    }
}

fn kind_of(mode: u32) -> &'static str {
    match mode & S_IFMT {
        S_IFREG => "file",
        S_IFDIR => "dir",
        S_IFLNK => "link",
        _ => "other",
    }
}

pub fn parse(data: &[u8]) -> Option<CpioInfo> {
    let magic = data.get(..6)?;
    let format = if magic == MAGIC_NEWC {
        "newc"
    } else if magic == MAGIC_CRC {
        "crc"
    } else {
        return None;
    };

    let (mut entries, mut pos) = (Vec::new(), 0usize);
    let (mut files, mut dirs, mut links, mut count) = (0u32, 0u32, 0u32, 0u32);
    let mut content_bytes = 0u64;
    let mut truncated = false;

    loop {
        let h = data.get(pos..pos + HEADER_LEN)?;
        let m = &h[..6];
        if m != MAGIC_NEWC && m != MAGIC_CRC {
            // A concatenated archive is padded with zeros to a block; anything
            // else here means the archive is not what it claimed.
            return if count > 0 { break } else { None };
        }
        let mode = hex8(h, 14)? as u32;
        let file_size = hex8(h, 54)?;
        let name_size = hex8(h, 94)? as usize;
        if name_size == 0 || name_size > 4096 {
            return None;
        }

        let name_at = pos + HEADER_LEN;
        let raw = data.get(name_at..name_at + name_size)?;
        let name = String::from_utf8_lossy(&raw[..name_size.saturating_sub(1)]).into_owned();

        // The name is padded so the data starts on a four-byte boundary, and
        // the data is padded the same way before the next header.
        let data_at = align4(name_at + name_size);
        let next = align4(data_at + file_size as usize);

        if name == TRAILER {
            return Some(CpioInfo {
                format,
                entry_count: count,
                file_count: files,
                dir_count: dirs,
                link_count: links,
                content_bytes,
                // The trailer belongs to the archive; padding after it does not.
                archive_bytes: (data_at.max(next)) as u64,
                entries,
                entries_truncated: truncated,
            });
        }

        count += 1;
        match kind_of(mode) {
            "file" => files += 1,
            "dir" => dirs += 1,
            "link" => links += 1,
            _ => {}
        }
        content_bytes += file_size;
        if entries.len() < MAX_ENTRIES {
            entries.push(CpioEntry {
                path: normalise(&name),
                bytes: file_size,
                kind: kind_of(mode),
            });
        } else {
            truncated = true;
        }

        if next <= pos {
            return None; // no forward progress: refuse to spin
        }
        pos = next;
    }

    // Ran out of archive without a trailer: report what was read.
    (count > 0).then(|| CpioInfo {
        format,
        entry_count: count,
        file_count: files,
        dir_count: dirs,
        link_count: links,
        content_bytes,
        archive_bytes: pos as u64,
        entries,
        entries_truncated: truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(mode: u32, size: usize, name: &str) -> Vec<u8> {
        let mut h = Vec::new();
        h.extend_from_slice(MAGIC_NEWC);
        let f = |v: u64| format!("{v:08X}");
        for v in [
            1u64,             // ino
            mode as u64,      // mode
            0,                // uid
            0,                // gid
            1,                // nlink
            0,                // mtime
            size as u64,      // filesize
            0,
            0,
            0,
            0,                // dev/rdev
            name.len() as u64 + 1, // namesize, NUL included
            0,                // check
        ] {
            h.extend_from_slice(f(v).as_bytes());
        }
        assert_eq!(h.len(), HEADER_LEN);
        h.extend_from_slice(name.as_bytes());
        h.push(0);
        while h.len() % 4 != 0 {
            h.push(0);
        }
        h
    }

    fn archive(items: &[(u32, &str, &[u8])]) -> Vec<u8> {
        let mut a = Vec::new();
        for (mode, name, body) in items {
            a.extend_from_slice(&header(*mode, body.len(), name));
            a.extend_from_slice(body);
            while a.len() % 4 != 0 {
                a.push(0);
            }
        }
        a.extend_from_slice(&header(0, 0, TRAILER));
        a
    }

    #[test]
    fn lists_every_entry_with_sizes_and_kinds() {
        let img = archive(&[
            (S_IFDIR | 0o755, "bin", b""),
            (S_IFREG | 0o755, "bin/busybox", &[0xAA; 300]),
            (S_IFLNK | 0o777, "bin/sh", b"busybox"),
            (S_IFREG | 0o644, "etc/inittab", &[b'x'; 42]),
        ]);
        let c = parse(&img).expect("cpio");
        assert_eq!(c.format, "newc");
        assert_eq!(c.entry_count, 4);
        assert_eq!(c.file_count, 2);
        assert_eq!(c.dir_count, 1);
        assert_eq!(c.link_count, 1);
        assert_eq!(c.content_bytes, 300 + 7 + 42);
        assert!(!c.entries_truncated);

        let paths: Vec<&str> = c.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["/bin", "/bin/busybox", "/bin/sh", "/etc/inittab"]);
        let by = |p: &str| c.entries.iter().find(|e| e.path == p).unwrap();
        assert_eq!(by("/bin/busybox").bytes, 300);
        assert_eq!(by("/bin/busybox").kind, "file");
        assert_eq!(by("/bin").kind, "dir");
        assert_eq!(by("/bin/sh").kind, "link");
        // The archive ends at the trailer, before any block padding.
        assert_eq!(c.archive_bytes as usize, img.len());
    }

    /// `find . | cpio -o` writes relative names and stores the top of the tree
    /// as a bare dot, which has to read as the root rather than as "/.".
    #[test]
    fn relative_names_become_absolute_paths() {
        let img = archive(&[
            (S_IFDIR | 0o755, ".", b""),
            (S_IFREG | 0o644, "./etc/passwd", b"root"),
            (S_IFREG | 0o644, "/already/absolute", b"x"),
        ]);
        let c = parse(&img).unwrap();
        let paths: Vec<&str> = c.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["/", "/etc/passwd", "/already/absolute"]);
    }

    #[test]
    fn accepts_the_crc_variant() {
        let mut img = archive(&[(S_IFREG | 0o644, "a", b"hi")]);
        img[..6].copy_from_slice(MAGIC_CRC);
        assert_eq!(parse(&img).unwrap().format, "crc");
    }

    #[test]
    fn trailing_block_padding_is_not_content() {
        let mut img = archive(&[(S_IFREG | 0o644, "a", b"hi")]);
        let end = img.len();
        img.resize(end + 8192, 0); // pad to a block, as cpio -o does
        let c = parse(&img).unwrap();
        assert_eq!(c.archive_bytes as usize, end);
        assert_eq!(c.entry_count, 1);
    }

    #[test]
    fn rejects_non_cpio() {
        assert!(parse(&[0u8; 4096]).is_none());
        assert!(parse(b"070701").is_none()); // magic but no header
        assert!(parse(&vec![0xFFu8; 4096]).is_none());
        // an old binary-format archive is not newc
        assert!(parse(&[0xC7, 0x71, 0, 0, 0, 0, 0, 0]).is_none());
    }
}
