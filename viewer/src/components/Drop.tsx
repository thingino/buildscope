import { useCallback, useRef, useState } from "react";
import { parseReportJson } from "../data";
import {
  groupByRoot,
  isArtifactName,
  looksLikeBuild,
  readDirectoryEntry,
  scanArtifact,
  scanPickedTree,
} from "../scan";
import { Report } from "../types";

type Picked = { path: string; file: File };

export default function Drop({ onReports }: { onReports: (r: Report[]) => void }) {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [over, setOver] = useState(false);
  const filesRef = useRef<HTMLInputElement | null>(null);
  const dirRef = useRef<HTMLInputElement | null>(null);

  /** Route a set of picked paths: build directories, images, or reports. */
  const ingest = useCallback(
    async (picked: Picked[]) => {
      setError(null);
      const reports: Report[] = [];
      const problems: string[] = [];

      const groups = groupByRoot(picked);
      const loose = groups.get("") ?? [];
      groups.delete("");

      try {
        // Directories: scan each one that looks like a build.
        for (const [root, files] of groups) {
          if (!looksLikeBuild(files)) {
            // A directory of firmware images rather than a build tree.
            const artifacts = files.filter((f) => isArtifactName(f.file.name));
            if (artifacts.length === 0) {
              problems.push(`${root}: not a Buildroot output directory`);
              continue;
            }
            for (const a of artifacts) {
              setBusy(`analyzing ${a.file.name}`);
              try {
                reports.push(await scanArtifact(a.file));
              } catch (e) {
                problems.push(`${a.file.name}: ${msg(e)}`);
              }
            }
            continue;
          }
          setBusy(`scanning ${root}`);
          try {
            reports.push(
              await scanPickedTree(root, files, (stage) => setBusy(`${root}: ${stage}`))
            );
          } catch (e) {
            problems.push(`${root}: ${msg(e)}`);
          }
        }

        // Loose files: reports as-is, everything else as a firmware image.
        for (const { file } of loose) {
          if (/\.json$/i.test(file.name)) {
            try {
              reports.push(parseReportJson(await file.text()));
            } catch (e) {
              problems.push(`${file.name}: ${msg(e)}`);
            }
            continue;
          }
          if (!isArtifactName(file.name)) continue;
          setBusy(`analyzing ${file.name}`);
          try {
            reports.push(await scanArtifact(file));
          } catch (e) {
            problems.push(`${file.name}: ${msg(e)}`);
          }
        }
      } finally {
        setBusy(null);
      }

      if (reports.length > 0) onReports(reports);
      setError(problems.length > 0 ? problems.join(" · ") : null);
    },
    [onReports]
  );

  const onDrop = useCallback(
    async (e: React.DragEvent) => {
      e.preventDefault();
      setOver(false);
      const items = Array.from(e.dataTransfer.items ?? []);
      const entries = items
        .map((i) => (i.kind === "file" && i.webkitGetAsEntry ? i.webkitGetAsEntry() : null))
        .filter((x): x is FileSystemEntry => x !== null);

      if (entries.length > 0) {
        const picked: Picked[] = [];
        for (const entry of entries) {
          if (entry.isDirectory) {
            setBusy(`reading ${entry.name}`);
            const files = await readDirectoryEntry(entry as FileSystemDirectoryEntry);
            picked.push(...files.map((f) => ({ path: `${entry.name}/${f.path}`, file: f.file })));
          } else {
            const file = await new Promise<File>((resolve, reject) =>
              (entry as FileSystemFileEntry).file(resolve, reject)
            );
            picked.push({ path: file.name, file });
          }
        }
        await ingest(picked);
        return;
      }
      // Browsers without entry support still give us a flat file list.
      await ingest(Array.from(e.dataTransfer.files).map((f) => ({ path: f.name, file: f })));
    },
    [ingest]
  );

  const fromInput = useCallback(
    async (list: FileList | null) => {
      if (!list) return;
      await ingest(
        Array.from(list).map((f) => ({
          path: (f as File & { webkitRelativePath?: string }).webkitRelativePath || f.name,
          file: f,
        }))
      );
    },
    [ingest]
  );

  return (
    <div
      className={`drop ${over ? "over" : ""}`}
      onDragOver={(e) => {
        e.preventDefault();
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => void onDrop(e)}
    >
      <div className="drop-glyph">▤</div>
      <div className="drop-title">
        {busy ? busy : "drop a build directory, a firmware image, or a report"}
      </div>
      <div className="drop-sub">
        Everything is analyzed in this browser: a Buildroot output directory gets the full
        breakdown, a bare <code>.bin</code> gets its partition map carved out of the image
        itself, and a <code>buildscope-report.json</code> is rendered as-is. No uploads.
      </div>
      <div className="drop-actions">
        <button className="btn" disabled={busy !== null} onClick={() => dirRef.current?.click()}>
          choose build directory
        </button>
        <button
          className="btn btn-quiet"
          disabled={busy !== null}
          onClick={() => filesRef.current?.click()}
        >
          choose files
        </button>
      </div>
      {busy && <div className="drop-busy">working: {busy}</div>}
      <input
        ref={dirRef}
        type="file"
        hidden
        multiple
        // Directory picking is a non-standard but universally shipped input
        // attribute; React needs it spelled this way.
        {...({ webkitdirectory: "", directory: "" } as Record<string, string>)}
        onChange={(e) => void fromInput(e.target.files)}
      />
      <input
        ref={filesRef}
        type="file"
        hidden
        multiple
        onChange={(e) => void fromInput(e.target.files)}
      />
      {error && <div className="drop-error">{error}</div>}
    </div>
  );
}

function msg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
