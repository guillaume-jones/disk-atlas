import type { Patch } from "./api";
import type { Camera } from "./camera";
import { clamp, intersects, type Rect } from "./geometry";
import { BAND_EDGES, RECENCY_BANDS } from "./recency";
import { rgb, type TerrainStyle } from "./theme";
import fragmentSource from "./terrain.frag.glsl?raw";
import vertexSource from "./terrain.vert.glsl?raw";

const GRAIN_BASE_FREQUENCY = 400;
const COAST_BASE_FREQUENCY = 90;
const COAST_OCTAVES = 12;
const COAST_GAIN = 0.6;
/** Grain octaves further than this from their 2 px peak weigh under 2% and are skipped. */
const GRAIN_REACH = 2;
const GRAIN_MAX_OCTAVE = 14;
const GRAIN_OCTAVES = 4;
const MULTISAMPLES = 4;

/** Cells per side of the grid each mesh's triangles are sorted into, so only on-screen cells get drawn. */
const MESH_GRID = 16;

interface MeshCell {
  bounds: Rect;
  first: number;
  count: number;
}

export interface GLPatch {
  vao: WebGLVertexArrayObject;
  buffers: WebGLBuffer[];
  cells: MeshCell[];
}

/** Sorts `patch.tris` by grid cell, returning the reordered indices and the cells' index ranges. */
function sortedByCell({ pos, tris, ox, oy, cols, rows, spacing }: Patch) {
  const cellOf = new Uint16Array(tris.length / 3);
  const counts = new Uint32Array(MESH_GRID ** 2);
  const bounds = new Float32Array(4 * MESH_GRID ** 2).fill(Infinity);
  for (let t = 0; t < cellOf.length; t++) {
    const first = tris[3 * t] ?? 0;
    const column = Math.floor((((pos[2 * first] ?? 0) - ox) / (cols * spacing)) * MESH_GRID);
    const row = Math.floor((((pos[2 * first + 1] ?? 0) - oy) / (rows * spacing)) * MESH_GRID);
    const cell = clamp(row, 0, MESH_GRID - 1) * MESH_GRID + clamp(column, 0, MESH_GRID - 1);
    cellOf[t] = cell;
    counts[cell] = (counts[cell] ?? 0) + 3;
    for (let corner = 0; corner < 3; corner++) {
      const vertex = tris[3 * t + corner] ?? 0;
      const x = pos[2 * vertex] ?? 0;
      const y = pos[2 * vertex + 1] ?? 0;
      bounds[4 * cell] = Math.min(bounds[4 * cell] ?? 0, x);
      bounds[4 * cell + 1] = Math.min(bounds[4 * cell + 1] ?? 0, y);
      bounds[4 * cell + 2] = Math.min(bounds[4 * cell + 2] ?? 0, -x);
      bounds[4 * cell + 3] = Math.min(bounds[4 * cell + 3] ?? 0, -y);
    }
  }
  const cells: MeshCell[] = [];
  const cursors = new Uint32Array(counts.length);
  let next = 0;
  for (const [cell, count] of counts.entries()) {
    cursors[cell] = next;
    if (count === 0) continue;
    cells.push({
      bounds: {
        x0: bounds[4 * cell] ?? 0,
        y0: bounds[4 * cell + 1] ?? 0,
        x1: -(bounds[4 * cell + 2] ?? 0),
        y1: -(bounds[4 * cell + 3] ?? 0),
      },
      first: next,
      count,
    });
    next += count;
  }
  const sorted = new Uint32Array(tris.length);
  for (const [t, cell] of cellOf.entries()) {
    const at = cursors[cell] ?? 0;
    sorted.set(tris.subarray(3 * t, 3 * t + 3), at);
    cursors[cell] = at + 3;
  }
  return { sorted, cells };
}

/** WebGL2 hypsometric terrain, one mesh per loaded patch. */
export class Terrain {
  private readonly gl: WebGL2RenderingContext;
  private readonly program: WebGLProgram;
  private readonly multisampled: WebGLFramebuffer;
  private readonly multisampledColor: WebGLRenderbuffer;
  private readonly multisampledDepth: WebGLRenderbuffer;
  private multisampledSize = "";
  private readonly uniforms: Record<
    | "scale"
    | "offset"
    | "depth"
    | "grainFirst"
    | "grainCount"
    | "grainWeights"
    | "coastCount"
    | "coastAmps"
    | "coastTails",
    WebGLUniformLocation | null
  >;

