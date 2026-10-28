import * as api from "./api";
import type { City, Label, PatchData, Region, Status, WorldMeta } from "./api";
import { BAND_COLORS, Terrain, type GLPatch, type View } from "./terrain";

const NONE = 0xffffffff;
const WORLD_W = 2;
const WORLD_H = 1;
/** Request a finer patch once a patch's cells are this many CSS pixels wide. */
const REFINE_PX = 4;
const MAX_ZOOM = 3000;
const MAX_TILES = 24;
/** Width of the frame margin holding the degree numbers, in CSS pixels. */
const FRAME = 16;
/** The side margins carry horizontal degree numbers, so they need more room than top and bottom. */
const FRAME_SIDE = 30;
/** Printed-key scale at the fitted zoom; it then grows with the map like everything else on the sheet. */
const KEY_SCALE = 0.8;
const OLD = `"Old Standard TT", Georgia, serif`;

type Box = [number, number, number, number];
/** Segments by style, then by cell of a grid over the patch, so only on-screen cells get stroked. */
type Cells = Map<number, { path: Path2D; box: Box }>;
type Strokes = Map<number, Cells>;
interface Loaded {
  d: PatchData;
  gl: GLPatch;
  borders: Strokes;
  rivers: Strokes;
  used: number;
}
const CHUNKS = 8;
const culled = new WeakMap<Cells, { key: string; path: Path2D }>();

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const glCanvas = $<HTMLCanvasElement>("terrain");
const ink = $<HTMLCanvasElement>("ink");
const ctx = ink.getContext("2d")!;
const terrain = new Terrain(glCanvas);

const patches = new Map<number, Loaded>();
const tiles = new Map<string, Loaded>();
const pending = new Set<string>();
const failed = new Set<string>();
const labels = new Map<number, { l: Label; depth: number }>();
const cities = new Map<number, { c: City; depth: number }>();
let meta: WorldMeta | null = null;
let regions: Region[] = [];
let home = "";
const view: View = { cx: 1, cy: 0.5, scale: 500, w: 1, h: 1, dpr: 1 };
let dirty = true;

const clamp = (v: number, a: number, b: number) => Math.min(b, Math.max(a, v));
/** Room below the map for the frame margin and the recency strip printed on the sheet. */
const KEY_SPACE = 100;
/** Room above the map for the New Survey slip. */
const TOP_SPACE = 64;
const fitScale = () => Math.min((view.w - 2 * (FRAME_SIDE + 14)) / WORLD_W, (view.h - KEY_SPACE - TOP_SPACE) / WORLD_H) * 0.96;
const fitCy = () => WORLD_H / 2 + (KEY_SPACE - TOP_SPACE) / 2 / fitScale();
const zoom = () => view.scale / fitScale();
const toScreen = (x: number, y: number) => [
  (x - view.cx) * view.scale + view.w / 2,
  (y - view.cy) * view.scale + view.h / 2,
];
const toWorld = (sx: number, sy: number) => [
  (sx - view.w / 2) / view.scale + view.cx,
  (sy - view.h / 2) / view.scale + view.cy,
];
const spaced = (s: string, n: number) => s.split("").join(" ".repeat(n));

function bytes(b: number) {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (b >= 1000 && i < units.length - 1) {
    b /= 1000;
    i++;
  }
  return `${b < 10 && i > 0 ? b.toFixed(1) : Math.round(b)} ${units[i]}`;
}

function ago(t: number) {
  if (!t || !meta) return "never";
  const d = (meta.now - t) / 86400;
  if (d < 1) return "today";
  if (d < 2) return "yesterday";
  if (d < 45) return `${Math.round(d)} days ago`;
  if (d < 540) return `${Math.round(d / 30)} months ago`;
  return `${(d / 365).toFixed(1)} years ago`;
}

function ancestors(id: number) {
  const chain: Region[] = [];
  for (let r = id; r !== NONE && r !== 0; r = regions[r].parent) chain.unshift(regions[r]);
  return chain;
}

const shortPath = (p: string) => (home && p.startsWith(home) ? "~" + p.slice(home.length) : p);

// ── Patches ─────────────────────────────────────────────────────────────

