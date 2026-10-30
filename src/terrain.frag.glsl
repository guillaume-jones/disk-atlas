precision highp float;
in float v_elev;
in float v_shade;
in float v_ice;
in vec2 v_world;
// Noise octaves are chosen per frame in terrain.ts.
uniform int u_grainFirst;
uniform int u_grainCount;
uniform float u_grainWeights[GRAIN_OCTAVES];
uniform int u_coastCount;
uniform float u_coastAmps[COAST_OCTAVES];
// How far the octaves after each one can still move the sum.
uniform float u_coastTails[COAST_OCTAVES];
// Recency key: band colours oldest to newest, and the elevation where each later band begins.
uniform vec3 u_bands[BAND_COUNT];
uniform float u_edges[BAND_COUNT - 1];
// Theme: see TerrainStyle in theme.ts.
uniform vec3 u_shallows;
uniform vec3 u_sea;
uniform vec3 u_shoreLine;
uniform vec3 u_snow;
uniform vec3 u_ice;
uniform vec3 u_coast;
uniform vec3 u_paperTint;
uniform float u_contourOpacity;
uniform float u_hillShading;
uniform float u_shoreLineOpacity;
uniform float u_coastOpacity;
uniform float u_paperTintOpacity;
uniform float u_saturation;
uniform float u_grainFloor;
uniform float u_grainRange;
uniform float u_patchiness;
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
  float freq = GRAIN_FREQUENCY * pow(2.0, float(u_grainFirst));
  for (int i = 0; i < u_grainCount; i++) {
    sum += u_grainWeights[i] * hash(floor(p * freq) + float(u_grainFirst + i) * 7.1);
    freq *= 2.0;
  }
  return u_grainCount > 0 ? sum : 0.5;
}

float coastOctave(vec2 p, int k, float freq) {
  return u_coastAmps[k] * (vnoise(p * freq + float(k) * 17.3) - 0.5);
}

// Octaves fade in as their wavelength exceeds a few pixels, so coastlines keep fraying as you zoom.
float coastNoise(vec2 p) {
  float sum = 0.0;
  float freq = COAST_FREQUENCY;
  for (int k = 0; k < u_coastCount; k++) {
    sum += coastOctave(p, k, freq);
    freq *= 2.0;
  }
  return sum;
}

float bandEdgeDistance(float tone) {
  float nearest = 1e9;
  for (int i = 0; i < BAND_COUNT - 1; i++) nearest = min(nearest, abs(tone - u_edges[i]));
  return nearest;
}

// coastNoise, stopping once the remaining octaves can't carry e + gain * noise across a band edge or within `margin` of one.
float bandNoise(vec2 p, float e, float gain, float margin) {
  float sum = 0.0;
  float freq = COAST_FREQUENCY;
  for (int k = 0; k < u_coastCount; k++) {
    sum += coastOctave(p, k, freq);
    if (bandEdgeDistance(e + gain * sum) > gain * u_coastTails[k] + margin) break;
    freq *= 2.0;
  }
  return sum;
}

void main() {
  float nearCoast = 1.0 - smoothstep(0.0, 0.1, abs(v_elev));
  float noise = nearCoast > 0.0 ? coastNoise(v_world) : 0.0;
  float e = v_elev + noise * 0.05 * nearCoast;
  float aa = max(fwidth(e), 1e-6);
  float land = smoothstep(-aa, aa, e);

  // Water: pale printed blue with engraved water-lines hugging the shore.
  float depth = clamp(-e, 0.0, 1.0);
  vec3 water = depth < 0.04 ? u_shallows : u_sea;
  float k = depth * 55.0;
  float line = 1.0 - smoothstep(0.0, 1.1, abs(fract(k) - 0.5) / max(fwidth(k), 1e-5));
  water = mix(water, u_shoreLine, line * (1.0 - smoothstep(0.02, 0.09, depth)) * u_shoreLineOpacity);

  // Land: stepped tints with a thin contour at each band edge.
  float tone = e;
  if (u_patchiness > 0.0 && land > 0.0)
    tone += u_patchiness * (nearCoast > 0.0 ? noise : bandNoise(v_world, e, u_patchiness, aa));
  int band = 0;
  for (int i = 0; i < BAND_COUNT - 1; i++) {
    if (tone >= u_edges[i]) band = i + 1;
  }
  float nearestEdge = bandEdgeDistance(tone);
  vec3 ground = mix(u_bands[band], u_snow, smoothstep(0.98, 1.15, e));
  ground = mix(ground, ground * 0.6, (1.0 - smoothstep(0.4, 1.0, nearestEdge / aa)) * u_contourOpacity);
  if (v_ice > 0.5) ground = u_ice;
  ground *= mix(1.0, clamp(0.84 + 0.22 * v_shade, 0.9, 1.06), u_hillShading);

  vec3 col = mix(water, ground, land);
  float coast = 1.0 - smoothstep(0.0, 1.3, abs(e) / aa);
  col = mix(col, u_coast, coast * u_coastOpacity);

  // Age the print: warm the inks, mute saturation, add paper grain. The recency key in sheet.ts mirrors this.
  col = mix(col, col * u_paperTint, u_paperTintOpacity);
  float lum = dot(col, vec3(0.3, 0.59, 0.11));
  col = mix(vec3(lum), col, u_saturation);
  col *= u_grainFloor + (u_grainRange > 0.0 ? u_grainRange * grain(v_world) : 0.0);
  o = vec4(col, 1.0);
}
