import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Drift from "./components/Drift";
import Drop from "./components/Drop";
import Files from "./components/Files";
import Flash from "./components/Flash";
import Modules from "./components/Modules";
import Packages from "./components/Packages";
import Settings, { GearIcon } from "./components/Settings";
import Timings from "./components/Timings";
import { inlineReports, Loaded, parseReportJson, tryApi } from "./data";
import { dateOf, humanBytes, seconds } from "./format";
import { I18nContext, useI18nState, useT } from "./i18n";
import { IndexEntry, Report } from "./types";

type Tab = "flash" | "packages" | "files" | "modules" | "time" | "drift";
const TABS: { id: Tab; key: string }[] = [
  { id: "flash", key: "tab_flash" },
  { id: "packages", key: "tab_packages" },
  { id: "files", key: "tab_files" },
  { id: "modules", key: "tab_modules" },
  { id: "time", key: "tab_time" },
  { id: "drift", key: "tab_drift" },
];

/// An artifact-only report (a carved .bin) has no packages, modules or
/// timings; hide those tabs rather than showing empty panes.
function tabHasData(tab: Tab, r: Report, reportCount: number): boolean {
  switch (tab) {
    case "flash":
      return true;
    case "packages":
      return r.packages.length > 0 || r.rootfs !== null;
    case "files":
      // Either an attributed rootfs walk, or an image that reconstructed its
      // own contents.
      return (
        r.packages.some((p) => (p.files ?? p.top_files ?? []).length > 0) ||
        r.images.some((i) => Array.isArray((i.detail as { entries?: unknown[] }).entries))
      );
    case "modules":
      return r.modules.length > 0;
    case "time":
      return r.timings.length > 0;
    case "drift":
      return reportCount > 1;
  }
}

function readHash(): { b: number; t: Tab } {
  const h = new URLSearchParams(window.location.hash.replace(/^#/, ""));
  const t = h.get("t") as Tab | null;
  return {
    b: Number(h.get("b") ?? 0) || 0,
    t: t && TABS.some((x) => x.id === t) ? t : "flash",
  };
}

export default function App() {
  const i18n = useI18nState();
  return (
    <I18nContext.Provider value={i18n}>
      <Viewer />
    </I18nContext.Provider>
  );
}

function Viewer() {
  const t = useT();
  const [settingsOpen, setSettingsOpen] = useState(false);
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
  // Selected tab may not apply to the current report (switching builds, or
  // a carved artifact with no package data): fall back to Flash.
  const effectiveTab: Tab =
    report && !tabHasData(tab, report, entries.length) ? "flash" : tab;

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
              <span className="readout-item">
                {t("kernel_prefix")} {report.build.kernel_version}
              </span>
            )}
            {report.build.build_active_seconds !== null && (
              <span className="readout-item">{seconds(report.build.build_active_seconds)}</span>
            )}
            {report.build.completed_at_unix && (
              <span className="readout-item muted">{dateOf(report.build.completed_at_unix)}</span>
            )}
            <span className={`chip ctx-${report.scan.context_source}`}>
              {t(`ctx_${report.scan.context_source}`)}
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
          <button
            className="iconbtn"
            onClick={() => setSettingsOpen(true)}
            title={t("title_settings")}
            aria-label={t("title_settings")}
          >
            <GearIcon />
          </button>
          {staticMode && staticReports.length > 0 && (
            <label className="btn btn-sm">
              {t("add_report")}
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
                {report.scan.warnings.length === 1
                  ? t("warning_one")
                  : t("warning_many", { n: report.scan.warnings.length })}
              </button>
            )}
          </div>
          <nav className="tabs">
            {TABS.filter((tab) => tabHasData(tab.id, report, entries.length)).map((tab) => (
              <button
                key={tab.id}
                className={`tab ${effectiveTab === tab.id ? "active" : ""}`}
                onClick={() => setTab(tab.id)}
              >
                {t(tab.key)}
              </button>
            ))}
          </nav>
          <main key={`${current}:${effectiveTab}`} className="content">
            {effectiveTab === "flash" && <Flash report={report} />}
            {effectiveTab === "packages" && <Packages report={report} />}
            {effectiveTab === "files" && <Files report={report} />}
            {effectiveTab === "modules" && <Modules report={report} />}
            {effectiveTab === "time" && <Timings report={report} />}
            {effectiveTab === "drift" && entries.length > 1 && (
              <Drift entries={entries} currentIdx={current} current={report} getReport={getReport} />
            )}
          </main>
        </>
      ) : (
        <div className="empty page-empty">{loadError ?? t("loading")}</div>
      )}

      {settingsOpen && <Settings onClose={() => setSettingsOpen(false)} />}

      <footer className="foot">
        <a
          href="https://github.com/thingino/buildscope"
          target="_blank"
          rel="noopener"
          className="foot-link"
        >
          buildscope
        </a>{" "}
        v{__APP_VERSION__}-{__GIT_SHA__}
        {report && (
          <>
            {" · "}
            {t("report_schema", { n: report.schema })}
            {" · "}
            {/* Only the mode. Where the report's context came from is already
                the chip up in the header, and saying it twice on one screen
                just makes the reader check whether the two agree. */}
            {t("scan_mode", { mode: report.scan.scan_mode })}
            {report.generator.version !== __APP_VERSION__ && (
              <>
                {" · "}
                {t("report_by", {
                  name: report.generator.name,
                  version: report.generator.version,
                })}
              </>
            )}
          </>
        )}
      </footer>
    </div>
  );
}
