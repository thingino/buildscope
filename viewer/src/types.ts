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

export interface FileRef {
  path: string;
  bytes: number;
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

export interface IndexEntry {
  id: number;
  name: string;
}

export const UNATTRIBUTED = "_unattributed";
