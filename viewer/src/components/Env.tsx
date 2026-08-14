import { useMemo, useState } from "react";
import { humanBytes } from "../format";
import { TFn, useT } from "../i18n";
import { EnvVar, ImageReport, Report } from "../types";

/// The variables of one U-Boot environment partition. The values are the board's
/// own configuration -- boot command, memory split, partition list -- so they
/// explain a good deal of what the rest of this tab shows.
function EnvPanel({ img, t }: { img: ImageReport; t: TFn }) {
  const d = img.detail as Record<string, unknown>;
  // Rebuilt every render otherwise, which would defeat the memo below it.
  const vars = useMemo(() => (Array.isArray(d.vars) ? d.vars : []) as EnvVar[], [d.vars]);
  const [query, setQuery] = useState("");

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return vars;
    return vars.filter(
      (v) => v.key.toLowerCase().includes(q) || v.value.toLowerCase().includes(q)
    );
  }, [vars, query]);

  if (vars.length === 0) return null;
  const crcOk = d.crc_ok === true;

  return (
    <div className="panel">
      <div className="panel-head">
        <span className="panel-title">
          {t("env_title")} <span className="muted machine">{img.name}</span>
        </span>
        <span className="muted">
          {t("n_vars", { n: d.var_count as number })} · {humanBytes(d.used_bytes as number)}{" "}
          {t("th_used")}
          {typeof d.free_bytes === "number" ? ` · ${humanBytes(d.free_bytes)} ${t("th_free")}` : ""}
          {d.redundant === true ? " · redundant" : ""} ·{" "}
          <span className={crcOk ? "ok" : "crit"}>crc {crcOk ? "ok" : "BAD"}</span>
        </span>
      </div>
      <div className="controls">
        <input
          className="search"
          placeholder={t("filter_vars")}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        {query.trim() !== "" && (
          <span className="muted">{t("vars_matching", { n: shown.length })}</span>
        )}
      </div>
      <div className="tbl-wrap">
        <table className="tbl env-table">
          <thead>
            <tr>
              <th>{t("th_variable")}</th>
              <th>{t("th_value")}</th>
              <th className="num">{t("th_bytes")}</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((v) => (
              <tr key={v.key}>
                <td className="env-key">{v.key}</td>
                <td>
                  <span className="env-val">{v.value}</span>
                </td>
                <td className="num">{v.bytes}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {shown.length === 0 && <div className="empty">{t("no_matches")}</div>}
      {d.vars_truncated === true && <div className="muted trunc-note">{t("vars_capped")}</div>}
    </div>
  );
}


/// A tab of its own, because an environment is reference material: a table of
/// thirty variables, some of them eight lines long, buried the flash map it
/// sits under. What that map needs from it -- which source the layout came
/// from -- the map already states in its own header.
export default function Env({ report }: { report: Report }) {
  const t = useT();
  const envs = report.images.filter((i) => i.format === "uboot-env");
  return (
    <div className="pane">
      {envs.map((i) => (
        <EnvPanel key={i.name} img={i} t={t} />
      ))}
    </div>
  );
}
