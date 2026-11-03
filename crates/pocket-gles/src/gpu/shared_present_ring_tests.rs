//! Native transport prototype, not an enabled application presentation path.
//! Textures and GLsync handles cross threads only inside one live share group.
use super::*;
use std::{sync::mpsc, time::Duration};

struct SyncApi {
    fence: unsafe extern "system" fn(u32, u32) -> *mut c_void,
    wait: unsafe extern "system" fn(*mut c_void, u32, u64),
    delete: unsafe extern "system" fn(*mut c_void),
}
impl SyncApi {
    unsafe fn load(host: &Host) -> Self {
        let fence = host.load(c"glFenceSync");
        let wait = host.load(c"glWaitSync");
        let delete = host.load(c"glDeleteSync");
        assert!(!fence.is_null() && !wait.is_null() && !delete.is_null());
        Self {
            fence: std::mem::transmute::<
                *mut c_void,
                unsafe extern "system" fn(u32, u32) -> *mut c_void,
            >(fence),
            wait: std::mem::transmute::<
                *mut c_void,
                unsafe extern "system" fn(*mut c_void, u32, u64),
            >(wait),
            delete: std::mem::transmute::<*mut c_void, unsafe extern "system" fn(*mut c_void)>(
                delete,
            ),
        }
    }
    unsafe fn publish(&self, gl: &Gl) -> usize {
        let fence = (self.fence)(0x9117, 0); // GL_SYNC_GPU_COMMANDS_COMPLETE
        assert!(!fence.is_null());
        // glWaitSync on another context cannot flush this producer's queue.
        (gl.Flush)();
        fence as usize
    }
    unsafe fn acquire(&self, fence: usize) {
        assert_ne!(fence, 0);
        (self.wait)(fence as *mut c_void, 0, u64::MAX); // GL_TIMEOUT_IGNORED
        (self.delete)(fence as *mut c_void);
    }
}

#[derive(Debug)]
struct Frame {
    slot: usize,
    texture: u32,
    ready: usize,
    index: usize,
    width: u32,
    height: u32,
}

fn colors(index: usize) -> ([f32; 4], [u8; 4]) {
    // Non-endpoint values catch accidental RGB565 conversion/raw-color loss.
    let rgb = [
        ((index * 37 + 11) % 256) as u8,
        ((index * 71 + 29) % 256) as u8,
        ((index * 13 + 53) % 256) as u8,
        255,
    ];
    (rgb.map(|v| v as f32 / 255.), rgb)
}

