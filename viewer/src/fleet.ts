/**
 * Fleet mode: a whole matrix of builds published as two release assets --
 * fleet-index.json and fleet-reports.tar.gz -- instead of as a hosted site.
 *
 * GitHub omits CORS headers on release asset *bytes*, so a browser cannot read
 * them from github.com directly and they come through a proxy. Discovering
 * which snapshots exist goes through it too -- not for CORS, but because the
 * REST releases endpoint answers that question in ~18 MB and out of the
 * reader's own rate limit.
 *
 * The index is small and is loaded up front; the tarball is not touched until
 * a build is actually opened, and is then held decompressed but unparsed, so
 * only the report being read costs anything to turn into objects.
 */
import { Loaded } from "./data";
import { IndexEntry, Report } from "./types";

/** buildscope's own proxy (worker/), separate from the ones firmware flashing
 *  and the image builder use so a bad deploy of one cannot take down another. */
const ASSET_PROXY = "https://buildscope-fleet.thingino.workers.dev/fleet";
/** Short names the proxy also knows; `?repo=` picks one. Kept in step with
 *  its allow-list, which is what actually decides what can be read. */
const REPOS: Record<string, string> = {
  thingino: "themactep/thingino-firmware",
  gtxaspec: "gtxaspec/thingino-firmware",
};
const DEFAULT_REPO = "thingino";
const INDEX_ASSET = "fleet-index.json";
const TAR_ASSET = "fleet-reports.tar.gz";
const BLOCK = 512;

export interface FleetSource {
  indexUrl: string;
  tarUrl: string;
  /** The release this came from, or null when pointed at a plain directory. */
  tag: string | null;
}

/** The `?fleet=` the page was opened with, if any. */
export function fleetSpec(): string | null {
  return new URLSearchParams(window.location.search).get("fleet");
}

/** `?repo=`, falling back to the main one. An unknown name is not an error
 *  worth stopping for: the proxy would reject it anyway. */
export function fleetRepo(): string {
  const r = new URLSearchParams(window.location.search).get("repo");
  return r && r in REPOS ? r : DEFAULT_REPO;
}

/**
 * A spec is either a URL to a directory holding the two assets, or a release
 * tag. The URL form keeps this useful to any project that publishes a
 * snapshot; the tag form is the shorthand for the one that ships with it.
 */
export function resolveSource(spec: string, repo = DEFAULT_REPO): FleetSource {
  if (/^https?:\/\//i.test(spec)) {
    const base = spec.endsWith("/") ? spec : spec + "/";
    return { indexUrl: base + INDEX_ASSET, tarUrl: base + TAR_ASSET, tag: null };
  }
  const at = (name: string) =>
    `${ASSET_PROXY}?repo=${encodeURIComponent(repo)}` +
    `&tag=${encodeURIComponent(spec)}&name=${encodeURIComponent(name)}`;
  return { indexUrl: at(INDEX_ASSET), tarUrl: at(TAR_ASSET), tag: spec };
}

/**
 * Releases that carry a snapshot.
 *
 * Answered by the proxy, not by api.github.com directly. That endpoint embeds
 * every release's full asset list -- ~18 MB of JSON for a firmware repo, to
 * learn a handful of tag names -- and spends the reader's 60-per-hour
 * unauthenticated quota, so a shared address exhausts it and sees nothing at
 * all. The proxy answers the same question in tens of bytes, cached.
 */
export async function listReleases(repo = DEFAULT_REPO): Promise<string[]> {
  const res = await fetch(`${ASSET_PROXY}/releases?repo=${encodeURIComponent(repo)}`);
  if (!res.ok) throw new Error(`releases: HTTP ${res.status}`);
  const body = (await res.json()) as { tags?: unknown };
  return Array.isArray(body.tags) ? (body.tags as string[]) : [];
}

async function fetchBytes(
  url: string,
  onProgress?: (done: number, total: number | null) => void
): Promise<Uint8Array> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const len = res.headers.get("content-length");
  const total = len ? Number(len) : null;
  if (!res.body || !onProgress) return new Uint8Array(await res.arrayBuffer());

  const reader = res.body.getReader();
  const chunks: Uint8Array[] = [];
  let done = 0;
  for (;;) {
    const step = await reader.read();
    if (step.done) break;
    chunks.push(step.value);
    done += step.value.length;
    onProgress(done, total);
  }
  const out = new Uint8Array(done);
  let at = 0;
  for (const c of chunks) {
    out.set(c, at);
    at += c.length;
  }
  return out;
}