function strokes(d: PatchData, segs: Float32Array, style: (v: number) => number): Strokes {
  const out: Strokes = new Map();
  for (let i = 0; i < segs.length; i += 5) {
    const [ax, ay, bx, by] = segs.subarray(i, i + 4);
    const key = style(segs[i + 4]);
    let cells = out.get(key);
    if (!cells) out.set(key, (cells = new Map()));
    const cx = clamp(Math.floor(((ax - d.ox) / (d.cols * d.s)) * CHUNKS), 0, CHUNKS - 1);
    const cy = clamp(Math.floor(((ay - d.oy) / (d.rows * d.s)) * CHUNKS), 0, CHUNKS - 1);
    let c = cells.get(cy * CHUNKS + cx);
    if (!c) cells.set(cy * CHUNKS + cx, (c = { path: new Path2D(), box: [ax, ay, ax, ay] }));
    c.path.moveTo(ax, ay);
    c.path.lineTo(bx, by);
    const b = c.box;
    b[0] = Math.min(b[0], ax, bx);
    b[1] = Math.min(b[1], ay, by);
    b[2] = Math.max(b[2], ax, bx);
    b[3] = Math.max(b[3], ay, by);
  }
  return out;
}

function load(d: PatchData): Loaded {
  const borders = strokes(d, d.borders, (v) => Math.min(v, 5));
  const rivers = strokes(d, d.rivers, (v) => Math.min(8, Math.floor(v)));
  dirty = true;
  return { d, gl: terrain.upload(d), borders, rivers, used: performance.now() };
}

function addPatch(d: PatchData) {
  patches.set(d.root, load(d));
  for (const l of d.labels) {
    const cur = labels.get(l.id);
    if (!cur || cur.depth <= d.depth) labels.set(l.id, { l, depth: d.depth });
  }
  for (const c of d.cities) {
    const cur = cities.get(c.id);
    if (!cur || cur.depth <= d.depth) cities.set(c.id, { c, depth: d.depth });
  }
}

function addTile(key: string, d: PatchData) {
  tiles.set(key, load(d));
  if (tiles.size <= MAX_TILES) return;
  const [oldest, old] = [...tiles].sort((a, b) => a[1].used - b[1].used)[0];
  terrain.free(old.gl);
  tiles.delete(oldest);
}

const layers = () => [...patches.values(), ...tiles.values()].sort((a, b) => a.d.depth - b.d.depth);

function request(key: string, job: () => Promise<void>) {
  if (pending.has(key) || failed.has(key)) return;
  pending.add(key);
  job()
    .catch((err) => {
      failed.add(key);
      console.error(key, err);
    })
    .finally(() => {
      pending.delete(key);
      $("toast").hidden = pending.size === 0;
      refine();
    });
}

function refine() {
  const [x0, y0] = toWorld(0, 0);
  const [x1, y1] = toWorld(view.w, view.h);
  const visible = (b: number[]) => !(b[2] < x0 || b[0] > x1 || b[3] < y0 || b[1] > y1);
  for (const p of [...patches.values()]) {
    if (p.d.s * view.scale < REFINE_PX) continue;
    for (const id of regions[p.d.root].children) {
      const l = labels.get(id)?.l;
      const kind = regions[id].kind;
      if (!l || kind === "ocean" || kind === "isles" || patches.has(id) || !visible(l.bbox))
        continue;
      request(`r${id}`, () => api.patch(id).then(addPatch));
    }
  }
  // Below the region hierarchy, square tiles keep adding terrain detail.
  const centre = probe(view.cx, view.cy);
  const regionsLoading = [...pending].some((k) => k[0] === "r");
  if (
    centre &&
    regions[centre.cell].kind !== "ocean" &&
    centre.p.d.s * view.scale >= REFINE_PX &&
    !regionsLoading
  ) {
    const level = clamp(Math.ceil(Math.log2(view.scale / (REFINE_PX * 316))), 1, 24);
    const size = 2 ** -level;
    for (let ix = Math.floor(x0 / size); ix <= Math.floor(x1 / size); ix++) {
      for (let iy = Math.floor(y0 / size); iy <= Math.floor(y1 / size); iy++) {
        const key = `${level}/${ix}/${iy}`;
        const t = tiles.get(key);
        if (t) t.used = performance.now();
        else request(`t${key}`, () => api.tile(level, ix, iy).then((d) => addTile(key, d)));
      }
    }
  }
  $("toast").hidden = pending.size === 0;
}

