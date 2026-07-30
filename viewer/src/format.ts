const KI = 1024;

export function humanBytes(b: number): string {
  if (b >= KI * KI * KI) return (b / KI / KI / KI).toFixed(2) + " GiB";
  if (b >= KI * KI) return (b / KI / KI).toFixed(2) + " MiB";
  if (b >= KI) return (b / KI).toFixed(1) + " KiB";
  return b + " B";
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
