/**
 * The landing view for a fleet: every build in the snapshot, from the index
 * alone. Nothing here reads a report, so a fleet of any size paints from a few
 * hundred bytes each and the tarball is not touched until a build is opened.
 *
 * Sorted by name: at fleet scale the first thing asked of it is usually "where
 * is this device", and a list that reorders itself between runs is hard to
 * navigate. Sorting by fill -- which builds are closest to overflowing -- is
 * one click on that column.
 */
import { useMemo, useState } from "react";
import { humanBytes, pct } from "../format";
import { useT } from "../i18n";
import { IndexEntry } from "../types";

function fillStatus(frac: number): string {
  if (frac > 1.0) return "crit";
  if (frac > 0.95) return "serious";
  if (frac > 0.85) return "warn";
  return "good";
}

type Col = "name" | "flash" | "rootfs" | "fill";

export default function Fleet({
  entries,
  onOpen,
}: {
  entries: IndexEntry[];
  onOpen: (i: number) => void;
}) {
  const t = useT();
  const [q, setQ] = useState("");
  // By name, so a reader can find a known device without hunting. Fill is one
  // click away, and its column header says which way it is sorted.
  const [sort, setSort] = useState<Col>("name");
  const [asc, setAsc] = useState(true);

  const rows = useMemo(() => {
    const needle = q.trim().toLowerCase();
    const key = (e: IndexEntry): number | string => {
      switch (sort) {
        case "name":
          return e.name;
        case "flash":
          return e.flash_bytes ?? -1;
        case "rootfs":
          return e.rootfs_bytes ?? -1;
        case "fill":
          return e.fullest_fill ?? -1;
      }
    };
    return entries
      .map((e, i) => ({ e, i }))
      .filter(({ e }) => !needle || e.name.toLowerCase().includes(needle))
      .sort((a, b) => {
        const ka = key(a.e);
        const kb = key(b.e);
        const c =
          typeof ka === "string"
            ? ka.localeCompare(kb as string)
            : (ka as number) - (kb as number);
        return asc ? c : -c;
      });
  }, [entries, q, sort, asc]);

  const head = (col: Col, label: string, num = false) => (
    <th
      className={`${num ? "num " : ""}sortable${sort === col ? " sorted" : ""}`}
      onClick={() => {
        if (sort === col) setAsc(!asc);
        else {
          setSort(col);
          setAsc(col === "name");
        }
      }}
    >
      {label}
      {sort === col && <span className="sortmark">{asc ? " ▲" : " ▼"}</span>}
    </th>
  );

  return (
    <div className="pane">
      <div className="panel">
        <div className="panel-head">
          <span className="panel-title">{t("fleet_title")}</span>
          <input
            className="search"
            type="search"
            placeholder={t("fleet_filter")}
            value={q}
            onChange={(e) => setQ(e.target.value)}
          />
        </div>
        <div className="tbl-wrap">
          <table className="tbl">
            <thead>
              <tr>
                {head("name", t("th_build"))}
                <th>{t("th_branch")}</th>
                {head("flash", t("th_flash"), true)}
                {head("rootfs", t("th_rootfs"), true)}
                <th>{t("th_partition")}</th>
                {head("fill", t("th_fill"), true)}
                <th className="bar-col" />
              </tr>
            </thead>
            <tbody>
              {rows.map(({ e, i }) => {
                const frac = e.fullest_fill ?? null;
                const st = frac === null ? "good" : fillStatus(frac);
                return (
                  <tr key={`${i}:${e.name}`} className="rowlink" onClick={() => onOpen(i)}>
                    <td>{e.name}</td>
                    <td className="muted">{e.build_ref ?? "–"}</td>
                    <td className="num">{e.flash_bytes ? humanBytes(e.flash_bytes) : "–"}</td>
                    <td className="num">{e.rootfs_bytes ? humanBytes(e.rootfs_bytes) : "–"}</td>
                    <td className="muted">{e.fullest_partition ?? "–"}</td>
                    <td className={`num ${frac !== null && frac > 0.85 ? `tx-${st}` : ""}`}>
                      {frac === null ? "–" : pct(frac)}
                    </td>
                    <td className="bar-col">
                      {frac !== null && (
                        <div className="minibar">
                          <div
                            className={`minibar-fill st-${st}`}
                            style={{ width: `${Math.min(frac, 1) * 100}%` }}
                          />
                        </div>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
        <div className="panel-foot muted">{t("fleet_builds", { n: rows.length })}</div>
      </div>
    </div>
  );
}
