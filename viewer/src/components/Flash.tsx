import { Fragment } from "react";
import { hex, humanBytes, pct } from "../format";
import { TFn, useT } from "../i18n";
import { ImageReport, PartitionReport, Report, UbiDetail } from "../types";
import { useTooltip } from "../tooltip";

function fillStatus(frac: number): string {
  if (frac > 1.0) return "crit";
  if (frac > 0.95) return "serious";
  if (frac > 0.85) return "warn";
  return "good";
}

function VerifiedMark({ v, t }: { v: boolean | null; t: TFn }) {
  if (v === null) return <span className="muted">–</span>;
  return v ? (
    <span className="ok" title={t("verified_title")}>
      ✓ {t("verified")}
    </span>
  ) : (
    <span className="crit" title={t("mismatch_title")}>
      ✕ {t("mismatch")}
    </span>
  );
}

function imageNote(i: ImageReport): string {
  const d = i.detail as Record<string, number | string | boolean>;
  switch (i.format) {
    case "squashfs":
      return `${humanBytes(d.bytes_used as number)} used · ${d.compression} · ${d.inode_count} inodes`;
    case "jffs2":
      return `${humanBytes(d.used_bytes as number)} used · ${humanBytes(d.free_bytes as number)} free · ${d.live_files} files`;
    case "uimage": {
      const trees = (d.builtin_device_trees ?? []) as unknown as { model: string; compatible: string[] }[];
      const dt = Array.isArray(trees) && trees.length
        ? ` · dtb ${trees[0].model || trees[0].compatible?.[0] || "?"}`
        : "";
      return (
        `${d.type} · ${d.compression} · payload ${humanBytes(d.declared_size as number)} · load ${d.load_addr}` +
        (d.payload_uncompressed_bytes
          ? ` · ${humanBytes(d.payload_uncompressed_bytes as number)} uncompressed`
          : "") +
        dt
      );
    }
    case "uboot-env":
      return `${humanBytes(d.used_bytes as number)} of ${humanBytes(i.bytes)} used · crc ${d.crc_ok ? "ok" : "BAD"} · ${d.var_count} vars`;
    case "ubi": {
      const spare = (d.free_pebs as number) + (d.erased_pebs as number);
      const bad = d.bad_pebs as number;
      return (
        `${humanBytes(d.used_bytes as number)} used · ${humanBytes(d.peb_size as number)} PEB` +
        (spare > 0 ? ` · ${spare} spare` : "") +
        (bad > 0 ? ` · ${bad} bad` : "")
      );
    }
    case "ext2":
    case "ext3":
    case "ext4":
      return (
        `${humanBytes(d.used_bytes as number)} used · ${humanBytes(d.free_bytes as number)} free · ` +
        `${humanBytes(d.block_size as number)} blocks · ${d.inode_count} inodes` +
        (d.label ? ` · ${d.label}` : "") +
        (d.clean === false ? " · NOT CLEAN" : "")
      );
    case "fat12":
    case "fat16":
    case "fat32":
      return (
        `${humanBytes(d.used_bytes as number)} used · ${humanBytes(d.free_bytes as number)} free · ` +
        `${humanBytes(d.cluster_bytes as number)} clusters` +
        (d.label ? ` · ${d.label}` : "")
      );
    case "cpio":
      return `${d.entry_count} entries · ${humanBytes(d.content_bytes as number)} of content · ${d.cpio_format}`;
    case "fit": {
      const imgs = (d.images ?? []) as unknown as { type: string }[];
      const types = Array.isArray(imgs) ? imgs.map((x) => x.type).join(", ") : "";
      return `${Array.isArray(imgs) ? imgs.length : 0} images (${types}) · ${humanBytes(
        d.payload_bytes as number
      )} of payload`;
    }
    case "dtb":
    case "dtbo": {
      const compat = (d.compatible ?? []) as unknown as string[];
      const targets = (d.overlay_targets ?? []) as unknown as string[];
      const who =
        (d.model as string) ||
        (Array.isArray(compat) && compat.length ? compat[0] : "") ||
        (Array.isArray(targets) && targets.length ? `patches ${targets.join(", ")}` : "device tree");
      return `${who} · ${d.node_count} nodes${d.bootargs ? " · has bootargs" : ""}`;
    }
    case "ubifs":
      return (
        `${humanBytes(d.total_bytes as number)} · ${d.leb_count} blocks · ${d.compression}` +
        (d.live_files ? ` · ${d.live_files} files` : "") +
        (d.autoresize_pending ? ` · grows to ${humanBytes(d.max_bytes as number)}` : "")
      );
    case "disk-image": {
      const n = Array.isArray(d.partitions) ? (d.partitions as unknown[]).length : 0;
      return `${d.table ?? "?"} · ${n} partitions`;
    }
    case "flash-image":
      return `content to ${humanBytes(d.content_end as number)}`;
    case "raw": {
      const trees = (d.device_trees ?? []) as unknown as { model: string; compatible: string[] }[];
      if (Array.isArray(trees) && trees.length) {
        const who = trees[0].model || trees[0].compatible?.[0] || "device tree";
        return `carries a dtb: ${who}`;
      }
      return d.trailing_padding && (d.trailing_padding as number) > 0
        ? `content ${humanBytes(d.content_end as number)} + ${humanBytes(d.trailing_padding as number)} padding`
        : "";
    }
    default:
      return "";
  }
}

