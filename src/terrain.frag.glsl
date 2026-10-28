precision highp float;
in float v_elev;
in float v_shade;
in float v_ice;
in vec2 v_world;
// World units per device pixel.
uniform float u_px;
// Recency key: band colours oldest to newest, and the elevation where each later band begins.
uniform vec3 u_bands[BAND_COUNT];
uniform float u_edges[BAND_COUNT - 1];
out vec4 o;

float hash(vec2 p) {
  p = fract(p * vec2(123.34, 456.21));
  p += dot(p, p + 45.32);
  return fract(p.x * p.y);
}

float vnoise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  float bottom = mix(hash(i), hash(i + vec2(1, 0)), u.x);
  float top = mix(hash(i + vec2(0, 1)), hash(i + vec2(1, 1)), u.x);
  return mix(bottom, top, u.y);
}

// Paper speckle anchored to the map: each octave peaks around a 2 px wavelength, so it stays fine and moves with the map.
float grain(vec2 p) {
  float sum = 0.0;
  float weight = 0.0;
  float freq = 400.0;
  for (int k = 0; k < 14; k++) {
    float octavesFromPeak = log2(1.0 / (freq * u_px)) - 1.0;
    if (octavesFromPeak < -4.0) break;
    if (octavesFromPeak < 4.0) {
      float a = exp(-octavesFromPeak * octavesFromPeak);
      sum += a * hash(floor(p * freq) + float(k) * 7.1);
      weight += a;
    }
    freq *= 2.0;
  }
  return weight > 0.0 ? sum / weight : 0.5;
}

// Octaves fade in as their wavelength exceeds a few pixels, so coastlines keep fraying as you zoom.
float coastNoise(vec2 p) {
  float sum = 0.0;
  float amp = 0.5;
  float freq = 90.0;
  for (int k = 0; k < 12; k++) {
    float w = smoothstep(2.0, 7.0, 1.0 / (freq * u_px));
    if (w == 0.0) break;
    sum += w * amp * (vnoise(p * freq + float(k) * 17.3) - 0.5);
    freq *= 2.0;
    amp *= 0.6;
  }
  return sum;
}

const vec3 SHALLOWS = vec3(0.81, 0.89, 0.88);
const vec3 SEA = vec3(0.72, 0.83, 0.85);
const vec3 WATER_LINE = vec3(0.33, 0.47, 0.56);
const vec3 SNOW = vec3(0.95, 0.93, 0.88);
const vec3 ICE = vec3(0.93, 0.94, 0.92);
const vec3 COAST = vec3(0.20, 0.22, 0.24);
const vec3 AGED_INK = vec3(1.0, 0.94, 0.80);

void main() {
  float nearCoast = 1.0 - smoothstep(0.0, 0.1, abs(v_elev));
  float e = nearCoast > 0.0 ? v_elev + coastNoise(v_world) * 0.05 * nearCoast : v_elev;
  float aa = max(fwidth(e), 1e-6);
  float land = smoothstep(-aa, aa, e);

  // Water: pale printed blue with engraved water-lines hugging the shore.
  float depth = clamp(-e, 0.0, 1.0);
  vec3 water = depth < 0.04 ? SHALLOWS : SEA;
  float k = depth * 55.0;
  float line = 1.0 - smoothstep(0.0, 1.1, abs(fract(k) - 0.5) / max(fwidth(k), 1e-5));
  water = mix(water, WATER_LINE, line * (1.0 - smoothstep(0.02, 0.09, depth)) * 0.55);

  // Land: stepped tints with a thin contour at each band edge.
  int band = 0;
  float nearestEdge = 1e9;
  for (int i = 0; i < BAND_COUNT - 1; i++) {
    if (e >= u_edges[i]) band = i + 1;
    nearestEdge = min(nearestEdge, abs(e - u_edges[i]));
  }
  vec3 ground = mix(u_bands[band], SNOW, smoothstep(0.98, 1.15, e));
  ground = mix(ground, ground * 0.6, (1.0 - smoothstep(0.4, 1.0, nearestEdge / aa)) * 0.5);
  if (v_ice > 0.5) ground = ICE;
  ground *= clamp(0.84 + 0.22 * v_shade, 0.9, 1.06);

  vec3 col = mix(water, ground, land);
  float coast = 1.0 - smoothstep(0.0, 1.3, abs(e) / aa);
  col = mix(col, COAST, coast * 0.75);

  // Age the print: warm the inks, mute saturation, add paper grain. The recency key in sheet.ts mirrors this.
  col = mix(col, col * AGED_INK, 0.45);
  float lum = dot(col, vec3(0.3, 0.59, 0.11));
  col = mix(vec3(lum), col, 0.85);
  col *= 0.93 + 0.12 * grain(v_world);
  o = vec4(col, 1.0);
}
