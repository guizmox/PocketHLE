//! GPU box reduction to guest dimensions, with optional CPU materialization.
use super::*;

pub(super) struct LogicalResolve {
    program: u32,
    pub(super) fbo: u32,
    pub(super) texture: u32,
    buffer: u32,
    source_location: i32,
    inverse_size_location: i32,
    flip_location: i32,
    quantize_location: i32,
    scale: u32,
    width: u32,
    height: u32,
    pub(super) pixels: Vec<u8>,
}
impl LogicalResolve {
    pub(super) unsafe fn delete(self, gl: &Gl) {
        (gl.DeleteProgram)(self.program);
        (gl.DeleteFramebuffers)(1, &self.fbo);
        (gl.DeleteTextures)(1, &self.texture);
        (gl.DeleteBuffers)(1, &self.buffer);
    }
}

fn sources(scale: u32) -> (String, String) {
    let vertex = "attribute vec2 position; void main(){gl_Position=vec4(position,0.0,1.0);}";
    let mut fragment = format!(
        "#ifdef GL_ES\nprecision highp float;\n#endif\nuniform sampler2D sourceImage;\nuniform vec2 inverseSize;uniform float flipOutput,outputHeight,quantize565;\nvoid main(){{vec2 pixel=floor(gl_FragCoord.xy);if(flipOutput>0.5)pixel.y=outputHeight-1.0-pixel.y;vec2 base=pixel*{scale}.0;vec4 sum=vec4(0.0);\n"
    );
    for y in 0..scale {
        for x in 0..scale {
            fragment.push_str(&format!(
                "sum+=floor(texture2D(sourceImage,(base+vec2({x}.5,{y}.5))*inverseSize)*255.0+0.5);\n"
            ));
        }
    }
    // Integer sums have fractional quotient steps >= 1/16. A 1/64 bias
    // protects exact quotients from reciprocal-rounding error without moving
    // the next lower quotient across an integer boundary (including ES highp).
    fragment.push_str(&format!(
        "vec4 b=floor(sum/{}.0+0.015625);if(quantize565>0.5){{vec3 levels=vec3(31.0,63.0,31.0);vec3 q=floor((b.rgb*levels+127.0)/255.0);b=vec4(floor((q*255.0+floor(levels/2.0))/levels),255.0);}}gl_FragColor=b/255.0;}}",
        scale * scale
    ));
    (vertex.into(), fragment)
}

