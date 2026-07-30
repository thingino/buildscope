# buildscope

Size and composition analyzer for [Buildroot](https://buildroot.org) output trees.

Point it at any Buildroot output directory and get a full accounting of where
your flash bytes went: every artifact in `images/`, real partition budgets, and
per-package attribution of the rootfs, with no changes to your build required.

```
buildscope scan output/
```

## What it reports

- **Every file in `images/`**, with format-aware introspection instead of bare
  file sizes:
  - squashfs: real bytes used, compression algorithm, block size (padding
    excluded, straight from the superblock)
  - jffs2: actual used bytes vs free space from a node-level scan, because a
    jffs2 image padded to its partition size is not "full"
  - uImage: declared payload size, compression type, load and entry address,
    header CRC check
  - U-Boot environment images: CRC validity, bytes used vs environment size,
    variable count
  - composite flash images: trailing-padding detection and partition content
    verification by magic at each offset
- **Partition budgets** parsed from the build itself (a `mtdparts=` string in
  an environment source, a partition table inside a disk image), never from a
  hardcoded table: content size vs partition size vs true used bytes, for every
  partition.
- **Per-package sizes**: every file in the final rootfs attributed to the
  Buildroot package that installed it via `packages-file-list.txt`, with
  per-package approximate compressed cost derived from the measured rootfs
  compression ratio.
- **Kernel modules**: size, owning package, and whether anything auto-loads
  them.
- **Build timings** per package from `build-time.log`.

Output is a single schema-versioned `report.json` per build, plus a terminal
summary. A local web viewer renders reports with partition bars, package
treemaps, and sortable tables:

```
buildscope serve output/
```

## Two ways to run it

**Post-hoc (default).** `buildscope scan <dir>` works on any existing output
directory, including builds finished long ago. Context (`.config`, `build/`,
`target/`, `images/`) is discovered from the tree layout. You can pass a single
build directory or a parent containing many.

**Hooked.** For automatic reports on every build, add the bundled hook to your
defconfig:

```
BR2_ROOTFS_POST_IMAGE_SCRIPT="path/to/buildscope/hooks/post-image.sh"
```

Buildroot then invokes buildscope after image assembly with exact context
(`BINARIES_DIR`, `TARGET_DIR`, `BUILD_DIR`, `BR2_CONFIG`), and a `report.json`
report lands in `images/` on every build. Both modes produce identical
reports on the same tree; the report records which mode produced it.

## Design rules

- Read-only over what Buildroot already produces. Nothing is ever embedded in
  or added to your firmware images.
- No host tools required: all image formats are parsed natively.
- If something cannot be determined, the report says unknown. It never guesses.

## Building

```
cargo build --release        # CLI at target/release/buildscope
```

The viewer is a separate npm project under `viewer/`; see `viewer/README.md`.

## License

MIT
