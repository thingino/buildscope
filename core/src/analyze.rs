//! Snapshot in, Report out. Pure: no IO, no clocks, no environment.

use crate::inputs::{buildtime, config::BrConfig, pfl};
use crate::parsers::{
    genimage, jffs2, mbr, mtdparts, padding, squashfs, ubi, ubifs, ubootenv, uimage,
};
use crate::report::*;
use crate::snapshot::Snapshot;
use serde_json::json;
use std::collections::{HashMap, HashSet};


pub(crate) struct Classified {
    pub(crate) name: String,
    pub(crate) bytes: u64,
    pub(crate) format: String,
    pub(crate) detail: serde_json::Value,
    pub(crate) squash: Option<squashfs::SquashfsInfo>,
    pub(crate) jffs2: Option<jffs2::Jffs2Info>,
    pub(crate) uimage: Option<uimage::UimageInfo>,
    pub(crate) env: Option<ubootenv::UbootEnvInfo>,
    pub(crate) mbr: Option<Vec<mbr::MbrPartition>>,
    pub(crate) ubi: Option<ubi::UbiInfo>,
    pub(crate) ubifs: Option<ubifs::UbifsInfo>,
    pub(crate) content_end: u64,
    pub(crate) partition: Option<String>,
}

fn is_texty(name: &str, bytes: Option<&[u8]>) -> bool {
    let suffix_hit = [".txt", ".md", ".json", ".sha256sum", ".sha256", ".cfg"]
        .iter()
        .any(|s| name.to_ascii_lowercase().ends_with(s));
    match bytes {
        Some(b) => {
            let probe = &b[..b.len().min(4096)];
            if probe.is_empty() {
                return suffix_hit;
            }
            let printable = probe
                .iter()
                .filter(|&&c| c == b'\n' || c == b'\r' || c == b'\t' || (0x20..0x7F).contains(&c))
                .count();
            printable * 100 / probe.len() >= 95
        }
        None => suffix_hit,
    }
}

pub(crate) fn classify(name: &str, size: u64, bytes: Option<&[u8]>) -> Classified {
    let mut c = Classified {
        name: name.to_string(),
        bytes: size,
        format: "raw".to_string(),
        detail: json!({}),
        squash: None,
        jffs2: None,
        uimage: None,
        env: None,
        mbr: None,
        ubi: None,
        ubifs: None,
        content_end: size,
        partition: None,
    };
    let Some(data) = bytes else {
        c.format = if is_texty(name, None) { "text" } else { "raw" }.into();
        return c;
    };

    if let Some(info) = squashfs::parse(data) {
        c.format = "squashfs".into();
        c.detail = json!({
            "bytes_used": info.bytes_used,
            "padding_bytes": size.saturating_sub(info.bytes_used),
            "compression": info.compression,
            "block_size": info.block_size,
            "inode_count": info.inode_count,
            "fragment_count": info.fragment_count,
            "version": format!("{}.{}", info.version_major, info.version_minor),
        });
        c.squash = Some(info);
        return c;
    }
    if let Some(info) = jffs2::parse(data) {
        c.format = "jffs2".into();
        c.detail = json!({
            "used_bytes": info.used_bytes,
            "free_bytes": info.free_bytes,
            "dirty_bytes": info.dirty_bytes,
            "node_count": info.node_count,
            "clean_markers": info.clean_markers,
            "live_files": info.live_files,
            "live_dirs": info.live_dirs,
            "live_other": info.live_other,
            "logical_content_bytes": info.logical_content_bytes,
            "crc_errors": info.crc_errors,
            "endianness": info.endianness,
            "entries_truncated": info.entries_truncated,
            "entries": info.entries.iter().map(|e| json!({
                "path": e.path,
                "bytes": e.bytes,
                "kind": e.kind,
            })).collect::<Vec<_>>(),
        });
        c.jffs2 = Some(info);
        return c;
    }
    // A UBI area is only claimed when it begins at the start of the input: a
    // composite flash image has one further in, and finding it there is the
    // layout detector's job, not this one's.
    if let Some(info) = ubi::parse_at(data, 0) {
        c.format = "ubi".into();
        c.detail = ubi_detail(&info, 0);
        c.content_end = info.used_bytes();
        c.ubi = Some(info);
        return c;
    }
    if let Some(info) = ubifs::parse(data) {
        c.format = "ubifs".into();
        c.detail = json!({
            "leb_size": info.leb_size,
            "leb_count": info.leb_count,
            "max_leb_count": info.max_leb_count,
            "min_io_size": info.min_io_size,
            "format_version": info.format_version,
            "compression": info.default_compression,
            "uuid": info.uuid,
            "total_bytes": info.total_bytes,
            "max_bytes": info.max_bytes,
            "autoresize_pending": info.max_leb_count > info.leb_count,
            "crc_ok": info.crc_ok,
        });
        // How much of a UBIFS volume is live needs the wandering tree and a
        // decompressor. The written extent is the honest upper bound.
        c.content_end = padding::analyze(data).content_end;
        c.ubifs = Some(info);
        return c;
    }
    if let Some(info) = uimage::parse(data) {
        c.format = "uimage".into();
        c.detail = json!({
            "name": info.name,
            "type": info.type_name,
            "compression": info.compression_name,
            "declared_size": info.declared_size,
            "total_with_header": info.declared_size as u64 + uimage::HEADER_LEN as u64,
            "padding_bytes": size.saturating_sub(info.declared_size as u64 + uimage::HEADER_LEN as u64),
            "load_addr": format!("0x{:08x}", info.load_addr),
            "entry_point": format!("0x{:08x}", info.entry_point),
            "header_crc_ok": info.header_crc_ok,
            "timestamp": info.timestamp,
        });
        c.uimage = Some(info);
        return c;
    }
    if let Some(parts) = mbr::parse(data) {
        // Only believe an MBR in a file big enough to hold its partitions.
        let span = parts.iter().map(|p| p.offset + p.size).max().unwrap_or(0);
        if span <= size && size >= 1024 * 1024 {
            c.format = "disk-image".into();
            c.detail = json!({
                "partitions": parts.iter().map(|p| json!({
                    "index": p.index,
                    "type": format!("0x{:02x}", p.part_type),
                    "bootable": p.bootable,
                    "offset": p.offset,
                    "size": p.size,
                })).collect::<Vec<_>>(),
            });
            c.mbr = Some(parts);
            return c;
        }
    }
    if let Some(info) = ubootenv::parse(data) {
        c.format = "uboot-env".into();
        let (vars, vars_truncated) = env_vars_json(&info);
        c.detail = json!({
            "crc_ok": info.crc_ok,
            "redundant": info.redundant,
            "used_bytes": info.used_bytes,
            "free_bytes": info.total_bytes.saturating_sub(info.used_bytes),
            "var_count": info.vars.len(),
            "vars": vars,
            "vars_truncated": vars_truncated,
        });
        c.env = Some(info);
        return c;
    }
    if is_texty(name, Some(data)) {
        c.format = "text".into();
        return c;
    }
    let pad = padding::analyze(data);
    c.content_end = pad.content_end;
    c.detail = json!({
        "content_end": pad.content_end,
        "trailing_padding": pad.trailing_bytes,
    });
    c
}

