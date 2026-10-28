import type { PatchData } from "./api";

const VS = `#version 300 es
in vec2 a_pos;
in float a_elev;
in float a_shade;
in float a_ice;
uniform vec2 u_scale;
uniform vec2 u_offset;
uniform float u_z;
out float v_elev;
out float v_shade;
out float v_ice;
out vec2 v_world;
void main() {
  v_elev = a_elev;
  v_shade = a_shade;
  v_ice = a_ice;
  v_world = a_pos;
  gl_Position = vec4(a_pos * u_scale + u_offset, u_z, 1.0);
}`;

/** Band boundaries of the recency key, in days since last modified. */
export const BAND_DAYS = [730, 365, 180, 90, 45, 21, 7, 1];
/** Old-atlas hypsometric tints, oldest (lowland) to newest (highland). */
export const BAND_COLORS = ["#9eb489", "#abbd92", "#c1cd9f", "#d9daab", "#ede3b3", "#e8ce9e", "#d9b285", "#c29571", "#a57d61"];
const TAU_DAYS = 60;
const TAIL_DAYS = 400;
/** Mean land elevation the generator produces for a given age (base + average ridge contribution). */
const elevationFor = (days: number) => 0.075 + 0.71 * (0.85 * Math.exp(-days / TAU_DAYS) + 0.15 * Math.exp(-days / TAIL_DAYS));

const glFloat = (v: number) => v.toFixed(5);
const glColor = (hex: string) => `vec3(${[1, 3, 5].map((i) => glFloat(parseInt(hex.slice(i, i + 2), 16) / 255)).join(",")})`;

const FS = `#version 300 es
precision highp float;
in float v_elev;
in float v_shade;
in float v_ice;
in vec2 v_world;
uniform float u_px;
out vec4 o;

const float EDGES[${BAND_DAYS.length}] = float[](${BAND_DAYS.map((d) => glFloat(elevationFor(d))).join(",")});
const vec3 BANDS[${BAND_COLORS.length}] = vec3[](${BAND_COLORS.map(glColor).join(",")});

float hash(vec2 p) {
  p = fract(p * vec2(123.34, 456.21));
  p += dot(p, p + 45.32);
  return fract(p.x * p.y);
}

float vnoise(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1, 0)), u.x), mix(hash(i + vec2(0, 1)), hash(i + vec2(1, 1)), u.x), u.y);
}

// Paper speckle anchored to the map: each octave peaks around a 2 px wavelength, so it stays fine and moves with the map.
float grain(vec2 p) {
  float s = 0.0, w = 0.0, f = 400.0;
  for (int k = 0; k < 14; k++) {
    float px = log2(1.0 / (f * u_px)) - 1.0;
    if (px < -4.0) break;
    if (px < 4.0) {
      float a = exp(-px * px);
      s += a * hash(floor(p * f) + float(k) * 7.1);
      w += a;
    }
    f *= 2.0;
  }
  return w > 0.0 ? s / w : 0.5;
}

// Octaves fade in as their wavelength exceeds a few pixels, so coastlines keep fraying as you zoom.
float coastNoise(vec2 p) {
  float s = 0.0, a = 0.5, f = 90.0;
  for (int k = 0; k < 12; k++) {
    float w = smoothstep(2.0, 7.0, 1.0 / (f * u_px));
    if (w == 0.0) break;
    s += w * a * (vnoise(p * f + float(k) * 17.3) - 0.5);
    f *= 2.0;
    a *= 0.6;
  }
  return s;
}

void main() {
  float near = 1.0 - smoothstep(0.0, 0.1, abs(v_elev));
  float e = near > 0.0 ? v_elev + coastNoise(v_world) * 0.05 * near : v_elev;
  float aa = max(fwidth(e), 1e-6);
  float land = smoothstep(-aa, aa, e);

  // Water: pale printed blue with engraved water-lines hugging the shore.
  float depth = clamp(-e, 0.0, 1.0);
  vec3 water = depth < 0.04 ? vec3(0.81, 0.89, 0.88) : vec3(0.72, 0.83, 0.85);
  float k = depth * 55.0;
  float line = 1.0 - smoothstep(0.0, 1.1, abs(fract(k) - 0.5) / max(fwidth(k), 1e-5));
  water = mix(water, vec3(0.33, 0.47, 0.56), line * (1.0 - smoothstep(0.02, 0.09, depth)) * 0.55);

  // Land: stepped tints with a thin contour at each band edge.
  int band = 0;
  float nearest = 1e9;
  for (int i = 0; i < EDGES.length(); i++) {
    if (e >= EDGES[i]) band = i + 1;
    nearest = min(nearest, abs(e - EDGES[i]));
  }
  vec3 ground = mix(BANDS[band], vec3(0.95, 0.93, 0.88), smoothstep(0.98, 1.15, e));
  ground = mix(ground, ground * 0.6, (1.0 - smoothstep(0.4, 1.0, nearest / aa)) * 0.5);
  if (v_ice > 0.5) ground = vec3(0.93, 0.94, 0.92);
  ground *= clamp(0.84 + 0.22 * v_shade, 0.9, 1.06);

  vec3 col = mix(water, ground, land);
  float coast = 1.0 - smoothstep(0.0, 1.3, abs(e) / aa);
  col = mix(col, vec3(0.20, 0.22, 0.24), coast * 0.75);

  // Age the print: warm the inks, mute saturation, add paper grain.
  col = mix(col, col * vec3(1.0, 0.94, 0.80), 0.45);
  float lum = dot(col, vec3(0.3, 0.59, 0.11));
  col = mix(vec3(lum), col, 0.85);
  col *= 0.93 + 0.12 * grain(v_world);
  o = vec4(col, 1.0);
}`;

