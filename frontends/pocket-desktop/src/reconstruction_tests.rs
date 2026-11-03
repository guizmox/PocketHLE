use super as reconstruction;
use crate::runner;
use eframe::glow;
use glow::HasContext;
use std::{
    ffi::{c_char, c_void, CStr},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
type P = *mut c_void;
#[link(name = "dl")]
extern "C" {
    fn dlopen(n: *const c_char, f: i32) -> P;
    fn dlsym(p: P, n: *const c_char) -> P;
    fn dlclose(p: P) -> i32;
}
struct Ui {
    lib: P,
    display: P,
    context: P,
    surface: P,
    get: unsafe extern "system" fn(*const c_char) -> P,
    make: unsafe extern "system" fn(P, P, P, P) -> u32,
    dc: unsafe extern "system" fn(P, P) -> u32,
    ds: unsafe extern "system" fn(P, P) -> u32,
    term: unsafe extern "system" fn(P) -> u32,
}
impl Ui {
    unsafe fn new(es: bool) -> Self {
        let lib = dlopen(c"libEGL.so.1".as_ptr(), 2);
        assert!(!lib.is_null());
        macro_rules! sym {
            ($n:literal,$t:ty) => {{
                let p = dlsym(
                    lib,
                    CStr::from_bytes_with_nul(concat!($n, "\0").as_bytes())
                        .unwrap()
                        .as_ptr(),
                );
                assert!(!p.is_null());
                std::mem::transmute::<P, $t>(p)
            }};
        }
        let get = sym!(
            "eglGetProcAddress",
            unsafe extern "system" fn(*const c_char) -> P
        );
        let platform: unsafe extern "system" fn(u32, P, *const i32) -> P =
            std::mem::transmute(get(c"eglGetPlatformDisplayEXT".as_ptr()));
        let display = platform(0x31dd, std::ptr::null_mut(), std::ptr::null());
        assert!(!display.is_null());
        let init = sym!(
            "eglInitialize",
            unsafe extern "system" fn(P, *mut i32, *mut i32) -> u32
        );
        assert_ne!(init(display, std::ptr::null_mut(), std::ptr::null_mut()), 0);
        let bind = sym!("eglBindAPI", unsafe extern "system" fn(u32) -> u32);
        assert_ne!(bind(if es { 0x30a0 } else { 0x30a2 }), 0);
        let choose = sym!(
            "eglChooseConfig",
            unsafe extern "system" fn(P, *const i32, *mut P, i32, *mut i32) -> u32
        );
        let mut config = std::ptr::null_mut();
        let mut count = 0;
        assert_ne!(
            choose(
                display,
                [
                    0x3033,
                    1,
                    0x3040,
                    if es { 4 } else { 8 },
                    0x3024,
                    8,
                    0x3023,
                    8,
                    0x3022,
                    8,
                    0x3038
                ]
                .as_ptr(),
                &mut config,
                1,
                &mut count
            ),
            0
        );
        assert_ne!(count, 0);
        let cs = sym!(
            "eglCreatePbufferSurface",
            unsafe extern "system" fn(P, P, *const i32) -> P
        );
        let surface = cs(display, config, [0x3057, 1, 0x3056, 1, 0x3038].as_ptr());
        assert!(!surface.is_null());
        let cc = sym!(
            "eglCreateContext",
            unsafe extern "system" fn(P, P, P, *const i32) -> P
        );
        let ca = if es {
            vec![0x3098, 3, 0x3038]
        } else {
            // Eframe's desktop context uses modern core GL. The producer keeps
            // its independent compatibility context inside the same share group.
            vec![0x3098, 3, 0x30fb, 3, 0x30fd, 1, 0x3038]
        };
        let context = cc(display, config, std::ptr::null_mut(), ca.as_ptr());
        assert!(!context.is_null());
        let make = sym!(
            "eglMakeCurrent",
            unsafe extern "system" fn(P, P, P, P) -> u32
        );
        assert_ne!(make(display, surface, surface, context), 0);
        Self {
            lib,
            display,
            context,
            surface,
            get,
            make,
            dc: sym!("eglDestroyContext", unsafe extern "system" fn(P, P) -> u32),
            ds: sym!("eglDestroySurface", unsafe extern "system" fn(P, P) -> u32),
            term: sym!("eglTerminate", unsafe extern "system" fn(P) -> u32),
        }
    }
}
impl Drop for Ui {
    fn drop(&mut self) {
        unsafe {
            (self.make)(
                self.display,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            (self.dc)(self.display, self.context);
            (self.ds)(self.display, self.surface);
            (self.term)(self.display);
            dlclose(self.lib);
        }
    }
}
fn frame(
    source: pocket_gles::gpu::SharedContextSource,
    w: u32,
    h: u32,
    policy: u8,
    level: u32,
) -> runner::FrameSnapshot {
    std::thread::spawn(move || {
        pocket_gles::gpu::with_shared_context_source(Some(source.clone()), || {
            let mut c = pocket_gles::context::Context::new_rgb565(w, h);
            c.set_internal_resolution(policy);
            c.enable_gpu().unwrap();
            c.clear_color = [0.13, 0.54, 0.81, 0.25];
            c.clear(pocket_gles::GL_COLOR_BUFFER_BIT);
            for (rect, color) in [
                ((0, 0, w as i32 / 2, h as i32 / 2), [0.97, 0.12, 0.37, 0.]),
                (
                    (w as i32 / 2, h as i32 / 2, w as i32 / 2, h as i32 / 2),
                    [0.19, 0.94, 0.21, 1.],
                ),
                ((w as i32 / 3, 0, 1, h as i32), [1., 1., 1., 0.]),
            ] {
                c.state.scissor = Some(rect);
                c.clear_color = color;
                c.clear(pocket_gles::GL_COLOR_BUFFER_BIT);
            }
            let mut fb = pocket_core::kernel::Framebuffer::new(w, h);
            c.present_rgb565(&mut fb.pixels);
            fb.mark_dirty();
            let mut high = Vec::new();
            if let Some((pw, ph)) = c.scaled_display_rgba_into(&mut high) {
                fb.set_render_presentation_rgba(pw, ph, high);
            }
            fb.set_backlight_level(level);
            let mut rgba = Vec::new();
            fb.snapshot_presentation_rgba8888_into(&mut rgba);
            let shared = c.snapshot_shared_frame();
            assert!(shared.is_some());
            let provider = Arc::new(
                pocket_core::winceapi::gles::deferred_frame::GlesCpuPixelSource::new(
                    shared.as_ref().unwrap().clone(),
                    source.clone(),
                    (w, h),
                )
                .unwrap(),
            );
            let mut checked =
                pocket_core::kernel::framebuffer::deferred::DeferredFramebuffer::new(fb).unwrap();
            checked.publish_gpu(provider).unwrap();
            let mut snapshot =
                runner::FrameSnapshot::from_shared(&checked, shared.unwrap()).unwrap();
            assert!(snapshot.rgba.is_empty());
            assert!(!checked.cpu_resident());
            assert!(snapshot.deferred.as_ref().unwrap().cached_rgba().is_none());
            // Keep an independently computed CPU reference for pixel comparisons.
            snapshot.rgba = rgba;
            snapshot
        })
    })
    .join()
    .unwrap()
}
unsafe fn output(gl: &glow::Context, w: i32, h: i32) -> (glow::Texture, glow::Framebuffer) {
    let t = gl.create_texture().unwrap();
    gl.bind_texture(glow::TEXTURE_2D, Some(t));
    gl.tex_image_2d(
        glow::TEXTURE_2D,
        0,
        glow::RGBA8 as i32,
        w,
        h,
        0,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        None,
    );
    let f = gl.create_framebuffer().unwrap();
    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(f));
    gl.framebuffer_texture_2d(
        glow::FRAMEBUFFER,
        glow::COLOR_ATTACHMENT0,
        glow::TEXTURE_2D,
        Some(t),
        0,
    );
    assert_eq!(
        gl.check_framebuffer_status(glow::FRAMEBUFFER),
        glow::FRAMEBUFFER_COMPLETE
    );
    (t, f)
}
unsafe fn paint(
    gl: &glow::Context,
    r: &mut reconstruction::Reconstruction,
    f: &runner::FrameSnapshot,
    filter: i32,
    uv: [f32; 6],
    target: glow::Framebuffer,
) -> Vec<u8> {
    paint_impl(gl, r, f, filter, uv, target, false)
}
unsafe fn paint_impl(
    gl: &glow::Context,
    r: &mut reconstruction::Reconstruction,
    f: &runner::FrameSnapshot,
    filter: i32,
    uv: [f32; 6],
    target: glow::Framebuffer,
    masked: bool,
) -> Vec<u8> {
    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target));
    gl.viewport(0, 0, 96, 64);
    for cap in [
        glow::BLEND,
        glow::DEPTH_TEST,
        glow::CULL_FACE,
        glow::SCISSOR_TEST,
        glow::DITHER,
    ] {
        gl.disable(cap);
    }
    gl.color_mask(true, true, true, true);
    gl.clear_color(0., 0., 0., 1.);
    gl.clear(glow::COLOR_BUFFER_BIT);
    if masked {
        gl.viewport(3, 2, 88, 59);
        gl.enable(glow::SCISSOR_TEST);
        gl.scissor(11, 7, 73, 49);
        gl.color_mask(true, false, true, false);
    }
    r.queue(f, filter);
    r.paint(gl, uv, filter);
    let mut pixels = vec![0; 96 * 64 * 4];
    gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
    gl.read_pixels(
        0,
        0,
        96,
        64,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        glow::PixelPackData::Slice(&mut pixels),
    );
    assert_eq!(gl.get_error(), glow::NO_ERROR);
    pixels
}
#[test]
#[ignore = "requires native EGL desktop GL/GLES; compares real presenter paths"]
fn presenter_shared_and_cpu_paths_match_all_gpu_filters_and_fallbacks() {
    unsafe {
        for es in [false, true] {
            let ui = Ui::new(es);
            let gl =
                glow::Context::from_loader_function_cstr(|n| (ui.get)(n.as_ptr()).cast_const());
            println!("UI {}", gl.get_parameter_string(glow::VERSION));
            let mut owner = pocket_gles::gpu::ExternalGpuContext::capture_current().unwrap();
            let policy = Arc::new(AtomicBool::new(false));
            let mut shared = reconstruction::Reconstruction::new(
                &gl,
                Some(owner.consumer()),
                policy.clone(),
                true,
            )
            .unwrap();
            assert!(policy.load(Ordering::Relaxed));
            let mut cpu = reconstruction::Reconstruction::new(
                &gl,
                None,
                Arc::new(AtomicBool::new(false)),
                true,
            )
            .unwrap();
            let (t, out) = output(&gl, 96, 64);
            let rotations = [
                [0., 0., 1., 0., 0., 1.],
                [0., 1., 0., -1., 1., 0.],
                [1., 1., -1., 0., 0., -1.],
                [1., 0., 0., 1., -1., 0.],
            ];
            for ((w, h), scale, level) in [
                ((320, 240), 1, 11),
                ((240, 320), 1, 1),
                ((320, 240), 5, 21),
                ((240, 320), 5, 31),
            ] {
                let f = frame(owner.source(), w, h, scale, level);
                let mut plain = f.clone();
                plain.shared = None;
                let mut gpu_only = f.clone();
                gpu_only.rgba.clear();
                for filter in [0, 1, 2, 3, 4, 5, 7] {
                    for uv in rotations {
                        let a = paint(&gl, &mut shared, &f, filter, uv, out);
                        let b = paint(&gl, &mut cpu, &plain, filter, uv, out);
                        assert_eq!(
                            a, b,
                            "es={es} {w}x{h} policy={scale} filter={filter} rotation={uv:?}"
                        );
                        let no_readback = paint(&gl, &mut shared, &gpu_only, filter, uv, out);
                        assert_eq!(no_readback, b, "GPU-only presenter differs");
                        assert!(policy.load(Ordering::Relaxed));
                        assert_eq!(
                            shared.reported_path,
                            Some(super::PresentationPath::GpuShared)
                        );
                        assert_eq!(
                            cpu.reported_path,
                            Some(super::PresentationPath::Cpu("no_shared_frame_supplied"))
                        );
                        assert!(
                            gpu_only.deferred.as_ref().unwrap().cached_rgba().is_none(),
                            "GPU presentation unexpectedly materialized CPU pixels"
                        );
                    }
                }
                let lazy = gpu_only.clone();
                let filtered = paint(&gl, &mut shared, &lazy, 6, rotations[0], out);
                let expected = paint(&gl, &mut cpu, &plain, 6, rotations[0], out);
                assert_eq!(
                    filtered, expected,
                    "lazy CPU xBRZ differs: es={es} {w}x{h} scale={scale}"
                );
                assert!(lazy.deferred.as_ref().unwrap().cached_rgba().is_some());
                assert!(!policy.load(Ordering::Relaxed));
            }
            let f = frame(owner.source(), 320, 240, 1, 11);
            let mut plain = f.clone();
            plain.shared = None;
            let a = paint_impl(&gl, &mut shared, &f, 0, rotations[0], out, true);
            let mut viewport = [0; 4];
            gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
            let mut mask = [0; 4];
            gl.get_parameter_i32_slice(glow::COLOR_WRITEMASK, &mut mask);
            assert_eq!(viewport, [3, 2, 88, 59]);
            assert_eq!(mask, [1, 0, 1, 0]);
            assert!(gl.is_enabled(glow::SCISSOR_TEST));
            let b = paint_impl(&gl, &mut cpu, &plain, 0, rotations[0], out, true);
            assert_eq!(a, b, "shared conversion did not preserve UI state");
            let mut wrong = pocket_gles::gpu::ExternalGpuContext::capture_current().unwrap();
            let failed = Arc::new(AtomicBool::new(false));
            let mut fallback = reconstruction::Reconstruction::new(
                &gl,
                Some(wrong.consumer()),
                failed.clone(),
                true,
            )
            .unwrap();
            let mut lazy = f.clone();
            lazy.rgba.clear();
            assert!(lazy.deferred.as_ref().unwrap().cached_rgba().is_none());
            assert_eq!(
                paint(&gl, &mut fallback, &lazy, 3, rotations[0], out),
                paint(&gl, &mut cpu, &plain, 3, rotations[0], out)
            );
            assert!(!failed.load(Ordering::Relaxed));
            assert_eq!(lazy.cpu_rgba().unwrap().as_ref(), f.rgba);
            assert!(lazy.deferred.as_ref().unwrap().cached_rgba().is_some());
            assert_eq!(
                paint(&gl, &mut fallback, &plain, 1, rotations[0], out),
                paint(&gl, &mut cpu, &plain, 1, rotations[0], out)
            );
            assert!(!failed.load(Ordering::Relaxed));
            fallback.destroy(&gl);
            wrong.close();
            owner.close();
            assert_eq!(
                paint(&gl, &mut shared, &lazy, 0, rotations[0], out),
                paint(&gl, &mut cpu, &plain, 0, rotations[0], out)
            );
            assert!(!policy.load(Ordering::Relaxed));
            shared.destroy(&gl);
            cpu.destroy(&gl);
            gl.delete_framebuffer(out);
            gl.delete_texture(t);
            assert_eq!(gl.get_error(), glow::NO_ERROR);
        }
    }
}

