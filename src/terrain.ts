import type { Patch } from "./api";
import type { Camera } from "./camera";
import type { Rect } from "./geometry";
import { BAND_EDGES, RECENCY_BANDS, rgb } from "./recency";
import fragmentSource from "./terrain.frag.glsl?raw";
import vertexSource from "./terrain.vert.glsl?raw";

export interface GLPatch {
  vao: WebGLVertexArrayObject;
  buffers: WebGLBuffer[];
  indexCount: number;
}

/** WebGL2 hypsometric terrain, one mesh per loaded patch. */
export class Terrain {
  private readonly gl: WebGL2RenderingContext;
  private readonly program: WebGLProgram;
  private readonly uniforms: Record<
    "scale" | "offset" | "px" | "depth",
    WebGLUniformLocation | null
  >;

  constructor(private readonly canvas: HTMLCanvasElement) {
    const gl = canvas.getContext("webgl2", { antialias: true, preserveDrawingBuffer: true });
    if (!gl) throw new Error("WebGL2 unavailable");
    this.gl = gl;

    const header = `#version 300 es\n#define BAND_COUNT ${RECENCY_BANDS.length}\n`;
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
      scale: gl.getUniformLocation(program, "u_scale"),
      offset: gl.getUniformLocation(program, "u_offset"),
      px: gl.getUniformLocation(program, "u_px"),
      depth: gl.getUniformLocation(program, "u_depth"),
    };
    gl.useProgram(program);
    gl.uniform3fv(
      gl.getUniformLocation(program, "u_bands"),
      RECENCY_BANDS.flatMap(({ color }) => rgb(color)),
    );
    gl.uniform1fv(gl.getUniformLocation(program, "u_edges"), BAND_EDGES);
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
    const indices = gl.createBuffer();
    buffers.push(indices);
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, indices);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, patch.tris, gl.STATIC_DRAW);
    gl.bindVertexArray(null);
    return { vao, buffers, indexCount: patch.tris.length };
  }

  free(patch: GLPatch) {
    this.gl.deleteVertexArray(patch.vao);
    for (const buffer of patch.buffers) this.gl.deleteBuffer(buffer);
  }

  /** `patches` run coarse to fine; nothing renders outside `clip`, the map rectangle in CSS px. */
  draw(patches: readonly GLPatch[], camera: Camera, clip: Rect) {
    const { gl, canvas } = this;
    const { cx, cy, scale, width, height, dpr } = camera;
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

    gl.useProgram(this.program);
    gl.uniform2f(this.uniforms.scale, (2 * scale) / width, (-2 * scale) / height);
    gl.uniform2f(this.uniforms.offset, (-2 * cx * scale) / width, (2 * cy * scale) / height);
    gl.uniform1f(this.uniforms.px, 1 / (scale * dpr));
    // Finest first, nearest the viewer: depth testing then skips shading the coarser layers they cover.
    for (let i = patches.length - 1; i >= 0; i--) {
      const patch = patches[i];
      if (!patch) continue;
      gl.uniform1f(this.uniforms.depth, 1 - (i + 1) / (patches.length + 1));
      gl.bindVertexArray(patch.vao);
      gl.drawElements(gl.TRIANGLES, patch.indexCount, gl.UNSIGNED_INT, 0);
    }
    gl.bindVertexArray(null);
  }
}
