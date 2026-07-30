import { useState } from "react";
import { dateOf, seconds } from "../format";
import { Report } from "../types";

const SHOW = 48;

export default function Timings({ report }: { report: Report }) {
  const [open, setOpen] = useState<string | null>(null);
  const [showAll, setShowAll] = useState(false);

  if (report.timings.length === 0) {
    return (
      <div className="pane">
        <div className="panel">
          <div className="empty">No build-time.log in this tree.</div>
        </div>
      </div>
    );
  }

  const max = Math.max(...report.timings.map((t) => t.seconds));
  const list = showAll ? report.timings : report.timings.slice(0, SHOW);

  return (
    <div className="pane">
      <div className="statrow">
        <div className="stat">
          <div className="stat-label">active build time</div>
          <div className="stat-value">
            {report.build.build_active_seconds !== null ? seconds(report.build.build_active_seconds) : "–"}
          </div>
        </div>
        <div className="stat">
          <div className="stat-label">packages timed</div>
          <div className="stat-value">{report.timings.length}</div>
        </div>
        <div className="stat">
          <div className="stat-label">finished</div>
          <div className="stat-value">{dateOf(report.build.completed_at_unix) || "–"}</div>
        </div>
      </div>

      <div className="panel">
        <div className="timing-list">
          {list.map((t) => {
            const isOpen = open === t.package;
            return (
              <div key={t.package}>
                <button
                  className="timing-row"
                  onClick={() => setOpen(isOpen ? null : t.package)}
                >
                  <span className="timing-name">{t.package}</span>
                  <span className="timing-bar">
                    <span className="timing-fill" style={{ width: `${(t.seconds / max) * 100}%` }} />
                  </span>
                  <span className="timing-sec num">{seconds(t.seconds)}</span>
                </button>
                {isOpen && (
                  <div className="timing-steps">
                    {t.steps.map((s) => (
                      <div key={s.step} className="timing-step">
                        <span className="mono-dim">{s.step}</span>
                        <span className="timing-bar sm">
                          <span
                            className="timing-fill"
                            style={{ width: `${(s.seconds / t.seconds) * 100}%` }}
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
            {showAll ? "show top" : `show all ${report.timings.length}`}
          </button>
        )}
      </div>
    </div>
  );
}
