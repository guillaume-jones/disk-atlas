import { invoke, isTauri, type InvokeArgs } from "@tauri-apps/api/core";
import type { Rect } from "./geometry";
import * as parse from "./validate";

const KINDS = [
  "world",
  "continent",
  "island",
  "polar",
  "ocean",
  "isles",
  "land",
  "file",
  "bundle",
  "unseen",
] as const;
export type Kind = (typeof KINDS)[number];

const PHASES = ["idle", "scanning", "mapping", "ready", "failed"] as const;
type Phase = (typeof PHASES)[number];

/** Region id meaning "none": the world's parent, and mesh cells outside every region. */
export const NO_REGION = 0xffffffff;

export interface Region {
  id: number;
  parent: number;
  depth: number;
  kind: Kind;
  /** The folder's distinctive word, from which themes build names. */
  word: string | null;
  /** Stable per region, seeding its themed name. */
  hash: number;
  /** Set by `nameRegions`. */
  name: string;
  /** Shown instead of the path for regions that aren't folders, e.g. the sea of free space. */
  label: string;
  path: string;
  size: number;
  /** Unix seconds; 0 when unknown. */
  mtime: number;
  settled: boolean;
  /** Set by `nameRegions`. */
  city: string | null;
  children: number[];
}

export interface SurveyMeta {
  /** Bytes represented by the whole map. */
  capacity: number;
  scanned: number;
  free: number;
  /** Unix seconds at scan time. */
  now: number;
  regions: Region[];
}

export interface Label {
  id: number;
  x: number;
  y: number;
  radius: number;
  bounds: Rect;
}

export interface City {
  id: number;
  x: number;
  y: number;
  population: number;
}

/** A region patch or detail tile: a grid-ordered Voronoi mesh plus its ink. */
export interface Patch {
  /** The region a patch details, or NO_REGION for a tile. */
  root: number;
  /** Finer patches draw over coarser ones; tiles sit below every region patch. */
  depth: number;
  ox: number;
  oy: number;
  /** Grid spacing in world units. */
  spacing: number;
  cols: number;
  rows: number;
  labels: Label[];
  cities: City[];
  /** Per vertex, interleaved x, y. */
  pos: Float32Array;
  elev: Float32Array;
  shade: Float32Array;
  ice: Float32Array;
  /** Owning region per vertex, or NO_REGION. */
  cell: Uint32Array;
  tris: Uint32Array;
  /** Segments of (ax, ay, bx, by, border level). */
  borders: Float32Array;
  /** Segments of (ax, ay, bx, by, river magnitude). */
  rivers: Float32Array;
}

export interface Status {
  phase: Phase;
  message: string;
  files: number;
  bytes: number;
}

export interface Roots {
  home: string;
  disk: string;
}

const inTauri = isTauri();

async function http(path: string, method = "GET"): Promise<Response> {
  const response = await fetch(`/api/${path}`, { method });
  if (!response.ok) throw new Error(await response.text());
  return response;
}

async function json(command: string, path: string): Promise<unknown> {
  return inTauri ? invoke<unknown>(command) : (await http(path)).json();
}

async function binary(command: string, args: InvokeArgs, path: string): Promise<ArrayBuffer> {
  const payload = inTauri
    ? await invoke<unknown>(command, args)
    : await (await http(path)).arrayBuffer();
  if (!(payload instanceof ArrayBuffer))
    throw new TypeError(`${command} did not return binary data`);
  return payload;
}

export async function startScan(root: string): Promise<void> {
  if (inTauri) await invoke("start_scan", { root });
  else await http(`scan?root=${encodeURIComponent(root)}`, "POST");
}

export async function status(): Promise<Status> {
  const fields = parse.object(await json("scan_status", "status"));
  return {
    phase: parse.literal(fields.phase, PHASES),
    message: parse.string(fields.message),
    files: parse.number(fields.files),
    bytes: parse.number(fields.bytes),
  };
}

export async function roots(): Promise<Roots> {
  const fields = parse.object(await json("scan_roots", "roots"));
  return { home: parse.string(fields.home), disk: parse.string(fields.disk) };
}

export async function needsFullDiskAccess(root: string): Promise<boolean> {
  return inTauri
    ? invoke<boolean>("full_disk_access_needed", { root })
    : (await http(`access?root=${encodeURIComponent(root)}`)).json();
}

const FULL_DISK_ACCESS_SETTINGS =
  "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

