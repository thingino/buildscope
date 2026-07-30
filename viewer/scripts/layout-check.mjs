// Load real reports in a real browser at a desktop and a phone width, walk
// every tab, and fail on the things that only a layout engine can tell you:
// a page that scrolls sideways, content wider than the cell holding it, or a
// runtime error. Screenshots of each tab are written out for eyeballing.
//
//   node scripts/layout-check.mjs [--out DIR] <exported-report.html>...
//
// Inputs are files written by `buildscope export`, which carry their own data,
// so no server is needed. Playwright is not a dependency of this project: if it
// is not installed the check skips rather than failing, since it is a local
// tool and not part of the build. Point BUILDSCOPE_PLAYWRIGHT at a playwright
// entry point to use an install from elsewhere (an `npx playwright` cache, say).
import { mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { createRequire } from "node:module";

const args = process.argv.slice(2);
let outDir = "layout-shots";
const files = [];
for (let i = 0; i < args.length; i++) {
  if (args[i] === "--out") outDir = args[++i];
  else files.push(args[i]);
}
if (files.length === 0) {
  console.error("usage: layout-check.mjs [--out DIR] <exported-report.html>...");
  process.exit(2);
}

async function loadPlaywright() {
  const override = process.env.BUILDSCOPE_PLAYWRIGHT;
  if (override) {
    const mod = await import(override);
    return mod.chromium ?? mod.default?.chromium;
  }
  try {
    const require = createRequire(import.meta.url);
    const mod = await import(require.resolve("playwright"));
    return mod.chromium ?? mod.default?.chromium;
  } catch {
    return null;
  }
}

const chromium = await loadPlaywright();
if (!chromium) {
  console.log("layout-check: playwright not installed, skipping.");
  console.log("  npm i -D playwright && npx playwright install chromium");
  console.log("  (or set BUILDSCOPE_PLAYWRIGHT=/path/to/playwright/index.js)");
  process.exit(0);
}

const VIEWS = [
  { name: "desktop", width: 1280, height: 1400 },
  // The narrowest width the interface is expected to hold up at.
  { name: "phone", width: 412, height: 900 },
];

mkdirSync(outDir, { recursive: true });
const browser = await chromium.launch({ headless: true });
let problems = 0;

const fail = (where, msg) => {
  console.log(`  FAIL ${where}: ${msg}`);
  problems++;
};

for (const file of files) {
  const stem = file.split("/").pop().replace(/\.html?$/, "");
  for (const view of VIEWS) {
    const ctx = await browser.newContext({
      viewport: { width: view.width, height: view.height },
      deviceScaleFactor: 2,
    });
    const page = await ctx.newPage();
    const errors = [];
    // Looking for a local API and not finding one is how the page decides it is
    // running standalone, so that 404 is a result, not a fault.
    const expected = (s) => s.includes("/api/index");
    page.on("pageerror", (e) => errors.push(e.message));
    page.on(
      "console",
      (m) => m.type() === "error" && !expected(m.text()) && errors.push(m.text())
    );

    await page.goto(`file://${resolve(file)}`);
    await page.waitForSelector(".tabs .tab", { timeout: 20000 });

    const tabs = await page.$$eval(".tabs .tab", (els) => els.map((e) => e.textContent.trim()));
    for (const tabName of tabs) {
      await page.click(`.tabs .tab:text-is("${tabName}")`);
      await page.waitForTimeout(120); // let a treemap measure itself

      // 1. The page must never scroll sideways.
      const over = await page.evaluate(
        () =>
          Math.max(
            document.documentElement.scrollWidth - document.documentElement.clientWidth,
            document.body.scrollWidth - document.body.clientWidth
          )
      );
      if (over > 1) fail(`${stem}/${view.name}/${tabName}`, `page scrolls sideways by ${over}px`);

      // 2. Nothing may be wider than the table cell holding it. Wide tables are
      //    allowed to scroll inside .tbl-wrap; their contents are not allowed to
      //    spill out of a cell.
      const spills = await page.evaluate(() => {
        const out = [];
        for (const cell of document.querySelectorAll(".tbl td")) {
          const cw = cell.getBoundingClientRect().width;
          for (const child of cell.children) {
            const w = child.getBoundingClientRect().width;
            if (w > cw + 1) {
              out.push({ text: (child.textContent ?? "").slice(0, 40), by: Math.round(w - cw) });
            }
          }
        }
        return out.slice(0, 5);
      });
      for (const s of spills) {
        fail(`${stem}/${view.name}/${tabName}`, `content overflows its cell by ${s.by}px: "${s.text}"`);
      }

      // 3. A row whose label sinks away from the top of its own tall cell is a
      //    baseline bug, not a wrap: check the first cell's text starts near the
      //    top of the row.
      const sunk = await page.evaluate(() => {
        const out = [];
        for (const tr of document.querySelectorAll(".tbl tbody tr")) {
          const r = tr.getBoundingClientRect();
          if (r.height < 60) continue; // only tall rows can show the problem
          const first = tr.querySelector("td");
          const span = first?.querySelector("*") ?? first;
          if (!span) continue;
          const offset = span.getBoundingClientRect().top - r.top;
          if (offset > r.height / 2) {
            out.push({ text: (first.textContent ?? "").slice(0, 24), offset: Math.round(offset) });
          }
        }
        return out.slice(0, 5);
      });
      for (const s of sunk) {
        fail(
          `${stem}/${view.name}/${tabName}`,
          `row label sits ${s.offset}px below the top of its row: "${s.text}"`
        );
      }

      await page.screenshot({
        path: `${outDir}/${stem}-${view.name}-${tabName.toLowerCase().replace(/\W+/g, "-")}.png`,
        fullPage: true,
      });
    }

    if (errors.length) fail(`${stem}/${view.name}`, `page errors: ${errors.slice(0, 3).join(" | ")}`);
    console.log(`  ${stem}/${view.name}: ${tabs.length} tabs checked (${tabs.join(", ")})`);
    await ctx.close();
  }
}

await browser.close();
console.log(
  problems === 0 ? `\nLAYOUT OK (screenshots in ${outDir}/)` : `\n${problems} layout problem(s)`
);
process.exit(problems === 0 ? 0 : 1);
