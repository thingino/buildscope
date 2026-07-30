# buildscope

Size and composition analyzer for [Buildroot](https://buildroot.org) output
trees and the firmware images they produce.

**Analyze firmware in your browser: [buildscope.thingino.com](https://buildscope.thingino.com)**
(nothing is uploaded, everything runs locally)

Point buildscope at any Buildroot output directory and get a full accounting of
where your flash bytes went: every artifact in `images/`, real partition
budgets, and per-package attribution of the rootfs. No changes to your build
are required, and nothing is ever added to your firmware.

```
buildscope scan output/
```

```
== camera_t31x_gc4653-3.10.14-uclibc ==
   mipsel | uclibc | 3.10.14 | build time 9m25s
   flash jz_sfc 16.00 MiB via mtdparts (uenv.txt)
   boot       320.0 KiB [####################..] 290.0 KiB used ( 90.6%)  ok
   env         64.0 KiB [......................]     686 B used (  1.0%)  ok
   kernel      1.38 MiB [#####################.]  1.33 MiB used ( 96.6%)  ok
   rootfs      4.00 MiB [######################]  3.96 MiB used ( 99.1%)  ok
   data       10.25 MiB [......................]   3.5 KiB used (  0.0%)  ok
```

## What it reports

- **Every file in `images/`**, with format-aware introspection instead of bare
  file sizes:
  - squashfs: real bytes used, compression algorithm, block size (padding
    excluded, straight from the superblock)
  - ext2/ext3/ext4: used vs free from the block counts in the superblock, plus
    inode usage, block size, label and UUID
  - jffs2: actual used bytes vs free space from a node-level scan, because a
    jffs2 image padded to its partition size is not "full"
  - FAT12/16/32: used vs free counted from the allocation table itself rather
    than assumed from the partition size, plus cluster size and volume label
  - cpio (`newc`/`crc`): the whole archive listing with names, sizes and kinds,
    so an initramfs rootfs is as browsable as a build tree
  - uImage: declared payload size, compression type, load and entry address,
    header CRC check
  - U-Boot environment images: CRC validity, bytes used vs environment size,
    and every variable, so the board's own configuration is readable next to
    the layout it describes
  - UBI: eraseblock geometry, every volume with the space its table reserved
    against the payload actually written, per-volume flash cost including
    per-block headers, spare and unwritten blocks, and each volume's contents
    identified in turn (a kernel volume as a uImage, a rootfs volume as
    squashfs, and so on)
  - UBIFS: formatted size, block count and size, compression, and whether it
    is still set to grow into its volume on first mount
  - composite flash images: trailing-padding detection, and verification that
    each partition really holds what its name implies
- **Partition budgets** parsed from the build itself (a `mtdparts=` string in
  an environment source, a genimage configuration, a GUID or MBR partition
  table, or UBI's own volume table), never from a hardcoded table: content size
  vs partition size vs true used bytes, for every partition. `--flash-map` and
  `--genimage` cover layouts kept somewhere unusual. Raw flash and card images
  are both covered: NOR, NAND, and a GPT card with a FAT boot partition and an
  ext4 root all resolve to the same report.
- **Per-package sizes**: every file in the final rootfs attributed to the
  Buildroot package that installed it via `packages-file-list.txt`, with a
  per-package approximate compressed cost from the measured rootfs compression
  ratio.
- **A browsable file listing**: every path in the rootfs with its size and
  owning package, so "why is this partition full" is a tree you can walk rather
  than a number. A jffs2 partition additionally reconstructs its own listing
  from the image, names and sizes included, which needs no decompression and so
  works on a bare `.bin` too.
- **Kernel modules**: size, owning package, and whether anything auto-loads
  them.
- **Installed but not shipped**: files a package installed that are absent from
  the final rootfs (project-level trims and replacements), with install sizes
  recovered from `per-package/`. Buildroot's own always-removed development
  files are filtered out.
- **Build timings** per package from `build-time.log`.

Output is one schema-versioned `buildscope-report.json` per build, plus a
terminal summary.

## Commands

```
buildscope scan output/                  # one build, or a directory of builds
buildscope serve output/                 # browse them in the local web viewer
buildscope diff output/old output/new    # what grew, what shrank, what appeared
buildscope export output/my-build        # one self-contained HTML file
buildscope carve firmware.bin            # a released image, with no build tree
```

`diff` takes reports, build directories, or bare images, and `--json` gives the
full delta for scripting.

`serve` listens on **both IP families**: by default on both loopbacks
(`127.0.0.1` and `::1`), so an IPv6-only client works with no extra flags.
`--bind` takes a comma-separated list of addresses, or `all` for every
interface on both families:

```
buildscope serve output/                      # 127.0.0.1 and [::1]
buildscope serve output/ --bind all           # every interface, IPv4 + IPv6
buildscope serve output/ --bind ::1           # IPv6 loopback only
buildscope serve output/ --bind 2001:db8::5   # a specific IPv6 address
```

It prints every URL it is listening on, with IPv6 literals bracketed the way a
browser needs them. Addresses are bound IPv6-first, because a wildcard IPv6
socket on a dual-stack host also carries IPv4: binding IPv4 first would make
the IPv6 bind fail and silently drop IPv6 support.

## Analyzing firmware you did not build

With no build tree at all, buildscope recovers what the image itself knows:

```
buildscope carve firmware.bin
buildscope carve downloaded-release/     # every image in the directory
buildscope serve downloaded-release/     # browse them all
```

The partition layout comes from the image. A CRC-valid U-Boot environment
block is located by scanning, and its `mtdparts` spec is the partition table --
taken from the variable of that name, or from wherever the environment builds
its kernel command line, which is the only place a NAND board keeps it. Failing
that, a partition table is read directly, or UBI's volume table is, since UBI
describes itself and needs no help. Each partition is then carved and identified
with the same parsers used on build trees, so you get real per-partition usage,
filesystem facts, and kernel image details.

A NAND image is a raw boot region followed by one UBI area, and the volumes
inside it are what the flash really holds, so they take the place of the area in
the layout: `uboot-env`, `kernel`, `rootfs` and `overlay` appear as partitions
with their own usage, exactly as their NOR counterparts do. The area keeps an
entry of its own for what a volume cannot express -- eraseblock geometry, spare
blocks, and any volume the image reserved but never wrote to. A bare `.ubi`
container describes itself even though it usually carries the environment of the
chip it is destined for, whose layout describes a boot region the file does not
have.
Per-package attribution is impossible without a build tree, and the report
says so rather than guessing.

Because every partition is checked against what its name implies, this doubles
as an integrity check: a short or partly-transferred image is reported as
truncated against the layout it declares.

## In the browser

The analysis core also compiles to WebAssembly, so
[buildscope.thingino.com](https://buildscope.thingino.com) can do all of the
above with no server: drop a Buildroot output directory for the full
breakdown, or a bare firmware image to carve it. Nothing is uploaded. The File
API supplies names and sizes as metadata, so enumerating a target tree is free
and only the small build inputs and the files in `images/` are actually read.

Browser scans record `scan_mode: browser`, which differs from a native scan in
exactly one way: the File API exposes no inode links, so hardlinked files
cannot be charged once. Buildroot target trees rarely contain any.

## Two ways to run it

**Post-hoc (default).** `buildscope scan <dir>` works on any existing output
directory, including builds that finished long ago. Context (`.config`,
`build/`, `target/`, `images/`) is discovered from the tree layout. Pass a
single build directory or a parent containing many.

**Hooked.** For a report on every build, add the bundled hook to your
defconfig:

```
BR2_ROOTFS_POST_IMAGE_SCRIPT="path/to/buildscope/hooks/post-image.sh"
```

Buildroot then invokes buildscope after image assembly with exact context
(`BINARIES_DIR`, `TARGET_DIR`, `BUILD_DIR`, `BR2_CONFIG`), and the report lands
in `images/` on every build. Projects that assemble their final image after
Buildroot's image step should instead call `buildscope scan "$OUTPUT_DIR"` at
the end of that step. Both modes produce identical reports for the same tree;
the report records which one produced it.

## Languages

The viewer is translated into 15 languages, picked up from the browser with an
in-page override, and mirrors itself for right-to-left languages. `?lang=de`
opens a link in a specific language without changing the reader's preference.
Report content is never translated: package and partition names, image formats,
and the analysis core's diagnostics are the same words everywhere. See
[`viewer/README.md`](viewer/README.md) to add or fix a translation.

## Design rules

- Read-only over what Buildroot already produces. Nothing is ever embedded in,
  or added to, your firmware images.
- No host tools required: every image format is parsed natively.
- If something cannot be determined, the report says so. It never guesses.

## Building

Rust (stable) for the CLI and the WASM core, Node 20+ for the viewer:

```
cargo build --release                    # CLI at target/release/buildscope
cargo test --workspace                   # unit and integration tests

cargo build --release --target wasm32-unknown-unknown -p buildscope-wasm
cd viewer && npm ci && npm run build     # viewer at viewer/dist
```

`buildscope serve` picks the viewer up from `viewer/dist`, from beside the
binary, or from `--viewer-dir`.

## Layout

| Path | What it is |
|---|---|
| `core/` | the analysis core: format parsers, report schema, diff. Pure, no I/O |
| `cli/` | the `buildscope` command, native filesystem walker, local server |
| `wasm/` | the core compiled to WebAssembly behind a plain C ABI, plus parity harnesses |
| `viewer/` | the web viewer (React + Vite) |
| `hooks/` | the Buildroot post-image hook |
| `docs/` | roadmap |

The core never touches the filesystem: it consumes a snapshot (a file list plus
the contents that matter) and returns a report. That is why the same code runs
natively and in a browser, and why it is straightforward to test.

## License

MIT, see [LICENSE](LICENSE).