/** The finest loaded layer with a cell at (x, y), and that cell's region. */
function probe(x: number, y: number): { p: Loaded; cell: number } | null {
  for (const p of layers().reverse()) {
    const d = p.d;
    const i0 = Math.floor((x - d.ox) / d.s);
    const j0 = Math.floor((y - d.oy) / d.s);
    let best = -1;
    let bd = Infinity;
    for (let j = Math.max(0, j0 - 2); j <= Math.min(d.rows - 1, j0 + 2); j++) {
      for (let i = Math.max(0, i0 - 2); i <= Math.min(d.cols - 1, i0 + 2); i++) {
        const k = j * d.cols + i;
        const dd = (d.pos[2 * k] - x) ** 2 + (d.pos[2 * k + 1] - y) ** 2;
        if (dd < bd) {
          bd = dd;
          best = k;
        }
      }
    }
    if (best >= 0 && d.cell[best] !== NONE && bd < (1.5 * d.s) ** 2)
      return { p, cell: d.cell[best] };
  }
  return null;
}

const pick = (x: number, y: number) => probe(x, y)?.cell ?? null;

// ── Drawing ─────────────────────────────────────────────────────────────

const BORDER_STYLES = [
  { color: "rgba(40,28,20,0.85)", w: 1.5, dash: [] as number[] },
  { color: "rgba(122,36,24,0.75)", w: 1.3, dash: [7, 2.5, 1.5, 2.5] },
  { color: "rgba(122,52,36,0.6)", w: 1, dash: [3, 2.5] },
  { color: "rgba(90,60,45,0.5)", w: 0.8, dash: [1, 2.5] },
];

/** Black type, no halos: bold block capitals for continents, capitals for nations, italics below. */
function labelStyle(r: Region, rs: number) {
  if (r.kind === "ocean") {
    const size = clamp(rs * 0.09, 13, 24);
    return { size, font: `italic ${size}px ${OLD}`, space: 5, upper: true };
  }
  if (r.depth === 1) {
    const size = clamp(rs * 0.17, 12, 30);
    return { size, font: `700 ${size}px ${OLD}`, space: 3, upper: true };
  }
  if (r.depth === 2) {
    const size = clamp(rs * 0.17, 10, 17);
    return { size, font: `${size}px ${OLD}`, space: 2, upper: true };
  }
  const size = clamp(rs * 0.22, 10, 14);
  return { size, font: `italic ${size}px ${OLD}`, space: 0, upper: false };
}

const overlaps = (boxes: Box[], b: Box) =>
  boxes.some((o) => b[0] < o[2] && b[2] > o[0] && b[1] < o[3] && b[3] > o[1]);