#[repr(C)]
struct ThreadTime {
    seconds: std::ffi::c_long,
    nanos: std::ffi::c_long,
}
extern "C" {
    fn clock_gettime(clock: i32, out: *mut ThreadTime) -> i32;
}
fn thread_cpu_ns() -> u64 {
    let mut t = ThreadTime {
        seconds: 0,
        nanos: 0,
    };
    assert_eq!(unsafe { clock_gettime(3, &mut t) }, 0);
    t.seconds as u64 * 1_000_000_000 + t.nanos as u64
}

// Synthetic steady-state benchmark, explicitly opt-in. No guest ARM execution,
// vsync or game pacing. Source and UI CPU omit Mesa worker CPU; wall time drains
// the final UI GPU queue. Context setup/drop and six warmup frames are excluded.
#[test]
#[ignore = "requires native GL/GLES; injects recoverable presentation failures"]
fn presenter_keeps_last_image_when_shared_and_cpu_paths_both_fail() {
    #[derive(Debug)]
    struct FailedSource {
        logical: (u32, u32),
        native: (u32, u32),
    }
    impl pocket_core::kernel::framebuffer::deferred::CpuPixelSource for FailedSource {
        fn logical_size(&self) -> (u32, u32) {
            self.logical
        }
        fn native_size(&self) -> (u32, u32) {
            self.native
        }
        fn materialize(
            &self,
        ) -> Result<pocket_core::kernel::framebuffer::deferred::MaterializedPixels, String>
        {
            Err("injected unavailable GPU source".into())
        }
    }
    unsafe {
        for es in [false, true] {
            let ui = Ui::new(es);
            let gl =
                glow::Context::from_loader_function_cstr(|n| (ui.get)(n.as_ptr()).cast_const());
            let mut owner = pocket_gles::gpu::ExternalGpuContext::capture_current().unwrap();
            let policy = Arc::new(AtomicBool::new(false));
            let mut presenter = reconstruction::Reconstruction::new(
                &gl,
                Some(owner.consumer()),
                policy.clone(),
                true,
            )
            .unwrap();
            let mut reference = reconstruction::Reconstruction::new(
                &gl,
                None,
                Arc::new(AtomicBool::new(false)),
                false,
            )
            .unwrap();
            let (texture, out) = output(&gl, 96, 64);
            let uv = [0., 0., 1., 0., 0., 1.];
            let old = frame(owner.source(), 320, 240, 1, 11);
            let mut plain = old.clone();
            plain.shared = None;
            let expected = paint(&gl, &mut reference, &plain, 0, uv, out);
            assert_eq!(paint(&gl, &mut presenter, &plain, 0, uv, out), expected);
            assert!(
                presenter.staging.is_none(),
                "unused shared capability changed CPU upload path"
            );
            assert_eq!(paint(&gl, &mut presenter, &old, 0, uv, out), expected);
            assert!(
                reference.staging.is_none(),
                "CPU-only path allocated a spare texture"
            );
            let current = presenter.texture;
            let valid_size = presenter.size;
            let next = frame(owner.source(), 320, 240, 5, 21);
            let mut unreadable = next.clone();
            unreadable.rgba.clear();
            unreadable.deferred = Some(Arc::new(
                pocket_core::kernel::framebuffer::deferred::DeferredPresentation::new(
                    Arc::new(FailedSource {
                        logical: (320, 240),
                        native: (next.texture_width, next.texture_height),
                    }),
                    21,
                )
                .unwrap(),
            ));
            // Failure occurs after reallocating the larger spare attachment.
            // The displayed texture must remain untouched, even if CPU fallback fails.
            let program = presenter.copy_program.unwrap();
            gl.delete_program(program);
            assert_eq!(
                paint(&gl, &mut presenter, &unreadable, 0, uv, out),
                expected
            );
            presenter.copy_program = None; // Already deleted for the injection.
            assert_eq!(presenter.texture, current);
            assert_eq!(
                presenter.reported_path,
                Some(super::PresentationPath::Retained(
                    "gpu_and_cpu_frame_unavailable"
                ))
            );
            assert_eq!(presenter.size, valid_size);
            assert!(!policy.load(Ordering::Relaxed));
            assert_eq!(gl.get_error(), glow::NO_ERROR);
            // A later valid CPU upload must recover and commit the new native size.
            let mut next_cpu = next.clone();
            next_cpu.shared = None;
            let expected_next = paint(&gl, &mut reference, &next_cpu, 0, uv, out);
            assert_eq!(
                paint(&gl, &mut presenter, &next_cpu, 0, uv, out),
                expected_next
            );
            assert_eq!(presenter.size, [next.texture_width, next.texture_height]);
            let current = presenter.texture;
            let mut rejected = next_cpu.clone();
            rejected.texture_width = gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE) as u32 + 1;
            rejected.texture_height = 1;
            rejected.rgba = vec![0; rejected.texture_width as usize * 4];
            rejected.deferred = None;
            assert_eq!(
                paint(&gl, &mut presenter, &rejected, 0, uv, out),
                expected_next
            );
            assert_eq!(
                presenter.texture, current,
                "failed CPU upload replaced displayed image"
            );
            assert_eq!(presenter.size, [next.texture_width, next.texture_height]);
            assert_eq!(gl.get_error(), glow::NO_ERROR);
            assert_eq!(paint(&gl, &mut presenter, &plain, 0, uv, out), expected);
            presenter.destroy(&gl);
            reference.destroy(&gl);
            owner.close();
            gl.delete_framebuffer(out);
            gl.delete_texture(texture);
            assert_eq!(gl.get_error(), glow::NO_ERROR);
        }
    }
}

