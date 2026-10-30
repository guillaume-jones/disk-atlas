import type { Marker, Theme, Typography } from "../theme";

const FIBRE = `<svg xmlns='http://www.w3.org/2000/svg' width='160' height='160'><filter id='n'><feTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='2' stitchTiles='stitch'/><feColorMatrix values='0 0 0 0 0.35 0 0 0 0 0.25 0 0 0 0 0.12 0 0 0 0.09 0'/></filter><rect width='160' height='160' filter='url(#n)'/></svg>`;

const type = { ink: "#000", halo: null } satisfies Partial<Typography>;

/** Free cities are squares; capitals get a ring around a dot. */
const dot: Marker = (ctx, { x, y, radius, capital, free }) => {
  ctx.fillStyle = "#000";
  ctx.strokeStyle = "#000";
  ctx.lineWidth = 1.2;
  ctx.beginPath();
  if (free) ctx.rect(x - radius * 0.8, y - radius * 0.8, radius * 1.6, radius * 1.6);
  else ctx.arc(x, y, capital ? radius : radius * 0.7, 0, 2 * Math.PI);
  if (capital) {
    ctx.stroke();
    ctx.beginPath();
    ctx.arc(x, y, radius * 0.5, 0, 2 * Math.PI);
  }
  ctx.fill();
};

