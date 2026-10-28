in vec2 a_pos;
in float a_elev;
in float a_shade;
in float a_ice;
uniform vec2 u_scale;
uniform vec2 u_offset;
uniform float u_depth;
out float v_elev;
out float v_shade;
out float v_ice;
out vec2 v_world;

void main() {
  v_elev = a_elev;
  v_shade = a_shade;
  v_ice = a_ice;
  v_world = a_pos;
  gl_Position = vec4(a_pos * u_scale + u_offset, u_depth, 1.0);
}