/** `map` is the map rectangle on screen; labels are nudged inside it so the frame never cuts them. */
function drawPlaces(z: number, maxLevel: number, map: Box) {
  const boxes: Box[] = [];
  const onScreen = (sx: number, sy: number) =>
    sx > -150 && sx < view.w + 150 && sy > -40 && sy < view.h + 40;
  ctx.textBaseline = "middle";
  ctx.lineJoin = "round";

  const items = [...labels.values()]
    .map(({ l }) => ({ l, r: regions[l.id] }))
    .filter(({ r }) => r.kind !== "isles" && r.depth <= maxLevel + 1)
    .sort((a, b) => a.r.depth - b.r.depth || b.l.r - a.l.r);
  for (const { l, r } of items) {
    const rs = l.r * view.scale;
    const thin = r.kind === "island" || r.kind === "polar" || r.kind === "unseen";
    if (rs < (thin ? 6 : 14) || rs > Math.max(view.w, view.h) * 0.9) continue;
    let [sx, sy] = toScreen(l.x, l.y);
    if (!onScreen(sx, sy)) continue;
    const st = labelStyle(r, rs);
    ctx.font = st.font;
    const text = spaced(st.upper ? r.name.toUpperCase() : r.name, st.space);
    const tw = ctx.measureText(text).width;
    const stretch = r.kind === "polar" || r.kind === "unseen" || r.kind === "ocean" ? 30 : r.kind === "island" ? 12 : 3.4;
    if (tw > rs * stretch) continue;
    sx = clamp(sx, map[0] + tw / 2 + 6, map[2] - tw / 2 - 6);
    sy = clamp(sy, map[1] + st.size, map[3] - st.size);
    const box: Box = [sx - tw / 2 - 4, sy - st.size * 0.65, sx + tw / 2 + 4, sy + st.size * 0.65];
    if (overlaps(boxes, box)) continue;
    boxes.push(box);
    ctx.textAlign = "center";
    ctx.fillStyle = "#000";
    ctx.fillText(text, sx, sy);
  }

  const minPop = 4000 / (z * z);
  const towns = [...cities.values()]
    .map(({ c }) => ({ c, r: regions[c.id] }))
    .sort((a, b) => b.c.pop - a.c.pop);
  for (const { c, r } of towns) {
    const rs = (labels.get(c.id)?.l.r ?? 0) * view.scale;
    if (!r.city || (c.pop < minPop && rs < 30)) continue;
    const [sx, sy] = toScreen(c.x, c.y);
    if (!onScreen(sx, sy)) continue;
    const mag = Math.log10(Math.max(c.pop, 10));
    const rad = 1.4 + 0.75 * mag;
    const size = 10.5 + Math.min(3.5, Math.max(0, mag - 2));
    ctx.font = `${mag > 4 ? "700 " : ""}${size}px ${OLD}`;
    const tw = ctx.measureText(r.city).width;
    const flip = sx + rad + 5 + tw > map[2] - 4;
    const box: Box = flip
      ? [sx - rad - 5 - tw, sy - size * 0.6, sx + rad + 1, sy + size * 0.6]
      : [sx - rad - 1, sy - size * 0.6, sx + rad + 5 + tw, sy + size * 0.6];
    if (overlaps(boxes, box)) continue;
    boxes.push(box);
    ctx.fillStyle = ctx.strokeStyle = "#000";
    ctx.lineWidth = 1.2;
    ctx.beginPath();
    if (r.kind === "bundle") ctx.rect(sx - rad * 0.8, sy - rad * 0.8, rad * 1.6, rad * 1.6);
    else if (mag > 4.5) ctx.arc(sx, sy, rad, 0, Math.PI * 2);
    else ctx.arc(sx, sy, rad * 0.7, 0, Math.PI * 2);
    if (mag > 4.5) {
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(sx, sy, rad * 0.5, 0, Math.PI * 2);
    }
    ctx.fill();
    ctx.textAlign = flip ? "right" : "left";
    ctx.fillStyle = "#000";
    ctx.fillText(r.city, flip ? sx - rad - 4 : sx + rad + 4, sy);
  }
}

