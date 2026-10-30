import type { Camera } from "./camera";
import type { Point } from "./geometry";

/** A press that moves further than this many CSS px is a drag, not a click. */
const DRAG_THRESHOLD_PX = 4;
/** Zoom per wheel delta unit, for pinch gestures (ctrl-wheel) and for mouse wheels. */
const PINCH_ZOOM_RATE = 0.012;
const WHEEL_ZOOM_RATE = 0.0018;
/** Mouse wheels send whole, large vertical steps; trackpads send anything else and pan. */
const MOUSE_WHEEL_MIN_STEP = 40;
const KEY_ZOOM = 1.6;

interface InputHandlers {
  /** Whether a map is showing; keyboard shortcuts are ignored otherwise. */
  isActive(): boolean;
  hover(point: Point | null): void;
  click(point: Point, reveal: boolean): void;
}

/** Drag or trackpad-scroll to pan, wheel or pinch to zoom, click to descend, ⌘- or Ctrl-click to reveal. */
export function bindInput(canvas: HTMLCanvasElement, camera: Camera, handlers: InputHandlers) {
  let drag: { x: number; y: number; moved: boolean } | null = null;
  const pointOf = (event: MouseEvent): Point => ({ x: event.clientX, y: event.clientY });

  canvas.addEventListener("pointerdown", (event) => {
    drag = { x: event.clientX, y: event.clientY, moved: false };
    canvas.setPointerCapture(event.pointerId);
  });

  canvas.addEventListener("pointermove", (event) => {
    if (drag) {
      const dx = event.clientX - drag.x;
      const dy = event.clientY - drag.y;
      if (drag.moved || Math.hypot(dx, dy) > DRAG_THRESHOLD_PX) {
        drag = { x: event.clientX, y: event.clientY, moved: true };
        camera.cancelAnimation();
        camera.panBy(-dx, -dy);
        return;
      }
    }
    handlers.hover(pointOf(event));
  });

  canvas.addEventListener("pointerup", (event) => {
    const clicked = drag !== null && !drag.moved;
    drag = null;
    if (clicked) handlers.click(pointOf(event), event.metaKey || event.ctrlKey);
  });

  canvas.addEventListener("pointerleave", () => handlers.hover(null));

  canvas.addEventListener(
    "wheel",
    (event) => {
      event.preventDefault();
      camera.cancelAnimation();
      const pinch = event.ctrlKey;
      const trackpad =
        !pinch &&
        event.deltaMode === WheelEvent.DOM_DELTA_PIXEL &&
        (event.deltaX !== 0 ||
          !Number.isInteger(event.deltaY) ||
          Math.abs(event.deltaY) < MOUSE_WHEEL_MIN_STEP);
      if (trackpad) camera.panBy(event.deltaX, event.deltaY);
      else
        camera.zoomAt(
          event.clientX,
          event.clientY,
          Math.exp(-event.deltaY * (pinch ? PINCH_ZOOM_RATE : WHEEL_ZOOM_RATE)),
        );
      handlers.hover(pointOf(event));
    },
    { passive: false },
  );

  window.addEventListener("keydown", (event) => {
    if (!handlers.isActive() || event.target instanceof HTMLInputElement) return;
    const centreX = camera.width / 2;
    const centreY = camera.height / 2;
    switch (event.key) {
      case "Escape":
      case "0":
        camera.fit();
        break;
      case "=":
      case "+":
        camera.zoomAt(centreX, centreY, KEY_ZOOM);
        break;
      case "-":
        camera.zoomAt(centreX, centreY, 1 / KEY_ZOOM);
        break;
    }
  });
}
