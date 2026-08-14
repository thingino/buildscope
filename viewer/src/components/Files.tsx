import { useMemo, useState } from "react";
import { CATEGORY_COLOR, categorize } from "../categorize";
import { humanBytes } from "../format";
import { useT } from "../i18n";
import { Report, UNATTRIBUTED } from "../types";

/** A file as the browser needs it: where it is, how big, who put it there. */
interface Entry {
  path: string;
  bytes: number;
  /** What the file costs once compressed, when the image could be read. */
  compressed?: number;
  /** Owning package, when the source knows one. */
  pkg?: string;
  /** Directory entries carry no size of their own. */
  isDir?: boolean;
}

interface Source {
  id: string;
  label: string;
  entries: Entry[];
  truncated: boolean;
}

/** A directory in the browsable tree, with its subtree total. */
interface Node {
  name: string;
  path: string;
  bytes: number;
  /** Subtree total of the compressed cost, where it is known. */
  compressed: number;
  files: number;
  children: Map<string, Node>;
  leaf?: Entry;
}

function emptyNode(name: string, path: string): Node {
  return { name, path, bytes: 0, compressed: 0, files: 0, children: new Map() };
}

/** Fold a flat path list into a tree, summing sizes up every level. */
function buildTree(entries: Entry[]): Node {
  const root = emptyNode("/", "");
  for (const e of entries) {
    const parts = e.path.split("/").filter(Boolean);
    let node = root;
    if (!e.isDir) {
      root.bytes += e.bytes;
      root.compressed += e.compressed ?? 0;
      root.files += 1;
    }
    for (let i = 0; i < parts.length; i++) {
      const last = i === parts.length - 1;
      const path = "/" + parts.slice(0, i + 1).join("/");
      let child = node.children.get(parts[i]);
      if (!child) {
        child = emptyNode(parts[i], path);
        node.children.set(parts[i], child);
      }
      if (last && !e.isDir) {
        child.leaf = e;
        child.bytes += e.bytes;
        child.compressed += e.compressed ?? 0;
        child.files += 1;
      } else if (!last) {
        child.bytes += e.bytes;
        child.compressed += e.compressed ?? 0;
        child.files += e.isDir ? 0 : 1;
      }
      node = child;
    }
  }
  return root;
}

function sortedChildren(node: Node): Node[] {
  return [...node.children.values()].sort((a, b) => {
    const ad = a.children.size > 0 ? 0 : 1;
    const bd = b.children.size > 0 ? 0 : 1;
    if (ad !== bd) return ad - bd; // directories first
    return b.bytes - a.bytes || a.name.localeCompare(b.name);
  });
}

