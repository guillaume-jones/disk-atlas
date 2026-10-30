import type { Kind } from "./api";
import type { Camera } from "./camera";
import { clamp, intersects, type Point, type Rect } from "./geometry";
import type { Layers } from "./layers";
import type { Survey } from "./survey";
import type { Theme } from "./theme";

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
const HALO_PX = 3;

/** Sets `text` at (x, y) under the current alignment, over an outline when the theme has one. */
function print(ctx: CanvasRenderingContext2D, text: string, x: number, y: number, halo: string | null) {
  if (halo) {
    ctx.strokeStyle = halo;
    ctx.lineWidth = HALO_PX;
    ctx.strokeText(text, x, y);
  }
  ctx.fillText(text, x, y);
}

const ICE_FIT = { minRadiusPx: 6, maxWidthPerRadius: 30 };
const LAND_FIT = { minRadiusPx: 14, maxWidthPerRadius: 3.4 };
/** Smallest region radius in CSS px worth naming, and how wide its name may run relative to that radius. */
const LABEL_FIT: Record<Kind, typeof LAND_FIT> = {
  polar: ICE_FIT,
  unseen: ICE_FIT,
  island: { minRadiusPx: 6, maxWidthPerRadius: 12 },
  ocean: { minRadiusPx: 14, maxWidthPerRadius: 30 },
  world: LAND_FIT,
  continent: LAND_FIT,
  isles: LAND_FIT,
  land: LAND_FIT,
  file: LAND_FIT,
  bundle: LAND_FIT,
};

/** Places region names and city symbols, largest first, skipping anything that would collide. */
export function drawPlaces(
  ctx: CanvasRenderingContext2D,
  camera: Camera,
  survey: Survey,
  layers: Layers,
  detailLevel: number,
  map: Rect,
  { places, ui }: Theme,
) {
  const typeface = ui["text-font"];
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

  const labels = [...layers.labels.values()]
    .map(({ item }) => ({ label: item, region: survey.region(item.id) }))
    .filter(({ region }) => region.kind !== "isles" && region.depth <= detailLevel + 1)
    .sort((a, b) => a.region.depth - b.region.depth || b.label.radius - a.label.radius);
  for (const { label, region } of labels) {
    const radiusPx = label.radius * camera.scale;
    const fit = LABEL_FIT[region.kind];
    if (
      radiusPx < fit.minRadiusPx ||
      radiusPx > Math.max(camera.width, camera.height) * MAX_LABEL_SHARE
    )
      continue;
    const anchor = camera.toScreen(label.x, label.y);
    if (!nearScreen(anchor)) continue;

    const type =
      region.kind === "ocean"
        ? places.ocean
        : region.depth === 1
          ? places.continent
          : region.depth === 2
            ? places.nation
            : places.local;
    const size = clamp(radiusPx * type.sizePerRadius, type.minSize, type.maxSize);
    ctx.font = `${type.style} ${size}px ${typeface}`;
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
    ctx.fillStyle = type.ink;
    print(ctx, text, x, y, type.halo);
  }

  const minPopulation = CITY_POPULATION / camera.zoom ** 2;
  const cities = [...layers.cities.values()]
    .map(({ item }) => ({ city: item, region: survey.region(item.id) }))
    .sort((a, b) => b.city.population - a.city.population);
  for (const { city, region } of cities) {
    const name = region.city;
    const regionPx = (layers.labels.get(city.id)?.item.radius ?? 0) * camera.scale;
    if (!name || (city.population < minPopulation && regionPx < CITY_REGION_PX)) continue;
    const at = camera.toScreen(city.x, city.y);
    if (!nearScreen(at)) continue;

    const magnitude = Math.log10(Math.max(city.population, 10));
    const capital = magnitude > 4.5;
    const radius = (1.4 + 0.75 * magnitude) * places.markerScale;
    const size = 10.5 + clamp(magnitude - 2, 0, 3.5);
    ctx.font = `${magnitude > 4 ? "700" : "normal"} ${size}px ${typeface}`;
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

    places.marker(ctx, { ...at, radius, capital, free: region.kind === "bundle" });
    ctx.textAlign = flip ? "right" : "left";
    ctx.fillStyle = places.city.ink;
    const x = flip ? at.x - radius - GAP : at.x + radius + GAP;
    print(ctx, name, x, at.y, places.city.halo);
  }
}
