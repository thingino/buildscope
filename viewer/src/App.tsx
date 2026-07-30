import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Drift from "./components/Drift";
import Drop from "./components/Drop";
import Flash from "./components/Flash";
import Modules from "./components/Modules";
import Packages from "./components/Packages";
import Timings from "./components/Timings";
import { inlineReports, Loaded, parseReportJson, tryApi } from "./data";
import { dateOf, humanBytes, seconds } from "./format";
import { IndexEntry, Report } from "./types";

type Tab = "flash" | "packages" | "modules" | "time" | "drift";
const TABS: { id: Tab; label: string }[] = [
  { id: "flash", label: "Flash" },
  { id: "packages", label: "Packages" },
  { id: "modules", label: "Modules" },
  { id: "time", label: "Build time" },
  { id: "drift", label: "Drift" },
];

function readHash(): { b: number; t: Tab } {
  const h = new URLSearchParams(window.location.hash.replace(/^#/, ""));
  const t = h.get("t") as Tab | null;
  return {
    b: Number(h.get("b") ?? 0) || 0,
    t: t && TABS.some((x) => x.id === t) ? t : "flash",
  };
}

export default function App() {
  const [api, setApi] = useState<Loaded | null | "loading">("loading");
  const [staticReports, setStaticReports] = useState<Report[]>([]);
  const [current, setCurrent] = useState(() => readHash().b);
  const [tab, setTab] = useState<Tab>(() => readHash().t);
  const [report, setReport] = useState<Report | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    const inline = inlineReports();
    if (inline && inline.length > 0) {
      setStaticReports(inline);
      setApi(null);
      return;
    }
    void tryApi().then(setApi);
  }, []);

  const entries: IndexEntry[] = useMemo(() => {
    if (api !== "loading" && api !== null) return api.entries;
    return staticReports.map((r, i) => ({ id: i, name: r.build.name }));
  }, [api, staticReports]);

  useEffect(() => {
    const h = new URLSearchParams();
    h.set("b", String(current));
    h.set("t", tab);
    history.replaceState(null, "", "#" + h.toString());
  }, [current, tab]);

  useEffect(() => {
    setLoadError(null);
    if (api === "loading") return;
    if (api !== null) {
      const id = entries[current]?.id ?? entries[0]?.id;
      if (id === undefined) return;
      api
        .fetchReport(id)
        .then(setReport)
        .catch((e) => setLoadError(String(e)));
    } else {
      setReport(staticReports[current] ?? staticReports[0] ?? null);
    }
  }, [api, current, entries, staticReports]);

  // Fetch-with-cache for secondary reports (drift baselines).
  const cacheRef = useRef<Map<number, Report>>(new Map());
  const getReport = useCallback(
    async (i: number): Promise<Report> => {
      if (api !== "loading" && api !== null) {
        const cached = cacheRef.current.get(i);
        if (cached) return cached;
        const id = entries[i]?.id;
        if (id === undefined) throw new Error("no such report");
        const r = await api.fetchReport(id);
        cacheRef.current.set(i, r);
        return r;
      }
      const r = staticReports[i];
      if (!r) throw new Error("no such report");
      return r;
    },
    [api, entries, staticReports]
  );

  const addReports = useCallback((rs: Report[]) => {
    setStaticReports((prev) => {
      const next = [...prev, ...rs];
      setCurrent(next.length - rs.length); // jump to the first new one
      return next;
    });
  }, []);

  const staticMode = api === null;
  const showDrop = staticMode && staticReports.length === 0;

  return (
    <div className="app">
      <header className="top">
        <div className="brand">
          <span className="brand-mark" aria-hidden>
            <i className="bm bm-1" />
            <i className="bm bm-2" />
            <i className="bm bm-3" />
          </span>
          buildscope
        </div>
        {report && (
          <div className="readout">
            {report.flash?.mtd_id && report.flash.total_bytes && (
              <span className="readout-item">
                {report.flash.mtd_id} · {humanBytes(report.flash.total_bytes)}
              </span>
            )}
            {report.build.arch && <span className="readout-item">{report.build.arch}</span>}
            {report.build.libc && <span className="readout-item">{report.build.libc}</span>}
            {report.build.kernel_version && (
              <span className="readout-item">linux {report.build.kernel_version}</span>
            )}
            {report.build.build_active_seconds !== null && (
              <span className="readout-item">{seconds(report.build.build_active_seconds)}</span>
            )}
            {report.build.completed_at_unix && (
              <span className="readout-item muted">{dateOf(report.build.completed_at_unix)}</span>
            )}
            <span className={`chip ctx-${report.scan.context_source}`}>
              {report.scan.context_source}
            </span>
          </div>
        )}
        <div className="top-right">
          {entries.length > 1 && (
            <select
              className="select"
              value={current}
              onChange={(e) => setCurrent(Number(e.target.value))}
            >
              {entries.map((b, i) => (
                <option key={b.id} value={i}>
                  {b.name}
                </option>
              ))}
            </select>
          )}
          {staticMode && staticReports.length > 0 && (
            <label className="btn btn-sm">
              add report
              <input
                type="file"
                accept=".json,application/json"
                multiple
                hidden
                onChange={(e) => {
                  const files = e.target.files;
                  if (!files) return;
                  void (async () => {
                    const rs: Report[] = [];
                    for (const f of Array.from(files)) {
                      try {
                        rs.push(parseReportJson(await f.text()));
                      } catch {
                        /* ignore unparsable */
                      }
                    }
                    if (rs.length) addReports(rs);
                  })();
                }}
              />
            </label>
          )}
        </div>
      </header>

      {showDrop ? (
        <Drop onReports={addReports} />
      ) : report ? (
        <>
          <div className="buildline">
            <span className="buildname">{report.build.name}</span>
            {report.scan.warnings.length > 0 && (
              <button className="chip chip-warn" onClick={() => setTab("flash")}>
                {report.scan.warnings.length} warning{report.scan.warnings.length > 1 ? "s" : ""}
              </button>
            )}
          </div>
          <nav className="tabs">
            {TABS.filter((t) => t.id !== "drift" || entries.length > 1).map((t) => (
              <button
                key={t.id}
                className={`tab ${tab === t.id ? "active" : ""}`}
                onClick={() => setTab(t.id)}
              >
                {t.label}
              </button>
            ))}
          </nav>
          <main key={`${current}:${tab}`} className="content">
            {tab === "flash" && <Flash report={report} />}
            {tab === "packages" && <Packages report={report} />}
            {tab === "modules" && <Modules report={report} />}
            {tab === "time" && <Timings report={report} />}
            {tab === "drift" && entries.length > 1 && (
              <Drift entries={entries} currentIdx={current} current={report} getReport={getReport} />
            )}
          </main>
        </>
      ) : (
        <div className="empty page-empty">{loadError ?? "loading"}</div>
      )}

      <footer className="foot">
        <span>
          {report ? `report schema ${report.schema} · generated by ${report.generator.name} ${report.generator.version} · scan ${report.scan.scan_mode}/${report.scan.context_source}` : ""}
        </span>
      </footer>
    </div>
  );
}
