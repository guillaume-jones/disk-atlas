import type { Camera } from "./camera";
import { clamp, type Rect } from "./geometry";
import type { Layer, Layers } from "./layers";
import { SHEET, WORLD, WORLD_HEIGHT, WORLD_WIDTH } from "./layout";
import { drawPlaces } from "./places";
import type { Survey } from "./survey";
import type { BorderStyle, Theme } from "./theme";

const DEGREES_PER_WORLD_UNIT = 180;
const GRATICULE_DEGREES = 30;
/** Strokes within this many CSS px outside the viewport still get drawn, so line caps don't pop. */
const CULL_MARGIN_PX = 8;
const DEGREE_FONT_PX = 8.5;
const FRAME_RULE_MAX_PX = 4;

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
  draw(
    camera: Camera,
    survey: Survey,
    layers: Layers,
    visible: readonly Layer[],
    theme: Theme,
  ) {
    const { ctx } = this;
    const { ink, ui } = theme;
    const { dpr, scale, zoom } = camera;
    const px = 1 / scale;
    // Deepest border and name level shown; fractional, so the next level fades in.
    const level = 2 + Math.log2(Math.max(zoom, 1)) * 0.9;
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
    if (ink.graticule) this.drawGraticule(ink.graticule, px);
    this.drawRivers(ink.river, visible, view, zoom, px);
    this.drawBorders(ink.borders, visible, view, level, px);
    ctx.restore();
    if (ink.frame) this.drawFrame(ink.frame, ui["text-font"], camera);

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const map = camera.mapOnScreen();
    ctx.save();
    ctx.beginPath();
    ctx.rect(map.x0, map.y0, map.x1 - map.x0, map.y1 - map.y0);
    ctx.clip();
    drawPlaces(ctx, camera, survey, layers, level, map, theme);
    ctx.restore();
  }

  private drawGraticule({ color, width }: NonNullable<Theme["ink"]["graticule"]>, px: number) {
    const { ctx } = this;
    const step = GRATICULE_DEGREES / DEGREES_PER_WORLD_UNIT;
    ctx.strokeStyle = color;
    ctx.lineWidth = width * px;
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

  private drawRivers(
    color: string,
    visible: readonly Layer[],
    view: Rect,
    zoom: number,
    px: number,
  ) {
    const { ctx } = this;
    const minMagnitude = Math.max(1, 3.5 / zoom);
    const zoomWidening = Math.min(2.2, zoom ** 0.3);
    ctx.strokeStyle = color;
    for (const layer of visible) {
      for (const [magnitude, strokes] of layer.rivers) {
        if (magnitude < minMagnitude) continue;
        ctx.lineWidth = Math.min(3.4, 0.45 + 0.3 * magnitude) * zoomWidening * px;
        ctx.stroke(strokes.visible(view).path);
      }
    }
  }

  private drawBorders(
    styles: readonly BorderStyle[],
    visible: readonly Layer[],
    view: Rect,
    level: number,
    px: number,
  ) {
    const { ctx } = this;
    for (const layer of visible) {
      for (const [borderLevel, strokes] of layer.borders) {
        const fade = clamp(level + 1 - borderLevel, 0, 1);
        const style = styles[Math.min(borderLevel, styles.length) - 1];
        if (fade <= 0 || !style) continue;
        ctx.globalAlpha = fade;
        ctx.strokeStyle = style.color;
        ctx.lineWidth = style.width * px;
        ctx.setLineDash(style.dash.map((length) => length * px));
        const { path, runs } = strokes.visible(view);
        if (style.dash.length === 0) ctx.stroke(path);
        else {
          for (const run of runs) {
            ctx.lineDashOffset = run.offset;
            ctx.stroke(run.path);
          }
        }
      }
    }
    ctx.globalAlpha = 1;
    ctx.setLineDash([]);
    ctx.lineDashOffset = 0;
  }

  /** Thin inner rule, a margin carrying degree numbers, thin outer rule; all magnify with the map. */
  private drawFrame(color: string, typeface: string, camera: Camera) {
    const { ctx } = this;
    const { zoom, scale, dpr } = camera;
    const px = 1 / scale;
    const marginY = SHEET.frameY * zoom;
    const marginX = SHEET.frameX * zoom;
    ctx.strokeStyle = color;
    ctx.lineWidth = Math.min(zoom, FRAME_RULE_MAX_PX) * px;
    ctx.strokeRect(WORLD.x0, WORLD.y0, WORLD_WIDTH, WORLD_HEIGHT);
    ctx.strokeRect(
      WORLD.x0 - marginX * px,
      WORLD.y0 - marginY * px,
      WORLD_WIDTH + 2 * marginX * px,
      WORLD_HEIGHT + 2 * marginY * px,
    );

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = color;
    ctx.font = `${DEGREE_FONT_PX * zoom}px ${typeface}`;
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
