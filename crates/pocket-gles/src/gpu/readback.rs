//! Explicit, lazy CPU materialization of the immutable swap snapshot.
use super::*;
use std::sync::Arc;

/// Thread-confined readback context. Reading a retained frame never switches the
/// guest's canonical render target or samples its possibly newer color image.
/// The source UI/display must outlive this reader, as for any shared renderer.
pub struct SharedReadback {
    host: Host,
    gl: Gl,
    sync: shared::SyncApi,
    fbo: u32,
    group: Arc<shared::ShareGroup>,
    cached: Option<SharedFrame>,
    native: Vec<u8>,
}
impl SharedReadback {
    pub fn new(source: &SharedContextSource) -> Result<Self, String> {
        let mut previous = platform::SavedCurrent::capture()?;
        let host = Host::shared(source)?;
        let gl = unsafe { Gl::load(&host)? };
        let sync = unsafe { shared::SyncApi::load(|n| host.load(n))? };
        let mut fbo = 0;
        unsafe {
            (gl.GenFramebuffers)(1, &mut fbo);
        }
        if fbo == 0 {
            return Err("snapshot readback framebuffer allocation failed".into());
        }
        let reader = Self {
            host,
            gl,
            sync,
            fbo,
            group: source.1.clone(),
            cached: None,
            native: Vec::new(),
        };
        previous.restore()?;
        Ok(reader)
    }
    fn ensure_native(&mut self, frame: &SharedFrame) -> Result<(), String> {
        frame.read_source(&self.group)?;
        if self.cached.as_ref().is_some_and(|f| f.same_snapshot(frame)) {
            return Ok(());
        }
        // Invalidate before overwriting the cache, including on a failed read.
        self.cached = None;
        let mut previous = platform::SavedCurrent::capture()?;
        let result = (|| {
            self.host.bind_client_api()?;
            self.host.make_current()?;
            let (texture, ready, _) = frame.read_source(&self.group)?;
            let (w, h) = frame.size();
            let bytes = (w as usize)
                .checked_mul(h as usize)
                .and_then(|n| n.checked_mul(4))
                .ok_or("snapshot byte size overflow")?;
            if bytes > self.native.len() {
                self.native
                    .try_reserve_exact(bytes - self.native.len())
                    .map_err(|e| e.to_string())?;
            }
            self.native.resize(bytes, 0);
            unsafe {
                self.sync.wait(ready);
                (self.gl.BindFramebuffer)(0x8d40, self.fbo);
                (self.gl.FramebufferTexture2D)(0x8d40, 0x8ce0, GL_TEXTURE_2D, texture, 0);
                if (self.gl.CheckFramebufferStatus)(0x8d40) != 0x8cd5 {
                    return Err("snapshot readback framebuffer incomplete".into());
                }
                (self.gl.PixelStorei)(0x0d05, 1);
                (self.gl.ReadPixels)(
                    0,
                    0,
                    w as i32,
                    h as i32,
                    GL_RGBA,
                    GL_UNSIGNED_BYTE,
                    self.native.as_mut_ptr().cast(),
                );
                let error = (self.gl.GetError)();
                if error != 0 {
                    return Err(format!("snapshot readback error 0x{error:04x}"));
                }
            }
            Ok(())
        })();
        let restored = previous.restore();
        result?;
        restored?;
        self.cached = Some(frame.clone());
        Ok(())
    }
    pub(crate) fn read_guest_rgb_texture(
        &mut self,
        frame: &SharedFrame,
    ) -> Result<Vec<u8>, String> {
        self.ensure_native(frame)?;
        let mut out = self.native.clone();
        for pixel in out.chunks_exact_mut(4) {
            pixel.copy_from_slice(&crate::raster::rgb565_pixel([
                pixel[0], pixel[1], pixel[2], 255,
            ]));
        }
        Ok(out)
    }

