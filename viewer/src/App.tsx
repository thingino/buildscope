import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Drift from "./components/Drift";
import Drop from "./components/Drop";
import Fleet from "./components/Fleet";
import FleetEntry from "./components/FleetEntry";
import Files from "./components/Files";
import DeviceTree from "./components/DeviceTree";
import Env from "./components/Env";
import Flash from "./components/Flash";
import Kernel, { kernelConfigOf } from "./components/Kernel";
import Packages from "./components/Packages";
import Settings, { GearIcon } from "./components/Settings";
import Timings from "./components/Timings";
import { inlineReports, Loaded, parseReportJson, tryApi } from "./data";
import { fleetSpec, loadFleet } from "./fleet";
import { HelpProvider, useHelp } from "./help";
import { dateOf, humanBytes, seconds } from "./format";
import { I18nContext, useI18nState, useT } from "./i18n";
import { IndexEntry, Report } from "./types";

type Tab = "flash" | "env" | "dtb" | "packages" | "files" | "kernel" | "time" | "drift";
const TABS: { id: Tab; key: string }[] = [
  { id: "flash", key: "tab_flash" },
  { id: "env", key: "tab_env" },
  { id: "dtb", key: "tab_dtb" },
  { id: "packages", key: "tab_packages" },
  { id: "files", key: "tab_files" },
  { id: "kernel", key: "tab_kernel" },
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
    case "kernel":
      // Anything the report knows about the kernel: a build tree brings the
      // modules and the built-in list, a bare image brings the config, and
      // either alone is worth a tab.
      return (
        r.modules.length > 0 ||
        (r.modules_meta?.builtin.length ?? 0) > 0 ||
        kernelConfigOf(r).length > 0
      );
    case "time":
      return r.timings.length > 0;
    case "drift":
      return reportCount > 1;
  }
}

/**
 * The branch and revision a build came from, as its own os-release recorded
 * it. BUILD_ID conventionally reads "<branch>+<rev>, <date>", so the part
 * before the comma is the useful half; the codename alone is the fallback.
 * Nothing here is project-specific: os-release is a Buildroot-generated file.
 */
function buildRef(r: Report): string | null {
  const os = r.build.os_release ?? {};
  const head = os.BUILD_ID?.split(",")[0]?.trim();
  return head || os.VERSION_CODENAME || null;
}

/** Tab ids that have been renamed, so links made before still land. */
const TAB_ALIASES: Record<string, Tab> = { modules: "kernel", kconfig: "kernel" };

/** Question mark, drawn to match the gear beside it. */
function QuestionIcon() {
  return (
    <svg
      className="icon"
      viewBox="0 0 16 16"
      width="13"
      height="13"
      fill="currentColor"
      aria-hidden="true"
      focusable="false"
    >
      <path d="M5.25 5.5a2.75 2.75 0 1 1 3.9 2.5c-.6.28-.9.79-.9 1.3v.45a.6.6 0 0 1-1.2 0V9.3c0-1.05.63-1.9 1.6-2.36a1.55 1.55 0 1 0-2.2-1.44.6.6 0 0 1-1.2 0M8 13.1a.85.85 0 1 1 0-1.7.85.85 0 0 1 0 1.7" />
    </svg>
  );
}

function readHash(): { b: number; t: Tab } {
  const h = new URLSearchParams(window.location.hash.replace(/^#/, ""));
  const raw = h.get("t");
  const t = (raw && TAB_ALIASES[raw]) ?? (raw as Tab | null);
  return {
    b: Number(h.get("b") ?? 0) || 0,
    t: t && TABS.some((x) => x.id === t) ? t : "flash",
  };
}

export default function App() {
  const i18n = useI18nState();
  return (
    <I18nContext.Provider value={i18n}>
      <HelpProvider>
        <Viewer />
      </HelpProvider>
    </I18nContext.Provider>
  );
}

function Viewer() {
  const t = useT();
  const { on: helpOn, setOn: setHelpOn } = useHelp();
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

  // Home is the start of the site, which is the drop target -- so leaving a
  // fleet is part of going home, and ?fleet= comes off. Everything else in the
  // query stays: a reader who arrived with ?lang= or ?repo= keeps them.
  //
  // Within a fleet the step up from a build is its overview, and the header's
  // own control does that; this is the step above both.
  const goHome = useMemo(() => {
    const u = new URL(window.location.href);
    u.searchParams.delete("fleet");
    return `${u.pathname}${u.search}`;
  }, []);
  const resetToHome = useCallback(() => {
    // A fleet is loaded state, not a view: dropping back to the file selector
    // is a different page, so let the browser make it one.
    if (fleetMode) {
      window.location.assign(goHome);
      return;
    }
    setCurrent(0);
    setTab("flash");
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
            {buildRef(report) && (
              <span className="readout-item" title={report.build.os_release?.BUILD_ID}>
                {buildRef(report)}
              </span>
            )}
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
            <span
              className={`chip ctx-${report.scan.context_source}`}
              data-help={`help_ctx_${report.scan.context_source}`}
            >
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
            <button className="btn btn-sm" data-help="help_all_devices" onClick={() => setOverview(true)}>
              {t("back_to_fleet")}
            </button>
          )}
          {fleet && fleet.tags.length > 0 && (
            /* Switching snapshots reloads the page rather than swapping the
               data underneath: a different release is a different fleet, and
               the URL should say which one is being read. */
            <select
              className="select"
              data-help="help_snapshot"
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
          {/* Stepping, for reading several in a row without going back to the
              list each time. Follows the order the index was published in,
              which is by name -- the same order the list opens on. The ends
              stop rather than wrap, so it is clear when you have run out. */}
          {!overview && entries.length > 1 && (
            <div className="stepper" data-help="help_stepper">
              <button
                className="stepbtn"
                onClick={() => setCurrent(current - 1)}
                disabled={current <= 0}
                title={t("title_prev")}
                aria-label={t("title_prev")}
              >
                ‹
              </button>
              <span className="stepper-at">
                {current + 1}/{entries.length}
              </span>
              <button
                className="stepbtn"
                onClick={() => setCurrent(current + 1)}
                disabled={current >= entries.length - 1}
                title={t("title_next")}
                aria-label={t("title_next")}
              >
                ›
              </button>
            </div>
          )}
          <button
            className={`iconbtn ${helpOn ? "help-active" : ""}`}
            onClick={() => setHelpOn(!helpOn)}
            title={t("title_help")}
            aria-label={t("title_help")}
            aria-pressed={helpOn}
            data-help="help_help"
          >
            <QuestionIcon />
          </button>
          <button
            className="iconbtn"
            onClick={() => setSettingsOpen(true)}
            title={t("title_settings")}
            aria-label={t("title_settings")}
            data-help="help_settings"
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
            {effectiveTab === "kernel" && <Kernel report={report} />}
            {effectiveTab === "time" && <Timings report={report} />}
            {effectiveTab === "drift" && entries.length > 1 && (
              <Drift
                entries={entries}
                currentIdx={current}
                current={report}
                getReport={getReport}
                fleet={fleet}
              />
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
            <span data-help={`help_scan_${report.scan.scan_mode}`}>
              {t("scan_mode", { mode: report.scan.scan_mode })}
            </span>
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