#[test]
#[ignore = "requires native EGL context destruction/recreation"]
fn presenter_recreates_from_cached_pixels_without_reusing_old_gpu_names() {
    unsafe {
        for es in [false, true] {
            let previous = Ui::new(es);
            let mut old_owner = pocket_gles::gpu::ExternalGpuContext::capture_current().unwrap();
            let mut saved = frame(old_owner.source(), 320, 240, 5, 21);
            let expected = saved.rgba.clone();
            saved.rgba.clear();
            assert_eq!(saved.cpu_rgba().unwrap().as_ref(), expected);
            let mut uncached = frame(old_owner.source(), 320, 240, 1, 11);
            uncached.rgba.clear();
            assert!(uncached.deferred.as_ref().unwrap().cached_rgba().is_none());
            old_owner.close();
            drop(previous); // Destroy the actual old context/surface/display.
            let next = Ui::new(es);
            let gl =
                glow::Context::from_loader_function_cstr(|n| (next.get)(n.as_ptr()).cast_const());
            let mut owner = pocket_gles::gpu::ExternalGpuContext::capture_current().unwrap();
            assert!(
                uncached.cpu_only().is_err(),
                "uncached pixels resurrected from a dead share group"
            );
            assert!(uncached.deferred.as_ref().unwrap().cached_rgba().is_none());
            let captured = crate::app::filtered_snapshot_image(
                &saved,
                1,
                image::imageops::FilterType::Nearest,
            )
            .unwrap();
            assert_eq!(
                (captured.width(), captured.height()),
                (saved.texture_width, saved.texture_height)
            );
            assert_eq!(captured.into_raw(), expected);
            let detached = saved.cpu_only().unwrap();
            assert!(detached.shared.is_none() && detached.deferred.is_none());
            assert_eq!(detached.rgba, expected);
            let policy = Arc::new(AtomicBool::new(false));
            let mut recreated = reconstruction::Reconstruction::new(
                &gl,
                Some(owner.consumer()),
                policy.clone(),
                true,
            )
            .unwrap();
            let mut reference = reconstruction::Reconstruction::new(
                &gl,
                None,
                Arc::new(AtomicBool::new(false)),
                false,
            )
            .unwrap();
            let (texture, out) = output(&gl, 96, 64);
            let uv = [0., 0., 1., 0., 0., 1.];
            for filter in [0, 5, 6, 7] {
                assert_eq!(
                    paint(&gl, &mut recreated, &detached, filter, uv, out),
                    paint(&gl, &mut reference, &detached, filter, uv, out)
                );
            }
            assert!(
                policy.load(Ordering::Relaxed),
                "old share group disabled new GPU capability"
            );
            assert!(
                recreated.staging.is_none(),
                "detached CPU frame attempted stale GPU conversion"
            );
            let fresh = frame(owner.source(), 320, 240, 1, 11);
            let mut plain = fresh.clone();
            plain.shared = None;
            let mut lazy = fresh.clone();
            lazy.rgba.clear();
            assert_eq!(
                paint(&gl, &mut recreated, &lazy, 3, uv, out),
                paint(&gl, &mut reference, &plain, 3, uv, out)
            );
            assert!(lazy.deferred.as_ref().unwrap().cached_rgba().is_none());
            assert!(policy.load(Ordering::Relaxed));
            recreated.destroy(&gl);
            reference.destroy(&gl);
            owner.close();
            gl.delete_framebuffer(out);
            gl.delete_texture(texture);
            assert_eq!(gl.get_error(), glow::NO_ERROR);
        }
    }
}

