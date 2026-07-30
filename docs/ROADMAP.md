# Roadmap

## Phase 1 (done)

- Core parsers: squashfs superblock, jffs2 node scan, uImage header, U-Boot
  env, mtdparts, MBR, padding detection
- Buildroot inputs: packages-file-list.txt, .config, build-time.log, modules
- `scan` with a terminal summary and a report written into images/
- Hooked mode via BR2_ROOTFS_POST_IMAGE_SCRIPT
- Web viewer: overview (partition bars, images), packages (table + treemap),
  modules, build timings; `serve` command

## Phase 2 (done)

- Drift: `buildscope diff` and the viewer Drift tab (partition, package,
  image, module deltas; added/removed tracking)
- Single-file HTML export: `buildscope export` (viewer + data in one file)
- Installed-vs-shipped: per-package/ source-size recovery, generic
  target-finalize removals filtered
- genimage.cfg partition-layout detector with per-partition image hints

## Phase 2b (done)

- Artifact-only mode: `buildscope carve` recovers the layout from an embedded
  CRC-valid U-Boot environment block (or an MBR), carves and identifies every
  partition, and flags truncated images. `diff`, `export`, `scan` and `serve`
  all accept bare artifacts and directories of them.

## Phase 3a (done)

- `buildscope-wasm`: the analysis core compiled to WebAssembly behind a plain
  C ABI (byte buffers and integers only, no binding generator)
- Browser-side scanning: drop a Buildroot output directory for the full
  breakdown, or a bare firmware image to carve. The File API supplies names
  and sizes as metadata, so only the small build inputs and the files in
  `images/` are read; nothing is uploaded
- Parity proven against the native CLI for both the tree path and the
  artifact path (`wasm/test/*.mjs`), plus an in-browser harness for the
  classification layer (`viewer/scan-check.html`)

## Phase 3b (deployed)

- Live at [buildscope.thingino.com](https://buildscope.thingino.com): the
  browser scanner, published by `.github/workflows/deploy.yml` on every push
  to `main`. The workflow builds the WASM core and viewer from source, gates
  on the test suite, and refuses to publish a bundle whose scanner does not
  instantiate with the expected exports and report schema.

## Phase 3c

- Render CI-published reports on the same site, so a project's build history
  is browsable without scanning anything locally
- CI aggregation recipe: publish one report per build, retain history,
  trends over time
- Fleet-scale viewer work: search in the build picker, and an overview table
  across builds (which are nearest their partition limits)

## Distant / recorded, not planned

- Per-file true compressed cost via squashfs block/fragment mapping instead of
  ratio approximation
- Deep in-image file listing for artifact-only mode (full squashfs/jffs2
  directory reconstruction)
- FIT image parsing (needs a minimal DTB reader)
- ext4/ubifs deep stats beyond superblock level