function DieMap({ parts, total }: { parts: PartitionReport[]; total: number }) {
  const { node, show, hide } = useTooltip();
  const t = useT();
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
                    <div>{t("tt_partition")} {humanBytes(size)}</div>
                    {p.image && (
                      <div>
                        {t("tt_image")} {p.image} ({humanBytes(p.content_bytes ?? 0)})
                      </div>
                    )}
                    <div>{t("tt_used")} {humanBytes(used)} ({pct(frac)})</div>
                    <div>{t("tt_free")} {humanBytes(Math.max(0, size - used))}</div>
                  </div>
                )
              }
              onClick={(e) =>
                show(
                  e,
                  <div>
                    <div className="tt-title">{p.name}{p.read_only ? " (ro)" : ""}</div>
                    <div>{hex(p.offset)} – {hex(p.offset + size)}</div>
                    <div>{t("tt_partition")} {humanBytes(size)}</div>
                    {p.image && (
                      <div>
                        {t("tt_image")} {p.image} ({humanBytes(p.content_bytes ?? 0)})
                      </div>
                    )}
                    <div>{t("tt_used")} {humanBytes(used)} ({pct(frac)})</div>
                    <div>{t("tt_free")} {humanBytes(Math.max(0, size - used))}</div>
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
        <span className="muted diemap-note">{t("diemap_note")}</span>
        <span>{hex(total)}</span>
      </div>
      {node}
    </div>
  );
}

