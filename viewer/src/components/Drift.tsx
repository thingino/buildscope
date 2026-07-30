import { useEffect, useMemo, useState } from "react";
import { computeDrift, NamedDelta } from "../drift";
import { humanBytes } from "../format";
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

function DeltaTable({ title, list }: { title: string; list: NamedDelta[] }) {
  const [showAll, setShowAll] = useState(false);
  if (list.length === 0) return null;
  const rows = showAll ? list : list.slice(0, 20);
  return (
    <div className="panel">
      <div className="panel-head">
        <span className="panel-title">{title}</span>
        <span className="muted">{list.length} changed</span>
      </div>
      <table className="tbl">
        <thead>
          <tr>
            <th>name</th>
            <th className="num">baseline</th>
            <th className="num">current</th>
            <th className="num">delta</th>
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
                {n.before === null && <span className="chip chip-new">new</span>}
                {n.after === null && <span className="chip chip-gone">removed</span>}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {list.length > 20 && (
        <button className="linkbtn" onClick={() => setShowAll(!showAll)}>
          {showAll ? "show top" : `show all ${list.length}`}
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
        <span className="muted">baseline</span>
        <select className="select" value={baseIdx} onChange={(e) => setBaseIdx(Number(e.target.value))}>
          {entries.map((b, i) =>
            i === currentIdx ? null : (
              <option key={b.id} value={i}>
                {b.name}
              </option>
            )
          )}
        </select>
        <span className="muted">compared against the current build ({current.build.name})</span>
      </div>

      {error && <div className="panel empty">{error}</div>}
      {!drift && !error && <div className="panel empty">loading baseline</div>}

      {drift && (
        <>
          <div className="statrow">
            {drift.rootfsCompressed && (
              <div className="stat">
                <div className="stat-label">rootfs compressed</div>
                <div className="stat-value">
                  <DeltaCell d={drift.rootfsCompressed.delta} />
                </div>
                <div className="stat-sub">
                  {humanBytes(drift.rootfsCompressed.before)} to {humanBytes(drift.rootfsCompressed.after)}
                </div>
              </div>
            )}
            {drift.rootfsUncompressed && (
              <div className="stat">
                <div className="stat-label">rootfs uncompressed</div>
                <div className="stat-value">
                  <DeltaCell d={drift.rootfsUncompressed.delta} />
                </div>
                <div className="stat-sub">
                  {humanBytes(drift.rootfsUncompressed.before)} to {humanBytes(drift.rootfsUncompressed.after)}
                </div>
              </div>
            )}
            {/* Artifact-only reports carry no package or module data at
                all, so a zero count there would be noise, not a finding. */}
            {(current.packages.length > 0 || (baseline?.packages.length ?? 0) > 0) && (
              <div className="stat">
                <div className="stat-label">packages changed</div>
                <div className="stat-value">{drift.packages.length}</div>
              </div>
            )}
            {(current.modules.length > 0 || (baseline?.modules.length ?? 0) > 0) && (
              <div className="stat">
                <div className="stat-label">modules changed</div>
                <div className="stat-value">{drift.modules.length}</div>
              </div>
            )}
            {drift.partitions.length > 0 && !drift.rootfsCompressed && (
              <div className="stat">
                <div className="stat-label">partitions changed</div>
                <div className="stat-value">{drift.partitions.length}</div>
              </div>
            )}
          </div>

          {drift.partitions.length > 0 && (
            <div className="panel">
              <div className="panel-head">
                <span className="panel-title">Partitions (used bytes)</span>
              </div>
              <table className="tbl">
                <thead>
                  <tr>
                    <th>partition</th>
                    <th className="num">baseline</th>
                    <th className="num">current</th>
                    <th className="num">delta</th>
                    <th className="num">partition size</th>
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
                          : `${opt(p.sizeBefore)} to ${opt(p.sizeAfter)}`}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}

          <DeltaTable title="Packages" list={drift.packages} />
          <DeltaTable title="Images" list={drift.images} />
          <DeltaTable title="Kernel modules" list={drift.modules} />

          {drift.partitions.length === 0 &&
            drift.packages.length === 0 &&
            drift.images.length === 0 &&
            drift.modules.length === 0 && (
              <div className="panel empty">No differences between these two builds.</div>
            )}
        </>
      )}
    </div>
  );
}