function drawInk(sorted: Loaded[]) {
  const { w, h, dpr, scale } = view;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  if (!meta) return;
  const z = zoom();
  const px = 1 / scale;
  ctx.setTransform(
    dpr * scale,
    0,
    0,
    dpr * scale,
    dpr * (w / 2 - view.cx * scale),
    dpr * (h / 2 - view.cy * scale),
  );
  ctx.lineCap = "round";
  ctx.lineJoin = "round";

  ctx.save();
  ctx.beginPath();
  ctx.rect(0, 0, WORLD_W, WORLD_H);
  ctx.clip();

  ctx.strokeStyle = "rgba(20,20,30,0.3)";
  ctx.lineWidth = 0.8 * px;
  ctx.beginPath();
  for (let x = 1 / 6; x < WORLD_W; x += 1 / 6) {
    ctx.moveTo(x, 0);
    ctx.lineTo(x, WORLD_H);
  }
  for (let y = 1 / 6; y < WORLD_H; y += 1 / 6) {
    ctx.moveTo(0, y);
    ctx.lineTo(WORLD_W, y);
  }
  ctx.stroke();

  const [vx0, vy0] = toWorld(-8, -8);
  const [vx1, vy1] = toWorld(w + 8, h + 8);
  const onScreen = (cells: Cells) => {
    const shown = [...cells].filter(([, { box: b }]) => b[0] < vx1 && b[2] > vx0 && b[1] < vy1 && b[3] > vy0);
    const key = shown.map(([i]) => i).join();
    let hit = culled.get(cells);
    if (hit?.key !== key) {
      const path = new Path2D();
      for (const [, c] of shown) path.addPath(c.path);
      culled.set(cells, (hit = { key, path }));
    }
    return hit.path;
  };

  const minRiver = Math.max(1, 3.5 / z);
  ctx.strokeStyle = "rgba(52,84,110,0.85)";
  for (const p of sorted) {
    for (const [b, cells] of p.rivers) {
      if (b < minRiver) continue;
      ctx.lineWidth = Math.min(3.4, 0.45 + 0.3 * b) * Math.min(2.2, z ** 0.3) * px;
      ctx.stroke(onScreen(cells));
    }
  }

  const maxLevel = 2 + Math.log2(Math.max(z, 1)) * 0.9;
  for (const p of sorted) {
    for (const [lvl, cells] of p.borders) {
      const fade = clamp(maxLevel + 1 - lvl, 0, 1);
      if (fade <= 0) continue;
      const st = BORDER_STYLES[Math.min(lvl, 4) - 1];
      ctx.globalAlpha = fade;
      ctx.strokeStyle = st.color;
      ctx.lineWidth = st.w * px;
      ctx.setLineDash(st.dash.map((v) => v * px));
      ctx.stroke(onScreen(cells));
    }
  }
  ctx.globalAlpha = 1;
  ctx.setLineDash([]);
  ctx.restore();

  // Frame: thin inner rule, a margin carrying degree numbers, thin outer rule.
  // Everything printed on the sheet magnifies with the map.
  const f = FRAME * z;
  const fs = FRAME_SIDE * z;
  ctx.strokeStyle = "#000";
  ctx.lineWidth = px * Math.min(z, 4);
  ctx.strokeRect(0, 0, WORLD_W, WORLD_H);
  ctx.strokeRect(-fs * px, -f * px, WORLD_W + 2 * fs * px, WORLD_H + 2 * f * px);

  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = "#000";
  ctx.font = `${8.5 * z}px ${OLD}`;
  ctx.textBaseline = "middle";
  const [fx0, fy0] = toScreen(0, 0);
  const [fx1, fy1] = toScreen(WORLD_W, WORLD_H);
  ctx.textAlign = "center";
  for (let i = 0; i <= 12; i++) {
    const [sx] = toScreen((i / 12) * WORLD_W, 0);
    const label = `${Math.abs(-180 + i * 30)}°`;
    ctx.fillText(label, sx, fy0 - f / 2);
    ctx.fillText(label, sx, fy1 + f / 2);
  }
  for (let j = 1; j < 6; j++) {
    const [, sy] = toScreen(0, (j / 6) * WORLD_H);
    const label = `${Math.abs(90 - j * 30)}°`;
    ctx.fillText(label, fx0 - fs / 2, sy);
    ctx.fillText(label, fx1 + fs / 2, sy);
  }

  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.save();
  ctx.beginPath();
  ctx.rect(fx0, fy0, fx1 - fx0, fy1 - fy0);
  ctx.clip();
  drawPlaces(z, maxLevel, [fx0, fy0, fx1, fy1]);
  ctx.restore();
}

function frame() {
  requestAnimationFrame(frame);
  if (!dirty) return;
  dirty = false;
  const [x0, y0] = toWorld(0, 0);
  const [x1, y1] = toWorld(view.w, view.h);
  const sorted = layers().filter(({ d }) => d.ox < x1 && d.oy < y1 && d.ox + d.cols * d.s > x0 && d.oy + d.rows * d.s > y0);
  const [mx0, my0] = toScreen(0, 0);
  const [mx1, my1] = toScreen(WORLD_W, WORLD_H);
  const d = view.dpr;
  terrain.draw(
    sorted.map((p) => p.gl),
    view,
    [mx0 * d, my0 * d, (mx1 - mx0) * d, (my1 - my0) * d],
  );
  drawInk(sorted);
  placeSheet();
}

/** The paper the map is printed on, clipped to just beyond the viewport so its shadow stays cheap. */
function placeSheet() {
  const sheet = $("sheet");
  sheet.hidden = !meta;
  const z = zoom();
  const m = (FRAME + 10) * z;
  const ms = (FRAME_SIDE + 10) * z;
  const [x0, y0] = toScreen(0, 0);
  const [x1, y1] = toScreen(WORLD_W, WORLD_H);
  const legend = $("legend");
  const k = KEY_SCALE * z;
  // `zoom` re-lays out the key at the new size, so WebKit redraws it sharply instead of scaling a bitmap.
  Object.assign(legend.style, { right: `${view.w - x1}px`, top: `${y1 + (FRAME + 12) * z}px` });
  $("legend-body").style.zoom = `${k}`;
  const l = Math.max(x0 - ms, -80);
  const t = Math.max(y0 - m, -80);
  const r = Math.min(x1 + ms, view.w + 80);
  const b = Math.min(y1 + (FRAME + 24) * z + legend.offsetHeight, view.h + 80);
  Object.assign(sheet.style, { left: `${l}px`, top: `${t}px`, width: `${Math.max(0, r - l)}px`, height: `${Math.max(0, b - t)}px` });
}