/// A UBI area's volumes. This is the only place a volume the image reserved but
/// never wrote to shows up: it has no location, so the flash map cannot hold it.
function UbiVolumes({ img, t }: { img: ImageReport; t: TFn }) {
  const d = img.detail as unknown as UbiDetail;
  if (!Array.isArray(d.volumes) || d.volumes.length === 0) return null;
  const spare = d.free_pebs + d.erased_pebs;
  return (
    <div className="panel">
      <div className="panel-head">
        <span className="panel-title">
          {t("ubi_volumes")} <span className="muted">{img.name}</span>
        </span>
        <span className="muted">
          {humanBytes(d.peb_size)} PEB · {humanBytes(d.leb_size)} LEB ·{" "}
          {d.mapped_pebs}/{d.total_pebs} {t("th_blocks")}
          {spare > 0 ? ` · ${spare} spare` : ""}
          {d.bad_pebs > 0 ? ` · ${d.bad_pebs} bad` : ""} · @ {hex(d.ubi_offset)}
        </span>
      </div>
      <div className="tbl-wrap">
        <table className="tbl">
          <thead>
            <tr>
              <th>{t("th_volume")}</th>
              <th>{t("th_type")}</th>
              <th>{t("th_range")}</th>
              <th className="num">{t("th_blocks")}</th>
              <th className="num">{t("th_reserved")}</th>
              <th className="num">{t("th_payload")}</th>
              <th className="num">{t("th_on_flash")}</th>
              <th className="num">{t("th_fill")}</th>
            </tr>
          </thead>
          <tbody>
            {d.volumes.map((v) => {
              const frac = v.capacity_bytes > 0 ? v.bytes / v.capacity_bytes : 0;
              const placed = v.offset !== null;
              return (
                <tr key={v.id} className={placed ? "" : "dim"}>
                  <td>
                    {v.name || `vol${v.id}`}
                    <span className="muted"> id {v.id}</span>
                    {v.autoresize && <span className="chip"> autoresize</span>}
                    {v.has_holes && <span className="crit"> {t("volume_holes")}</span>}
                  </td>
                  <td className="mono-dim">{v.type}</td>
                  <td className="mono-dim">
                    {placed ? (
                      <>
                        {hex(v.offset as number)}–{hex((v.offset as number) + v.flash_bytes)}
                      </>
                    ) : (
                      <span className="muted">{t("volume_unwritten")}</span>
                    )}
                  </td>
                  <td className="num">
                    {v.mapped_pebs}
                    <span className="muted">/{v.reserved_pebs}</span>
                  </td>
                  <td className="num">{humanBytes(v.capacity_bytes)}</td>
                  <td className="num">{placed ? humanBytes(v.bytes) : "–"}</td>
                  <td className="num">{placed ? humanBytes(v.flash_bytes) : "–"}</td>
                  <td className="num">{placed ? pct(frac) : "–"}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <div className="muted trunc-note">{t("ubi_volumes_note")}</div>
    </div>
  );
}

export default function Flash({ report }: { report: Report }) {
  const t = useT();
  const flash = report.flash;
  return (
    <div className="pane">
      {flash && flash.total_bytes ? (
        <>
          <div className="panel">
            <div className="panel-head">
              <span className="panel-title">{t("flash_map")}</span>
              <span className="muted">
                {flash.mtd_id ?? t("device")} · {humanBytes(flash.total_bytes)} · {flash.source}
              </span>
            </div>
            <DieMap parts={flash.partitions} total={flash.total_bytes} />
            <div className="tbl-wrap">
              <table className="tbl">
              <thead>
                <tr>
                  <th>{t("th_partition")}</th>
                  <th>{t("th_range")}</th>
                  <th className="num">{t("th_size")}</th>
                  <th>{t("th_image")}</th>
                  <th className="num">{t("th_content")}</th>
                  <th className="num">{t("th_used")}</th>
                  <th className="num">{t("th_free")}</th>
                  <th className="num">{t("th_fill")}</th>
                  <th>{t("th_check")}</th>
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
                        {p.read_only ? <span className="muted"> {t("read_only_short")}</span> : null}
                        {p.overlaps ? <span className="muted"> {t("spans")}</span> : null}
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
                      <td><VerifiedMark v={p.verified} t={t} /></td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
              </div>
          </div>
        </>
      ) : (
        <div className="panel">
          <div className="panel-head">
            <span className="panel-title">{t("flash_map")}</span>
          </div>
          <div className="empty">{t("no_layout")}</div>
        </div>
      )}

      {report.images
        .filter((i) => i.format === "ubi")
        .map((i) => (
          <UbiVolumes key={i.name} img={i} t={t} />
        ))}

      <div className="panel">
        <div className="panel-head">
          <span className="panel-title">{t("images_dir")}</span>
          <span className="muted">{t("n_files", { n: report.images.length })}</span>
        </div>
        <div className="tbl-wrap">
              <table className="tbl">
          <thead>
            <tr>
              <th>{t("th_file")}</th>
              <th className="num">{t("th_size")}</th>
              <th>{t("th_format")}</th>
              <th>{t("th_partition")}</th>
              <th>{t("th_introspection")}</th>
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
      </div>

      {report.scan.warnings.length > 0 && (
        <div className="panel">
          <div className="panel-head">
            <span className="panel-title">{t("warnings_title")}</span>
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
