//! Artifact-only analysis: a bare composite flash image with no build tree.
//!
//! The layout is recovered from the image itself: a CRC-valid U-Boot
//! environment block found by scanning (its `mtdparts` variable is the
//! partition table), or an MBR for disk images. Each partition is then
//! carved and classified with the same format parsers used everywhere else.
//! Package attribution is impossible in this mode and is reported as such.

use crate::analyze::{classify, partition_role, used_bytes_of, Role};
use crate::parsers::{mbr, mtdparts, padding, ubootenv};
use crate::report::*;
use crate::snapshot::{ContextSource, ScanMode};
use serde_json::json;

/// Common environment sizes, most likely first.
const ENV_SIZES: &[usize] = &[0x10000, 0x8000, 0x4000, 0x20000, 0x2000, 0x40000, 0x1000];
/// Environment blocks sit on erase boundaries; 4 KiB covers small-sector NOR.
const SCAN_STEP: usize = 0x1000;

/// Cheap prefilter: right after the CRC (and optional redundant-env flags
/// byte) a real environment starts with `KEY=`.
fn plausible_env_at(data: &[u8], off: usize) -> bool {
    for skip in [4usize, 5] {
        let start = off + skip;
        let Some(win) = data.get(start..(start + 96).min(data.len())) else {
            continue;
        };
        for (i, &b) in win.iter().enumerate() {
            if b == b'=' {
                if i > 0 {
                    return true;
                }
                break;
            }
            if !(b.is_ascii_alphanumeric() || b"_-.,+#".contains(&b)) {
                break;
            }
        }
    }
    false
}

/// Scan for a CRC-valid environment block. Returns (offset, size, env).
pub fn find_env(data: &[u8]) -> Option<(usize, usize, ubootenv::UbootEnvInfo)> {
    let mut off = 0usize;
    while off + 8 <= data.len() {
        if plausible_env_at(data, off) {
            for &size in ENV_SIZES {
                if off + size > data.len() {
                    continue;
                }
                if let Some(info) = ubootenv::parse(&data[off..off + size]) {
                    if info.crc_ok {
                        return Some((off, size, info));
                    }
                }
            }
        }
        off += SCAN_STEP;
    }
    None
}

struct CarvedLayout {
    source: String,
    mtd_id: Option<String>,
    partitions: Vec<mtdparts::MtdPartition>,
    /// Size of the device the layout describes, which can exceed the file
    /// when the artifact was trimmed of its trailing erased space.
    device_bytes: u64,
}

