// Browser-side equivalent of the native walker: classify the files of a
// picked Buildroot output directory, read only the ones that matter, and feed
// the WASM core. The File API exposes name and size as metadata, so the
// target tree costs nothing to enumerate; only small inputs and the images in
// images/ are actually read.

import { KIND, TreeScan, carveBytes } from "./wasm";
import { Report } from "./types";

/** A file the browser handed us, with its path relative to the picked root. */
export interface PickedFile {
  /** Path relative to the picked directory, e.g. "images/uImage". */
  path: string;
  file: File;
}

const MAX_TEXT_BYTES = 256 * 1024;

/** Files that must never be treated as firmware artifacts. */
const NON_ARTIFACT = /\.(json|md|txt|sha256sum|sha256|sha1|md5|asc|sig|log|cfg|html)$/i;

export function isArtifactName(name: string): boolean {
  return !NON_ARTIFACT.test(name) && !name.startsWith(".");
}

/** Split "root/rest/of/path" into its first segment and the remainder. */
function splitRoot(p: string): [string, string] {
  const i = p.indexOf("/");
  return i < 0 ? ["", p] : [p.slice(0, i), p.slice(i + 1)];
}

/**
 * Turn a picked directory into a report. `onProgress` reports coarse stages
 * so a large tree does not look frozen.
 *
 * Not reachable from the UI: `webkitdirectory` and dropped-directory
 * recursion both hand over every file beneath the directory before any of
 * this runs, and a Buildroot output tree carries hundreds of thousands of
 * files in build/ and per-package/ that the scan never reads (a real one
 * measured 1,053,413 files, of which it wants about a dozen). Bringing the
 * affordance back needs a directory *handle* (File System Access API) so
 * only target/, images/ and named files are ever visited; this function and
 * its harness (scan-check.html) are what that would drive.
 */
export async function scanPickedTree(
  rootName: string,
  files: PickedFile[],
  onProgress?: (stageKey: string, params?: Record<string, string>) => void
): Promise<Report> {
  const scan = await TreeScan.open();
  try {
    scan.setText(KIND.ROOT, rootName || "build", "(browser)");

    const byPath = new Map<string, File>();
    for (const f of files) byPath.set(f.path, f.file);

    const readText = async (path: string): Promise<string | null> => {
      const f = byPath.get(path);
      if (!f || f.size > MAX_TEXT_BYTES) return null;
      return await f.text();
    };

    onProgress?.("stage_reading_inputs");
    const cfg = await readText(".config");
    if (cfg !== null) scan.setText(KIND.CONFIG, ".config", cfg);
    const pflText = await readText("build/packages-file-list.txt");
    if (pflText !== null) scan.setText(KIND.PFL, "packages-file-list.txt", pflText);
    const btl = byPath.get("build/build-time.log");
    if (btl) scan.setText(KIND.BUILD_TIME_LOG, "build-time.log", await btl.text());
    const etcModules = await readText("target/etc/modules");
    if (etcModules !== null) scan.setText(KIND.ETC_MODULES, "modules", etcModules);

    // Current Buildroot writes usr/lib/os-release and leaves etc/os-release a
    // symlink to it; older trees have only the latter.
    const osRelease =
      (await readText("target/usr/lib/os-release")) ?? (await readText("target/etc/os-release"));
    if (osRelease !== null) scan.setText(KIND.OS_RELEASE, "os-release", osRelease);

    // modules.builtin from the first kernel version directory present.
    const builtinPath = [...byPath.keys()]
      .filter((p) => /^target\/(usr\/)?lib\/modules\/[^/]+\/modules\.builtin$/.test(p))
      .sort()[0];
    if (builtinPath) {
      const t = await readText(builtinPath);
      if (t !== null) scan.setText(KIND.MODULES_BUILTIN, "modules.builtin", t);
    }

    // Environment sources and genimage configs (small text files only).
    for (const [path, f] of byPath) {
      const name = path.split("/").pop() ?? path;
      if (/^genimage.*\.cfg$/i.test(name)) {
        if (f.size <= MAX_TEXT_BYTES) scan.setText(KIND.GENIMAGE, name, await f.text());
        continue;
      }
      const depth = path.split("/").length;
      const inRootOrImages = depth === 1 || path.startsWith("images/");
      if (!inRootOrImages) continue;
      if (!/\.(txt|env|cfg)$/i.test(name) || f.size > MAX_TEXT_BYTES) continue;
      const text = await f.text();
      if (name.toLowerCase().startsWith("uenv") || text.includes("mtdparts=")) {
        scan.setText(KIND.ENV_TEXT, name, text);
      }
    }

    // target/: metadata only. No reads, no inode information (so no hardlink
    // dedup, which the report records via scan_mode).
    onProgress?.("stage_indexing_target");
    const targetRecords: string[] = [];
    const targetPaths = new Set<string>();
    for (const { path, file } of files) {
      if (!path.startsWith("target/")) continue;
      const rel = path.slice("target/".length);
      if (!rel) continue;
      targetPaths.add(rel);
      targetRecords.push(`${file.size}\t0\t${rel}`);
    }
    if (targetRecords.length) scan.addTargets(targetRecords.join("\n"));

    // Installed-but-not-shipped: paths the package list records that are not
    // in the final tree, with install sizes taken from per-package/ when the
    // pick included it.
    if (pflText) {
      const perPackage = new Map<string, number>();
      for (const { path, file } of files) {
        const m = path.match(/^per-package\/([^/]+)\/target\/(.+)$/);
        if (m) perPackage.set(`${m[1]}\0${m[2]}`, file.size);
      }
      const removed: string[] = [];
      for (const line of pflText.split("\n")) {
        const comma = line.indexOf(",");
        if (comma < 0) continue;
        const pkg = line.slice(0, comma);
        let rel = line.slice(comma + 1);
        if (rel.startsWith("./")) rel = rel.slice(2);
        if (!pkg || !rel || targetPaths.has(rel)) continue;
        removed.push(`${perPackage.get(`${pkg}\0${rel}`) ?? 0}\t${pkg}\t${rel}`);
      }
      if (removed.length) scan.addRemoved(removed.join("\n"));
    }

    // images/: read bytes so every format can be introspected.
    onProgress?.("stage_reading_images");
    const images = files
      .filter((f) => {
        const [dir, rest] = splitRoot(f.path);
        return dir === "images" && rest && !rest.includes("/");
      })
      .sort((a, b) => a.path.localeCompare(b.path));
    let newest = 0;
    for (const { path, file } of images) {
      const name = path.slice("images/".length);
      if (name === "buildscope-report.json") continue;
      onProgress?.("stage_reading", { name });
      newest = Math.max(newest, Math.floor(file.lastModified / 1000));
      const bytes = new Uint8Array(await file.arrayBuffer());
      scan.addImage(name, file.size, bytes);
    }
    if (newest) scan.setImagesMtime(newest);

    onProgress?.("stage_analyzing");
    return scan.analyze() as Report;
  } catch (e) {
    scan.abandon();
    throw e;
  }
}


