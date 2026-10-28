const TEXTURE_PX = 1024;
const PLANKS = 5;
const GRAIN_LINES_PER_PLANK = 90;
/** Horizontal step between points along a grain line, in texture px. */
const GRAIN_STEP_PX = 8;

/** A tileable table top of planks with wavy grain, as a JPEG data URL. Seeded, so it's the same every launch. */
export function woodTexture(): string {
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

  const plank = TEXTURE_PX / PLANKS;
  for (let p = 0; p < PLANKS; p++) {
    const top = p * plank;
    const tone = 0.82 + random() * 0.3;
    g.fillStyle = `rgb(${92 * tone},${58 * tone},${34 * tone})`;
    g.fillRect(0, top, TEXTURE_PX, plank);

    for (let i = 0; i < GRAIN_LINES_PER_PLANK; i++) {
      const y = top + random() * plank;
      const amplitude = 2 + random() * 6;
      const frequency = 0.004 + random() * 0.01;
      const phase = random() * 10;
      g.strokeStyle =
        random() < 0.7
          ? `rgba(35,18,8,${0.05 + random() * 0.14})`
          : `rgba(190,130,80,${0.04 + random() * 0.07})`;
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
    g.fillStyle = "rgba(20,10,4,0.55)";
    g.fillRect(0, top, TEXTURE_PX, 2);
    g.fillStyle = "rgba(200,150,100,0.08)";
    g.fillRect(0, top + 2, TEXTURE_PX, 1);
  }
  return canvas.toDataURL("image/jpeg", 0.85);
}
