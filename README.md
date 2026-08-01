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
  - squashfs: real bytes used, compression algorithm, block size, and the
    whole directory tree with a *measured* per-file cost -- a file's inode
    records the size of every block it occupies, and a tail sharing a fragment
    with other files is charged its share, so the numbers add up to the image
    (gzip, xz, zstd and lz4; an lzo image says so rather than guessing)
  - ext2/ext3/ext4: used vs free from the block counts in the superblock, plus
    inode usage, block size, label and UUID
  - jffs2: actual used bytes vs free space from a node-level scan, because a
    jffs2 image padded to its partition size is not "full"
  - FAT12/16/32: used vs free counted from the allocation table itself rather
    than assumed from the partition size, plus cluster size and volume label
  - cpio (`newc`/`crc`): the whole archive listing with names, sizes and kinds,
    so an initramfs rootfs is as browsable as a build tree
  - JZLZMA: Ingenic's hardware LZ77, which their bootloaders decompress with an
    engine in the SoC. It borrows LZMA's distance model but packs the bits
    plainly instead of range-coding them, so it is neither lzma nor any lz4
    shape and no standard tool reads it -- a partition holding one is opaque to
    `binwalk` and `file` alike. Their uImages routinely declare `comp=5` (lz4)
    for one anyway, so the header is not evidence; the container is checked
    instead. Decoded, it reports what the stream expands to and identifies the
    contents, so a vendor rootfs inside one is named rather than left as a
    number.
  - FIT (`.itb`): each payload itemised -- kernel, ramdisk, device tree -- with
    type, load address, compression and hashes, plus the configurations. A
    device tree payload is read in turn, so `fdt-1` is reported as the board it
    describes.
  - device trees: the `model` and `compatible` that say which board a `.dtb` is
    for, plus the kernel command line it carries. A Buildroot build ships a
    directory of these, all much the same size, so which is which is the whole
    question. An overlay is identified as one, by the nodes it patches.
    Device trees that never become files are found too, because on a board that
    boots from raw flash they usually do not: a bootloader built with
    `CONFIG_OF_SEPARATE` has its tree appended to its binary, and a kernel built
    with `CONFIG_BUILTIN_DTB` carries one inside, behind the kernel's own
    compression. Both cost real flash and neither appears in `images/`, so both
    are reported where they sit -- which is also the only way to confirm that
    the board's intended tree is the one that actually got built in. Whole
    trees are carried in the report, so the viewer can show one as source.
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
  - UBIFS: formatted size, block count and size, compression, and whether it is
    still set to grow into its volume on first mount, plus the contents by
    scanning its nodes -- which works with no decompressor because UBIFS
    compresses file data but not directory entries or inodes
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
  Buildroot package that installed it via `packages-file-list.txt`, and what
  each costs on flash. Where the rootfs image can be read that cost is measured
  per file rather than estimated from an average ratio, which matters because
  the average is wrong in both directions: in one real build a web interface
  compressed to 20% while busybox managed only 41%, against a 29% average.
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

Output is one schema-versioned `buildscope-report.json` per build, written into
the build directory beside the build's other metadata rather than into
`images/`, which holds only what gets flashed. Plus a terminal summary. A report records where it was scanned from as a path relative
to where the command ran -- `output/master/my-build` -- rather than an absolute
one, because a report gets committed, attached to releases and published, and
the builder's home directory is nobody else's business.

## Commands

```
buildscope scan output/                  # one build, or a directory of builds
buildscope diff output/old output/new    # what grew, what shrank, what appeared
buildscope export output/my-build        # one self-contained HTML file
buildscope export --site -o site/ output/  # a static site, one JSON per build
buildscope export --fleet -o . reports/*.json  # index + tarball, for a release
buildscope carve firmware.bin            # a released image, with no build tree
```

`diff` takes reports, build directories, or bare images, and `--json` gives the
full delta for scripting.

`export` takes as many builds as you like. Given several, it inlines them all
into one file, which is how a local HTML gets a build picker and a drift
comparison with nothing running:

```
buildscope export output/master/cam-a output/master/cam-b -o compare.html
```

For a fleet, inlining every build would mean downloading all of them to read
one, so `--site` writes the viewer once and one JSON per build beside it:

```
buildscope export --site -o site/ output/master/
```

```
site/index.html      the viewer
site/api/index       a few hundred bytes per build: name, flash size, the
                     partition nearest its limit
site/api/report/0    one per build, fetched only when that build is opened
```

