// Parity harness: drive the WASM module exactly as the browser does (file
// metadata for target/, bytes only for images/ and the small inputs) and
// compare the resulting report against the native CLI's report for the same
// tree. Run: node wasm/test/parity.mjs <build-dir> <native-report.json>
//
// The two walks legitimately differ in two places, which the harness allows
// and reports: `scan_mode` ("browser" vs "native"), and hardlink accounting,
// since the File API exposes no inode links.

import { readFileSync, readdirSync, statSync, lstatSync } from "node:fs";
import { join, basename } from "node:path";

const KIND = {
  ROOT: 1,
  CONFIG: 2,
  PFL: 3,
  BUILD_TIME_LOG: 4,
  ETC_MODULES: 5,
  MODULES_BUILTIN: 6,
  ENV_TEXT: 7,
  GENIMAGE: 8,
};

const wasmPath =
  process.env.BUILDSCOPE_WASM ??
  new URL("../../target/wasm32-unknown-unknown/release/buildscope_wasm.wasm", import.meta.url)
    .pathname;

const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
const x = instance.exports;

const enc = new TextEncoder();
const dec = new TextDecoder();

function put(bytes) {
  if (bytes.length === 0) return [0, 0];
  const ptr = x.bs_alloc(bytes.length);
  new Uint8Array(x.memory.buffer, ptr, bytes.length).set(bytes);
  return [ptr, bytes.length];
}

function putText(s) {
  return put(enc.encode(s));
}

function take(ptr) {
  const len = new DataView(x.memory.buffer).getUint32(ptr, true);
  const json = dec.decode(new Uint8Array(x.memory.buffer, ptr + 4, len));
  x.bs_free(ptr, 4 + len);
  return JSON.parse(json);
}

function setText(h, kind, name, text) {
  const [np, nl] = putText(name);
  const [tp, tl] = putText(text);
  x.bs_set_text(h, kind, np, nl, tp, tl);
  x.bs_free(np, nl);
  x.bs_free(tp, tl);
}

function readIfPresent(p) {
  try {
    return readFileSync(p, "utf8");
  } catch {
    return null;
  }
}

// Walk target/ collecting only what the File API would expose: path and size
// (plus symlink-ness, which node knows and the browser infers as a plain file).
function walkTarget(root) {
  const out = [];
  const stack = [root];
  while (stack.length) {
    const dir = stack.pop();
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const e of entries) {
      const full = join(dir, e.name);
      if (e.isDirectory()) {
        stack.push(full);
        continue;
      }
      if (!e.isFile() && !e.isSymbolicLink()) continue;
      let st;
      try {
        st = lstatSync(full);
      } catch {
        continue;
      }
      out.push({
        path: full.slice(root.length + 1),
        size: st.size,
        symlink: e.isSymbolicLink(),
        nlink: st.nlink,
      });
    }
  }
  return out;
}

const buildDir = process.argv[2];
const nativePath = process.argv[3];
if (!buildDir || !nativePath) {
  console.error("usage: node parity.mjs <build-dir> <native-report.json>");
  process.exit(2);
}

const h = x.bs_new();
setText(h, KIND.ROOT, basename(buildDir), buildDir);

const cfg = readIfPresent(join(buildDir, ".config"));
if (cfg) setText(h, KIND.CONFIG, ".config", cfg);
const pfl = readIfPresent(join(buildDir, "build/packages-file-list.txt"));
if (pfl) setText(h, KIND.PFL, "packages-file-list.txt", pfl);
const btl = readIfPresent(join(buildDir, "build/build-time.log"));
if (btl) setText(h, KIND.BUILD_TIME_LOG, "build-time.log", btl);
const etcModules = readIfPresent(join(buildDir, "target/etc/modules"));
if (etcModules) setText(h, KIND.ETC_MODULES, "modules", etcModules);
const uenv = readIfPresent(join(buildDir, "uenv.txt"));
if (uenv) setText(h, KIND.ENV_TEXT, "uenv.txt", uenv);

// modules.builtin from the first kernel version directory present.
for (const base of ["target/lib/modules", "target/usr/lib/modules"]) {
  let vers = [];
  try {
    vers = readdirSync(join(buildDir, base), { withFileTypes: true })
      .filter((d) => d.isDirectory())
      .map((d) => d.name)
      .sort();
  } catch {
    continue;
  }
  if (vers.length) {
    const mb = readIfPresent(join(buildDir, base, vers[0], "modules.builtin"));
    if (mb) setText(h, KIND.MODULES_BUILTIN, "modules.builtin", mb);
    break;
  }
}

// target/ metadata. Browser mode cannot dedup hardlinks; count how many
// entries the native walk would have discounted so the diff is explainable.
const targetRoot = join(buildDir, "target");
const entries = walkTarget(targetRoot);
const hardlinked = entries.filter((e) => !e.symlink && e.nlink > 1);
const blob = entries.map((e) => `${e.size}\t${e.symlink ? 1 : 0}\t${e.path}`).join("\n");
const [bp, bl] = putText(blob);
const added = x.bs_add_targets(h, bp, bl);
x.bs_free(bp, bl);

// images/: read bytes, exactly like a browser reading the picked files.
const imagesDir = join(buildDir, "images");
let newest = 0;
for (const name of readdirSync(imagesDir).sort()) {
  if (name === "buildscope-report.json") continue;
  const full = join(imagesDir, name);
  const st = statSync(full);
  if (!st.isFile()) continue;
  newest = Math.max(newest, Math.floor(st.mtimeMs / 1000));
  const bytes = new Uint8Array(readFileSync(full));
  const [np, nl] = putText(name);
  const [dp, dl] = put(bytes);
  x.bs_add_image(h, np, nl, BigInt(st.size), dp, dl);
  x.bs_free(np, nl);
  x.bs_free(dp, dl);
}
if (newest) x.bs_set_images_mtime(h, BigInt(newest));

const wasmReport = take(x.bs_analyze(h));
x.bs_drop(h);

const native = JSON.parse(readFileSync(nativePath, "utf8"));

// ---- comparison ----
let failures = 0;
const check = (label, a, b) => {
  const same = JSON.stringify(a) === JSON.stringify(b);
  if (!same) failures++;
  console.log(`${same ? "  ok  " : " FAIL "} ${label}${same ? "" : `\n        native: ${JSON.stringify(a)}\n        wasm:   ${JSON.stringify(b)}`}`);
};

console.log(`wasm module: ${wasmPath}`);
console.log(`target entries fed: ${added} (${hardlinked.length} hardlinked, not dedupable in browser mode)\n`);

check("schema", native.schema, wasmReport.schema);
check("scan_mode is browser", "browser", wasmReport.scan.scan_mode);
check("flash", native.flash, wasmReport.flash);
check("images", native.images, wasmReport.images);
check("rootfs", native.rootfs, wasmReport.rootfs);
check("packages", native.packages, wasmReport.packages);
check("modules", native.modules, wasmReport.modules);
check("modules_meta", native.modules_meta, wasmReport.modules_meta);
check("timings", native.timings, wasmReport.timings);
check("removed_not_shipped (native only: needs per-package/)", native.removed_not_shipped.length > 0, true);
check(
  "build (minus name/paths)",
  { ...native.build, name: null },
  { ...wasmReport.build, name: null }
);

console.log(`\n${failures === 0 ? "PARITY OK" : `${failures} MISMATCH(ES)`}`);
process.exit(failures === 0 ? 0 : 1);
