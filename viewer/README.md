# buildscope viewer

Static single-page viewer for buildscope reports. React + Vite, `react` and
`react-dom` as the only runtime dependencies; the treemap is hand-rolled. Dark
thingino-family theme, and the chart colors are a CVD-validated palette.

## Where the data comes from

1. **Inlined**: `buildscope export` writes one self-contained HTML file with
   the data baked in as `window.__BUILDSCOPE_REPORT__` -- one report, or an
   array of them, which is what gives a local file a build picker and a drift
   comparison with nothing running.
2. **Fetched**: `buildscope export --site` writes this bundle plus `api/index`
   and one `api/report/<n>` per build, as plain files. The app looks for that
   index on load and, finding it, fetches a report only when it is opened.
   Any static host will do; browsers block `fetch` over `file://`, so this
   form needs to be served rather than opened.
3. **A fleet snapshot**: `?fleet=<tag-or-url>` reads the pair
   `buildscope export --fleet` writes. The index is small and loads up front,
   so a fleet of any size paints its overview from a few hundred bytes per
   build; the tarball is not fetched until a build is opened, and is then
   decompressed once with the browser's own `DecompressionStream` and held
   unparsed, so only the report being read costs anything to turn into
   objects. Browsing 166 builds and opening two of them costs two requests.
4. **Carved in the browser**: with none of those the page becomes a drop
   target backed by the WASM build of the analysis core (`buildscope.wasm`).
   Drop a firmware image, a folder of images, or a `buildscope-report.json`.
   Nothing is uploaded.

Picking a *build directory* is offered where the browser can do it without
melting. `webkitdirectory` and dropped-directory recursion both hand over every
file beneath the directory before any code can filter, and an output tree is
mostly `build/` and `per-package/` -- a real one measures 1,158,991 entries, of
which a scan wants about 1,090. So the affordance uses `showDirectoryPicker()`
instead: a directory *handle* can be navigated, so `build/packages-file-list.txt`
is opened by name without `build/` ever being listed, and only `target/` and
`images/` are walked. That API is Chromium-only, so the button is absent
elsewhere rather than present and broken.

`per-package/` is not visited -- it is 804,325 entries on its own and feeds one
section of the report, which says so rather than appearing empty.

## The fleet view

Where a snapshot holds many builds, the way in is a list of all of them rather
than one of them: name, branch, flash and rootfs size, and how full the fullest
partition is. Sorted by name, because at fleet scale the first question is
usually where a known device is, and an order that shifts between runs is hard
to navigate; fill is one click on its column.

A second view maps them. One row per device on a single absolute scale, so an
8 MiB chip is half the bar of a 16 MiB one rather than being stretched to match
it, each partition a fixed hue -- faint for its extent, solid for what is used
-- so a layout that differs from its neighbours shows up as a step in the
stack. Segments like `env`, 64 KiB on a 16 MiB chip, are 0.4% of a bar and
unreadable as geometry, so the sizes are available as aligned columns beside
it, sortable per partition. Both come from the index, so neither costs a
report.

## The Flash tab

The map and partition table are joined by a UBI volumes table whenever an image
is a UBI area. Volumes take part in the flash map like any other partition, so
the extra table exists for what a partition row cannot say: the space the volume
table reserved as against what was written, the per-block header cost, and
volumes that were reserved but never written and therefore have no place on the
map at all.

## The Environment tab

A U-Boot environment partition's variables, with a filter over both names and
values. It is the board's own configuration -- `mtdparts` is where the partition
table came from, `bootcmd` says which partition boots and how -- but it is
reference material, thirty rows with some values eight lines long, so it has a
tab rather than sitting under the flash map. What the map needs from it, the
source its layout came from, the map already states in its own header. Values
are shown in full and wrap rather than truncate, because the interesting part of
a `bootcmd` is usually its tail. The tab appears only for a report that has one.

## The Device tree tab

Every device tree the firmware carries, and the whole of the selected one as
source. A board that boots from raw flash usually ships none of these as a
file, so this is the only place they can be collected: the bootloader keeps
its tree appended to its binary, and the kernel carries one inside itself
behind the kernel's own compression. The table says which board each is for
and what it costs; picking a row shows its nodes and properties, rendered the
way `dtc` prints them, with a filter over node paths, property names and
values. The tab appears only for a report that has one.

## The Kernel tab

