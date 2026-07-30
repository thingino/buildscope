import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// base "./" so the bundle works from `buildscope serve`, a file:// preview,
// or any static-hosting subpath.
export default defineConfig({
  base: "./",
  plugins: [react()],
  build: { chunkSizeWarningLimit: 700 },
});