/** The 1900s school atlas: engraved sea, stepped hypsometric tints, black type. */
export const edwardian: Theme = {
  id: "1900",
  label: "1900",
  vocabulary: {
    landmarks: {
      world: "The Known World",
      polar: "The System Ice",
      unseen: "The Unseen Ice",
      isles: "The Scattered Isles",
      ocean: "The Unclaimed Sea",
    },
    continent: ["{}land", "{}ia", "Greater {}"],
    island: ["Isle of {}", "{} Island", "The {} Isles"],
    nation: ["Kingdom of {}", "{}stan", "Republic of {}", "Duchy of {}", "{}mark"],
    province: ["{}shire", "{} Province", "March of {}", "{} Vale"],
    locale: ["{} Downs", "{} Reach", "{} Fells", "{} Heath", "{} Hollow", "{} Weald"],
    glacier: ["{} Glacier", "{} Icefield", "{} Shelf"],
    barrens: ["{} Barrens", "{} Expanse", "{} Plateau"],
    freeCity: ["Free City of {}", "{} Free City"],
    city: [
      "{}ton", "{}burg", "Port {}", "{}ville", "{}ham", "Fort {}", "{}ford", "{}field", "{}stead",
      "{}minster", "{}holm", "{}wick", "{}by", "{}mouth", "{}dale", "{}haven", "Saint {}",
      "{}polis", "{}grad", "Mount {}", "{}bridge", "{}port",
    ],
    outpost: ["Fort {}", "{} Base", "{} Station", "Camp {}", "Outpost {}"],
    qualifiers: ["Upper", "Lower", "North", "South", "East", "West", "New", "Old", "Far", "Little"],
    phonology: {
      consonants: [
        ["p", "t", "k", "m", "n", "l", "s", "r"],
        ["b", "d", "g", "m", "n", "l", "r", "v", "z"],
        ["t", "k", "sh", "m", "n", "r", "h", "y"],
        ["th", "d", "l", "r", "n", "v", "g", "m", "s"],
        ["k", "kh", "t", "z", "r", "n", "b", "d", "g"],
        ["f", "v", "l", "r", "m", "n", "s", "t", "c"],
        ["q", "k", "t", "m", "n", "r", "s", "l", "y", "w"],
      ],
      vowels: [
        ["a", "e", "i", "o", "u"],
        ["a", "i", "u"],
        ["a", "e", "o", "ae", "ia"],
        ["e", "i", "o", "y", "ei"],
        ["a", "o", "u", "au", "ou"],
      ],
      finals: [
        ["n", "r", "l", "s"],
        ["m", "n", "th"],
        ["k", "t", "r"],
        ["n", "l", "sh", "x"],
        ["n", "r"],
      ],
      shapes: [["CV", "CVF", "V"], ["CV", "CVF"], ["CV", "VF", "CVF"], ["CV", "CV", "CVF"]],
      suffixes: [
        ["ia", "or", "an"],
        ["eth", "ium", "os"],
        ["ar", "en", "is"],
        ["a", "ul", "heim"],
        ["ara", "oth", "und"],
        ["ane", "orn", "ys"],
      ],
    },
    invention: 0,
  },
  terrain: {
    bands: [
      "#9eb489", "#abbd92", "#c1cd9f", "#d9daab", "#ede3b3", "#e8ce9e", "#d9b285", "#c29571",
      "#a57d61",
    ],
    colors: {
      shallows: "#cfe3e0",
      sea: "#b8d4d9",
      shoreLine: "#54788f",
      snow: "#f2ede0",
      ice: "#edf0eb",
      coast: "#33383d",
      paperTint: "#fff0cc",
    },
    params: {
      contourOpacity: 0.5,
      hillShading: 1,
      shoreLineOpacity: 0.55,
      coastOpacity: 0.75,
      paperTintOpacity: 0.45,
      saturation: 0.85,
      grainFloor: 0.93,
      grainRange: 0.12,
      patchiness: 0,
    },
  },
  ink: {
    graticule: { color: "rgba(20,20,30,0.3)", width: 0.8 },
    river: "rgba(52,84,110,0.85)",
    borders: [
      { color: "rgba(40,28,20,0.85)", width: 1.5, dash: [] },
      { color: "rgba(122,36,24,0.75)", width: 1.3, dash: [7, 2.5, 1.5, 2.5] },
      { color: "rgba(122,52,36,0.6)", width: 1, dash: [3, 2.5] },
      { color: "rgba(90,60,45,0.5)", width: 0.8, dash: [1, 2.5] },
    ],
    frame: "#000",
  },
  places: {
    ocean: { ...type, sizePerRadius: 0.09, minSize: 13, maxSize: 24, style: "italic", letterSpacing: 5, upper: true },
    continent: { ...type, sizePerRadius: 0.17, minSize: 12, maxSize: 30, style: "700", letterSpacing: 3, upper: true },
    nation: { ...type, sizePerRadius: 0.17, minSize: 10, maxSize: 17, style: "normal", letterSpacing: 2, upper: true },
    local: { ...type, sizePerRadius: 0.22, minSize: 10, maxSize: 14, style: "italic", letterSpacing: 0, upper: false },
    city: type,
    marker: dot,
    markerScale: 1,
  },
  table: {
    color: [92, 58, 34],
    toneSpread: 0.3,
    planks: 5,
    grainLines: 90,
    waviness: 1,
    darkGrain: [35, 18, 8],
    lightGrain: [190, 130, 80],
    grainStrength: 1,
    seam: 0.55,
  },
  ui: {
    ink: "#2a1f16",
    "ink-soft": "#6e5a45",
    "print-ink": "#000",
    accent: "#7a2a1a",
    paper: "#efe5cc",
    "paper-hover": "#e6dab9",
    "paper-light": "rgba(255, 250, 235, 0.5)",
    "paper-dark": "rgba(150, 110, 60, 0.12)",
    "paper-fibre": `url("data:image/svg+xml,${encodeURIComponent(FIBRE)}")`,
    table: "#4a2f1c",
    vignette: "rgba(10, 5, 2, 0.55)",
    "title-backdrop": "rgba(20, 10, 4, 0.35)",
    rule: "rgba(42, 31, 22, 0.6)",
    "note-paper": "#f4eedc",
    "note-margin": "rgba(170, 60, 50, 0.4)",
    "note-lines": "rgba(70, 100, 140, 0.18)",
    "legend-letter-spacing": "0.16em",
    "heading-case": "uppercase",
    "key-border": "1px solid #000",
    "ticket-border": "3px double #000",
    "corner-radius": "0",
    "display-font": `"IM Fell English SC", Georgia, serif`,
    "body-font": `"IM Fell English", Georgia, serif`,
    "text-font": `"Old Standard TT", Georgia, serif`,
  },
};
