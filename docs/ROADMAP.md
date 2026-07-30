# Roadmap

## Phase 1 (current)

- Core parsers: squashfs superblock, jffs2 node scan, uImage header, U-Boot
  env, mtdparts, MBR, padding detection
- Buildroot inputs: packages-file-list.txt, .config, build-time.log, modules
- `scan` with terminal summary and report file
- Hooked mode via BR2_ROOTFS_POST_IMAGE_SCRIPT
- Web viewer: overview (partition bars, images), packages (table + treemap),
  modules, build timings; `serve` command

## Phase 2

- Drift view: compare any two reports (package-level and partition-level)
- Single-file HTML export (report inlined, no server)
- Installed-vs-shipped panel: diff packages-file-list.txt against the final
  target tree, recovering pre-removal sizes from per-package/ where present
- genimage.cfg as an additional partition-layout detector
- Artifact-only mode: carve a bare composite flash image using an embedded
  environment block and magics (no package attribution possible in this mode)

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
