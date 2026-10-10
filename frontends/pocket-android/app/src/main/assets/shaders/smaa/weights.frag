in vec2 v_pos;
out vec4 out_color;
uniform sampler2D u_edges;
uniform sampler2D u_area;
uniform sampler2D u_search;
void main() {
    vec2 uv=vec2(v_pos.x,1.0-v_pos.y);
    vec4 offsets[3];vec2 pixel;
    SMAABlendingWeightCalculationVS(uv,pixel,offsets);
    out_color=SMAABlendingWeightCalculationPS(uv,pixel,offsets,u_edges,u_area,u_search,vec4(0.0));
}
