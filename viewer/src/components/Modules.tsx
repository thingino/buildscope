import { useMemo, useState } from "react";
import { humanBytes } from "../format";
import { useT } from "../i18n";
import { Report } from "../types";

export default function Modules({ report }: { report: Report }) {
  const t = useT();
  const [onDemandOnly, setOnDemandOnly] = useState(false);

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
        {report.modules_meta && report.modules_meta.builtin.length > 0 && (
          <div className="stat">
            <div className="stat-label">{t("stat_builtin")}</div>
            <div className="stat-value">{report.modules_meta.builtin.length}</div>
          </div>
        )}
      </div>

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
