import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { CATEGORY_COLOR, CATEGORY_LABEL, CATEGORY_ORDER, Category, categorize } from "../categorize";
import { humanBytes, pct } from "../format";
import { squarify } from "../treemap";
import { PackageReport, Report, UNATTRIBUTED } from "../types";
import { useTooltip } from "../tooltip";

type SortKey = "bytes" | "approx" | "files" | "name";

function displayName(p: string): string {
  return p === UNATTRIBUTED ? "(overlay / post-build)" : p;
}

const MAP_W = 900;
const MAP_H = 560;
const NARROW_PX = 760;

function Treemap({ packages, total }: { packages: PackageReport[]; total: number }) {
  const { node, show, hide } = useTooltip();
  // A phone renders the map a third as wide, so whether a label fits has to be
  // decided in real pixels, not layout units. Measure the box and derive the
  // scale from it; cells too small for a label stay tappable for the tooltip.
  const boxRef = useRef<HTMLDivElement | null>(null);
  const [box, setBox] = useState({ w: 0, h: 0 });
  const narrow = typeof window !== "undefined" && window.innerWidth < NARROW_PX;

  useEffect(() => {
    const el = boxRef.current;
    if (!el) return;
    const measure = () => {
      const r = el.getBoundingClientRect();
      setBox({ w: r.width, h: r.height });
    };
    measure();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", measure);
      return () => window.removeEventListener("resize", measure);
    }
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const rects = useMemo(
    () =>
      squarify(
        packages.map((p) => ({ weight: p.bytes, data: p })),
        0,
        0,
        MAP_W,
        MAP_H
      ),
    [packages]
  );

  const scaleX = box.w > 0 ? box.w / MAP_W : 0;
  const scaleY = box.h > 0 ? box.h / MAP_H : 0;

  return (
    <div className="treemap-box">
      <div
        ref={boxRef}
        className="treemap"
        style={{ aspectRatio: narrow ? "4 / 5" : `${MAP_W} / ${MAP_H}` }}
      >
        {rects.map((r) => {
          const cat = categorize(r.data.name);
          const wp = (r.w / MAP_W) * 100;
          const hp = (r.h / MAP_H) * 100;
          // Room for a truncated name (and its size, when there are two lines
          // of vertical space) measured against the rendered geometry.
          const pxW = r.w * scaleX;
          const pxH = r.h * scaleY;
          const showLabel = pxW >= 62 && pxH >= 26;
          const showBytes = pxH >= 42;
          return (
            <div
              key={r.data.name}
              className="tm-cell"
              style={{
                left: `${(r.x / MAP_W) * 100}%`,
                top: `${(r.y / MAP_H) * 100}%`,
                width: `${wp}%`,
                height: `${hp}%`,
              }}
              onMouseMove={(e) =>
                show(
                  e,
                  <div>
                    <div className="tt-title">{displayName(r.data.name)}</div>
                    <div>{CATEGORY_LABEL[cat]}</div>
                    <div>
                      {humanBytes(r.data.bytes)} · {pct(r.data.bytes / total)} of rootfs
                    </div>
                    <div>{r.data.file_count} files</div>
                    {r.data.compressed_bytes_approx !== null && (
                      <div>~{humanBytes(r.data.compressed_bytes_approx)} compressed (approx)</div>
                    )}
                  </div>
                )
              }
              onClick={(e) =>
                show(
                  e,
                  <div>
                    <div className="tt-title">{displayName(r.data.name)}</div>
                    <div>{CATEGORY_LABEL[cat]}</div>
                    <div>
                      {humanBytes(r.data.bytes)} · {pct(r.data.bytes / total)} of rootfs
                    </div>
                    <div>{r.data.file_count} files</div>
                  </div>
                )
              }
              onMouseLeave={hide}
            >
              <div className="tm-fill" style={{ background: CATEGORY_COLOR[cat] }}>
                {showLabel && (
                  <div className="tm-label">
                    <div className="tm-name">{displayName(r.data.name)}</div>
                    {showBytes && <div className="tm-bytes">{humanBytes(r.data.bytes)}</div>}
                  </div>
                )}
              </div>
            </div>
          );
        })}
      </div>
      {node}
    </div>
  );
}

export default function Packages({ report }: { report: Report }) {
  const [sort, setSort] = useState<SortKey>("bytes");
  const [query, setQuery] = useState("");
  const [cats, setCats] = useState<Set<Category>>(new Set(CATEGORY_ORDER));
  const [open, setOpen] = useState<string | null>(null);

  const total = report.packages.reduce((a, p) => a + p.bytes, 0);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = report.packages.filter(
      (p) => cats.has(categorize(p.name)) && (q === "" || p.name.toLowerCase().includes(q))
    );
    const cmp: Record<SortKey, (a: PackageReport, b: PackageReport) => number> = {
      bytes: (a, b) => b.bytes - a.bytes,
      approx: (a, b) => (b.compressed_bytes_approx ?? 0) - (a.compressed_bytes_approx ?? 0),
      files: (a, b) => b.file_count - a.file_count,
      name: (a, b) => a.name.localeCompare(b.name),
    };
    return [...list].sort(cmp[sort]);
  }, [report.packages, sort, query, cats]);

  const maxBytes = filtered.length > 0 ? Math.max(...filtered.map((p) => p.bytes)) : 1;

  const toggleCat = (c: Category) => {
    setCats((prev) => {
      const next = new Set(prev);
      if (next.has(c)) {
        if (next.size === 1) return new Set(CATEGORY_ORDER); // last chip re-enables all
        next.delete(c);
      } else {
        next.add(c);
      }
      return next;
    });
  };

  return (
    <div className="pane">
      <div className="statrow">
        <div className="stat">
          <div className="stat-label">rootfs uncompressed</div>
          <div className="stat-value">{report.rootfs ? humanBytes(report.rootfs.uncompressed_bytes) : "–"}</div>
        </div>
        <div className="stat">
          <div className="stat-label">compressed ({report.rootfs?.compression ?? "?"})</div>
          <div className="stat-value">
            {report.rootfs?.compressed_bytes ? humanBytes(report.rootfs.compressed_bytes) : "–"}
            {report.rootfs?.compression_ratio && (
              <span className="stat-sub"> ×{report.rootfs.compression_ratio.toFixed(3)}</span>
            )}
          </div>
        </div>
        <div className="stat">
          <div className="stat-label">packages</div>
          <div className="stat-value">{report.packages.length}</div>
        </div>
        <div className="stat">
          <div className="stat-label">files in rootfs</div>
          <div className="stat-value">{report.rootfs?.file_count ?? "–"}</div>
        </div>
      </div>

      <div className="controls">
        <input
          className="search"
          placeholder="filter packages"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <select className="select" value={sort} onChange={(e) => setSort(e.target.value as SortKey)}>
          <option value="bytes">by uncompressed size</option>
          <option value="approx">by approx compressed</option>
          <option value="files">by file count</option>
          <option value="name">by name</option>
        </select>
        <div className="legend">
          {CATEGORY_ORDER.map((c) => (
            <button
              key={c}
              className={`chip legend-chip ${cats.has(c) ? "" : "off"}`}
              onClick={() => toggleCat(c)}
              title={`toggle ${CATEGORY_LABEL[c]}`}
            >
              <span className="dot" style={{ background: CATEGORY_COLOR[c] }} />
              {CATEGORY_LABEL[c]}
            </button>
          ))}
        </div>
      </div>

      <div className="pkg-split">
        <div className="panel pkg-table">
          <div className="tbl-wrap">
              <table className="tbl">
            <thead>
              <tr>
                <th className="pkg-name">package</th>
                <th className="num">bytes</th>
                <th className="num col-approx">~flash</th>
                <th className="num">files</th>
                <th className="bar-col">share</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((p) => {
                const cat = categorize(p.name);
                const isOpen = open === p.name;
                return (
                  <Fragment key={p.name}>
                    <tr
                      className="rowlink"
                      onClick={() => setOpen(isOpen ? null : p.name)}
                    >
                      <td className="pkg-name">
                        <span className="dot" style={{ background: CATEGORY_COLOR[cat] }} />
                        {displayName(p.name)}
                      </td>
                      <td className="num">{humanBytes(p.bytes)}</td>
                      <td className="num col-approx">
                        {p.compressed_bytes_approx !== null ? "~" + humanBytes(p.compressed_bytes_approx) : "–"}
                      </td>
                      <td className="num">{p.file_count}</td>
                      <td className="bar-col">
                        <div className="minibar">
                          <div
                            className="minibar-fill"
                            style={{ width: `${(p.bytes / maxBytes) * 100}%`, background: CATEGORY_COLOR[cat] }}
                          />
                        </div>
                      </td>
                    </tr>
                    {isOpen && (
                      <tr className="subrow">
                        <td colSpan={5}>
                          <div className="topfiles">
                            {p.top_files.map((f) => (
                              <div key={f.path} className="topfile">
                                <span className="mono-dim">{f.path}</span>
                                <span className="num">{humanBytes(f.bytes)}</span>
                              </div>
                            ))}
                          </div>
                        </td>
                      </tr>
                    )}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
              </div>
        </div>
        <div className="panel pkg-map">
          <Treemap packages={filtered} total={total || 1} />
        </div>
      </div>

      <NotShipped report={report} />
    </div>
  );
}

function NotShipped({ report }: { report: Report }) {
  const [showAll, setShowAll] = useState(false);
  const removed = report.removed_not_shipped ?? [];
  if (removed.length === 0) return null;
  const totalBytes = removed.reduce((a, r) => a + r.source_bytes, 0);
  const rows = showAll ? removed : removed.slice(0, 20);
  return (
    <div className="panel">
      <div className="panel-head">
        <span className="panel-title">Installed but not shipped</span>
        <span className="muted">
          {removed.length} files removed before imaging · {humanBytes(totalBytes)} at install time
        </span>
      </div>
      <div className="tbl-wrap">
              <table className="tbl">
        <thead>
          <tr>
            <th>path</th>
            <th className="pkg-name">package</th>
            <th className="num">install size</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.path}>
              <td className="mono-dim">{r.path}</td>
              <td>{r.package}</td>
              <td className="num">{r.source_bytes > 0 ? humanBytes(r.source_bytes) : "–"}</td>
            </tr>
          ))}
        </tbody>
      </table>
              </div>
      {removed.length > 20 && (
        <button className="linkbtn" onClick={() => setShowAll(!showAll)}>
          {showAll ? "show top" : `show all ${removed.length}`}
        </button>
      )}
    </div>
  );
}
