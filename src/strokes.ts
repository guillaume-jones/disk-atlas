import { SEGMENT_FLOATS } from "./api";
import { clamp, contains, intersects, type Rect } from "./geometry";

/** Cells per side of the grid laid over a patch, so only on-screen cells get stroked. */
const GRID = 8;
/** Strokes get cut to the view grown by this share of a view size on each side, so most pans needn't recut them. */
const WINDOW_MARGIN = 0.25;
/** Recut once the window spans this many view sizes, so zooming in keeps dropping what left the screen. */
const WINDOW_MAX_SPAN = 3;

interface Segment {
  ax: number;
  ay: number;
  bx: number;
  by: number;
}

interface Cell {
  segments: Segment[];
  chains: number[][] | null;
  path: Path2D | null;
  bounds: Rect;
}

/** A stretch of a chain, `offset` world units along it, so its dash pattern can resume where the chain has it. */
interface StrokeRun {
  path: Path2D;
  offset: number;
}

interface VisibleStrokes {
  /** Every visible stroke in one path, for solid lines. */
  path: Path2D;
  /** The same strokes split where dashes have to resume mid-chain. */
  runs: readonly StrokeRun[];
}

function chainPath(chains: readonly number[][]): Path2D {
  const path = new Path2D();
  for (const chain of chains) {
    path.moveTo(chain[0] ?? 0, chain[1] ?? 0);
    for (let i = 2; i < chain.length; i += 2) path.lineTo(chain[i] ?? 0, chain[i + 1] ?? 0);
  }
  return path;
}

/** The stretches of `chain` inside `window`. */
function cutChain(chain: readonly number[], window: Rect): StrokeRun[] {
  const runs: StrokeRun[] = [];
  let run: Path2D | null = null;
  let along = 0;
  for (let i = 2; i < chain.length; i += 2) {
    const ax = chain[i - 2] ?? 0;
    const ay = chain[i - 1] ?? 0;
    const bx = chain[i] ?? 0;
    const by = chain[i + 1] ?? 0;
    const inside = intersects(
      { x0: Math.min(ax, bx), y0: Math.min(ay, by), x1: Math.max(ax, bx), y1: Math.max(ay, by) },
      window,
    );
    if (!inside) run = null;
    else {
      if (!run) {
        run = new Path2D();
        run.moveTo(ax, ay);
        runs.push({ path: run, offset: along });
      }
      run.lineTo(bx, by);
    }
    along += Math.hypot(bx - ax, by - ay);
  }
  return runs;
}

/** Joins segments end to end, so a dash pattern runs along a whole border instead of restarting at each segment. */
function chained(segments: readonly Segment[]): number[][] {
  const key = (x: number, y: number) => `${x},${y}`;
  const touching = new Map<string, Segment[]>();
  const touch = (at: string, segment: Segment) => {
    const list = touching.get(at);
    if (list) list.push(segment);
    else touching.set(at, [segment]);
  };
  for (const segment of segments) {
    touch(key(segment.ax, segment.ay), segment);
    touch(key(segment.bx, segment.by), segment);
  }
  const used = new Set<Segment>();
  const chains: number[][] = [];
  for (const start of segments) {
    if (used.has(start)) continue;
    used.add(start);
    const chain = [start.ax, start.ay, start.bx, start.by];
    chains.push(chain);
    let tipX = start.bx;
    let tipY = start.by;
    for (;;) {
      const next = touching.get(key(tipX, tipY))?.find((segment) => !used.has(segment));
      if (!next) break;
      used.add(next);
      const forward = next.ax === tipX && next.ay === tipY;
      tipX = forward ? next.bx : next.ax;
      tipY = forward ? next.by : next.ay;
      chain.push(tipX, tipY);
    }
  }
  return chains;
}

/** One stroke style's segments in a patch, bucketed by grid cell. */
export class StrokeGrid {
  private readonly cells = new Map<number, Cell>();
  private window: Rect | null = null;
  private cut: VisibleStrokes = { path: new Path2D(), runs: [] };

  add(cellIndex: number, ax: number, ay: number, bx: number, by: number) {
    let cell = this.cells.get(cellIndex);
    if (!cell) {
      cell = { segments: [], chains: null, path: null, bounds: { x0: ax, y0: ay, x1: ax, y1: ay } };
      this.cells.set(cellIndex, cell);
    }
    cell.segments.push({ ax, ay, bx, by });
    const bounds = cell.bounds;
    bounds.x0 = Math.min(bounds.x0, ax, bx);
    bounds.y0 = Math.min(bounds.y0, ay, by);
    bounds.x1 = Math.max(bounds.x1, ax, bx);
    bounds.y1 = Math.max(bounds.y1, ay, by);
  }

  /** The strokes overlapping `view`, recut only when the view leaves the window they were last cut to. */
  visible(view: Rect): VisibleStrokes {
    const width = view.x1 - view.x0;
    const height = view.y1 - view.y0;
    const { window } = this;
    if (
      window &&
      contains(window, view) &&
      window.x1 - window.x0 <= WINDOW_MAX_SPAN * width &&
      window.y1 - window.y0 <= WINDOW_MAX_SPAN * height
    )
      return this.cut;

    const next = {
      x0: view.x0 - WINDOW_MARGIN * width,
      y0: view.y0 - WINDOW_MARGIN * height,
      x1: view.x1 + WINDOW_MARGIN * width,
      y1: view.y1 + WINDOW_MARGIN * height,
    };
    const whole = new Path2D();
    const runs: StrokeRun[] = [{ path: whole, offset: 0 }];
    for (const cell of this.cells.values()) {
      if (!intersects(cell.bounds, next)) continue;
      cell.chains ??= chained(cell.segments);
      if (contains(next, cell.bounds)) {
        cell.path ??= chainPath(cell.chains);
        whole.addPath(cell.path);
      } else {
        for (const chain of cell.chains) {
          for (const run of cutChain(chain, next)) {
            if (run.offset === 0) whole.addPath(run.path);
            else runs.push(run);
          }
        }
      }
    }
    const path = new Path2D();
    for (const run of runs) path.addPath(run.path);
    this.window = next;
    this.cut = { path, runs };
    return this.cut;
  }
}

/** Groups (ax, ay, bx, by, value) segments by `styleOf(value)`, then by grid cell over `area`. */
export function buildStrokes(
  segments: Float32Array,
  area: Rect,
  styleOf: (value: number) => number,
) {
  const styles = new Map<number, StrokeGrid>();
  const cellOf = (value: number, min: number, max: number) =>
    clamp(Math.floor(((value - min) / (max - min)) * GRID), 0, GRID - 1);
  for (let i = 0; i < segments.length; i += SEGMENT_FLOATS) {
    const [ax = 0, ay = 0, bx = 0, by = 0, value = 0] = segments.subarray(i, i + SEGMENT_FLOATS);
    const style = styleOf(value);
    let grid = styles.get(style);
    if (!grid) {
      grid = new StrokeGrid();
      styles.set(style, grid);
    }
    const column = cellOf(ax, area.x0, area.x1);
    const row = cellOf(ay, area.y0, area.y1);
    grid.add(row * GRID + column, ax, ay, bx, by);
  }
  return styles;
}