export interface GLPatch {
  vao: WebGLVertexArrayObject;
  buffers: WebGLBuffer[];
  count: number;
}

export interface View {
  cx: number;
  cy: number;
  scale: number;
  w: number;
  h: number;
  dpr: number;
}

export class Terrain {
  private gl: WebGL2RenderingContext;
  private prog: WebGLProgram;
  private u: Record<string, WebGLUniformLocation | null> = {};

  constructor(canvas: HTMLCanvasElement) {
    const gl = canvas.getContext("webgl2", { antialias: true, preserveDrawingBuffer: true });
    if (!gl) throw new Error("WebGL2 unavailable");
    this.gl = gl;
    const sh = (type: number, src: string) => {
      const s = gl.createShader(type)!;
      gl.shaderSource(s, src);
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS))
        throw new Error(gl.getShaderInfoLog(s) ?? "shader");
      return s;
    };
    const prog = gl.createProgram()!;
    gl.attachShader(prog, sh(gl.VERTEX_SHADER, VS));
    gl.attachShader(prog, sh(gl.FRAGMENT_SHADER, FS));
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS))
      throw new Error(gl.getProgramInfoLog(prog) ?? "link");
    this.prog = prog;
    for (const name of ["u_scale", "u_offset", "u_px", "u_z"])
      this.u[name] = gl.getUniformLocation(prog, name);
  }

  upload(d: PatchData): GLPatch {
    const gl = this.gl;
    const vao = gl.createVertexArray()!;
    const buffers: WebGLBuffer[] = [];
    gl.bindVertexArray(vao);
    const attr = (name: string, data: Float32Array, size: number) => {
      const loc = gl.getAttribLocation(this.prog, name);
      const buf = gl.createBuffer()!;
      buffers.push(buf);
      gl.bindBuffer(gl.ARRAY_BUFFER, buf);
      gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);
      gl.enableVertexAttribArray(loc);
      gl.vertexAttribPointer(loc, size, gl.FLOAT, false, 0, 0);
    };
    attr("a_pos", d.pos, 2);
    attr("a_elev", d.elev, 1);
    attr("a_shade", d.shade, 1);
    attr("a_ice", d.ice, 1);
    const idx = gl.createBuffer()!;
    buffers.push(idx);
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, idx);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, d.tris, gl.STATIC_DRAW);
    gl.bindVertexArray(null);
    return { vao, buffers, count: d.tris.length };
  }

  free(p: GLPatch) {
    this.gl.deleteVertexArray(p.vao);
    p.buffers.forEach((b) => this.gl.deleteBuffer(b));
  }

  /** `clip` is the map rectangle in device pixels (x, y from top, w, h); nothing renders outside it. */
  draw(patches: GLPatch[], v: View, clip: [number, number, number, number]) {
    const gl = this.gl;
    const canvas = gl.canvas as HTMLCanvasElement;
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.disable(gl.SCISSOR_TEST);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    gl.enable(gl.DEPTH_TEST);
    const [x, y, w, h] = clip.map(Math.round);
    gl.enable(gl.SCISSOR_TEST);
    gl.scissor(x, canvas.height - y - h, Math.max(0, w), Math.max(0, h));
    gl.useProgram(this.prog);
    gl.uniform2f(this.u.u_scale, (2 * v.scale) / v.w, (-2 * v.scale) / v.h);
    gl.uniform2f(this.u.u_offset, (-2 * v.cx * v.scale) / v.w, (2 * v.cy * v.scale) / v.h);
    gl.uniform1f(this.u.u_px, 1 / (v.scale * v.dpr));
    // Finest first: depth testing then skips shading the coarser layers they cover.
    for (let i = patches.length - 1; i >= 0; i--) {
      const p = patches[i];
      gl.uniform1f(this.u.u_z, 1 - (i + 1) / (patches.length + 1));
      gl.bindVertexArray(p.vao);
      gl.drawElements(gl.TRIANGLES, p.count, gl.UNSIGNED_INT, 0);
    }
    gl.bindVertexArray(null);
  }
}
