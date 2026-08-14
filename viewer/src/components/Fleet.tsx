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
import { humanBytes, hex, pct } from "../format";
import { useT } from "../i18n";
import { useTooltip } from "../tooltip";
import { IndexEntry } from "../types";

/* One hue per partition name, in the order the layout puts them, so a column
 * of devices reads down consistently -- rootfs is the same colour on every
 * row, and a layout that differs shows up as a step in the stack. Assigned
 * from the whole snapshot rather than the filtered rows, so filtering never
 * repaints what survives. */
const PART_HUES = ["#3987e5", "#d95926", "#199e70", "#c98500", "#d55181", "#008300"];
const PART_FALLBACK = "#6e7681";

function hueMap(entries: IndexEntry[]): Map<string, string> {
  const seen: string[] = [];
  for (const e of entries)
    for (const [name] of e.partitions ?? []) if (!seen.includes(name)) seen.push(name);
  return new Map(seen.map((n, i) => [n, PART_HUES[i] ?? PART_FALLBACK]));
}

function fillStatus(frac: number): string {
  if (frac > 1.0) return "crit";
  if (frac > 0.95) return "serious";
  if (frac > 0.85) return "warn";
  return "good";
}

/** The fixed columns, plus one per partition: `part:<name>`. */
type Col = "name" | "flash" | "rootfs" | "fill" | `part:${string}`;
type View = "table" | "map";