Those are plain files, so any static host serves them -- GitHub Pages, nginx,
an object store -- with nothing of buildscope's running. Browsers block `fetch`
over `file://`, so a site has to be served; a single exported file does not,
and opens straight from disk.

When the builds come out of CI and the viewer is already hosted somewhere,
there is nothing left to publish but the data. `--fleet` writes just that pair:

```
buildscope export --fleet -o . reports/*.json
```

```
fleet-index.json       one small entry per build, enough to fill a picker
fleet-reports.tar.gz   every report, opened only when a build is opened
```

Both are ordinary artifacts to attach to a release. The archive is written
without shelling out to `tar` and `gzip`, and with mtimes fixed, so the same
reports always produce the same bytes.

The viewer reads a snapshot from `?fleet=`, which takes either a URL to a
directory holding the two files, or the tag of a release that carries them:

```
https://buildscope.thingino.com/?fleet=https://example.org/snapshots/2026-07-30/
https://buildscope.thingino.com/?fleet=latest
```

It opens on the fleet itself -- every build, sorted by how close its fullest
partition is to overflowing -- built from the index alone. The tarball is not
fetched until a build is opened, and is then decompressed once and kept, so
only the report being read costs anything to parse. Decompression is the
browser's own `DecompressionStream`, so no library is involved.

## Analyzing firmware you did not build

With no build tree at all, buildscope recovers what the image itself knows:

```
buildscope carve firmware.bin
buildscope carve downloaded-release/     # every image in the directory
```

The partition layout comes from the image. A CRC-valid U-Boot environment
block is located by scanning, and its `mtdparts` spec is the partition table --
taken from the variable of that name, or from wherever the environment builds
its kernel command line, which is the only place a NAND board keeps it. Failing
that, a partition table is read directly, or UBI's volume table is, since UBI
describes itself and needs no help. Each partition is then carved and identified
with the same parsers used on build trees, so you get real per-partition usage,
filesystem facts, and kernel image details.

Some vendors keep no environment a reader can check -- the bootloader is
proprietary and stores its settings somewhere private -- and for those the
layout is read out of a kernel command line instead, by finding the uImage,
decompressing it and taking the `mtdparts` it was booted with. Last of all
comes the same string sitting loose in the bootloader binary. Both are reported
as what they are, and warned about, because neither is checkable the way an
environment's CRC is: one image here carries a four-partition map in its
bootloader and really has six, which is why the kernel's is preferred and why
the partition-by-partition checks matter. Between them these two recover the
layout of images that otherwise carve as one opaque blob.

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
(`BINARIES_DIR`, `TARGET_DIR`, `BUILD_DIR`, `BR2_CONFIG`), and
`buildscope-report.json` lands in the output directory on every build. Projects that assemble their final image after
Buildroot's image step should instead call `buildscope scan "$OUTPUT_DIR"` at
the end of that step. Both modes produce identical reports for the same tree;
the report records which one produced it.

## Integrating it into your build

Nothing here is specific to any project: buildscope reads a Buildroot output
tree, and the recipe below is the same whatever is built from it.

### 1. Get the binary

Each release carries a static musl binary per architecture, so it runs on any
distribution and inside any build container regardless of its libc:

```sh
VER=v0.1.3
T=$([ "$(uname -m)" = aarch64 ] && echo aarch64 || echo x86_64)-unknown-linux-musl
BASE="https://github.com/thingino/buildscope/releases/download/$VER"
curl -fsSL "$BASE/buildscope-$T" -o /tmp/buildscope
curl -fsSL "$BASE/buildscope-$T.sha256sum" -o /tmp/buildscope.sha256sum
sed "s|buildscope-$T|buildscope|" /tmp/buildscope.sha256sum > /tmp/bs.sum
( cd /tmp && sha256sum -c bs.sum )
install -m 0755 /tmp/buildscope /usr/local/bin/buildscope
```

Verify before installing, not after: a truncated download left on `PATH` is
worse than none at all. Building from source works too (`cargo build --release
-p buildscope`), and pins the exact revision if you would rather not trust a
release asset.

### 2. Report on every build

Point Buildroot's post-image hook at the bundled script:

```
BR2_ROOTFS_POST_IMAGE_SCRIPT="path/to/buildscope/hooks/post-image.sh"
```

