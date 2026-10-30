import type { Rect } from "./geometry";

/** The map in world units: 2 × 1, spanning 360° × 180°. */
export const WORLD: Rect = { x0: 0, y0: 0, x1: 2, y1: 1 };
export const WORLD_WIDTH = WORLD.x1 - WORLD.x0;
export const WORLD_HEIGHT = WORLD.y1 - WORLD.y0;

/** Sheet layout in CSS px at the fitted zoom; everything printed on the sheet magnifies with the map. */
export const SHEET = {
  /** Frame margin above and below the map, holding the degree numbers. */
  frameY: 16,
  /** The side margins carry horizontal degree numbers, so they need more room. */
  frameX: 30,
  /** Paper beyond the outer frame rule. */
  paper: 10,
  /** Between the frame and the recency key, and below the key. */
  keyGap: 12,
  /** Room reserved below the map for the frame and the recency key when fitting. */
  fitBelow: 100,
  /** Room reserved above the map for the New Survey ticket when fitting. */
  fitAbove: 64,
  /** Table left showing beside the paper when fitting. */
  fitBeside: 14,
} as const;
