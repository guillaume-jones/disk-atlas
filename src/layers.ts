import * as api from "./api";
import { NO_REGION, type City, type Label, type Patch } from "./api";
import type { Camera } from "./camera";
import { clamp, intersects, type Rect } from "./geometry";
import { buildStrokes, type StrokeGrid } from "./strokes";
import type { Survey } from "./survey";
import type { GLPatch, Terrain } from "./terrain";

/** Request finer detail once a patch's cells are this many CSS px wide. */
const REFINE_PX = 4;
/** A detail tile is about √100 000 cells across (`CELLS` in the Rust world builder). */
const TILE_CELLS_ACROSS = 316;
const TILE_LEVEL_MIN = 1;
const TILE_LEVEL_MAX = 24;
/** Detail tiles kept on the GPU, matching the Rust tile cache. */
const MAX_TILES = 24;
/** Grid cells searched on each side of a probed point. */
const PROBE_REACH = 2;
/** A probe only hits within this many cells of a mesh point, so gaps between patches stay empty. */
const PROBE_RADIUS_CELLS = 1.5;
const MAX_BORDER_LEVEL = 5;
const MAX_RIVER_MAGNITUDE = 8;

export interface Layer {
  patch: Patch;
  gl: GLPatch;
  bounds: Rect;
  /** Keyed by border level. */
  borders: Map<number, StrokeGrid>;
  /** Keyed by river magnitude. */
  rivers: Map<number, StrokeGrid>;
}

/** An item kept from the finest patch that carried it. */
export interface Finest<T> {
  item: T;
  depth: number;
}

function keepFinest<T>(map: Map<number, Finest<T>>, id: number, item: T, depth: number) {
  const known = map.get(id);
  if (!known || known.depth <= depth) map.set(id, { item, depth });
}

class Requests<K> {
  private readonly pending = new Set<K>();
  private readonly failed = new Set<K>();

  constructor(private readonly onSettled: () => void) {}

  get busy() {
    return this.pending.size > 0;
  }

  run(key: K, job: () => Promise<void>) {
    if (this.pending.has(key) || this.failed.has(key)) return;
    this.pending.add(key);
    job()
      .catch((error: unknown) => {
        this.failed.add(key);
        console.error(key, error);
      })
      .finally(() => {
        this.pending.delete(key);
        this.onSettled();
      });
  }
}

export interface LayerEvents {
  /** A layer was added or evicted. */
  onChange: () => void;
  onBusy: (busy: boolean) => void;
}

/**
 * The loaded meshes of one survey: region patches as the viewer descends the region tree,
 * then square detail tiles below it, plus the labels and cities they carry.
 */
export class Layers {
  readonly patches = new Map<number, Layer>();
  /** In order of last use, oldest first. */
  readonly tiles = new Map<string, Layer>();
  readonly labels = new Map<number, Finest<Label>>();
  readonly cities = new Map<number, Finest<City>>();
  /** Coarse to fine. */
  private ordered: Layer[] = [];
  private readonly patchRequests = new Requests<number>(() => this.refine());
  private readonly tileRequests = new Requests<string>(() => this.refine());
  private disposed = false;

  constructor(
    private readonly survey: Survey,
    root: Patch,
    private readonly terrain: Terrain,
    private readonly camera: Camera,
    private readonly events: LayerEvents,
  ) {
    this.addPatch(root);
  }

  /** Coarse to fine, overlapping `view`. */
  visible(view: Rect): Layer[] {
    return this.ordered.filter((layer) => intersects(layer.bounds, view));
  }