export default function Files({ report }: { report: Report }) {
  const t = useT();

  // Where a listing can come from: the attributed rootfs walk, and any image
  // that reconstructed its own contents (jffs2 does, from its nodes).
  const sources = useMemo<Source[]>(() => {
    const out: Source[] = [];
    const rootfsEntries: Entry[] = [];
    let rootfsTruncated = false;
    for (const p of report.packages) {
      if (p.files_truncated) rootfsTruncated = true;
      for (const f of p.files ?? p.top_files ?? []) {
        rootfsEntries.push({
          path: f.path,
          bytes: f.bytes,
          compressed: f.compressed_bytes,
          pkg: p.name,
        });
      }
    }
    if (rootfsEntries.length > 0) {
      out.push({
        id: "rootfs",
        label: t("files_source_rootfs"),
        entries: rootfsEntries,
        truncated: rootfsTruncated,
      });
    }
    for (const img of report.images) {
      const d = img.detail as {
        entries?: { path: string; bytes: number; kind: string; compressed_bytes?: number }[];
        entries_truncated?: boolean;
      };
      if (!Array.isArray(d.entries) || d.entries.length === 0) continue;
      out.push({
        id: img.name,
        label: img.partition ? `${img.partition} (${img.format})` : `${img.name} (${img.format})`,
        entries: d.entries.map((e) => ({
          path: e.path,
          bytes: e.bytes,
          compressed: e.compressed_bytes,
          isDir: e.kind === "dir",
        })),
        truncated: d.entries_truncated === true,
      });
    }
    return out;
  }, [report, t]);

  const [sourceId, setSourceId] = useState(sources[0]?.id ?? "");
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<Set<string>>(new Set([""]));

  const source = sources.find((s) => s.id === sourceId) ?? sources[0];

  const tree = useMemo(() => {
    if (!source) return emptyNode("/", "");
    const q = query.trim().toLowerCase();
    const entries = q ? source.entries.filter((e) => e.path.toLowerCase().includes(q)) : source.entries;
    return buildTree(entries);
  }, [source, query]);

  // A filtered view is most useful fully expanded; the unfiltered one starts
  // collapsed so a thousand-file rootfs is approachable.
  const filtering = query.trim() !== "";
  const isOpen = (path: string) => filtering || open.has(path);

  const toggle = (path: string) =>
    setOpen((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });

  if (!source) {
    return (
      <div className="pane">
        <div className="panel">
          <div className="empty">{t("no_files")}</div>
        </div>
      </div>
    );
  }

  // Only offer the column when something in this listing carries the cost.
  const hasCost = source.entries.some((e) => e.compressed !== undefined);

  const rows: JSX.Element[] = [];
  const walk = (node: Node, depth: number) => {
    for (const child of sortedChildren(node)) {
      const dir = child.children.size > 0;
      const shown = isOpen(child.path);
      const share = tree.bytes > 0 ? child.bytes / tree.bytes : 0;
      const pkg = child.leaf?.pkg;
      rows.push(
        <tr key={child.path} className={dir ? "rowlink" : ""} onClick={dir ? () => toggle(child.path) : undefined}>
          <td>
            <span className="tree-name" style={{ paddingInlineStart: `${depth * 14}px` }}>
              {dir ? <span className="tree-caret">{shown ? "▾" : "▸"}</span> : <span className="tree-caret" />}
              {dir ? <span className="tree-dir">{child.name}/</span> : child.name}
            </span>
          </td>
          <td className="num">{humanBytes(child.bytes)}</td>
          {hasCost && (
            <td className="num">{child.compressed > 0 ? humanBytes(child.compressed) : "–"}</td>
          )}
          <td className="num">{dir ? child.files : ""}</td>
          <td>
            {pkg && (
              <span className="pkg-chip machine">
                <span className="dot" style={{ background: CATEGORY_COLOR[categorize(pkg)] }} />
                {pkg === UNATTRIBUTED ? t("overlay_post_build") : pkg}
              </span>
            )}
          </td>
          <td className="bar-col">
            <div className="minibar">
              {/* Directories get the neutral accent; a file is tinted by the
                  category of the package that installed it, matching the
                  treemap and the package table. */}
              <div
                className="minibar-fill"
                style={{
                  width: `${Math.min(share, 1) * 100}%`,
                  background: pkg ? CATEGORY_COLOR[categorize(pkg)] : "var(--accent)",
                }}
              />
            </div>
          </td>
        </tr>
      );
      if (dir && shown) walk(child, depth + 1);
    }
  };
  walk(tree, 0);

  return (
    <div className="pane">
      <div className="statrow">
        <div className="stat">
          <div className="stat-label">{t("files_total")}</div>
          <div className="stat-value">{tree.files}</div>
        </div>
        <div className="stat">
          <div className="stat-label">{t("files_bytes")}</div>
          <div className="stat-value">{humanBytes(tree.bytes)}</div>
        </div>
        {hasCost && (
          <div className="stat">
            <div className="stat-label">{t("th_on_flash")}</div>
            <div className="stat-value">{humanBytes(tree.compressed)}</div>
          </div>
        )}
        {source.id === "rootfs" && report.rootfs && (
          <div className="stat">
            <div className="stat-label">{t("stat_compressed", { algo: report.rootfs.compression ?? "?" })}</div>
            <div className="stat-value">
              {report.rootfs.compressed_bytes ? humanBytes(report.rootfs.compressed_bytes) : "–"}
              {report.rootfs.compression_ratio && (
                <span className="stat-sub"> ×{report.rootfs.compression_ratio.toFixed(3)}</span>
              )}
            </div>
          </div>
        )}
      </div>

      <div className="controls">
        {sources.length > 1 && (
          <select className="select" value={source.id} onChange={(e) => setSourceId(e.target.value)}>
            {sources.map((s) => (
              <option key={s.id} value={s.id}>
                {s.label}
              </option>
            ))}
          </select>
        )}
        <input
          className="search"
          placeholder={t("filter_paths")}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        {filtering && <span className="muted">{t("files_matching", { n: tree.files })}</span>}
      </div>

      <div className="panel">
        <div className="panel-head">
          <span className="panel-title">{t("files_title")}</span>
          {/* Which listing this is. More than one can exist: the attributed
              rootfs walk, and any image that rebuilt its own contents. */}
          <span className="muted">{source.label}</span>
        </div>
        {source.truncated && <div className="muted trunc-note">{t("files_capped")}</div>}
        <div className="tbl-wrap">
          <table className="tbl">
            <thead>
              <tr>
                <th>{t("th_path")}</th>
                <th className="num">{t("th_size")}</th>
                {hasCost && <th className="num">{t("th_on_flash")}</th>}
                <th className="num">{t("th_files")}</th>
                <th>{t("th_package")}</th>
                <th className="bar-col">{t("th_share")}</th>
              </tr>
            </thead>
            <tbody>{rows}</tbody>
          </table>
        </div>
        {rows.length === 0 && <div className="empty">{t("no_matches")}</div>}
      </div>
    </div>
  );
}
