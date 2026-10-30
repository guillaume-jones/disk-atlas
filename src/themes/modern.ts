import type { Marker, Theme, Typography } from "../theme";

const LABEL = "#202124";
const FONT = `Roboto, system-ui, sans-serif`;
const type = { ink: LABEL, halo: "#fff", upper: false, letterSpacing: 0 } satisfies Partial<Typography>;

/** A white dot, with a black centre for capitals. */
const pin: Marker = (ctx, { x, y, radius, capital }) => {
  ctx.fillStyle = "#fff";
  ctx.strokeStyle = "#5f6368";
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.arc(x, y, radius * 0.7, 0, 2 * Math.PI);
  ctx.fill();
  ctx.stroke();
  if (!capital) return;
  ctx.fillStyle = "#000";
  ctx.beginPath();
  ctx.arc(x, y, radius * 0.35, 0, 2 * Math.PI);
  ctx.fill();
};

/** A web map: flat colour, no shading, sans-serif labels with white halos. */
export const modern: Theme = {
  id: "today",
  label: "Today",
  vocabulary: {
    landmarks: {
      world: "World",
      polar: "System Territory",
      unseen: "Restricted Area",
      isles: "Minor Outlying Islands",
      ocean: "Open Ocean",
    },
    continent: ["{}", "{}ia", "North {}", "{}land"],
    island: ["{} Island", "{} Islands", "{}"],
    nation: ["{}", "{}a", "{}land", "{}stan", "United {}", "{}ia", "Republic of {}"],
    province: ["{}", "{} County", "{} Territory", "New {}", "{}a"],
    locale: ["{} Valley", "{} Hills", "{} Park", "{} Lake", "{} Heights"],
    glacier: ["{} Base Area", "{} Ice Shelf", "{} Sector"],
    barrens: ["{} Basin", "{} Flats", "{} Reserve"],
    freeCity: ["{} City", "{} Special Zone"],
    city: [
      "{}", "{} City", "{}ville", "{}ton", "{} Springs", "{} Falls", "{} Beach", "San {}", "{}burg",
      "Lake {}", "{} Heights", "{}port", "Santa {}", "Fort {}",
    ],
    outpost: ["{} Station", "{} Research Base", "Camp {}"],
    qualifiers: ["New", "East", "West", "North", "South", "Greater", "Upper", "Lower", "Central", "Little"],
    phonology: {
      consonants: [
        ["t", "tl", "k", "m", "n", "sh", "w", "y", "h"],
        ["m", "n", "s", "k", "w", "t", "p", "sh"],
        ["q", "ch", "p", "t", "k", "y", "r", "w", "m"],
        ["h", "k", "l", "m", "n", "p", "w"],
        ["t", "k", "n", "m", "g", "w", "b", "z"],
      ],
      vowels: [
        ["a", "e", "i", "o"],
        ["a", "i", "u"],
        ["a", "e", "i", "o", "u"],
      ],
      finals: [["n", "k", "t"], ["n", "m"], ["k", "q", "n"], ["n", "s"]],
      shapes: [["CV", "CVF"], ["CV", "CV"], ["CV", "V", "CV"], ["CV", "CV", "CVF"]],
      suffixes: [
        ["wa", "tuck", "sett"],
        ["pan", "tlan", "co"],
        ["cha", "pampa", "yacu"],
        ["hoe", "lani", "nui"],
        ["ville", "ee", "kee"],
      ],
    },
    invention: 0.35,
  },
  terrain: {
    bands: [
      "#7fb58f", "#92c4a0", "#a9d4b4", "#c0e4cb", "#d6e9d0", "#ebe8d2", "#f3ead6", "#f8f3e8",
      "#ffffff",
    ],
    colors: {
      shallows: "#7fd0e5",
      sea: "#7fd0e5",
      shoreLine: "#7fd0e5",
      snow: "#ffffff",
      ice: "#f1f3f4",
      coast: "#7fd0e5",
      paperTint: "#ffffff",
    },
    params: {
      contourOpacity: 0,
      hillShading: 0,
      shoreLineOpacity: 0,
      coastOpacity: 0,
      paperTintOpacity: 0,
      saturation: 1,
      grainFloor: 1,
      grainRange: 0,
      patchiness: 0.16,
    },
  },
  ink: {
    graticule: null,
    river: "#7fd0e5",
    borders: [
      { color: "rgba(60,64,67,0.85)", width: 1, dash: [] },
      { color: "rgba(60,64,67,0.7)", width: 0.9, dash: [] },
      { color: "rgba(60,64,67,0.6)", width: 0.8, dash: [4, 3] },
      { color: "rgba(60,64,67,0.45)", width: 0.7, dash: [2, 3] },
    ],
    frame: null,
  },
  places: {
    ocean: { ...type, sizePerRadius: 0.09, minSize: 12, maxSize: 20, style: "normal", ink: "#1a7a9c", halo: null },
    continent: { ...type, sizePerRadius: 0.16, minSize: 12, maxSize: 22, style: "700" },
    nation: { ...type, sizePerRadius: 0.16, minSize: 11, maxSize: 15, style: "700" },
    local: { ...type, sizePerRadius: 0.2, minSize: 10, maxSize: 12, style: "normal" },
    city: type,
    marker: pin,
    markerScale: 0.8,
  },
  table: {
    color: [212, 176, 130],
    toneSpread: 0.05,
    planks: 3,
    grainLines: 130,
    waviness: 0.35,
    darkGrain: [130, 90, 50],
    lightGrain: [240, 215, 175],
    grainStrength: 0.9,
    seam: 0.1,
  },
  ui: {
    ink: LABEL,
    "ink-soft": "#5f6368",
    "print-ink": LABEL,
    accent: "#1a73e8",
    paper: "#ffffff",
    "paper-hover": "#f1f3f4",
    "paper-light": "transparent",
    "paper-dark": "transparent",
    "paper-fibre": "none",
    table: "#d4b082",
    vignette: "transparent",
    "title-backdrop": "rgba(32, 33, 36, 0.3)",
    rule: "rgba(32, 33, 36, 0.3)",
    "note-paper": "#ffffff",
    "note-margin": "transparent",
    "note-lines": "transparent",
    "legend-letter-spacing": "0.04em",
    "heading-case": "none",
    "key-border": "1px solid #dadce0",
    "ticket-border": "1px solid #dadce0",
    "corner-radius": "8px",
    "display-font": FONT,
    "body-font": FONT,
    "text-font": FONT,
  },
};
