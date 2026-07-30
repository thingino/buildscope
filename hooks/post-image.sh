#!/bin/sh
# Buildroot post-image hook: emit a buildscope report.json into BINARIES_DIR.
# Wire up with: BR2_ROOTFS_POST_IMAGE_SCRIPT="<this file>"
# Buildroot passes BINARIES_DIR as $1 and exports TARGET_DIR, BUILD_DIR,
# BR2_CONFIG, BASE_DIR. Never fails the build: a missing buildscope binary
# only prints a notice.

BUILDSCOPE="${BUILDSCOPE:-buildscope}"

if ! command -v "$BUILDSCOPE" >/dev/null 2>&1; then
    echo "buildscope: binary not found, skipping report (set BUILDSCOPE=/path/to/buildscope)"
    exit 0
fi

exec "$BUILDSCOPE" scan --hook "$1"