Which version, what was compiled into it, what ships beside it as loadable
modules, and the options that decided all three. They belong together because
they answer each other -- a driver is built in because a `CONFIG_` line said
`y`, and is a `.ko` because it said `m` -- and because which of them exist
depends on what was analyzed rather than on the subject: a build tree brings
the modules and the built-in list, a bare image brings the config out of the
kernel itself via `CONFIG_IKCONFIG`, and either alone is worth showing.

The two long lists sit behind the counts that raise the question about them,
so the default view stays short. Disabled options are kept as `n` and hidden
until asked for: that an option was considered and turned off is worth being
able to find, but it is most of the file.

## The Files tab

Browses a listing as a collapsible directory tree with per-directory totals,
the owning package per file, and a path filter. Two kinds of source feed it:
the rootfs walk attributed to packages (present whenever a build tree was
scanned), and any image that reconstructed its own contents -- jffs2, cpio,
UBIFS and squashfs all do. Where the cost of a file on flash is known, as it is
for squashfs, the tree totals it per directory alongside the uncompressed size. Directories are ordered before files and both by size, so the heavy
paths surface first.

## Languages

The interface is translated into the same 15 languages as the other thingino
web apps, using the same conventions: browser detection with a `lang`
localStorage override, native language names in the picker (behind the gear),
and `dir="rtl"` for right-to-left languages. `?lang=de` opens a link in one
language without changing the reader's saved preference.

`src/locales/en.ts` is the source of truth for the key list. To add or fix a
language, edit `src/locales/<lang>.ts`; missing keys fall back to English, so a
partial translation is safe to ship. Only English is bundled eagerly, the rest
load on demand.

Deliberately **not** translated, because they are the same words everywhere and
the analysis core emits them for the CLI too: package and partition names,
image formats, unit symbols, and the diagnostic warnings inside a report.

Layout mirrors itself under RTL through CSS logical properties. Three things
are forced back to left-to-right in every language, because bidi reordering
mangles them otherwise: measurements (`320.0 KiB` becomes `KiB 320.0`), the
core's English diagnostics, and machine text such as paths and hex offsets.
The flash map and treemap also stay left-to-right: a flash address space runs
low to high left to right, and a treemap is fixed geometry.

## Develop / build

```
npm install
npm run wasm            # copy the WASM core into public/ (build it first, below)
npm run dev             # dev server
npm run check:locales   # key, placeholder and markup parity against en.ts
npm run build           # copies WASM, checks locales, type-checks, writes dist/
```

The WASM core comes from the workspace root:

```
cargo build --release --target wasm32-unknown-unknown -p buildscope-wasm
```

`buildscope export` picks up the bundle from `viewer/dist`, from beside the
binary, or via `--viewer-dir`.

## Checks

* `node ../wasm/test/parity.mjs <build-dir> <native-report.json>` proves the
  WASM core returns what the native CLI returns for the same tree.
* `node ../wasm/test/carve-parity.mjs <image.bin> <native-report.json>` does
  the same for the artifact path.
* `npm run dev`, then open `/scan-check.html`: exercises the browser-side
  classification in `src/scan.ts` end to end against a synthetic output tree
  built in the page.
* `npm run check:locales` runs in `npm run build`, so a locale that drifts
  from `en.ts` fails the build and therefore CI.
* `npm run check:render -- <report.json>...` renders the Flash and Environment
  panes to static HTML with react-dom and asserts that what the report contains actually reaches
  the markup: every environment variable with its value intact, every UBI volume
  including ones with nothing written, every partition row. Assertions come from
  the report, so any report works and absent sections are skipped. This is the
  component-level check that needs no browser.
* `npm run check:layout -- --out shots <exported.html>...` opens files written
  by `buildscope export` in headless Chromium at 1280px and 412px, walks every
  tab, and fails on what only a layout engine knows: a page that scrolls
  sideways, content wider than the table cell holding it, a row label sunk to
  the bottom of its own tall row, or a runtime error. Screenshots of every tab
  at both widths are written to `--out`. Playwright is not a dependency here, so
  the check skips cleanly when it is missing; set `BUILDSCOPE_PLAYWRIGHT` to an
  install elsewhere. Both real bugs it has caught so far were invisible to the
  static checks: a value's inline-block baseline dragging its own name eight
  lines down the row, and a long unbreakable kernel version widening the entire
  page past the viewport.