    /// Release the retained slot once the caller owns its materialized bytes.
    pub fn clear_cache(&mut self) {
        self.cached = None;
    }
    pub fn matches_source(&self, source: &SharedContextSource) -> bool {
        Arc::ptr_eq(&self.group, &source.1)
    }
    /// Top-down opaque RGBA at existing display precision, before brightness.
    pub fn read_display_rgba_into(
        &mut self,
        frame: &SharedFrame,
        out: &mut Vec<u8>,
    ) -> Result<(), String> {
        self.ensure_native(frame)?;
        let (w, _) = frame.size();
        let len = self.native.len();
        if len > out.len() {
            out.try_reserve_exact(len - out.len())
                .map_err(|e| e.to_string())?;
        }
        out.resize(len, 0);
        let stride = w as usize * 4;
        for (src, dst) in self
            .native
            .chunks_exact(stride)
            .rev()
            .zip(out.chunks_exact_mut(stride))
        {
            for (p, q) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
                let r = p[0] >> 3;
                let g = p[1] >> 2;
                let b = p[2] >> 3;
                q.copy_from_slice(&[
                    (r << 3) | (r >> 2),
                    (g << 2) | (g >> 4),
                    (b << 3) | (b >> 2),
                    255,
                ]);
            }
        }
        Ok(())
    }
    /// Produce the existing logical top-down RGB565 bytes. Scaled RGB565
    /// surfaces retain the old box-average and rounding before final packing.
    /// `out` is unchanged on failure. Brightness is host presentation only.
    pub fn read_rgb565_into(
        &mut self,
        frame: &SharedFrame,
        logical: (u32, u32),
        out: &mut Vec<u8>,
    ) -> Result<(), String> {
        let (w, h) = frame.size();
        let (lw, lh) = logical;
        if lw == 0
            || lh == 0
            || w % lw != 0
            || h % lh != 0
            || w / lw != h / lh
            || !(1..=4).contains(&(w / lw))
        {
            return Err("snapshot logical size does not match native scale".into());
        }
        let (_, _, rgb565) = frame.read_source(&self.group)?;
        let len = (lw as usize)
            .checked_mul(lh as usize)
            .and_then(|n| n.checked_mul(2))
            .ok_or("logical snapshot size overflow")?;
        // Reserve before any output mutation; read failures preserve old bytes.
        if len > out.len() {
            out.try_reserve_exact(len - out.len())
                .map_err(|e| e.to_string())?;
        }
        self.ensure_native(frame)?;
        out.resize(len, 0);
        let k = (w / lw) as usize;
        let count = (k * k) as u32;
        for y in 0..lh as usize {
            for x in 0..lw as usize {
                let mut sum = [0u32; 3];
                for dy in 0..k {
                    for dx in 0..k {
                        let p = ((h as usize - 1 - (y * k + dy)) * w as usize + x * k + dx) * 4;
                        for c in 0..3 {
                            sum[c] += self.native[p + c] as u32;
                        }
                    }
                }
                let mut p = [
                    (sum[0] / count) as u8,
                    (sum[1] / count) as u8,
                    (sum[2] / count) as u8,
                    255,
                ];
                if k > 1 && rgb565 {
                    p = crate::raster::rgb565_pixel(p);
                }
                let value =
                    ((p[0] as u16 >> 3) << 11) | ((p[1] as u16 >> 2) << 5) | (p[2] as u16 >> 3);
                let at = (y * lw as usize + x) * 2;
                out[at..at + 2].copy_from_slice(&value.to_le_bytes());
            }
        }
        Ok(())
    }
}
impl Drop for SharedReadback {
    fn drop(&mut self) {
        let Ok(mut previous) = platform::SavedCurrent::capture() else {
            return;
        };
        if self.host.bind_client_api().is_ok() && self.host.make_current().is_ok() {
            unsafe {
                (self.gl.DeleteFramebuffers)(1, &self.fbo);
            }
        }
        self.cached.take();
        if let Err(error) = previous.restore() {
            log::warn!("Snapshot reader teardown: {error}");
        }
        // Host::drop deletes its inactive context without unbinding the restored
        // UI/guest context. Cached frame retirement is collected by its group.
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires native desktop GL/GLES shared contexts"]
    fn snapshot_materialization_preserves_old_image_cache_format_and_native_binding() {
        for es in [false, true] {
            for rgb565 in [false, true] {
                for (w, h, policy) in [(8, 6, 1), (8, 6, 2), (320, 240, 4), (240, 320, 5)] {
                    let ui = if es {
                        Renderer::new_es_for_test(8, 8).unwrap()
                    } else {
                        Renderer::new(8, 8).unwrap()
                    };
                    let source = ui.shared_context_source();
                    let mut producer = Renderer::new_shared(&source, w, h).unwrap();
                    producer.set_surface_rgb565(rgb565).unwrap();
                    producer.set_render_scale(policy).unwrap();
                    let data: Vec<u8> = (0..producer.width * producer.height * 4)
                        .map(|i| i.wrapping_mul(73).wrapping_add(i / 11).wrapping_add(7) as u8)
                        .collect();
                    unsafe {
                        (producer.gl.BindTexture)(GL_TEXTURE_2D, producer.color);
                        (producer.gl.TexImage2D)(
                            GL_TEXTURE_2D,
                            0,
                            GL_RGBA as i32,
                            producer.width as i32,
                            producer.height as i32,
                            0,
                            GL_RGBA,
                            GL_UNSIGNED_BYTE,
                            data.as_ptr().cast(),
                        );
                    }
                    let frame = producer.snapshot_shared_frame().unwrap().unwrap();
                    let mut target = if rgb565 {
                        RenderTarget::with_format(w, h, true)
                    } else {
                        RenderTarget::new(w, h)
                    };
                    let mut expected = vec![0; (w * h * 2) as usize];
                    producer.read_present(&mut target, &mut expected).unwrap();
                    let state = PipelineState {
                        viewport: (0, 0, w as i32, h as i32),
                        ..Default::default()
                    };
                    producer
                        .clear(w, h, &state, [0., 1., 0., 1.], 1., GL_COLOR_BUFFER_BIT)
                        .unwrap();
                    let later = producer.snapshot_shared_frame().unwrap().unwrap();
                    drop(producer); // Both snapshots survive independently of the producer.
                    let other = if es {
                        Renderer::new(8, 8).unwrap()
                    } else {
                        Renderer::new_es_for_test(8, 8).unwrap()
                    };
                    let mut owner = unsafe { ExternalGpuContext::capture_current().unwrap() };
                    let consumer = owner.consumer();
                    assert!(consumer.is_current());
                    let mut reader = SharedReadback::new(&source).unwrap();
                    assert!(
                        consumer.is_current(),
                        "reader creation changed caller binding"
                    );
                    let mut out = vec![77; 7];
                    reader.read_rgb565_into(&frame, (w, h), &mut out).unwrap();
                    assert_eq!(
                        out, expected,
                        "es={es} rgb565={rgb565} {w}x{h} policy={policy}"
                    );
                    assert!(consumer.is_current());
                    let fbo = reader.fbo;
                    reader.fbo = 0;
                    // A cache hit must not touch an unusable read FBO.
                    reader
                        .read_rgb565_into(&frame.clone(), (w, h), &mut out)
                        .unwrap();
                    assert_eq!(out, expected);
                    let before = out.clone();
                    assert!(reader.read_rgb565_into(&later, (w, h), &mut out).is_err());
                    assert_eq!(out, before);
                    assert!(consumer.is_current());
                    reader.fbo = fbo;
                    // A failed read must not leave the old snapshot label on new bytes.
                    reader.read_rgb565_into(&frame, (w, h), &mut out).unwrap();
                    assert_eq!(out, expected);
                    let before = out.clone();
                    assert!(reader.read_rgb565_into(&frame, (0, h), &mut out).is_err());
                    assert_eq!(out, before);
                    let mut wrong = SharedReadback::new(&other.shared_context_source()).unwrap();
                    assert!(wrong.read_rgb565_into(&frame, (w, h), &mut out).is_err());
                    assert_eq!(out, before);
                    drop(wrong);
                    assert!(consumer.is_current());
                    reader.read_rgb565_into(&later, (w, h), &mut out).unwrap();
                    assert!(out.chunks_exact(2).all(|p| p == [0xe0, 7]));
                    drop(reader);
                    assert!(
                        consumer.is_current(),
                        "reader destructor unbound the caller"
                    );
                    owner.close();
                    drop(owner);
                    drop(other);
                    ui.host.bind_client_api().unwrap();
                    ui.host.make_current().unwrap();
                }
            }
        }
    }
}
