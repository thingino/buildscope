import { useCallback, useRef, useState } from "react";
import { parseReportJson } from "../data";
import {
  canPickDirectory,
  isArtifactName,
  pickDirectory,
  readDirectoryShallow,
  scanArtifact,
  scanDirectoryHandle,
} from "../scan";
import { useT } from "../i18n";
import { Report } from "../types";

export default function Drop({ onReports }: { onReports: (r: Report[]) => void }) {
  const t = useT();
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [over, setOver] = useState(false);
  const filesRef = useRef<HTMLInputElement | null>(null);

  /** Reports render as-is; every other file is analyzed as a firmware image. */
  const ingest = useCallback(
    async (files: File[]) => {
      setError(null);
      const reports: Report[] = [];
      const problems: string[] = [];
      try {
        for (const file of files) {
          if (/\.json$/i.test(file.name)) {
            try {
              reports.push(parseReportJson(await file.text()));
            } catch (e) {
              problems.push(`${file.name}: ${msg(e)}`);
            }
            continue;
          }
          if (!isArtifactName(file.name)) continue;
          setBusy(t("stage_analyzing_file", { name: file.name }));
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
    [onReports, t]
  );

  const pickBuild = useCallback(async () => {
    const handle = await pickDirectory();
    if (!handle) return; // cancelled
    setError(null);
    setBusy(t("stage_scanning", { name: handle.name }));
    try {
      const report = await scanDirectoryHandle(handle, (key, params) =>
        setBusy(t(key, params))
      );
      onReports([report]);
    } catch (e) {
      setError(msg(e));
    } finally {
      setBusy(null);
    }
  }, [onReports, t]);

  const onDrop = useCallback(
    async (e: React.DragEvent) => {
      e.preventDefault();
      setOver(false);
      const items = Array.from(e.dataTransfer.items ?? []);
      const entries = items
        .map((i) => (i.kind === "file" && i.webkitGetAsEntry ? i.webkitGetAsEntry() : null))
        .filter((x): x is FileSystemEntry => x !== null);

      if (entries.length > 0) {
        const picked: File[] = [];
        for (const entry of entries) {
          if (entry.isDirectory) {
            // Direct children only: a dropped folder is a place where images
            // sit side by side, and descending into a build tree would mean
            // enumerating hundreds of thousands of files nothing reads.
            setBusy(t("stage_reading", { name: entry.name }));
            const files = await readDirectoryShallow(entry as FileSystemDirectoryEntry);
            picked.push(...files.map((f) => f.file));
          } else {
            picked.push(
              await new Promise<File>((resolve, reject) =>
                (entry as FileSystemFileEntry).file(resolve, reject)
              )
            );
          }
        }
        await ingest(picked);
        return;
      }
      // Browsers without entry support still give us a flat file list.
      await ingest(Array.from(e.dataTransfer.files));
    },
    [ingest]
  );

  const fromInput = useCallback(
    async (list: FileList | null) => {
      if (list) await ingest(Array.from(list));
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
        {busy ? busy : t("drop_title")}
      </div>
      {/* Carries inline <code> markup, so it is set as HTML from the
          dictionary, which is trusted content shipped with the app. */}
      <div className="drop-sub" dangerouslySetInnerHTML={{ __html: t("drop_sub_html") }} />
      <div className="drop-actions">
        <button className="btn" disabled={busy !== null} onClick={() => filesRef.current?.click()}>
          {t("choose_files")}
        </button>
        {/* Opening a build directory needs a directory handle, which only
            Chromium-based browsers offer; elsewhere the button is absent
            rather than present and broken. */}
        {canPickDirectory() && (
          <button className="btn btn-quiet" disabled={busy !== null} onClick={() => void pickBuild()}>
            {t("choose_build_dir")}
          </button>
        )}
      </div>
      <div className="drop-hint" dangerouslySetInnerHTML={{ __html: t("drop_cli_hint") }} />
      {busy && <div className="drop-busy">{t("working", { what: busy })}</div>}
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
