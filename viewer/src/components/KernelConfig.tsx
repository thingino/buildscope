/**
 * The kernel's own .config, recovered from the image by CONFIG_IKCONFIG.
 *
 * It answers what nothing else in an artifact can: which options this kernel
 * was actually built with. The build tree's .config is gone once the tree is,
 * and a module list only says what shipped, not why it could.
 *
 * Options disabled with the "is not set" form are kept and shown as n --
 * knowing an option was considered and turned off is different from it never
 * having existed here -- but they are filtered out of the default view, which
 * is otherwise mostly noise.
 */
import { useMemo, useState } from "react";
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

export default function KernelConfig({ report }: { report: Report }) {
  const t = useT();
  const entries = useMemo(() => {
    for (const image of report.images) {
      const found = (image.detail as { kernel_config?: ConfigEntry[] }).kernel_config;
      if (Array.isArray(found) && found.length > 0) return found;
    }
    return [];
  }, [report]);

  const [query, setQuery] = useState("");
  const [showUnset, setShowUnset] = useState(false);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return entries.filter((e) => {
      if (!showUnset && e.value === "n") return false;
      if (!q) return true;
      return e.key.toLowerCase().includes(q) || e.value.toLowerCase().includes(q);
    });
  }, [entries, query, showUnset]);

  if (entries.length === 0) return null;

  const set = entries.filter((e) => e.value !== "n").length;

  return (
    <div className="pane">
      <div className="panel">
        <div className="panel-head">
          <span className="panel-title">{t("kconfig_title")}</span>
          <span className="muted">
            {t("kconfig_counts", { set, total: entries.length })}
          </span>
          <input
            className="search"
            type="search"
            placeholder={t("filter_options")}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <button
            className={`viewtoggle-btn standalone ${showUnset ? "active" : ""}`}
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
              {shown.map((e) => (
                <tr key={e.key}>
                  <td>{e.key}</td>
                  <td className={valueClass(e.value)}>{e.value}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="panel-foot muted">{t("vars_matching", { n: shown.length })}</div>
      </div>
    </div>
  );
}
