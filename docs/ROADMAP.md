# Roadmap

## Phase 1 (done)

- Core parsers: squashfs superblock, jffs2 node scan, uImage header, U-Boot
  env, mtdparts, MBR, padding detection
- Buildroot inputs: packages-file-list.txt, .config, build-time.log, modules
- `scan` with a terminal summary and a report written into the build dir
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

## Phase 5 (done)

- Block-device coverage, so the tool is not just a raw-flash tool: ext2/3/4
  superblocks, GUID partition tables, FAT12/16/32 with usage counted from the
  allocation table, and cpio archives with a full listing. A Buildroot card
  image (GPT + vfat boot + ext4 root) now resolves end to end, validated
  against parted, dumpe2fs, fsck.fat and cpio.

## Phase 6 (done)

- squashfs contents in artifact-only mode, with pure-Rust decoders for gzip,
  xz, zstd and lz4, so a released rootfs browses in the terminal and in the
  browser with no build tree. Validated entry-by-entry against unsquashfs.
- Per-file true compressed cost from the block and fragment tables, replacing
  the ratio estimate for per-package flash cost wherever the image is readable
- FIT parsing, on a minimal flattened-device-tree reader
- UBIFS contents, by scanning nodes rather than walking the index
- Raw NAND dumps that still carry their out-of-band bytes

## Considered and deferred

- Reading `per-package/` in the browser, which would restore
  installed-but-not-shipped to a directory scan. It is 804,325 entries against
  the ~1,090 the rest of the scan visits, so it would cost more than everything
  else together for one section of the report.

## Phase 7 (done)

- Browser scanning of a build directory, by directory handle
  (`showDirectoryPicker`, Chromium only): the named inputs are opened by name
  and only `target/` and `images/` are walked, so a scan touches about 1,090
  entries of a 1,158,991-entry tree and never lists `build/` or `per-package/`

- `export` takes any number of builds and inlines them all, so a local file
  has a build picker and a drift comparison with nothing running
- `export --site` writes the viewer plus one JSON per build, fetched as each
  is opened, for publishing a fleet to any static host
- `serve` removed. Everything it did is now either a file you open or a
  directory you host, and the tool ships with no network listener at all.

## Phase 3c

- Render CI-published reports on the same site, so a project's build history
  is browsable without scanning anything locally: `export --site` produces the
  files, so what is left is the workflow that publishes them
- CI aggregation recipe: publish one report per build, retain history,
  trends over time
- Fleet-scale viewer work: search in the build picker, and an overview table
  across builds. The site index already carries what the table needs -- each
  build's flash size and the partition nearest its limit -- so the work is in
  the viewer.

## Distant / recorded, not planned

- squashfs or UBIFS compressed with lzo. The pure-Rust lzo crates do not build
  for wasm32, and splitting behaviour between the CLI and the browser would be
  worse than saying plainly that the image cannot be read.
- Reading file *data* out of a UBIFS volume. The listing comes from scanning
  nodes, but the data nodes are compressed and reaching them properly means the
  wandering B-tree. Sizes come from the inodes either way, which is what a size
  report asks for.
