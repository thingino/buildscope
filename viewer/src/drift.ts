// Client-side drift computation, mirroring core's diff.rs semantics:
// unchanged entries omitted, added/removed carried as null sides.

import { envVarsOf, kernelConfigOf, kernelVersionOf } from "./extract";
import { Report } from "./types";

export interface TotalDelta {
  before: number;
  after: number;
  delta: number;
}

export interface NamedDelta {
  name: string;
  before: number | null;
  after: number | null;
  delta: number;
}

export interface PartitionDelta {
  name: string;
  sizeBefore: number | null;
  sizeAfter: number | null;
  usedBefore: number | null;
  usedAfter: number | null;
  /** Null for a partition that contains others: see `overlaps`. */
  usedDelta: number | null;
  /** This partition spans others, so its bytes are counted twice over. */
  overlaps: boolean;
}

/**
 * A setting that changed, rather than a size that did.
 *
 * Config options and environment variables carry text, so the interesting
 * thing is the pair of values, not a signed number: `y` becoming `m` is the
 * whole story and has no magnitude.
 */
export interface ValueDelta {
  name: string;
  before: string | null;
  after: string | null;
}

/** Set when the two sides are not really comparable, so a caller can say so
 *  instead of listing thousands of differences that all mean "different
 *  kernel". */
export interface ValueDiff {
  entries: ValueDelta[];
  comparable: boolean;
  beforeLabel: string | null;
  afterLabel: string | null;
}

export interface Drift {
  rootfsUncompressed: TotalDelta | null;
  rootfsCompressed: TotalDelta | null;
  config: ValueDiff;
  env: ValueDelta[];
  partitions: PartitionDelta[];
  images: NamedDelta[];
  packages: NamedDelta[];
  modules: NamedDelta[];
}

function namedDeltas(a: Map<string, number>, b: Map<string, number>): NamedDelta[] {
  const names = new Set([...a.keys(), ...b.keys()]);
  const out: NamedDelta[] = [];
  for (const n of names) {
    const before = a.has(n) ? a.get(n)! : null;
    const after = b.has(n) ? b.get(n)! : null;
    if (before === after) continue;
    out.push({ name: n, before, after, delta: (after ?? 0) - (before ?? 0) });
  }
  out.sort((x, y) => Math.abs(y.delta) - Math.abs(x.delta) || x.name.localeCompare(y.name));
  return out;
}

const toMap = (entries: [string, number][]) => new Map(entries);

/** The dotted version at the front of a kernel string, dropping whatever a
 *  vendor tree appended to it. */
function series(v: string): string {
  return v.match(/^\d+(?:\.\d+)*/)?.[0] ?? v;
}

/** Text settings that differ, sorted so the eye lands on the name. */
function valueDeltas(a: Map<string, string>, b: Map<string, string>): ValueDelta[] {
  const out: ValueDelta[] = [];
  for (const n of new Set([...a.keys(), ...b.keys()])) {
    const before = a.has(n) ? a.get(n)! : null;
    const after = b.has(n) ? b.get(n)! : null;
    if (before === after) continue;
    out.push({ name: n, before, after });
  }
  out.sort((x, y) => x.name.localeCompare(y.name));
  return out;
}

/**
 * Kernel options that changed, and whether asking was meaningful.
 *
 * Across a kernel version bump almost every option differs, for the same
 * reason two different kernels are two different kernels; a list of three
 * thousand rows would bury the handful that were actually decided. The
 * versions are reported instead and the comparison withheld.
 */
function configDiff(a: Report, b: Report): ValueDiff {
  const va = kernelVersionOf(a);
  const vb = kernelVersionOf(b);
  // Compared on the version proper, not the whole string: a vendor tree
  // appends its own name, so "3.10.14__isvp_swan_1.0__" and "3.10.14" are the
  // same kernel and their options are worth diffing. Only a real series change
  // makes the comparison meaningless.
  const comparable = !va || !vb || series(va) === series(vb);
  const entries = comparable
    ? valueDeltas(
        new Map(kernelConfigOf(a).map((e) => [e.key, e.value])),
        new Map(kernelConfigOf(b).map((e) => [e.key, e.value]))
      )
    : [];
  return { entries, comparable, beforeLabel: va, afterLabel: vb };
}

export function computeDrift(a: Report, b: Report): Drift {
  const rootfsUncompressed =
    a.rootfs && b.rootfs
      ? {
          before: a.rootfs.uncompressed_bytes,
          after: b.rootfs.uncompressed_bytes,
          delta: b.rootfs.uncompressed_bytes - a.rootfs.uncompressed_bytes,
        }
      : null;
  const ca = a.rootfs?.compressed_bytes ?? null;
  const cb = b.rootfs?.compressed_bytes ?? null;
  const rootfsCompressed =
    ca !== null && cb !== null ? { before: ca, after: cb, delta: cb - ca } : null;

  const partitions: PartitionDelta[] = [];
  const pa = new Map((a.flash?.partitions ?? []).map((p) => [p.name, p]));
  const pb = new Map((b.flash?.partitions ?? []).map((p) => [p.name, p]));
  // A partition spanning others -- a whole-chip alias like `all` -- holds no
  // bytes of its own: everything in it is already counted in the rows it
  // covers. Subtracting one snapshot's copy of that total from the other's is
  // a difference between two double-counts, and when the alias is added or
  // removed by a layout change it reads as the whole chip being filled or
  // freed, which is the largest number in the table and means nothing.
  //
  // Only worth suppressing while something else accounts for the bytes; a
  // layout that is nothing but a container still has to report itself.
  const anyReal = [...pa.values(), ...pb.values()].some((p) => !p.overlaps);

  for (const n of new Set([...pa.keys(), ...pb.keys()])) {
    const x = pa.get(n);
    const y = pb.get(n);
    const usedBefore = x ? x.used_bytes ?? x.content_bytes : null;
    const usedAfter = y ? y.used_bytes ?? y.content_bytes : null;
    if (x && y && usedBefore === usedAfter) continue;
    const overlaps = !!(x?.overlaps || y?.overlaps) && anyReal;
    partitions.push({
      name: n,
      sizeBefore: x?.size ?? null,
      sizeAfter: y?.size ?? null,
      usedBefore,
      usedAfter,
      usedDelta: overlaps ? null : (usedAfter ?? 0) - (usedBefore ?? 0),
      overlaps,
    });
  }
  // Rows with no meaningful delta sort last rather than leading the table.
  partitions.sort((x, y) => {
    if (x.usedDelta === null || y.usedDelta === null) {
      return (x.usedDelta === null ? 1 : 0) - (y.usedDelta === null ? 1 : 0);
    }
    return Math.abs(y.usedDelta) - Math.abs(x.usedDelta);
  });

  return {
    rootfsUncompressed,
    rootfsCompressed,
    config: configDiff(a, b),
    env: valueDeltas(envVarsOf(a), envVarsOf(b)),
    partitions,
    images: namedDeltas(
      toMap(a.images.map((i) => [i.name, i.bytes])),
      toMap(b.images.map((i) => [i.name, i.bytes]))
    ),
    packages: namedDeltas(
      toMap(a.packages.map((p) => [p.name, p.bytes])),
      toMap(b.packages.map((p) => [p.name, p.bytes]))
    ),
    modules: namedDeltas(
      toMap(a.modules.map((m) => [m.name, m.bytes])),
      toMap(b.modules.map((m) => [m.name, m.bytes]))
    ),
  };
}
