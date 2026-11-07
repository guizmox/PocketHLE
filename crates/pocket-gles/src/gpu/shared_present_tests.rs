//! Feasibility test only. No new presentation path is enabled in the product.
use super::*;
use std::{sync::mpsc, time::Duration};

#[test]
#[ignore = "requires EGL desktop GL and GLES shared contexts on two threads"]
fn shared_texture_handoff_preserves_pixels_and_context_lifetime() {
    for es in [false, true] {
        let host = if es {
            Host::es_for_test().unwrap()
        } else {
            Host::new().unwrap()
        };
        let source = host.shared_source();
        let mut ui = Renderer::with_host(host, 320, 240).unwrap();
        let mut target = RenderTarget::new(320, 240);
        let vertex = "attribute vec2 position; VARYING vec2 uv; void main(){uv=vec2(position.x*0.5+0.5,0.5-position.y*0.5);gl_Position=vec4(position,0.0,1.0);}";
        let fragment = "VARYING vec2 uv; uniform sampler2D sourceImage; void main(){vec3 b=floor(texture2D(sourceImage,uv).rgb*255.0+0.5);vec3 q=floor(b/vec3(8.0,4.0,8.0));gl_FragColor=vec4((q*vec3(8.0,4.0,8.0)+floor(q/vec3(4.0,16.0,4.0)))/255.0,1.0);}";
        let mut compose = unsafe { ui.compile_program(vertex, fragment).unwrap() };
        std::thread::scope(|scope| {
            let (frames, receive) = mpsc::sync_channel(1);
            let (release, released) = mpsc::sync_channel(1);
            let worker = scope.spawn(move || {
                // The capability serializes child creation against source
                // closure; the non-Send renderer remains on its owning thread.
                let mut producer =
                    with_shared_context_source(Some(source), || Renderer::new(320, 240).unwrap());
                producer.set_surface_rgb565(true).unwrap();
                let palette = [
                    [1., 0., 0., 1.],
                    [0., 1., 0., 1.],
                    [0., 0., 1., 1.],
                    [1., 1., 0., 1.],
                ];
                for (index, color) in palette.into_iter().enumerate() {
                    let mut state = PipelineState {
                        viewport: (0, 0, 320, 240),
                        ..Default::default()
                    };
                    producer
                        .clear(320, 240, &state, color, 1., GL_COLOR_BUFFER_BIT)
                        .unwrap();
                    state.scissor = Some((0, 120, 320, 120));
                    let top = [1. - color[0], 1. - color[1], 1. - color[2], 1.];
                    producer
                        .clear(320, 240, &state, top, 1., GL_COLOR_BUFFER_BIT)
                        .unwrap();
                    producer.finish().unwrap();
                    assert!(
                        producer.readback.is_empty(),
                        "producer downloaded frame pixels"
                    );
                    frames
                        .send((
                            producer.color,
                            index,
                            color,
                            top,
                            producer.shared_context_source(),
                        ))
                        .unwrap();
                    released.recv_timeout(Duration::from_secs(10)).unwrap();
                }
                assert!(producer.readback.is_empty());
            });
            for _ in 0..4 {
                let (texture, index, bottom, top, producer_source) =
                    receive.recv_timeout(Duration::from_secs(10)).unwrap();
                if index == 2 {
                    ui.host.make_current().unwrap();
                    unsafe {
                        (ui.gl.DeleteProgram)(compose);
                    }
                    // Recreate the consumer using the still-live producer's
                    // context, not the original UI context's handle.
                    ui = Renderer::new_shared(&producer_source, 320, 240).unwrap();
                    ui.host.make_current().unwrap();
                    compose = unsafe { ui.compile_program(vertex, fragment).unwrap() };
                }
                assert_ne!(texture, ui.color);
                ui.host.make_current().unwrap();
                unsafe {
                    (ui.gl.BindFramebuffer)(0x8d40, ui.fbo);
                    (ui.gl.Viewport)(0, 0, 320, 240);
                    for cap in [GL_BLEND, GL_DEPTH_TEST, GL_CULL_FACE, GL_SCISSOR_TEST] {
                        (ui.gl.Disable)(cap);
                    }
                    (ui.gl.ColorMask)(1, 1, 1, 1);
                    (ui.gl.UseProgram)(compose);
                    (ui.gl.ActiveTexture)(GL_TEXTURE0);
                    (ui.gl.BindTexture)(GL_TEXTURE_2D, texture);
                    (ui.gl.Uniform1i)(
                        (ui.gl.GetUniformLocation)(compose, c"sourceImage".as_ptr()),
                        0,
                    );
                    (ui.gl.BindBuffer)(0x8892, ui.buffer);
                    let quad: [f32; 8] = [-1., -1., 1., -1., -1., 1., 1., 1.];
                    (ui.gl.BufferData)(0x8892, 32, quad.as_ptr().cast(), 0x88e4);
                    for a in 0..8 {
                        (ui.gl.DisableVertexAttribArray)(a);
                    }
                    (ui.gl.EnableVertexAttribArray)(0);
                    (ui.gl.VertexAttribPointer)(0, 2, GL_FLOAT, 0, 8, std::ptr::null());
                    (ui.gl.DrawArrays)(GL_TRIANGLE_STRIP, 0, 4);
                    (ui.gl.Finish)();
                    // Read only the CONSUMER result as a pixel oracle. No image
                    // buffer crosses the worker channel or leaves the producer.
                    let mut pixels = vec![0u8; 320 * 240 * 4];
                    (ui.gl.ReadPixels)(
                        0,
                        0,
                        320,
                        240,
                        GL_RGBA,
                        GL_UNSIGNED_BYTE,
                        pixels.as_mut_ptr().cast(),
                    );
                    ui.check().unwrap();
                    let expected = |c: [f32; 4]| {
                        [
                            (c[0] * 255.) as u8,
                            (c[1] * 255.) as u8,
                            (c[2] * 255.) as u8,
                            255,
                        ]
                    };
                    assert_eq!(
                        &pixels[..4],
                        &expected(top),
                        "es={es} frame={index} top/origin"
                    );
                    assert_eq!(
                        &pixels[pixels.len() - 4..],
                        &expected(bottom),
                        "es={es} frame={index} bottom"
                    );
                    (ui.gl.BindTexture)(GL_TEXTURE_2D, 0);
                }
                release.send(()).unwrap();
            }
            worker.join().unwrap();
        });
        ui.host.make_current().unwrap();
        unsafe {
            (ui.gl.UseProgram)(ui.program);
            (ui.gl.DeleteProgram)(compose);
        }
        let state = PipelineState {
            viewport: (0, 0, 320, 240),
            ..Default::default()
        };
        ui.clear(320, 240, &state, [0., 0., 1., 1.], 1., GL_COLOR_BUFFER_BIT)
            .unwrap();
        ui.read(&mut target).unwrap();
        assert_eq!(
            &target.color[..4],
            &[0, 0, 255, 255],
            "UI context failed after producer teardown"
        );
    }
}

