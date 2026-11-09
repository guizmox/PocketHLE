//! Bounded native snapshots. No GL operation runs from a frame token's Drop.
use super::*;
/// Exact integer conversion for packed top-down CPU uploads.
pub const RGB565_DISPLAY_FRAGMENT: &str = include_str!("shaders/rgb565_display.frag");
/// Pre-filter conversion from native GL rows to top-down display pixels.
pub const SHARED_DISPLAY_FRAGMENT: &str = include_str!("shaders/shared_display.frag");
use std::sync::{Arc, Mutex};

#[derive(Default, Debug)]
pub(crate) struct ShareGroup {
    retired: Mutex<Vec<Retired>>,
}
#[derive(Debug)]
struct Retired {
    texture: u32,
    ready: usize,
    done: usize,
}
type Fence = unsafe extern "system" fn(u32, u32) -> *mut c_void;
type Wait = unsafe extern "system" fn(*mut c_void, u32, u64);
type Delete = unsafe extern "system" fn(*mut c_void);
#[derive(Clone, Copy)]
pub(super) struct SyncApi {
    fence: Fence,
    wait: Wait,
    delete: Delete,
}
impl SyncApi {
    pub(super) unsafe fn load(load: impl Fn(&CStr) -> *mut c_void) -> Result<Self, String> {
        let get = load(c"glGetString");
        if get.is_null() {
            return Err("native GL version unavailable".into());
        }
        let get: unsafe extern "system" fn(u32) -> *const c_char = std::mem::transmute(get);
        let version = get(0x1f02);
        if version.is_null() {
            return Err("native GL version unavailable".into());
        }
        let version = CStr::from_ptr(version).to_string_lossy();
        let es = version.starts_with("OpenGL ES");
        let number = version
            .split_whitespace()
            .find(|s| s.starts_with(|c: char| c.is_ascii_digit()))
            .unwrap_or("");
        let mut numbers = number.split('.').filter_map(|s| s.parse::<u32>().ok());
        let (major, minor) = (numbers.next().unwrap_or(0), numbers.next().unwrap_or(0));
        let core_sync = if es {
            major >= 3
        } else {
            major > 3 || (major == 3 && minor >= 2)
        };
        if !core_sync {
            let mut arb_sync = false;
            if !es && major < 3 {
                let extensions = get(0x1f03);
                arb_sync = !extensions.is_null()
                    && CStr::from_ptr(extensions)
                        .to_string_lossy()
                        .split_whitespace()
                        .any(|s| s == "GL_ARB_sync");
            } else if !es {
                let get_i = load(c"glGetStringi");
                let get_n = load(c"glGetIntegerv");
                if !get_i.is_null() && !get_n.is_null() {
                    let get_i: unsafe extern "system" fn(u32, u32) -> *const c_char =
                        std::mem::transmute(get_i);
                    let get_n: unsafe extern "system" fn(u32, *mut i32) =
                        std::mem::transmute(get_n);
                    let mut count = 0;
                    get_n(0x821d, &mut count);
                    for i in 0..count.max(0) as u32 {
                        let extension = get_i(0x1f03, i);
                        if !extension.is_null()
                            && CStr::from_ptr(extension).to_bytes() == b"GL_ARB_sync"
                        {
                            arb_sync = true;
                            break;
                        }
                    }
                }
            }
            if !arb_sync {
                return Err("shared frames require GL 3.2/ARB_sync or GLES3".into());
            }
        }
        let fence = load(c"glFenceSync");
        let wait = load(c"glWaitSync");
        let delete = load(c"glDeleteSync");
        if fence.is_null() || wait.is_null() || delete.is_null() {
            return Err("shared frames require native GL sync".into());
        }
        Ok(Self {
            fence: std::mem::transmute::<*mut c_void, Fence>(fence),
            wait: std::mem::transmute::<*mut c_void, Wait>(wait),
            delete: std::mem::transmute::<*mut c_void, Delete>(delete),
        })
    }
    unsafe fn publish(&self, flush: unsafe extern "system" fn()) -> usize {
        let fence = (self.fence)(0x9117, 0) as usize;
        flush();
        fence
    }
    pub(super) unsafe fn wait(&self, fence: usize) {
        if fence != 0 {
            (self.wait)(fence as *mut c_void, 0, u64::MAX);
        }
    }
    unsafe fn delete(&self, fence: usize) {
        if fence != 0 {
            (self.delete)(fence as *mut c_void);
        }
    }
}
impl ShareGroup {
    unsafe fn collect(&self, load: impl Fn(&CStr) -> *mut c_void) -> Result<(), String> {
        if self
            .retired
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
        {
            return Ok(());
        }
        let sync = SyncApi::load(&load)?;
        let delete = load(c"glDeleteTextures");
        if delete.is_null() {
            return Err("shared texture deletion unavailable".into());
        }
        let delete: unsafe extern "system" fn(i32, *const u32) = std::mem::transmute(delete);
        let retired = std::mem::take(&mut *self.retired.lock().unwrap_or_else(|e| e.into_inner()));
        for r in retired {
            // Deletion itself is ordered/deferred by GL for already submitted
            // commands. Reuse requires a wait; retirement never reuses a name.
            sync.delete(r.ready);
            sync.delete(r.done);
            delete(1, &r.texture);
        }
        Ok(())
    }
}

