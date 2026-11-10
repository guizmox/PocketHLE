//! Validate host 16.16 attribute fetch once, before accepting native streams.
//!
//! A fixed value of 65536 must become exactly 1.0. In particular, approximation
//! above 1.0 clips full-screen backgrounds placed on the far plane (Vase and
//! Trilinear in the PowerVR SDK). Test fetch independently of scene geometry;
//! failed/unsupported hosts retain the existing CPU-to-float stream conversion.
use super::*;

const VALUES: [[i32; 4]; 8] = [
    [0, 65536, -65536, 32768],
    [65536, -65536, 32768, -32768],
    [-65536, 32768, -32768, 1],
    [32768, -32768, 1, -1],
    [65535, 65537, -65535, -65537],
    [1, -1, 1234567, -1234567],
    [i32::MAX, i32::MIN, 0, 65536],
    [1234567, -1234567, i32::MAX, i32::MIN],
];
const VERTEX: &str = "attribute vec2 position; attribute vec4 color,tex0; uniform float probeShift; VARYING vec4 probeColor; void main(){ gl_Position=vec4(position,0.0,1.0); gl_PointSize=1.0; probeColor=all(equal(color,tex0+vec4(probeShift,0.0,0.0,0.0)))?vec4(0.0,1.0,0.0,1.0):vec4(1.0,0.0,0.0,1.0); }";
const FRAGMENT: &str = "VARYING vec4 probeColor; void main(){gl_FragColor=probeColor;}";

pub(super) unsafe fn validate(r: &mut Renderer) -> Result<bool, String> {
    measure(r, GL_FIXED)
}

