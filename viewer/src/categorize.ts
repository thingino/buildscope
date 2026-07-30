// Coarse Buildroot-package category for treemap identity. Generic rules
// only: no project-specific package names. First match wins.
// Categorical hues come from the validated dark-surface palette; assignment
// is fixed by slot, never cycled.

import { UNATTRIBUTED } from "./types";

export type Category =
  | "apps"
  | "kernel"
  | "libraries"
  | "firmware"
  | "base"
  | "overlay";

export const CATEGORY_ORDER: Category[] = [
  "apps",
  "kernel",
  "libraries",
  "firmware",
  "base",
  "overlay",
];

/** Translation keys; the legend and tooltips resolve these at render time. */
export const CATEGORY_KEY: Record<Category, string> = {
  apps: "cat_apps",
  kernel: "cat_kernel",
  libraries: "cat_libraries",
  firmware: "cat_firmware",
  base: "cat_base",
  overlay: "cat_overlay",
};

// Slots 1..6 of the validated categorical palette (dark).
export const CATEGORY_COLOR: Record<Category, string> = {
  apps: "#3987e5",
  kernel: "#d95926",
  libraries: "#199e70",
  firmware: "#c98500",
  base: "#d55181",
  overlay: "#008300",
};

const RULES: [RegExp, Category][] = [
  [new RegExp(`^${UNATTRIBUTED}$`), "overlay"],
  [/^skeleton/, "base"],
  [/^linux$|^linux-|-linux-compat$/, "kernel"],
  [/firmware|^wifi-|-wifi$|-blobs?$/, "firmware"],
  [/^busybox$|^toolchain|^uclibc|^musl|^glibc|^ifupdown|^initscripts|^(e)?udev$|^mdev|^ca-certificates$|^tzdata$|^urandom/, "base"],
  [/^lib|^zlib|^openssl|^mbedtls|^wolfssl|^json-c$|^jansson$|^pcre|^expat$|^ncurses|^readline$|^sqlite|^gmp$|^nettle$|^popt$|^attr$|^acl$/, "libraries"],
];

export function categorize(pkg: string): Category {
  for (const [re, cat] of RULES) {
    if (re.test(pkg)) return cat;
  }
  return "apps";
}