/**
 * The File System Access API, as much of it as this needs. Declared here
 * because it is Chromium-only and not in the DOM types this project builds
 * against.
 */
export interface DirHandle {
  kind: "directory";
  name: string;
  entries(): AsyncIterableIterator<[string, DirHandle | FileHandle]>;
  getFileHandle(name: string): Promise<FileHandle>;
  getDirectoryHandle(name: string): Promise<DirHandle>;
}
export interface FileHandle {
  kind: "file";
  name: string;
  getFile(): Promise<File>;
}

/** True where a build directory can be opened at all. */
export function canPickDirectory(): boolean {
  return typeof (window as unknown as { showDirectoryPicker?: unknown }).showDirectoryPicker
    === "function";
}

export async function pickDirectory(): Promise<DirHandle | null> {
  const w = window as unknown as { showDirectoryPicker(o?: unknown): Promise<DirHandle> };
  try {
    return await w.showDirectoryPicker({ mode: "read", id: "buildscope-build" });
  } catch {
    return null; // the reader cancelled
  }
}

/** Open one file by path without listing any directory along the way. */
async function fileAt(root: DirHandle, path: string): Promise<File | null> {
  const parts = path.split("/");
  const name = parts.pop() as string;
  let dir = root;
  try {
    for (const p of parts) dir = await dir.getDirectoryHandle(p);
    return await (await dir.getFileHandle(name)).getFile();
  } catch {
    return null; // absent, which for every one of these is allowed
  }
}

/** Everything under one subtree, as paths relative to the picked directory. */
async function walk(
  dir: DirHandle,
  prefix: string,
  out: PickedFile[],
  limit: number
): Promise<void> {
  const dirs: [string, DirHandle][] = [];
  const pending: Promise<void>[] = [];
  for await (const [name, h] of dir.entries()) {
    if (out.length + pending.length >= limit) return;
    const path = `${prefix}${name}`;
    if (h.kind === "directory") {
      dirs.push([path, h as DirHandle]);
    } else {
      // Sizes come from the File object; nothing is read here.
      pending.push(
        (h as FileHandle).getFile().then((file) => {
          out.push({ path, file });
        })
      );
    }
  }
  await Promise.all(pending);
  for (const [path, h] of dirs) await walk(h, `${path}/`, out, limit);
}

/**
 * Scan a build directory the reader picked, by handle.
 *
 * This is the affordance `scanPickedTree` describes as needing a directory
 * handle. A Buildroot output tree is over a million entries, almost all of
 * them in build/ and per-package/, and a scan wants about a thousand: the
 * named inputs, target/ and images/. A handle can be navigated to exactly
 * those, so `build/packages-file-list.txt` is opened without build/ ever
 * being listed. The pickers that work in every browser cannot do this --
 * they hand over every file first -- which is why this one is Chromium-only.
 *
 * per-package/ is deliberately not visited: it is the largest thing in the
 * tree by far and feeds one section of the report, which says so instead.
 */
