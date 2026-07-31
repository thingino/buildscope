import { IndexEntry, Report } from "./types";

export interface Loaded {
  entries: IndexEntry[];
  /** By position in `entries`; each source knows how to locate its own. */
  fetchReport: (i: number) => Promise<Report>;
}

/** Try a served static site (export --site). Null means file-drop mode. */
export async function tryApi(): Promise<Loaded | null> {
  try {
    const res = await fetch("./api/index");
    if (!res.ok) return null;
    const idx = (await res.json()) as { reports: IndexEntry[] };
    if (!Array.isArray(idx.reports)) return null;
    return {
      entries: idx.reports,
      fetchReport: async (i: number) => {
        const id = idx.reports[i]?.id;
        if (id === undefined) throw new Error("no such report");
        const r = await fetch(`./api/report/${id}`);
        if (!r.ok) throw new Error(`report ${id}: HTTP ${r.status}`);
        return (await r.json()) as Report;
      },
    };
  } catch {
    return null;
  }
}

/** Reports embedded by `buildscope export` (single self-contained HTML). */
export function inlineReports(): Report[] | null {
  const g = (window as unknown as { __BUILDSCOPE_REPORT__?: Report | Report[] })
    .__BUILDSCOPE_REPORT__;
  if (!g) return null;
  const arr = Array.isArray(g) ? g : [g];
  return arr.filter((r) => r && typeof r.schema === "number");
}

export function parseReportJson(text: string): Report {
  const r = JSON.parse(text) as Report;
  if (typeof r !== "object" || r === null || typeof r.schema !== "number") {
    throw new Error("not a buildscope report (missing schema)");
  }
  if (r.schema !== 1) {
    throw new Error(`unsupported report schema ${r.schema}`);
  }
  return r;
}
