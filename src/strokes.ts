import { clamp, intersects, type Rect } from "./geometry";

/** Cells per side of the grid laid over a patch, so only on-screen cells get stroked. */
const GRID = 8;
const SEGMENT_FLOATS = 5;

interface Cell {
  path: Path2D;
  bounds: Rect;
}

/** One stroke style's segments in a patch, bucketed by grid cell. */
export class StrokeGrid {
  private readonly cells = new Map<number, Cell>();
  private visibleKey = "";
  private visiblePath = new Path2D();

  add(cellIndex: number, ax: number, ay: number, bx: number, by: number) {
    let cell = this.cells.get(cellIndex);
    if (!cell) {
      cell = { path: new Path2D(), bounds: { x0: ax, y0: ay, x1: ax, y1: ay } };
      this.cells.set(cellIndex, cell);
    }
    cell.path.moveTo(ax, ay);
    cell.path.lineTo(bx, by);
    const bounds = cell.bounds;
    bounds.x0 = Math.min(bounds.x0, ax, bx);
    bounds.y0 = Math.min(bounds.y0, ay, by);
    bounds.x1 = Math.max(bounds.x1, ax, bx);
    bounds.y1 = Math.max(bounds.y1, ay, by);
  }

  /** The merged path of cells overlapping `view`, rebuilt only when that set changes. */
  visible(view: Rect): Path2D {
    const shown = [...this.cells].filter(([, cell]) => intersects(cell.bounds, view));
    const key = shown.map(([index]) => index).join();
    if (key !== this.visibleKey) {
      this.visibleKey = key;
      this.visiblePath = new Path2D();
      for (const [, cell] of shown) this.visiblePath.addPath(cell.path);
    }
    return this.visiblePath;
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
