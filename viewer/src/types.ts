// Mirrors buildscope-core's report schema v1 (core/src/report.rs).

export interface Report {
  schema: number;
  generator: { name: string; version: string };
  scan: {
    context_source: string;
    scan_mode: string;
    root: string;
    warnings: string[];
  };
  build: {
    name: string;
    defconfig: string | null;
    arch: string | null;
    target_cpu: string | null;
    libc: string | null;
    kernel_version: string | null;
    rootfs_types: string[];
    build_active_seconds: number | null;
    completed_at_unix: number | null;
  };
  flash: FlashInfo | null;
  images: ImageReport[];
  rootfs: {
    uncompressed_bytes: number;
    file_count: number;
    compressed_bytes: number | null;
    compression: string | null;
    compression_ratio: number | null;
  } | null;
  packages: PackageReport[];
  modules: ModuleReport[];
  modules_meta: {
    kernel_version: string;
    builtin: string[];
    autoload: string[];
  } | null;
  timings: TimingReport[];
  removed_not_shipped?: RemovedReport[];
}

export interface FlashInfo {
  source: string;
  mtd_id: string | null;
  total_bytes: number | null;
  partitions: PartitionReport[];
}

export interface PartitionReport {
  name: string;
  offset: number;
  size: number | null;
  read_only: boolean;
  image: string | null;
  content_bytes: number | null;
  used_bytes: number | null;
  overlaps: boolean;
  verified: boolean | null;
}

export interface ImageReport {
  name: string;
  bytes: number;
  format: string;
  partition: string | null;
  detail: Record<string, unknown>;
}

/** One variable from the `detail` of an image whose format is "uboot-env". */
export interface EnvVar {
  key: string;
  value: string;
  bytes: number;
}

/** Shape of the `detail` on an image whose format is "ubi". */
export interface UbiDetail {
  ubi_offset: number;
  peb_size: number;
  leb_size: number;
  total_pebs: number;
  mapped_pebs: number;
  free_pebs: number;
  erased_pebs: number;
  bad_pebs: number;
  used_bytes: number;
  overhead_bytes: number;
  volume_table_found: boolean;
  unmapped_volumes: string[];
  volumes: UbiVolume[];
}

export interface UbiVolume {
  id: number;
  name: string;
  /** "static" | "dynamic" */
  type: string;
  reserved_pebs: number;
  mapped_pebs: number;
  /** What the volume table set aside: reserved blocks × block size. */
  capacity_bytes: number;
  /** Payload actually present in this image. */
  bytes: number;
  /** Flash the written blocks occupy, per-block headers included. */
  flash_bytes: number;
  /** Null when the table reserved the volume but nothing was written. */
  offset: number | null;
  autoresize: boolean;
  contiguous: boolean | null;
  has_holes: boolean | null;
}

export interface FileRef {
  path: string;
  bytes: number;
  /** Measured from the filesystem image, so absent when there was none. */
  compressed_bytes?: number;
}

export interface PackageReport {
  name: string;
  bytes: number;
  file_count: number;
  compressed_bytes_approx: number | null;
  /** Every file the package installed, largest first. */
  files: FileRef[];
  files_truncated?: boolean;
  /** Reports written before the full list existed carry only the top few. */
  top_files?: FileRef[];
}

export interface ModuleReport {
  name: string;
  path: string;
  bytes: number;
  package: string | null;
  autoloaded: boolean;
}

export interface TimingReport {
  package: string;
  seconds: number;
  steps: { step: string; seconds: number }[];
}

export interface RemovedReport {
  path: string;
  package: string;
  source_bytes: number;
}

/**
 * One line of a build index, cheap enough that a whole fleet of them loads
 * before anything is opened. How a report is located differs by source: a
 * static site numbers them, a fleet snapshot names a member of its tarball.
 */
export interface IndexEntry {
  name: string;
  /** Static site: the report's number under api/report/. */
  id?: number;
  /** Fleet snapshot: the member holding this report inside the tarball. */
  file?: string;
  flash_bytes?: number | null;
  rootfs_bytes?: number | null;
  fullest_partition?: string | null;
  fullest_fill?: number | null;
}

export const UNATTRIBUTED = "_unattributed";