pub fn carve_flash_image(file_name: &str, data: &[u8], root: &str, scan_mode: ScanMode) -> Report {
    let mut warnings: Vec<String> =
        vec!["artifact-only scan: no build tree, package attribution unavailable".to_string()];
    let total = data.len() as u64;
    let whole = classify(file_name, total, Some(data));
    let whole_pad = padding::analyze(data);

    // Layout: embedded env's mtdparts first, then a partition table.
    let env_hit = find_env(data);
    let mut layout: Option<CarvedLayout> = None;
    if let Some((off, _size, env)) = &env_hit {
        if let Some((_, v)) = env.vars.iter().find(|(k, _)| k == "mtdparts") {
            if let Some(mut p) = mtdparts::parse(v) {
                // The layout, not the file, defines the device size: a
                // released image may be trimmed of trailing erased space.
                let device = p.declared_end.max(total);
                p.resolve_remainders(device);
                layout = Some(CarvedLayout {
                    source: format!("mtdparts (embedded env @ 0x{off:X})"),
                    mtd_id: Some(p.mtd_id.clone()),
                    partitions: p.partitions,
                    device_bytes: device,
                });
            }
        }
        if layout.is_none() {
            warnings.push(format!(
                "embedded environment found at 0x{off:X} but it carries no mtdparts"
            ));
        }
    }
    if layout.is_none() {
        if let Some(parts) = mbr::parse(data) {
            let span = parts.iter().map(|p| p.offset + p.size).max().unwrap_or(0);
            if span <= total {
                layout = Some(CarvedLayout {
                    source: "mbr (embedded partition table)".to_string(),
                    mtd_id: None,
                    partitions: parts
                        .iter()
                        .map(|p| mtdparts::MtdPartition {
                            name: format!("p{}", p.index),
                            offset: p.offset,
                            size: Some(p.size),
                            read_only: false,
                        })
                        .collect(),
                    device_bytes: span.max(total),
                });
            }
        }
    }

    let mut images: Vec<ImageReport> = Vec::new();
    let mut kernel_version: Option<String> = None;

    // The input file itself.
    if whole.format == "raw" && layout.is_some() {
        images.push(ImageReport {
            name: file_name.to_string(),
            bytes: total,
            format: "flash-image".to_string(),
            partition: None,
            detail: json!({
                "content_end": whole_pad.content_end,
                "trailing_padding": whole_pad.trailing_bytes,
            }),
        });
    } else {
        // A lone squashfs/jffs2/uImage/env file dropped straight in.
        images.push(ImageReport {
            name: whole.name.clone(),
            bytes: whole.bytes,
            format: whole.format.clone(),
            partition: None,
            detail: whole.detail.clone(),
        });
    }

    let flash = layout.map(|layout| {
        let device = layout.device_bytes;
        if device > total {
            warnings.push(format!(
                "artifact is {} bytes but its layout describes a {}-byte device: the image is truncated (trailing partitions are only partly present)",
                total, device
            ));
        }
        let n = layout.partitions.len();
        // Containment/overlap marking, same semantics as tree analysis.
        let mut overlaps = vec![false; n];
        for a in 0..n {
            for b in 0..n {
                if a == b {
                    continue;
                }
                let (pa, pb) = (&layout.partitions[a], &layout.partitions[b]);
                let (Some(sa), Some(sb)) = (pa.size, pb.size) else {
                    continue;
                };
                let (a0, a1) = (pa.offset, pa.offset + sa);
                let (b0, b1) = (pb.offset, pb.offset + sb);
                if a0 <= b0 && a1 >= b1 && sa > sb {
                    overlaps[a] = true;
                } else if a0 < b1 && b0 < a1 && !(b0 <= a0 && b1 >= a1) && !(a0 <= b0 && a1 >= b1) {
                    overlaps[a] = true;
                }
            }
        }

        let mut partition_reports = Vec::with_capacity(n);
        for (pi, p) in layout.partitions.iter().enumerate() {
            let size = p.size.unwrap_or(0);
            if overlaps[pi] {
                // The spanning entry is the file itself.
                let is_whole = p.offset == 0 && p.size == Some(device);
                partition_reports.push(PartitionReport {
                    name: p.name.clone(),
                    offset: p.offset,
                    size: p.size,
                    read_only: p.read_only,
                    image: is_whole.then(|| file_name.to_string()),
                    content_bytes: is_whole.then_some(total),
                    used_bytes: is_whole.then_some(whole_pad.content_end),
                    overlaps: true,
                    verified: None,
                });
                continue;
            }
            let off = p.offset as usize;
            if off >= data.len() || size == 0 {
                warnings.push(format!(
                    "partition '{}' lies outside the image (offset 0x{:X})",
                    p.name, p.offset
                ));
                partition_reports.push(PartitionReport {
                    name: p.name.clone(),
                    offset: p.offset,
                    size: p.size,
                    read_only: p.read_only,
                    image: None,
                    content_bytes: None,
                    used_bytes: None,
                    overlaps: false,
                    verified: Some(false),
                });
                continue;
            }
            let end = (off + size as usize).min(data.len());
            let truncated = off + size as usize > data.len();
            if truncated {
                warnings.push(format!(
                    "partition '{}' extends past the end of the artifact: only {} of {} bytes are present",
                    p.name,
                    end - off,
                    size
                ));
            }
            let slice = &data[off..end];
            let slice_pad = padding::analyze(slice);
            // Name carved regions by partition alone so reports of different
              // layouts still diff cleanly; the offset lives in the detail.
              let carved_name = p.name.clone();

            // The environment block is sized by CONFIG_ENV_SIZE, which is
            // routinely smaller than the partition holding it (the tail is
            // spare or a redundant copy). Its CRC only covers the block, so
            // use the block found by the scan instead of the whole slice.
            if let Some((eoff, esize, env)) = env_hit
                .as_ref()
                .filter(|(eoff, esize, _)| *eoff >= off && eoff + esize <= end)
            {
                let detail = json!({
                    "crc_ok": env.crc_ok,
                    "redundant": env.redundant,
                    "used_bytes": env.used_bytes,
                    "free_bytes": (*esize as u64).saturating_sub(env.used_bytes),
                    "var_count": env.vars.len(),
                    "env_block_bytes": esize,
                    "env_block_offset": eoff,
                    "region_bytes": slice.len(),
                    "content_end": slice_pad.content_end,
                    "offset": p.offset,
                });
                if *esize < slice.len() {
                    warnings.push(format!(
                        "partition '{}' is {} bytes but holds a {}-byte environment block",
                        p.name,
                        slice.len(),
                        esize
                    ));
                }
                images.push(ImageReport {
                    name: carved_name.clone(),
                    bytes: slice.len() as u64,
                    format: "uboot-env".to_string(),
                    partition: Some(p.name.clone()),
                    detail,
                });
                partition_reports.push(PartitionReport {
                    name: p.name.clone(),
                    offset: p.offset,
                    size: p.size,
                    read_only: p.read_only,
                    image: Some(carved_name),
                    content_bytes: Some(slice_pad.content_end),
                    used_bytes: Some(env.used_bytes),
                    overlaps: false,
                    verified: Some(env.crc_ok),
                });
                continue;
            }

            let c = classify(&carved_name, slice.len() as u64, Some(slice));

            if kernel_version.is_none() {
                if let Some(u) = &c.uimage {
                    if let Some(v) = u.name.strip_prefix("Linux-") {
                        kernel_version =
                            Some(v.split_whitespace().next().unwrap_or(v).to_string());
                    }
                }
            }

            let role = partition_role(&p.name);
            let verified = match role {
                Role::Kernel => Some(c.uimage.is_some()),
                Role::Rootfs => Some(c.squash.is_some()),
                Role::Data => Some(c.jffs2.is_some() || slice_pad.content_end == 0),
                Role::Env => Some(c.env.as_ref().map(|e| e.crc_ok).unwrap_or(false)),
                Role::Boot => Some(slice_pad.content_end > 0),
                Role::Span | Role::Other => None,
            };
            if verified == Some(false) {
                warnings.push(format!(
                    "partition '{}' does not contain what its name implies (found: {})",
                    p.name, c.format
                ));
            }

            let used = used_bytes_of(&c).or(Some(slice_pad.content_end));
            // The entry represents the carved region, so its size is the
            // region size; what is actually occupied lives in the detail.
            let mut detail = c.detail.clone();
            if let Some(obj) = detail.as_object_mut() {
                obj.insert("region_bytes".into(), json!(slice.len()));
                obj.insert("offset".into(), json!(p.offset));
                if truncated {
                    obj.insert("truncated".into(), json!(true));
                }
                obj.entry("content_end")
                    .or_insert(json!(slice_pad.content_end));
            }
            images.push(ImageReport {
                name: carved_name.clone(),
                bytes: slice.len() as u64,
                format: c.format.clone(),
                partition: Some(p.name.clone()),
                detail,
            });
            partition_reports.push(PartitionReport {
                name: p.name.clone(),
                offset: p.offset,
                size: p.size,
                read_only: p.read_only,
                image: Some(carved_name),
                content_bytes: Some(slice_pad.content_end),
                used_bytes: used,
                overlaps: false,
                verified,
            });
        }

        FlashInfo {
            source: layout.source,
            mtd_id: layout.mtd_id,
            total_bytes: Some(device),
            partitions: partition_reports,
        }
    });

    if flash.is_none() {
        warnings.push(
            "no embedded partition layout found (no CRC-valid environment with mtdparts, no partition table)"
                .to_string(),
        );
    }

    let stem = file_name
        .strip_suffix(".bin")
        .or_else(|| file_name.strip_suffix(".img"))
        .unwrap_or(file_name);

    Report {
        schema: SCHEMA,
        generator: Generator {
            name: crate::GENERATOR_NAME.to_string(),
            version: crate::GENERATOR_VERSION.to_string(),
        },
        scan: ScanInfo {
            context_source: ContextSource::Artifact.as_str().to_string(),
            scan_mode: scan_mode.as_str().to_string(),
            root: root.to_string(),
            warnings,
        },
        build: BuildInfo {
            name: stem.to_string(),
            kernel_version,
            ..Default::default()
        },
        flash,
        images,
        rootfs: None,
        packages: Vec::new(),
        modules: Vec::new(),
        modules_meta: None,
        timings: Vec::new(),
        removed_not_shipped: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crc::{crc32_ieee, crc32_jffs2};
    use crate::parsers::{squashfs, uimage};

    const K: usize = 1024;

    fn synth_env(pairs: &[(&str, &str)], size: usize) -> Vec<u8> {
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

    fn synth_composite() -> Vec<u8> {
        // 64k boot, 64k env, 256k kernel, 512k rootfs, 128k data = 1 MiB
        let mut flash = vec![0xFFu8; 1024 * K];

        let boot = vec![0xAAu8; 10_000];
        flash[..boot.len()].copy_from_slice(&boot);

        let env = synth_env(
            &[
                ("baudrate", "115200"),
                (
                    "mtdparts",
                    "nor0:64k(boot),64k(env),256k(kernel),512k(rootfs),128k(data)",
                ),
            ],
            64 * K,
        );
        flash[64 * K..64 * K + env.len()].copy_from_slice(&env);

        // uImage named Linux-3.10.14
        let payload = vec![0xABu8; 50_000];
        let mut h = vec![0u8; 64];
        h[0..4].copy_from_slice(&uimage::MAGIC.to_be_bytes());
        h[12..16].copy_from_slice(&(payload.len() as u32).to_be_bytes());
        h[24..28].copy_from_slice(&crc32_ieee(&payload).to_be_bytes());
        h[28] = 5;
        h[30] = 2;
        h[31] = 3;
        h[32..45].copy_from_slice(b"Linux-3.10.14");
        let hcrc = crc32_ieee(&h);
        h[4..8].copy_from_slice(&hcrc.to_be_bytes());
        h.extend_from_slice(&payload);
        flash[128 * K..128 * K + h.len()].copy_from_slice(&h);

        // squashfs superblock
        let mut sq = vec![0u8; 300_000];
        sq[0..4].copy_from_slice(&squashfs::MAGIC.to_le_bytes());
        sq[4..8].copy_from_slice(&10u32.to_le_bytes());
        sq[12..16].copy_from_slice(&131_072u32.to_le_bytes());
        sq[20..22].copy_from_slice(&4u16.to_le_bytes());
        sq[22..24].copy_from_slice(&17u16.to_le_bytes());
        sq[28..30].copy_from_slice(&4u16.to_le_bytes());
        sq[40..48].copy_from_slice(&250_000u64.to_le_bytes());
        flash[384 * K..384 * K + sq.len()].copy_from_slice(&sq);

        // jffs2 cleanmarker at the data partition
        let mut node = Vec::new();
        node.extend_from_slice(&0x1985u16.to_le_bytes());
        node.extend_from_slice(&0x2003u16.to_le_bytes());
        node.extend_from_slice(&12u32.to_le_bytes());
        let crc = crc32_jffs2(&node[0..8]);
        node.extend_from_slice(&crc.to_le_bytes());
        flash[896 * K..896 * K + node.len()].copy_from_slice(&node);

        flash
    }

    #[test]
    fn carves_composite() {
        let data = synth_composite();
        let r = carve_flash_image("camera.bin", &data, "/tmp", ScanMode::Native);
        assert_eq!(r.scan.context_source, "artifact");
        let flash = r.flash.as_ref().unwrap();
        assert!(flash.source.contains("embedded env @ 0x10000"));
        assert_eq!(flash.total_bytes, Some(1024 * K as u64));
        assert_eq!(flash.partitions.len(), 5);
        for p in &flash.partitions {
            assert_eq!(p.verified, Some(true), "partition {}", p.name);
        }
        let part = |n: &str| flash.partitions.iter().find(|p| p.name == n).unwrap();
        assert_eq!(part("rootfs").used_bytes, Some(250_000));
        assert_eq!(part("kernel").used_bytes, Some(50_064));
        assert_eq!(part("data").used_bytes, Some(12));
        assert_eq!(part("boot").used_bytes, Some(10_000));
        assert_eq!(r.build.kernel_version.as_deref(), Some("3.10.14"));
        assert_eq!(r.build.name, "camera");
        // Carved regions became introspectable image entries.
        assert!(r.images.iter().any(|i| i.format == "squashfs"));
        assert!(r.images.iter().any(|i| i.format == "jffs2"));
        assert!(r.packages.is_empty());
    }

    /// Real thingino images size the env block by CONFIG_ENV_SIZE (32 KiB)
    /// inside a larger env partition (64 KiB). The CRC covers only the
    /// block, so verification must use the block, not the partition.
    #[test]
    fn env_block_smaller_than_partition() {
        let mut flash = vec![0xFFu8; 1024 * K];
        let env = synth_env(
            &[
                ("bootcmd", "sf probe"),
                (
                    "mtdparts",
                    "nor0:64k(boot),64k(env),256k(kernel),512k(rootfs),128k(data)",
                ),
            ],
            32 * K, // block is half the 64 KiB partition
        );
        flash[64 * K..64 * K + env.len()].copy_from_slice(&env);
        let r = carve_flash_image("cam.bin", &flash, "/tmp", ScanMode::Native);
        let flash_info = r.flash.as_ref().unwrap();
        let envp = flash_info.partitions.iter().find(|p| p.name == "env").unwrap();
        assert_eq!(envp.verified, Some(true), "env must verify from its block");
        assert_eq!(envp.size, Some(64 * K as u64));
        let envimg = r.images.iter().find(|i| i.partition.as_deref() == Some("env")).unwrap();
        assert_eq!(envimg.detail["env_block_bytes"], json!(32 * K));
        assert_eq!(envimg.detail["crc_ok"], json!(true));
        assert!(r
            .scan
            .warnings
            .iter()
            .any(|w| w.contains("32768-byte environment block")));
    }

    /// A short or partly-downloaded image must be reported as truncated
    /// against the layout it declares, not silently measured as if the
    /// missing bytes were never meant to exist.
    #[test]
    fn truncated_artifact_is_flagged() {
        let full = synth_composite();
        let short = &full[..600 * K]; // cut inside the rootfs partition
        let r = carve_flash_image("cut.bin", short, "/tmp", ScanMode::Native);
        let flash = r.flash.as_ref().unwrap();
        // The device size comes from the layout, not the file.
        assert_eq!(flash.total_bytes, Some(1024 * K as u64));
        assert!(r
            .scan
            .warnings
            .iter()
            .any(|w| w.contains("truncated")), "{:?}", r.scan.warnings);
        // rootfs straddles the cut; data begins beyond it entirely.
        assert!(r
            .scan
            .warnings
            .iter()
            .any(|w| w.contains("'rootfs' extends past the end")), "{:?}", r.scan.warnings);
        assert!(r
            .scan
            .warnings
            .iter()
            .any(|w| w.contains("'data' lies outside the image")));
        let rootfs = r
            .images
            .iter()
            .find(|i| i.partition.as_deref() == Some("rootfs"))
            .unwrap();
        assert_eq!(rootfs.detail["truncated"], json!(true));
        // A complete image says nothing about truncation.
        let ok = carve_flash_image("full.bin", &full, "/tmp", ScanMode::Native);
        assert!(!ok.scan.warnings.iter().any(|w| w.contains("truncated")));
    }

    #[test]
    fn no_layout_reports_gracefully() {
        let data = vec![0x5Au8; 512 * K];
        let r = carve_flash_image("mystery.bin", &data, "/tmp", ScanMode::Native);
        assert!(r.flash.is_none());
        assert!(r
            .scan
            .warnings
            .iter()
            .any(|w| w.contains("no embedded partition layout")));
    }

    #[test]
    fn lone_filesystem_image() {
        let mut sq = vec![0u8; 100_000];
        sq[0..4].copy_from_slice(&squashfs::MAGIC.to_le_bytes());
        sq[12..16].copy_from_slice(&131_072u32.to_le_bytes());
        sq[20..22].copy_from_slice(&4u16.to_le_bytes());
        sq[22..24].copy_from_slice(&17u16.to_le_bytes());
        sq[28..30].copy_from_slice(&4u16.to_le_bytes());
        sq[40..48].copy_from_slice(&90_000u64.to_le_bytes());
        let r = carve_flash_image("rootfs.squashfs", &sq, "/tmp", ScanMode::Native);
        assert_eq!(r.images[0].format, "squashfs");
        assert!(r.flash.is_none());
    }
}
