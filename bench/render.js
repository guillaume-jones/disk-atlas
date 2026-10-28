// Render benchmark. Paste into devtools (or evaluate via automation) on http://localhost:1420 once a survey has loaded.
// Measures requestAnimationFrame intervals per phase; 8.3 ms is the cap on a 120 Hz display.
async () => {
  const { camera, session } = window.atlas;
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
  const zoomAtCentre = (factor) => camera.zoomAt(camera.width / 2, camera.height / 2, factor);
  const panBackAndForth = (i) => camera.panBy((i % 30 < 15 ? 1 : -1) * 6, 0);
  const land = [...session.layers.labels.values()]
    .map(({ item }) => item)
    .filter((label) => session.survey.region(label.id).kind === "continent")
    .sort((a, b) => b.cells - a.cells)[0];

  camera.fit();
  await settle(1500);
  const results = {};
  results.panFitted = await measure(90, panBackAndForth);
  camera.jumpTo(land.x, land.y, camera.scale);
  results.zoomIn = await measure(62, () => zoomAtCentre(1.05));
  await settle(4000);
  results.panZoomed = await measure(90, panBackAndForth);
  results.zoomOut = await measure(62, () => zoomAtCentre(1 / 1.05));
  results.loaded = { patches: session.layers.patches.size, tiles: session.layers.tiles.size };
  return results;
};
