// Copy the built WASM core next to the viewer so the page can fetch it
// same-origin. A missing module is not fatal: the viewer still renders
// reports, it just cannot scan locally.
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";

const src = new URL(
  "../../target/wasm32-unknown-unknown/release/buildscope_wasm.wasm",
  import.meta.url
).pathname;
const dest = new URL("../public/buildscope.wasm", import.meta.url).pathname;

if (!existsSync(src)) {
  console.warn(
    "copy-wasm: no WASM build found; run\n" +
      "  cargo build --release --target wasm32-unknown-unknown -p buildscope-wasm"
  );
  process.exit(0);
}
mkdirSync(dirname(dest), { recursive: true });
copyFileSync(src, dest);
console.log(`copy-wasm: ${dest}`);
