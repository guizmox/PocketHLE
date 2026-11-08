//! Persistent root survives destruction/recreation of independent UI consumers.
use super::*;

#[test]
#[ignore = "requires native EGL/GL and GLES contexts"]
fn persistent_root_recreates_consumer_and_keeps_deferred_readback_valid() {
    for es in [false, true] {
        let root_host = if es {
            Host::es_for_test().unwrap()
        } else {
            Host::new().unwrap()
        };
        let root = unsafe { PersistentSharedContext::capture_current().unwrap() };
        let source = root.source();
        // Produce on a separate thread, without retaining its renderer on UI.
        let worker_source = source.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let (stop, stopped) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut producer = Renderer::new_shared(&worker_source, 8, 6).unwrap();
            producer.set_surface_rgb565(true).unwrap();
            producer
                .clear(
                    8,
                    6,
                    &PipelineState::default(),
                    [1., 0., 0., 1.],
                    1.,
                    GL_COLOR_BUFFER_BIT,
                )
                .unwrap();
            let frame = producer.snapshot_shared_frame().unwrap().unwrap();
            assert!(
                producer.readback.is_empty(),
                "export must not download pixels"
            );
            tx.send(frame).unwrap();
            stopped.recv().unwrap();
        });
        let frame = rx.recv().unwrap();
        for _ in 0..3 {
            let ui = Host::shared(&source).unwrap();
            let mut guard = unsafe { root.capture_consumer().unwrap() };
            let gl = unsafe { Gl::load(&ui).unwrap() };
            unsafe {
                frame
                    .with_texture(&mut guard, |texture| {
                        (gl.BindTexture)(GL_TEXTURE_2D, texture);
                        assert_eq!((gl.GetError)(), 0);
                    })
                    .unwrap();
            }
            // Closing this consumer must never close the independent root.
            guard.close();
            drop(guard);
            drop(ui);
            let mut read = SharedReadback::new(&source).unwrap();
            let mut pixels = vec![0; 8 * 6 * 2];
            read.read_rgb565_into(&frame, (8, 6), &mut pixels).unwrap();
            assert!(pixels.chunks_exact(2).all(|p| p == [0, 0xf8]));
        }
        drop(frame);
        stop.send(()).unwrap();
        worker.join().unwrap();
        drop(root);
        drop(root_host);
    }
}

#[test]
#[ignore = "requires native EGL GLES3"]
fn android_conversion_program_links_with_the_actual_vertex_shader() {
    let ui = Renderer::new_es_for_test(8, 6).unwrap();
    let prefix = "#version 300 es\nprecision highp float;\nprecision highp int;\n";
    let vertex = include_str!(
        "../../../../frontends/pocket-android/app/src/main/assets/shaders/reconstruction.vert"
    );
    unsafe {
        let vs = ui.shader(0x8b31, &format!("{prefix}{vertex}")).unwrap();
        let fs = ui
            .shader(0x8b30, &format!("{prefix}{SHARED_DISPLAY_FRAGMENT}"))
            .unwrap();
        let program = (ui.gl.CreateProgram)();
        (ui.gl.AttachShader)(program, vs);
        (ui.gl.AttachShader)(program, fs);
        (ui.gl.LinkProgram)(program);
        let mut linked = 0;
        (ui.gl.GetProgramiv)(program, 0x8b82, &mut linked);
        (ui.gl.DeleteShader)(vs);
        (ui.gl.DeleteShader)(fs);
        (ui.gl.DeleteProgram)(program);
        assert_eq!(linked, 1, "Android shared conversion program must link");
        assert_eq!((ui.gl.GetError)(), 0);
    }
}
