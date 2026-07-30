//! Read squashfs images built by mksquashfs, and check the listing and the
//! per-file compressed costs against unsquashfs.
//!
//! Skipped when the tools are absent, so the suite still runs anywhere.

use buildscope_core::parsers::{squashfs, squashfs_reader};
use std::process::Command;

fn have(tool: &str) -> bool {
    Command::new("which")
        .arg(tool)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Build a tree and pack it with the given compressor.
fn build(dir: &std::path::Path, comp: &str) -> Option<Vec<u8>> {
    let src = dir.join(format!("src-{comp}"));
    std::fs::create_dir_all(src.join("bin")).ok()?;
    std::fs::create_dir_all(src.join("etc/conf.d")).ok()?;
    // A large file (multi-block), a small one (fragment), and more smalls to
    // share that fragment, plus a symlink and an empty file.
    std::fs::write(src.join("bin/big"), vec![0x41u8; 400_000]).ok()?;
    std::fs::write(src.join("bin/small"), vec![0x42u8; 900]).ok()?;
    std::fs::write(src.join("etc/conf.d/one"), b"one\n").ok()?;
    std::fs::write(src.join("etc/conf.d/two"), vec![0x43u8; 5000]).ok()?;
    std::fs::write(src.join("etc/empty"), b"").ok()?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("big", src.join("bin/link")).ok();

    let out = dir.join(format!("{comp}.squashfs"));
    let _ = std::fs::remove_file(&out);
    let status = Command::new("mksquashfs")
        .args([
            src.to_str()?,
            out.to_str()?,
            "-comp",
            comp,
            "-no-progress",
            "-quiet",
        ])
        .status()
        .ok()?;
    status.success().then(|| std::fs::read(&out).ok()).flatten()
}

#[test]
fn matches_unsquashfs_on_real_images() {
    if !have("mksquashfs") || !have("unsquashfs") {
        eprintln!("skipping: mksquashfs/unsquashfs not installed");
        return;
    }
    let dir = std::env::temp_dir().join("buildscope-sqfs-test");
    std::fs::create_dir_all(&dir).unwrap();

    let mut tested = 0;
    for comp in ["gzip", "xz", "zstd", "lz4"] {
        let Some(image) = build(&dir, comp) else {
            eprintln!("skipping {comp}: mksquashfs cannot build it here");
            continue;
        };
        let sb = squashfs::parse(&image).expect("superblock");
        assert_eq!(sb.compression, comp);
        let listing = match squashfs_reader::read(&image, &sb) {
            Ok(l) => l,
            Err(e) => panic!("{comp}: {e:?}"),
        };

        // Ground truth: unsquashfs lists every path with its size.
        let out = Command::new("unsquashfs")
            .args([
                "-ll",
                "-no-progress",
                dir.join(format!("{comp}.squashfs")).to_str().unwrap(),
            ])
            .output()
            .expect("unsquashfs");
        let text = String::from_utf8_lossy(&out.stdout);
        let mut truth: Vec<(String, u64)> = Vec::new();
        for line in text.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            // permissions owner size date time path
            if f.len() < 6 || !f[0].starts_with(['-', 'd', 'l']) {
                continue;
            }
            let Ok(size) = f[2].parse::<u64>() else {
                continue;
            };
            let path = f[5].trim_start_matches("squashfs-root");
            if path.is_empty() || line.starts_with('l') {
                continue; // the root itself, and symlink sizes differ by tool
            }
            if line.starts_with('-') {
                truth.push((path.to_string(), size));
            }
        }
        assert!(!truth.is_empty(), "{comp}: no ground truth parsed");

        for (path, size) in &truth {
            let got = listing
                .entries
                .iter()
                .find(|e| &e.path == path)
                .unwrap_or_else(|| panic!("{comp}: {path} missing from the listing"));
            assert_eq!(got.bytes, *size, "{comp}: {path} size");
            assert_eq!(got.kind, "file", "{comp}: {path} kind");
            assert!(
                got.compressed_bytes.is_some(),
                "{comp}: {path} has no compressed cost"
            );
        }

        // Every file's cost together must be within the image's data area:
        // it cannot exceed what the whole image occupies.
        assert!(
            listing.compressed_bytes <= sb.bytes_used,
            "{comp}: costs {} exceed the image's {}",
            listing.compressed_bytes,
            sb.bytes_used
        );
        // The big file compresses well, so its cost must be far under its size.
        let big = listing
            .entries
            .iter()
            .find(|e| e.path == "/bin/big")
            .unwrap();
        assert!(
            big.compressed_bytes.unwrap() < big.bytes / 10,
            "{comp}: a 400k run of one byte should compress hard, got {:?}",
            big.compressed_bytes
        );
        assert_eq!(
            listing.file_count as usize,
            truth.len(),
            "{comp}: file count"
        );
        tested += 1;
    }
    assert!(tested > 0, "no compressor could be tested");
}
