import { useState } from "react";
import { dateOf, seconds } from "../format";
import { useT } from "../i18n";
import { Report } from "../types";

const SHOW = 48;

export default function Timings({ report }: { report: Report }) {
  const t = useT();
  const [open, setOpen] = useState<string | null>(null);
  const [showAll, setShowAll] = useState(false);

  if (report.timings.length === 0) {
    return (
      <div className="pane">
        <div className="panel">
          <div className="empty">{t("no_build_log")}</div>
        </div>
      </div>
    );
  }

  const max = Math.max(...report.timings.map((row) => row.seconds));
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
      </div>

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
    </div>
  );
}
