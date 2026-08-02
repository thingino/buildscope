/**
 * Pulling the key/value material out of a report.
 *
 * The kernel's own config and the U-Boot environment are both carried inside
 * an image's `detail` rather than as fields of the report, so more than one
 * view needs the same little search. It lives here rather than in a component
 * because the drift computation wants it too, and that is a data module with
 * no business importing React.
 */
import { EnvVar, Report } from "./types";

export interface ConfigEntry {
  key: string;
  value: string;
}

/** The kernel's own config, if CONFIG_IKCONFIG put one in the image. */
export function kernelConfigOf(report: Report): ConfigEntry[] {
  for (const image of report.images) {
    const found = (image.detail as { kernel_config?: ConfigEntry[] }).kernel_config;
    if (Array.isArray(found) && found.length > 0) return found;
  }
  return [];
}

/** The kernel version the config belongs to, so two of them can be compared
 *  knowingly: across a version bump nearly every option differs and the
 *  comparison says nothing. */
export function kernelVersionOf(report: Report): string | null {
  return report.modules_meta?.kernel_version ?? null;
}

/**
 * Every U-Boot variable in the report, by name.
 *
 * A board can carry more than one environment -- a NOR env partition, a NAND
 * `uboot-env` volume -- and they hold the same variables. First wins, which is
 * the one the layout was read from.
 */
export function envVarsOf(report: Report): Map<string, string> {
  const out = new Map<string, string>();
  for (const image of report.images) {
    if (image.format !== "uboot-env") continue;
    const vars = (image.detail as { vars?: EnvVar[] }).vars;
    if (!Array.isArray(vars)) continue;
    for (const v of vars) {
      if (!out.has(v.key)) out.set(v.key, v.value);
    }
  }
  return out;
}