/// A U-Boot environment is small, so the whole of it is reported; the cap only
/// exists so a corrupt image cannot produce an unbounded report.
pub(crate) const MAX_ENV_VARS: usize = 1024;

/// The environment's variables, sorted by key so a report diffs cleanly and
/// reads the way `fw_printenv` prints it.
pub(crate) fn env_vars_json(info: &ubootenv::UbootEnvInfo) -> (serde_json::Value, bool) {
    let mut vars: Vec<&(String, String)> = info.vars.iter().collect();
    vars.sort_by(|a, b| a.0.cmp(&b.0));
    let truncated = vars.len() > MAX_ENV_VARS;
    let out: Vec<serde_json::Value> = vars
        .iter()
        .take(MAX_ENV_VARS)
        .map(|(k, v)| json!({ "key": k, "value": v, "bytes": v.len() }))
        .collect();
    (json!(out), truncated)
}

/// Describe a UBI area. `base` is where the area sits in the file being
/// reported, so volume offsets come out absolute even when the area was parsed
/// from a slice.
pub(crate) fn ubi_detail(info: &ubi::UbiInfo, base: u64) -> serde_json::Value {
    let unmapped: Vec<&str> = info
        .volumes
        .iter()
        .filter(|v| v.mapped_pebs == 0)
        .map(|v| v.name.as_str())
        .collect();
    json!({
        "ubi_offset": base + info.start_offset,
        "peb_size": info.peb_size,
        "leb_size": info.leb_size,
        "vid_hdr_offset": info.vid_hdr_offset,
        "data_offset": info.data_offset,
        "image_seq": format!("0x{:08x}", info.image_seq),
        "total_pebs": info.total_pebs,
        "mapped_pebs": info.mapped_pebs,
        "free_pebs": info.free_pebs,
        "erased_pebs": info.erased_pebs,
        "bad_pebs": info.bad_pebs,
        "used_bytes": info.used_bytes(),
        "overhead_bytes": info.mapped_pebs as u64 * info.data_offset as u64,
        "volume_table_found": info.layout_found,
        "unmapped_volumes": unmapped,
        "volumes": info.volumes.iter().map(|v| json!({
            "id": v.id,
            "name": v.name,
            "type": v.vol_type,
            "reserved_pebs": v.reserved_pebs,
            "mapped_pebs": v.mapped_pebs,
            "capacity_bytes": v.reserved_pebs as u64 * info.leb_size as u64,
            "bytes": v.bytes,
            "flash_bytes": v.flash_bytes,
            "offset": v.peb_offset.map(|o| base + o),
            "autoresize": v.autoresize,
            // Not applicable to a volume the image never wrote a block for.
            "contiguous": (v.mapped_pebs > 0).then_some(v.contiguous),
            "has_holes": (v.mapped_pebs > 0).then_some(v.has_holes),
        })).collect::<Vec<_>>(),
    })
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Role {
    Boot,
    Env,
    Kernel,
    Rootfs,
    Data,
    /// A UBI area: a container for volumes rather than a filesystem itself.
    Ubi,
    Span,
    Other,
}

pub(crate) fn partition_role(name: &str) -> Role {
    let n = name.to_ascii_lowercase();
    if n == "all" || n == "whole" || n == "flash" {
        return Role::Span;
    }
    if n.contains("env") {
        return Role::Env;
    }
    if n.contains("kern") || n == "linux" || n == "uimage" || n == "zimage" {
        return Role::Kernel;
    }
    if n.contains("root") || n.contains("squash") || n == "system" {
        return Role::Rootfs;
    }
    if n.contains("data") || n.contains("overlay") || n.contains("user") {
        return Role::Data;
    }
    // NAND names its one big partition after the layer that manages it.
    if n == "ubi" || n.starts_with("ubi") {
        return Role::Ubi;
    }
    if n.contains("boot") || n.contains("spl") || n.contains("loader") {
        return Role::Boot;
    }
    Role::Other
}

fn image_stem(name: &str) -> String {
    name.split('.').next().unwrap_or(name).to_ascii_lowercase()
}

fn image_fits_role(img: &Classified, role: Role) -> bool {
    match role {
        Role::Kernel => img.uimage.is_some(),
        Role::Rootfs => img.squash.is_some(),
        // NOR keeps its writable area in jffs2, NAND in a UBIFS volume.
        Role::Data => img.jffs2.is_some() || img.ubifs.is_some(),
        Role::Ubi => img.ubi.is_some(),
        Role::Env => img.env.is_some(),
        Role::Boot => {
            img.format == "raw" && {
                let n = img.name.to_ascii_lowercase();
                (n.contains("boot") || n.contains("spl")) && !n.contains("env")
            }
        }
        _ => false,
    }
}

pub(crate) fn used_bytes_of(img: &Classified) -> Option<u64> {
    if let Some(s) = &img.squash {
        return Some(s.bytes_used);
    }
    if let Some(j) = &img.jffs2 {
        return Some(j.used_bytes);
    }
    if let Some(u) = &img.uimage {
        return Some(u.declared_size as u64 + uimage::HEADER_LEN as u64);
    }
    if let Some(e) = &img.env {
        return Some(e.used_bytes);
    }
    if let Some(u) = &img.ubi {
        // What the mapped eraseblocks cost, not what the area spans.
        return Some(u.used_bytes());
    }
    if img.ubifs.is_some() {
        return Some(img.content_end);
    }
    if img.format == "raw" || img.format == "flash-image" {
        return Some(img.content_end);
    }
    None
}

struct Layout {
    source: String,
    mtd_id: Option<String>,
    partitions: Vec<mtdparts::MtdPartition>,
    declared_end: u64,
    /// partition name -> image filename, when the layout source names the
    /// content directly (genimage does).
    image_hints: Vec<(String, String)>,
}

fn detect_layout(snap: &Snapshot, images: &[Classified], warnings: &mut Vec<String>) -> Option<Layout> {
    for t in &snap.env_texts {
        if let Some(p) = mtdparts::find_in_text(&t.text) {
            return Some(Layout {
                source: format!("mtdparts ({})", t.name),
                mtd_id: Some(p.mtd_id.clone()),
                declared_end: p.declared_end,
                partitions: p.partitions,
                image_hints: Vec::new(),
            });
        }
    }
    for img in images {
        if let Some(env) = &img.env {
            if let Some((_, v)) = env.vars.iter().find(|(k, _)| k == "mtdparts") {
                if let Some(p) = mtdparts::parse(v) {
                    return Some(Layout {
                        source: format!("mtdparts ({})", img.name),
                        mtd_id: Some(p.mtd_id.clone()),
                        declared_end: p.declared_end,
                        partitions: p.partitions,
                        image_hints: Vec::new(),
                    });
                }
            }
        }
    }
    if let Some(cfg) = &snap.config {
        if let Some(p) = mtdparts::find_in_text(cfg) {
            return Some(Layout {
                source: "mtdparts (.config)".to_string(),
                mtd_id: Some(p.mtd_id.clone()),
                declared_end: p.declared_end,
                partitions: p.partitions,
                image_hints: Vec::new(),
            });
        }
    }
    for t in &snap.genimage_texts {
        let Some(images_cfg) = genimage::parse(&t.text) else {
            continue;
        };
        // Take the image with the most partitions (the composite).
        let Some(best) = images_cfg.iter().max_by_key(|im| im.partitions.len()) else {
            continue;
        };
        if best.partitions.is_empty() {
            continue;
        }
        let resolved = genimage::resolve_offsets(&best.partitions);
        let partitions: Vec<mtdparts::MtdPartition> = resolved
            .iter()
            .map(|(name, offset, size, _)| mtdparts::MtdPartition {
                name: name.clone(),
                offset: *offset,
                size: *size,
                read_only: false,
            })
            .collect();
        let image_hints: Vec<(String, String)> = resolved
            .iter()
            .filter_map(|(name, _, _, img)| img.clone().map(|i| (name.clone(), i)))
            .collect();
        let declared_end = resolved
            .iter()
            .filter_map(|(_, o, s, _)| s.map(|s| o + s))
            .max()
            .unwrap_or(0);
        return Some(Layout {
            source: format!("genimage ({})", t.name),
            mtd_id: None,
            partitions,
            declared_end,
            image_hints,
        });
    }
    for img in images {
        if let Some(parts) = &img.mbr {
            let partitions = parts
                .iter()
                .map(|p| mtdparts::MtdPartition {
                    name: format!("p{}", p.index),
                    offset: p.offset,
                    size: Some(p.size),
                    read_only: false,
                })
                .collect::<Vec<_>>();
            let declared_end = parts.iter().map(|p| p.offset + p.size).max().unwrap_or(0);
            return Some(Layout {
                source: format!("mbr ({})", img.name),
                mtd_id: None,
                partitions,
                declared_end,
                image_hints: Vec::new(),
            });
        }
    }
    warnings.push("no flash layout found (no mtdparts source, no partition table); partition budgets unavailable".into());
    None
}

/// Paths Buildroot's own target-finalize pass removes on every build:
/// development and documentation files that never ship. Filtering these
/// keeps removed_not_shipped down to deliberate, project-level removals.
fn is_default_finalize_removal(rel: &str) -> bool {
    const SUFFIXES: &[&str] = &[".a", ".la", ".h", ".hh", ".hpp", ".hxx", ".pc", ".cmake"];
    const PREFIXES: &[&str] = &[
        "usr/include/",
        "usr/lib/pkgconfig/",
        "usr/share/pkgconfig/",
        "usr/lib/cmake/",
        "usr/share/cmake/",
        "usr/share/man/",
        "usr/share/doc/",
        "usr/share/info/",
        "usr/share/aclocal/",
        "usr/share/gtk-doc/",
        "usr/share/locale/",
        "usr/share/zoneinfo/",
        "lib/pkgconfig/",
        "include/",
    ];
    SUFFIXES.iter().any(|s| rel.ends_with(s)) || PREFIXES.iter().any(|p| rel.starts_with(p))
}

pub fn analyze(snap: &Snapshot) -> Report {
    let mut warnings: Vec<String> = Vec::new();

    // Config
    let cfg = snap.config.as_deref().map(BrConfig::parse);
    if cfg.is_none() {
        warnings.push(".config not found; build facts limited".into());
    }
    let summary = cfg.as_ref().map(|c| c.summary()).unwrap_or_default();

    // Package attribution over target/
    let pfl_map = snap.pfl.as_deref().map(pfl::parse);
    if pfl_map.is_none() {
        warnings.push("packages-file-list.txt not found; per-package attribution unavailable".into());
    }
    struct Acc {
        bytes: u64,
        count: u64,
        files: Vec<(u64, String)>,
    }
    let mut per_pkg: HashMap<String, Acc> = HashMap::new();
    let mut rootfs_total: u64 = 0;
    let mut rootfs_files: u64 = 0;
    for e in &snap.target {
        rootfs_files += 1;
        if !e.charged {
            continue;
        }
        rootfs_total += e.size;
        if let Some(map) = &pfl_map {
            let pkg = map
                .get(&e.path)
                .map(|s| s.as_str())
                .unwrap_or(UNATTRIBUTED)
                .to_string();
            let acc = per_pkg.entry(pkg).or_insert(Acc {
                bytes: 0,
                count: 0,
                files: Vec::new(),
            });
            acc.bytes += e.size;
            acc.count += 1;
            acc.files.push((e.size, e.path.clone()));
        }
    }

    // Images classification
    let mut images: Vec<Classified> = snap
        .images
        .iter()
        .filter(|i| i.name != REPORT_FILENAME)
        .map(|i| classify(&i.name, i.size, i.bytes.as_deref()))
        .collect();

    // Flash layout
    let layout = detect_layout(snap, &images, &mut warnings);
    let mut flash: Option<FlashInfo> = None;

    if let Some(layout) = layout {
        let mut parts = mtdparts::MtdParts {
            mtd_id: layout.mtd_id.clone().unwrap_or_default(),
            partitions: layout.partitions,
            declared_end: layout.declared_end,
        };
        // Resolve remainder sizes: prefer the declared span; otherwise the
        // largest image that could plausibly be the whole device.
        let mut total = parts.declared_end;
        if parts.partitions.iter().any(|p| p.size.is_none()) {
            let candidate = images
                .iter()
                .filter(|i| i.format == "raw")
                .map(|i| i.bytes)
                .filter(|&sz| sz >= total)
                .max();
            if let Some(sz) = candidate {
                total = sz;
            }
            if total > 0 {
                parts.resolve_remainders(total);
            } else {
                warnings.push("flash layout has a remainder partition but total size is unknown".into());
            }
        }
        let total_bytes = if total > 0 { Some(total) } else { None };

        // Overlap marking: containers get flagged; partial overlaps flag both.
        let n = parts.partitions.len();
        let mut overlaps = vec![false; n];
        for a in 0..n {
            for b in 0..n {
                if a == b {
                    continue;
                }
                let (pa, pb) = (&parts.partitions[a], &parts.partitions[b]);
                let (Some(sa), Some(sb)) = (pa.size, pb.size) else {
                    continue;
                };
                let (a0, a1) = (pa.offset, pa.offset + sa);
                let (b0, b1) = (pb.offset, pb.offset + sb);
                if a0 <= b0 && a1 >= b1 && (sa > sb) {
                    overlaps[a] = true; // a contains b
                } else if a0 < b1 && b0 < a1 && !(b0 <= a0 && b1 >= a1) && !(a0 <= b0 && a1 >= b1) {
                    overlaps[a] = true; // genuine partial overlap
                }
            }
        }

        // Identify a composite whole-flash image.
        let composite_idx = images.iter().position(|i| {
            (i.format == "raw" || i.format == "disk-image")
                && total_bytes == Some(i.bytes)
        });
        if let Some(ci) = composite_idx {
            if images[ci].format == "raw" {
                images[ci].format = "flash-image".into();
                let pad = json!({
                    "content_end": images[ci].content_end,
                    "trailing_padding": images[ci].bytes - images[ci].content_end,
                });
                images[ci].detail = pad;
            }
        }

        // Match images to partitions: layout hints first (genimage names the
        // content file per partition), then exact stem, then role.
        let mut assigned_image: Vec<Option<usize>> = vec![None; n];
        let mut image_taken: HashSet<usize> = HashSet::new();
        for (pname, iname) in &layout.image_hints {
            let Some(pi) = parts.partitions.iter().position(|p| &p.name == pname) else {
                continue;
            };
            let Some(ii) = images.iter().position(|img| &img.name == iname) else {
                continue;
            };
            if assigned_image[pi].is_none() && !image_taken.contains(&ii) {
                assigned_image[pi] = Some(ii);
                image_taken.insert(ii);
            }
        }
        if let Some(ci) = composite_idx {
            if let Some(span_idx) = (0..n).find(|&i| {
                overlaps[i] && parts.partitions[i].offset == 0 && parts.partitions[i].size == total_bytes
            }) {
                assigned_image[span_idx] = Some(ci);
            }
            image_taken.insert(ci);
        }
        for (pi, p) in parts.partitions.iter().enumerate() {
            if assigned_image[pi].is_some() {
                continue;
            }
            if let Some(ii) = images
                .iter()
                .enumerate()
                .position(|(ii, img)| !image_taken.contains(&ii) && image_stem(&img.name) == p.name.to_ascii_lowercase())
            {
                assigned_image[pi] = Some(ii);
                image_taken.insert(ii);
            }
        }
        for (pi, p) in parts.partitions.iter().enumerate() {
            if assigned_image[pi].is_some() || overlaps[pi] {
                continue;
            }
            let role = partition_role(&p.name);
            if let Some(ii) = images
                .iter()
                .enumerate()
                .position(|(ii, img)| !image_taken.contains(&ii) && image_fits_role(img, role))
            {
                assigned_image[pi] = Some(ii);
                image_taken.insert(ii);
            }
        }

        // Verification against the composite.
        let composite_bytes: Option<&[u8]> = composite_idx.and_then(|ci| {
            snap.images
                .iter()
                .filter(|i| i.name != REPORT_FILENAME)
                .nth(ci)
                .and_then(|i| i.bytes.as_deref())
        });

        let mut partition_reports = Vec::with_capacity(n);
        for (pi, p) in parts.partitions.iter().enumerate() {
            let img = assigned_image[pi].map(|ii| &images[ii]);
            let content_bytes = img.map(|i| i.bytes);
            let used = img.and_then(used_bytes_of);
            if let (Some(cb), Some(sz)) = (content_bytes, p.size) {
                if cb > sz && !overlaps[pi] {
                    warnings.push(format!(
                        "partition '{}': content {} bytes exceeds partition size {} bytes",
                        p.name, cb, sz
                    ));
                }
            }
            let verified = match (composite_bytes, overlaps[pi]) {
                (Some(flash_bytes), false) => {
                    verify_partition(flash_bytes, p, img, assigned_image[pi].and_then(|ii| {
                        snap.images
                            .iter()
                            .filter(|i| i.name != REPORT_FILENAME)
                            .nth(ii)
                            .and_then(|i| i.bytes.as_deref())
                    }))
                }
                _ => None,
            };
            if verified == Some(false) {
                warnings.push(format!(
                    "partition '{}': flash image content does not match expectation at offset 0x{:x}",
                    p.name, p.offset
                ));
            }
            partition_reports.push(PartitionReport {
                name: p.name.clone(),
                offset: p.offset,
                size: p.size,
                read_only: p.read_only,
                image: img.map(|i| i.name.clone()),
                content_bytes,
                used_bytes: used,
                overlaps: overlaps[pi],
                verified,
            });
        }

        // Backfill image -> partition labels.
        for (pi, ai) in assigned_image.iter().enumerate() {
            if let Some(ii) = ai {
                images[*ii].partition = Some(parts.partitions[pi].name.clone());
            }
        }

        flash = Some(FlashInfo {
            source: layout.source,
            mtd_id: layout.mtd_id,
            total_bytes,
            partitions: partition_reports,
        });
    }

    // Rootfs compression facts from the (single) squashfs image.
    let squash_images: Vec<&Classified> = images.iter().filter(|i| i.squash.is_some()).collect();
    let rootfs = if rootfs_total > 0 || !squash_images.is_empty() {
        let (compressed, compression) = if squash_images.len() == 1 {
            let s = squash_images[0].squash.as_ref().unwrap();
            (Some(s.bytes_used), Some(s.compression.clone()))
        } else {
            (None, None)
        };
        let ratio = match (compressed, rootfs_total) {
            (Some(c), t) if t > 0 => Some(c as f64 / t as f64),
            _ => None,
        };
        Some(RootfsReport {
            uncompressed_bytes: rootfs_total,
            file_count: rootfs_files,
            compressed_bytes: compressed,
            compression,
            compression_ratio: ratio,
        })
    } else {
        None
    };
    let ratio = rootfs.as_ref().and_then(|r| r.compression_ratio);

    // Packages
    let mut packages: Vec<PackageReport> = per_pkg
        .into_iter()
        .map(|(name, mut acc)| {
            acc.files.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            let truncated = acc.files.len() > MAX_FILES_PER_PACKAGE;
            PackageReport {
                name,
                bytes: acc.bytes,
                file_count: acc.count,
                compressed_bytes_approx: ratio.map(|r| (acc.bytes as f64 * r) as u64),
                files: acc
                    .files
                    .iter()
                    .take(MAX_FILES_PER_PACKAGE)
                    .map(|(b, p)| FileRef {
                        path: format!("/{p}"),
                        bytes: *b,
                    })
                    .collect(),
                files_truncated: truncated,
            }
        })
        .collect();
    packages.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.name.cmp(&b.name)));

    // Kernel modules
    let normalize = |s: &str| s.replace('-', "_");
    let autoload: HashSet<String> = snap
        .etc_modules
        .as_deref()
        .map(|t| {
            t.lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .filter_map(|l| l.split_whitespace().next())
                .map(|m| normalize(m.trim_end_matches(".ko")))
                .collect()
        })
        .unwrap_or_default();
    let builtin: Vec<String> = snap
        .modules_builtin
        .as_deref()
        .map(|t| {
            t.lines()
                .filter_map(|l| l.trim().strip_suffix(".ko"))
                .filter_map(|l| l.rsplit('/').next())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();

    let mut modules: Vec<ModuleReport> = Vec::new();
    let mut kver: Option<String> = None;
    for e in &snap.target {
        // Merged-usr trees symlink /lib to usr/lib; accept both spellings.
        let Some(rest) = e
            .path
            .strip_prefix("lib/modules/")
            .or_else(|| e.path.strip_prefix("usr/lib/modules/"))
        else {
            continue;
        };
        if !e.path.ends_with(".ko") {
            continue;
        }
        let mut comps = rest.splitn(2, '/');
        let version = comps.next().unwrap_or("");
        if kver.is_none() && !version.is_empty() {
            kver = Some(version.to_string());
        }
        let fname = e.path.rsplit('/').next().unwrap_or(&e.path);
        let name = fname.trim_end_matches(".ko").to_string();
        modules.push(ModuleReport {
            name: name.clone(),
            path: format!("/{}", e.path),
            bytes: e.size,
            package: pfl_map
                .as_ref()
                .and_then(|m| m.get(&e.path).cloned()),
            autoloaded: autoload.contains(&normalize(&name)),
        });
    }
    modules.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.name.cmp(&b.name)));
    let modules_meta = kver.as_ref().map(|k| ModulesMeta {
        kernel_version: k.clone(),
        builtin,
        autoload: {
            let mut v: Vec<String> = autoload.iter().cloned().collect();
            v.sort();
            v
        },
    });

    // Build timings
    let times = snap
        .build_time_log
        .as_deref()
        .map(buildtime::parse)
        .unwrap_or_default();
    let timings: Vec<TimingReport> = times
        .packages
        .iter()
        .map(|p| TimingReport {
            package: p.package.clone(),
            seconds: p.seconds,
            steps: p
                .steps
                .iter()
                .map(|s| StepReport {
                    step: s.step.clone(),
                    seconds: s.seconds,
                })
                .collect(),
        })
        .collect();

    let build = BuildInfo {
        name: snap.root_name.clone(),
        defconfig: summary.defconfig,
        arch: summary.arch,
        target_cpu: summary.target_cpu,
        libc: summary.libc,
        // The modules directory carries the real runtime version; configs
        // often hold a git rev or a custom-repo reference instead.
        kernel_version: kver.clone().or(summary.kernel_version),
        rootfs_types: summary.rootfs_types,
        build_active_seconds: times.active_seconds,
        completed_at_unix: snap
            .images_mtime
            .or(times.finished_at.map(|f| f as i64)),
    };

    let image_reports: Vec<ImageReport> = images
        .into_iter()
        .map(|c| ImageReport {
            name: c.name,
            bytes: c.bytes,
            format: c.format,
            partition: c.partition,
            detail: c.detail,
        })
        .collect();

    // Deliberate removals: installed by a package, absent from the final
    // rootfs, and not something Buildroot's own finalize always strips.
    let mut removed_not_shipped: Vec<RemovedReport> = snap
        .removed_candidates
        .iter()
        .filter(|c| !is_default_finalize_removal(&c.path))
        .map(|c| RemovedReport {
            path: format!("/{}", c.path),
            package: c.package.clone(),
            source_bytes: c.source_bytes,
        })
        .collect();
    removed_not_shipped
        .sort_by(|a, b| b.source_bytes.cmp(&a.source_bytes).then(a.path.cmp(&b.path)));

    Report {
        schema: SCHEMA,
        generator: Generator {
            name: crate::GENERATOR_NAME.to_string(),
            version: crate::GENERATOR_VERSION.to_string(),
        },
        scan: ScanInfo {
            context_source: snap.context_source.as_str().to_string(),
            scan_mode: snap.scan_mode.as_str().to_string(),
            root: snap.root_path.clone(),
            warnings,
        },
        build,
        flash,
        images: image_reports,
        rootfs,
        packages,
        modules,
        modules_meta,
        timings,
        removed_not_shipped,
    }
}