/** Native gzip. No library, and nothing of the WASM core is involved. */
async function gunzip(body: Uint8Array): Promise<Uint8Array> {
  const stream = new Blob([body as BlobPart])
    .stream()
    .pipeThrough(new DecompressionStream("gzip"));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

function field(d: Uint8Array, off: number, len: number): string {
  let end = off;
  while (end < off + len && d[end] !== 0) end++;
  return new TextDecoder().decode(d.subarray(off, end));
}

/**
 * ustar entries as name -> bytes. The values are views into `d`, not copies,
 * so holding the whole map costs one buffer rather than one per member.
 */
export function walkTar(d: Uint8Array): Map<string, Uint8Array> {
  const out = new Map<string, Uint8Array>();
  for (let off = 0; off + BLOCK <= d.length; ) {
    const name = field(d, off, 100);
    if (!name) break; // the pair of zero blocks that ends an archive
    const size = parseInt(field(d, off + 124, 12).trim() || "0", 8);
    if (!Number.isFinite(size) || size < 0) break;
    const start = off + BLOCK;
    out.set(name, d.subarray(start, start + size));
    off = start + Math.ceil(size / BLOCK) * BLOCK;
  }
  return out;
}

/**
 * One release's snapshot, addressed by build NAME rather than by position.
 *
 * Drift across releases needs this: position means nothing between two
 * snapshots, since a profile added or dropped shifts everything after it, and
 * the whole point is to compare a profile against its own past.
 */
export interface Snapshot {
  entries: IndexEntry[];
  /** Null when that profile was not built in this release, which is an answer
   *  rather than a failure -- a build gets added or retired between runs. */
  byName(name: string): Promise<Report | null>;
}

/** Held per tag for the life of the page: flipping the baseline back and forth
 *  across a few releases should cost one download each, not one per look. */
const snapshots = new Map<string, Promise<Snapshot>>();

export function loadSnapshot(tag: string, repo = DEFAULT_REPO): Promise<Snapshot> {
  const key = `${repo}\0${tag}`;
  let held = snapshots.get(key);
  if (!held) {
    // A rejection must not be cached, or one flaky fetch makes that release
    // permanently broken for the rest of the session.
    held = fetchSnapshot(tag, repo).catch((e) => {
      snapshots.delete(key);
      throw e;
    });
    snapshots.set(key, held);
  }
  return held;
}

async function fetchSnapshot(tag: string, repo: string): Promise<Snapshot> {
  const src = resolveSource(tag, repo);
  const res = await fetch(src.indexUrl);
  if (!res.ok) throw new Error(`fleet index: HTTP ${res.status}`);
  const idx = (await res.json()) as { reports: IndexEntry[] };
  if (!Array.isArray(idx.reports)) throw new Error("fleet index: malformed");

  const fileOf = new Map(idx.reports.map((e) => [e.name, e.file]));
  let members: Promise<Map<string, Uint8Array>> | null = null;
  const tarball = () => (members ??= fetchBytes(src.tarUrl).then(gunzip).then(walkTar));

  return {
    entries: idx.reports,
    async byName(name: string): Promise<Report | null> {
      const file = fileOf.get(name);
      if (!file) return null;
      const found = (await tarball()).get(file);
      if (!found) throw new Error(`${file} is not in the ${tag} snapshot`);
      return JSON.parse(new TextDecoder().decode(found)) as Report;
    },
  };
}

/**
 * Resolve a spec to a loaded fleet. `spec` may be "latest", a release tag, or
 * a URL. `onProgress` reports the tarball download, which is the only part
 * big enough to be worth waiting on.
 */
export async function loadFleet(
  spec: string,
  onProgress?: (done: number, total: number | null) => void
): Promise<Loaded & { tag: string | null; tags: string[] }> {
  const repo = fleetRepo();
  let tags: string[] = [];
  let resolved = spec;
  if (spec === "latest" || spec === "") {
    tags = await listReleases(repo);
    if (tags.length === 0) throw new Error("no published fleet snapshots yet");
    resolved = tags[0];
  }
  const src = resolveSource(resolved, repo);
  // The picker needs the list even when the caller named a tag outright.
  // Failing to get it costs the picker, not the snapshot.
  if (src.tag && tags.length === 0) tags = await listReleases(repo).catch(() => []);

  const res = await fetch(src.indexUrl);
  if (!res.ok) throw new Error(`fleet index: HTTP ${res.status}`);
  const idx = (await res.json()) as { reports: IndexEntry[] };
  if (!Array.isArray(idx.reports)) throw new Error("fleet index: malformed");

  // Fetched once, on the first build actually opened, and shared by every
  // read after that -- including the drift baselines.
  let members: Promise<Map<string, Uint8Array>> | null = null;
  const tarball = () =>
    (members ??= fetchBytes(src.tarUrl, onProgress).then(gunzip).then(walkTar));

  return {
    entries: idx.reports,
    tag: src.tag,
    tags,
    fetchReport: async (i: number): Promise<Report> => {
      const entry = idx.reports[i];
      if (!entry?.file) throw new Error("no such build in this snapshot");
      const found = (await tarball()).get(entry.file);
      if (!found) throw new Error(`${entry.file} is not in the snapshot`);
      return JSON.parse(new TextDecoder().decode(found)) as Report;
    },
  };
}