export default function Fleet({
  entries,
  missing,
  onOpen,
}: {
  entries: IndexEntry[];
  /** A build carried here from another release that this one does not have.
   *  Without it the reader cannot tell "not built in this release" apart from
   *  the listing simply forgetting where they were. */
  missing?: string | null;
  onOpen: (i: number) => void;
}) {
  const t = useT();
  const [q, setQ] = useState("");
  // By name, so a reader can find a known device without hunting. Fill is one
  // click away, and its column header says which way it is sorted.
  const [sort, setSort] = useState<Col>("name");
  const [asc, setAsc] = useState(true);
  // The table answers "how full is it"; the map answers "is this one laid out
  // like the others". The table stays the way in.
  const [view, setView] = useState<View>("table");
  // Segments like env -- 64 KiB on a 16 MiB chip, 0.4% of the bar -- are too
  // small to read as geometry. The numbers say what the bar cannot.
  const [numbers, setNumbers] = useState(false);
  const { node: tip, show, hide } = useTooltip();
  const hues = useMemo(() => hueMap(entries), [entries]);
  // Layouts arrived in the index later than the viewer that draws them, so a
  // snapshot published before that has nothing to map. Offer the view only
  // when there is something in it, rather than a screen of empty bars.
  const mappable = hues.size > 0;
  // One scale for every row, so an 8 MiB device is visibly half a 16 MiB one
  // rather than being stretched to match it.
  const widest = useMemo(
    () => Math.max(1, ...entries.map((e) => e.flash_bytes ?? 0)),
    [entries]
  );

  const rows = useMemo(() => {
    const needle = q.trim().toLowerCase();
    const key = (e: IndexEntry): number | string => {
      // By that partition's size: it is the number the column leads with, and
      // what actually separates one layout from another. Fill barely varies
      // within a partition -- rootfs is ~100% everywhere, data ~0.
      if (sort.startsWith("part:")) {
        const want = sort.slice(5);
        return (e.partitions ?? []).find((p) => p[0] === want)?.[2] ?? -1;
      }
      switch (sort) {
        case "name":
          return e.name;
        case "flash":
          return e.flash_bytes ?? -1;
        case "rootfs":
          return e.rootfs_bytes ?? -1;
        case "fill":
          return e.fullest_fill ?? -1;
        default:
          return e.name;
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
          {mappable && (
          <div className="viewtoggle" data-help="help_fleet_view">
            {(["table", "map"] as View[]).map((v) => (
              <button
                key={v}
                className={`viewtoggle-btn ${view === v ? "active" : ""}`}
                onClick={() => setView(v)}
              >
                {t(v === "table" ? "view_table" : "view_map")}
              </button>
            ))}
          </div>
          )}
          {mappable && view === "map" && (
            <button
              className={`viewtoggle-btn standalone ${numbers ? "active" : ""}`}
              data-help="help_fleet_numbers"
              onClick={() => setNumbers(!numbers)}
            >
              {t("view_numbers")}
            </button>
          )}
          <input
            className="search"
            type="search"
            placeholder={t("fleet_filter")}
            value={q}
            onChange={(e) => setQ(e.target.value)}
          />
        </div>
        {missing && <div className="muted trunc-note">{t("fleet_missing", { name: missing })}</div>}
        {view === "map" && mappable ? (
          <div className={`fmap ${numbers ? "with-nums" : ""}`}>
            <div className="fmap-legend">
              {[...hues].map(([name, hue]) => (
                <span key={name} className="fmap-key machine">
                  <i style={{ background: hue }} />
                  {name}
                </span>
              ))}
              <span className="muted fmap-scale">{humanBytes(widest)}</span>
            </div>
            {numbers && (
              <div className="fmap-row fmap-head">
                <div className="fmap-name" />
                <div className="fmap-track" />
                <div className="fmap-nums">
                  {[...hues.keys()].map((name) => {
                    const col = `part:${name}` as Col;
                    return (
                      <span
                        key={name}
                        className={`fmap-num sortable${sort === col ? " sorted" : ""}`}
                        onClick={() => {
                          if (sort === col) setAsc(!asc);
                          else {
                            setSort(col);
                            setAsc(false); // biggest first: the interesting end
                          }
                        }}
                      >
                        {name}
                        {sort === col && <span className="sortmark">{asc ? " ▲" : " ▼"}</span>}
                      </span>
                    );
                  })}
                </div>
              </div>
            )}
            {rows.map(({ e, i }) => (
              <div key={`${i}:${e.name}`} className="fmap-row rowlink" onClick={() => onOpen(i)}>
                <div className="fmap-name trunc">{e.name}</div>
                <div className="fmap-track">
                  <div
                    className="fmap-bar"
                    style={{ width: `${((e.flash_bytes ?? 0) / widest) * 100}%` }}
                  >
                    {(e.partitions ?? []).map(([name, offset, size, used]) => {
                      const span = size ?? 0;
                      const total = e.flash_bytes || 1;
                      const frac = span > 0 ? used / span : 0;
                      const hue = hues.get(name) ?? PART_FALLBACK;
                      return (
                        <div
                          key={`${name}:${offset}`}
                          className="fmap-seg"
                          style={{
                            left: `${(offset / total) * 100}%`,
                            width: `${(span / total) * 100}%`,
                            // 0x33 alpha: present enough to read the layout,
                            // faint enough that the solid fill reads as fill.
                            background: `${hue}33`,
                          }}
                          onMouseMove={(ev) =>
                            show(
                              ev,
                              <div>
                                <div className="tt-title">{name}</div>
                                <div>
                                  {hex(offset)} – {hex(offset + span)}
                                </div>
                                <div>
                                  {t("tt_partition")} {humanBytes(span)}
                                </div>
                                <div>
                                  {t("tt_used")} {humanBytes(used)} ({pct(frac)})
                                </div>
                                <div>
                                  {t("tt_free")} {humanBytes(Math.max(0, span - used))}
                                </div>
                              </div>
                            )
                          }
                          onMouseLeave={hide}
                        >
                          {/* Solid over translucent: the colour says which
                              partition, how much of it is filled says how
                              close this one is to the edge. */}
                          <div
                            className="fmap-used"
                            style={{ width: `${Math.min(frac, 1) * 100}%`, background: hue }}
                          />
                        </div>
                      );
                    })}
                  </div>
                </div>
                {numbers && (
                  <div className="fmap-nums">
                    {[...hues.keys()].map((name) => {
                      const part = (e.partitions ?? []).find((p) => p[0] === name);
                      if (!part) return <span key={name} className="fmap-num muted">–</span>;
                      const span = part[2] ?? 0;
                      const frac = span > 0 ? part[3] / span : 0;
                      return (
                        <span key={name} className="fmap-num">
                          {humanBytes(span)}
                          <i className={frac > 0.95 ? "tx-serious" : "muted"}>{pct(frac)}</i>
                        </span>
                      );
                    })}
                  </div>
                )}
              </div>
            ))}
            {tip}
          </div>
        ) : (
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
                    <td className="machine">{e.name}</td>
                    <td className="muted machine">{e.build_ref ?? "–"}</td>
                    <td className="num">{e.flash_bytes ? humanBytes(e.flash_bytes) : "–"}</td>
                    <td className="num">{e.rootfs_bytes ? humanBytes(e.rootfs_bytes) : "–"}</td>
                    <td className="muted machine">{e.fullest_partition ?? "–"}</td>
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
        )}
        <div className="panel-foot muted">{t("fleet_builds", { n: rows.length })}</div>
      </div>
    </div>
  );
}
