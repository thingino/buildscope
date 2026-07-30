# buildscope viewer

Static single-page viewer for buildscope reports. React + vite, no other
runtime dependencies; the treemap is hand-rolled. Dark instrument-panel
theme; categorical and status colors are a CVD-validated palette.

## Data sources

1. **Served**: `buildscope serve <dirs>` exposes `/api/index` and
   `/api/report/<n>` and serves this bundle; the app picks the API up
   automatically.
2. **Static**: without an API the app becomes a drop target; drag any
   `buildscope-report.json` (several at once for multi-build browsing).

## Develop / build

```
npm install
npm run dev      # against a running `buildscope serve` on another port? use static drop mode
npm run build    # dist/ (picked up automatically by `buildscope serve`)
```

`buildscope serve` looks for the bundle at `viewer/dist` relative to the
working directory, next to the binary, or via `--viewer-dir`.
