// Packed logical rows are already top-down. Keep that texture row order.
uniform highp usampler2D u_source;
uniform int u_backlight;
out vec4 out_color;
void main() {
    uint p = texelFetch(u_source, ivec2(gl_FragCoord.xy), 0).r;
    uvec3 bits = uvec3((p >> 11u) & 31u, (p >> 5u) & 63u, p & 31u);
    uvec3 bytes = (bits << uvec3(3u,2u,3u)) | (bits >> uvec3(2u,4u,2u));
    uint level = (u_backlight >= 1 && u_backlight <= 31) ? uint(u_backlight) : 11u;
    if (level <= 11u) bytes -= bytes * (11u-level) / 50u;
    else bytes += (uvec3(255u)-bytes) * (level-11u) / 240u;
    out_color = vec4(vec3(bytes) / 255.0, 1.0);
}
