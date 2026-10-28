// Render benchmark. Paste into devtools (or evaluate via automation) on http://localhost:1420 once a survey has loaded.
// Measures requestAnimationFrame intervals per phase; 8.3 ms is the cap on a 120 Hz display.
async () => {
  const atlas = window.atlas;
  const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));
  const settle = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const stats = (intervals) => {
    const sorted = [...intervals].sort((a, b) => a - b);
    const at = (q) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))].toFixed(1);
    return { median: at(0.5), p95: at(0.95) };
  };
  const measure = async (steps, step) => {
    const intervals = [];
    let last = performance.now();
    for (let i = 0; i < steps; i++) {
      step(i);
      await frame();
      const now = performance.now();
      intervals.push(now - last);
      last = now;
    }
    return stats(intervals);
  };
  const centre = () => [atlas.view.w / 2, atlas.view.h / 2];
  const land = [...atlas.labels.values()]
    .map(({ l }) => l)
    .filter((l) => atlas.regions()[l.id].kind === "continent")
    .sort((a, b) => b.cells - a.cells)[0];

  atlas.fit();
  await settle(1500);
  const results = {};
  results.panFitted = await measure(90, (i) => {
    atlas.view.cx += ((i % 30 < 15 ? 1 : -1) * 6) / atlas.view.scale;
    atlas.zoomAt(...centre(), 1);
  });
  atlas.view.cx = land.x;
  atlas.view.cy = land.y;
  results.zoomIn = await measure(62, () => atlas.zoomAt(...centre(), 1.05));
  await settle(4000);
  results.panZoomed = await measure(90, (i) => {
    atlas.view.cx += ((i % 30 < 15 ? 1 : -1) * 6) / atlas.view.scale;
    atlas.zoomAt(...centre(), 1);
  });
  results.zoomOut = await measure(62, () => atlas.zoomAt(...centre(), 1 / 1.05));
  results.loaded = { patches: atlas.patches.size, tiles: atlas.tiles.size };
  return results;
};
