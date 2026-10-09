in vec2 v_pos;
out vec4 out_color;
uniform sampler2D u_color;
void main() {
    vec2 uv=vec2(v_pos.x,1.0-v_pos.y);
    vec4 offsets[3];SMAAEdgeDetectionVS(uv,offsets);
    out_color=vec4(SMAALumaEdgeDetectionPS(uv,offsets,u_color),0.0,0.0);
}
