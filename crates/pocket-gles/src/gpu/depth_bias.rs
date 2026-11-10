//! Host polygon-offset resolution for the virtual Intel fixed-point bias.
//!
//! GL's resolvable offset step depends on the host. Measure it once per depth
//! format, on the first nonzero Intel-biased draw, rather than letting the same
//! emulated register produce different offsets on desktop GL and GLES hosts.
use super::*;

const VERTEX: &str = "attribute vec2 position; uniform float probeZ; void main(){gl_Position=vec4(position,probeZ,1.0);}";
const FRAGMENT: &str = "uniform vec4 probeColor; void main(){gl_FragColor=probeColor;}";

pub(super) unsafe fn measure(r: &mut Renderer) -> Result<f32, String> {
    let program = r.compile_program(VERTEX, FRAGMENT)?;
    let (mut fbo, mut color, mut depth, mut buffer) = (0, 0, 0, 0);
    let mut clear_color = [0.; 4];
    let mut clear_depth = 1.;
    (r.gl.GetFloatv)(0x0c22, clear_color.as_mut_ptr());
    (r.gl.GetFloatv)(0x0b73, &mut clear_depth);
    let depth_clear = r.host.load(if r.host.es {
        c"glClearDepthf"
    } else {
        c"glClearDepth"
    });
    let set_depth = |value: f32| {
        if !depth_clear.is_null() {
            if r.host.es {
                let f: unsafe extern "system" fn(f32) = std::mem::transmute(depth_clear);
                f(value);
            } else {
                let f: unsafe extern "system" fn(f64) = std::mem::transmute(depth_clear);
                f(value as f64);
            }
        }
    };
    (r.gl.GenFramebuffers)(1, &mut fbo);
    (r.gl.GenTextures)(1, &mut color);
    (r.gl.GenRenderbuffers)(1, &mut depth);
    (r.gl.GenBuffers)(1, &mut buffer);
    let result = (|| {
        if depth_clear.is_null() {
            return Err("driver lacks depth clear for polygon-offset setup".into());
        }
        (r.gl.ActiveTexture)(GL_TEXTURE0);
        (r.gl.BindTexture)(GL_TEXTURE_2D, color);
        (r.gl.TexImage2D)(
            GL_TEXTURE_2D,
            0,
            GL_RGBA as i32,
            2,
            2,
            0,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            std::ptr::null(),
        );
        (r.gl.TexParameteri)(GL_TEXTURE_2D, 0x2801, 0x2600);
        (r.gl.TexParameteri)(GL_TEXTURE_2D, 0x2800, 0x2600);
        (r.gl.BindRenderbuffer)(0x8d41, depth);
        (r.gl.RenderbufferStorage)(0x8d41, r.depth_format, 2, 2);
        (r.gl.BindFramebuffer)(0x8d40, fbo);
        (r.gl.FramebufferTexture2D)(0x8d40, 0x8ce0, GL_TEXTURE_2D, color, 0);
        (r.gl.FramebufferRenderbuffer)(0x8d40, 0x8d00, 0x8d41, depth);
        if (r.gl.CheckFramebufferStatus)(0x8d40) != 0x8cd5 {
            return Err("polygon-offset setup framebuffer is incomplete".into());
        }
        (r.gl.UseProgram)(program);
        let z = (r.gl.GetUniformLocation)(program, c"probeZ".as_ptr());
        let c = (r.gl.GetUniformLocation)(program, c"probeColor".as_ptr());
        let vertices: [f32; 6] = [-1., -1., 3., -1., -1., 3.];
        (r.gl.BindBuffer)(GL_ARRAY_BUFFER, buffer);
        (r.gl.BufferData)(
            GL_ARRAY_BUFFER,
            std::mem::size_of_val(&vertices) as isize,
            vertices.as_ptr().cast(),
            0x88e4,
        );
        (r.gl.EnableVertexAttribArray)(0);
        (r.gl.VertexAttribPointer)(0, 2, GL_FLOAT, 0, 0, std::ptr::null());
        (r.gl.Viewport)(0, 0, 2, 2);
        (r.gl.Disable)(GL_BLEND);
        (r.gl.Disable)(GL_CULL_FACE);
        (r.gl.Disable)(GL_SCISSOR_TEST);
        (r.gl.Enable)(GL_DEPTH_TEST);
        (r.gl.DepthFunc)(GL_LESS);
        (r.gl.DepthMask)(1);
        (r.gl.ColorMask)(1, 1, 1, 1);
        (r.gl.ClearColor)(0., 0., 0., 1.);
        set_depth(1.);
        let visible = |units: f32| {
            (r.gl.Disable)(GL_POLYGON_OFFSET_FILL);
            (r.gl.Clear)(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);
            (r.gl.Uniform1f)(z, 0.);
            (r.gl.Uniform4fv)(c, 1, [0., 1., 0., 1.].as_ptr());
            (r.gl.DrawArrays)(GL_TRIANGLES, 0, 3);
            (r.gl.Enable)(GL_POLYGON_OFFSET_FILL);
            (r.gl.PolygonOffset)(0., -units);
            (r.gl.Uniform1f)(z, 1. / 512.);
            (r.gl.Uniform4fv)(c, 1, [1., 0., 0., 1.].as_ptr());
            (r.gl.DrawArrays)(GL_TRIANGLES, 0, 3);
            let mut pixel = [0u8; 4];
            (r.gl.ReadPixels)(
                0,
                0,
                1,
                1,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                pixel.as_mut_ptr().cast(),
            );
            pixel[0] > pixel[1]
        };
        if !visible(0.75 * 2f32.powi(24)) || visible(0.) {
            return Err("host polygon offset failed its setup control".into());
        }
        // Search powers of two rather than individual units. The 3/4 factor
        // keeps each comparison away from equal depths and storage rounding;
        // a 16- or 24-bit fixed attachment needs at most five search readbacks.
        let (mut low, mut high) = (0, 24);
        while low < high {
            let mid = (low + high) / 2;
            if visible(0.75 * 2f32.powi(mid)) {
                high = mid;
            } else {
                low = mid + 1;
            }
        }
        r.check()?;
        // The first passing exponent n satisfies (3/4)*2^n*r > 2^-10.
        Ok(2f32.powi(-9 - high))
    })();
    set_depth(clear_depth);
    (r.gl.ClearColor)(
        clear_color[0],
        clear_color[1],
        clear_color[2],
        clear_color[3],
    );
    (r.gl.Disable)(GL_POLYGON_OFFSET_FILL);
    (r.gl.DisableVertexAttribArray)(0);
    (r.gl.BindBuffer)(GL_ARRAY_BUFFER, 0);
    (r.gl.BindFramebuffer)(0x8d40, r.fbo);
    (r.gl.BindRenderbuffer)(0x8d41, r.depth);
    (r.gl.UseProgram)(r.program);
    (r.gl.DeleteBuffers)(1, &buffer);
    (r.gl.DeleteFramebuffers)(1, &fbo);
    (r.gl.DeleteRenderbuffers)(1, &depth);
    (r.gl.DeleteTextures)(1, &color);
    (r.gl.DeleteProgram)(program);
    r.pipeline = None;
    r.attribute_layout = None;
    r.stream_layout = None;
    result
}
