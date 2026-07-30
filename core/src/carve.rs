//! Artifact-only analysis: a bare composite flash image with no build tree.
//!
//! The layout is recovered from the image itself: a CRC-valid U-Boot
//! environment block found by scanning (its `mtdparts` spec is the partition
//! table), a GUID or MBR partition table for card and disk images, or UBI's
//! own volume table on NAND. Each
//! partition is then carved and classified with the same format parsers used
//! everywhere else. Package attribution is impossible in this mode and is
//! reported as such.

use crate::analyze::{classify, env_vars_json, partition_role, ubi_detail, used_bytes_of, Role};
use crate::parsers::{gpt, mbr, mtdparts, padding, ubi, ubootenv};
use crate::report::*;
use crate::snapshot::{ContextSource, ScanMode};
use serde_json::{json, Map, Value};

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

/// Find an `mtdparts` spec in the environment, and say where it came from.
///
/// A NOR board sets `mtdparts` as its own variable. A NAND board has no reason
/// to: its layout is two entries and it only needs them on the kernel command
/// line, so the spec is buried in the `bootcmd` that builds `bootargs`. Both
/// are the same syntax and equally authoritative.
fn mtdparts_spec(env: &ubootenv::UbootEnvInfo) -> Option<(String, String)> {
    if let Some((k, v)) = env.vars.iter().find(|(k, _)| k == "mtdparts") {
        return Some((v.clone(), k.clone()));
    }
    for (k, v) in &env.vars {
        let Some(rest) = v.split("mtdparts=").nth(1) else {
            continue;
        };
        // The spec ends at the next command-line argument.
        let spec = rest.split_whitespace().next().unwrap_or("");
        // A bare assignment with no partition list is not a layout.
        if spec.contains('(') && spec.contains(':') {
            return Some((spec.to_string(), k.clone()));
        }
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

/// A layout for an image that is, or ends with, a UBI area: the raw region
/// ahead of it (where a bootloader lives) and the area itself, which the
/// expansion step then replaces with its volumes.
fn ubi_layout(start: u64, total: u64) -> CarvedLayout {
    let mut partitions = Vec::new();
    if start > 0 {
        partitions.push(mtdparts::MtdPartition {
            name: "boot".to_string(),
            offset: 0,
            size: Some(start),
            read_only: false,
        });
    }
    partitions.push(mtdparts::MtdPartition {
        name: "ubi".to_string(),
        offset: start,
        size: Some(total - start),
        read_only: false,
    });
    CarvedLayout {
        source: format!("ubi volume table (@ 0x{start:X})"),
        mtd_id: None,
        partitions,
        device_bytes: total,
    }
}

/// One carved region. Usually a partition slice of the artifact, but a UBI
/// volume's payload is assembled from eraseblocks that need not be adjacent,
/// so it carries its own bytes.
struct Region {
    name: String,
    offset: u64,
    size: Option<u64>,
    read_only: bool,
    /// Present when the payload is not a contiguous slice of the artifact.
    content: Option<Vec<u8>>,
    /// Facts about the region that the format parsers cannot see.
    extra: Map<String, Value>,
}

impl Region {
    fn plain(p: &mtdparts::MtdPartition) -> Self {
        Region {
            name: p.name.clone(),
            offset: p.offset,
            size: p.size,
            read_only: p.read_only,
            content: None,
            extra: Map::new(),
        }
    }
}

/// Turn a UBI area into one region per written volume.
///
/// The volumes replace the container in the layout rather than nesting inside
/// it: they are what the flash actually holds, they carry the names a NOR
/// layout would have used, and a nested container would double-count every
/// byte. What is lost by dropping it -- geometry, spare blocks, volumes with
/// no content -- goes into the container's own image entry.
fn ubi_regions(info: &ubi::UbiInfo, base: u64, container: &str) -> Vec<Region> {
    let mut out = Vec::new();
    for v in &info.volumes {
        let Some(peb_offset) = v.peb_offset else {
            continue; // reserved but never written; the container reports it
        };
        let mut extra = Map::new();
        extra.insert("ubi_container".into(), json!(container));
        extra.insert("ubi_volume_id".into(), json!(v.id));
        extra.insert("ubi_volume_type".into(), json!(v.vol_type));
        extra.insert("reserved_pebs".into(), json!(v.reserved_pebs));
        extra.insert("mapped_pebs".into(), json!(v.mapped_pebs));
        extra.insert(
            "capacity_bytes".into(),
            json!(v.reserved_pebs as u64 * info.leb_size as u64),
        );
        extra.insert("payload_bytes".into(), json!(v.bytes));
        extra.insert("flash_bytes".into(), json!(v.flash_bytes));
        extra.insert("leb_size".into(), json!(info.leb_size));
        if v.autoresize {
            extra.insert("autoresize".into(), json!(true));
        }
        if !v.contiguous {
            extra.insert("fragmented".into(), json!(true));
        }
        if v.has_holes {
            extra.insert("missing_blocks".into(), json!(true));
        }
        out.push(Region {
            // An unnamed volume still needs a stable identity.
            name: if v.name.is_empty() {
                format!("{container}:vol{}", v.id)
            } else {
                v.name.clone()
            },
            offset: base + peb_offset,
            size: Some(v.peb_span),
            read_only: false,
            content: Some(v.content.clone()),
            extra,
        });
    }
    out
}

pub fn carve_flash_image(file_name: &str, data: &[u8], root: &str, scan_mode: ScanMode) -> Report {
    let mut warnings: Vec<String> =
        vec!["artifact-only scan: no build tree, package attribution unavailable".to_string()];
    let total = data.len() as u64;
    let whole = classify(file_name, total, Some(data));
    let whole_pad = padding::analyze(data);

    let env_hit = find_env(data);
    let mut layout: Option<CarvedLayout> = None;

    // A bare UBI container describes itself, and must say so before anything
    // else gets a chance to. Such a file usually holds the environment volume
    // of the device it is destined for, and that environment's mtdparts
    // describes the whole chip -- a boot region this file does not have,
    // followed by the area this file *is*. Believing it would slice the
    // container at the wrong offset and shred every volume in it.
    if whole.ubi.is_some() {
        layout = Some(ubi_layout(0, total));
    }

    // Otherwise the embedded environment's mtdparts spec, then a partition
    // table, then a UBI area further into the image.
    if layout.is_none() {
        if let Some((off, _size, env)) = &env_hit {
            if let Some((spec, from)) = mtdparts_spec(env) {
                if let Some(mut p) = mtdparts::parse(&spec) {
                    // The layout, not the file, defines the device size: a
                    // released image may be trimmed of trailing erased space.
                    let device = p.declared_end.max(total);
                    p.resolve_remainders(device);
                    let via = if from == "mtdparts" {
                        String::new()
                    } else {
                        format!(", from ${from}")
                    };
                    layout = Some(CarvedLayout {
                        source: format!("mtdparts (embedded env @ 0x{off:X}{via})"),
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
    }
    // A GUID table before an MBR: a GPT disk carries a protective MBR that
    // describes one partition covering the lot, which would hide the real one.
    if layout.is_none() {
        if let Some(g) = gpt::parse(data) {
            let span = g
                .partitions
                .iter()
                .map(|p| p.offset + p.size)
                .max()
                .unwrap_or(0);
            layout = Some(CarvedLayout {
                source: format!(
                    "gpt (embedded, {}-byte sectors{})",
                    g.sector_size,
                    if g.header_crc_ok { "" } else { ", header crc BAD" }
                ),
                mtd_id: None,
                partitions: g
                    .partitions
                    .iter()
                    .map(|p| mtdparts::MtdPartition {
                        name: if p.name.is_empty() {
                            format!("p{}", p.index)
                        } else {
                            p.name.clone()
                        },
                        offset: p.offset,
                        size: Some(p.size),
                        read_only: false,
                    })
                    .collect(),
                device_bytes: span.max(total),
            });
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
    if layout.is_none() {
        // NAND with no usable environment: UBI still describes itself, so the
        // only thing left to infer is the raw region ahead of it.
        if let Some(start) = ubi::find_start(data) {
            layout = Some(ubi_layout(start as u64, total));
        }
    }

    let mut images: Vec<ImageReport> = Vec::new();
    let mut kernel_version: Option<String> = None;

    // The input file itself.
    if (whole.format == "raw" || whole.format == "ubi") && layout.is_some() {
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

        // Expand any UBI container into the volumes it holds.
        let mut regions: Vec<Region> = Vec::new();
        for p in &layout.partitions {
            let off = p.offset as usize;
            let end = off
                .saturating_add(p.size.unwrap_or(0) as usize)
                .min(data.len());
            let found = data
                .get(off..end)
                .filter(|s| s.len() > 4)
                .and_then(|s| ubi::parse_at(s, 0));
            let Some(info) = found else {
                regions.push(Region::plain(p));
                continue;
            };
            let volumes = ubi_regions(&info, p.offset, &p.name);
            if info.bad_pebs > 0 {
                warnings.push(format!(
                    "UBI area '{}' has {} eraseblock(s) with neither a valid header nor erased flash",
                    p.name, info.bad_pebs
                ));
            }
            if !info.layout_found {
                warnings.push(format!(
                    "UBI area '{}' carries no volume table, so its volumes are unnamed",
                    p.name
                ));
            }
            for v in info.volumes.iter().filter(|v| v.has_holes) {
                let who = if v.name.is_empty() {
                    format!("id {}", v.id)
                } else {
                    format!("'{}'", v.name)
                };
                warnings.push(format!(
                    "UBI volume {who} is missing logical blocks, so its payload has holes"
                ));
            }
            // The container keeps an image entry of its own: it is the only
            // place the eraseblock geometry, the spare blocks and any volume
            // with no content are recorded.
            images.push(ImageReport {
                name: p.name.clone(),
                bytes: (end - off) as u64,
                format: "ubi".to_string(),
                partition: None,
                detail: ubi_detail(&info, p.offset),
            });
            regions.extend(volumes);
        }

        let n = regions.len();
        // Containment/overlap marking, same semantics as tree analysis.
        let mut overlaps = vec![false; n];
        for a in 0..n {
            for b in 0..n {
                if a == b {
                    continue;
                }
                let (pa, pb) = (&regions[a], &regions[b]);
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
        for (pi, p) in regions.iter().enumerate() {
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
            // An assembled payload is complete by construction; only a plain
            // slice can run off the end of the artifact.
            let truncated = p.content.is_none() && off + size as usize > data.len();
            if truncated {
                warnings.push(format!(
                    "partition '{}' extends past the end of the artifact: only {} of {} bytes are present",
                    p.name,
                    end - off,
                    size
                ));
            }
            let slice: &[u8] = match &p.content {
                Some(c) => c,
                None => &data[off..end],
            };
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
                let (vars, vars_truncated) = env_vars_json(env);
                let mut detail = json!({
                    "crc_ok": env.crc_ok,
                    "redundant": env.redundant,
                    "used_bytes": env.used_bytes,
                    "free_bytes": (*esize as u64).saturating_sub(env.used_bytes),
                    "var_count": env.vars.len(),
                    "vars": vars,
                    "vars_truncated": vars_truncated,
                    "env_block_bytes": esize,
                    "env_block_offset": eoff,
                    "region_bytes": slice.len(),
                    "content_end": slice_pad.content_end,
                    "offset": p.offset,
                });
                if let Some(obj) = detail.as_object_mut() {
                    obj.extend(p.extra.clone());
                }
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
                // Raw flash ships squashfs; a card image ships ext or cpio.
                Role::Rootfs => Some(c.squash.is_some() || c.ext.is_some() || c.cpio.is_some()),
                // NOR keeps the writable area in jffs2, NAND in a UBIFS
                // volume, a card image in an ext filesystem.
                Role::Data => Some(
                    c.jffs2.is_some()
                        || c.ubifs.is_some()
                        || c.ext.is_some()
                        || slice_pad.content_end == 0,
                ),
                Role::Env => Some(c.env.as_ref().map(|e| e.crc_ok).unwrap_or(false)),
                // Only reached when the area could not be expanded into
                // volumes, which is itself the finding.
                Role::Ubi => Some(c.ubi.is_some()),
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
                obj.extend(p.extra.clone());
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
            "no embedded partition layout found (no CRC-valid environment with mtdparts, no partition table, no UBI volume table)"
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
    use crate::crc::{crc32_ieee, crc32_jffs2, crc32_raw};
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

    fn synth_uimage(name: &str, payload_len: usize) -> Vec<u8> {
        let payload = vec![0xABu8; payload_len];
        let mut h = vec![0u8; 64];
        h[0..4].copy_from_slice(&uimage::MAGIC.to_be_bytes());
        h[12..16].copy_from_slice(&(payload.len() as u32).to_be_bytes());
        h[24..28].copy_from_slice(&crc32_ieee(&payload).to_be_bytes());
        h[28] = 5;
        h[30] = 2;
        h[31] = 3;
        h[32..32 + name.len()].copy_from_slice(name.as_bytes());
        let hcrc = crc32_ieee(&h);
        h[4..8].copy_from_slice(&hcrc.to_be_bytes());
        h.extend_from_slice(&payload);
        h
    }

    fn synth_squashfs(size: usize, used: u64) -> Vec<u8> {
        let mut sq = vec![0u8; size];
        sq[0..4].copy_from_slice(&squashfs::MAGIC.to_le_bytes());
        sq[4..8].copy_from_slice(&10u32.to_le_bytes());
        sq[12..16].copy_from_slice(&131_072u32.to_le_bytes());
        sq[20..22].copy_from_slice(&4u16.to_le_bytes());
        sq[22..24].copy_from_slice(&17u16.to_le_bytes());
        sq[28..30].copy_from_slice(&4u16.to_le_bytes());
        sq[40..48].copy_from_slice(&used.to_le_bytes());
        sq
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

        let h = synth_uimage("Linux-3.10.14", 50_000);
        flash[128 * K..128 * K + h.len()].copy_from_slice(&h);

        let sq = synth_squashfs(300_000, 250_000);
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

    // --- UBI/NAND synthesis, matching what ubinize emits -----------------

    const PEB: usize = 128 * K;
    const VID_OFF: usize = 2048;
    const DATA_OFF: usize = 4096;
    const LEB: usize = PEB - DATA_OFF;
    const LAYOUT_VOL: u32 = 0x7FFF_EFFF;

    fn ubi_crc(d: &[u8]) -> u32 {
        crc32_raw(0xFFFF_FFFF, d)
    }

    fn ec_hdr() -> Vec<u8> {
        let mut h = vec![0u8; 64];
        h[0..4].copy_from_slice(&0x5542_4923u32.to_be_bytes());
        h[4] = 1;
        h[16..20].copy_from_slice(&(VID_OFF as u32).to_be_bytes());
        h[20..24].copy_from_slice(&(DATA_OFF as u32).to_be_bytes());
        h[24..28].copy_from_slice(&0xC0FF_EE01u32.to_be_bytes());
        let c = ubi_crc(&h[..60]);
        h[60..64].copy_from_slice(&c.to_be_bytes());
        h
    }

    fn vid_hdr(vol_id: u32, lnum: u32, vol_type: u8, data_size: u32) -> Vec<u8> {
        let mut h = vec![0u8; 64];
        h[0..4].copy_from_slice(&0x5542_4921u32.to_be_bytes());
        h[4] = 1;
        h[5] = vol_type;
        h[8..12].copy_from_slice(&vol_id.to_be_bytes());
        h[12..16].copy_from_slice(&lnum.to_be_bytes());
        h[20..24].copy_from_slice(&data_size.to_be_bytes());
        let c = ubi_crc(&h[..60]);
        h[60..64].copy_from_slice(&c.to_be_bytes());
        h
    }

    fn vtbl_rec(reserved_pebs: u32, vol_type: u8, name: &str, flags: u8) -> Vec<u8> {
        let mut r = vec![0u8; 172];
        r[0..4].copy_from_slice(&reserved_pebs.to_be_bytes());
        r[4..8].copy_from_slice(&1u32.to_be_bytes());
        r[12] = vol_type;
        r[14..16].copy_from_slice(&(name.len() as u16).to_be_bytes());
        r[16..16 + name.len()].copy_from_slice(name.as_bytes());
        r[144] = flags;
        let c = ubi_crc(&r[..168]);
        r[168..].copy_from_slice(&c.to_be_bytes());
        r
    }

    fn push_peb(img: &mut Vec<u8>, vid: Option<Vec<u8>>, payload: &[u8]) {
        let base = img.len();
        img.resize(base + PEB, 0xFF);
        img[base..base + 64].copy_from_slice(&ec_hdr());
        if let Some(v) = vid {
            img[base + VID_OFF..base + VID_OFF + 64].copy_from_slice(&v);
            img[base + DATA_OFF..base + DATA_OFF + payload.len()].copy_from_slice(payload);
        }
    }

    /// Write a payload as consecutive logical blocks of one volume.
    fn push_volume(img: &mut Vec<u8>, vol_id: u32, vol_type: u8, payload: &[u8]) {
        let mut lnum = 0u32;
        for chunk in payload.chunks(LEB) {
            let declared = if vol_type == 2 { chunk.len() as u32 } else { 0 };
            push_peb(img, Some(vid_hdr(vol_id, lnum, vol_type, declared)), chunk);
            lnum += 1;
        }
    }

    /// A NAND firmware image the way the build assembles one: a 1 MiB raw boot
    /// region, then a UBI area with uboot-env / kernel / rootfs written and an
    /// autoresize overlay reserved but empty.
    fn synth_nand(env_size: usize) -> Vec<u8> {
        let mut img = vec![0xFFu8; K];
        let ub = vec![0x5Au8; 200_000]; // the u-boot binary at offset 0
        img.resize(1024 * K, 0xFF);
        img[..ub.len()].copy_from_slice(&ub);

        let mut table = Vec::new();
        table.extend_from_slice(&vtbl_rec(3, 1, "uboot-env", 0));
        table.extend_from_slice(&vtbl_rec(2, 2, "kernel", 0));
        table.extend_from_slice(&vtbl_rec(4, 2, "rootfs", 0));
        table.extend_from_slice(&vtbl_rec(5, 1, "overlay", 1));
        push_peb(&mut img, Some(vid_hdr(LAYOUT_VOL, 0, 1, 0)), &table);
        push_peb(&mut img, Some(vid_hdr(LAYOUT_VOL, 1, 1, 0)), &table);

        // vol0: the environment, carrying its mtdparts inside bootcmd
        let env = synth_env(
            &[
                ("baudrate", "115200"),
                (
                    "bootcmd",
                    "ubi part ubi;ubi read ${loadaddr} kernel;setenv bootargs console=ttyS1 \
                     mtdparts=sfc_nand:1024k(boot),-(ubi) ubi.mtd=ubi ubi.block=0,rootfs \
                     root=/dev/ubiblock0_2;bootm ${loadaddr}",
                ),
            ],
            env_size,
        );
        push_volume(&mut img, 0, 1, &env);
        push_volume(&mut img, 1, 2, &synth_uimage("Linux-4.4.94", 1_500_000));
        push_volume(&mut img, 2, 2, &synth_squashfs(400_000, 396_000));
        img
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
        let sq = synth_squashfs(100_000, 90_000);
        let r = carve_flash_image("rootfs.squashfs", &sq, "/tmp", ScanMode::Native);
        assert_eq!(r.images[0].format, "squashfs");
        assert!(r.flash.is_none());
    }

    /// A NAND image: the layout comes from the mtdparts buried in bootcmd, and
    /// the single "ubi" partition becomes the volumes it actually holds.
    #[test]
    fn carves_nand_ubi_image() {
        let data = synth_nand(64 * K);
        let r = carve_flash_image("thingino-cam.bin", &data, "/tmp", ScanMode::Native);
        let flash = r.flash.as_ref().unwrap();
        assert!(
            flash.source.contains("$bootcmd"),
            "layout source should credit the variable it came from: {}",
            flash.source
        );
        assert_eq!(flash.mtd_id.as_deref(), Some("sfc_nand"));

        // boot stays raw; the ubi partition is replaced by its volumes.
        let names: Vec<&str> = flash.partitions.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["boot", "uboot-env", "kernel", "rootfs"]);
        let part = |n: &str| flash.partitions.iter().find(|p| p.name == n).unwrap();
        for n in ["boot", "uboot-env", "kernel", "rootfs"] {
            assert_eq!(part(n).verified, Some(true), "partition {n}");
            assert!(!part(n).overlaps, "partition {n} must not overlap");
        }
        assert_eq!(part("boot").offset, 0);
        assert_eq!(part("boot").size, Some(1024 * K as u64));
        assert_eq!(part("boot").used_bytes, Some(200_000));

        // Volumes sit where their eraseblocks are, after the volume table.
        assert_eq!(part("uboot-env").offset, 1024 * K as u64 + 2 * PEB as u64);
        assert_eq!(part("kernel").used_bytes, Some(1_500_064));
        assert_eq!(part("rootfs").used_bytes, Some(396_000));
        assert_eq!(r.build.kernel_version.as_deref(), Some("4.4.94"));

        // The container keeps the geometry and the volume the image left empty.
        let ubi = r.images.iter().find(|i| i.format == "ubi").unwrap();
        assert_eq!(ubi.detail["peb_size"], json!(PEB));
        assert_eq!(ubi.detail["leb_size"], json!(LEB));
        assert_eq!(ubi.detail["volume_table_found"], json!(true));
        assert_eq!(ubi.detail["unmapped_volumes"], json!(["overlay"]));
        let vols = ubi.detail["volumes"].as_array().unwrap();
        assert_eq!(vols.len(), 4);
        let overlay = vols.iter().find(|v| v["name"] == "overlay").unwrap();
        assert_eq!(overlay["autoresize"], json!(true));
        assert_eq!(overlay["mapped_pebs"], json!(0));
        assert_eq!(overlay["capacity_bytes"], json!(5 * LEB));
        assert_eq!(overlay["offset"], Value::Null);

        // A volume region records what it cost on flash beyond its payload.
        let rootfs = r
            .images
            .iter()
            .find(|i| i.partition.as_deref() == Some("rootfs"))
            .unwrap();
        assert_eq!(rootfs.format, "squashfs");
        assert_eq!(rootfs.detail["ubi_volume_id"], json!(2));
        assert_eq!(rootfs.detail["ubi_volume_type"], json!("static"));
        assert_eq!(rootfs.detail["payload_bytes"], json!(400_000));
        assert_eq!(rootfs.detail["flash_bytes"], json!(4 * PEB));
        assert_eq!(rootfs.detail["capacity_bytes"], json!(4 * LEB));

        assert!(
            !r.scan.warnings.iter().any(|w| w.contains("does not contain")),
            "{:?}",
            r.scan.warnings
        );
    }

    /// With no environment at all, UBI still describes the whole layout.
    #[test]
    fn ubi_layout_without_an_environment() {
        let mut img = vec![0xFFu8; 1024 * K];
        let mut table = Vec::new();
        table.extend_from_slice(&vtbl_rec(4, 2, "rootfs", 0));
        push_peb(&mut img, Some(vid_hdr(LAYOUT_VOL, 0, 1, 0)), &table);
        push_peb(&mut img, Some(vid_hdr(LAYOUT_VOL, 1, 1, 0)), &table);
        push_volume(&mut img, 0, 2, &synth_squashfs(300_000, 290_000));

        let r = carve_flash_image("nand.bin", &img, "/tmp", ScanMode::Native);
        let flash = r.flash.as_ref().unwrap();
        assert!(flash.source.starts_with("ubi volume table"), "{}", flash.source);
        let names: Vec<&str> = flash.partitions.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["boot", "rootfs"]);
        let rootfs = flash.partitions.iter().find(|p| p.name == "rootfs").unwrap();
        assert_eq!(rootfs.used_bytes, Some(290_000));
        assert_eq!(rootfs.verified, Some(true));
    }

    /// A bare ubinize output, no boot region and no environment.
    #[test]
    fn lone_ubi_image() {
        let mut img = Vec::new();
        let table = vtbl_rec(4, 2, "rootfs", 0);
        push_peb(&mut img, Some(vid_hdr(LAYOUT_VOL, 0, 1, 0)), &table);
        push_peb(&mut img, Some(vid_hdr(LAYOUT_VOL, 1, 1, 0)), &table);
        push_volume(&mut img, 0, 2, &synth_squashfs(300_000, 290_000));

        let r = carve_flash_image("rootfs.ubi", &img, "/tmp", ScanMode::Native);
        let flash = r.flash.as_ref().unwrap();
        // No raw region ahead of the volume table, so no boot partition.
        let names: Vec<&str> = flash.partitions.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["rootfs"]);
        // The file itself is recognised as UBI rather than raw bytes.
        let whole = r.images.iter().find(|i| i.name == "rootfs.ubi");
        assert!(whole.is_some() || r.images.iter().any(|i| i.format == "ubi"));
    }

    /// The UBI image built for a NAND camera carries that camera's environment
    /// in a volume, and that environment's mtdparts describes the whole chip:
    /// a 1 MiB boot region and then the area this file *is*. Applying it here
    /// would cut the container at 1 MiB and shred every volume, so a
    /// self-describing container must win over an mtdparts spec.
    #[test]
    fn whole_chip_mtdparts_does_not_slice_a_bare_ubi_container() {
        let full = synth_nand(64 * K);
        let bare = &full[1024 * K..]; // just the ubinize output
        let r = carve_flash_image("rootfs.ubi", bare, "/tmp", ScanMode::Native);
        let flash = r.flash.as_ref().unwrap();
        assert!(
            flash.source.starts_with("ubi volume table"),
            "the container must describe itself, not defer to mtdparts: {}",
            flash.source
        );
        let names: Vec<&str> = flash.partitions.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["uboot-env", "kernel", "rootfs"]);
        let part = |n: &str| flash.partitions.iter().find(|p| p.name == n).unwrap();
        assert_eq!(part("kernel").used_bytes, Some(1_500_064));
        assert_eq!(part("rootfs").used_bytes, Some(396_000));
        for n in ["uboot-env", "kernel", "rootfs"] {
            assert_eq!(part(n).verified, Some(true), "partition {n}");
        }
        // Exactly one UBI area, covering the whole file.
        let containers: Vec<&ImageReport> =
            r.images.iter().filter(|i| i.format == "ubi").collect();
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].bytes, bare.len() as u64);
        assert!(
            !r.scan.warnings.iter().any(|w| w.contains("no volume table")),
            "{:?}",
            r.scan.warnings
        );
    }
}