#[derive(Default, Debug)]
struct SlotSync {
    ready: usize,
    done: usize,
    poisoned: bool,
}
#[derive(Debug)]
struct Slot {
    texture: u32,
    width: u32,
    height: u32,
    rgb565: bool,
    group: Arc<ShareGroup>,
    sync: Mutex<SlotSync>,
}
impl Drop for Slot {
    fn drop(&mut self) {
        let s = self.sync.get_mut().unwrap_or_else(|e| e.into_inner());
        self.group
            .retired
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Retired {
                texture: self.texture,
                ready: s.ready,
                done: s.done,
            });
    }
}

/// Immutable raw native snapshot. Coordinates are GL bottom-up; consumer must
/// respect its source format, row order and display brightness.
/// Clones retain the slot; dropping an unpainted frame returns it without any GL
/// work. The producer cannot overwrite a retained frame, including after resize.
#[derive(Clone, Debug)]
pub struct SharedFrame(Arc<Slot>);
impl SharedFrame {
    pub(super) fn read_source(
        &self,
        group: &Arc<ShareGroup>,
    ) -> Result<(u32, usize, bool), String> {
        if !Arc::ptr_eq(group, &self.0.group) {
            return Err("readback uses a different share group".into());
        }
        let sync = self.0.sync.lock().unwrap_or_else(|e| e.into_inner());
        if sync.poisoned || sync.ready == 0 {
            return Err("snapshot synchronization unavailable".into());
        }
        Ok((self.0.texture, sync.ready, self.0.rgb565))
    }
    pub(super) fn same_snapshot(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub fn belongs_to_source(&self, source: &SharedContextSource) -> bool {
        Arc::ptr_eq(&self.0.group, &source.1)
    }
    pub fn is_rgb565(&self) -> bool {
        self.0.rgb565
    }
    pub fn size(&self) -> (u32, u32) {
        (self.0.width, self.0.height)
    }

    /// Sample through a current consumer in the original live share group.
    /// A completion fence is recorded even if the drawing closure unwinds.
    ///
    /// # Safety
    /// Do not change current context inside `draw`, retain the texture name,
    /// delete or modify its contents, or use it after the closure returns.
    pub unsafe fn with_texture<R>(
        &self,
        ui: &mut ExternalGpuContext,
        draw: impl FnOnce(u32) -> R,
    ) -> Result<R, String> {
        self.with_consumer(&mut ui.consumer(), draw)
    }
    /// Consume through sendable UI metadata while its native owner stays on UI.
    ///
    /// # Safety
    /// As with `with_texture`, keep the context current, do not retain the
    /// texture name, and do not delete or modify the texture inside `draw`.
    pub unsafe fn with_consumer<R>(
        &self,
        ui: &mut SharedConsumer,
        draw: impl FnOnce(u32) -> R,
    ) -> Result<R, String> {
        if !Arc::ptr_eq(&self.0.group, &ui.source().1) || !ui.is_current() {
            return Err(
                "shared frame belongs to a different or closed UI context generation".into(),
            );
        }
        ui.collect_retired_frames()?;
        let api = SyncApi::load(|n| ui.load(n))?;
        let flush = ui.load(c"glFlush");
        if flush.is_null() {
            return Err("shared frame flush unavailable".into());
        }
        let flush: unsafe extern "system" fn() = std::mem::transmute(flush);
        let mut s = self.0.sync.lock().unwrap_or_else(|e| e.into_inner());
        if s.poisoned {
            return Err("shared frame synchronization failed".into());
        }
        api.wait(s.ready);
        struct Release<'a> {
            s: &'a mut SlotSync,
            api: SyncApi,
            flush: unsafe extern "system" fn(),
        }
        impl Drop for Release<'_> {
            fn drop(&mut self) {
                unsafe {
                    let done = self.api.publish(self.flush);
                    self.api.delete(self.s.done);
                    self.s.done = done;
                    self.s.poisoned |= done == 0;
                }
            }
        }
        let release = Release {
            s: &mut s,
            api,
            flush,
        };
        let result = draw(self.0.texture);
        drop(release);
        if s.poisoned {
            Err("shared frame completion fence failed".into())
        } else {
            Ok(result)
        }
    }
}
impl ExternalGpuContext {
    /// Call on the current UI GL thread after replacing/dropping GPU snapshots.
    /// Never deletes objects from an unrelated/recreated context generation.
    pub fn collect_retired_frames(&mut self) -> Result<(), String> {
        if !self.is_current() {
            return Err("UI context is not current".into());
        }
        unsafe { self.source().1.collect(|n| self.load(n)) }
    }
}

