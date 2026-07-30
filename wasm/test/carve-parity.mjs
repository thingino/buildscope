// Parity harness for the artifact path: carve a bare image through the WASM
// module and compare against the native CLI's carve report for the same file.
// Run: node wasm/test/carve-parity.mjs <image.bin> <native-report.json>

import { readFileSync } from "node:fs";
import { basename } from "node:path";

const wasmPath =
  process.env.BUILDSCOPE_WASM ??
  new URL("../../target/wasm32-unknown-unknown/release/buildscope_wasm.wasm", import.meta.url)
    .pathname;

const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
const x = instance.exports;
const enc = new TextEncoder();
const dec = new TextDecoder();

const imagePath = process.argv[2];
const nativePath = process.argv[3];
if (!imagePath || !nativePath) {
  console.error("usage: node carve-parity.mjs <image.bin> <native-report.json>");
  process.exit(2);
}

const bytes = new Uint8Array(readFileSync(imagePath));
const nameBytes = enc.encode(basename(imagePath));

const np = x.bs_alloc(nameBytes.length);
new Uint8Array(x.memory.buffer, np, nameBytes.length).set(nameBytes);
const dp = x.bs_alloc(bytes.length);
new Uint8Array(x.memory.buffer, dp, bytes.length).set(bytes);

const outPtr = x.bs_carve(np, nameBytes.length, dp, bytes.length);
const len = new DataView(x.memory.buffer).getUint32(outPtr, true);
const wasmReport = JSON.parse(dec.decode(new Uint8Array(x.memory.buffer, outPtr + 4, len)));
x.bs_free(outPtr, 4 + len);
x.bs_free(np, nameBytes.length);
x.bs_free(dp, bytes.length);

const native = JSON.parse(readFileSync(nativePath, "utf8"));

let failures = 0;
const check = (label, a, b) => {
  const same = JSON.stringify(a) === JSON.stringify(b);
  if (!same) failures++;
  console.log(
    `${same ? "  ok  " : " FAIL "} ${label}${same ? "" : `\n        native: ${JSON.stringify(a)}\n        wasm:   ${JSON.stringify(b)}`}`
  );
};

console.log(`image: ${basename(imagePath)} (${bytes.length} bytes)\n`);
check("context_source is artifact", "artifact", wasmReport.scan.context_source);
check("scan_mode is browser", "browser", wasmReport.scan.scan_mode);
check("flash", native.flash, wasmReport.flash);
check("images", native.images, wasmReport.images);
check("build", native.build, wasmReport.build);
check("warnings", native.scan.warnings, wasmReport.scan.warnings);

console.log(`\n${failures === 0 ? "CARVE PARITY OK" : `${failures} MISMATCH(ES)`}`);
process.exit(failures === 0 ? 0 : 1);