#[test]
#[ignore = "native EGL benchmark; run alone with --nocapture --test-threads=1"]
fn presenter_benchmark_current_cpu_and_shared_transport() {
    unsafe {
        const WARM: usize = 6;
        const COUNT: usize = 24;
        for es in [false, true] {
            let ui = Ui::new(es);
            let gl =
                glow::Context::from_loader_function_cstr(|n| (ui.get)(n.as_ptr()).cast_const());
            let mut owner = pocket_gles::gpu::ExternalGpuContext::capture_current().unwrap();
            let shared_policy = Arc::new(AtomicBool::new(false));
            let mut gpu = reconstruction::Reconstruction::new(
                &gl,
                Some(owner.consumer()),
                shared_policy.clone(),
                true,
            )
            .unwrap();
            let mut cpu = reconstruction::Reconstruction::new(
                &gl,
                None,
                Arc::new(AtomicBool::new(false)),
                true,
            )
            .unwrap();
            let (texture, target) = output(&gl, 960, 720);
            for scale in [1u8, 5] {
                // Alternate the order to reduce warm-cache/order bias.
                let order = match std::env::var("POCKET_PRESENTER_BENCH_ORDER")
                    .unwrap_or_default()
                    .as_str()
                {
                    "rotate1" => [1, 2, 0, 0, 2, 1],
                    "rotate2" => [2, 0, 1, 1, 0, 2],
                    _ => [0, 1, 2, 2, 1, 0],
                };
                for mode in order {
                    let use_shared = mode != 0;
                    let source = owner.source();
                    let barrier = Arc::new(std::sync::Barrier::new(2));
                    let (send, receive) = std::sync::mpsc::sync_channel(1);
                    let (ack, acked) = std::sync::mpsc::sync_channel(1);
                    let (stats, received_stats) = std::sync::mpsc::sync_channel(1);
                    let (stop, stopping) = std::sync::mpsc::sync_channel(1);
                    let worker = std::thread::spawn({
                        let barrier = barrier.clone();
                        move || {
                            pocket_gles::gpu::with_shared_context_source(Some(source), || {
                                let mut c = pocket_gles::context::Context::new_rgb565(320, 240);
                                c.set_internal_resolution(scale);
                                c.enable_gpu().unwrap();
                                let mut fb = pocket_core::kernel::Framebuffer::new(320, 240);
                                let mut started = 0;
                                for index in 0..WARM + COUNT {
                                    if index == WARM {
                                        barrier.wait();
                                        started = thread_cpu_ns();
                                    }
                                    c.state.scissor = None;
                                    c.clear_color =
                                        [0.2 + (index % 50) as f32 / 100., 0.54, 0.81, 1.];
                                    c.clear(pocket_gles::GL_COLOR_BUFFER_BIT);
                                    c.state.scissor = Some((80, 60, 160, 120));
                                    c.clear_color = [0.9, 0.2, 0.4, 1.];
                                    c.clear(pocket_gles::GL_COLOR_BUFFER_BIT);
                                    let (rgba, shared, (tw, th)) = if mode == 2 {
                                        let shared = c
                                            .snapshot_shared_frame()
                                            .expect("GPU-only export failed");
                                        let size = shared.size();
                                        (Vec::new(), Some(shared), size)
                                    } else {
                                        let mut high = fb.take_render_pixels();
                                        c.present_rgb565(&mut fb.pixels);
                                        let physical = c.scaled_display_rgba_into(&mut high);
                                        fb.mark_dirty();
                                        if let Some((w, h)) = physical {
                                            fb.set_render_presentation_rgba(w, h, high);
                                        }
                                        let mut rgba = Vec::new();
                                        fb.snapshot_presentation_rgba8888_into(&mut rgba);
                                        let shared =
                                            if use_shared {
                                                Some(c.snapshot_shared_frame().expect(
                                                    "benchmark did not use shared transport",
                                                ))
                                            } else {
                                                None
                                            };
                                        let (tw, th) = fb.presentation_size();
                                        (rgba, shared, (tw, th))
                                    };
                                    send.send(runner::FrameSnapshot {
                                        presentation_stats: Default::default(),
                                        width: 320,
                                        height: 240,
                                        texture_width: tw,
                                        texture_height: th,
                                        rgba,
                                        text_input: false,
                                        shared,
                                        deferred: None,
                                        backlight: 11,
                                    })
                                    .unwrap();
                                    acked.recv().unwrap();
                                }
                                stats.send(thread_cpu_ns() - started).unwrap();
                                // Keep native resources alive until the measured final UI
                                // queue drain, excluding context teardown from this result.
                                stopping.recv().unwrap();
                            })
                        }
                    });
                    let renderer = if use_shared { &mut gpu } else { &mut cpu };
                    let mut start_cpu = 0;
                    let mut wall = None;
                    for index in 0..WARM + COUNT {
                        if index == WARM {
                            gl.finish();
                            barrier.wait();
                            start_cpu = thread_cpu_ns();
                            wall = Some(std::time::Instant::now());
                        }
                        let frame = receive.recv().unwrap();
                        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target));
                        gl.viewport(0, 0, 960, 720);
                        for cap in [
                            glow::BLEND,
                            glow::DEPTH_TEST,
                            glow::CULL_FACE,
                            glow::SCISSOR_TEST,
                            glow::DITHER,
                        ] {
                            gl.disable(cap);
                        }
                        gl.color_mask(true, true, true, true);
                        renderer.queue(&frame, 1);
                        renderer.paint(&gl, [0., 0., 1., 0., 0., 1.], 1);
                        gl.flush();
                        if use_shared {
                            assert!(
                                shared_policy.load(Ordering::Relaxed),
                                "benchmark silently fell back to CPU"
                            );
                        }
                        drop(frame);
                        ack.send(()).unwrap();
                    }
                    gl.finish();
                    let wall_ms = wall.unwrap().elapsed().as_secs_f64() * 1000. / COUNT as f64;
                    let ui_cpu = (thread_cpu_ns() - start_cpu) as f64 / 1_000_000. / COUNT as f64;
                    let source_cpu =
                        received_stats.recv().unwrap() as f64 / 1_000_000. / COUNT as f64;
                    stop.send(()).unwrap();
                    worker.join().unwrap();
                    println!("PRESENTER_BENCH api={} scale={} path={} producer_cpu_ms={source_cpu:.5} ui_cpu_ms={ui_cpu:.5} sum_cpu_ms={:.5} wall_ms={wall_ms:.5}",if es{"gles"}else{"gl_core"},if scale==1{1}else{4},if mode==2 {"gpu_only"}else if use_shared {"shared"}else{"cpu"},source_cpu+ui_cpu);
                    assert_eq!(gl.get_error(), glow::NO_ERROR);
                }
            }
            gpu.destroy(&gl);
            cpu.destroy(&gl);
            owner.collect_retired_frames().unwrap();
            owner.close();
            gl.delete_framebuffer(target);
            gl.delete_texture(texture);
        }
    }
}
