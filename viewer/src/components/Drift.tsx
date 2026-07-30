import { useEffect, useMemo, useState } from "react";
import { computeDrift, NamedDelta } from "../drift";
import { humanBytes } from "../format";
import { useT } from "../i18n";
import { IndexEntry, Report } from "../types";

function sdelta(d: number): string {
  return (d >= 0 ? "+" : "-") + humanBytes(Math.abs(d));
}

function DeltaCell({ d }: { d: number }) {
  return <span className={d > 0 ? "grow" : d < 0 ? "shrink" : "muted"}>{sdelta(d)}</span>;
}

function opt(v: number | null): string {
  return v === null ? "–" : humanBytes(v);
}

function DeltaTable({ titleKey, list }: { titleKey: string; list: NamedDelta[] }) {
  const t = useT();
  const [showAll, setShowAll] = useState(false);
  if (list.length === 0) return null;
  const rows = showAll ? list : list.slice(0, 20);
  return (
    <div className="panel">
      <div className="panel-head">
        <span className="panel-title">{t(titleKey)}</span>
        <span className="muted">{t("n_changed", { n: list.length })}</span>
      </div>
      <div className="tbl-wrap">
              <table className="tbl">
        <thead>
          <tr>
            <th>{t("th_name")}</th>
            <th className="num">{t("th_baseline")}</th>
            <th className="num">{t("th_current")}</th>
            <th className="num">{t("th_delta")}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {rows.map((n) => (
            <tr key={n.name}>
              <td>{n.name}</td>
              <td className="num">{opt(n.before)}</td>
              <td className="num">{opt(n.after)}</td>
              <td className="num">
                <DeltaCell d={n.delta} />
              </td>
              <td>
                {n.before === null && <span className="chip chip-new">{t("chip_new")}</span>}
                {n.after === null && <span className="chip chip-gone">{t("chip_removed")}</span>}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
              </div>
      {list.length > 20 && (
        <button className="linkbtn" onClick={() => setShowAll(!showAll)}>
          {showAll ? t("show_top") : t("show_all", { n: list.length })}
        </button>
      )}
    </div>
  );
}

export default function Drift({
  entries,
  currentIdx,
  current,
  getReport,
}: {
  entries: IndexEntry[];
  currentIdx: number;
  current: Report;
  getReport: (i: number) => Promise<Report>;
}) {
  const t = useT();
  // Prefer the previous build in the list as the baseline.
  const firstOther =
    currentIdx > 0 ? currentIdx - 1 : entries.findIndex((_, i) => i !== currentIdx);
  const [baseIdx, setBaseIdx] = useState(firstOther);
  const [baseline, setBaseline] = useState<Report | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (baseIdx < 0 || baseIdx === currentIdx) return;
    setBaseline(null);
    setError(null);
    getReport(baseIdx)
      .then(setBaseline)
      .catch((e) => setError(String(e)));
  }, [baseIdx, currentIdx, getReport]);

  const drift = useMemo(
    () => (baseline ? computeDrift(baseline, current) : null),
    [baseline, current]
  );

  return (
    <div className="pane">
      <div className="controls">
        <span className="muted">{t("baseline")}</span>
        <select className="select" value={baseIdx} onChange={(e) => setBaseIdx(Number(e.target.value))}>
          {entries.map((b, i) =>
            i === currentIdx ? null : (
              <option key={b.id} value={i}>
                {b.name}
              </option>
            )
          )}
        </select>
        <span className="muted">{t("compared_against", { name: current.build.name })}</span>
      </div>

      {error && <div className="panel empty">{error}</div>}
      {!drift && !error && <div className="panel empty">{t("loading_baseline")}</div>}

      {drift && (
        <>
          <div className="statrow">
            {drift.rootfsCompressed && (
              <div className="stat">
                <div className="stat-label">{t("stat_rootfs_compressed")}</div>
                <div className="stat-value">
                  <DeltaCell d={drift.rootfsCompressed.delta} />
                </div>
                <div className="stat-sub">
                  {t("range_from_to", {
                    from: humanBytes(drift.rootfsCompressed.before),
                    to: humanBytes(drift.rootfsCompressed.after),
                  })}
                </div>
              </div>
            )}
            {drift.rootfsUncompressed && (
              <div className="stat">
                <div className="stat-label">{t("stat_rootfs_uncompressed")}</div>
                <div className="stat-value">
                  <DeltaCell d={drift.rootfsUncompressed.delta} />
                </div>
                <div className="stat-sub">
                  {t("range_from_to", {
                    from: humanBytes(drift.rootfsUncompressed.before),
                    to: humanBytes(drift.rootfsUncompressed.after),
                  })}
                </div>
              </div>
            )}
            {/* Artifact-only reports carry no package or module data at
                all, so a zero count there would be noise, not a finding. */}
            {(current.packages.length > 0 || (baseline?.packages.length ?? 0) > 0) && (
              <div className="stat">
                <div className="stat-label">{t("stat_packages_changed")}</div>
                <div className="stat-value">{drift.packages.length}</div>
              </div>
            )}
            {(current.modules.length > 0 || (baseline?.modules.length ?? 0) > 0) && (
              <div className="stat">
                <div className="stat-label">{t("stat_modules_changed")}</div>
                <div className="stat-value">{drift.modules.length}</div>
              </div>
            )}
            {drift.partitions.length > 0 && !drift.rootfsCompressed && (
              <div className="stat">
                <div className="stat-label">{t("stat_partitions_changed")}</div>
                <div className="stat-value">{drift.partitions.length}</div>
              </div>
            )}
          </div>

          {drift.partitions.length > 0 && (
            <div className="panel">
              <div className="panel-head">
                <span className="panel-title">{t("partitions_used_bytes")}</span>
              </div>
              <div className="tbl-wrap">
              <table className="tbl">
                <thead>
                  <tr>
                    <th>{t("th_partition")}</th>
                    <th className="num">{t("th_baseline")}</th>
                    <th className="num">{t("th_current")}</th>
                    <th className="num">{t("th_delta")}</th>
                    <th className="num">{t("th_partition_size")}</th>
                  </tr>
                </thead>
                <tbody>
                  {drift.partitions.map((p) => (
                    <tr key={p.name}>
                      <td>{p.name}</td>
                      <td className="num">{opt(p.usedBefore)}</td>
                      <td className="num">{opt(p.usedAfter)}</td>
                      <td className="num">
                        <DeltaCell d={p.usedDelta} />
                      </td>
                      <td className="num mono-dim">
                        {p.sizeBefore === p.sizeAfter
                          ? opt(p.sizeAfter)
                          : t("range_from_to", { from: opt(p.sizeBefore), to: opt(p.sizeAfter) })}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              </div>
            </div>
          )}

          <DeltaTable titleKey="drift_packages" list={drift.packages} />
          <DeltaTable titleKey="drift_images" list={drift.images} />
          <DeltaTable titleKey="drift_modules" list={drift.modules} />

          {drift.partitions.length === 0 &&
            drift.packages.length === 0 &&
            drift.images.length === 0 &&
            drift.modules.length === 0 && (
              <div className="panel empty">{t("no_differences")}</div>
            )}
        </>
      )}
    </div>
  );
}
