Benchmarks for scanning and rendering. Numbers are from an Apple Silicon MacBook, warm file cache.

`bench/scan.sh [root ...]` builds the release CLI and times scan + map generation, plus two region patches per root.

`bench/render.js` measures frame intervals (median / p95 ms) while panning and zooming. Run the dev stack against a survey of `~/Documents` (`npm run dev`, `cargo run --bin atlas -- serve 7878`, POST `/api/scan?root=...`), open http://localhost:1420, then evaluate the file's function in devtools. 8.3 ms is the 120 Hz vsync cap, so watch p95.

Baseline (before simplification pass):

| Scan + map    | Files | Time  | Patch builds |
| ------------- | ----- | ----- | ------------ |
| ~/Documents   | 549k  | 1.8 s | 0.07 s       |
| /Applications | 188k  | 0.6 s | n/a          |
| /opt/homebrew | 97k   | 0.3 s | 0.07–0.08 s  |

| Render phase                      | Median | p95  |
| --------------------------------- | ------ | ---- |
| Pan, fitted                       | 8.3    | 8.6  |
| Zoom in ×20                       | 8.2    | 9.9  |
| Pan, zoomed (10 patches, 6 tiles) | 8.3    | 10.3 |
| Zoom out                          | 8.7    | 12.6 |
