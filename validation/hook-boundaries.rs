mod pocket_desktop {
use std::sync::{Arc,Mutex,atomic::{AtomicBool,AtomicU32,Ordering},mpsc::{Sender,Receiver,TryRecvError}};
use std::time::{Duration,Instant};
use pocket_core::kernel::{KernelState,FrameAction,FrameHook,InputEvent};
pub struct FrameSnapshot {
    pub presentation_stats: pocket_core::kernel::framebuffer::PresentationStats,
    pub width: u32,
    pub height: u32,
    pub texture_width: u32,
    pub texture_height: u32,
    pub rgba: Vec<u8>,
    pub text_input: bool,
    pub shared: Option<pocket_core::winceapi::gles::SharedFrame>,
    pub deferred: Option<Arc<pocket_core::kernel::framebuffer::deferred::DeferredPresentation>>,
    pub backlight: u32,
}
impl FrameSnapshot {
    pub(crate) fn from_shared(
        fb: &pocket_core::kernel::framebuffer::deferred::DeferredFramebuffer,
        shared: pocket_core::winceapi::gles::SharedFrame,
    ) -> Result<Self, String> {
        let source = fb
            .gpu_source()
            .cloned()
            .ok_or("missing GPU pixel authority")?;
        if source.logical_size() != fb.logical_size() || fb.bpp != 16 {
            return Err("shared logical geometry mismatch".into());
        }
        let deferred = Arc::new(
            pocket_core::kernel::framebuffer::deferred::DeferredPresentation::new(
                source,
                fb.backlight_level(),
            )?,
        );
        if deferred.size() != shared.size() {
            return Err("shared snapshot geometry mismatch".into());
        }
        Ok(Self {
            presentation_stats: fb.presentation_stats.clone(),
            width: fb.width,
            height: fb.height,
            texture_width: shared.size().0,
            texture_height: shared.size().1,
            rgba: Vec::new(),
            text_input: false,
            shared: Some(shared),
            deferred: Some(deferred),
            backlight: fb.backlight_level(),
        })
    }
    /// Detach a saved image from a previous UI share group. Call before a
    /// planned context teardown, or after teardown only if pixels are cached.
    pub(crate) fn cpu_only(&self) -> Result<Self, String> {
        Ok(Self {
            presentation_stats: self.presentation_stats.clone(),
            width: self.width,
            height: self.height,
            texture_width: self.texture_width,
            texture_height: self.texture_height,
            rgba: self.cpu_rgba()?.into_owned(),
            text_input: self.text_input,
            shared: None,
            deferred: None,
            backlight: self.backlight,
        })
    }
    /// CPU filters, captures and conversion failures request bytes explicitly.
    pub(crate) fn cpu_rgba(&self) -> Result<std::borrow::Cow<'_, [u8]>, String> {
        let expected = (self.texture_width as usize)
            .checked_mul(self.texture_height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or("snapshot size overflow")?;
        if self.rgba.len() == expected {
            return Ok(std::borrow::Cow::Borrowed(&self.rgba));
        }
        let deferred = self
            .deferred
            .as_ref()
            .ok_or("CPU fallback pixels unavailable")?;
        if deferred.size() != (self.texture_width, self.texture_height) {
            return Err("CPU fallback geometry mismatch".into());
        }
        Ok(std::borrow::Cow::Owned((*deferred.rgba()?).clone()))
    }
    fn from_framebuffer(fb: &pocket_core::kernel::Framebuffer) -> Self {
        let mut rgba = Vec::new();
        fb.snapshot_presentation_rgba8888_into(&mut rgba);
        Self {
            presentation_stats: fb.presentation_stats.clone(),
            text_input: false,
            shared: None,
            deferred: None,
            backlight: fb.backlight_level(),
            width: fb.width,
            height: fb.height,
            texture_width: fb.presentation_size().0,
            texture_height: fb.presentation_size().1,
            rgba,
        }
    }

    /// Like [`FrameSnapshot::from_framebuffer`] but reuses an existing
    /// `rgba` buffer the caller owns. Saves the per-frame 300 KiB
    /// allocation that the naive constructor does.
    fn fill_from_framebuffer(fb: &pocket_core::kernel::Framebuffer, scratch: &mut Vec<u8>) -> Self {
        fb.snapshot_presentation_rgba8888_into(scratch);
        Self {
            presentation_stats: fb.presentation_stats.clone(),
            text_input: false,
            shared: None,
            deferred: None,
            backlight: fb.backlight_level(),
            width: fb.width,
            height: fb.height,
            texture_width: fb.presentation_size().0,
            texture_height: fb.presentation_size().1,
            // `std::mem::take` hands ownership of the scratch buffer
            // to the snapshot we are about to ship across the
            // channel and leaves an empty `Vec` behind. The next
            // call resizes it back up — same allocation, no churn.
            rgba: std::mem::take(scratch),
        }
    }
}
const FRAME_PUSH_INTERVAL: Duration = Duration::from_millis(16);
fn presentation_phase(last: Option<Instant>, now: Instant) -> Option<Instant> {
    let Some(last) = last else {
        return Some(now);
    };
    let elapsed = now.saturating_duration_since(last);
    if elapsed < FRAME_PUSH_INTERVAL {
        return None;
    }
    // Skip missed slots after a pause; never send a burst of catch-up frames.
    let remainder =
        Duration::from_nanos((elapsed.as_nanos() % FRAME_PUSH_INTERVAL.as_nanos()) as u64);
    Some(now - remainder)
}
impl FrameHook for RunHook {
    fn on_frame(&mut self, state: &mut KernelState) -> FrameAction {
        self.reap_children();
        let active = self.foreground.load(Ordering::Acquire) == self.pid;
        let mut stop_requested = if active {
            self.drain_input(&mut state.pending_input)
        } else {
            self.stop_all.load(Ordering::Acquire)
        };
        if std::mem::take(&mut self.recording_audio_pending) {
            state.audio.record_output(self.recording_audio.clone());
        }
        if std::mem::take(&mut self.refresh_colors_gps) && !stop_requested {
            if let Some(config) = self.colors_random_gps.as_ref() {
                if let Some(position) = config.simulated_gps_position() {
                    if state
                        .vfs
                        .gps_service()
                        .set_fixed_position(Some(position))
                        .is_ok()
                    {
                        if let Some(tx) = &self.gps_notice_tx {
                            let _ = tx.send(position);
                        }
                    }
                }
            }
        }
        if !stop_requested {
            self.run_pending_child(state);
        }
        stop_requested |= self.stopped_by_user;

        // Stream the latest framebuffer up to the GUI, but at most
        // once per `FRAME_PUSH_INTERVAL`. The hook is invoked after
        // every dispatched WinCE call; without throttling a single
        // game tick that does ~2k `BitBlt`s would generate ~2k
        // 300 KiB snapshots — the work that turned a logical 60 fps
        // game into a sub-1 fps preview in the desktop launcher.
        if !self.frame_send_failed && self.foreground.load(Ordering::Acquire) == self.pid {
            if let Some(tx) = self.frame_tx.as_ref() {
                let counter = state.framebuffer.frame_counter;
                if counter != self.last_frame {
                    let now = Instant::now();
                    // Keep the presentation phase instead of restarting the
                    // interval after each GPU readback/scheduler delay. The
                    // latter accumulates jitter and periodically skips frames.
                    if let Some(phase) = presentation_phase(self.last_emit_at, now) {
                        let shared = if !state.shell_keyboard.visible
                            && state.gdi.shell_menus.tracking.is_none()
                        {
                            pocket_core::winceapi::gles::take_shared_window_frame(
                                &state.framebuffer,
                            )
                        } else {
                            None
                        };
                        let mut snapshot = if let Some(shared) = shared {
                            match FrameSnapshot::from_shared(&state.framebuffer, shared) {
                                Ok(frame) => frame,
                                Err(error) => {
                                    log::error!("GPU snapshot failed: {error}");
                                    state.should_stop = true;
                                    return FrameAction::Stop;
                                }
                            }
                        } else {
                            let black = match state.framebuffer.cpu() {
                                Ok(frame) => frame.is_all_black(),
                                Err(error) => {
                                    log::error!("framebuffer read failed: {error}");
                                    state.should_stop = true;
                                    return FrameAction::Stop;
                                }
                            };
                            if black && self.saw_non_black {
                                return if stop_requested {
                                    state.should_stop = true;
                                    FrameAction::Stop
                                } else {
                                    FrameAction::Continue
                                };
                            }
                            let presentation = match state.presentation_framebuffer() {
                                Ok(frame) => frame,
                                Err(error) => {
                                    log::error!("presentation read failed: {error}");
                                    state.should_stop = true;
                                    return FrameAction::Stop;
                                }
                            };
                            FrameSnapshot::fill_from_framebuffer(&presentation, &mut self.scratch)
                        };
                        // Valid GPU frames (including black clears) are accepted without a
                        // full CPU black scan. Final CPU snapshots keep the legacy heuristic.
                        self.saw_non_black = true;
                        self.last_frame = counter;
                        self.last_emit_at = Some(phase);
                        snapshot.text_input = state.gapi_action_key
                            == pocket_core::kernel::gapi::VK_RETURN
                            && state
                                .controls
                                .get(if state.shell_keyboard.sip {
                                    state.shell_keyboard.target
                                } else {
                                    state.controls.focus
                                })
                                .is_some_and(|c| {
                                    c.class == pocket_core::kernel::controls::ControlClass::Edit
                                        && c.visible
                                        && c.enabled
                                        && c.style & 0x800 == 0
                                });
                        if tx.send(snapshot).is_err() {
                            // GUI thread dropped the receiver — the user
                            // closed the run screen / quit the launcher.
                            self.frame_send_failed = true;
                        }
                    }
                }
            }
        }

        if stop_requested {
            state.should_stop = true;
            FrameAction::Stop
        } else {
            FrameAction::Continue
        }
    }
}
impl RunHook {fn new(frame_tx:Option<Sender<FrameSnapshot>>,_:Option<Receiver<()>>)->Self {Self {recording_audio:None,recording_audio_pending:false,colors_random_gps:None,refresh_colors_gps:false,gps_notice_tx:None,stop_all:Arc::new(AtomicBool::new(false)),foreground:Arc::new(AtomicU32::new(1)),pid:1,stopped_by_user:false,frame_send_failed:false,frame_tx,last_frame:0,saw_non_black:false,last_emit_at:None,scratch:vec![]}}}
struct GpsConfig; impl GpsConfig {fn simulated_gps_position(&self)->Option<(f64,f64)>{None}}
struct RunHook { recording_audio: Option<Arc<pocket_core::kernel::audio::OutputRecording>>, recording_audio_pending:bool,
 colors_random_gps:Option<GpsConfig>, refresh_colors_gps:bool, gps_notice_tx:Option<Sender<(f64,f64)>>,
 stop_all:Arc<AtomicBool>, foreground:Arc<AtomicU32>, pid:u32, stopped_by_user:bool,
 frame_send_failed:bool, frame_tx:Option<Sender<FrameSnapshot>>, last_frame:u64, saw_non_black:bool,
 last_emit_at:Option<Instant>, scratch:Vec<u8> }
impl RunHook {fn reap_children(&mut self){} fn run_pending_child(&mut self,_:&mut KernelState){} fn drain_input(&mut self,_:&mut std::collections::VecDeque<InputEvent>)->bool{false}}

enum InputCommand {Stop}
#[derive(Debug,Clone)]
pub struct Runner {
    inner: Arc<Mutex<()>>,
    missing_api_logging: Arc<AtomicBool>,
    shared_context_source: Arc<Mutex<Option<pocket_core::winceapi::gles::SharedContextSource>>>,
    shared_export: Arc<AtomicBool>,
}
impl Default for Runner {
    fn default() -> Self {
        Self {
            inner: Default::default(),
            missing_api_logging: Arc::new(AtomicBool::new(true)),
            shared_context_source: Default::default(),
            shared_export: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl Runner {
pub fn new() -> Self {
        Self::default()
    }
pub fn set_shared_context_source(
        &self,
        source: Option<pocket_core::winceapi::gles::SharedContextSource>,
    ) {
        *self
            .shared_context_source
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = source;
    }
pub fn shared_export_policy(&self) -> Arc<AtomicBool> {
        self.shared_export.clone()
    }
pub(crate) fn stop_shared_worker(
        &self,
        context: &mut Option<pocket_core::winceapi::gles::ExternalGpuContext>,
        input: &mut Option<Sender<InputCommand>>,
        frames: &mut Option<Receiver<FrameSnapshot>>,
        worker: &mut Option<std::thread::JoinHandle<()>>,
    ) {
        self.shared_export.store(false, Ordering::Release);
        self.set_shared_context_source(None);
        if let Some(input) = input.take() {
            let _ = input.send(InputCommand::Stop);
        }
        frames.take();
        if let Some(worker) = worker.take() {
            let _ = worker.join();
        }
        if let Some(context) = context.as_mut() {
            context.close();
        }
        context.take();
    }
}

mod deferred_read_tests {
    use super::*;
    use pocket_core::kernel::framebuffer::deferred::{CpuPixelSource, MaterializedPixels};
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[derive(Debug)]
    struct Image {
        reads: AtomicUsize,
        fail: AtomicBool,
    }
    impl CpuPixelSource for Image {
        fn logical_size(&self) -> (u32, u32) {
            (3, 2)
        }
        fn native_size(&self) -> (u32, u32) {
            (3, 2)
        }
        fn materialize(&self) -> Result<MaterializedPixels, String> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            if self.fail.load(Ordering::Relaxed) {
                return Err("test read failure".into());
            }
            Ok(MaterializedPixels {
                logical_size: (3, 2),
                native_size: (3, 2),
                rgb565: vec![0, 0xf8, 0, 0xf8, 0, 0xf8, 0, 0xf8, 0, 0xf8, 0, 0xf8],
                native_rgba: None,
            })
        }
    }
    fn process() -> pocket_core::kernel::Process {
        use pocket_core::{
            cpu::stub::StubCpu,
            pe::{LoadedImage, LoadedSection},
        };
        let mut cpu = StubCpu::new();
        let image = LoadedImage {
            source_path: "hook-test".into(),
            machine: pocket_core::pe::machine::ARM,
            subsystem: pocket_core::pe::subsystem::WINDOWS_CE_GUI,
            image_base: 0x10000,
            stack_reserve: 0x10000,
            size_of_image: 0x2000,
            entry_point: 0x1000,
            sections: vec![LoadedSection {
                name: ".text".into(),
                virtual_address: 0x1000,
                virtual_size: 0x1000,
                characteristics: 0x60000020,
                data: vec![0; 0x1000],
            }],
            imports: vec![],
            exports: Default::default(),
            resources: vec![],
            managed_runtime: None,
        };
        pocket_core::kernel::Process::map_into(
            image,
            &mut cpu,
            &|_, _| None,
            &pocket_core::kernel::NullDispatcher,
        )
        .unwrap()
    }
    #[test]
    #[ignore = "requires native UI sharing and an execution thread"]
    fn shutdown_materializes_the_final_gpu_frame_before_closing_ui_source() {
        use pocket_core::kernel::framebuffer::{deferred::DeferredFramebuffer, Framebuffer};
        let ui = pocket_gles::gpu::Renderer::new(8, 8).unwrap();
        let mut owner = Some(unsafe {
            pocket_core::winceapi::gles::ExternalGpuContext::capture_current().unwrap()
        });
        let source = owner.as_ref().unwrap().source();
        let runner = Runner::new();
        runner.set_shared_context_source(Some(source.clone()));
        let policy = runner.shared_export_policy();
        policy.store(true, Ordering::Release);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (input_tx, input_rx) = std::sync::mpsc::channel();
        let (_frame_tx, frame_rx) = std::sync::mpsc::channel::<FrameSnapshot>();
        let (final_tx, final_rx) = std::sync::mpsc::channel();
        let worker_source = source.clone();
        let mut worker = Some(std::thread::spawn(move || {
            pocket_gles::gpu::with_shared_context_source(Some(worker_source.clone()), || {
                let mut c = pocket_gles::context::Context::new_rgb565(320, 240);
                c.enable_gpu().unwrap();
                c.clear_color = [1., 0., 0., 1.];
                c.clear(pocket_gles::GL_COLOR_BUFFER_BIT);
                let frame = c.snapshot_shared_frame().unwrap();
                let provider = Arc::new(
                    pocket_core::winceapi::gles::deferred_frame::GlesCpuPixelSource::new(
                        frame,
                        worker_source,
                        (320, 240),
                    )
                    .unwrap(),
                );
                let mut fb = DeferredFramebuffer::new(Framebuffer::new(320, 240)).unwrap();
                fb.publish_gpu(provider).unwrap();
                assert!(!fb.cpu_resident());
                ready_tx.send(()).unwrap();
                assert!(matches!(input_rx.recv().unwrap(), InputCommand::Stop));
                assert!(!policy.load(Ordering::Acquire));
                // No readback context existed before stop. The source must
                // remain open until this final CPU consumer has completed.
                let pixels = fb.cpu().unwrap().snapshot_rgba8888();
                final_tx.send(pixels[..4].to_vec()).unwrap();
                pocket_core::winceapi::gles::deferred_frame::release_reader();
            });
        }));
        ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let mut input = Some(input_tx);
        let mut frames = Some(frame_rx);
        runner.stop_shared_worker(&mut owner, &mut input, &mut frames, &mut worker);
        assert_eq!(
            final_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
            [255, 0, 0, 255]
        );
        assert!(owner.is_none() && input.is_none() && frames.is_none() && worker.is_none());
        assert!(
            pocket_gles::gpu::SharedReadback::new(&source).is_err(),
            "source not closed after join"
        );
        runner.stop_shared_worker(&mut owner, &mut input, &mut frames, &mut worker);
        drop(ui);
    }

    #[test]
    #[ignore = "requires native shared OpenGL"]
    fn actual_swap_to_hook_keeps_gpu_pixels_until_a_capture_requests_them() {
        use pocket_core::{
            cpu::{regs::ArmReg, stub::StubCpu, Cpu, Prot},
            kernel::{DispatchOutcome, Dispatcher, Thunk},
            pe::ImportBinding,
        };
        fn api(
            d: &mut pocket_core::winceapi::WinCeDispatcher,
            cpu: &mut StubCpu,
            k: &mut KernelState,
            name: &str,
            args: &[u32],
        ) -> u32 {
            for (index, reg) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
                .into_iter()
                .enumerate()
            {
                cpu.write_reg(reg, args.get(index).copied().unwrap_or(0))
                    .unwrap();
            }
            let thunk = Thunk {
                thunk_va: 0x70000000,
                iat_va: 0,
                dll: "libGLES_CM.dll".into(),
                binding: ImportBinding::Name(name.into()),
                friendly_name: None,
            };
            match d.dispatch(cpu, &thunk, k).unwrap() {
                DispatchOutcome::ReturnedR0(value) => value,
                other => panic!("unexpected {name} result: {other:?}"),
            }
        }
        let ui = pocket_gles::gpu::Renderer::new(8, 8).unwrap();
        let source = ui.shared_context_source();
        let mut d = pocket_core::winceapi::WinCeDispatcher::new();
        let mut cpu = StubCpu::new();
        cpu.map_region(0x200000, 0x4000, Prot::READ | Prot::WRITE)
            .unwrap();
        cpu.write_reg(ArmReg::Sp, 0x203000).unwrap();
        let mut process = process();
        let k = &mut process.state;
        k.framebuffer.resize(320, 240);
        let enabled = Arc::new(AtomicBool::new(true));
        pocket_core::winceapi::gles::with_shared_context_source(Some(source), || {
            pocket_core::winceapi::gles::with_shared_frame_export(enabled, || {
                let display = api(&mut d, &mut cpu, k, "eglGetDisplay", &[0]);
                assert_ne!(display, 0);
                assert_eq!(
                    api(&mut d, &mut cpu, k, "eglInitialize", &[display, 0, 0]),
                    1
                );
                assert_eq!(
                    api(
                        &mut d,
                        &mut cpu,
                        k,
                        "eglGetConfigs",
                        &[display, 0x200000, 1, 0x200004]
                    ),
                    1
                );
                let config =
                    u32::from_le_bytes(cpu.read_mem(0x200000, 4).unwrap().try_into().unwrap());
                let surface = api(
                    &mut d,
                    &mut cpu,
                    k,
                    "eglCreateWindowSurface",
                    &[display, config, 1, 0],
                );
                let context = api(
                    &mut d,
                    &mut cpu,
                    k,
                    "eglCreateContext",
                    &[display, config, 0, 0],
                );
                assert_ne!(surface, 0);
                assert_ne!(context, 0);
                assert_eq!(
                    api(
                        &mut d,
                        &mut cpu,
                        k,
                        "eglMakeCurrent",
                        &[display, surface, surface, context]
                    ),
                    1
                );
                pocket_core::winceapi::gles::set_gpu_renderer(true);
                let (tx, rx) = std::sync::mpsc::channel();
                let mut hook = RunHook::new(Some(tx), None);
                let mut frames = Vec::new();
                for color in [[65536, 0, 0, 65536], [0, 65536, 0, 65536]] {
                    api(&mut d, &mut cpu, k, "glClearColorx", &color);
                    api(&mut d, &mut cpu, k, "glClear", &[0x4000]);
                    // Pixel handoff test; cooperative pacing has its own native suite.
                    k.framebuffer.directdraw.last_present = None;
                    k.framebuffer.directdraw.pending.clear();
                    assert_eq!(
                        api(&mut d, &mut cpu, k, "eglSwapBuffers", &[display, surface]),
                        1
                    );
                    assert!(!k.framebuffer.cpu_resident());
                    hook.last_emit_at = None;
                    assert!(matches!(hook.on_frame(k), FrameAction::Continue));
                    assert!(
                        !k.framebuffer.cpu_resident(),
                        "display hook read GPU pixels"
                    );
                    let frame = rx.try_recv().unwrap();
                    assert!(frame.rgba.is_empty());
                    assert!(frame.shared.is_some());
                    assert!(frame.deferred.as_ref().unwrap().cached_rgba().is_none());
                    frames.push(frame);
                }
                pocket_core::winceapi::gles::set_gpu_renderer(false);
                assert_eq!(&frames[0].cpu_rgba().unwrap()[..4], &[255, 0, 0, 255]);
                assert_eq!(&frames[1].cpu_rgba().unwrap()[..4], &[0, 255, 0, 255]);
                assert!(
                    !k.framebuffer.cpu_resident(),
                    "UI capture populated kernel pixels"
                );
                pocket_core::winceapi::gles::deferred_frame::release_reader();
            });
        });
    }
    #[test]
    fn throttled_frames_never_read_gpu_and_due_read_failures_stop_without_stale_pixels() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut hook = RunHook::new(Some(tx), None);
        let mut process = process();
        let kernel = &mut process.state;
        kernel.framebuffer.resize(3, 2);
        let source = Arc::new(Image {
            reads: AtomicUsize::new(0),
            fail: AtomicBool::new(true),
        });
        kernel.framebuffer.publish_gpu(source.clone()).unwrap();
        hook.last_emit_at = Some(Instant::now() + Duration::from_secs(60));
        assert!(matches!(hook.on_frame(kernel), FrameAction::Continue));
        assert_eq!(source.reads.load(Ordering::Relaxed), 0);
        assert!(!kernel.framebuffer.cpu_resident());
        hook.last_emit_at = None;
        assert!(matches!(hook.on_frame(kernel), FrameAction::Stop));
        assert!(kernel.should_stop);
        assert_eq!(source.reads.load(Ordering::Relaxed), 1);
        assert!(!kernel.framebuffer.cpu_resident());
        kernel.should_stop = false;
        source.fail.store(false, Ordering::Relaxed);
        assert!(matches!(hook.on_frame(kernel), FrameAction::Continue));
        assert_eq!(source.reads.load(Ordering::Relaxed), 2);
        assert!(kernel.framebuffer.cpu_resident());
        assert_eq!(&rx.try_recv().unwrap().rgba[..4], &[255, 0, 0, 255]);
    }
}
}
mod pocket_android_jni {
use std::sync::{Arc,Mutex,atomic::{AtomicBool,AtomicU32,Ordering},mpsc::{Sender,Receiver,TryRecvError}};
use std::time::{Duration,Instant};
use pocket_core::kernel::{KernelState,FrameAction,FrameHook,InputEvent};
pub struct FrameSnapshot {
    pub presentation_stats: pocket_core::kernel::framebuffer::PresentationStats,
    pub width: u32,
    pub height: u32,
    pub texture_width: u32,
    pub texture_height: u32,
    pub rgba: Vec<u8>,
}
impl FrameSnapshot {
    pub(crate) fn from_framebuffer(fb: &pocket_core::kernel::Framebuffer) -> Self {
        let mut rgba = Vec::new();
        fb.snapshot_presentation_rgba8888_into(&mut rgba);
        Self {
            presentation_stats: fb.presentation_stats.clone(),
            width: fb.width,
            height: fb.height,
            texture_width: fb.presentation_size().0,
            texture_height: fb.presentation_size().1,
            rgba,
        }
    }

    fn from_framebuffer_into(fb: &pocket_core::kernel::Framebuffer, scratch: &mut Vec<u8>) -> Self {
        fb.snapshot_presentation_rgba8888_into(scratch);
        Self {
            presentation_stats: fb.presentation_stats.clone(),
            width: fb.width,
            height: fb.height,
            texture_width: fb.presentation_size().0,
            texture_height: fb.presentation_size().1,
            rgba: std::mem::take(scratch),
        }
    }
}
const FRAME_PUSH_INTERVAL: Duration = Duration::from_millis(16);
fn presentation_phase(last: Option<Instant>, now: Instant) -> Option<Instant> {
    let Some(last) = last else {
        return Some(now);
    };
    let elapsed = now.saturating_duration_since(last);
    if elapsed < FRAME_PUSH_INTERVAL {
        return None;
    }
    // Skip missed slots after a pause; never send a burst of catch-up frames.
    let remainder =
        Duration::from_nanos((elapsed.as_nanos() % FRAME_PUSH_INTERVAL.as_nanos()) as u64);
    Some(now - remainder)
}
impl FrameHook for SessionHook {
    fn on_frame(&mut self, kernel: &mut KernelState) -> FrameAction {
        // Drain any pending UI input into the kernel's queue.
        self.reap_children();
        let mut stop_requested = self.stop_all.load(Ordering::Acquire);
        if !self.input_disconnected && self.foreground.load(Ordering::Acquire) == self.pid {
            let input_rx = Arc::clone(&self.input_rx);
            let input = input_rx.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                match input.try_recv() {
                    Ok(InputCommand::Input(ev)) => {
                        self.queue_input(ev, &kernel.vfs.gps_service(), &mut kernel.pending_input);
                    }
                    Ok(InputCommand::Stop) => stop_requested = true,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.input_disconnected = true;
                        break;
                    }
                }
            }
        }

        stop_requested |= self.input_disconnected;
        if stop_requested {
            self.stop_all.store(true, Ordering::Release);
        }
        if !stop_requested {
            self.run_pending_child(kernel);
        }
        stop_requested |= self.stop_all.load(Ordering::Acquire);

        // Stream a fresh framebuffer if the guest produced one.
        if self.foreground.load(Ordering::Acquire) == self.pid {
            if let Ok(mut slot) = self.state.audio.lock() {
                *slot = Some(kernel.audio.tap());
            }
        }
        let counter = kernel.framebuffer.frame_counter;
        if self.foreground.load(Ordering::Acquire) == self.pid && counter != self.last_frame {
            let now = Instant::now();
            if let Some(phase) = presentation_phase(self.last_emit_at, now) {
                let black = match kernel.framebuffer.cpu() {
                    Ok(frame) => frame.is_all_black(),
                    Err(error) => {
                        log::error!("framebuffer read failed: {error}");
                        kernel.should_stop = true;
                        return FrameAction::Stop;
                    }
                };
                if black && self.saw_non_black {
                    return if stop_requested {
                        kernel.should_stop = true;
                        FrameAction::Stop
                    } else {
                        FrameAction::Continue
                    };
                }
                self.saw_non_black = !black;

                self.last_frame = counter;
                self.last_emit_at = Some(phase);
                let presentation = match kernel.presentation_framebuffer() {
                    Ok(frame) => frame,
                    Err(error) => {
                        log::error!("presentation read failed: {error}");
                        kernel.should_stop = true;
                        return FrameAction::Stop;
                    }
                };
                let frame = FrameSnapshot::from_framebuffer_into(&presentation, &mut self.scratch);
                push_frame(&self.state, frame);
            }
        }

        if stop_requested {
            kernel.should_stop = true;
            FrameAction::Stop
        } else {
            FrameAction::Continue
        }
    }
}
enum InputCommand {Input(InputEvent),Stop}
struct SessionState {audio:Mutex<Option<pocket_core::kernel::audio::AudioTap>>,latest_frame:Mutex<Option<FrameSnapshot>>} impl SessionState {fn new()->Self {Self {audio:Mutex::new(None),latest_frame:Mutex::new(None)}}}
struct SessionHook {stop_all:Arc<AtomicBool>, foreground:Arc<AtomicU32>, pid:u32,input_disconnected:bool,
 input_rx:Arc<Mutex<Receiver<InputCommand>>>,state:Arc<SessionState>,last_frame:u64,saw_non_black:bool,last_emit_at:Option<Instant>,scratch:Vec<u8>}
