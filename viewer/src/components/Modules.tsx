import { useMemo, useState } from "react";
import { humanBytes } from "../format";
import { useT } from "../i18n";
import { Report } from "../types";

export default function Modules({ report }: { report: Report }) {
  const t = useT();
  const [onDemandOnly, setOnDemandOnly] = useState(false);
  // The built-in count is the only stat that has more behind it, so the tile
  // is the control -- a tab would be a lot of furniture for one list.
  const [showBuiltin, setShowBuiltin] = useState(false);
  const [builtinQuery, setBuiltinQuery] = useState("");

  // Memoised because the ?? would hand the filter below a fresh array on
  // every render, defeating it.
  const builtin = useMemo(() => report.modules_meta?.builtin ?? [], [report.modules_meta]);
  const builtinShown = useMemo(() => {
    const q = builtinQuery.trim().toLowerCase();
    return q ? builtin.filter((n) => n.toLowerCase().includes(q)) : builtin;
  }, [builtin, builtinQuery]);

  const modules = useMemo(
    () => (onDemandOnly ? report.modules.filter((m) => !m.autoloaded) : report.modules),
    [report.modules, onDemandOnly]
  );
  const totalBytes = report.modules.reduce((a, m) => a + m.bytes, 0);
  const autoCount = report.modules.filter((m) => m.autoloaded).length;

  if (report.modules.length === 0) {
    return (
      <div className="pane">
        <div className="panel">
          <div className="empty">{t("no_modules")}</div>
        </div>
      </div>
    );
  }

  return (
    <div className="pane">
      <div className="statrow">
        <div className="stat">
          <div className="stat-label">{t("stat_modules")}</div>
          <div className="stat-value">{report.modules.length}</div>
        </div>
        <div className="stat">
          <div className="stat-label">{t("stat_total_size")}</div>
          <div className="stat-value">{humanBytes(totalBytes)}</div>
        </div>
        <div className="stat">
          <div className="stat-label">{t("stat_autoloaded")}</div>
          <div className="stat-value">{autoCount}</div>
        </div>
        <div className="stat">
          <div className="stat-label">{t("stat_kernel")}</div>
          <div className="stat-value">{report.modules_meta?.kernel_version ?? "–"}</div>
        </div>
        {builtin.length > 0 && (
          <button
            className={`stat stat-btn ${showBuiltin ? "active" : ""}`}
            onClick={() => setShowBuiltin(!showBuiltin)}
            aria-expanded={showBuiltin}
          >
            <div className="stat-label">
              {t("stat_builtin")}
              <span className="stat-more">{showBuiltin ? "▾" : "▸"}</span>
            </div>
            <div className="stat-value">{builtin.length}</div>
          </button>
        )}
      </div>

      {showBuiltin && builtin.length > 0 && (
        <div className="panel">
          <div className="panel-head">
            <span className="panel-title">{t("stat_builtin")}</span>
            <input
              className="search"
              type="search"
              placeholder={t("filter_modules")}
              value={builtinQuery}
              onChange={(e) => setBuiltinQuery(e.target.value)}
            />
          </div>
          {/* Names only: modules.builtin records what was compiled in, not
              where it came from or what it cost -- built-in code is part of
              the kernel image, with no size of its own to report. */}
          <div className="builtin-list">
            {builtinShown.map((name) => (
              <span key={name} className="builtin-item">
                {name}
              </span>
            ))}
          </div>
          <div className="panel-foot muted">
            {t("vars_matching", { n: builtinShown.length })}
          </div>
        </div>
      )}

      <div className="controls">
        <label className="checkline">
          <input
            type="checkbox"
            checked={onDemandOnly}
            onChange={(e) => setOnDemandOnly(e.target.checked)}
          />
          {t("on_demand_only")}
        </label>
      </div>

      <div className="panel">
        <div className="tbl-wrap">
              <table className="tbl">
          <thead>
            <tr>
              <th>{t("th_module")}</th>
              <th className="num">{t("th_bytes")}</th>
              <th>{t("th_package")}</th>
              <th>{t("th_load")}</th>
              <th>{t("th_path")}</th>
            </tr>
          </thead>
          <tbody>
            {modules.map((m) => (
              <tr key={m.path}>
                <td>{m.name}</td>
                <td className="num">{humanBytes(m.bytes)}</td>
                <td>{m.package ?? <span className="muted">–</span>}</td>
                <td>
                  {m.autoloaded ? (
                    <span className="chip chip-auto">{t("load_auto")}</span>
                  ) : (
                    <span className="muted">{t("load_on_demand")}</span>
                  )}
                </td>
                <td className="mono-dim trunc">{m.path}</td>
              </tr>
            ))}
          </tbody>
        </table>
              </div>
      </div>
    </div>
  );
}
