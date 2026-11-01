import type { Camera } from "./camera";
import { element } from "./dom";
import { keyTop } from "./key";
import { SHEET } from "./layout";
import type { Theme } from "./theme";

/** The paper stops this many CSS px beyond the viewport, so its shadow stays cheap to paint. */
const PAPER_OVERSCAN = 80;

/** The paper the map and its key are printed on. */
export class Sheet {
  private readonly paper = element("sheet", HTMLDivElement);
  /** Without a frame the paper hugs the map. */
  private framed = true;

  applyTheme({ ink }: Theme) {
    this.framed = ink.frame !== null;
  }

  place(camera: Camera, shown: boolean) {
    this.paper.hidden = !shown;
    const { zoom } = camera;
    const map = camera.mapOnScreen();
    const frameX = this.framed ? SHEET.frameX : 0;
    const frameY = this.framed ? SHEET.frameY : 0;

    const left = Math.max(map.x0 - (frameX + SHEET.paper) * zoom, -PAPER_OVERSCAN);
    const top = Math.max(map.y0 - (frameY + SHEET.paper) * zoom, -PAPER_OVERSCAN);
    const right = Math.min(
      map.x1 + (frameX + SHEET.paper) * zoom,
      camera.width + PAPER_OVERSCAN,
    );
    const bottom = Math.min(
      keyTop(camera, this.framed) + (SHEET.keyHeight + SHEET.keyGap) * zoom,
      camera.height + PAPER_OVERSCAN,
    );
    Object.assign(this.paper.style, {
      left: `${left}px`,
      top: `${top}px`,
      width: `${Math.max(0, right - left)}px`,
      height: `${Math.max(0, bottom - top)}px`,
    });
  }
}
