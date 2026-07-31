import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Drift from "./components/Drift";
import Drop from "./components/Drop";
import Fleet from "./components/Fleet";
import FleetEntry from "./components/FleetEntry";
import Files from "./components/Files";
import DeviceTree from "./components/DeviceTree";
import Env from "./components/Env";
import Flash from "./components/Flash";
import Modules from "./components/Modules";
import Packages from "./components/Packages";
import Settings, { GearIcon } from "./components/Settings";
import Timings from "./components/Timings";
import { inlineReports, Loaded, parseReportJson, tryApi } from "./data";
import { fleetSpec, loadFleet } from "./fleet";
import { dateOf, humanBytes, seconds } from "./format";
import { I18nContext, useI18nState, useT } from "./i18n";
import { IndexEntry, Report } from "./types";

type Tab = "flash" | "env" | "dtb" | "packages" | "files" | "modules" | "time" | "drift";
const TABS: { id: Tab; key: string }[] = [
  { id: "flash", key: "tab_flash" },
  { id: "env", key: "tab_env" },
  { id: "dtb", key: "tab_dtb" },
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
    case "env":
      // Only where an environment was actually found and read.
      return r.images.some(
        (i) =>
          i.format === "uboot-env" &&
          Array.isArray((i.detail as { vars?: unknown[] }).vars) &&
          (i.detail as { vars: unknown[] }).vars.length > 0
      );
    case "dtb":
      // A tree of its own, or one found inside something else.
      return r.images.some(
        (i) =>
          i.format === "dtb" ||
          i.format === "dtbo" ||
          Array.isArray((i.detail as { builtin_device_trees?: unknown[] }).builtin_device_trees) ||
          Array.isArray((i.detail as { device_trees?: unknown[] }).device_trees)
      );
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
  // A fleet's own state: which snapshot is open, which others exist, and how
  // far its tarball has got. Kept apart from loadError, which is cleared on
  // every build change.
  const [fleet, setFleet] = useState<{ tag: string | null; tags: string[] } | null>(null);
  const [fleetMode] = useState(() => fleetSpec() !== null);
  // A fleet opens on its overview, not on some arbitrary first build -- unless
  // the URL already names a build, which is what a shared link does.
  const [overview, setOverview] = useState(
    () =>
      fleetSpec() !== null &&
      !new URLSearchParams(window.location.hash.replace(/^#/, "")).has("b")
  );
  const [initError, setInitError] = useState<string | null>(null);
  const [progress, setProgress] = useState<string | null>(null);

  useEffect(() => {
    const inline = inlineReports();
    if (inline && inline.length > 0) {
      setStaticReports(inline);
      setApi(null);
      return;
    }
    const spec = fleetSpec();
    if (spec !== null) {
      loadFleet(spec || "latest", (done, total) =>
        setProgress(total ? `${Math.round((done / total) * 100)}%` : humanBytes(done))
      )
        .then((f) => {
          setFleet({ tag: f.tag, tags: f.tags });
          setProgress(null);
          setApi(f);
        })
        .catch((e: unknown) => {
          setProgress(null);
          setInitError(String(e));
          setApi(null);
        });
      return;
    }
    void tryApi().then(setApi);
  }, []);

  const entries: IndexEntry[] = useMemo(() => {
    if (api !== "loading" && api !== null) return api.entries;
    return staticReports.map((r, i) => ({ id: i, name: r.build.name }));
  }, [api, staticReports]);

  useEffect(() => {
    // The overview is the fleet's own address: no build is open, so recording
    // one would make a reload land somewhere the reader did not choose.
    if (overview) {
      history.replaceState(null, "", `${location.pathname}${location.search}`);
      return;
    }
    const h = new URLSearchParams();
    h.set("b", String(current));
    h.set("t", tab);
    history.replaceState(null, "", "#" + h.toString());
  }, [current, tab, overview]);

  useEffect(() => {
    setLoadError(null);
    if (api === "loading" || overview) return;
    if (api !== null) {
      if (entries.length === 0) return;
      api
        .fetchReport(current < entries.length ? current : 0)
        .then(setReport)
        .catch((e) => setLoadError(String(e)));
    } else {
      setReport(staticReports[current] ?? staticReports[0] ?? null);
    }
  }, [api, current, entries, staticReports, overview]);

  // Fetch-with-cache for secondary reports (drift baselines).
  const cacheRef = useRef<Map<number, Report>>(new Map());
  const getReport = useCallback(
    async (i: number): Promise<Report> => {
      if (api !== "loading" && api !== null) {
        const cached = cacheRef.current.get(i);
        if (cached) return cached;
        if (!entries[i]) throw new Error("no such report");
        const r = await api.fetchReport(i);
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

  // Home is this page without the fragment that records which build and tab
  // are open. The query is kept, so a reader who arrived with ?lang= does not
  // lose their language by going back to the start.
  const goHome = `${location.pathname}${location.search}`;
  const resetToHome = useCallback(() => {
    setCurrent(0);
    setTab("flash");
    // For a fleet the start is its overview, not its first build.
    if (fleetMode) setOverview(true);
    history.replaceState(null, "", goHome);
    // In the browser the start is the drop target, so let go of what was
    // dropped. Served or inlined, the reports are not ours to discard.
    if (api === null && inlineReports() === null) setStaticReports([]);
  }, [api, fleetMode, goHome]);

  const staticMode = api === null;
  // A page opened on a fleet is not a drop target: if its snapshot failed to
  // load, say so rather than silently offering something else.
  const showDrop = staticMode && staticReports.length === 0 && !fleetMode;
  // Selected tab may not apply to the current report (switching builds, or
  // a carved artifact with no package data): fall back to Flash.
  const effectiveTab: Tab =
    report && !tabHasData(tab, report, entries.length) ? "flash" : tab;

  return (
    <div className="app">
      <header className="top">
        {/* The way back. A bare anchor would not do it on its own: going from
            "#b=0&t=files" to the same path only changes the fragment, which
            the browser handles without reloading, so the state is reset here.
            It stays an anchor so it behaves like one -- a middle click still
            opens the app fresh in a new tab. */}
        <a
          className="brand"
          href={goHome}
          title={t("title_home")}
          onClick={(e) => {
            if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
            e.preventDefault();
            resetToHome();
          }}
        >
          <span className="brand-mark" aria-hidden>
            <i className="bm bm-1" />
            <i className="bm bm-2" />
            <i className="bm bm-3" />
          </span>
          buildscope
        </a>
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
          {/* The way back up. The brand link does this too, but nothing says
              so, and with a single-build fleet there is no device picker
              either -- leaving a reader who opened a build with no visible
              route back to the listing. Text rather than another glyph beside
              the gear, since being found is the whole point of it. */}
          {fleetMode && !overview && entries.length > 0 && (
            <button className="btn btn-sm" onClick={() => setOverview(true)}>
              {t("back_to_fleet")}
            </button>
          )}
          {fleet && fleet.tags.length > 0 && (
            /* Switching snapshots reloads the page rather than swapping the
               data underneath: a different release is a different fleet, and
               the URL should say which one is being read. */
            <select
              className="select"
              value={fleet.tag ?? ""}
              title={t("title_snapshot")}
              onChange={(e) => {
                const u = new URL(location.href);
                u.searchParams.set("fleet", e.target.value);
                u.hash = "";
                location.assign(u.toString());
              }}
            >
              {fleet.tags.map((tag) => (
                <option key={tag} value={tag}>
                  {tag}
                </option>
              ))}
            </select>
          )}
          {entries.length > 1 && (
            <select
              className="select"
              value={current}
              onChange={(e) => {
                setCurrent(Number(e.target.value));
                setOverview(false);
              }}
            >
              {entries.map((b, i) => (
                <option key={`${i}:${b.name}`} value={i}>
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

      {overview && entries.length > 0 ? (
        <Fleet
          entries={entries}
          onOpen={(i) => {
            setCurrent(i);
            setTab("flash");
            setOverview(false);
          }}
        />
      ) : showDrop ? (
        <>
          <Drop onReports={addReports} />
          <FleetEntry />
        </>
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
            {effectiveTab === "env" && <Env report={report} />}
            {effectiveTab === "dtb" && <DeviceTree report={report} />}
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
        <div className="empty page-empty">
          {initError ??
            loadError ??
            (progress ? t("loading_reports", { pct: progress }) : t("loading"))}
        </div>
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
            {/* Only the mode. Where the report's context came from is already
                the chip up in the header, and saying it twice on one screen
                just makes the reader check whether the two agree.

                The report's schema number is deliberately not shown: it is a
                compatibility contract between the writer and this viewer, which
                still checks it before rendering (see parseReportJson) and
                refuses a version it does not understand. Nobody reading a size
                report acts on the number. */}
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