#[test]
#[ignore = "requires shared native OpenGL"]
fn guest_texture_copy_survives_producer_surface_change() {
    let root = Renderer::new(4, 4).unwrap();
    let source = root.shared_context_source();
    with_shared_context_source(Some(source.clone()), || {
        let mut producer = Renderer::new(4, 4).unwrap();
        producer.set_surface_rgb565(true).unwrap();
        producer
            .clear(
                4,
                4,
                &PipelineState::default(),
                [1., 0., 0., 1.],
                1.,
                GL_COLOR_BUFFER_BIT,
            )
            .unwrap();
        let frame = producer
            .copy_guest_rgb_texture(1, 1, 2, 2)
            .unwrap()
            .unwrap();
        assert!(producer.readback.is_empty());
        drop(producer);

        let consumer = Renderer::new(4, 4).unwrap();
        consumer.host.make_current().unwrap();
        let mut texture = 0;
        unsafe {
            (consumer.gl.GenTextures)(1, &mut texture);
            (consumer.gl.BindTexture)(GL_TEXTURE_2D, texture);
            assert!(consumer.import_guest_rgb_texture(&frame).unwrap());
            (consumer.gl.BindFramebuffer)(0x8d40, consumer.fbo);
            (consumer.gl.FramebufferTexture2D)(0x8d40, 0x8ce0, GL_TEXTURE_2D, texture, 0);
            let mut pixels = [0u8; 16];
            (consumer.gl.ReadPixels)(
                0,
                0,
                2,
                2,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                pixels.as_mut_ptr().cast(),
            );
            assert_eq!(pixels, [255, 0, 0, 255].repeat(4).as_slice());
            (consumer.gl.FramebufferTexture2D)(0x8d40, 0x8ce0, GL_TEXTURE_2D, consumer.color, 0);
            (consumer.gl.DeleteTextures)(1, &texture);
            consumer.check().unwrap();
        }
        let mut reader = SharedReadback::new(&source).unwrap();
        assert_eq!(
            reader.read_guest_rgb_texture(&frame).unwrap(),
            [255, 0, 0, 255].repeat(4)
        );
    });
}

#[test]
#[ignore = "requires native shared OpenGL"]
fn supersampled_guest_texture_copies_and_subimages_keep_logical_pixels() {
    let root = Renderer::new(4, 4).unwrap();
    let source = root.shared_context_source();
    with_shared_context_source(Some(source.clone()), || {
        for policy in [1, 3, 4, 5] {
            let mut producer = Renderer::new(320, 240).unwrap();
            producer.set_surface_rgb565(true).unwrap();
            producer.set_render_scale(policy).unwrap();
            let state = PipelineState {
                viewport: (0, 0, 320, 240),
                ..Default::default()
            };
            producer
                .clear(320, 240, &state, [1., 0., 0., 1.], 1., GL_COLOR_BUFFER_BIT)
                .unwrap();
            let original = producer
                .copy_guest_rgb_texture(1, 1, 4, 4)
                .unwrap()
                .unwrap();
            producer
                .clear(320, 240, &state, [0., 1., 0., 1.], 1., GL_COLOR_BUFFER_BIT)
                .unwrap();
            let patch = producer
                .copy_guest_rgb_texture(0, 0, 2, 2)
                .unwrap()
                .unwrap();
            let mut texture = Texture::default();
            texture.set_level(0, 4, 4, Vec::new());
            texture.gpu_copy = Some(original.clone());
            let result = producer
                .patch_guest_rgb_texture(&texture, &patch, 1, 1)
                .unwrap()
                .unwrap();
            assert!(
                producer.readback.is_empty(),
                "copy downloaded native framebuffer"
            );
            let mut reader = SharedReadback::new(&source).unwrap();
            let pixels = reader.read_guest_rgb_texture(&result).unwrap();
            for y in 0..4 {
                for x in 0..4 {
                    let expected = if (1..3).contains(&x) && (1..3).contains(&y) {
                        [0, 255, 0, 255]
                    } else {
                        [255, 0, 0, 255]
                    };
                    let offset = (y * 4 + x) * 4;
                    assert_eq!(
                        &pixels[offset..offset + 4],
                        &expected,
                        "policy={policy} x={x} y={y}"
                    );
                }
            }
            assert_eq!(
                reader.read_guest_rgb_texture(&original).unwrap(),
                [255, 0, 0, 255].repeat(16)
            );
        }
    });
}
