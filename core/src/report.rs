//! The schema-versioned output of an analysis. Serialized as report.json;
//! the viewer and any downstream consumer treat this as the contract.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA: u32 = 1;

/// Filename buildscope writes into images/. Scanners must skip this file
/// when reading images/ so reports never describe themselves.
pub const REPORT_FILENAME: &str = "buildscope-report.json";

/// Ceiling on the per-package file list, so a rootfs with tens of thousands
/// of files cannot turn a report into a multi-megabyte document.
pub const MAX_FILES_PER_PACKAGE: usize = 3000;

/// Synthetic package name for rootfs files not present in
/// packages-file-list.txt (overlay contents, post-build script output).
pub const UNATTRIBUTED: &str = "_unattributed";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Generator {
    pub name: String,
    pub version: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ScanInfo {
    pub context_source: String,
    pub scan_mode: String,
    pub root: String,
    pub warnings: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct BuildInfo {
    pub name: String,
    pub defconfig: Option<String>,
    pub arch: Option<String>,
    pub target_cpu: Option<String>,
    pub libc: Option<String>,
    pub kernel_version: Option<String>,
    pub rootfs_types: Vec<String>,
    /// Union of instrumented build-step intervals from build-time.log.
    pub build_active_seconds: Option<f64>,
    pub completed_at_unix: Option<i64>,
    /// The target's os-release, parsed. Standard freedesktop keys plus
    /// whatever the project adds; a branch and revision usually land in
    /// VERSION_CODENAME and BUILD_ID. Empty when the tree had no such file,
    /// which is the case for a bare artifact.
    #[serde(default)]
    pub os_release: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PartitionReport {
    pub name: String,
    pub offset: u64,
    /// None when the layout declared a remainder that could not be resolved.
    pub size: Option<u64>,
    pub read_only: bool,
    /// Matched file from images/, if any.
    pub image: Option<String>,
    /// Size of the matched image file.
    pub content_bytes: Option<u64>,
    /// Format-aware real usage inside the partition (squashfs bytes_used,
    /// jffs2 valid nodes, env used bytes, ...). None when unknowable.
    pub used_bytes: Option<u64>,
    /// True when this partition's range contains or intersects another's
    /// (e.g. a whole-device spanning entry).
    pub overlaps: bool,
    /// Content check against the composite flash image: Some(true) when the
    /// bytes at this partition's offset match expectations, None when there
    /// was nothing to check against.
    pub verified: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FlashInfo {
    /// Human-readable provenance, e.g. "mtdparts (uenv.txt)".
    pub source: String,
    pub mtd_id: Option<String>,
    pub total_bytes: Option<u64>,
    pub partitions: Vec<PartitionReport>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ImageReport {
    pub name: String,
    pub bytes: u64,
    /// "squashfs" | "jffs2" | "uimage" | "uboot-env" | "disk-image" |
    /// "flash-image" | "text" | "raw"
    pub format: String,
    /// Partition this image was matched to, if any.
    pub partition: Option<String>,
    /// Format-specific facts.
    pub detail: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RootfsReport {
    pub uncompressed_bytes: u64,
    pub file_count: u64,
    pub compressed_bytes: Option<u64>,
    pub compression: Option<String>,
    pub compression_ratio: Option<f64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileRef {
    pub path: String,
    pub bytes: u64,
    /// What the file costs on the medium once compressed, read from the
    /// filesystem image rather than estimated. Absent when the image could not
    /// be read, or when there is no image to read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compressed_bytes: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PackageReport {
    pub name: String,
    pub bytes: u64,
    pub file_count: u64,
    pub compressed_bytes_approx: Option<u64>,
    /// Every file the package installed, largest first, so a reader can
    /// browse the rootfs rather than guess from package totals. Capped at
    /// `MAX_FILES_PER_PACKAGE`; the alias keeps reports written before this
    /// held the whole list readable.
    #[serde(alias = "top_files")]
    pub files: Vec<FileRef>,
    /// True when the list above was cut short by the cap.
    #[serde(default)]
    pub files_truncated: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModuleReport {
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub package: Option<String>,
    pub autoloaded: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModulesMeta {
    pub kernel_version: String,
    pub builtin: Vec<String>,
    pub autoload: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RemovedReport {
    /// Absolute path as it would have appeared in the rootfs.
    pub path: String,
    pub package: String,
    /// Size in per-package/<pkg>/target when recoverable, else 0.
    pub source_bytes: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StepReport {
    pub step: String,
    pub seconds: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TimingReport {
    pub package: String,
    pub seconds: f64,
    pub steps: Vec<StepReport>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Report {
    pub schema: u32,
    pub generator: Generator,
    pub scan: ScanInfo,
    pub build: BuildInfo,
    pub flash: Option<FlashInfo>,
    pub images: Vec<ImageReport>,
    pub rootfs: Option<RootfsReport>,
    pub packages: Vec<PackageReport>,
    pub modules: Vec<ModuleReport>,
    pub modules_meta: Option<ModulesMeta>,
    pub timings: Vec<TimingReport>,
    /// Files a package installed that are absent from the final rootfs,
    /// excluding Buildroot's default target-finalize removals (headers,
    /// static libs, docs, ...). Populated only when the scanner had
    /// filesystem access to per-package/.
    #[serde(default)]
    pub removed_not_shipped: Vec<RemovedReport>,
}
