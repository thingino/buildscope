// Client-side drift computation, mirroring core's diff.rs semantics:
// unchanged entries omitted, added/removed carried as null sides.

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
  usedDelta: number;
}

export interface Drift {
  rootfsUncompressed: TotalDelta | null;
  rootfsCompressed: TotalDelta | null;
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
  for (const n of new Set([...pa.keys(), ...pb.keys()])) {
    const x = pa.get(n);
    const y = pb.get(n);
    const usedBefore = x ? x.used_bytes ?? x.content_bytes : null;
    const usedAfter = y ? y.used_bytes ?? y.content_bytes : null;
    if (x && y && usedBefore === usedAfter) continue;
    partitions.push({
      name: n,
      sizeBefore: x?.size ?? null,
      sizeAfter: y?.size ?? null,
      usedBefore,
      usedAfter,
      usedDelta: (usedAfter ?? 0) - (usedBefore ?? 0),
    });
  }
  partitions.sort((x, y) => Math.abs(y.usedDelta) - Math.abs(x.usedDelta));

  return {
    rootfsUncompressed,
    rootfsCompressed,
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