  /** Requests whatever finer patches and tiles the current view needs. */
  refine() {
    if (this.disposed) return;
    const { camera, survey } = this;
    const view = camera.visibleWorld();
    const isCoarse = (patch: Patch) => patch.spacing * camera.scale >= REFINE_PX;

    for (const { patch } of this.patches.values()) {
      if (!isCoarse(patch)) continue;
      for (const id of survey.region(patch.root).children) {
        const label = this.labels.get(id)?.item;
        const kind = survey.region(id).kind;
        if (!label || kind === "ocean" || kind === "isles" || this.patches.has(id)) continue;
        if (intersects(label.bounds, view))
          this.patchRequests.run(id, async () => this.addPatch(await api.patch(id)));
      }
    }

    // Below the region tree, square tiles keep adding terrain detail once the region patches have settled.
    const centre = this.probe(camera.cx, camera.cy);
    if (
      centre &&
      survey.region(centre.region).kind !== "ocean" &&
      isCoarse(centre.layer.patch) &&
      !this.patchRequests.busy
    ) {
      const level = clamp(
        Math.ceil(Math.log2(camera.scale / (REFINE_PX * TILE_CELLS_ACROSS))),
        TILE_LEVEL_MIN,
        TILE_LEVEL_MAX,
      );
      const size = 2 ** -level;
      for (let ix = Math.floor(view.x0 / size); ix <= Math.floor(view.x1 / size); ix++) {
        for (let iy = Math.floor(view.y0 / size); iy <= Math.floor(view.y1 / size); iy++) {
          const key = `${level}/${ix}/${iy}`;
          const tile = this.tiles.get(key);
          if (tile) {
            this.tiles.delete(key);
            this.tiles.set(key, tile);
          } else {
            this.tileRequests.run(key, async () =>
              this.addTile(key, await api.tile(level, ix, iy)),
            );
          }
        }
      }
    }
    this.events.onBusy(this.patchRequests.busy || this.tileRequests.busy);
  }

  /** The finest loaded layer with a mesh point near (x, y), and that point's region. */
  probe(x: number, y: number): { layer: Layer; region: number } | null {
    for (const layer of [...this.ordered].reverse()) {
      const { ox, oy, spacing, cols, rows, pos, cell } = layer.patch;
      const i0 = Math.floor((x - ox) / spacing);
      const j0 = Math.floor((y - oy) / spacing);
      let nearest = -1;
      let nearestDistance = Infinity;
      for (let j = Math.max(0, j0 - PROBE_REACH); j <= Math.min(rows - 1, j0 + PROBE_REACH); j++) {
        for (
          let i = Math.max(0, i0 - PROBE_REACH);
          i <= Math.min(cols - 1, i0 + PROBE_REACH);
          i++
        ) {
          const k = j * cols + i;
          const distance = ((pos[2 * k] ?? NaN) - x) ** 2 + ((pos[2 * k + 1] ?? NaN) - y) ** 2;
          if (distance < nearestDistance) {
            nearestDistance = distance;
            nearest = k;
          }
        }
      }
      const region = cell[nearest];
      if (
        region !== undefined &&
        region !== NO_REGION &&
        nearestDistance < (PROBE_RADIUS_CELLS * spacing) ** 2
      )
        return { layer, region };
    }
    return null;
  }

  dispose() {
    this.disposed = true;
    for (const layer of this.ordered) this.terrain.free(layer.gl);
  }

  private load(patch: Patch): Layer {
    const bounds = {
      x0: patch.ox,
      y0: patch.oy,
      x1: patch.ox + patch.cols * patch.spacing,
      y1: patch.oy + patch.rows * patch.spacing,
    };
    return {
      patch,
      gl: this.terrain.upload(patch),
      bounds,
      borders: buildStrokes(patch.borders, bounds, (level) => Math.min(level, MAX_BORDER_LEVEL)),
      rivers: buildStrokes(patch.rivers, bounds, (magnitude) =>
        Math.min(Math.floor(magnitude), MAX_RIVER_MAGNITUDE),
      ),
    };
  }

  private addPatch(patch: Patch) {
    if (this.disposed) return;
    this.patches.set(patch.root, this.load(patch));
    for (const label of patch.labels) keepFinest(this.labels, label.id, label, patch.depth);
    for (const city of patch.cities) keepFinest(this.cities, city.id, city, patch.depth);
    this.reorder();
  }

  private addTile(key: string, patch: Patch) {
    if (this.disposed) return;
    this.tiles.set(key, this.load(patch));
    const [oldestKey, oldest] = this.tiles.entries().next().value ?? [];
    if (this.tiles.size > MAX_TILES && oldestKey !== undefined && oldest) {
      this.terrain.free(oldest.gl);
      this.tiles.delete(oldestKey);
    }
    this.reorder();
  }

  private reorder() {
    this.ordered = [...this.patches.values(), ...this.tiles.values()].sort(
      (a, b) => a.patch.depth - b.patch.depth,
    );
    this.events.onChange();
  }
}