fn verify_partition(
    flash: &[u8],
    p: &mtdparts::MtdPartition,
    img: Option<&Classified>,
    img_bytes: Option<&[u8]>,
) -> Option<bool> {
    let off = p.offset as usize;
    if off >= flash.len() {
        return None;
    }
    if let Some(ib) = img_bytes {
        let cap = p
            .size
            .map(|s| s as usize)
            .unwrap_or(flash.len() - off)
            .min(flash.len() - off);
        let n = ib.len().min(cap);
        if n == 0 {
            return None;
        }
        return Some(&flash[off..off + n] == &ib[..n]);
    }
    // No candidate file: probe by role expectation.
    let role = partition_role(&p.name);
    let window_end = p
        .size
        .map(|s| (off + s as usize).min(flash.len()))
        .unwrap_or(flash.len());
    let window = &flash[off..window_end];
    let _ = img;
    match role {
        Role::Kernel => Some(window.len() >= 4 && crate::parsers::be_u32(window, 0) == Some(uimage::MAGIC)),
        Role::Rootfs => Some(window.len() >= 4 && crate::parsers::le_u32(window, 0) == Some(squashfs::MAGIC)),
        Role::Data => Some(
            window.len() >= 2
                && (window[..2] == [0x85, 0x19] || window[..2] == [0x19, 0x85] || window[..2] == [0xFF, 0xFF]),
        ),
        Role::Env => ubootenv::parse(window).map(|e| e.crc_ok),
        Role::Ubi => Some(ubi::has_header_at(window, 0)),
        Role::Boot => {
            let probe = &window[..window.len().min(256)];
            Some(!probe.iter().all(|&b| b == 0xFF || b == 0x00))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crc::{crc32_ieee, crc32_jffs2};
    use crate::snapshot::{ContextSource, ImageInput, NamedText, ScanMode, Snapshot, TargetEntry};

    fn synth_squashfs(bytes_used: u64, file_size: usize) -> Vec<u8> {
        let mut d = vec![0u8; file_size];
        d[0..4].copy_from_slice(&squashfs::MAGIC.to_le_bytes());
        d[4..8].copy_from_slice(&42u32.to_le_bytes());
        d[12..16].copy_from_slice(&131_072u32.to_le_bytes());
        d[20..22].copy_from_slice(&4u16.to_le_bytes()); // xz
        d[22..24].copy_from_slice(&17u16.to_le_bytes());
        d[28..30].copy_from_slice(&4u16.to_le_bytes());
        d[40..48].copy_from_slice(&bytes_used.to_le_bytes());
        d
    }

    fn synth_jffs2(file_size: usize) -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&0x1985u16.to_le_bytes());
        buf.extend_from_slice(&0x2003u16.to_le_bytes());
        buf.extend_from_slice(&12u32.to_le_bytes());
        let crc = crc32_jffs2(&buf[0..8]);
        buf.extend_from_slice(&crc.to_le_bytes());
        buf.resize(file_size, 0xFF);
        buf
    }

    fn synth_uimage(payload_len: usize, file_size: usize) -> Vec<u8> {
        let payload = vec![0xABu8; payload_len];
        let mut h = vec![0u8; 64];
        h[0..4].copy_from_slice(&uimage::MAGIC.to_be_bytes());
        h[12..16].copy_from_slice(&(payload_len as u32).to_be_bytes());
        h[24..28].copy_from_slice(&crc32_ieee(&payload).to_be_bytes());
        h[28] = 5;
        h[29] = 5;
        h[30] = 2;
        h[31] = 3;
        h[32..38].copy_from_slice(b"kernel");
        let hcrc = crc32_ieee(&h);
        h[4..8].copy_from_slice(&hcrc.to_be_bytes());
        h.extend_from_slice(&payload);
        h.resize(file_size, 0xFF);
        h
    }

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

    #[test]
    fn env_vars_are_reported_in_full_sorted_by_key() {
        let img = synth_env(
            &[
                ("bootcmd", "bootm 0x80600000"),
                ("baudrate", "115200"),
                ("mtdparts", "nor0:64k(boot),-(rootfs)"),
                // An empty value is a real thing to set, and must survive as
                // an empty string rather than becoming a missing key.
                ("debug", ""),
            ],
            4096,
        );
        let c = classify("u-boot-env.bin", img.len() as u64, Some(&img));
        assert_eq!(c.format, "uboot-env");
        assert_eq!(c.detail["var_count"], json!(4));
        assert_eq!(c.detail["vars_truncated"], json!(false));
        let vars = c.detail["vars"].as_array().unwrap();

        // Sorted by key, the way fw_printenv prints them.
        let keys: Vec<&str> = vars.iter().map(|v| v["key"].as_str().unwrap()).collect();
        assert_eq!(keys, vec!["baudrate", "bootcmd", "debug", "mtdparts"]);

        let get = |k: &str| vars.iter().find(|v| v["key"] == k).unwrap();
        assert_eq!(get("bootcmd")["value"], json!("bootm 0x80600000"));
        assert_eq!(get("bootcmd")["bytes"], json!(16));
        assert_eq!(get("mtdparts")["value"], json!("nor0:64k(boot),-(rootfs)"));
        assert_eq!(get("debug")["value"], json!(""));
        assert_eq!(get("debug")["bytes"], json!(0));
        // Every variable carries its value; nothing is dropped or rewritten.
        assert!(vars.iter().all(|v| v["value"].is_string()));
    }

    const K: u64 = 1024;

    fn build_snapshot() -> Snapshot {
        let mut s = Snapshot::empty("testbuild");
        s.root_path = "/tmp/testbuild".into();
        s.context_source = ContextSource::Inferred;
        s.scan_mode = ScanMode::Native;

        s.config = Some(
            "BR2_ARCH=\"mipsel\"\nBR2_TOOLCHAIN_USES_UCLIBC=y\nBR2_TARGET_ROOTFS_SQUASHFS=y\nBR2_TARGET_ROOTFS_SQUASHFS4_XZ=y\n".into(),
        );
        s.pfl = Some("busybox,./bin/busybox\nkmod-x,./lib/modules/3.10.14/kernel/net/x.ko\n".into());
        s.build_time_log = Some("100.0:start:build:  busybox\n150.0:end  :build:  busybox\n".into());
        s.etc_modules = Some("x\n".into());
        s.target = vec![
            TargetEntry {
                path: "bin/busybox".into(),
                size: 500_000,
                is_symlink: false,
                charged: true,
            },
            TargetEntry {
                path: "etc/overlayfile".into(),
                size: 2_000,
                is_symlink: false,
                charged: true,
            },
            TargetEntry {
                path: "lib/modules/3.10.14/kernel/net/x.ko".into(),
                size: 5_000,
                is_symlink: false,
                charged: true,
            },
        ];

        // Layout: 64k boot, 64k env, 256k kernel, 512k rootfs, 128k data = 1 MiB
        s.env_texts = vec![NamedText {
            name: "uenv.txt".into(),
            text: "mtdparts=nor0:64k(boot),64k(env),256k(kernel),512k(rootfs),128k(data)\n".into(),
        }];

        let boot = {
            let mut b = vec![0xAAu8; 10_000];
            b.resize(20_000, 0xFF);
            b
        };
        let env = synth_env(&[("bootcmd", "run x"), ("baudrate", "115200")], 64 * K as usize);
        let kernel = synth_uimage(100_000, 150_000);
        let rootfs = synth_squashfs(300_000, 400_000);
        let data = synth_jffs2(128 * K as usize);

        let mut flashimg = vec![0xFFu8; 1024 * K as usize];
        flashimg[0..boot.len()].copy_from_slice(&boot);
        flashimg[(64 * K) as usize..(64 * K) as usize + env.len()].copy_from_slice(&env);
        flashimg[(128 * K) as usize..(128 * K) as usize + kernel.len()].copy_from_slice(&kernel);
        flashimg[(384 * K) as usize..(384 * K) as usize + rootfs.len()].copy_from_slice(&rootfs);
        flashimg[(896 * K) as usize..(896 * K) as usize + data.len()].copy_from_slice(&data);

        s.images = vec![
            ImageInput {
                name: "u-boot.bin".into(),
                size: boot.len() as u64,
                bytes: Some(boot),
            },
            ImageInput {
                name: "u-boot-env.bin".into(),
                size: env.len() as u64,
                bytes: Some(env),
            },
            ImageInput {
                name: "uImage".into(),
                size: kernel.len() as u64,
                bytes: Some(kernel),
            },
            ImageInput {
                name: "rootfs.squashfs".into(),
                size: rootfs.len() as u64,
                bytes: Some(rootfs),
            },
            ImageInput {
                name: "data.jffs2".into(),
                size: data.len() as u64,
                bytes: Some(data),
            },
            ImageInput {
                name: "firmware.bin".into(),
                size: flashimg.len() as u64,
                bytes: Some(flashimg),
            },
        ];
        s
    }

    #[test]
    fn full_pipeline() {
        let report = analyze(&build_snapshot());

        // Flash layout detected from uenv.txt, 1 MiB total.
        let flash = report.flash.as_ref().unwrap();
        assert!(flash.source.contains("uenv.txt"));
        assert_eq!(flash.total_bytes, Some(1024 * K));
        assert_eq!(flash.partitions.len(), 5);

        let part = |n: &str| flash.partitions.iter().find(|p| p.name == n).unwrap();
        assert_eq!(part("boot").image.as_deref(), Some("u-boot.bin"));
        assert_eq!(part("env").image.as_deref(), Some("u-boot-env.bin"));
        assert_eq!(part("kernel").image.as_deref(), Some("uImage"));
        assert_eq!(part("rootfs").image.as_deref(), Some("rootfs.squashfs"));
        assert_eq!(part("data").image.as_deref(), Some("data.jffs2"));

        // Format-aware usage.
        assert_eq!(part("rootfs").used_bytes, Some(300_000));
        assert_eq!(part("kernel").used_bytes, Some(100_064));
        assert_eq!(part("data").used_bytes, Some(12));
        assert_eq!(part("boot").used_bytes, Some(10_000));

        // Composite verification: every matched partition checks out.
        for p in &flash.partitions {
            assert_eq!(p.verified, Some(true), "partition {}", p.name);
        }

        // The composite got reclassified.
        let fw = report.images.iter().find(|i| i.name == "firmware.bin").unwrap();
        assert_eq!(fw.format, "flash-image");

        // Rootfs facts and ratio-based package approximation.
        let rootfs = report.rootfs.as_ref().unwrap();
        assert_eq!(rootfs.uncompressed_bytes, 507_000);
        assert_eq!(rootfs.compressed_bytes, Some(300_000));
        assert!(rootfs.compression_ratio.unwrap() > 0.5);

        // Packages: busybox, kmod-x, plus unattributed overlay file.
        assert_eq!(report.packages.len(), 3);
        assert_eq!(report.packages[0].name, "busybox");
        assert!(report
            .packages
            .iter()
            .any(|p| p.name == UNATTRIBUTED && p.bytes == 2_000));

        // Modules with autoload flag.
        assert_eq!(report.modules.len(), 1);
        assert!(report.modules[0].autoloaded);
        assert_eq!(report.modules_meta.as_ref().unwrap().kernel_version, "3.10.14");

        // Timings.
        assert_eq!(report.timings.len(), 1);
        assert!((report.timings[0].seconds - 50.0).abs() < 1e-9);

        // Build facts.
        assert_eq!(report.build.libc.as_deref(), Some("uclibc"));
        assert_eq!(report.build.kernel_version.as_deref(), Some("3.10.14"));
    }

    #[test]
    fn no_layout_still_reports() {
        let mut s = build_snapshot();
        s.env_texts.clear();
        s.images.retain(|i| i.name != "u-boot-env.bin" && i.name != "firmware.bin");
        let report = analyze(&s);
        assert!(report.flash.is_none());
        assert!(!report.packages.is_empty());
        assert!(report
            .scan
            .warnings
            .iter()
            .any(|w| w.contains("no flash layout")));
    }
}
