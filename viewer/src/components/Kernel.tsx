/**
 * Everything the artifact knows about the kernel: which version, what was
 * compiled into it, what ships beside it as loadable modules, and the options
 * that decided all three.
 *
 * They belong together because they answer each other -- a driver is built in
 * because a CONFIG_ line said y, and is a .ko because it said m -- and because
 * which of them exist depends on what was analyzed rather than on the subject.
 * A build tree has the modules and the built-in list; a bare image has neither
 * but still carries the config inside the kernel. One tab shows whichever the
 * report actually has.
 *
 * The two long lists sit behind the counts that raise the question about them,
 * so the default view stays the short one.
 */
import { useMemo, useState } from "react";
import { humanBytes } from "../format";
import { useT } from "../i18n";
import { Report } from "../types";

interface ConfigEntry {
  key: string;
  value: string;
}

function valueClass(value: string): string {
  if (value === "y") return "kc-y";
  if (value === "m") return "kc-m";
  if (value === "n") return "muted";
  return "";
}

/** The kernel's own config, if CONFIG_IKCONFIG put one in the image. */
export function kernelConfigOf(report: Report): ConfigEntry[] {
  for (const image of report.images) {
    const found = (image.detail as { kernel_config?: ConfigEntry[] }).kernel_config;
    if (Array.isArray(found) && found.length > 0) return found;
  }
  return [];
}

export default function Kernel({ report }: { report: Report }) {
  const t = useT();
  const [onDemandOnly, setOnDemandOnly] = useState(false);
  const [showBuiltin, setShowBuiltin] = useState(false);
  const [builtinQuery, setBuiltinQuery] = useState("");
  const [showConfig, setShowConfig] = useState(false);
  const [configQuery, setConfigQuery] = useState("");
  const [showUnset, setShowUnset] = useState(false);

  // Memoised because the ?? would hand the filter below a fresh array on
  // every render, defeating it.
  const builtin = useMemo(() => report.modules_meta?.builtin ?? [], [report.modules_meta]);
  const builtinShown = useMemo(() => {
    const q = builtinQuery.trim().toLowerCase();
    return q ? builtin.filter((n) => n.toLowerCase().includes(q)) : builtin;
  }, [builtin, builtinQuery]);

  const config = useMemo(() => kernelConfigOf(report), [report]);
  const configSet = useMemo(() => config.filter((e) => e.value !== "n").length, [config]);
  const configShown = useMemo(() => {
    const q = configQuery.trim().toLowerCase();
    return config.filter((e) => {
      if (!showUnset && e.value === "n") return false;
      if (!q) return true;
      return e.key.toLowerCase().includes(q) || e.value.toLowerCase().includes(q);
    });
  }, [config, configQuery, showUnset]);

  const modules = useMemo(
    () => (onDemandOnly ? report.modules.filter((m) => !m.autoloaded) : report.modules),
    [report.modules, onDemandOnly]
  );
  const totalBytes = report.modules.reduce((a, m) => a + m.bytes, 0);
  const autoCount = report.modules.filter((m) => m.autoloaded).length;

  // Empty only when the report knows nothing about the kernel at all -- not
  // merely when it shipped no loadable modules, which a perfectly ordinary
  // build does.
  if (report.modules.length === 0 && builtin.length === 0 && config.length === 0) {
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
          <div className="stat-label">{t("stat_kernel")}</div>
          <div className="stat-value">{report.modules_meta?.kernel_version ?? "–"}</div>
        </div>
        {report.modules.length > 0 && (
          <>
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
          </>
        )}
        {builtin.length > 0 && (
          <button
            className={`stat stat-btn ${showBuiltin ? "active" : ""}`}
            data-help="help_builtin"
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
        {config.length > 0 && (
          <button
            className={`stat stat-btn ${showConfig ? "active" : ""}`}
            data-help="help_kconfig"
            onClick={() => setShowConfig(!showConfig)}
            aria-expanded={showConfig}
          >
            <div className="stat-label">
              {t("stat_config")}
              <span className="stat-more">{showConfig ? "▾" : "▸"}</span>
            </div>
            <div className="stat-value">{configSet}</div>
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

      {showConfig && config.length > 0 && (
        <div className="panel">
          <div className="panel-head">
            <span className="panel-title">{t("kconfig_title")}</span>
            <span className="muted">
              {t("kconfig_counts", { set: configSet, total: config.length })}
            </span>
            <input
              className="search"
              type="search"
              placeholder={t("filter_options")}
              value={configQuery}
              onChange={(e) => setConfigQuery(e.target.value)}
            />
            <button
              className={`viewtoggle-btn standalone ${showUnset ? "active" : ""}`}
              data-help="help_show_unset"
              onClick={() => setShowUnset(!showUnset)}
            >
              {t("kconfig_show_unset")}
            </button>
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
                {configShown.map((e) => (
                  <tr key={e.key}>
                    <td>{e.key}</td>
                    <td className={valueClass(e.value)}>{e.value}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="panel-foot muted">
            {t("vars_matching", { n: configShown.length })}
          </div>
        </div>
      )}

      {report.modules.length > 0 && (
        <>
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
        </>
      )}
    </div>
  );
}