impl SessionHook {fn reap_children(&mut self){} fn run_pending_child(&mut self,_:&mut KernelState){} fn queue_input(&mut self,ev:InputEvent,_:&pocket_core::kernel::gps::Service,q:&mut std::collections::VecDeque<InputEvent>){q.push_back(ev);}}
fn push_frame(s: &Arc<SessionState>,f:FrameSnapshot){*s.latest_frame.lock().unwrap()=Some(f);}
impl SessionHook {fn new(state:Arc<SessionState>,input_rx:Receiver<InputCommand>)->Self {Self {stop_all:Arc::new(AtomicBool::new(false)),foreground:Arc::new(AtomicU32::new(1)),pid:1,input_disconnected:false,input_rx:Arc::new(Mutex::new(input_rx)),state,last_frame:0,saw_non_black:false,last_emit_at:None,scratch:vec![]}}}

mod deferred_read_tests {
    use super::*;
    use pocket_core::kernel::framebuffer::deferred::{CpuPixelSource, MaterializedPixels};
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[derive(Debug)]
    struct Image {
        reads: AtomicUsize,
        fail: AtomicBool,
    }
    impl CpuPixelSource for Image {
        fn logical_size(&self) -> (u32, u32) {
            (3, 2)
        }
        fn native_size(&self) -> (u32, u32) {
            (3, 2)
        }
        fn materialize(&self) -> Result<MaterializedPixels, String> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            if self.fail.load(Ordering::Relaxed) {
                return Err("test read failure".into());
            }
            Ok(MaterializedPixels {
                logical_size: (3, 2),
                native_size: (3, 2),
                rgb565: vec![0, 0xf8, 0, 0xf8, 0, 0xf8, 0, 0xf8, 0, 0xf8, 0, 0xf8],
                native_rgba: None,
            })
        }
    }
    fn process() -> pocket_core::kernel::Process {
        use pocket_core::{
            cpu::stub::StubCpu,
            pe::{LoadedImage, LoadedSection},
        };
        let mut cpu = StubCpu::new();
        let image = LoadedImage {
            source_path: "hook-test".into(),
            machine: pocket_core::pe::machine::ARM,
            subsystem: pocket_core::pe::subsystem::WINDOWS_CE_GUI,
            image_base: 0x10000,
            stack_reserve: 0x10000,
            size_of_image: 0x2000,
            entry_point: 0x1000,
            sections: vec![LoadedSection {
                name: ".text".into(),
                virtual_address: 0x1000,
                virtual_size: 0x1000,
                characteristics: 0x60000020,
                data: vec![0; 0x1000],
            }],
            imports: vec![],
            exports: Default::default(),
            resources: vec![],
            managed_runtime: None,
        };
        pocket_core::kernel::Process::map_into(
            image,
            &mut cpu,
            &|_, _| None,
            &pocket_core::kernel::NullDispatcher,
        )
        .unwrap()
    }
    #[test]
    fn throttled_frames_never_read_gpu_and_due_read_failures_stop_without_stale_pixels() {
        let (_tx, rx) = std::sync::mpsc::channel();
        let state = Arc::new(SessionState::new());
        let mut hook = SessionHook::new(state.clone(), rx);
        let mut process = process();
        let kernel = &mut process.state;
        kernel.framebuffer.resize(3, 2);
        let source = Arc::new(Image {
            reads: AtomicUsize::new(0),
            fail: AtomicBool::new(true),
        });
        kernel.framebuffer.publish_gpu(source.clone()).unwrap();
        hook.last_emit_at = Some(Instant::now() + Duration::from_secs(60));
        assert!(matches!(hook.on_frame(kernel), FrameAction::Continue));
        assert_eq!(source.reads.load(Ordering::Relaxed), 0);
        assert!(!kernel.framebuffer.cpu_resident());
        hook.last_emit_at = None;
        assert!(matches!(hook.on_frame(kernel), FrameAction::Stop));
        assert!(kernel.should_stop);
        assert_eq!(source.reads.load(Ordering::Relaxed), 1);
        assert!(!kernel.framebuffer.cpu_resident());
        kernel.should_stop = false;
        source.fail.store(false, Ordering::Relaxed);
        assert!(matches!(hook.on_frame(kernel), FrameAction::Continue));
        assert_eq!(source.reads.load(Ordering::Relaxed), 2);
        assert!(kernel.framebuffer.cpu_resident());
        assert_eq!(
            &state.latest_frame.lock().unwrap().as_ref().unwrap().rgba[..4],
            &[255, 0, 0, 255]
        );
    }
}
}