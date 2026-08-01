import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The workspace manifest is the one version this project has: it is what the
// CLI stamps into `generator.version`, and the footer has to agree with that
// or a report made by the matching CLI looks like it came from a different
// build. package.json carried its own copy and they drifted at the first
// release that only bumped one.
// Anchored to the section rather than to the first `version =` in the file,
// so a dependency pinned above it cannot become the version on the page.
const cargoToml = readFileSync(new URL("../Cargo.toml", import.meta.url), "utf8");
const version = cargoToml
  .split(/^\[/m)
  .find((section) => section.startsWith("workspace.package]"))
  ?.match(/^\s*version\s*=\s*"([^"]+)"/m)?.[1];
if (!version) {
  throw new Error("no version under [workspace.package] in Cargo.toml");
}

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