unsafe fn measure(r: &mut Renderer, input_type: u32) -> Result<bool, String> {
    let program = r.compile_program(VERTEX, FRAGMENT)?;
    let (mut fbo, mut color) = (0, 0);
    let mut buffers = [0; 3];
    let mut clear_color = [0.; 4];
    (r.gl.GetFloatv)(0x0c22, clear_color.as_mut_ptr());
    (r.gl.GenFramebuffers)(1, &mut fbo);
    (r.gl.GenTextures)(1, &mut color);
    (r.gl.GenBuffers)(3, buffers.as_mut_ptr());
    let result = (|| {
        if program == 0 || fbo == 0 || color == 0 || buffers.contains(&0) {
            return Err("fixed-attribute validation allocation failed".into());
        }
        (r.gl.ActiveTexture)(GL_TEXTURE0);
        (r.gl.BindTexture)(GL_TEXTURE_2D, color);
        (r.gl.TexImage2D)(
            GL_TEXTURE_2D,
            0,
            GL_RGBA as i32,
            8,
            3,
            0,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            std::ptr::null(),
        );
        (r.gl.TexParameteri)(GL_TEXTURE_2D, 0x2801, GL_NEAREST as i32);
        (r.gl.TexParameteri)(GL_TEXTURE_2D, 0x2800, GL_NEAREST as i32);
        (r.gl.BindFramebuffer)(0x8d40, fbo);
        (r.gl.FramebufferTexture2D)(0x8d40, 0x8ce0, GL_TEXTURE_2D, color, 0);
        if (r.gl.CheckFramebufferStatus)(0x8d40) != 0x8cd5 {
            return Err("fixed-attribute validation framebuffer is incomplete".into());
        }
        let mut positions = Vec::<f32>::new();
        let mut fixed = Vec::<i32>::new();
        let mut floats = Vec::<f32>::new();
        for row in 0..3 {
            for (column, values) in VALUES.iter().enumerate() {
                positions.extend([
                    (column as f32 + 0.5) / 4. - 1.,
                    (row as f32 + 0.5) * (2. / 3.) - 1.,
                ]);
                fixed.extend(values);
                floats.extend(values.map(crate::fixed::to_f32));
            }
        }
        for (buffer, ptr, bytes) in [
            (
                buffers[0],
                positions.as_ptr().cast::<c_void>(),
                positions.len() * 4,
            ),
            (buffers[1], fixed.as_ptr().cast(), fixed.len() * 4),
            (buffers[2], floats.as_ptr().cast(), floats.len() * 4),
        ] {
            (r.gl.BindBuffer)(GL_ARRAY_BUFFER, buffer);
            (r.gl.BufferData)(GL_ARRAY_BUFFER, bytes as isize, ptr, 0x88e4);
        }
        (r.gl.UseProgram)(program);
        let shift = (r.gl.GetUniformLocation)(program, c"probeShift".as_ptr());
        (r.gl.Viewport)(0, 0, 8, 3);
        for cap in [GL_BLEND, GL_CULL_FACE, GL_SCISSOR_TEST, GL_DEPTH_TEST] {
            (r.gl.Disable)(cap);
        }
        (r.gl.ColorMask)(1, 1, 1, 1);
        (r.gl.ClearColor)(0., 0., 0., 1.);
        for index in 0..3 {
            (r.gl.EnableVertexAttribArray)(index);
        }
        let mut pixels = [0u8; 8 * 3 * 4];
        // Float equality and intentional inequality are controls. Only the
        // final pass tests fixed fetch; no readbacks occur during game draws.
        for pass in 0..3 {
            (r.gl.Clear)(GL_COLOR_BUFFER_BIT);
            (r.gl.Uniform1f)(shift, if pass == 1 { 1. } else { 0. });
            for row in 0..3 {
                (r.gl.BindBuffer)(GL_ARRAY_BUFFER, buffers[0]);
                (r.gl.VertexAttribPointer)(0, 2, GL_FLOAT, 0, 8, (row * 8 * 8) as *const c_void);
                (r.gl.BindBuffer)(
                    GL_ARRAY_BUFFER,
                    if pass == 2 && input_type != GL_FLOAT {
                        buffers[1]
                    } else {
                        buffers[2]
                    },
                );
                (r.gl.VertexAttribPointer)(
                    1,
                    row + 2,
                    if pass == 2 { input_type } else { GL_FLOAT },
                    0,
                    16,
                    (row * 8 * 16) as *const c_void,
                );
                (r.gl.BindBuffer)(GL_ARRAY_BUFFER, buffers[2]);
                (r.gl.VertexAttribPointer)(
                    2,
                    row + 2,
                    GL_FLOAT,
                    0,
                    16,
                    (row * 8 * 16) as *const c_void,
                );
                (r.gl.DrawArrays)(GL_POINTS, 0, 8);
            }
            (r.gl.ReadPixels)(
                0,
                0,
                8,
                3,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                pixels.as_mut_ptr().cast(),
            );
            r.check()?;
            let matched = pixels.chunks_exact(4).all(|p| {
                p == if pass == 1 {
                    &[255, 0, 0, 255]
                } else {
                    &[0, 255, 0, 255]
                }
            });
            if !matched {
                return if pass == 2 {
                    Ok(false)
                } else {
                    Err("fixed-attribute validation control failed".into())
                };
            }
        }
        Ok(true)
    })();
    (r.gl.ClearColor)(
        clear_color[0],
        clear_color[1],
        clear_color[2],
        clear_color[3],
    );
    for index in 0..3 {
        (r.gl.DisableVertexAttribArray)(index);
    }
    (r.gl.BindBuffer)(GL_ARRAY_BUFFER, r.buffer);
    (r.gl.BindFramebuffer)(0x8d40, r.fbo);
    (r.gl.BindTexture)(GL_TEXTURE_2D, r.white);
    (r.gl.UseProgram)(r.program);
    (r.gl.DeleteBuffers)(3, buffers.as_ptr());
    (r.gl.DeleteFramebuffers)(1, &fbo);
    (r.gl.DeleteTextures)(1, &color);
    (r.gl.DeleteProgram)(program);
    r.pipeline = None;
    r.attribute_layout = None;
    r.stream_layout = None;
    // Drain probe errors so a failed optional capability cannot poison draws.
    let cleanup = upload_error(&r.gl);
    match (result, cleanup) {
        (Err(error), _) => Err(error),
        (_, Err(error)) => Err(error),
        (_, Ok(error)) if error != 0 => Err(format!(
            "fixed-attribute validation cleanup error 0x{error:04x}"
        )),
        (result, _) => result,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires native GL/GLES; fixed fetch validation and controls"]
    fn validation_accepts_exact_fetch_and_rejects_wrong_component_decoding() {
        for es in [false, true] {
            let mut r = if es {
                Renderer::new_es_for_test(8, 8).unwrap()
            } else {
                Renderer::new(8, 8).unwrap()
            };
            unsafe {
                assert!(measure(&mut r, GL_FLOAT).unwrap());
                assert!(!measure(&mut r, GL_SHORT).unwrap());
                let expected = validate(&mut r).unwrap();
                assert_eq!(r.supports_fixed_vertices(), expected);
                r.check().unwrap();
            }
        }
    }
}