// ── View ────────────────────────────────────────────────────────────────

let refineTimer = 0;
function changed() {
  const f = fitScale();
  view.scale = clamp(view.scale, f * 0.8, f * MAX_ZOOM);
  // Allow panning onto the printed margins and the recency key below the map (their size is fixed in sheet units).
  const side = (FRAME_SIDE + 10) / f;
  view.cx = clamp(view.cx, -side, WORLD_W + side);
  view.cy = clamp(view.cy, -(FRAME + 10) / f, WORLD_H + KEY_SPACE / f);
  dirty = true;
  clearTimeout(refineTimer);
  refineTimer = window.setTimeout(() => {
    refine();
    updateChrome();
  }, 120);
}

function resize() {
  view.dpr = window.devicePixelRatio || 1;
  const z = meta ? zoom() : 1;
  view.w = window.innerWidth;
  view.h = window.innerHeight;
  for (const c of [glCanvas, ink]) {
    c.width = Math.round(view.w * view.dpr);
    c.height = Math.round(view.h * view.dpr);
  }
  view.scale = fitScale() * z;
  changed();
}

function fit() {
  animateTo(WORLD_W / 2, fitCy(), fitScale());
}

function zoomAt(sx: number, sy: number, f: number) {
  const [wx, wy] = toWorld(sx, sy);
  view.scale = clamp(view.scale * f, fitScale() * 0.8, fitScale() * MAX_ZOOM);
  view.cx = wx - (sx - view.w / 2) / view.scale;
  view.cy = wy - (sy - view.h / 2) / view.scale;
  changed();
}

let anim = 0;
function animateTo(cx: number, cy: number, scale: number) {
  cancelAnimationFrame(anim);
  const from = { ...view };
  const to = { cx, cy, scale: clamp(scale, fitScale() * 0.8, fitScale() * MAX_ZOOM) };
  const t0 = performance.now();
  const step = (t: number) => {
    const k = clamp((t - t0) / 550, 0, 1);
    const e = k < 0.5 ? 4 * k * k * k : 1 - (-2 * k + 2) ** 3 / 2;
    view.scale = Math.exp(Math.log(from.scale) + (Math.log(to.scale) - Math.log(from.scale)) * e);
    view.cx = from.cx + (to.cx - from.cx) * e;
    view.cy = from.cy + (to.cy - from.cy) * e;
    changed();
    if (k < 1) anim = requestAnimationFrame(step);
  };
  anim = requestAnimationFrame(step);
}

function focus(id: number, sx: number, sy: number) {
  for (const r of ancestors(id)) {
    if (r.kind === "ocean") return;
    const l = labels.get(r.id)?.l;
    if (!l) continue;
    const [a, b, c, d] = l.bbox;
    const bw = Math.max(c - a, 4 / view.scale);
    const bh = Math.max(d - b, 4 / view.scale);
    if (bw * view.scale < view.w * 0.55 && bh * view.scale < view.h * 0.55) {
      animateTo((a + c) / 2, (b + d) / 2, Math.min(view.w / (bw * 1.35), view.h / (bh * 1.35)));
      return;
    }
  }
  const [wx, wy] = toWorld(sx, sy);
  animateTo(wx, wy, view.scale * 2);
}

// ── Chrome ──────────────────────────────────────────────────────────────

function updateChrome() {
  if (!meta) return;
  const target = 14;
  const raw = ((target / view.scale) ** 2 / (WORLD_W * WORLD_H)) * meta.capacity;
  const mag = 10 ** Math.floor(Math.log10(raw));
  const nice = [1, 2, 5, 10]
    .map((m) => m * mag)
    .reduce((a, b) => (Math.abs(Math.log(b / raw)) < Math.abs(Math.log(a / raw)) ? b : a));
  const side = view.scale * Math.sqrt((nice / meta.capacity) * WORLD_W * WORLD_H);
  const box = $("area-box");
  box.style.width = box.style.height = `${side.toFixed(1)}px`;
  $("area-text").textContent = `= ${bytes(nice)}`;
}

/** The note in the viewer's hand: the hovered region, or the whole survey when nothing is hovered. */
function showPage(id: number | null) {
  if (!meta) return;
  const r = id == null ? regions[0] : regions[id];
  $("note-name").textContent = r.name;
  $("note-path").textContent = r.path ? shortPath(r.path) : r.label;
  const size = r.kind === "world" ? `${bytes(meta.scanned)} surveyed, ${bytes(meta.free)} free` : bytes(r.size);
  $("note-facts").textContent = r.mtime && r.kind !== "world" ? `${size} · ${ago(r.mtime)}` : size;
}

