import { readFileSync } from "node:fs";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const { version } = JSON.parse(readFileSync(new URL("package.json", import.meta.url), "utf8"));

// base "./" so the bundle works from `buildscope serve`, a file:// preview,
// or any static-hosting subpath.
export default defineConfig({
  base: "./",
  define: { __APP_VERSION__: JSON.stringify(version) },
  plugins: [react()],
  build: { chunkSizeWarningLimit: 700 },
});
