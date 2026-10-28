import { invoke, isTauri } from "@tauri-apps/api/core";

export const inTauri = isTauri();

export type Kind =
  | "world"
  | "continent"
  | "island"
  | "polar"
  | "ocean"
  | "isles"
  | "land"
  | "file"
  | "bundle"
  | "unseen";

export interface Region {
  id: number;
  parent: number;
  depth: number;
  kind: Kind;
  name: string;
  label: string;
  path: string;
  size: number;
  count: number;
  mtime: number;
  city: string | null;
  children: number[];
}

export interface Label {
  id: number;
  x: number;
  y: number;
  r: number;
  bbox: [number, number, number, number];
  cells: number;
}

export interface City {
  id: number;
  x: number;
  y: number;
  pop: number;
}

export interface WorldMeta {
  capacity: number;
  disk: number;
  scanned: number;
  free: number;
  now: number;
  root: string;
  regions: Region[];
}

export interface PatchData {
  root: number;
  depth: number;
  level: number | null;
  ox: number;
  oy: number;
  s: number;
  cols: number;
  rows: number;
  labels: Label[];
  cities: City[];
  world: WorldMeta | null;
  pos: Float32Array;
  elev: Float32Array;
  shade: Float32Array;
  ice: Float32Array;
  cell: Uint32Array;
  tris: Uint32Array;
  borders: Float32Array;
  rivers: Float32Array;
}

export interface Status {
  phase: "idle" | "scanning" | "mapping" | "ready" | "failed";
  message: string;
  files: number;
  dirs: number;
  bytes: number;
  denied: number;
  elapsed: number;
  regions: number;
}

async function http(url: string, method = "GET"): Promise<Response> {
  const res = await fetch(url, { method });
  if (!res.ok) throw new Error(await res.text());
  return res;
}

export const startScan = (root: string) =>
  inTauri
    ? invoke<void>("start_scan", { root })
    : http(`/api/scan?root=${encodeURIComponent(root)}`, "POST").then(() => undefined);

export const status = (): Promise<Status> =>
  inTauri ? invoke<Status>("scan_status") : http("/api/status").then((r) => r.json());

export const roots = (): Promise<{ home: string; disk: string }> =>
  inTauri ? invoke("scan_roots") : http("/api/roots").then((r) => r.json());

export const world = async () =>
  decode(
    inTauri ? await invoke<ArrayBuffer>("world") : await (await http("/api/world")).arrayBuffer(),
  );

export const patch = async (id: number) =>
  decode(
    inTauri
      ? await invoke<ArrayBuffer>("patch", { id })
      : await (await http(`/api/patch?id=${id}`)).arrayBuffer(),
  );

export const tile = async (level: number, x: number, y: number) =>
  decode(
    inTauri
      ? await invoke<ArrayBuffer>("tile", { level, x, y })
      : await (await http(`/api/tile?l=${level}&x=${x}&y=${y}`)).arrayBuffer(),
  );

export async function reveal(path: string) {
  if (!inTauri || !path) return;
  const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
  await revealItemInDir(path);
}

function decode(buf: ArrayBuffer): PatchData {
  const len = new DataView(buf).getUint32(4, true);
  const m = JSON.parse(new TextDecoder().decode(new Uint8Array(buf, 8, len)));
  let off = 8 + Math.ceil(len / 4) * 4;
  const n = m.cols * m.rows;
  const f32 = (k: number) => {
    const a = new Float32Array(buf, off, k);
    off += k * 4;
    return a;
  };
  const u32 = (k: number) => {
    const a = new Uint32Array(buf, off, k);
    off += k * 4;
    return a;
  };
  return {
    ...m,
    labels: m.labels.map(([id, x, y, r, x0, y0, x1, y1, cells]: number[]) => ({
      id,
      x,
      y,
      r,
      bbox: [x0, y0, x1, y1],
      cells,
    })),
    cities: m.cities.map(([id, x, y, pop]: number[]) => ({ id, x, y, pop })),
    pos: f32(2 * n),
    elev: f32(n),
    shade: f32(n),
    ice: f32(n),
    cell: u32(n),
    tris: u32(3 * m.ntri),
    borders: f32(5 * m.nborder),
    rivers: f32(5 * m.nriver),
  };
}
