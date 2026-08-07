const KI = 1024;

/**
 * How sizes read, set by the units preference.
 *
 * `human` rounds, which is right for reading a report and wrong for working
 * with one: a partition and the image inside it often round alike while
 * differing by kilobytes. `bytes` is the exact count. `hex` matches the way
 * flash addresses are already written, so a size can be added to an offset
 * without converting it first.
 *
 * A module value rather than an argument because this is called from about
 * eighty render paths, and threading it through all of them would be a lot of
 * churn for a display choice. `UnitsProvider` owns the state that re-renders
 * the tree and keeps this in step; nothing else should write it.
 */
export type Units = "human" | "bytes" | "hex";

let units: Units = "human";

export function setUnits(u: Units): void {
  units = u;
}

export function humanBytes(b: number): string {
  // Ungrouped on purpose: the reason to ask for exact bytes is usually to
  // compare or copy one, and a separator would have to be either wrong in most
  // of the fifteen languages or in the way of a paste.
  if (units === "bytes") return b + " B";
  if (units === "hex") return hexSize(b);
  if (b >= KI * KI * KI) return (b / KI / KI / KI).toFixed(2) + " GiB";
  if (b >= KI * KI) return (b / KI / KI).toFixed(2) + " MiB";
  if (b >= KI) return (b / KI).toFixed(1) + " KiB";
  return b + " B";
}

/**
 * A size in hex, padded to the same six digits as an address.
 *
 * Padding is what makes a column of them readable: unpadded, 0x6E4 and
 * 0x930000 start in different places and the eye cannot compare magnitudes at
 * a glance. Six because that is what `hex()` gives the range column, so a size
 * and an offset line up and can be added by eye. Anything larger simply runs
 * longer, as an address does.
 *
 * The sign is kept rather than wrapped: a delta can be negative, and -0x1000
 * says so more clearly than a two's-complement word nobody asked for.
 */
function hexSize(b: number): string {
  const n = Math.abs(Math.trunc(b));
  return (b < 0 ? "-0x" : "0x") + n.toString(16).toUpperCase().padStart(6, "0");
}

export function hex(n: number): string {
  return "0x" + n.toString(16).toUpperCase().padStart(6, "0");
}

export function pct(frac: number): string {
  const p = frac * 100;
  return (p >= 10 ? p.toFixed(1) : p >= 0.1 ? p.toFixed(2) : p === 0 ? "0" : "<0.1") + "%";
}

export function seconds(s: number): string {
  if (s >= 3600) return `${Math.floor(s / 3600)}h${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}m`;
  if (s >= 60) return `${Math.floor(s / 60)}m${String(Math.round(s % 60)).padStart(2, "0")}s`;
  return s.toFixed(s >= 10 ? 0 : 1) + "s";
}

export function dateOf(unix: number | null): string {
  if (!unix) return "";
  return new Date(unix * 1000).toISOString().replace("T", " ").slice(0, 16) + " UTC";
}
