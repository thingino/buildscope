import { useCallback, useRef, useState } from "react";
import { parseReportJson } from "../data";
import { Report } from "../types";

export default function Drop({ onReports }: { onReports: (r: Report[]) => void }) {
  const [error, setError] = useState<string | null>(null);
  const [over, setOver] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const ingest = useCallback(
    async (files: FileList | File[]) => {
      const reports: Report[] = [];
      const errors: string[] = [];
      for (const f of Array.from(files)) {
        try {
          reports.push(parseReportJson(await f.text()));
        } catch (e) {
          errors.push(`${f.name}: ${e instanceof Error ? e.message : String(e)}`);
        }
      }
      if (reports.length > 0) onReports(reports);
      setError(errors.length > 0 ? errors.join("; ") : null);
    },
    [onReports]
  );

  return (
    <div
      className={`drop ${over ? "over" : ""}`}
      onDragOver={(e) => {
        e.preventDefault();
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        e.preventDefault();
        setOver(false);
        void ingest(e.dataTransfer.files);
      }}
    >
      <div className="drop-glyph">▤</div>
      <div className="drop-title">drop a report.json</div>
      <div className="drop-sub">
        Generate one with <code>buildscope scan &lt;output-dir&gt;</code>, then drop the
        <code> buildscope-report.json</code> from images/ here. Multiple files welcome.
      </div>
      <button className="btn" onClick={() => inputRef.current?.click()}>
        choose files
      </button>
      <input
        ref={inputRef}
        type="file"
        accept=".json,application/json"
        multiple
        hidden
        onChange={(e) => e.target.files && void ingest(e.target.files)}
      />
      {error && <div className="drop-error">{error}</div>}
    </div>
  );
}
