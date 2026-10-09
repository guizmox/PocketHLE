out vec2 v_pos;
void main() {
    vec2 p = vec2((gl_VertexID == 1) ? 3.0 : -1.0,
                  (gl_VertexID == 2) ? 3.0 : -1.0);
    gl_Position = vec4(p, 0.0, 1.0);
    v_pos = vec2((p.x + 1.0) * 0.5, (1.0 - p.y) * 0.5);
}