impl SharedConsumer {
    /// Call on the current UI GL thread after replacing/dropping GPU snapshots.
    /// Never deletes objects from an unrelated/recreated context generation.
    pub fn collect_retired_frames(&mut self) -> Result<(), String> {
        if !self.is_current() {
            return Err("UI context is not current".into());
        }
        unsafe { self.source().1.collect(|n| self.load(n)) }
    }
}

// Desktop has one kernel image, one mailbox image, and up to two UI images
// during receive/replace. Reserve one more slot to capture their successor.
// Bounded mailbox transport is essential: capacity alone cannot bound a FIFO.
const SNAPSHOT_CAPACITY: usize = 5;
pub(super) struct FramePool {
    sync: SyncApi,
    copy: unsafe extern "system" fn(u32, i32, i32, i32, i32, i32, i32, i32),
    slots: Vec<Arc<Slot>>,
    pressure_events: u64,
}
impl FramePool {
    unsafe fn new(host: &Host) -> Result<Self, String> {
        let sync = SyncApi::load(|n| host.load(n))?;
        let copy = host.load(c"glCopyTexSubImage2D");
        if copy.is_null() {
            return Err("native texture copy unavailable".into());
        }
        Ok(Self {
            sync,
            copy: std::mem::transmute::<
                *mut c_void,
                unsafe extern "system" fn(u32, i32, i32, i32, i32, i32, i32, i32),
            >(copy),
            slots: Vec::new(),
            pressure_events: 0,
        })
    }
    unsafe fn capture(&mut self, r: &mut Renderer) -> Result<Option<SharedFrame>, String> {
        let group = r.host.shared_source().1;
        group.collect(|n| r.host.load(n))?;
        let index = self.slots.iter().position(|s| Arc::strong_count(s) == 1);
        let index = match index {
            Some(i) => i,
            None if self.slots.len() < SNAPSHOT_CAPACITY => {
                let mut texture = 0;
                (r.gl.GenTextures)(1, &mut texture);
                if texture == 0 {
                    return Err("shared snapshot allocation failed".into());
                }
                self.slots.push(Arc::new(Slot {
                    texture,
                    width: 0,
                    height: 0,
                    rgb565: false,
                    group,
                    sync: Mutex::new(SlotSync::default()),
                }));
                self.slots.len() - 1
            }
            None => {
                // Temporary diagnosis: distinguish lease pressure from a driver
                // failure without a per-frame trace or any extra GL query.
                self.pressure_events = self.pressure_events.saturating_add(1);
                if self.pressure_events == 1 || self.pressure_events % 64 == 0 {
                    let leases: Vec<_> = self.slots.iter().map(Arc::strong_count).collect();
                    log::info!(
                        "SHARED_EXPORT=CPU reason=snapshot_pool_full slots={} lease_counts={leases:?} pressure_events={}",
                        self.slots.len(), self.pressure_events
                    );
                }
                return Ok(None);
            }
        };
        let slot = Arc::get_mut(&mut self.slots[index]).unwrap();
        let format_changed = slot.rgb565 != r.rgb565;
        slot.rgb565 = r.rgb565;
        let s = slot.sync.get_mut().unwrap_or_else(|e| e.into_inner());
        if s.poisoned {
            return Err("shared snapshot slot is poisoned".into());
        }
        self.sync.wait(s.done);
        self.sync.delete(s.done);
        self.sync.delete(s.ready);
        s.done = 0;
        s.ready = 0;
        (r.gl.BindFramebuffer)(0x8d40, r.fbo);
        (r.gl.ActiveTexture)(GL_TEXTURE0);
        (r.gl.BindTexture)(GL_TEXTURE_2D, slot.texture);
        if format_changed || (slot.width, slot.height) != (r.width, r.height) {
            (r.gl.TexImage2D)(
                GL_TEXTURE_2D,
                0,
                // GLES copies require compatible source/destination formats.
                // In particular ANGLE rejects RGB565 -> RGBA8 snapshots.
                if r.rgb565 { 0x8d62 } else { GL_RGBA as i32 },
                r.width as i32,
                r.height as i32,
                0,
                if r.rgb565 { GL_RGB } else { GL_RGBA },
                if r.rgb565 {
                    GL_UNSIGNED_SHORT_5_6_5
                } else {
                    GL_UNSIGNED_BYTE
                },
                std::ptr::null(),
            );
            r.params(
                Filter::Nearest,
                Filter::Nearest,
                Wrap::ClampToEdge,
                Wrap::ClampToEdge,
            );
            slot.width = r.width;
            slot.height = r.height;
        }
        (self.copy)(
            GL_TEXTURE_2D,
            0,
            0,
            0,
            0,
            0,
            r.width as i32,
            r.height as i32,
        );
        s.ready = self.sync.publish(r.gl.Flush);
        let result = r.check();
        s.poisoned = result.is_err() || s.ready == 0;
        result?;
        if s.poisoned {
            return Err("shared snapshot ready fence failed".into());
        }
        Ok(Some(SharedFrame(self.slots[index].clone())))
    }
}
impl Renderer {
    pub(crate) fn copy_guest_rgb_texture(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<Option<SharedFrame>, String> {
        let Some(source) = configured_shared_context_source() else {
            return Ok(None);
        };
        let group = self.host.shared_source().1;
        if !Arc::ptr_eq(&source.1, &group)
            || !self.rgb565
            || x < 0
            || y < 0
            || (x as u32).saturating_add(width) > self.width / self.render_scale
            || (y as u32).saturating_add(height) > self.height / self.render_scale
        {
            return Ok(None);
        }
        self.host.make_current()?;
        unsafe {
            let Ok(sync) = SyncApi::load(|n| self.host.load(n)) else {
                return Ok(None);
            };
            self.check()?;
            group.collect(|n| self.host.load(n))?;
            let mut texture = 0;
            (self.gl.GenTextures)(1, &mut texture);
            let slot = Arc::new(Slot {
                texture,
                width,
                height,
                rgb565: true,
                group,
                sync: Mutex::new(SlotSync::default()),
            });
            // GLES rejects an RGBA copy from the alpha-less RGB565 target.
            // Keep the immutable guest-copy chain RGBA8 (including subimage
            // patches/imports) using the existing GPU resolve at native size.
            // Supersampled copies already use this same conversion/resolve.
            let fbo = if self.render_scale > 1 || self.host.es {
                if !self.resolve_logical_image(
                    self.width / self.render_scale,
                    self.height / self.render_scale,
                    false,
                ) {
                    return Ok(None);
                }
                self.logical_resolve.as_ref().unwrap().fbo
            } else {
                self.fbo
            };
            (self.gl.BindFramebuffer)(0x8d40, fbo);
            (self.gl.ActiveTexture)(GL_TEXTURE0);
            (self.gl.BindTexture)(GL_TEXTURE_2D, texture);
            self.params(
                Filter::Nearest,
                Filter::Nearest,
                Wrap::ClampToEdge,
                Wrap::ClampToEdge,
            );
            (self.gl.CopyTexImage2D)(
                GL_TEXTURE_2D,
                0,
                GL_RGBA,
                x,
                y,
                width as i32,
                height as i32,
                0,
            );
            (self.gl.BindFramebuffer)(0x8d40, self.fbo);
            self.check()?;
            let ready = sync.publish(self.gl.Flush);
            if ready == 0 {
                return Err("guest texture copy fence failed".into());
            }
            slot.sync.lock().unwrap_or_else(|e| e.into_inner()).ready = ready;
            Ok(Some(SharedFrame(slot)))
        }
    }

    pub(crate) fn patch_guest_rgb_texture(
        &mut self,
        old: &Texture,
        source: &SharedFrame,
        x: i32,
        y: i32,
    ) -> Result<Option<SharedFrame>, String> {
        let Some(copy) = self.gl.CopyTexSubImage2D else {
            return Ok(None);
        };
        let group = self.host.shared_source().1;
        let Ok((source_texture, source_ready, _)) = source.read_source(&group) else {
            return Ok(None);
        };
        if old.gpu_copy.is_none() && old.rgba.len() != old.width as usize * old.height as usize * 4
        {
            return Ok(None);
        }
        self.host.make_current()?;
        unsafe {
            let sync = SyncApi::load(|n| self.host.load(n))?;
            let mut texture = 0;
            (self.gl.GenTextures)(1, &mut texture);
            let slot = Arc::new(Slot {
                texture,
                width: old.width,
                height: old.height,
                rgb565: true,
                group: group.clone(),
                sync: Mutex::new(SlotSync::default()),
            });
            (self.gl.ActiveTexture)(GL_TEXTURE0);
            (self.gl.BindTexture)(GL_TEXTURE_2D, texture);
            self.params(
                Filter::Nearest,
                Filter::Nearest,
                Wrap::ClampToEdge,
                Wrap::ClampToEdge,
            );
            let mut fbo = 0;
            (self.gl.GenFramebuffers)(1, &mut fbo);
            let result = (|| {
                (self.gl.BindFramebuffer)(0x8d40, fbo);
                if let Some(frame) = &old.gpu_copy {
                    let (previous, ready, _) = frame.read_source(&group)?;
                    sync.wait(ready);
                    (self.gl.FramebufferTexture2D)(0x8d40, 0x8ce0, GL_TEXTURE_2D, previous, 0);
                    if (self.gl.CheckFramebufferStatus)(0x8d40) != 0x8cd5 {
                        return Err("texture patch source framebuffer incomplete".into());
                    }
                    (self.gl.CopyTexImage2D)(
                        GL_TEXTURE_2D,
                        0,
                        GL_RGBA,
                        0,
                        0,
                        old.width as i32,
                        old.height as i32,
                        0,
                    );
                } else {
                    (self.gl.PixelStorei)(0x0cf5, 1);
                    (self.gl.TexImage2D)(
                        GL_TEXTURE_2D,
                        0,
                        GL_RGBA as i32,
                        old.width as i32,
                        old.height as i32,
                        0,
                        GL_RGBA,
                        GL_UNSIGNED_BYTE,
                        old.rgba.as_ptr().cast(),
                    );
                }
                sync.wait(source_ready);
                (self.gl.FramebufferTexture2D)(0x8d40, 0x8ce0, GL_TEXTURE_2D, source_texture, 0);
                if (self.gl.CheckFramebufferStatus)(0x8d40) != 0x8cd5 {
                    return Err("texture patch framebuffer incomplete".into());
                }
                let (width, height) = source.size();
                copy(GL_TEXTURE_2D, 0, x, y, 0, 0, width as i32, height as i32);
                self.check()
            })();
            (self.gl.BindFramebuffer)(0x8d40, self.fbo);
            (self.gl.DeleteFramebuffers)(1, &fbo);
            result?;
            let ready = sync.publish(self.gl.Flush);
            if ready == 0 {
                return Err("texture patch fence failed".into());
            }
            slot.sync.lock().unwrap_or_else(|e| e.into_inner()).ready = ready;
            Ok(Some(SharedFrame(slot)))
        }
    }

    pub(super) unsafe fn import_guest_rgb_texture(
        &self,
        frame: &SharedFrame,
    ) -> Result<bool, String> {
        let Ok((texture, ready, _)) = frame.read_source(&self.host.shared_source().1) else {
            return Ok(false);
        };
        let sync = SyncApi::load(|n| self.host.load(n))?;
        sync.wait(ready);
        let mut fbo = 0;
        (self.gl.GenFramebuffers)(1, &mut fbo);
        (self.gl.BindFramebuffer)(0x8d40, fbo);
        (self.gl.FramebufferTexture2D)(0x8d40, 0x8ce0, GL_TEXTURE_2D, texture, 0);
        let complete = (self.gl.CheckFramebufferStatus)(0x8d40) == 0x8cd5;
        if complete {
            let (width, height) = frame.size();
            (self.gl.CopyTexImage2D)(
                GL_TEXTURE_2D,
                0,
                GL_RGBA,
                0,
                0,
                width as i32,
                height as i32,
                0,
            );
        }
        (self.gl.BindFramebuffer)(0x8d40, self.fbo);
        (self.gl.DeleteFramebuffers)(1, &fbo);
        self.check()?;
        Ok(complete)
    }

    /// Snapshot without readback. Returns None when all bounded pipeline slots are leased.
    /// This alone does not make a window swap visible or update guest memory.
    pub fn snapshot_shared_frame(&mut self) -> Result<Option<SharedFrame>, String> {
        let _diagnostic = crate::graphics_diagnostic::Span::new(
            crate::graphics_diagnostic::Stage::SharedSnapshot,
        );
        self.host.make_current()?;
        let mut pool = match self.shared_frames.take() {
            Some(p) => p,
            None => unsafe { FramePool::new(&self.host)? },
        };
        let result = unsafe { pool.capture(self) };
        self.shared_frames = Some(pool);
        self.pipeline = None;
        result
    }
    /// Optional frontend export; a private fallback renderer cannot publish
    /// names into the UI's unrelated share group.
    pub fn snapshot_configured_shared_frame(&mut self) -> Result<Option<SharedFrame>, String> {
        // Deferred draw failures are fatal renderer failures, not an optional
        // transport failure. Check before pool pressure or disabled export can
        // take the CPU fallback and before snapshot code consumes the error.
        self.poll_error()?;
        if self.shared_export_failed {
            return Ok(None);
        }
        let Some(source) = SHARED_CONTEXT_SOURCE.with(|s| s.borrow().clone()) else {
            return Ok(None);
        };
        if !Arc::ptr_eq(&source.1, &self.host.shared_source().1) {
            return Ok(None);
        }
        match self.snapshot_shared_frame() {
            Ok(frame) => Ok(frame),
            Err(error) => {
                self.shared_export_failed = true;
                log::warn!("Shared frame export unavailable; retaining CPU presentation: {error}");
                Ok(None)
            }
        }
    }
    pub(super) fn retire_shared_frames(&mut self) {
        self.shared_frames.take();
        unsafe {
            let _ = self.host.shared_source().1.collect(|n| self.host.load(n));
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{sync::mpsc, time::Duration};

    unsafe fn read_sample(
        ui: &Renderer,
        external: &mut ExternalGpuContext,
        frame: &SharedFrame,
    ) -> ([u8; 4], u32) {
        frame
            .with_texture(external, |texture| {
                (ui.gl.BindFramebuffer)(0x8d40, ui.fbo);
                (ui.gl.FramebufferTexture2D)(0x8d40, 0x8ce0, GL_TEXTURE_2D, texture, 0);
                (ui.gl.FramebufferRenderbuffer)(0x8d40, 0x8d00, 0x8d41, 0);
                assert_eq!((ui.gl.CheckFramebufferStatus)(0x8d40), 0x8cd5);
                let mut pixel = [0; 4];
                // Consumer-side read is a pixel oracle, never the transport.
                (ui.gl.ReadPixels)(
                    0,
                    0,
                    1,
                    1,
                    GL_RGBA,
                    GL_UNSIGNED_BYTE,
                    pixel.as_mut_ptr().cast(),
                );
                (ui.gl.FramebufferTexture2D)(0x8d40, 0x8ce0, GL_TEXTURE_2D, ui.color, 0);
                ui.check().unwrap();
                (pixel, texture)
            })
            .unwrap()
    }

    #[test]
    #[ignore = "requires native shared GL/GLES"]
    fn guest_rgb_copy_supplies_alpha_before_gles_rgba_copy() {
        for es in [false, true] {
            for scale in [1, 2] {
                let mut gpu = if es {
                    Renderer::new_es_for_test(8, 8).unwrap()
                } else {
                    Renderer::new(8, 8).unwrap()
                };
                gpu.set_surface_rgb565(true).unwrap();
                gpu.set_render_scale(if scale == 1 { 1 } else { 2 })
                    .unwrap();
                let state = PipelineState {
                    viewport: (0, 0, 8, 8),
                    ..Default::default()
                };
                gpu.clear(8, 8, &state, [1., 0., 0., 1.], 1., GL_COLOR_BUFFER_BIT)
                    .unwrap();
                let source = gpu.shared_context_source();
                let frame = with_shared_context_source(Some(source.clone()), || {
                    gpu.copy_guest_rgb_texture(1, 1, 4, 4).unwrap().unwrap()
                });
                assert_eq!(frame.size(), (4, 4));
                assert!(
                    gpu.readback.is_empty(),
                    "the production copy must remain on GPU"
                );
                // Pixel oracle only: the production copy above does no readback.
                let mut reader = SharedReadback::new(&source).unwrap();
                assert_eq!(
                    reader.read_guest_rgb_texture(&frame).unwrap(),
                    [255, 0, 0, 255].repeat(16),
                    "es={es} scale={scale}"
                );
            }
        }
    }

    #[test]
    #[ignore = "requires shared native OpenGL/GLES3"]
    fn rgb565_snapshot_preserves_source_color_format() {
        for es in [false, true] {
            let ui = if es {
                Renderer::new_es_for_test(8, 8).unwrap()
            } else {
                Renderer::new(8, 8).unwrap()
            };
            let mut external = unsafe { ExternalGpuContext::capture_current().unwrap() };
            let source = external.source();
            let mut producer = Renderer::new_shared(&source, 8, 8).unwrap();
            producer.set_surface_rgb565(true).unwrap();
            let state = PipelineState {
                viewport: (0, 0, 8, 8),
                ..Default::default()
            };
            producer
                .clear(8, 8, &state, [1., 0., 0., 1.], 1., GL_COLOR_BUFFER_BIT)
                .unwrap();
            let frame = producer.snapshot_shared_frame().unwrap().unwrap();
            ui.host.make_current().unwrap();
            unsafe {
                frame
                    .with_texture(&mut external, |texture| {
                        (ui.gl.BindFramebuffer)(0x8d40, ui.fbo);
                        (ui.gl.FramebufferTexture2D)(0x8d40, 0x8ce0, GL_TEXTURE_2D, texture, 0);
                        let query: unsafe extern "system" fn(u32, u32, u32, *mut i32) =
                            std::mem::transmute(
                                ui.host.load(c"glGetFramebufferAttachmentParameteriv"),
                            );
                        let mut bits = [0; 4];
                        for (i, value) in bits.iter_mut().enumerate() {
                            query(0x8d40, 0x8ce0, 0x8212 + i as u32, value);
                        }
                        assert_eq!(bits, [5, 6, 5, 0], "snapshot must match RGB565 source");
                        (ui.gl.FramebufferTexture2D)(0x8d40, 0x8ce0, GL_TEXTURE_2D, ui.color, 0);
                        ui.check().unwrap();
                    })
                    .unwrap();
            }
            assert_eq!(
                unsafe { read_sample(&ui, &mut external, &frame) }.0,
                [255, 0, 0, 255]
            );
        }
    }

    #[test]
    #[ignore = "requires native shared OpenGL/GLES3"]
    fn deferred_draw_error_is_not_hidden_by_shared_export_or_pool_pressure() {
        use std::cell::Cell;
        thread_local! { static ERROR: Cell<u32> = const { Cell::new(0) }; }
        unsafe extern "system" fn error_probe() -> u32 {
            ERROR.with(|e| e.replace(0))
        }
        for es in [false, true] {
            let ui = if es {
                Renderer::new_es_for_test(8, 8).unwrap()
            } else {
                Renderer::new(8, 8).unwrap()
            };
            let source = ui.shared_context_source();
            with_shared_context_source(Some(source.clone()), || {
                let mut gpu = Renderer::new_shared(&source, 8, 8).unwrap();
                let original = gpu.gl.GetError;
                gpu.gl.GetError = error_probe;
                ERROR.with(|e| e.set(GL_OUT_OF_MEMORY));
                assert!(gpu.snapshot_configured_shared_frame().is_err());
                assert!(!gpu.shared_export_failed);
                assert!(
                    gpu.shared_frames.is_none(),
                    "failed batch must not allocate/copy a snapshot"
                );
                gpu.gl.GetError = original;
                let retained: Vec<_> = (0..SNAPSHOT_CAPACITY)
                    .map(|_| gpu.snapshot_configured_shared_frame().unwrap().unwrap())
                    .collect();
                assert!(gpu.snapshot_configured_shared_frame().unwrap().is_none());
                gpu.gl.GetError = error_probe;
                ERROR.with(|e| e.set(GL_INVALID_OPERATION));
                assert!(
                    gpu.snapshot_configured_shared_frame().is_err(),
                    "pool pressure must not swallow draw errors"
                );
                assert!(!gpu.shared_export_failed);
                gpu.gl.GetError = original;
                drop(retained);
                assert!(gpu.snapshot_configured_shared_frame().unwrap().is_some());
            });
        }
    }

    #[test]
    #[ignore = "requires shared EGL desktop GL/GLES and native sync"]
    fn bounded_latest_pipeline_keeps_capture_headroom_without_cpu_readback() {
        for es in [false, true] {
            let ui = if es {
                Renderer::new_es_for_test(8, 8).unwrap()
            } else {
                Renderer::new(8, 8).unwrap()
            };
            let mut external = unsafe { ExternalGpuContext::capture_current().unwrap() };
            let source = external.source();
            std::thread::scope(|scope| {
                let (tx, rx) = mpsc::sync_channel(1);
                let worker = scope.spawn(move || {
                    let mut producer = Renderer::new_shared(&source, 8, 8).unwrap();
                    let state = PipelineState {
                        viewport: (0, 0, 8, 8),
                        ..Default::default()
                    };
                    producer
                        .clear(8, 8, &state, [1., 0., 0., 1.], 1., GL_COLOR_BUFFER_BIT)
                        .unwrap();
                    let retained_ui = producer.snapshot_shared_frame().unwrap().unwrap();
                    let mut in_flight: Vec<_> = (0..3)
                        .map(|_| producer.snapshot_shared_frame().unwrap().unwrap())
                        .collect();
                    for _ in 0..1000 {
                        producer
                            .clear(8, 8, &state, [0., 1., 0., 1.], 1., GL_COLOR_BUFFER_BIT)
                            .unwrap();
                        let next = producer.snapshot_shared_frame().unwrap().expect(
                            "bounded UI/mailbox/kernel pipeline must leave capture headroom",
                        );
                        // Replace, rather than accumulate an unbounded history.
                        in_flight.rotate_left(1);
                        in_flight[2] = next;
                    }
                    assert!(producer.readback.is_empty());
                    assert_eq!(
                        producer.shared_frames.as_ref().unwrap().slots.len(),
                        SNAPSHOT_CAPACITY
                    );
                    tx.send((retained_ui, in_flight.pop().unwrap())).unwrap();
                });
                let (old, latest) = rx.recv_timeout(Duration::from_secs(10)).unwrap();
                worker.join().unwrap();
                assert_eq!(
                    unsafe { read_sample(&ui, &mut external, &old) }.0,
                    [255, 0, 0, 255]
                );
                assert_eq!(
                    unsafe { read_sample(&ui, &mut external, &latest) }.0,
                    [0, 255, 0, 255]
                );
            });
        }
    }

    #[test]
    #[ignore = "requires shared EGL desktop GL/GLES and native sync"]
    fn retained_frames_backpressure_resize_abandon_and_outlive_producer() {
        for es in [false, true] {
            let ui = if es {
                Renderer::new_es_for_test(8, 8).unwrap()
            } else {
                Renderer::new(8, 8).unwrap()
            };
            let mut external = unsafe { ExternalGpuContext::capture_current().unwrap() };
            let source = external.source();
            std::thread::scope(|scope| {
                let (send, frames) = mpsc::sync_channel(1);
                let (command, commands) = mpsc::sync_channel(1);
                let worker = scope.spawn(move || {
                    let mut producer = Renderer::new_shared(&source, 8, 8).unwrap();
                    let mut retained = Vec::new();
                    for color in [[1., 0., 0., 1.], [0., 1., 0., 1.], [0., 0., 1., 1.]] {
                        let state = PipelineState {
                            viewport: (0, 0, 8, 8),
                            ..Default::default()
                        };
                        producer
                            .clear(8, 8, &state, color, 1., GL_COLOR_BUFFER_BIT)
                            .unwrap();
                        retained.push(producer.snapshot_shared_frame().unwrap().unwrap());
                    }
                    // Keep the extra in-flight pipeline slots occupied too.
                    let pipeline: Vec<_> = (3..SNAPSHOT_CAPACITY)
                        .map(|_| producer.snapshot_shared_frame().unwrap().unwrap())
                        .collect();
                    assert!(producer.snapshot_shared_frame().unwrap().is_none());
                    assert_eq!(
                        producer.shared_frames.as_ref().unwrap().slots.len(),
                        SNAPSHOT_CAPACITY
                    );
                    send.send(retained).unwrap();
                    commands.recv_timeout(Duration::from_secs(10)).unwrap();
                    // The consumer abandons exactly one unpainted slot. Other
                    // retained snapshots must stay intact across this resize.
                    let state = PipelineState {
                        viewport: (0, 0, 6, 10),
                        ..Default::default()
                    };
                    producer
                        .clear(6, 10, &state, [1., 1., 0., 1.], 1., GL_COLOR_BUFFER_BIT)
                        .unwrap();
                    let replacement = producer.snapshot_shared_frame().unwrap().unwrap();
                    assert!(producer.snapshot_shared_frame().unwrap().is_none());
                    assert!(producer.readback.is_empty());
                    drop(pipeline);
                    send.send(vec![replacement]).unwrap();
                    commands.recv_timeout(Duration::from_secs(10)).unwrap();
                    assert!(producer.readback.is_empty());
                });
                let mut initial = frames.recv_timeout(Duration::from_secs(10)).unwrap();
                let blue = initial.pop().unwrap();
                let abandoned = initial.pop().unwrap();
                let red = initial.pop().unwrap();
                let red_clone = red.clone();
                drop(red);
                drop(abandoned);
                command.send(()).unwrap();
                let yellow = frames
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap()
                    .pop()
                    .unwrap();
                assert_eq!(red_clone.size(), (8, 8));
                assert_eq!(yellow.size(), (6, 10));
                let (red_pixel, red_name) = unsafe { read_sample(&ui, &mut external, &red_clone) };
                let (blue_pixel, blue_name) = unsafe { read_sample(&ui, &mut external, &blue) };
                assert_eq!(red_pixel, [255, 0, 0, 255]);
                assert_eq!(blue_pixel, [0, 0, 255, 255]);
                let mut wrong_generation =
                    unsafe { ExternalGpuContext::capture_current().unwrap() };
                assert!(unsafe {
                    yellow.with_texture(&mut wrong_generation, |_| {
                        panic!("wrong generation sampled a texture")
                    })
                }
                .is_err());
                drop(wrong_generation);
                command.send(()).unwrap();
                worker.join().unwrap();
                // The worker and its pool/context are gone. UI share-group
                // ownership and frame leases still preserve the three images.
                let (yellow_pixel, yellow_name) =
                    unsafe { read_sample(&ui, &mut external, &yellow) };
                assert_eq!(yellow_pixel, [255, 255, 0, 255]);
                assert_eq!(
                    unsafe { read_sample(&ui, &mut external, &blue) }.0,
                    [0, 0, 255, 255]
                );
                assert!(external.source().1.retired.lock().unwrap().is_empty());
                drop(red_clone);
                drop(blue);
                std::thread::spawn(move || drop(yellow)).join().unwrap();
                assert_eq!(external.source().1.retired.lock().unwrap().len(), 3);
                external.collect_retired_frames().unwrap();
                assert!(external.source().1.retired.lock().unwrap().is_empty());
                unsafe {
                    let is_texture: unsafe extern "system" fn(u32) -> u8 =
                        std::mem::transmute(ui.host.load(c"glIsTexture"));
                    for name in [red_name, blue_name, yellow_name] {
                        assert_eq!(is_texture(name), 0, "retired shared texture leaked");
                    }
                    ui.check().unwrap();
                }
            });
        }
    }
}
