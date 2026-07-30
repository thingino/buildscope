import { Fragment } from "react";
import { hex, humanBytes, pct } from "../format";
import { ImageReport, PartitionReport, Report } from "../types";
import { useTooltip } from "../tooltip";

function fillStatus(frac: number): string {
  if (frac > 1.0) return "crit";
  if (frac > 0.95) return "serious";
  if (frac > 0.85) return "warn";
  return "good";
}

function VerifiedMark({ v }: { v: boolean | null }) {
  if (v === null) return <span className="muted">–</span>;
  return v ? (
    <span className="ok" title="content verified against the flash image">✓ verified</span>
  ) : (
    <span className="crit" title="content does not match the flash image">✕ mismatch</span>
  );
}

function imageNote(i: ImageReport): string {
  const d = i.detail as Record<string, number | string | boolean>;
  switch (i.format) {
    case "squashfs":
      return `${humanBytes(d.bytes_used as number)} used · ${d.compression} · ${d.inode_count} inodes`;
    case "jffs2":
      return `${humanBytes(d.used_bytes as number)} used · ${humanBytes(d.free_bytes as number)} free · ${d.live_files} files`;
    case "uimage":
      return `${d.type} · ${d.compression} · payload ${humanBytes(d.declared_size as number)} · load ${d.load_addr}`;
    case "uboot-env":
      return `${humanBytes(d.used_bytes as number)} of ${humanBytes(i.bytes)} used · crc ${d.crc_ok ? "ok" : "BAD"} · ${d.var_count} vars`;
    case "flash-image":
      return `content to ${humanBytes(d.content_end as number)}`;
    case "raw":
      return d.trailing_padding && (d.trailing_padding as number) > 0
        ? `content ${humanBytes(d.content_end as number)} + ${humanBytes(d.trailing_padding as number)} padding`
        : "";
    default:
      return "";
  }
}

function DieMap({ parts, total }: { parts: PartitionReport[]; total: number }) {
  const { node, show, hide } = useTooltip();
  const sized = parts.filter((p) => !p.overlaps && p.size !== null);
  if (sized.length === 0 || total === 0) return null;

  // Proportional widths with a minimum so tiny partitions stay legible.
  const MIN = 0.055;
  const raw = sized.map((p) => (p.size as number) / total);
  const clamped = raw.map((f) => Math.max(f, MIN));
  const scale = clamped.reduce((a, b) => a + b, 0);

  return (
    <div className="diemap-wrap">
      <div className="diemap">
        {sized.map((p, i) => {
          const size = p.size as number;
          const used = p.used_bytes ?? p.content_bytes ?? 0;
          const frac = size > 0 ? used / size : 0;
          const st = fillStatus(frac);
          return (
            <div
              key={p.name}
              className={`seg reveal`}
              style={{ width: `${(clamped[i] / scale) * 100}%`, animationDelay: `${i * 60}ms` }}
              onMouseMove={(e) =>
                show(
                  e,
                  <div>
                    <div className="tt-title">{p.name}{p.read_only ? " (ro)" : ""}</div>
                    <div>{hex(p.offset)} – {hex(p.offset + size)}</div>
                    <div>partition {humanBytes(size)}</div>
                    {p.image && <div>image {p.image} ({humanBytes(p.content_bytes ?? 0)})</div>}
                    <div>used {humanBytes(used)} ({pct(frac)})</div>
                    <div>free {humanBytes(Math.max(0, size - used))}</div>
                  </div>
                )
              }
              onMouseLeave={hide}
            >
              <div className="seg-name">{p.name}</div>
              <div className="seg-size">{humanBytes(size)}</div>
              <div className="seg-fillpct">{pct(frac)}</div>
              <div className="seg-meter">
                <div className={`seg-fill st-${st}`} style={{ width: `${Math.min(frac, 1) * 100}%` }} />
              </div>
            </div>
          );
        })}
      </div>
      <div className="diemap-axis">
        <span>{hex(0)}</span>
        <span className="muted">widths proportional, small partitions clamped for legibility</span>
        <span>{hex(total)}</span>
      </div>
      {node}
    </div>
  );
}

export default function Flash({ report }: { report: Report }) {
  const flash = report.flash;
  return (
    <div className="pane">
      {flash && flash.total_bytes ? (
        <>
          <div className="panel">
            <div className="panel-head">
              <span className="panel-title">Flash map</span>
              <span className="muted">
                {flash.mtd_id ?? "device"} · {humanBytes(flash.total_bytes)} · {flash.source}
              </span>
            </div>
            <DieMap parts={flash.partitions} total={flash.total_bytes} />
            <table className="tbl">
              <thead>
                <tr>
                  <th>partition</th>
                  <th>range</th>
                  <th className="num">size</th>
                  <th>image</th>
                  <th className="num">content</th>
                  <th className="num">used</th>
                  <th className="num">free</th>
                  <th className="num">fill</th>
                  <th>check</th>
                </tr>
              </thead>
              <tbody>
                {flash.partitions.map((p) => {
                  const size = p.size ?? 0;
                  const used = p.used_bytes ?? p.content_bytes ?? 0;
                  const frac = size > 0 ? used / size : 0;
                  return (
                    <tr key={p.name} className={p.overlaps ? "dim" : ""}>
                      <td>
                        {p.name}
                        {p.read_only ? <span className="muted"> ro</span> : null}
                        {p.overlaps ? <span className="muted"> (spans)</span> : null}
                      </td>
                      <td className="mono-dim">
                        {hex(p.offset)}–{hex(p.offset + size)}
                      </td>
                      <td className="num">{humanBytes(size)}</td>
                      <td>{p.image ?? <span className="muted">–</span>}</td>
                      <td className="num">{p.content_bytes !== null ? humanBytes(p.content_bytes) : "–"}</td>
                      <td className="num">{p.used_bytes !== null ? humanBytes(p.used_bytes) : "–"}</td>
                      <td className="num">{size > 0 && !p.overlaps ? humanBytes(Math.max(0, size - used)) : "–"}</td>
                      <td className="num">{size > 0 && !p.overlaps ? pct(frac) : "–"}</td>
                      <td><VerifiedMark v={p.verified} /></td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </>
      ) : (
        <div className="panel">
          <div className="panel-head">
            <span className="panel-title">Flash map</span>
          </div>
          <div className="empty">No partition layout found for this build (no mtdparts source or partition table). Artifact sizes below are still exact.</div>
        </div>
      )}

      <div className="panel">
        <div className="panel-head">
          <span className="panel-title">images/</span>
          <span className="muted">{report.images.length} files</span>
        </div>
        <table className="tbl">
          <thead>
            <tr>
              <th>file</th>
              <th className="num">size</th>
              <th>format</th>
              <th>partition</th>
              <th>introspection</th>
            </tr>
          </thead>
          <tbody>
            {[...report.images]
              .sort((a, b) => b.bytes - a.bytes)
              .map((i) => (
                <tr key={i.name}>
                  <td>{i.name}</td>
                  <td className="num">{humanBytes(i.bytes)}</td>
                  <td>
                    <span className={`chip fmt-${i.format}`}>{i.format}</span>
                  </td>
                  <td>{i.partition ?? <span className="muted">–</span>}</td>
                  <td className="mono-dim">{imageNote(i)}</td>
                </tr>
              ))}
          </tbody>
        </table>
      </div>

      {report.scan.warnings.length > 0 && (
        <div className="panel">
          <div className="panel-head">
            <span className="panel-title">Warnings</span>
          </div>
          <ul className="warnings">
            {report.scan.warnings.map((w, i) => (
              <Fragment key={i}>
                <li>{w}</li>
              </Fragment>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
