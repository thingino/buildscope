import { IndexEntry, Report } from "./types";

export interface Loaded {
  entries: IndexEntry[];
  fetchReport: (id: number) => Promise<Report>;
}

/** Try the local API (buildscope serve). Null means static mode: file drop. */
export async function tryApi(): Promise<Loaded | null> {
  try {
    const res = await fetch("./api/index");
    if (!res.ok) return null;
    const idx = (await res.json()) as { reports: IndexEntry[] };
    if (!Array.isArray(idx.reports)) return null;
    return {
      entries: idx.reports,
      fetchReport: async (id: number) => {
        const r = await fetch(`./api/report/${id}`);
        if (!r.ok) throw new Error(`report ${id}: HTTP ${r.status}`);
        return (await r.json()) as Report;
      },
    };
  } catch {
    return null;
  }
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
