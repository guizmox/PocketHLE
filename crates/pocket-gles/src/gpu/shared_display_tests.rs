//! Pixel oracle for the exact fragment body intended for desktop presentation.
use super::*;

#[test]
#[ignore = "requires native GL/GLES shared textures"]
fn shared_display_shader_matches_rgb565_backlight_and_row_order() {
    let fragment = SHARED_DISPLAY_FRAGMENT
        .replace("out vec4 out_color;", "")
        .replace("out_color", "gl_FragColor")
        .replace("texture(", "texture2D(");
    for es in [false, true] {
        for (width, height) in [(8, 6), (6, 8), (320, 240), (480, 640)] {
            let mut ui = if es {
                Renderer::new_es_for_test(width, height).unwrap()
            } else {
                Renderer::new(width, height).unwrap()
            };
            let mut external = unsafe { ExternalGpuContext::capture_current().unwrap() };
            let consumer = external.consumer();
            let mut consumer = std::thread::spawn(move || consumer).join().unwrap();
            let mut producer = Renderer::new_shared(&external.source(), width, height).unwrap();
            let data: Vec<u8> = (0..width * height * 4)
                .map(|i| (i.wrapping_mul(73).wrapping_add(i / 13).wrapping_add(17)) as u8)
                .collect();
            unsafe {
                (producer.gl.BindTexture)(GL_TEXTURE_2D, producer.color);
                (producer.gl.TexImage2D)(
                    GL_TEXTURE_2D,
                    0,
                    GL_RGBA as i32,
                    width as i32,
                    height as i32,
                    0,
                    GL_RGBA,
                    GL_UNSIGNED_BYTE,
                    data.as_ptr().cast(),
                );
            }
            let frame = producer.snapshot_shared_frame().unwrap().unwrap();
            assert!(producer.readback.is_empty());
            ui.host.make_current().unwrap();
            unsafe {
                let prefix = if es {
                    "#version 300 es\nprecision highp float;\nprecision highp int;\n"
                } else {
                    "#version 140\n"
                };
                let shader = ui
                    .shader(0x8b30, &format!("{prefix}{SHARED_DISPLAY_FRAGMENT}"))
                    .unwrap();
                (ui.gl.DeleteShader)(shader);
            }
            let program = unsafe {
                ui.compile_program(
                    "attribute vec2 position; void main(){gl_Position=vec4(position,0.0,1.0);}",
                    &fragment,
                )
                .unwrap()
            };
            for level in [0u32, 1, 11, 21, 31, 99] {
                unsafe {
                    frame
                        .with_consumer(&mut consumer, |texture| {
                            (ui.gl.BindFramebuffer)(0x8d40, ui.fbo);
                            (ui.gl.Viewport)(0, 0, width as i32, height as i32);
                            for cap in [
                                GL_BLEND,
                                GL_DEPTH_TEST,
                                GL_CULL_FACE,
                                GL_SCISSOR_TEST,
                                0x0bd0,
                            ] {
                                (ui.gl.Disable)(cap);
                            }
                            (ui.gl.ColorMask)(1, 1, 1, 1);
                            (ui.gl.UseProgram)(program);
                            (ui.gl.ActiveTexture)(GL_TEXTURE0);
                            (ui.gl.BindTexture)(GL_TEXTURE_2D, texture);
                            (ui.gl.Uniform1i)(
                                (ui.gl.GetUniformLocation)(program, c"u_source".as_ptr()),
                                0,
                            );
                            (ui.gl.Uniform1i)(
                                (ui.gl.GetUniformLocation)(program, c"u_backlight".as_ptr()),
                                level as i32,
                            );
                            (ui.gl.Uniform2f)(
                                (ui.gl.GetUniformLocation)(program, c"u_size".as_ptr()),
                                width as f32,
                                height as f32,
                            );
                            let quad: [f32; 8] = [-1., -1., 1., -1., -1., 1., 1., 1.];
                            (ui.gl.BindBuffer)(0x8892, ui.buffer);
                            (ui.gl.BufferData)(0x8892, 32, quad.as_ptr().cast(), 0x88e4);
                            for i in 0..8 {
                                (ui.gl.DisableVertexAttribArray)(i);
                            }
                            (ui.gl.EnableVertexAttribArray)(0);
                            (ui.gl.VertexAttribPointer)(0, 2, GL_FLOAT, 0, 8, std::ptr::null());
                            (ui.gl.DrawArrays)(GL_TRIANGLE_STRIP, 0, 4);
                            (ui.gl.BindTexture)(GL_TEXTURE_2D, 0);
                        })
                        .unwrap();
                    let mut output = vec![0; data.len()];
                    (ui.gl.ReadPixels)(
                        0,
                        0,
                        width as i32,
                        height as i32,
                        GL_RGBA,
                        GL_UNSIGNED_BYTE,
                        output.as_mut_ptr().cast(),
                    );
                    ui.check().unwrap();
                    let valid = if (1..=31).contains(&level) { level } else { 11 };
                    let stride = width as usize * 4;
                    let mut expected = Vec::with_capacity(data.len());
                    for row in data.chunks_exact(stride).rev() {
                        for src in row.chunks_exact(4) {
                            for (i, shift) in [3u8, 2, 3].into_iter().enumerate() {
                                let bits = src[i] >> shift;
                                let c = ((bits << shift) | (bits >> (8 - shift * 2))) as u32;
                                let c = if valid <= 11 {
                                    c - c * (11 - valid) / 50
                                } else {
                                    c + (255 - c) * (valid - 11) / 240
                                };
                                expected.push(c as u8);
                            }
                            expected.push(255);
                        }
                    }
                    assert_eq!(output, expected, "es={es} {width}x{height} level={level}");
                }
            }
            drop(frame);
            drop(producer);
            ui.host.make_current().unwrap();
            external.collect_retired_frames().unwrap();
            unsafe {
                (ui.gl.DeleteProgram)(program);
            }
        }
    }
}
