import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const { version } = JSON.parse(readFileSync(new URL("package.json", import.meta.url), "utf8"));

// Short commit for the footer, in the same v<version>-<sha> form the other
// thingino web apps use. "dev" when built outside a git checkout.
let gitSha = "dev";
try {
  gitSha = execSync("git rev-parse --short HEAD", { encoding: "utf8" }).trim();
} catch {
  /* no git available: keep "dev" */
}

// base "./" so the bundle works from `buildscope serve`, a file:// preview,
// or any static-hosting subpath.
export default defineConfig({
  base: "./",
  define: {
    __APP_VERSION__: JSON.stringify(version),
    __GIT_SHA__: JSON.stringify(gitSha),
  },
  plugins: [react()],
  build: { chunkSizeWarningLimit: 700 },
});
