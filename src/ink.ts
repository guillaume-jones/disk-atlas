import type { Camera } from "./camera";
import { clamp, type Rect } from "./geometry";
import type { Layer, Layers } from "./layers";
import { SHEET, TYPEFACE, WORLD, WORLD_HEIGHT, WORLD_WIDTH } from "./layout";
import { drawPlaces } from "./places";
import type { Survey } from "./survey";

const DEGREES_PER_WORLD_UNIT = 180;
const GRATICULE_DEGREES = 30;
/** Strokes within this many CSS px outside the viewport still get drawn, so line caps don't pop. */
const CULL_MARGIN_PX = 8;
const GRATICULE_INK = "rgba(20,20,30,0.3)";
const RIVER_INK = "rgba(52,84,110,0.85)";
/** Border styles by level: continent, nation, province, then everything finer. */
const BORDER_STYLES = [
  { color: "rgba(40,28,20,0.85)", width: 1.5, dash: [] },
  { color: "rgba(122,36,24,0.75)", width: 1.3, dash: [7, 2.5, 1.5, 2.5] },
  { color: "rgba(122,52,36,0.6)", width: 1, dash: [3, 2.5] },
  { color: "rgba(90,60,45,0.5)", width: 0.8, dash: [1, 2.5] },
];
const DEGREE_FONT_PX = 8.5;
const FRAME_RULE_MAX_PX = 4;

/** Deepest border and name level shown at a zoom; fractional, so the next level fades in. */
const detailLevel = (zoom: number) => 2 + Math.log2(Math.max(zoom, 1)) * 0.9;

/** The 2D overlay printed over the terrain: graticule, rivers, borders, frame and names. */
export class Ink {
  private readonly ctx: CanvasRenderingContext2D;

  constructor(canvas: HTMLCanvasElement) {
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("Canvas 2D unavailable");
    this.ctx = ctx;
  }

  clear(camera: Camera) {
    this.ctx.setTransform(camera.dpr, 0, 0, camera.dpr, 0, 0);
    this.ctx.clearRect(0, 0, camera.width, camera.height);
  }

  /** `visible` runs coarse to fine. */
  draw(camera: Camera, survey: Survey, layers: Layers, visible: readonly Layer[]) {
    const { ctx } = this;
    const { dpr, scale, zoom } = camera;
    const px = 1 / scale;
    const level = detailLevel(zoom);
    const view = camera.visibleWorld(CULL_MARGIN_PX);
    this.clear(camera);

    ctx.setTransform(
      dpr * scale,
      0,
      0,
      dpr * scale,
      dpr * (camera.width / 2 - camera.cx * scale),
      dpr * (camera.height / 2 - camera.cy * scale),
    );
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.save();
    ctx.beginPath();
    ctx.rect(WORLD.x0, WORLD.y0, WORLD_WIDTH, WORLD_HEIGHT);
    ctx.clip();
    this.drawGraticule(px);
    this.drawRivers(visible, view, zoom, px);
    this.drawBorders(visible, view, level, px);
    ctx.restore();
    this.drawFrame(camera);

    const map = camera.mapOnScreen();
    ctx.save();
    ctx.beginPath();
    ctx.rect(map.x0, map.y0, map.x1 - map.x0, map.y1 - map.y0);
    ctx.clip();
    drawPlaces(ctx, camera, survey, layers, level, map);
    ctx.restore();
  }

  private drawGraticule(px: number) {
    const { ctx } = this;
    const step = GRATICULE_DEGREES / DEGREES_PER_WORLD_UNIT;
    ctx.strokeStyle = GRATICULE_INK;
    ctx.lineWidth = 0.8 * px;
    ctx.beginPath();
    for (let x = WORLD.x0 + step; x < WORLD.x1 - step / 2; x += step) {
      ctx.moveTo(x, WORLD.y0);
      ctx.lineTo(x, WORLD.y1);
    }
    for (let y = WORLD.y0 + step; y < WORLD.y1 - step / 2; y += step) {
      ctx.moveTo(WORLD.x0, y);
      ctx.lineTo(WORLD.x1, y);
    }
    ctx.stroke();
  }

  private drawRivers(visible: readonly Layer[], view: Rect, zoom: number, px: number) {
    const { ctx } = this;
    const minMagnitude = Math.max(1, 3.5 / zoom);
    const zoomWidening = Math.min(2.2, zoom ** 0.3);
    ctx.strokeStyle = RIVER_INK;
    for (const layer of visible) {
      for (const [magnitude, strokes] of layer.rivers) {
        if (magnitude < minMagnitude) continue;
        ctx.lineWidth = Math.min(3.4, 0.45 + 0.3 * magnitude) * zoomWidening * px;
        ctx.stroke(strokes.visible(view));
      }
    }
  }

  private drawBorders(visible: readonly Layer[], view: Rect, level: number, px: number) {
    const { ctx } = this;
    for (const layer of visible) {
      for (const [borderLevel, strokes] of layer.borders) {
        const fade = clamp(level + 1 - borderLevel, 0, 1);
        const style = BORDER_STYLES[Math.min(borderLevel, BORDER_STYLES.length) - 1];
        if (fade <= 0 || !style) continue;
        ctx.globalAlpha = fade;
        ctx.strokeStyle = style.color;
        ctx.lineWidth = style.width * px;
        ctx.setLineDash(style.dash.map((length) => length * px));
        ctx.stroke(strokes.visible(view));
      }
    }
    ctx.globalAlpha = 1;
    ctx.setLineDash([]);
  }

  /** Thin inner rule, a margin carrying degree numbers, thin outer rule; all magnify with the map. */
  private drawFrame(camera: Camera) {
    const { ctx } = this;
    const { zoom, scale, dpr } = camera;
    const px = 1 / scale;
    const marginY = SHEET.frameY * zoom;
    const marginX = SHEET.frameX * zoom;
    ctx.strokeStyle = "#000";
    ctx.lineWidth = Math.min(zoom, FRAME_RULE_MAX_PX) * px;
    ctx.strokeRect(WORLD.x0, WORLD.y0, WORLD_WIDTH, WORLD_HEIGHT);
    ctx.strokeRect(
      WORLD.x0 - marginX * px,
      WORLD.y0 - marginY * px,
      WORLD_WIDTH + 2 * marginX * px,
      WORLD_HEIGHT + 2 * marginY * px,
    );

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = "#000";
    ctx.font = `${DEGREE_FONT_PX * zoom}px ${TYPEFACE}`;
    ctx.textBaseline = "middle";
    ctx.textAlign = "center";
    const map = camera.mapOnScreen();
    for (let longitude = -180; longitude <= 180; longitude += GRATICULE_DEGREES) {
      const { x } = camera.toScreen(
        WORLD.x0 + (longitude + 180) / DEGREES_PER_WORLD_UNIT,
        WORLD.y0,
      );
      ctx.fillText(`${Math.abs(longitude)}°`, x, map.y0 - marginY / 2);
      ctx.fillText(`${Math.abs(longitude)}°`, x, map.y1 + marginY / 2);
    }
    for (let latitude = 90 - GRATICULE_DEGREES; latitude > -90; latitude -= GRATICULE_DEGREES) {
      const { y } = camera.toScreen(WORLD.x0, WORLD.y0 + (90 - latitude) / DEGREES_PER_WORLD_UNIT);
      ctx.fillText(`${Math.abs(latitude)}°`, map.x0 - marginX / 2, y);
      ctx.fillText(`${Math.abs(latitude)}°`, map.x1 + marginX / 2, y);
    }
  }
}
