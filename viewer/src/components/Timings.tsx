import { useMemo, useState } from "react";
import { dateOf, seconds } from "../format";
import { useT } from "../i18n";
import { Report } from "../types";

const SHOW = 48;

/**
 * The build: how it was configured, and how long it took.
 *
 * The profile and the expansion sit behind their counts rather than in the
 * open, the way the Kernel tab holds its option list -- a defconfig is a
 * couple of dozen lines but the expansion is several hundred, and neither is
 * what you came to the tab for.
 */
export default function Timings({ report }: { report: Report }) {
  const t = useT();
  const [open, setOpen] = useState<string | null>(null);
  const [showAll, setShowAll] = useState(false);
  const [showProfile, setShowProfile] = useState(false);
  const [showOptions, setShowOptions] = useState(false);
  const [query, setQuery] = useState("");

  const bc = report.build_config ?? null;
  const options = useMemo(() => bc?.options ?? [], [bc]);
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return options;
    return options.filter(
      (o) => o.key.toLowerCase().includes(q) || o.value.toLowerCase().includes(q)
    );
  }, [options, query]);

  const timed = report.timings.length > 0;
  const max = timed ? Math.max(...report.timings.map((row) => row.seconds)) : 0;
  const list = showAll ? report.timings : report.timings.slice(0, SHOW);

  return (
    <div className="pane">
      <div className="statrow">
        <div className="stat">
          <div className="stat-label">{t("stat_active_build_time")}</div>
          <div className="stat-value">
            {report.build.build_active_seconds !== null ? seconds(report.build.build_active_seconds) : "–"}
          </div>
        </div>
        <div className="stat">
          <div className="stat-label">{t("stat_packages_timed")}</div>
          <div className="stat-value">{report.timings.length}</div>
        </div>
        <div className="stat">
          <div className="stat-label">{t("stat_finished")}</div>
          <div className="stat-value">{dateOf(report.build.completed_at_unix) || "–"}</div>
        </div>
        {bc?.defconfig_text && (
          <button
            className={`stat stat-btn ${showProfile ? "active" : ""}`}
            data-help="help_profile"
            onClick={() => setShowProfile(!showProfile)}
            aria-expanded={showProfile}
          >
            <div className="stat-label">
              {t("stat_profile")}
              <span className="stat-more">{showProfile ? "▾" : "▸"}</span>
            </div>
            {/* The name, not a count: one file, and which file is the point. */}
            <div className="stat-value stat-name">{report.build.defconfig ?? "–"}</div>
          </button>
        )}
        {options.length > 0 && (
          <button
            className={`stat stat-btn ${showOptions ? "active" : ""}`}
            data-help="help_build_config"
            onClick={() => setShowOptions(!showOptions)}
            aria-expanded={showOptions}
          >
            <div className="stat-label">
              {t("stat_build_config")}
              <span className="stat-more">{showOptions ? "▾" : "▸"}</span>
            </div>
            <div className="stat-value">{options.length}</div>
          </button>
        )}
      </div>

      {showProfile && bc?.defconfig_text && (
        <div className="panel">
          <div className="panel-head">
            <span className="panel-title">{t("profile_title")}</span>
            <span className="muted">{report.build.defconfig}</span>
          </div>
          {/* Verbatim, comments and order intact: a defconfig is written to be
              read, and its comments carry the board name and the fragments. */}
          <pre className="defconfig">{bc.defconfig_text}</pre>
        </div>
      )}

      {showOptions && options.length > 0 && (
        <div className="panel">
          <div className="panel-head">
            <span className="panel-title">{t("build_config_title")}</span>
            <span className="muted">{t("n_options", { n: options.length })}</span>
            <input
              className="search"
              type="search"
              placeholder={t("filter_options")}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          <div className="tbl-wrap">
            <table className="tbl env-table">
              <thead>
                <tr>
                  <th>{t("th_option")}</th>
                  <th>{t("th_value")}</th>
                </tr>
              </thead>
              <tbody>
                {shown.map((o) => (
                  <tr key={o.key}>
                    <td className="env-key">{o.key}</td>
                    <td className={o.value === "y" ? "kc-y" : o.value === "m" ? "kc-m" : ""}>
                      <span className="env-val">{o.value}</span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="panel-foot muted">{t("vars_matching", { n: shown.length })}</div>
        </div>
      )}

      {!timed && (
        <div className="panel">
          <div className="empty">{t("no_build_log")}</div>
        </div>
      )}

      {timed && (
      <div className="panel">
        <div className="timing-list">
          {list.map((row) => {
            const isOpen = open === row.package;
            return (
              <div key={row.package}>
                <button
                  className="timing-row"
                  onClick={() => setOpen(isOpen ? null : row.package)}
                >
                  <span className="timing-name">{row.package}</span>
                  <span className="timing-bar">
                    <span className="timing-fill" style={{ width: `${(row.seconds / max) * 100}%` }} />
                  </span>
                  <span className="timing-sec num">{seconds(row.seconds)}</span>
                </button>
                {isOpen && (
                  <div className="timing-steps">
                    {row.steps.map((s) => (
                      <div key={s.step} className="timing-step">
                        <span className="mono-dim">{s.step}</span>
                        <span className="timing-bar sm">
                          <span
                            className="timing-fill"
                            style={{ width: `${(s.seconds / row.seconds) * 100}%` }}
                          />
                        </span>
                        <span className="num">{seconds(s.seconds)}</span>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
        {report.timings.length > SHOW && (
          <button className="linkbtn" onClick={() => setShowAll(!showAll)}>
            {showAll ? t("show_top") : t("show_all", { n: report.timings.length })}
          </button>
        )}
      </div>
      )}
    </div>
  );
}
