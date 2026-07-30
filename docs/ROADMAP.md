# Roadmap

## Phase 1 (done)

- Core parsers: squashfs superblock, jffs2 node scan, uImage header, U-Boot
  env, mtdparts, MBR, padding detection
- Buildroot inputs: packages-file-list.txt, .config, build-time.log, modules
- `scan` with terminal summary and report file
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

## Phase 3

- WASM build of the core; browser-side scanning of a dropped output directory
  (File API supplies names and sizes as metadata; only small inputs and the
  images are actually read)
- Static deployment (GitHub Pages) usable in two modes: drag-and-drop local
  scanning, and rendering CI-published report.json files
- CI aggregation recipe: publish one report per build, retain history, trends
  over time

## Distant / recorded, not planned

- Per-file true compressed cost via squashfs block/fragment mapping instead of
  ratio approximation
- Deep in-image file listing for artifact-only mode (full squashfs/jffs2
  directory reconstruction)
- FIT image parsing (needs a minimal DTB reader)
- ext4/ubifs deep stats beyond superblock level
