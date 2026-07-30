// Render the Flash tab to static HTML against real reports, so the panels that
// display parsed image structure are exercised on real data without needing a
// browser. Bundles the components with the esbuild that ships inside vite and
// renders them with react-dom.
//
//   node scripts/render-check.mjs <report.json>...
//
// Every assertion is derived from the report itself rather than hardcoded, so
// the same run works for a NOR build, a NAND build and a carved image, and a
// report with no environment or no UBI area simply skips those checks.
import { build } from "esbuild";
import { writeFileSync, readFileSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const VIEWER = fileURLToPath(new URL("..", import.meta.url));
const reports = process.argv.slice(2);
if (reports.length === 0) {
  console.error("usage: render-check.mjs <report.json>...");
  process.exit(2);
}

const dir = mkdtempSync(join(tmpdir(), "bs-render-"));
const entry = join(dir, "entry.tsx");
writeFileSync(
  entry,
  `import { renderToStaticMarkup } from "react-dom/server.browser";
   import Flash from "${join(VIEWER, "src/components/Flash")}";
   export function render(report: any): string {
     return renderToStaticMarkup(<Flash report={report} />);
   }`
);

const out = join(dir, "bundle.mjs");
await build({
  entryPoints: [entry],
  bundle: true,
  format: "esm",
  platform: "node",
  outfile: out,
  jsx: "automatic",
  logLevel: "error",
  absWorkingDir: VIEWER,
  // the entry lives outside the project, so point resolution at its deps
  nodePaths: [join(VIEWER, "node_modules")],
  loader: { ".css": "empty" },
});

const { render } = await import(out);

let failures = 0;
const check = (cond, label) => {
  console.log(`  ${cond ? "ok  " : "FAIL"} ${label}`);
  if (!cond) failures++;
};

for (const path of reports) {
  const report = JSON.parse(readFileSync(path, "utf8"));
  console.log(`\n${path.split("/").pop()}  (${report.build.name})`);
  let html;
  try {
    html = render(report);
  } catch (e) {
    console.log(`  FAIL render threw: ${e.message}`);
    failures++;
    continue;
  }

  const env = report.images.find((i) => i.format === "uboot-env");
  if (env) {
    const vars = env.detail.vars ?? [];
    check(html.includes("U-Boot environment"), "environment panel present");
    check(html.includes(`${env.detail.var_count} variables`), "variable count in the head");
    check(html.includes("crc ok") || html.includes("crc BAD"), "crc state shown");
    // Every variable must reach the DOM, keys and values alike.
    const missingKeys = vars.filter((v) => !html.includes(`>${v.key}<`));
    check(missingKeys.length === 0, `all ${vars.length} keys rendered` +
      (missingKeys.length ? ` (missing ${missingKeys.slice(0, 3).map((v) => v.key)})` : ""));
    const esc = (s) =>
      s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
    const missingVals = vars.filter((v) => v.value !== "" && !html.includes(esc(v.value)));
    check(missingVals.length === 0, "all values rendered verbatim" +
      (missingVals.length ? ` (missing ${missingVals.slice(0, 2).map((v) => v.key)})` : ""));
    const longest = vars.reduce((a, b) => (b.bytes > a.bytes ? b : a), vars[0]);
    check(html.includes(esc(longest.value)), `longest value intact (${longest.key}, ${longest.bytes} B)`);
    check(!html.includes("withheld"), "nothing withheld");
  }

  const ubi = report.images.find((i) => i.format === "ubi");
  if (ubi) {
    check(html.includes("UBI volumes"), "UBI volumes panel present");
    for (const v of ubi.detail.volumes) {
      check(html.includes(`>${v.name}<`) || html.includes(v.name), `volume ${v.name} listed`);
    }
    const unwritten = ubi.detail.volumes.filter((v) => v.offset === null);
    if (unwritten.length) {
      check(html.includes("nothing written"), "unwritten volume marked");
    }
    check(html.includes("autoresize") === ubi.detail.volumes.some((v) => v.autoresize),
      "autoresize chip matches the data");
  }

  // The flash map and partition table must still be there.
  check(html.includes("Flash map"), "flash map present");
  for (const p of report.flash?.partitions ?? []) {
    check(html.includes(p.name), `partition ${p.name} in the table`);
  }
}

console.log(failures === 0 ? "\nRENDER CHECK OK" : `\nRENDER CHECK: ${failures} failure(s)`);
process.exit(failures === 0 ? 0 : 1);
