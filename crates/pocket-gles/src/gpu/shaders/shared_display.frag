// Convert native GL rows to display pixels, retaining RGBA8 source precision.
// Apply RGB565 expansion only for an actual RGB565 source, before filters.
#ifdef GL_ES
precision highp float;
#endif
uniform sampler2D u_source;
uniform vec2 u_size;
uniform int u_backlight;
uniform int u_rgb565;
out vec4 out_color;
void main() {
    vec2 uv = vec2(gl_FragCoord.x / u_size.x, 1.0 - gl_FragCoord.y / u_size.y);
    vec3 bytes = floor(texture(u_source, uv).rgb * 255.0 + 0.5);
    if (u_rgb565 != 0) {
        vec3 bits = floor(bytes / vec3(8.0, 4.0, 8.0));
        bytes = bits * vec3(8.0, 4.0, 8.0) + floor(bits / vec3(4.0, 16.0, 4.0));
    }
    float level = (u_backlight >= 1 && u_backlight <= 31) ? float(u_backlight) : 11.0;
    // Integer-equivalent division. Bias is below the smallest nonzero step
    // (1/240) and protects exact quotients from floating reciprocal rounding.
    if (level <= 11.0) bytes -= floor(bytes * (11.0 - level) / 50.0 + 0.0001);
    else bytes += floor((255.0 - bytes) * (level - 11.0) / 240.0 + 0.0001);
    out_color = vec4(bytes / 255.0, 1.0);
}