impl Renderer {
    unsafe fn create_logical_resolve(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<LogicalResolve, String> {
        let (vertex, fragment) = sources(self.render_scale);
        let program = self.compile_program(&vertex, &fragment)?;
        let mut r = LogicalResolve {
            program,
            fbo: 0,
            texture: 0,
            buffer: 0,
            source_location: (self.gl.GetUniformLocation)(program, c"sourceImage".as_ptr()),
            inverse_size_location: (self.gl.GetUniformLocation)(program, c"inverseSize".as_ptr()),
            flip_location: (self.gl.GetUniformLocation)(program, c"flipOutput".as_ptr()),
            quantize_location: (self.gl.GetUniformLocation)(program, c"quantize565".as_ptr()),
            scale: self.render_scale,
            width,
            height,
            pixels: vec![0; width as usize * height as usize * 4],
        };
        let result = (|| {
            (self.gl.GenFramebuffers)(1, &mut r.fbo);
            (self.gl.GenTextures)(1, &mut r.texture);
            (self.gl.GenBuffers)(1, &mut r.buffer);
            if r.fbo == 0 || r.texture == 0 || r.buffer == 0 {
                return Err("logical resolve allocation failed".into());
            }
            (self.gl.ActiveTexture)(GL_TEXTURE0);
            (self.gl.BindTexture)(GL_TEXTURE_2D, r.texture);
            (self.gl.TexImage2D)(
                GL_TEXTURE_2D,
                0,
                GL_RGBA as i32,
                width as i32,
                height as i32,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                std::ptr::null(),
            );
            self.params(
                Filter::Nearest,
                Filter::Nearest,
                Wrap::ClampToEdge,
                Wrap::ClampToEdge,
            );
            (self.gl.BindFramebuffer)(0x8d40, r.fbo);
            (self.gl.FramebufferTexture2D)(0x8d40, 0x8ce0, GL_TEXTURE_2D, r.texture, 0);
            if (self.gl.CheckFramebufferStatus)(0x8d40) != 0x8cd5 {
                return Err("logical resolve framebuffer incomplete".into());
            }
            let mut bits = [0; 4];
            for (index, pname) in [0x0d52, 0x0d53, 0x0d54, 0x0d55].into_iter().enumerate() {
                (self.gl.GetIntegerv)(pname, &mut bits[index]);
            }
            if bits != [8, 8, 8, 8] {
                return Err(format!("logical resolve requires RGBA8: {bits:?}"));
            }
            let quad: [f32; 8] = [-1., -1., 1., -1., -1., 1., 1., 1.];
            (self.gl.BindBuffer)(0x8892, r.buffer);
            (self.gl.BufferData)(
                0x8892,
                std::mem::size_of_val(&quad) as isize,
                quad.as_ptr().cast(),
                0x88e4,
            );
            self.check()
        })();
        if let Err(error) = result {
            r.delete(&self.gl);
            return Err(error);
        }
        Ok(r)
    }

    /// Errors in this optional pass keep the established CPU resolve usable.
    pub(super) unsafe fn resolve_logical_gpu(&mut self, width: u32, height: u32) -> bool {
        self.resolve_logical_image(width, height, true)
    }

    pub(super) unsafe fn resolve_logical_image(
        &mut self,
        width: u32,
        height: u32,
        read_pixels: bool,
    ) -> bool {
        if self.logical_resolve_disabled || !(1..=4).contains(&self.render_scale) {
            return false;
        }
        // Dithering must not perturb the integer result written to RGBA8.
        let mut dither = 0;
        (self.gl.GetIntegerv)(0x0bd0, &mut dither);
        let result =
            (|| {
                if self.logical_resolve.as_ref().is_none_or(|r| {
                    (r.width, r.height, r.scale) != (width, height, self.render_scale)
                }) {
                    if let Some(r) = self.logical_resolve.take() {
                        r.delete(&self.gl);
                    }
                    self.logical_resolve = Some(self.create_logical_resolve(width, height)?);
                }
                let r = self.logical_resolve.as_mut().unwrap();
                (self.gl.BindFramebuffer)(0x8d40, r.fbo);
                (self.gl.Viewport)(0, 0, width as i32, height as i32);
                for cap in [
                    GL_BLEND,
                    GL_DEPTH_TEST,
                    GL_CULL_FACE,
                    GL_SCISSOR_TEST,
                    GL_POLYGON_OFFSET_FILL,
                    0x0bd0,
                ] {
                    (self.gl.Disable)(cap);
                }
                (self.gl.ColorMask)(1, 1, 1, 1);
                (self.gl.UseProgram)(r.program);
                (self.gl.ActiveTexture)(GL_TEXTURE0);
                (self.gl.BindTexture)(GL_TEXTURE_2D, self.color);
                (self.gl.Uniform1i)(r.source_location, 0);
                (self.gl.Uniform1f)(r.flip_location, if read_pixels { 1. } else { 0. });
                (self.gl.Uniform1f)(r.quantize_location, if self.rgb565 { 1. } else { 0. });
                (self.gl.Uniform1f)(
                    (self.gl.GetUniformLocation)(r.program, c"outputHeight".as_ptr()),
                    height as f32,
                );
                (self.gl.Uniform2f)(
                    r.inverse_size_location,
                    1. / self.width as f32,
                    1. / self.height as f32,
                );
                (self.gl.BindBuffer)(0x8892, r.buffer);
                for index in 0..8 {
                    (self.gl.DisableVertexAttribArray)(index);
                }
                (self.gl.EnableVertexAttribArray)(0);
                (self.gl.VertexAttribPointer)(0, 2, GL_FLOAT, 0, 8, std::ptr::null());
                let error = (self.gl.GetError)();
                if error != 0 {
                    return Err(format!("logical resolve setup: 0x{error:04x}"));
                }
                (self.gl.DrawArrays)(GL_TRIANGLE_STRIP, 0, 4);
                if read_pixels {
                    (self.gl.PixelStorei)(0x0d05, 1);
                    {
                        let _diagnostic = crate::graphics_diagnostic::Span::new(
                            crate::graphics_diagnostic::Stage::ReadPixels,
                        );
                        (self.gl.ReadPixels)(
                            0,
                            0,
                            width as i32,
                            height as i32,
                            GL_RGBA,
                            GL_UNSIGNED_BYTE,
                            r.pixels.as_mut_ptr().cast(),
                        );
                    }
                }
                self.check()
            })();
        // Next guest draw must rebuild state and attribute pointers. Program
        // identities/uniform caches remain untouched by this separate program.
        (self.gl.BindFramebuffer)(0x8d40, self.fbo);
        (self.gl.Viewport)(0, 0, self.width as i32, self.height as i32);
        (self.gl.UseProgram)(self.program);
        (self.gl.BindBuffer)(0x8892, self.buffer);
        (self.gl.BindBuffer)(0x8893, self.index_buffer);
        (self.gl.ActiveTexture)(GL_TEXTURE0);
        if dither != 0 {
            (self.gl.Enable)(0x0bd0);
        } else {
            (self.gl.Disable)(0x0bd0);
        }
        self.pipeline = None;
        self.attribute_layout = None;
        self.stream_layout = None;
        match result {
            Ok(()) => true,
            Err(error) => {
                self.logical_resolve_disabled = true;
                self.uniform_values.clear();
                // An optional allocation/draw error must not poison the
                // established native readback used by the CPU fallback.
                for _ in 0..16 {
                    if (self.gl.GetError)() == 0 {
                        break;
                    }
                }
                if let Some(r) = self.logical_resolve.take() {
                    r.delete(&self.gl);
                }
                log::warn!("GPU logical reduction unavailable; retaining CPU reduction: {error}");
                false
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires native desktop GL and GLES2"]
    fn logical_reduction_matches_cpu_bytes_for_all_scales_and_formats() {
        for es in [false, true] {
            for rgb565 in [false, true] {
                for (scale, width, height) in [
                    (2, 320, 240),
                    (3, 320, 240),
                    (4, 320, 240),
                    (2, 240, 320),
                    (3, 240, 320),
                    (4, 240, 320),
                ] {
                    let mut gpu = if es {
                        Renderer::new_es_for_test(width, height).unwrap()
                    } else {
                        Renderer::new(width, height).unwrap()
                    };
                    gpu.set_surface_rgb565(rgb565).unwrap();
                    gpu.set_render_scale((scale + 1) as u8).unwrap();
                    assert_eq!(gpu.render_scale, scale);
                    let mut bytes = Vec::new();
                    let mut random = 512u32;
                    for _ in 0..gpu.width * gpu.height {
                        random ^= random << 13;
                        random ^= random >> 17;
                        random ^= random << 5;
                        if rgb565 {
                            bytes.extend_from_slice(&(random as u16).to_le_bytes());
                        } else {
                            bytes.extend_from_slice(&random.to_le_bytes());
                        }
                    }
                    unsafe {
                        gpu.host.make_current().unwrap();
                        (gpu.gl.ActiveTexture)(GL_TEXTURE0);
                        (gpu.gl.BindTexture)(GL_TEXTURE_2D, gpu.color);
                        (gpu.gl.TexImage2D)(
                            GL_TEXTURE_2D,
                            0,
                            if rgb565 {
                                if es {
                                    GL_RGB as i32
                                } else {
                                    0x8d62
                                }
                            } else {
                                GL_RGBA as i32
                            },
                            gpu.width as i32,
                            gpu.height as i32,
                            0,
                            if rgb565 { GL_RGB } else { GL_RGBA },
                            if rgb565 {
                                GL_UNSIGNED_SHORT_5_6_5
                            } else {
                                GL_UNSIGNED_BYTE
                            },
                            bytes.as_ptr().cast(),
                        );
                        gpu.check().unwrap();
                    }
                    let mut reference = RenderTarget::with_format(width, height, rgb565);
                    gpu.logical_resolve_disabled = true;
                    gpu.dirty = true;
                    gpu.read(&mut reference).unwrap();
                    let mut native = Vec::new();
                    gpu.scaled_display_rgba_into(&mut native).unwrap();
                    let mut actual = RenderTarget::with_format(width, height, rgb565);
                    gpu.logical_resolve_disabled = false;
                    gpu.dirty = true;
                    gpu.read(&mut actual).unwrap();
                    assert!(!gpu.logical_resolve_disabled, "GPU pass fell back: es={es}");
                    let differences: Vec<_> = actual
                        .color
                        .iter()
                        .zip(&reference.color)
                        .enumerate()
                        .filter(|(_, (a, b))| a != b)
                        .take(12)
                        .collect();
                    assert!(
                        differences.is_empty(),
                        "es={es} rgb565={rgb565} scale={scale}: {differences:?}"
                    );
                    let mut display = Vec::new();
                    gpu.scaled_display_rgba_into(&mut display).unwrap();
                    assert_eq!(display, native, "native display pixels changed");
                }
            }
        }
    }
    #[test]
    #[ignore = "requires native desktop GL and GLES2"]
    fn logical_reduction_preserves_following_draws_resize_and_fallback() {
        for es in [false, true] {
            let make = || {
                if es {
                    Renderer::new_es_for_test(320, 240).unwrap()
                } else {
                    Renderer::new(320, 240).unwrap()
                }
            };
            let (mut gpu, mut reference) = (make(), make());
            reference.logical_resolve_disabled = true;
            let (mut a, mut b) = (RenderTarget::new(320, 240), RenderTarget::new(320, 240));
            for (step, policy) in [3, 5, 4, 1, 5].into_iter().enumerate() {
                gpu.set_render_scale(policy).unwrap();
                reference.set_render_scale(policy).unwrap();
                let mut state = PipelineState {
                    viewport: (3, 5, 301, 223),
                    ..Default::default()
                };
                state.depth_test = true;
                state.depth_func = CompareFunc::Always;
                state.blend = true;
                state.scissor = Some((7, 9, 280, 199));
                let vertices =
                    [[-1., -1., 0., 1.], [1., -1., 0., 1.], [0., 1., 0., 1.]].map(|pos| Vertex {
                        pos,
                        color: [0.31 + step as f32 * 0.03, 0.71, 0.19, 0.63],
                        ..Default::default()
                    });
                for pass in 0..2 {
                    let mut vertices = vertices;
                    for vertex in &mut vertices {
                        vertex.color[0] += pass as f32 * 0.05;
                    }
                    for (g, target) in [(&mut gpu, &mut a), (&mut reference, &mut b)] {
                        if pass == 0 {
                            g.clear(
                                320,
                                240,
                                &state,
                                [0.1, 0.2, 0.3, 0.4],
                                1.,
                                GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
                            )
                            .unwrap();
                        }
                        g.draw(
                            target,
                            &state,
                            GL_TRIANGLES,
                            &vertices,
                            &[],
                            &HashMap::new(),
                            [0; 6],
                        )
                        .unwrap();
                        let mut rgb = vec![0; 320 * 240 * 2];
                        g.read_present(target, &mut rgb).unwrap();
                        let mut expected = vec![0; rgb.len()];
                        target.to_rgb565(&mut expected);
                        assert_eq!(rgb, expected);
                    }
                    assert_eq!(a.color, b.color, "es={es} step={step} pass={pass}");
                }
            }
            assert!(!gpu.logical_resolve_disabled);
            gpu.logical_resolve_disabled = true;
            gpu.dirty = true;
            reference.dirty = true;
            gpu.read(&mut a).unwrap();
            reference.read(&mut b).unwrap();
            assert_eq!(a.color, b.color);
            // A failed optional draw must leave native pixels and GL errors
            // usable by the established CPU path and subsequent clears.
            gpu.logical_resolve_disabled = false;
            unsafe {
                gpu.host.make_current().unwrap();
                let fbo = gpu.logical_resolve.as_ref().unwrap().fbo;
                (gpu.gl.BindFramebuffer)(0x8d40, fbo);
                (gpu.gl.FramebufferTexture2D)(0x8d40, 0x8ce0, GL_TEXTURE_2D, 0, 0);
                (gpu.gl.BindFramebuffer)(0x8d40, gpu.fbo);
            }
            gpu.dirty = true;
            reference.dirty = true;
            gpu.read(&mut a).unwrap();
            reference.read(&mut b).unwrap();
            assert!(gpu.logical_resolve_disabled);
            assert_eq!(a.color, b.color);
            unsafe {
                gpu.host.make_current().unwrap();
                gpu.check().unwrap();
            }
        }
    }
}