Buildroot passes `BINARIES_DIR` as the first argument and exports `TARGET_DIR`,
`BUILD_DIR`, `BR2_CONFIG` and `BASE_DIR`; buildscope takes its context from
those rather than guessing, and records `context_source: hook` in the report so
you can tell later. The hook exits 0 when the binary is missing, so adding it
cannot break a build for someone who has not installed buildscope.

If your project assembles its final image *after* Buildroot's image step -- a
flash layout stitched together by your own `make` target, say -- call it at the
end of that step instead, so the report describes the artifact you actually
ship:

```make
    @BUILDSCOPE="$(HOST_DIR)/bin/buildscope"; \
        if [ ! -x "$$BUILDSCOPE" ]; then BUILDSCOPE=$$(command -v buildscope 2>/dev/null || true); fi; \
        if [ -n "$$BUILDSCOPE" ]; then "$$BUILDSCOPE" scan -q "$(OUTPUT_DIR)" || true; fi
```

Both forms write `buildscope-report.json` into the output directory, beside
`.config` and `images/`. It needs the build tree, so it has to run before
`distclean` removes it.

### 3. As a Buildroot package

To build the tool as part of the build itself, add a host package. buildscope
is a cargo workspace whose root is a virtual manifest, so the CLI is built from
its own subdirectory:

```make
BUILDSCOPE_VERSION = v0.1.3
BUILDSCOPE_SITE = $(call github,thingino,buildscope,$(BUILDSCOPE_VERSION))
BUILDSCOPE_LICENSE = MIT
BUILDSCOPE_LICENSE_FILES = LICENSE
HOST_BUILDSCOPE_SUBDIR = cli

$(eval $(host-cargo-package))
```

The hook above finds it at `$(HOST_DIR)/bin/buildscope` without further
configuration. Be aware this costs a Rust toolchain download and a compile in
every build; downloading a release binary is far cheaper across a matrix.

### 4. In CI, across a matrix

For one report per build and a snapshot of the whole set, three additions to a
matrix workflow:

- **each build job** installs the binary as above and uploads the resulting
  `buildscope-report.json` as an artifact. Make that step non-fatal: a report
  is an extra, and an unreachable release asset is no reason to fail a build.
- **a collector job** with `needs:` on the matrix downloads them all and runs
  `buildscope export --fleet -o . reports/*.json`, producing
  `fleet-index.json` and `fleet-reports.tar.gz`.
- **publish the pair** wherever the builds are published. As release assets
  they version themselves with the release that produced them.

Guard the collector with `!cancelled()` rather than `always()`, so a cancelled
run stops instead of reporting an error for itself, and let it exit 0 when it
finds no reports: a run where some builds failed should still publish a
snapshot of the ones that succeeded.

### 5. Read it

A `buildscope-report.json` opens in any hosted copy of the viewer by dropping
it on the page, no server and no upload. For a whole set, `export --site`
writes the viewer plus one JSON per build for any static host, and `--fleet`
writes the two assets described above.

Both fleet assets are ordinary files. Serving them from the same origin as the
viewer needs nothing; serving them from GitHub releases needs a proxy, because
release asset bytes carry no CORS headers. [`worker/`](worker/) is a small
Cloudflare Worker that does exactly that, if you want one.

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

`buildscope export` picks the viewer up from `viewer/dist`, from beside the
binary, or from `--viewer-dir`.

## Layout

| Path | What it is |
|---|---|
| `core/` | the analysis core: format parsers, report schema, diff. Pure, no I/O |
| `cli/` | the `buildscope` command and its native filesystem walker |
| `wasm/` | the core compiled to WebAssembly behind a plain C ABI, plus parity harnesses |
| `viewer/` | the web viewer (React + Vite) |
| `hooks/` | the Buildroot post-image hook |
| `worker/` | a Cloudflare Worker that serves fleet snapshots past GitHub's missing CORS |
| `docs/` | roadmap |

The core never touches the filesystem: it consumes a snapshot (a file list plus
the contents that matter) and returns a report. That is why the same code runs
natively and in a browser, and why it is straightforward to test.

## License

MIT, see [LICENSE](LICENSE).

## Working on it

```
cargo fmt --all --check                                   # formatting
cargo clippy --workspace --all-targets -- -D warnings     # lints, no warnings allowed
cargo test --workspace                                    # 113 tests
cd viewer && npm run build                                # locales, lint, types, bundle
```

CI runs all four on every push, so a tree that drifts stops the deploy rather
than reaching the site. The viewer has two further checks that need real
reports to run against, described in `viewer/README.md`.
