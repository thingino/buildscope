import { useMemo, useState } from "react";
import { humanBytes } from "../format";
import { Report } from "../types";

export default function Modules({ report }: { report: Report }) {
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
          <div className="empty">No kernel modules in this rootfs.</div>
        </div>
      </div>
    );
  }

  return (
    <div className="pane">
      <div className="statrow">
        <div className="stat">
          <div className="stat-label">modules</div>
          <div className="stat-value">{report.modules.length}</div>
        </div>
        <div className="stat">
          <div className="stat-label">total size</div>
          <div className="stat-value">{humanBytes(totalBytes)}</div>
        </div>
        <div className="stat">
          <div className="stat-label">autoloaded (/etc/modules)</div>
          <div className="stat-value">{autoCount}</div>
        </div>
        <div className="stat">
          <div className="stat-label">kernel</div>
          <div className="stat-value">{report.modules_meta?.kernel_version ?? "–"}</div>
        </div>
        {report.modules_meta && report.modules_meta.builtin.length > 0 && (
          <div className="stat">
            <div className="stat-label">built-in (in kernel image)</div>
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
          on-demand only (not autoloaded)
        </label>
      </div>

      <div className="panel">
        <div className="tbl-wrap">
              <table className="tbl">
          <thead>
            <tr>
              <th>module</th>
              <th className="num">bytes</th>
              <th>package</th>
              <th>load</th>
              <th>path</th>
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
                    <span className="chip chip-auto">auto</span>
                  ) : (
                    <span className="muted">on demand</span>
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
