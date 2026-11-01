import type { Camera } from "./camera";
import { formatBytes } from "./format";
import { SHEET, WORLD_HEIGHT, WORLD_WIDTH } from "./layout";
import { HAIR_SPACE } from "./places";
import { RECENCY_BANDS } from "./recency";
import { rgb, type TerrainStyle, type Theme } from "./theme";

const CELL_WIDTH = 42;
const CELL_HEIGHT = 22;
const BAND_FONT_PX = 6;
const BAND_LINE_PX = 7;
const CAPTION_FONT_PX = 7.5;
const RULE_PX = 0.5;
const STRIP_TOP = SHEET.keyHeight - CELL_HEIGHT;
/** Room right of the strip for the area scale. */
const AREA_WIDTH = 70;
const AREA_GAP = 5;
/** Side of the area-scale square we aim for, in world units. */
const AREA_SQUARE = 0.02;

/** The same ink ageing terrain.frag.glsl applies, at mean paper grain, so key swatches match the map. */
function agedSwatch(hex: string, { colors, params }: TerrainStyle): string {
  const { paperTintOpacity, saturation, grainFloor, grainRange } = params;
  const tint = rgb(colors.paperTint);
  const [r = 0, g = 0, b = 0] = rgb(hex).map(
    (channel, i) => channel * (1 - paperTintOpacity * (1 - (tint[i] ?? 1))),
  );
  const luminance = 0.3 * r + 0.59 * g + 0.11 * b;
  const print = (channel: number) =>
    Math.round(
      (luminance + (channel - luminance) * saturation) * (grainFloor + grainRange / 3) * 255,
    );
  return `rgb(${print(r)},${print(g)},${print(b)})`;
}

/** Screen y of the key's top edge, below the frame when the theme draws one. */
export const keyTop = (camera: Camera, framed: boolean) =>
  camera.mapOnScreen().y1 + ((framed ? SHEET.frameY : 0) + SHEET.keyGap) * camera.zoom;

/** The bytes a square near AREA_SQUARE holds, as a round number, and its side in world units. */
function areaScale(capacity: number) {
  const bytesPerWorldArea = capacity / (WORLD_WIDTH * WORLD_HEIGHT);
  const target = AREA_SQUARE ** 2 * bytesPerWorldArea;
  const magnitude = 10 ** Math.floor(Math.log10(target));
  const distance = (bytes: number) => Math.abs(Math.log(bytes / target));
  const bytes = [1, 2, 5, 10]
    .map((step) => step * magnitude)
    .reduce((best, candidate) => (distance(candidate) < distance(best) ? candidate : best));
  return { bytes, side: Math.sqrt(bytes / bytesPerWorldArea) };
}

/** Printed in sheet units anchored to the map's bottom right corner, so it magnifies exactly with the map. */
export function drawKey(
  ctx: CanvasRenderingContext2D,
  camera: Camera,
  capacity: number,
  { terrain, ink, ui }: Theme,
) {
  const { dpr, zoom, fitScale } = camera;
  ctx.setTransform(
    dpr * zoom,
    0,
    0,
    dpr * zoom,
    dpr * camera.mapOnScreen().x1,
    dpr * keyTop(camera, ink.frame !== null),
  );
  const typeface = ui["text-font"];
  const stripLeft = -AREA_WIDTH - RECENCY_BANDS.length * CELL_WIDTH;
  const middle = STRIP_TOP + CELL_HEIGHT / 2;
  ctx.fillStyle = ui["print-ink"];
  ctx.textBaseline = "middle";

  const heading = "Days since last modified";
  ctx.font = `700 ${CAPTION_FONT_PX}px ${typeface}`;
  ctx.textAlign = "left";
  ctx.fillText(
    (ui["heading-case"] === "uppercase" ? heading.toUpperCase() : heading)
      .split("")
      .join(HAIR_SPACE.repeat(ink.key.headingSpacing)),
    stripLeft,
    STRIP_TOP / 2,
  );

  ctx.font = `${BAND_FONT_PX}px ${typeface}`;
  ctx.textAlign = "center";
  ctx.strokeStyle = ink.key.rule;
  ctx.lineWidth = RULE_PX;
  [...RECENCY_BANDS.entries()].reverse().forEach(([band, { label }], column) => {
    const left = stripLeft + column * CELL_WIDTH;
    ctx.fillStyle = agedSwatch(terrain.bands[band] ?? "#000000", terrain);
    ctx.fillRect(left, STRIP_TOP, CELL_WIDTH, CELL_HEIGHT);
    ctx.strokeRect(left, STRIP_TOP, CELL_WIDTH, CELL_HEIGHT);
    ctx.fillStyle = ui["print-ink"];
    const lines = label.split("\n");
    lines.forEach((line, row) => {
      const y = middle + (row - (lines.length - 1) / 2) * BAND_LINE_PX;
      ctx.fillText(line, left + CELL_WIDTH / 2, y);
    });
  });

  const { bytes, side } = areaScale(capacity);
  const square = side * fitScale;
  const text = `= ${formatBytes(bytes)}`;
  ctx.font = `italic ${CAPTION_FONT_PX}px ${typeface}`;
  ctx.textAlign = "right";
  ctx.fillText(text, 0, middle);
  ctx.strokeStyle = ui["print-ink"];
  ctx.strokeRect(
    -ctx.measureText(text).width - AREA_GAP - square,
    middle - square / 2,
    square,
    square,
  );
}
