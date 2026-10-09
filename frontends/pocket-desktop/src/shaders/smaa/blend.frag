in vec2 v_pos;
out vec4 out_color;
uniform sampler2D u_color;
uniform sampler2D u_blend;
void main() {
    vec2 uv=vec2(v_pos.x,1.0-v_pos.y);
    vec4 offset;SMAANeighborhoodBlendingVS(uv,offset);
    out_color=vec4(SMAANeighborhoodBlendingPS(uv,offset,u_color,u_blend).rgb,1.0);
}
