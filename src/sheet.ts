import type { Camera } from "./camera";
import { element } from "./dom";
import { formatBytes } from "./format";
import { SHEET, WORLD_HEIGHT, WORLD_WIDTH } from "./layout";
import { RECENCY_BANDS, rgb } from "./recency";

/** Printed-key scale at the fitted zoom; it then grows with the map like everything else on the sheet. */
const KEY_SCALE = 0.8;
/** The paper stops this many CSS px beyond the viewport, so its shadow stays cheap to paint. */
const PAPER_OVERSCAN = 80;
/** Side of the area-scale square we aim for, in CSS px. */
const AREA_SQUARE_PX = 14;

/** The same ink ageing terrain.frag.glsl applies, at mean paper grain, so key swatches match the map. */
function agedSwatch(hex: string): string {
  const [r, g, b] = rgb(hex);
  const warmG = g * (1 - 0.45 * 0.06);
  const warmB = b * (1 - 0.45 * 0.2);
  const luminance = 0.3 * r + 0.59 * warmG + 0.11 * warmB;
  const print = (channel: number) =>
    Math.round((luminance + (channel - luminance) * 0.85) * 0.97 * 255);
  return `rgb(${print(r)},${print(warmG)},${print(warmB)})`;
}

/** The paper the map is printed on, and the recency key printed below the map. */
export class Sheet {
  private readonly paper = element("sheet", HTMLDivElement);
  private readonly legend = element("legend", HTMLElement);
  private readonly legendBody = element("legend-body", HTMLDivElement);
  private readonly areaBox = element("area-box", HTMLSpanElement);
  private readonly areaText = element("area-text", HTMLSpanElement);

  constructor() {
    const swatches = [...RECENCY_BANDS].reverse().map(({ label, color }) => {
      const swatch = document.createElement("li");
      swatch.textContent = label;
      swatch.style.background = agedSwatch(color);
      return swatch;
    });
    element("key", HTMLOListElement).replaceChildren(...swatches);
  }

  place(camera: Camera, shown: boolean) {
    this.paper.hidden = !shown;
    this.legend.hidden = !shown;
    const { zoom } = camera;
    const map = camera.mapOnScreen();
    const legendTop = map.y1 + (SHEET.frameY + SHEET.keyGap) * zoom;
    this.legend.style.right = `${camera.width - map.x1}px`;
    this.legend.style.top = `${legendTop}px`;
    // `zoom` re-lays out the key at the new size, so WebKit redraws it sharply instead of scaling a bitmap.
    this.legendBody.style.zoom = `${KEY_SCALE * zoom}`;

    const left = Math.max(map.x0 - (SHEET.frameX + SHEET.paper) * zoom, -PAPER_OVERSCAN);
    const top = Math.max(map.y0 - (SHEET.frameY + SHEET.paper) * zoom, -PAPER_OVERSCAN);
    const right = Math.min(
      map.x1 + (SHEET.frameX + SHEET.paper) * zoom,
      camera.width + PAPER_OVERSCAN,
    );
    const bottom = Math.min(
      legendTop + this.legend.offsetHeight + SHEET.keyGap * zoom,
      camera.height + PAPER_OVERSCAN,
    );
    Object.assign(this.paper.style, {
      left: `${left}px`,
      top: `${top}px`,
      width: `${Math.max(0, right - left)}px`,
      height: `${Math.max(0, bottom - top)}px`,
    });
  }

  /** Sizes the key's reference square to a round number of bytes near AREA_SQUARE_PX across. */
  updateScale(camera: Camera, capacity: number) {
    const bytesPerWorldArea = capacity / (WORLD_WIDTH * WORLD_HEIGHT);
    const target = (AREA_SQUARE_PX / camera.scale) ** 2 * bytesPerWorldArea;
    const magnitude = 10 ** Math.floor(Math.log10(target));
    const distance = (bytes: number) => Math.abs(Math.log(bytes / target));
    const round = [1, 2, 5, 10]
      .map((step) => step * magnitude)
      .reduce((best, bytes) => (distance(bytes) < distance(best) ? bytes : best));
    const side = `${(camera.scale * Math.sqrt(round / bytesPerWorldArea)).toFixed(1)}px`;
    this.areaBox.style.width = side;
    this.areaBox.style.height = side;
    this.areaText.textContent = `= ${formatBytes(round)}`;
  }
}
