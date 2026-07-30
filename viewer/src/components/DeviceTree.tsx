import { useMemo, useState } from "react";
import { hex, humanBytes } from "../format";
import { useT } from "../i18n";
import { Report } from "../types";

interface Node {
  path: string;
  depth: number;
  properties: [string, string][];
}

interface Tree {
  id: string;
  /** The board it describes, or what it patches. */
  who: string;
  compatible: string[];
  /** The file it is, or the file it is buried in and where. */
  where: string;
  bytes: number;
  nodeCount: number;
  propCount: number;
  bootargs: string;
  nodes: Node[];
  truncated: boolean;
}

/** Every device tree in a report, whether it is a file or is inside one. */
export function collectTrees(report: Report): Tree[] {
  const out: Tree[] = [];
  const name = (v: { model?: string; compatible?: string[] }) =>
    v.model || v.compatible?.[0] || "device tree";

  for (const img of report.images) {
    const d = img.detail as Record<string, unknown>;
    if (img.format === "dtb" || img.format === "dtbo") {
      const v = d as { model?: string; compatible?: string[]; bootargs?: string };
      out.push({
        id: img.name,
        who: name(v),
        compatible: v.compatible ?? [],
        where: img.name,
        bytes: (d.total_bytes as number) ?? img.bytes,
        nodeCount: (d.node_count as number) ?? 0,
        propCount: (d.property_count as number) ?? 0,
        bootargs: v.bootargs ?? "",
        nodes: (d.nodes as Node[]) ?? [],
        truncated: d.nodes_truncated === true,
      });
    }
    for (const key of ["builtin_device_trees", "device_trees"]) {
      const found = d[key];
      if (!Array.isArray(found)) continue;
      for (const v of found as (Node & Record<string, never>)[] as unknown as {
        model?: string;
        compatible?: string[];
        bootargs?: string;
        bytes: number;
        offset: number;
        node_count: number;
        property_count: number;
        nodes: Node[];
        nodes_truncated: boolean;
      }[]) {
        out.push({
          id: `${img.name}@${v.offset}`,
          who: name(v),
          compatible: v.compatible ?? [],
          where: `${img.name} ${hex(v.offset)}`,
          bytes: v.bytes,
          nodeCount: v.node_count,
          propCount: v.property_count,
          bootargs: v.bootargs ?? "",
          nodes: v.nodes ?? [],
          truncated: v.nodes_truncated === true,
        });
      }
    }
  }
  return out;
}

/// The tree as source, the way anyone who works with device trees reads one.
/// Nodes carry their depth, so the indentation and the closing braces come
/// from the data rather than from re-parsing paths.
function Source({ nodes, query }: { nodes: Node[]; query: string }) {
  const lines = useMemo(() => {
    const q = query.trim().toLowerCase();
    const match = (n: Node) =>
      !q ||
      n.path.toLowerCase().includes(q) ||
      n.properties.some(
        ([k, v]) => k.toLowerCase().includes(q) || v.toLowerCase().includes(q)
      );
    const shown = q ? nodes.filter(match) : nodes;

    const out: { text: string; kind: string; depth: number }[] = [];
    let open: number[] = [];
    for (const n of shown) {
      // Close every node we have left behind.
      while (open.length && open[open.length - 1] >= n.depth) {
        out.push({ text: "};", kind: "brace", depth: open.pop() as number });
      }
      const label = n.path === "/" ? "/" : n.path.slice(n.path.lastIndexOf("/") + 1);
      out.push({ text: `${label} {`, kind: "node", depth: n.depth });
      for (const [k, v] of n.properties) {
        out.push({ text: v ? `${k} = ${v};` : `${k};`, kind: "prop", depth: n.depth + 1 });
      }
      open.push(n.depth);
    }
    while (open.length) out.push({ text: "};", kind: "brace", depth: open.pop() as number });
    return out;
  }, [nodes, query]);

  return (
    <div className="dts">
      {lines.map((l, i) => (
        <div key={i} className={`dts-line dts-${l.kind}`} style={{ paddingInlineStart: `${l.depth * 16}px` }}>
          {l.text}
        </div>
      ))}
    </div>
  );
}

export default function DeviceTree({ report }: { report: Report }) {
  const t = useT();
  const trees = useMemo(() => collectTrees(report), [report]);
  const [id, setId] = useState(trees[0]?.id ?? "");
  const [query, setQuery] = useState("");
  const tree = trees.find((x) => x.id === id) ?? trees[0];
  if (!tree) return null;

  return (
    <div className="pane">
      <div className="panel">
        <div className="panel-head">
          <span className="panel-title">{t("device_trees")}</span>
          <span className="muted">{t("n_trees", { n: trees.length })}</span>
        </div>
        <div className="tbl-wrap">
          <table className="tbl env-table">
            <thead>
              <tr>
                <th>{t("th_board")}</th>
                <th>{t("th_found_in")}</th>
                <th className="num">{t("th_size")}</th>
                <th className="num">{t("th_nodes")}</th>
                <th>{t("th_bootargs")}</th>
              </tr>
            </thead>
            <tbody>
              {trees.map((x) => (
                <tr
                  key={x.id}
                  className={`rowlink ${x.id === tree.id ? "" : "dim"}`}
                  onClick={() => setId(x.id)}
                >
                  <td>
                    <span className="env-key">{x.who}</span>
                    {x.compatible.length > 1 && (
                      <div className="mono-dim">{x.compatible.slice(1).join(", ")}</div>
                    )}
                  </td>
                  <td className="mono-dim">{x.where}</td>
                  <td className="num">{humanBytes(x.bytes)}</td>
                  <td className="num">{x.nodeCount}</td>
                  <td>
                    <span className="env-val">{x.bootargs}</span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="muted trunc-note">{t("device_trees_note")}</div>
      </div>

      <div className="controls">
        <input
          className="search"
          placeholder={t("filter_nodes")}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <span className="muted">
          {t("n_properties", { n: tree.propCount })}
        </span>
      </div>

      <div className="panel">
        <div className="panel-head">
          <span className="panel-title">{tree.who}</span>
          <span className="muted">{tree.where}</span>
        </div>
        <Source nodes={tree.nodes} query={query} />
        {tree.truncated && <div className="muted trunc-note">{t("dtb_capped")}</div>}
      </div>
    </div>
  );
}