function hoverAt(sx: number, sy: number) {
  const [x, y] = toWorld(sx, sy);
  showPage(pick(x, y));
}

// ── Input ───────────────────────────────────────────────────────────────

let drag: { x: number; y: number; moved: boolean } | null = null;
ink.addEventListener("pointerdown", (e) => {
  drag = { x: e.clientX, y: e.clientY, moved: false };
  ink.setPointerCapture(e.pointerId);
});
ink.addEventListener("pointermove", (e) => {
  if (drag) {
    const dx = e.clientX - drag.x;
    const dy = e.clientY - drag.y;
    if (drag.moved || Math.hypot(dx, dy) > 4) {
      drag.moved = true;
      cancelAnimationFrame(anim);
      view.cx -= dx / view.scale;
      view.cy -= dy / view.scale;
      drag.x = e.clientX;
      drag.y = e.clientY;
      changed();
      return;
    }
  }
  hoverAt(e.clientX, e.clientY);
});
ink.addEventListener("pointerup", (e) => {
  const click = drag && !drag.moved;
  drag = null;
  if (!click || !meta) return;
  const [x, y] = toWorld(e.clientX, e.clientY);
  const id = pick(x, y);
  if (id == null) return;
  if (e.metaKey) api.reveal(regions[id].path).catch(console.error);
  else focus(id, e.clientX, e.clientY);
});
ink.addEventListener("pointerleave", () => showPage(null));
ink.addEventListener(
  "wheel",
  (e) => {
    e.preventDefault();
    cancelAnimationFrame(anim);
    const pinch = e.ctrlKey;
    const trackpad =
      !pinch &&
      e.deltaMode === 0 &&
      (e.deltaX !== 0 || !Number.isInteger(e.deltaY) || Math.abs(e.deltaY) < 40);
    if (trackpad) {
      view.cx += e.deltaX / view.scale;
      view.cy += e.deltaY / view.scale;
      changed();
    } else {
      zoomAt(e.clientX, e.clientY, Math.exp(-e.deltaY * (pinch ? 0.012 : 0.0018)));
    }
    hoverAt(e.clientX, e.clientY);
  },
  { passive: false },
);
window.addEventListener("keydown", (e) => {
  if (!meta || (e.target as HTMLElement).tagName === "INPUT") return;
  if (e.key === "Escape" || e.key === "0") fit();
  if (e.key === "=" || e.key === "+") zoomAt(view.w / 2, view.h / 2, 1.6);
  if (e.key === "-") zoomAt(view.w / 2, view.h / 2, 1 / 1.6);
});
window.addEventListener("resize", resize);

// ── Scanning ────────────────────────────────────────────────────────────

const start = $("start");
let rootPaths = { home: "", disk: "" };

function showProgress(s: Status) {
  $("progress").hidden = false;
  const verb = s.phase === "mapping" ? "Drawing the map" : "Surveying";
  $("progress").textContent = `${verb}… ${s.files.toLocaleString()} files, ${bytes(s.bytes)}`;
}

async function poll() {
  const s = await api.status();
  if (s.phase === "scanning" || s.phase === "mapping") {
    showProgress(s);
    setTimeout(poll, 250);
  } else if (s.phase === "ready") {
    await loadWorld();
  } else if (s.phase === "failed") {
    $("progress").hidden = true;
    $("error").hidden = false;
    $("error").textContent = s.message;
    setChoices(true);
  }
}

function setChoices(enabled: boolean) {
  start.querySelectorAll("button").forEach((b) => (b.disabled = !enabled));
}

async function scan(root: string) {
  $("error").hidden = true;
  setChoices(false);
  try {
    await api.startScan(root);
    poll();
  } catch (err) {
    $("error").hidden = false;
    $("error").textContent = String(err);
    setChoices(true);
  }
}

async function loadWorld() {
  const d = await api.world();
  [...patches.values(), ...tiles.values()].forEach((p) => terrain.free(p.gl));
  patches.clear();
  tiles.clear();
  failed.clear();
  labels.clear();
  cities.clear();
  pending.clear();
  meta = d.world!;
  regions = meta.regions;
  addPatch(d);
  start.hidden = true;
  $("progress").hidden = true;
  setChoices(true);
  for (const id of ["rescan", "legend", "note"]) $(id).hidden = false;
  showPage(null);
  view.scale = fitScale();
  view.cx = WORLD_W / 2;
  view.cy = fitCy();
  changed();
}

