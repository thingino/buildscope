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

## Phase 4 (done)

- Full per-package file lists in the report (capped, with a truncation flag),
  and a Files tab that browses the rootfs as a directory tree with sizes and
  owning packages
- jffs2 listings rebuilt from dirent and inode nodes, so the contents of a data
  partition are visible with or without a build tree

## Considered and deferred

- Browser scanning of a Buildroot output directory. The WASM core implements
  it and `viewer/scan-check.html` tests it, but no browser file picker can
  offer it usefully: both `webkitdirectory` and dropped-directory recursion
  enumerate everything beneath the directory first, and an output tree is
  mostly `build/` and `per-package/`. Bringing it back means traversing a
  directory *handle* (File System Access API, Chromium only) so only
  `target/`, `images/` and named files are visited. Worth doing only if people
  actually ask for it: anyone with a build tree can run the CLI.

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
- Listing squashfs contents in artifact-only mode. jffs2 already works because
  its names and sizes sit in plain node headers; squashfs keeps its inode and
  directory tables compressed, so this needs decoders for xz, gzip, zstd and
  lz4 (pure-Rust ones exist, at maybe 150-400 KB of WASM) plus a real squashfs
  reader. Worth it to browse a released rootfs with no build tree.
- FIT image parsing (needs a minimal DTB reader)
- ext4 superblock stats
- Listing UBIFS contents. Harder than squashfs: the index is a wandering B-tree
  that has to be walked, and its nodes are LZO or zlib compressed, so it needs
  both a reader and decompressors. The superblock already gives the size,
  geometry and compression, which is what a size report mostly asks for.
- Reading a raw NAND dump that still carries its out-of-band bytes. The UBI
  reader assumes an image whose eraseblocks are contiguous, which is what
  `ubinize` writes and what gets flashed; a dump taken with spare areas
  interleaved would need those stripped first, and the OOB layout is
  controller-specific.
