import type { Kind, Region } from "./api";
import type { Camera } from "./camera";
import { clamp, intersects, type Point, type Rect } from "./geometry";
import type { Layers } from "./layers";
import { TYPEFACE } from "./layout";
import type { Survey } from "./survey";

/** How far off screen (CSS px) an anchor may sit and still place its name. */
const OFFSCREEN_X = 150;
const OFFSCREEN_Y = 40;
/** Names of regions this large (share of the viewport's long side) are left off; you're inside them. */
const MAX_LABEL_SHARE = 0.9;
/** Cities this populous (in files, at the fitted zoom) are always named; the threshold falls with zoom². */
const CITY_POPULATION = 4000;
/** Smaller cities still get named once their region is this wide in CSS px. */
const CITY_REGION_PX = 30;
/** Clearance between a name and the map's frame, and between a city's symbol and its name. */
const GAP = 4;
const HAIR_SPACE = " ";

interface Typography {
  sizePerRadius: number;
  minSize: number;
  maxSize: number;
  style: "normal" | "italic" | "700";
  /** Hair spaces between letters. */
  letterSpacing: number;
  upper: boolean;
}

/** Black type, no halos: bold block capitals for continents, capitals for nations, italics below. */
function typography(region: Region): Typography {
  if (region.kind === "ocean")
    return {
      sizePerRadius: 0.09,
      minSize: 13,
      maxSize: 24,
      style: "italic",
      letterSpacing: 5,
      upper: true,
    };
  if (region.depth === 1)
    return {
      sizePerRadius: 0.17,
      minSize: 12,
      maxSize: 30,
      style: "700",
      letterSpacing: 3,
      upper: true,
    };
  if (region.depth === 2)
    return {
      sizePerRadius: 0.17,
      minSize: 10,
      maxSize: 17,
      style: "normal",
      letterSpacing: 2,
      upper: true,
    };
  return {
    sizePerRadius: 0.22,
    minSize: 10,
    maxSize: 14,
    style: "italic",
    letterSpacing: 0,
    upper: false,
  };
}

/** Smallest region radius in CSS px worth naming, and how wide its name may run relative to that radius. */
function labelFit(kind: Kind): { minRadiusPx: number; maxWidthPerRadius: number } {
  switch (kind) {
    case "polar":
    case "unseen":
      return { minRadiusPx: 6, maxWidthPerRadius: 30 };
    case "island":
      return { minRadiusPx: 6, maxWidthPerRadius: 12 };
    case "ocean":
      return { minRadiusPx: 14, maxWidthPerRadius: 30 };
    case "world":
    case "continent":
    case "isles":
    case "land":
    case "file":
    case "bundle":
      return { minRadiusPx: 14, maxWidthPerRadius: 3.4 };
  }
}

/** Places region names and city symbols, largest first, skipping anything that would collide. */
export function drawPlaces(
  ctx: CanvasRenderingContext2D,
  camera: Camera,
  survey: Survey,
  layers: Layers,
  detailLevel: number,
  map: Rect,
) {
  const taken: Rect[] = [];
  const claim = (box: Rect) => {
    if (taken.some((other) => intersects(other, box))) return false;
    taken.push(box);
    return true;
  };
  const nearScreen = ({ x, y }: Point) =>
    x > -OFFSCREEN_X &&
    x < camera.width + OFFSCREEN_X &&
    y > -OFFSCREEN_Y &&
    y < camera.height + OFFSCREEN_Y;
  ctx.textBaseline = "middle";
  ctx.fillStyle = "#000";
  ctx.strokeStyle = "#000";

  const labels = [...layers.labels.values()]
    .map(({ item }) => ({ label: item, region: survey.region(item.id) }))
    .filter(({ region }) => region.kind !== "isles" && region.depth <= detailLevel + 1)
    .sort((a, b) => a.region.depth - b.region.depth || b.label.radius - a.label.radius);
  for (const { label, region } of labels) {
    const radiusPx = label.radius * camera.scale;
    const fit = labelFit(region.kind);
    if (
      radiusPx < fit.minRadiusPx ||
      radiusPx > Math.max(camera.width, camera.height) * MAX_LABEL_SHARE
    )
      continue;
    const anchor = camera.toScreen(label.x, label.y);
    if (!nearScreen(anchor)) continue;

    const type = typography(region);
    const size = clamp(radiusPx * type.sizePerRadius, type.minSize, type.maxSize);
    ctx.font = `${type.style} ${size}px ${TYPEFACE}`;
    const name = type.upper ? region.name.toUpperCase() : region.name;
    const text = name.split("").join(HAIR_SPACE.repeat(type.letterSpacing));
    const halfWidth = ctx.measureText(text).width / 2;
    if (2 * halfWidth > radiusPx * fit.maxWidthPerRadius) continue;

    const x = clamp(anchor.x, map.x0 + halfWidth + 1.5 * GAP, map.x1 - halfWidth - 1.5 * GAP);
    const y = clamp(anchor.y, map.y0 + size, map.y1 - size);
    const halfHeight = size * 0.65;
    if (
      !claim({
        x0: x - halfWidth - GAP,
        y0: y - halfHeight,
        x1: x + halfWidth + GAP,
        y1: y + halfHeight,
      })
    )
      continue;
    ctx.textAlign = "center";
    ctx.fillText(text, x, y);
  }

  const minPopulation = CITY_POPULATION / camera.zoom ** 2;
  const cities = [...layers.cities.values()]
    .map(({ item }) => ({ city: item, region: survey.region(item.id) }))
    .sort((a, b) => b.city.population - a.city.population);
  ctx.lineWidth = 1.2;
  for (const { city, region } of cities) {
    const name = region.city;
    const regionPx = (layers.labels.get(city.id)?.item.radius ?? 0) * camera.scale;
    if (!name || (city.population < minPopulation && regionPx < CITY_REGION_PX)) continue;
    const at = camera.toScreen(city.x, city.y);
    if (!nearScreen(at)) continue;

    const magnitude = Math.log10(Math.max(city.population, 10));
    const capital = magnitude > 4.5;
    const radius = 1.4 + 0.75 * magnitude;
    const size = 10.5 + clamp(magnitude - 2, 0, 3.5);
    ctx.font = `${magnitude > 4 ? "700" : "normal"} ${size}px ${TYPEFACE}`;
    const textWidth = ctx.measureText(name).width;
    const flip = at.x + radius + GAP + 1 + textWidth > map.x1 - GAP;
    const halfHeight = size * 0.6;
    const box = flip
      ? {
          x0: at.x - radius - GAP - 1 - textWidth,
          y0: at.y - halfHeight,
          x1: at.x + radius + 1,
          y1: at.y + halfHeight,
        }
      : {
          x0: at.x - radius - 1,
          y0: at.y - halfHeight,
          x1: at.x + radius + GAP + 1 + textWidth,
          y1: at.y + halfHeight,
        };
    if (!claim(box)) continue;

    // Free cities (small app bundles) are squares; capitals get a ring around a dot.
    ctx.beginPath();
    if (region.kind === "bundle")
      ctx.rect(at.x - radius * 0.8, at.y - radius * 0.8, radius * 1.6, radius * 1.6);
    else ctx.arc(at.x, at.y, capital ? radius : radius * 0.7, 0, 2 * Math.PI);
    if (capital) {
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(at.x, at.y, radius * 0.5, 0, 2 * Math.PI);
    }
    ctx.fill();
    ctx.textAlign = flip ? "right" : "left";
    ctx.fillText(name, flip ? at.x - radius - GAP : at.x + radius + GAP, at.y);
  }
}