#[test]
#[ignore = "requires desktop GL/GLES3 shared contexts and cross-context sync"]
fn bounded_shared_frames_survive_overlap_resize_and_slow_consumer() {
    const SLOTS: usize = 3;
    const FRAMES: usize = 15;
    for es in [false, true] {
        let host = if es {
            Host::es_for_test().unwrap()
        } else {
            Host::new().unwrap()
        };
        let source = host.shared_source();
        let ui = Renderer::with_host(host, 320, 240).unwrap();
        let sync = unsafe { SyncApi::load(&ui.host) };
        std::thread::scope(|scope| {
            let (frames, incoming) = mpsc::sync_channel(SLOTS);
            let (release, released) = mpsc::sync_channel::<(usize, usize)>(SLOTS);
            let (primed, primed_rx) = mpsc::sync_channel(1);
            let worker = scope.spawn(move || {
                let mut producer = Renderer::new_shared(&source, 320, 240).unwrap();
                let sync = unsafe { SyncApi::load(&producer.host) };
                let mut textures = [0; SLOTS];
                let mut busy = [false; SLOTS];
                unsafe {
                    (producer.gl.GenTextures)(SLOTS as i32, textures.as_mut_ptr());
                }
                assert!(textures.iter().all(|id| *id != 0));
                for index in 0..FRAMES {
                    let slot = index % SLOTS;
                    if busy[slot] {
                        let (returned, fence) =
                            released.recv_timeout(Duration::from_secs(10)).unwrap();
                        assert_eq!(returned, slot, "producer reused an outstanding frame");
                        unsafe {
                            sync.acquire(fence);
                        }
                        busy[slot] = false;
                    }
                    let (width, height) = if index < 7 { (320, 240) } else { (240, 320) };
                    let state = PipelineState {
                        viewport: (0, 0, width as i32, height as i32),
                        ..Default::default()
                    };
                    producer
                        .clear(
                            width,
                            height,
                            &state,
                            colors(index).0,
                            1.,
                            GL_COLOR_BUFFER_BIT,
                        )
                        .unwrap();
                    let mut upper = state;
                    upper.scissor =
                        Some((0, (height / 2) as i32, width as i32, (height / 2) as i32));
                    producer
                        .clear(
                            width,
                            height,
                            &upper,
                            colors(index + 19).0,
                            1.,
                            GL_COLOR_BUFFER_BIT,
                        )
                        .unwrap();
                    unsafe {
                        (producer.gl.BindFramebuffer)(0x8d40, producer.fbo);
                        (producer.gl.ActiveTexture)(GL_TEXTURE0);
                        (producer.gl.BindTexture)(GL_TEXTURE_2D, textures[slot]);
                        // Immutable snapshot of this frame, separate from the
                        // attachment the next guest frame will overwrite.
                        (producer.gl.CopyTexImage2D)(
                            GL_TEXTURE_2D,
                            0,
                            GL_RGBA,
                            0,
                            0,
                            width as i32,
                            height as i32,
                            0,
                        );
                        producer.params(
                            Filter::Nearest,
                            Filter::Nearest,
                            Wrap::ClampToEdge,
                            Wrap::ClampToEdge,
                        );
                        let ready = sync.publish(&producer.gl);
                        producer.check().unwrap();
                        frames
                            .send(Frame {
                                slot,
                                texture: textures[slot],
                                ready,
                                index,
                                width,
                                height,
                            })
                            .unwrap();
                    }
                    busy[slot] = true;
                    assert!(
                        producer.readback.is_empty(),
                        "producer downloaded frame pixels"
                    );
                    if index == SLOTS - 1 {
                        // The consumer cannot start before all three distinct
                        // images exist: this makes overwrite bugs deterministic.
                        primed.send(()).unwrap();
                    }
                }
                for _ in 0..SLOTS {
                    let (slot, fence) = released.recv_timeout(Duration::from_secs(10)).unwrap();
                    assert!(busy[slot]);
                    unsafe {
                        sync.acquire(fence);
                    }
                    busy[slot] = false;
                }
                assert!(busy.iter().all(|b| !b));
                assert!(producer.readback.is_empty());
                unsafe {
                    (producer.gl.BindTexture)(GL_TEXTURE_2D, 0);
                    (producer.gl.DeleteTextures)(SLOTS as i32, textures.as_ptr());
                }
                unsafe {
                    producer.check().unwrap();
                }
            });
            primed_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            for index in 0..FRAMES {
                let frame = incoming.recv_timeout(Duration::from_secs(10)).unwrap();
                assert_eq!(frame.index, index);
                ui.host.make_current().unwrap();
                unsafe {
                    sync.acquire(frame.ready);
                    (ui.gl.BindFramebuffer)(0x8d40, ui.fbo);
                    (ui.gl.FramebufferTexture2D)(0x8d40, 0x8ce0, GL_TEXTURE_2D, frame.texture, 0);
                    // The consumer's old depth attachment may have a different
                    // size after rotation; it is irrelevant for this read oracle.
                    (ui.gl.FramebufferRenderbuffer)(0x8d40, 0x8d00, 0x8d41, 0);
                    assert_eq!((ui.gl.CheckFramebufferStatus)(0x8d40), 0x8cd5);
                    let mut pixels = vec![0; (frame.width * frame.height * 4) as usize];
                    (ui.gl.PixelStorei)(0x0d05, 1);
                    // Only the consumer reads, as a validation oracle. The
                    // transport contains IDs, fences and dimensions, no pixels.
                    (ui.gl.ReadPixels)(
                        0,
                        0,
                        frame.width as i32,
                        frame.height as i32,
                        GL_RGBA,
                        GL_UNSIGNED_BYTE,
                        pixels.as_mut_ptr().cast(),
                    );
                    assert_eq!(
                        &pixels[..4],
                        &colors(index).1,
                        "es={es} frame={index} bottom"
                    );
                    assert_eq!(
                        &pixels[pixels.len() - 4..],
                        &colors(index + 19).1,
                        "es={es} frame={index} top"
                    );
                    (ui.gl.FramebufferTexture2D)(0x8d40, 0x8ce0, GL_TEXTURE_2D, ui.color, 0);
                    let done = sync.publish(&ui.gl);
                    ui.check().unwrap();
                    release.send((frame.slot, done)).unwrap();
                }
            }
            worker.join().unwrap();
        });
        ui.host.make_current().unwrap();
        unsafe {
            (ui.gl.BindFramebuffer)(0x8d40, ui.fbo);
            (ui.gl.FramebufferRenderbuffer)(0x8d40, 0x8d00, 0x8d41, ui.depth);
        }
        unsafe {
            ui.check().unwrap();
        }
    }
}

#[test]
#[ignore = "requires native EGL desktop GL and GLES"]
fn closed_shared_sources_reject_creation_but_children_keep_the_group() {
    for es in [false, true] {
        let root = if es {
            Renderer::new_es_for_test(8, 8).unwrap()
        } else {
            Renderer::new(8, 8).unwrap()
        };
        let source = root.shared_context_source();
        std::thread::scope(|scope| {
            let (ready, incoming) = mpsc::sync_channel(1);
            let (finish, done) = mpsc::sync_channel(1);
            let worker = scope.spawn({
                let source = source.clone();
                move || {
                    let child = Renderer::new_shared(&source, 8, 8).unwrap();
                    ready.send(child.shared_context_source()).unwrap();
                    done.recv_timeout(Duration::from_secs(10)).unwrap();
                    child.host.make_current().unwrap();
                    unsafe {
                        child.check().unwrap();
                    }
                }
            });
            let child_source = incoming.recv_timeout(Duration::from_secs(10)).unwrap();
            drop(root);
            assert!(Renderer::new_shared(&source, 8, 8).is_err());
            let mut survivor = Renderer::new_shared(&child_source, 8, 8).unwrap();
            finish.send(()).unwrap();
            worker.join().unwrap();
            assert!(Renderer::new_shared(&child_source, 8, 8).is_err());
            let state = PipelineState {
                viewport: (0, 0, 8, 8),
                ..Default::default()
            };
            survivor
                .clear(8, 8, &state, [1., 0., 0., 1.], 1., GL_COLOR_BUFFER_BIT)
                .unwrap();
            let mut target = RenderTarget::new(8, 8);
            survivor.read(&mut target).unwrap();
            assert_eq!(&target.color[..4], &[255, 0, 0, 255]);
        });
    }
}