export async function scanDirectoryHandle(
  root: DirHandle,
  onProgress?: (stageKey: string, params?: Record<string, string>) => void
): Promise<Report> {
  /** A guard against a pick that is not a build tree after all. */
  const MAX_ENTRIES = 200_000;
  const files: PickedFile[] = [];

  onProgress?.("stage_reading_inputs");
  for (const path of [
    ".config",
    "build/packages-file-list.txt",
    "build/build-time.log",
  ]) {
    const file = await fileAt(root, path);
    if (file) files.push({ path, file });
  }

  // The root itself is small, and is where the environment sources and any
  // genimage config live.
  for await (const [name, h] of root.entries()) {
    if (h.kind !== "file") continue;
    if (!/\.(txt|env|cfg)$/i.test(name)) continue;
    const file = await (h as FileHandle).getFile();
    files.push({ path: name, file });
  }

  onProgress?.("stage_indexing_target");
  try {
    await walk(await root.getDirectoryHandle("target"), "target/", files, MAX_ENTRIES);
  } catch {
    /* a tree with no target/ still has images to report on */
  }
  onProgress?.("stage_reading_images");
  try {
    await walk(await root.getDirectoryHandle("images"), "images/", files, MAX_ENTRIES);
  } catch {
    /* ...and one with no images/ still has packages */
  }

  const report = await scanPickedTree(root.name, files, onProgress);
  // Say what was skipped rather than leaving a section quietly empty.
  report.scan.warnings = [
    ...report.scan.warnings,
    "scanned by directory handle: per-package/ was not visited, so installed-but-not-shipped is unavailable",
  ];
  return report;
}

/** Analyze a bare firmware image (a release .bin, a flash dump). */
export async function scanArtifact(file: File): Promise<Report> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  return (await carveBytes(file.name, bytes)) as Report;
}

/** Group picked files by their top-level directory (one entry per build). */
export function groupByRoot(files: { path: string; file: File }[]): Map<string, PickedFile[]> {
  const groups = new Map<string, PickedFile[]>();
  for (const f of files) {
    const [root, rest] = splitRoot(f.path);
    const key = root || "";
    if (!rest) continue;
    const list = groups.get(key) ?? [];
    list.push({ path: rest, file: f.file });
    groups.set(key, list);
  }
  return groups;
}

/** True when a picked group looks like a Buildroot output directory. */
export function looksLikeBuild(files: PickedFile[]): boolean {
  let hasConfig = false;
  let hasImages = false;
  let hasTarget = false;
  for (const f of files) {
    if (f.path === ".config") hasConfig = true;
    else if (f.path.startsWith("images/")) hasImages = true;
    else if (f.path.startsWith("target/")) hasTarget = true;
    if (hasConfig && (hasImages || hasTarget)) return true;
  }
  return hasImages;
}

/**
 * Direct children of a dropped directory, without descending. A dropped
 * folder is nearly always a place where images or reports sit side by side
 * (a downloaded release, a scratch dir), and recursing is what made this
 * unusable on build trees.
 */
export async function readDirectoryShallow(
  entry: FileSystemDirectoryEntry
): Promise<{ path: string; file: File }[]> {
  const out: { path: string; file: File }[] = [];
  const reader = entry.createReader();
  for (;;) {
    const batch = await new Promise<FileSystemEntry[]>((resolve, reject) =>
      reader.readEntries(resolve, reject)
    );
    if (batch.length === 0) break;
    for (const e of batch) {
      if (!e.isFile) continue;
      const file = await new Promise<File>((resolve, reject) =>
        (e as FileSystemFileEntry).file(resolve, reject)
      );
      out.push({ path: file.name, file });
    }
  }
  return out;
}

/** Read every file under a dropped directory entry (see scanPickedTree). */
export async function readDirectoryEntry(
  entry: FileSystemDirectoryEntry,
  prefix = ""
): Promise<{ path: string; file: File }[]> {
  const out: { path: string; file: File }[] = [];
  const reader = entry.createReader();
  const batches: FileSystemEntry[] = [];
  for (;;) {
    const batch = await new Promise<FileSystemEntry[]>((resolve, reject) =>
      reader.readEntries(resolve, reject)
    );
    if (batch.length === 0) break;
    batches.push(...batch);
  }
  for (const e of batches) {
    const path = prefix ? `${prefix}/${e.name}` : e.name;
    if (e.isDirectory) {
      out.push(...(await readDirectoryEntry(e as FileSystemDirectoryEntry, path)));
    } else {
      const file = await new Promise<File>((resolve, reject) =>
        (e as FileSystemFileEntry).file(resolve, reject)
      );
      out.push({ path, file });
    }
  }
  return out;
}
