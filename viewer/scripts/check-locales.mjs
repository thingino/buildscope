// Validate every locale against src/locales/en.ts, which is the source of
// truth for the key list. Catches the mistakes that are invisible until a
// user switches language: a missing key, a stray key that no longer exists,
// a mangled {placeholder}, lost markup, or a string long enough to break a
// table header.
//
// Run: node scripts/check-locales.mjs

import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const dir = join(here, "..", "src", "locales");

// Values are plain string literals, so the dictionaries can be read without
// a TypeScript toolchain: pull `key: "value"` pairs out of the source.
//
// The value may sit on the line after the key, which is how a formatter wraps
// the long ones. Those are exactly the strings that carry {placeholders} and
// <code> markup, so a line-anchored pattern would skip the entries most worth
// checking; allow an optional newline and indent between the two.
function parseLocale(file) {
  const src = readFileSync(join(dir, file), "utf8");
  const out = {};
  const re = /^ {2}("?)([A-Za-z0-9_]+)\1:[ \t]*(?:\r?\n[ \t]*)?("(?:[^"\\]|\\.)*")[ \t]*,/gm;
  let m;
  while ((m = re.exec(src)) !== null) {
    out[m[2]] = JSON.parse(m[3]);
  }
  return out;
}

const placeholders = (s) => [...s.matchAll(/\{([a-z_]+)\}/g)].map((m) => m[1]).sort();
const tags = (s) => [...s.matchAll(/<\/?([a-z]+)>/g)].map((m) => m[0]).sort();

const en = parseLocale("en.ts");
const enKeys = Object.keys(en);
if (enKeys.length === 0) {
  console.error("check-locales: could not parse en.ts");
  process.exit(1);
}

const files = readdirSync(dir)
  .filter((f) => f.endsWith(".ts") && f !== "en.ts")
  .sort();

let errors = 0;
let warnings = 0;
console.log(`en.ts: ${enKeys.length} keys, ${files.length} other locales\n`);

for (const file of files) {
  const dict = parseLocale(file);
  const keys = Object.keys(dict);
  const problems = [];
  const notes = [];

  const missing = enKeys.filter((k) => !(k in dict));
  const extra = keys.filter((k) => !(k in en));
  if (missing.length) problems.push(`missing ${missing.length}: ${missing.slice(0, 6).join(", ")}`);
  if (extra.length) problems.push(`unknown ${extra.length}: ${extra.slice(0, 6).join(", ")}`);

  for (const k of keys) {
    if (!(k in en)) continue;
    const a = en[k];
    const b = dict[k];
    if (b.trim() === "") {
      problems.push(`${k}: empty`);
      continue;
    }
    const pa = placeholders(a).join(",");
    const pb = placeholders(b).join(",");
    if (pa !== pb) problems.push(`${k}: placeholders {${pb}} should be {${pa}}`);
    const ta = tags(a).join("");
    const tb = tags(b).join("");
    if (ta !== tb) problems.push(`${k}: markup "${tb}" should be "${ta}"`);
    // Latin scripts run longer than English; CJK runs much shorter. Only flag
    // the extreme cases, which are the ones that break a column.
    if (a.length >= 4 && b.length > a.length * 2.5 && b.length - a.length > 12) {
      notes.push(`${k}: ${b.length} chars vs ${a.length} in English`);
    }
  }

  const lang = file.replace(/\.ts$/, "");
  if (problems.length) {
    errors++;
    console.log(`FAIL ${lang}`);
    for (const p of problems) console.log(`       ${p}`);
  } else {
    console.log(`ok   ${lang}  (${keys.length} keys)`);
  }
  for (const n of notes) {
    warnings++;
    console.log(`WARN ${lang}  ${n}`);
  }
}

console.log(
  `\n${errors === 0 ? "all locales valid" : `${errors} locale(s) with problems`}` +
    (warnings ? `, ${warnings} length warning(s)` : "")
);
process.exit(errors === 0 ? 0 : 1);