start
  .querySelectorAll<HTMLButtonElement>("button[data-root]")
  .forEach((b) =>
    b.addEventListener("click", () =>
      scan(b.dataset.root === "disk" ? rootPaths.disk : rootPaths.home),
    ),
  );
$("custom").addEventListener("submit", (e) => {
  e.preventDefault();
  const v = ($("custom-path") as HTMLInputElement).value.trim();
  if (v) scan(v.startsWith("~") ? home + v.slice(1) : v);
});
$("rescan").addEventListener("click", () => {
  start.hidden = false;
  $("close-start").hidden = !meta;
});
$("close-start").addEventListener("click", () => (start.hidden = true));

function woodTexture() {
  const c = document.createElement("canvas");
  c.width = c.height = 1024;
  const g = c.getContext("2d")!;
  let seed = 7;
  const rand = () => ((seed = (seed * 16807) % 2147483647) / 2147483647);
  const plank = 1024 / 5;
  for (let p = 0; p < 5; p++) {
    const y0 = p * plank;
    const tone = 0.82 + rand() * 0.3;
    g.fillStyle = `rgb(${92 * tone},${58 * tone},${34 * tone})`;
    g.fillRect(0, y0, 1024, plank);
    for (let i = 0; i < 90; i++) {
      const y = y0 + rand() * plank;
      const amp = 2 + rand() * 6;
      const freq = 0.004 + rand() * 0.01;
      const phase = rand() * 10;
      g.strokeStyle = rand() < 0.7 ? `rgba(35,18,8,${0.05 + rand() * 0.14})` : `rgba(190,130,80,${0.04 + rand() * 0.07})`;
      g.lineWidth = 0.6 + rand() * 1.8;
      g.beginPath();
      for (let x = 0; x <= 1024; x += 8) {
        const yy = Math.min(y0 + plank - 1, Math.max(y0 + 1, y + Math.sin(x * freq + phase) * amp + Math.sin(x * freq * 3.1) * amp * 0.3));
        if (x === 0) g.moveTo(x, yy);
        else g.lineTo(x, yy);
      }
      g.stroke();
    }
    g.fillStyle = "rgba(20,10,4,0.55)";
    g.fillRect(0, y0, 1024, 2);
    g.fillStyle = "rgba(200,150,100,0.08)";
    g.fillRect(0, y0 + 2, 1024, 1);
  }
  return c.toDataURL("image/jpeg", 0.85);
}

/** The same ink aging the terrain shader applies, so key swatches match the printed map. */
function aged(hex: string) {
  let [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  [r, g, b] = [r, g * (1 - 0.45 * 0.06), b * (1 - 0.45 * 0.2)];
  const lum = 0.3 * r + 0.59 * g + 0.11 * b;
  return `rgb(${[r, g, b].map((v) => Math.round((lum + (v - lum) * 0.85) * 0.97 * 255)).join(",")})`;
}

/** Isochrone-style strip: one cell per band, newest first, range written inside. */
function buildKey() {
  const ranges = ["Over 2 years", "1 to 2 years", "6 to 12 months", "3 to 6 months", "6 weeks to 3 months", "3 to 6 weeks", "1 to 3 weeks", "1 to 7 days", "Within a day"];
  $("key").innerHTML = BAND_COLORS.map((c, i) => `<li style="background:${aged(c)}">${ranges[i]}</li>`).reverse().join("");
}

async function boot() {
  document.body.style.backgroundImage = `url(${woodTexture()})`;
  buildKey();
  resize();
  document.fonts.ready.then(() => (dirty = true));
  requestAnimationFrame(frame);
  rootPaths = await api.roots();
  home = rootPaths.home;
  $("home-path").textContent = shortPath(rootPaths.home);
  $("disk-path").textContent = rootPaths.disk;
  const s = await api.status();
  if (s.phase === "ready") await loadWorld();
  else if (s.phase === "scanning" || s.phase === "mapping") {
    setChoices(false);
    poll();
  }
}

boot().catch((err) => {
  $("error").hidden = false;
  $("error").textContent = String(err);
});

Object.assign(window, {
  atlas: { view, patches, tiles, labels, regions: () => regions, fit, focus, zoomAt },
});
