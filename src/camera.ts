import { clamp, type Point, type Rect } from "./geometry";
import { SHEET, WORLD, WORLD_HEIGHT, WORLD_WIDTH } from "./layout";

/** Zoom limits relative to the fitted scale. */
const MIN_ZOOM = 0.8;
const MAX_ZOOM = 3000;
/** Share of the available room the fitted map fills. */
const FIT_FILL = 0.96;
const ANIMATION_MS = 550;
/** A region counts as already in view once it covers this share of the viewport. */
const FOCUS_SHARE = 0.55;
/** Room left around a focused region, as a factor of its size. */
const FOCUS_PADDING = 1.35;
/** Floor on a focused region's size, in CSS px, so tiny regions don't zoom to infinity. */
const FOCUS_MIN_PX = 4;

const easeInOutCubic = (t: number) => (t < 0.5 ? 4 * t ** 3 : 1 - (2 - 2 * t) ** 3 / 2);

/**
 * Maps world units to CSS px: `scale` px per world unit, with (cx, cy) at the viewport centre.
 * Every change is clamped and reported through `onChange`.
 */
export class Camera {
  cx = WORLD_WIDTH / 2;
  cy = WORLD_HEIGHT / 2;
  scale = 1;
  width = 1;
  height = 1;
  dpr = 1;
  private animation = 0;
  private sized = false;

  constructor(private readonly onChange: () => void) {}

  get fitScale(): number {
    const room = Math.min(
      (this.width - 2 * (SHEET.frameX + SHEET.fitBeside)) / WORLD_WIDTH,
      (this.height - SHEET.fitBelow - SHEET.fitAbove) / WORLD_HEIGHT,
    );
    return room * FIT_FILL;
  }

  /** Magnification relative to the fitted map; sheet furniture is sized by it. */
  get zoom(): number {
    return this.scale / this.fitScale;
  }

  toScreen(x: number, y: number): Point {
    return {
      x: (x - this.cx) * this.scale + this.width / 2,
      y: (y - this.cy) * this.scale + this.height / 2,
    };
  }

  toWorld(x: number, y: number): Point {
    return {
      x: (x - this.width / 2) / this.scale + this.cx,
      y: (y - this.height / 2) / this.scale + this.cy,
    };
  }

  /** World-space rectangle covering the viewport grown by `marginPx` on each side. */
  visibleWorld(marginPx = 0): Rect {
    const min = this.toWorld(-marginPx, -marginPx);
    const max = this.toWorld(this.width + marginPx, this.height + marginPx);
    return { x0: min.x, y0: min.y, x1: max.x, y1: max.y };
  }

  /** The map's rectangle in CSS px. */
  mapOnScreen(): Rect {
    const min = this.toScreen(WORLD.x0, WORLD.y0);
    const max = this.toScreen(WORLD.x1, WORLD.y1);
    return { x0: min.x, y0: min.y, x1: max.x, y1: max.y };
  }

  resize(width: number, height: number, dpr: number) {
    const zoom = this.sized ? this.zoom : 1;
    this.sized = true;
    this.width = width;
    this.height = height;
    this.dpr = dpr;
    this.scale = this.fitScale * zoom;
    this.update();
  }

  jumpTo(cx: number, cy: number, scale: number) {
    this.cancelAnimation();
    this.cx = cx;
    this.cy = cy;
    this.scale = scale;
    this.update();
  }

  /** Fitted with the map shifted up to make room for the key below it. */
  reset() {
    this.jumpTo(...this.fittedCentre(), this.fitScale);
  }

  fit() {
    this.animateTo(...this.fittedCentre(), this.fitScale);
  }

  panBy(dx: number, dy: number) {
    this.cx += dx / this.scale;
    this.cy += dy / this.scale;
    this.update();
  }

  zoomAt(sx: number, sy: number, factor: number) {
    const anchor = this.toWorld(sx, sy);
    this.scale = this.clampScale(this.scale * factor);
    this.cx = anchor.x - (sx - this.width / 2) / this.scale;
    this.cy = anchor.y - (sy - this.height / 2) / this.scale;
    this.update();
  }

  /** Whether `bounds` is still small on screen, so focusing on it would zoom in. */
  isSmall(bounds: Rect): boolean {
    const { width, height } = this.focusSize(bounds);
    return (
      width * this.scale < this.width * FOCUS_SHARE &&
      height * this.scale < this.height * FOCUS_SHARE
    );
  }

  focusOn(bounds: Rect) {
    const { width, height } = this.focusSize(bounds);
    const scale = Math.min(
      this.width / (width * FOCUS_PADDING),
      this.height / (height * FOCUS_PADDING),
    );
    this.animateTo((bounds.x0 + bounds.x1) / 2, (bounds.y0 + bounds.y1) / 2, scale);
  }

  animateTo(cx: number, cy: number, scale: number) {
    this.cancelAnimation();
    const from = { cx: this.cx, cy: this.cy, logScale: Math.log(this.scale) };
    const toLogScale = Math.log(this.clampScale(scale));
    const start = performance.now();
    const step = (now: number) => {
      const progress = clamp((now - start) / ANIMATION_MS, 0, 1);
      const eased = easeInOutCubic(progress);
      this.scale = Math.exp(from.logScale + (toLogScale - from.logScale) * eased);
      this.cx = from.cx + (cx - from.cx) * eased;
      this.cy = from.cy + (cy - from.cy) * eased;
      this.update();
      if (progress < 1) this.animation = requestAnimationFrame(step);
    };
    this.animation = requestAnimationFrame(step);
  }

  cancelAnimation() {
    cancelAnimationFrame(this.animation);
  }

  private fittedCentre(): [number, number] {
    return [
      WORLD_WIDTH / 2,
      WORLD_HEIGHT / 2 + (SHEET.fitBelow - SHEET.fitAbove) / 2 / this.fitScale,
    ];
  }

  private focusSize(bounds: Rect) {
    const floor = FOCUS_MIN_PX / this.scale;
    return {
      width: Math.max(bounds.x1 - bounds.x0, floor),
      height: Math.max(bounds.y1 - bounds.y0, floor),
    };
  }

  private clampScale(scale: number): number {
    return clamp(scale, this.fitScale * MIN_ZOOM, this.fitScale * MAX_ZOOM);
  }

  /** Panning may reach the printed margins and the recency key below the map. */
  private update() {
    const fit = this.fitScale;
    this.scale = this.clampScale(this.scale);
    const side = (SHEET.frameX + SHEET.paper) / fit;
    this.cx = clamp(this.cx, WORLD.x0 - side, WORLD.x1 + side);
    this.cy = clamp(
      this.cy,
      WORLD.y0 - (SHEET.frameY + SHEET.paper) / fit,
      WORLD.y1 + SHEET.fitBelow / fit,
    );
    this.onChange();
  }
}