  constructor(private readonly canvas: HTMLCanvasElement) {
    const gl = canvas.getContext("webgl2", { antialias: false });
    if (!gl) throw new Error("WebGL2 unavailable");
    this.gl = gl;

    const header = `#version 300 es\n#define BAND_COUNT ${RECENCY_BANDS.length}\n#define GRAIN_OCTAVES ${GRAIN_OCTAVES}\n#define COAST_OCTAVES ${COAST_OCTAVES}\n#define GRAIN_FREQUENCY ${GRAIN_BASE_FREQUENCY}.0\n#define COAST_FREQUENCY ${COAST_BASE_FREQUENCY}.0\n`;
    const compile = (type: GLenum, source: string) => {
      const shader = gl.createShader(type);
      if (!shader) throw new Error("Could not create shader");
      gl.shaderSource(shader, header + source);
      gl.compileShader(shader);
      if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS))
        throw new Error(gl.getShaderInfoLog(shader) ?? "Shader error");
      return shader;
    };
    const program = gl.createProgram();
    gl.attachShader(program, compile(gl.VERTEX_SHADER, vertexSource));
    gl.attachShader(program, compile(gl.FRAGMENT_SHADER, fragmentSource));
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS))
      throw new Error(gl.getProgramInfoLog(program) ?? "Link error");
    this.program = program;

    this.uniforms = {
      scale: this.uniform("scale"),
      offset: this.uniform("offset"),
      depth: this.uniform("depth"),
      grainFirst: this.uniform("grainFirst"),
      grainCount: this.uniform("grainCount"),
      grainWeights: this.uniform("grainWeights"),
      coastCount: this.uniform("coastCount"),
      coastAmps: this.uniform("coastAmps"),
      coastTails: this.uniform("coastTails"),
    };
    gl.useProgram(program);
    gl.uniform1fv(this.uniform("edges"), BAND_EDGES);

    this.multisampled = gl.createFramebuffer();
    this.multisampledColor = gl.createRenderbuffer();
    this.multisampledDepth = gl.createRenderbuffer();
  }

  applyTheme({ bands, colors, params }: TerrainStyle) {
    const { gl } = this;
    gl.useProgram(this.program);
    gl.uniform3fv(this.uniform("bands"), bands.flatMap(rgb));
    for (const [name, color] of Object.entries(colors))
      gl.uniform3fv(this.uniform(name), rgb(color));
    for (const [name, value] of Object.entries(params)) gl.uniform1f(this.uniform(name), value);
  }

  upload(patch: Patch): GLPatch {
    const gl = this.gl;
    const vao = gl.createVertexArray();
    const buffers: WebGLBuffer[] = [];
    gl.bindVertexArray(vao);
    const attribute = (name: string, data: Float32Array, size: number) => {
      const location = gl.getAttribLocation(this.program, name);
      const buffer = gl.createBuffer();
      buffers.push(buffer);
      gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
      gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);
      gl.enableVertexAttribArray(location);
      gl.vertexAttribPointer(location, size, gl.FLOAT, false, 0, 0);
    };
    attribute("a_pos", patch.pos, 2);
    attribute("a_elev", patch.elev, 1);
    attribute("a_shade", patch.shade, 1);
    attribute("a_ice", patch.ice, 1);
    const { sorted, cells } = sortedByCell(patch);
    const indices = gl.createBuffer();
    buffers.push(indices);
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, indices);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, sorted, gl.STATIC_DRAW);
    gl.bindVertexArray(null);
    return { vao, buffers, cells };
  }

  free(patch: GLPatch) {
    this.gl.deleteVertexArray(patch.vao);
    for (const buffer of patch.buffers) this.gl.deleteBuffer(buffer);
  }

  /**
   * `patches` run coarse to fine; nothing renders outside `clip`, the map rectangle in CSS px.
   * `smooth` antialiases mesh edges, which shades many pixels twice, so it's kept for when the view rests.
   */
  draw(patches: readonly GLPatch[], camera: Camera, clip: Rect, smooth: boolean) {
    const { gl, canvas } = this;
    const { cx, cy, scale, width, height, dpr } = camera;
    if (smooth) {
      gl.bindFramebuffer(gl.FRAMEBUFFER, this.multisampled);
      const size = `${canvas.width}x${canvas.height}`;
      if (size !== this.multisampledSize) {
        this.multisampledSize = size;
        for (const [buffer, format, attachment] of [
          [this.multisampledColor, gl.RGBA8, gl.COLOR_ATTACHMENT0],
          [this.multisampledDepth, gl.DEPTH_COMPONENT24, gl.DEPTH_ATTACHMENT],
        ] as const) {
          gl.bindRenderbuffer(gl.RENDERBUFFER, buffer);
          gl.renderbufferStorageMultisample(
            gl.RENDERBUFFER,
            MULTISAMPLES,
            format,
            canvas.width,
            canvas.height,
          );
          gl.framebufferRenderbuffer(gl.FRAMEBUFFER, attachment, gl.RENDERBUFFER, buffer);
        }
      }
    }
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.disable(gl.SCISSOR_TEST);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    gl.enable(gl.DEPTH_TEST);

    const left = Math.round(clip.x0 * dpr);
    const top = Math.round(clip.y0 * dpr);
    const clipWidth = Math.round((clip.x1 - clip.x0) * dpr);
    const clipHeight = Math.round((clip.y1 - clip.y0) * dpr);
    gl.enable(gl.SCISSOR_TEST);
    gl.scissor(
      left,
      canvas.height - top - clipHeight,
      Math.max(0, clipWidth),
      Math.max(0, clipHeight),
    );

    const view = camera.visibleWorld();
    gl.useProgram(this.program);
    gl.uniform2f(this.uniforms.scale, (2 * scale) / width, (-2 * scale) / height);
    gl.uniform2f(this.uniforms.offset, (-2 * cx * scale) / width, (2 * cy * scale) / height);

    const pixel = 1 / (scale * dpr);
    const grainPeaks = Array.from(
      { length: GRAIN_MAX_OCTAVE },
      (_, k) => Math.log2(1 / (GRAIN_BASE_FREQUENCY * 2 ** k * pixel)) - 1,
    );
    const grainFirst = grainPeaks.findIndex((peak) => Math.abs(peak) < GRAIN_REACH);
    const grainWeights = grainPeaks
      .filter((peak) => Math.abs(peak) < GRAIN_REACH)
      .map((peak) => Math.exp(-peak * peak));
    const grainTotal = grainWeights.reduce((sum, weight) => sum + weight, 0);
    gl.uniform1i(this.uniforms.grainFirst, Math.max(grainFirst, 0));
    gl.uniform1i(this.uniforms.grainCount, grainWeights.length);
    gl.uniform1fv(
      this.uniforms.grainWeights,
      new Float32Array(GRAIN_OCTAVES).map((_, i) => (grainWeights[i] ?? 0) / grainTotal),
    );

    const coastAmps = new Float32Array(COAST_OCTAVES);
    let coastCount = 0;
    for (let k = 0; k < COAST_OCTAVES; k++) {
      const wavelengthPx = 1 / (COAST_BASE_FREQUENCY * 2 ** k * pixel);
      const t = clamp((wavelengthPx - 2) / 5, 0, 1);
      const fade = t * t * (3 - 2 * t);
      if (fade === 0) break;
      coastAmps[k] = fade * 0.5 * COAST_GAIN ** k;
      coastCount++;
    }
    const coastTails = new Float32Array(COAST_OCTAVES);
    for (let k = coastCount - 2; k >= 0; k--)
      coastTails[k] = (coastTails[k + 1] ?? 0) + 0.5 * (coastAmps[k + 1] ?? 0);
    gl.uniform1i(this.uniforms.coastCount, coastCount);
    gl.uniform1fv(this.uniforms.coastAmps, coastAmps);
    gl.uniform1fv(this.uniforms.coastTails, coastTails);

    // Finest first, nearest the viewer: depth testing then skips shading the coarser layers they cover.
    for (let i = patches.length - 1; i >= 0; i--) {
      const patch = patches[i];
      if (!patch) continue;
      gl.uniform1f(this.uniforms.depth, 1 - (i + 1) / (patches.length + 1));
      gl.bindVertexArray(patch.vao);
      let first = 0;
      let count = 0;
      for (const cell of patch.cells) {
        if (!intersects(cell.bounds, view)) continue;
        if (first + count !== cell.first) {
          if (count > 0) gl.drawElements(gl.TRIANGLES, count, gl.UNSIGNED_INT, first * 4);
          first = cell.first;
          count = 0;
        }
        count += cell.count;
      }
      if (count > 0) gl.drawElements(gl.TRIANGLES, count, gl.UNSIGNED_INT, first * 4);
    }
    gl.bindVertexArray(null);

    if (smooth) {
      gl.disable(gl.SCISSOR_TEST);
      gl.bindFramebuffer(gl.READ_FRAMEBUFFER, this.multisampled);
      gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, null);
      const { width: w, height: h } = canvas;
      gl.blitFramebuffer(0, 0, w, h, 0, 0, w, h, gl.COLOR_BUFFER_BIT, gl.NEAREST);
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    }
  }

  private uniform(name: string) {
    return this.gl.getUniformLocation(this.program, `u_${name}`);
  }
}