export async function openFullDiskAccessSettings(): Promise<void> {
  const { openUrl } = await import("@tauri-apps/plugin-opener");
  await openUrl(FULL_DISK_ACCESS_SETTINGS);
}

export async function world(): Promise<{ survey: SurveyMeta; patch: Patch }> {
  const { patch, world } = decode(await binary("world", {}, "world"));
  return { survey: parseSurvey(world), patch };
}

export const patch = async (id: number) =>
  decode(await binary("patch", { id }, `patch?id=${id}`)).patch;

export const tile = async (level: number, x: number, y: number) =>
  decode(await binary("tile", { level, x, y }, `tile?l=${level}&x=${x}&y=${y}`)).patch;

export async function reveal(path: string): Promise<void> {
  if (!inTauri || !path) return;
  const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
  await revealItemInDir(path);
}

function parseRegion(value: unknown): Region {
  const fields = parse.object(value);
  return {
    id: parse.number(fields.id),
    parent: parse.number(fields.parent),
    depth: parse.number(fields.depth),
    kind: parse.literal(fields.kind, KINDS),
    word: parse.nullable(fields.word, parse.string),
    hash: parse.number(fields.hash),
    name: "",
    label: parse.string(fields.label),
    path: parse.string(fields.path),
    size: parse.number(fields.size),
    mtime: parse.number(fields.mtime),
    settled: parse.boolean(fields.settled),
    city: null,
    children: parse.array(fields.children, parse.number),
  };
}

function parseSurvey(value: unknown): SurveyMeta {
  const fields = parse.object(value);
  return {
    capacity: parse.number(fields.capacity),
    scanned: parse.number(fields.scanned),
    free: parse.number(fields.free),
    now: parse.number(fields.now),
    regions: parse.array(fields.regions, parseRegion),
  };
}

function parseLabel(value: unknown): Label {
  const [id, x, y, radius, x0, y0, x1, y1] = parse.numbers(value, 9);
  return { id, x, y, radius, bounds: { x0, y0, x1, y1 } };
}

function parseCity(value: unknown): City {
  const [id, x, y, population] = parse.numbers(value, 4);
  return { id, x, y, population };
}

const MAGIC = "ATL1";
const PREAMBLE_BYTES = 8;
/** Floats per border or river segment. */
export const SEGMENT_FLOATS = 5;

/** Layout: "ATL1", u32 JSON length, JSON header padded to 4 bytes, then the typed arrays in `Patch` order. */
function decode(buffer: ArrayBuffer): { patch: Patch; world: unknown } {
  const text = new TextDecoder();
  if (text.decode(new Uint8Array(buffer, 0, MAGIC.length)) !== MAGIC)
    throw new TypeError("Not an atlas payload");
  const headerBytes = new DataView(buffer).getUint32(MAGIC.length, true);
  const header = parse.object(
    JSON.parse(text.decode(new Uint8Array(buffer, PREAMBLE_BYTES, headerBytes))),
  );
  const cols = parse.number(header.cols);
  const rows = parse.number(header.rows);
  const vertices = cols * rows;

  let offset = PREAMBLE_BYTES + Math.ceil(headerBytes / 4) * 4;
  const floats = (length: number) => {
    const array = new Float32Array(buffer, offset, length);
    offset += array.byteLength;
    return array;
  };
  const uints = (length: number) => {
    const array = new Uint32Array(buffer, offset, length);
    offset += array.byteLength;
    return array;
  };

  const patch: Patch = {
    root: parse.number(header.root),
    depth: parse.number(header.depth),
    ox: parse.number(header.ox),
    oy: parse.number(header.oy),
    spacing: parse.number(header.s),
    cols,
    rows,
    labels: parse.array(header.labels, parseLabel),
    cities: parse.array(header.cities, parseCity),
    pos: floats(2 * vertices),
    elev: floats(vertices),
    shade: floats(vertices),
    ice: floats(vertices),
    cell: uints(vertices),
    tris: uints(3 * parse.number(header.ntri)),
    borders: floats(SEGMENT_FLOATS * parse.number(header.nborder)),
    rivers: floats(SEGMENT_FLOATS * parse.number(header.nriver)),
  };
  if (offset !== buffer.byteLength)
    throw new TypeError(`Atlas payload has ${buffer.byteLength - offset} stray bytes`);
  return { patch, world: header.world };
}
