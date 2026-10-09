in vec2 v_pos;
out vec4 out_color;
uniform sampler2D u_frame;
uniform vec2 u_size;
uniform vec2 u_origin;
uniform vec2 u_dx;
uniform vec2 u_dy;
uniform int u_filter;
vec4 sample_pixel(vec2 p) { return texture(u_frame, (p + 0.5) / u_size); }
vec4 cubic_weights(float x) {
    return vec4(-0.5*x + x*x - 0.5*x*x*x,
                 1.0 - 2.5*x*x + 1.5*x*x*x,
                 0.5*x + 2.0*x*x - 1.5*x*x*x,
                -0.5*x*x + 0.5*x*x*x);
}
float lanczos(float x) {
    x = abs(x);
    if (x < 0.00001) return 1.0;
    if (x >= 3.0) return 0.0;
    float p = 3.141592653589793 * x;
    return sin(p) * sin(p / 3.0) / (p * p / 3.0);
}
float distance_rgb(vec3 a, vec3 b) {
    vec3 d = a - b;
    return dot(d*d, vec3(0.25,0.5,0.25));
}
void main() {
    vec2 uv = u_origin + v_pos.x*u_dx + v_pos.y*u_dy;
    vec2 p = uv*u_size - 0.5;
    vec2 base = floor(p), f = fract(p);
    vec2 cell = floor(uv*u_size);
    vec4 center = sample_pixel(cell);
    if (u_filter == 0) { out_color = center; return; }
    if (u_filter == 1) { out_color = texture(u_frame,uv); return; }
    vec4 a = sample_pixel(base), b = sample_pixel(base+vec2(1,0));
    vec4 c = sample_pixel(base+vec2(0,1)), d = sample_pixel(base+vec2(1,1));
    vec4 low = min(min(a,b),min(c,d)), high = max(max(a,b),max(c,d));
    vec4 color = vec4(0);
    if (u_filter == 4) {
        float total = 0.0;
        for (int y=-2;y<=3;y++) for (int x=-2;x<=3;x++) {
            float w = lanczos(float(x)-f.x)*lanczos(float(y)-f.y);
            color += sample_pixel(base+vec2(x,y))*w;
            total += w;
        }
        color /= total;
    } else {
        // Merge the two positive inner Catmull-Rom weights: nine
        // bilinear fetches instead of sixteen point fetches.
        vec4 wx = cubic_weights(f.x), wy = cubic_weights(f.y);
        vec3 gx = vec3(wx.x,wx.y+wx.z,wx.w);
        vec3 gy = vec3(wy.x,wy.y+wy.z,wy.w);
        vec3 px = base.x + vec3(-1.0,wx.z/gx.y,2.0);
        vec3 py = base.y + vec3(-1.0,wy.z/gy.y,2.0);
        for (int y=0;y<3;y++) for (int x=0;x<3;x++)
            color += sample_pixel(vec2(px[x],py[y])) * gx[x]*gy[y];
    }
    // Local bounds prevent ringing around text and high-contrast borders.
    color = clamp(color,low,high);
    if (u_filter == 3) {
        vec2 local = fract(uv*u_size)-0.5;
        vec2 dir = vec2(local.x < 0.0 ? -1.0 : 1.0,
                        local.y < 0.0 ? -1.0 : 1.0);
        vec4 horizontal = sample_pixel(cell+vec2(dir.x,0));
        vec4 vertical = sample_pixel(cell+vec2(0,dir.y));
        vec4 other_h = sample_pixel(cell-vec2(dir.x,0));
        vec4 other_v = sample_pixel(cell-vec2(0,dir.y));
        float similarity = 1.0-smoothstep(0.0004,0.004,
            distance_rgb(horizontal.rgb,vertical.rgb));
        float contrast = smoothstep(0.006,0.025,
            distance_rgb(center.rgb,(horizontal.rgb+vertical.rgb)*0.5));
        float unique_edge = smoothstep(0.006,0.025,
            min(distance_rgb(horizontal.rgb,other_v.rgb),
                distance_rgb(vertical.rgb,other_h.rgb)));
        float confidence = similarity*contrast*unique_edge;
        float coverage = smoothstep(0.42,0.64,abs(local.x)+abs(local.y));
        vec4 diagonal = mix(center,(horizontal+vertical)*0.5,coverage);
        color = mix(color,diagonal,confidence);
    }
    out_color = vec4(color.rgb,1.0);
}
