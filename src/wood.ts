const TEXTURE_PX = 1024;
/** Horizontal step between points along a grain line, in texture px. */
const GRAIN_STEP_PX = 8;

type Rgb = readonly [number, number, number];

export interface Wood {
  color: Rgb;
  /** Tone difference between planks. */
  toneSpread: number;
  planks: number;
  grainLines: number;
  waviness: number;
  darkGrain: Rgb;
  lightGrain: Rgb;
  grainStrength: number;
  /** Opacity of the gap between planks. */
  seam: number;
}

/** A tileable table top of planks with wavy grain, as a JPEG data URL. Seeded, so it's the same every launch. */
export function woodTexture(wood: Wood): string {
  const canvas = document.createElement("canvas");
  canvas.width = TEXTURE_PX;
  canvas.height = TEXTURE_PX;
  const g = canvas.getContext("2d");
  if (!g) return "";

  // Park–Miller minimal standard generator.
  let seed = 7;
  const random = () => {
    seed = (seed * 16807) % 2147483647;
    return seed / 2147483647;
  };

  const [red, green, blue] = wood.color;
  const plank = TEXTURE_PX / wood.planks;
  for (let p = 0; p < wood.planks; p++) {
    const top = p * plank;
    const tone = 1 - wood.toneSpread * 0.6 + random() * wood.toneSpread;
    g.fillStyle = `rgb(${red * tone},${green * tone},${blue * tone})`;
    g.fillRect(0, top, TEXTURE_PX, plank);

    for (let i = 0; i < wood.grainLines; i++) {
      const y = top + random() * plank;
      const amplitude = (2 + random() * 6) * wood.waviness;
      const frequency = 0.004 + random() * 0.01;
      const phase = random() * 10;
      g.strokeStyle =
        random() < 0.7
          ? `rgba(${wood.darkGrain.join()},${(0.05 + random() * 0.14) * wood.grainStrength})`
          : `rgba(${wood.lightGrain.join()},${(0.04 + random() * 0.07) * wood.grainStrength})`;
      g.lineWidth = 0.6 + random() * 1.8;
      g.beginPath();
      for (let x = 0; x <= TEXTURE_PX; x += GRAIN_STEP_PX) {
        const wave =
          Math.sin(x * frequency + phase) * amplitude +
          Math.sin(x * frequency * 3.1) * amplitude * 0.3;
        g.lineTo(x, Math.min(top + plank - 1, Math.max(top + 1, y + wave)));
      }
      g.stroke();
    }

    // Dark seam, then a faint highlight on the next plank's edge.
    g.fillStyle = `rgba(20,10,4,${wood.seam})`;
    g.fillRect(0, top, TEXTURE_PX, 2);
    g.fillStyle = `rgba(200,150,100,${wood.seam * 0.15})`;
    g.fillRect(0, top + 2, TEXTURE_PX, 1);
  }

  return canvas.toDataURL("image/jpeg", 0.85);
}
