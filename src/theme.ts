import type { Vocabulary } from "./names";
import type { Wood } from "./wood";

/** What a city marker needs to know. */
interface Site {
  x: number;
  y: number;
  radius: number;
  capital: boolean;
  /** A free city: a small app bundle. */
  free: boolean;
}

export type Marker = (ctx: CanvasRenderingContext2D, site: Site) => void;

export interface BorderStyle {
  color: string;
  width: number;
  dash: readonly number[];
}

export interface Typography {
  sizePerRadius: number;
  minSize: number;
  maxSize: number;
  style: "normal" | "italic" | "700";
  /** Hair spaces between letters. */
  letterSpacing: number;
  upper: boolean;
  ink: string;
  /** Colour of an outline behind the letters. */
  halo: string | null;
}

/** Every key is a `u_<key>` uniform in terrain.frag.glsl. */
export interface TerrainStyle {
  /** One hex tint per recency band, oldest to newest. */
  bands: readonly string[];
  colors: Record<
    "shallows" | "sea" | "shoreLine" | "snow" | "ice" | "coast" | "paperTint",
    string
  >;
  params: Record<
    | "contourOpacity"
    | "hillShading"
    | "shoreLineOpacity"
    | "coastOpacity"
    | "paperTintOpacity"
    | "saturation"
    /** Paper grain multiplies the map by `grainFloor` up to `grainFloor + grainRange`; a range of 0 turns it off. */
    | "grainFloor"
    | "grainRange"
    /** Frays the edges between bands, like land-cover data. */
    | "patchiness",
    number
  >;
}

type PageStyle =
  | "ink"
  | "ink-soft"
  | "print-ink"
  | "accent"
  | "paper"
  | "paper-hover"
  | "paper-light"
  | "paper-dark"
  | "paper-fibre"
  | "table"
  | "vignette"
  | "title-backdrop"
  | "rule"
  | "note-paper"
  | "note-margin"
  | "note-lines"
  | "heading-case"
  | "legend-letter-spacing"
  | "key-border"
  | "ticket-border"
  | "corner-radius"
  | "display-font"
  | "body-font"
  | "text-font";

export interface Theme {
  id: string;
  label: string;
  vocabulary: Vocabulary;
  terrain: TerrainStyle;
  ink: {
    /** Omitted lines and frames aren't drawn. */
    graticule: { color: string; width: number } | null;
    river: string;
    /** By level: continent, nation, province, then everything finer. */
    borders: readonly BorderStyle[];
    frame: string | null;
  };
  places: {
    ocean: Typography;
    continent: Typography;
    nation: Typography;
    local: Typography;
    city: Pick<Typography, "ink" | "halo">;
    marker: Marker;
    markerScale: number;
  };
  table: Wood;
  /** CSS custom properties for the page around the map; `text-font` also sets the map's own lettering. */
  ui: Record<PageStyle, string>;
}

export function rgb(hex: string): [number, number, number] {
  const channel = (at: number) => parseInt(hex.slice(at, at + 2), 16) / 255;
  return [channel(1), channel(3), channel(5)];
}
