//! Software framebuffer that backs both GDI (`BitBlt`, `FillRect`,
//! ...) and GAPI (`GXBeginDraw`/`GXEndDraw`) rendering.
//!
//! The Pocket PC reference target is a 240×320 portrait LCD with a
//! 16-bit-per-pixel RGB565 framebuffer. We keep that as the canonical
//! pixel format because:
//!
//! * GAPI is documented to expose RGB565 on the vast majority of
//!   Pocket PC devices, including the JumpyBall test ROM.
//! * GDI bitmaps created via `CreateCompatibleBitmap` are also 16 bpp
//!   on a 16 bpp screen, so memory-DC-to-screen `BitBlt` becomes a
//!   straight memcpy.
//!
//! All host-facing operations work on top of [`Framebuffer::pixels`],
//! a row-major byte buffer. `width * 2` is the row stride.

/// Width of the emulated Pocket PC LCD.
pub const FB_WIDTH: u32 = 240;
/// Height of the emulated Pocket PC LCD.
pub const FB_HEIGHT: u32 = 320;
/// Bits per pixel of the canonical framebuffer (RGB565).
pub const FB_BPP: u32 = 16;
/// Total framebuffer size in bytes.
pub const FB_BYTES: u32 = FB_WIDTH * FB_HEIGHT * (FB_BPP / 8);

/// A single opaque DirectDraw overlay composited over the primary surface.
#[derive(Debug, Clone, Copy)]
pub struct DirectDrawOverlay {
    pub surface: u32,
    pub source: (i64, i64, i64, i64),
    pub destination: (i64, i64, i64, i64),
}

/// Modeled LCD clock shared by DirectDraw waits and primary presentations.
#[derive(Debug, Clone)]
pub struct DirectDrawTiming {
    pub epoch: std::time::Instant,
    pub last_present: Option<std::time::Instant>,
    /// Optional user ceiling, separate from the modeled LCD/vblank clock.
    /// None = native LCD timing; Some(0) = unlimited; other values = FPS cap.
    pub frame_rate_limit: Option<u32>,
    pub pending: std::collections::HashMap<(usize, u32, u32), std::time::Instant>,
    pub recent_blank: std::collections::HashMap<usize, std::time::Instant>,
    pub paced_primary_locks: std::collections::HashSet<u32>,
    pub primary_locks: std::collections::HashSet<u32>,
    pub overlay: Option<DirectDrawOverlay>,
    pub surface_dcs: std::collections::HashMap<u32, (u32, u32)>,
}
impl Default for DirectDrawTiming {
    fn default() -> Self {
        Self {
            epoch: std::time::Instant::now(),
            last_present: None,
            frame_rate_limit: None,
            pending: Default::default(),
            recent_blank: Default::default(),
            paced_primary_locks: Default::default(),
            primary_locks: Default::default(),
            overlay: None,
            surface_dcs: Default::default(),
        }
    }
}
impl DirectDrawTiming {
    // This is the emulated display policy, not a measured hardware frequency.
    pub const PERIOD: std::time::Duration = std::time::Duration::from_nanos(16_666_667);
    pub const BLANK: std::time::Duration = std::time::Duration::from_millis(1);
    pub fn blank_deadline(&self, now: std::time::Instant, end: bool) -> std::time::Instant {
        let elapsed = now.saturating_duration_since(self.epoch).as_nanos();
        let period = Self::PERIOD.as_nanos();
        let cycle = elapsed / period;
        let offset = if end { Self::BLANK.as_nanos() } else { 0 };
        let target = cycle * period + offset;
        let target = if target > elapsed {
            target
        } else {
            target + period
        };
        self.epoch + std::time::Duration::from_nanos(target as u64)
    }
    /// An optional cap does not change vblank timing, manufacture ticks, or catch up.
    pub fn frame_limit_deadline(&self, now: std::time::Instant) -> std::time::Instant {
        let fps = self
            .frame_rate_limit
            .filter(|fps| matches!(fps, 10 | 15 | 20 | 30 | 45 | 60 | 120));
        match (fps, self.last_present) {
            (Some(fps), Some(last)) => {
                let nanos = 1_000_000_000u64.div_ceil(u64::from(fps));
                (last + std::time::Duration::from_nanos(nanos)).max(now)
            }
            _ => now,
        }
    }
    pub fn present_deadline(&self, now: std::time::Instant) -> std::time::Instant {
        if self.frame_rate_limit == Some(0) {
            return now; // Unlimited bypasses implicit pacing, not explicit vblank waits.
        }
        if self.frame_rate_limit == Some(120) {
            // An explicit high-rate cap overrides implicit 60 Hz presentation
            // pacing, while blank_deadline/in_blank retain the modeled LCD.
            return self.frame_limit_deadline(now);
        }
        self.last_present
            .map_or(now, |last| (last + Self::PERIOD).max(now))
            .max(self.frame_limit_deadline(now))
    }
    pub fn in_blank(&self, now: std::time::Instant) -> bool {
        now.saturating_duration_since(self.epoch).as_nanos() % Self::PERIOD.as_nanos()
            < Self::BLANK.as_nanos()
    }
}

/// Presentation telemetry shared with frontends independently of pixel uploads.
/// Only completed, explicit display boundaries are FPS. Without one, callers
/// retain their changed-image rate (IPS), rather than guessing GDI frames.
#[derive(Debug, Clone, Default)]
pub struct PresentationStats {
    presents: std::sync::Arc<std::sync::Mutex<PresentationRate>>,
    changes: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<std::time::Instant>>>,
}

#[derive(Debug, Default)]
struct PresentationRate {
    completed: u64,
    started: Option<std::time::Instant>,
    last: Option<std::time::Instant>,
    frames: u64,
    fps: f32,
}

impl PresentationStats {
    pub fn record_present(&self) {
        self.record_at(std::time::Instant::now());
    }

    fn record_at(&self, now: std::time::Instant) {
        let Ok(mut rate) = self.presents.lock() else {
            return;
        };
        rate.completed = rate.completed.wrapping_add(1);
        if rate.last.is_some_and(|last| {
            now.saturating_duration_since(last) >= std::time::Duration::from_secs(1)
        }) {
            rate.started = None;
            rate.frames = 0;
            rate.fps = 0.0;
        }
        rate.last = Some(now);
        if let Some(start) = rate.started {
            rate.frames += 1;
            let elapsed = now.saturating_duration_since(start);
            if elapsed >= std::time::Duration::from_secs(1) {
                rate.fps = rate.frames as f32 / elapsed.as_secs_f32();
                rate.frames = 0;
                rate.started = Some(now);
            }
        } else {
            rate.started = Some(now);
        }
    }

    pub fn fps(&self) -> Option<f32> {
        self.fps_at(std::time::Instant::now())
    }

    fn fps_at(&self, now: std::time::Instant) -> Option<f32> {
        let rate = self.presents.lock().ok()?;
        let last = rate.last?;
        Some(
            if now.saturating_duration_since(last) >= std::time::Duration::from_secs(1) {
                0.0
            } else {
                rate.fps
            },
        )
    }

    pub fn record_image_update(&self) {
        self.record_image_at(std::time::Instant::now());
    }

    fn record_image_at(&self, now: std::time::Instant) {
        let Ok(mut changes) = self.changes.lock() else {
            return;
        };
        while changes.front().is_some_and(|last| {
            now.saturating_duration_since(*last) >= std::time::Duration::from_secs(1)
        }) {
            changes.pop_front();
        }
        changes.push_back(now);
    }

    pub fn ips(&self) -> Option<f32> {
        self.ips_at(std::time::Instant::now())
    }

    fn ips_at(&self, now: std::time::Instant) -> Option<f32> {
        let changes = self.changes.lock().ok()?;
        changes.back()?;
        Some(
            changes
                .iter()
                .rev()
                .take_while(|last| {
                    now.saturating_duration_since(**last) < std::time::Duration::from_secs(1)
                })
                .count() as f32,
        )
    }

    pub fn completed(&self) -> u64 {
        self.presents.lock().map_or(0, |rate| rate.completed)
    }

    pub fn reset(&self) {
        if let Ok(mut rate) = self.presents.lock() {
            *rate = PresentationRate::default();
        }
        if let Ok(mut changes) = self.changes.lock() {
            changes.clear();
        }
    }
}

/// Optional host presentation image; never mapped into guest address space.
#[derive(Debug, Clone)]
struct RenderPresentation {
    rgba: bool,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

/// Presentation metadata contains no pixels and never triggers a GPU readback.
#[derive(Debug, Clone)]
pub struct FramebufferMetadata {
    pub width: u32,
    pub height: u32,
    pub bpp: u32,
    pub frame_counter: u64,
    pub presentation_stats: PresentationStats,
    pub directdraw: DirectDrawTiming,
}
/// An eager RGB565 image. Pixel storage is separate from presentation metadata.
#[derive(Debug, Clone)]
pub struct Framebuffer {
    metadata: FramebufferMetadata,
    pub pixels: Vec<u8>,
    backlight_level: u32,
    render_presentation: Option<RenderPresentation>,
}
impl std::ops::Deref for Framebuffer {
    type Target = FramebufferMetadata;
    fn deref(&self) -> &Self::Target {
        &self.metadata
    }
}
impl std::ops::DerefMut for Framebuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.metadata
    }
}

impl Default for Framebuffer {
    fn default() -> Self {
        Self::new(FB_WIDTH, FB_HEIGHT)
    }
}

impl Framebuffer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            metadata: FramebufferMetadata {
                width,
                height,
                bpp: FB_BPP,
                frame_counter: 0,
                presentation_stats: PresentationStats::default(),
                directdraw: DirectDrawTiming::default(),
            },
            pixels: vec![0u8; (width * height * 2) as usize],
            backlight_level: 11,
            render_presentation: None,
        }
    }

    pub fn set_backlight_level(&mut self, level: u32) {
        // Zero/uninitialised and malformed registry values must stay readable.
        let level = if (1..=31).contains(&level) { level } else { 11 };
        if self.backlight_level != level {
            self.backlight_level = level;
            // Publish even when a menu's native pixels have not changed.
            self.frame_counter = self.frame_counter.wrapping_add(1);
        }
    }
    pub fn backlight_level(&self) -> u32 {
        self.backlight_level
    }
    fn backlight_channel(&self, channel: u8) -> u8 {
        let c = channel as u32;
        if self.backlight_level <= 11 {
            // The minimum blends one fifth toward black: more distinct than
            // the previous one-twelfth step, without the old 50% dimming.
            (c - c * (11 - self.backlight_level) / 50) as u8
        } else {
            // Colors' four positions are 1/11/21/31: the second is neutral.
            // Level 4 now uses the previous level 3 (one twelfth white).
            // Level 3 is halfway from neutral to that ceiling (one twenty-fourth).
            (c + (255 - c) * (self.backlight_level - 11) / 240) as u8
        }
    }

    /// Re-point the emulated panel at a new geometry.
    ///
    /// Pocket PC games that want to run in landscape rotate the
    /// display (`ChangeDisplaySettingsEx` with `DM_DISPLAYORIENTATION`,
    /// or simply a landscape `SDL_SetVideoMode`) and then present a
    /// surface with the rotated dimensions. Keeping the framebuffer
    /// pinned to 240x320 meant every one of those blits was clipped to
    /// the portrait rectangle, so the visible frame was a corner crop
    /// of the real image. Resizing keeps the emulated panel the same
    /// shape as what the guest actually draws.
    ///
    /// The contents are discarded (a real panel is not expected to
    /// retain pixels across a mode switch) and `frame_counter` keeps
    /// counting so hosts notice the change.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 || (self.width == width && self.height == height) {
            return;
        }
        self.width = width;
        self.height = height;
        self.pixels = vec![0u8; (width * height * (self.bpp / 8)) as usize];
        self.mark_dirty();
    }

    pub fn byte_size(&self) -> u32 {
        self.width * self.height * (self.bpp / 8)
    }

    pub fn stride_bytes(&self) -> u32 {
        self.width * (self.bpp / 8)
    }

    pub fn mark_dirty(&mut self) {
        self.render_presentation = None;
        self.frame_counter = self.frame_counter.wrapping_add(1);
    }

    pub fn clear_render_presentation(&mut self) {
        self.render_presentation = None;
    }

    /// Reuse the large host buffer across EGL presentations.
    pub fn take_render_pixels(&mut self) -> Vec<u8> {
        self.render_presentation
            .take()
            .map_or_else(Vec::new, |image| image.pixels)
    }

    pub fn set_render_presentation(&mut self, width: u32, height: u32, pixels: Vec<u8>) {
        self.set_render_image(width, height, pixels, false);
    }

    /// Host-only RGBA at source precision; never guest mapped.
    pub fn set_render_presentation_rgba(&mut self, width: u32, height: u32, pixels: Vec<u8>) {
        self.set_render_image(width, height, pixels, true);
    }

    fn set_render_image(&mut self, width: u32, height: u32, pixels: Vec<u8>, rgba: bool) {
        let valid = width > 0
            && height > 0
            && self.width > 0
            && self.height > 0
            && width % self.width == 0
            && height % self.height == 0
            && width / self.width >= if rgba { 1 } else { 2 }
            && width / self.width == height / self.height
            && (width as usize)
                .checked_mul(height as usize)
                .and_then(|n| n.checked_mul(if rgba { 4 } else { 2 }))
                == Some(pixels.len());
        self.render_presentation = valid.then_some(RenderPresentation {
            rgba,
            width,
            height,
            pixels,
        });
    }

    /// Physical source image size, independent of guest/touch/display geometry.
    pub fn presentation_size(&self) -> (u32, u32) {
        self.render_presentation
            .as_ref()
            .map_or((self.width, self.height), |image| {
                (image.width, image.height)
            })
    }

    /// Native pixels when no alternate image or brightness conversion is needed.
    pub fn presentation_rgb565(&self) -> Option<&[u8]> {
        (self.backlight_level == 11)
            .then(|| self.presentation_rgb565_unadjusted())
            .flatten()
    }

    /// Packed little-endian logical pixels for a host that applies backlight
    /// itself. Alternate high-density images keep their established path.
    pub fn presentation_rgb565_unadjusted(&self) -> Option<&[u8]> {
        let bytes = (self.width as usize)
            .checked_mul(self.height as usize)?
            .checked_mul(2)?;
        (self.render_presentation.is_none() && self.bpp == 16 && bytes == self.pixels.len())
            .then_some(self.pixels.as_slice())
    }

    pub fn snapshot_presentation_rgba8888_into(&self, out: &mut Vec<u8>) {
        let Some(image) = self.render_presentation.as_ref() else {
            return self.snapshot_rgba8888_into(out);
        };
        if image.rgba {
            out.clear();
            out.extend_from_slice(&image.pixels);
            if self.backlight_level != 11 {
                for pixel in out.chunks_exact_mut(4) {
                    pixel[0] = self.backlight_channel(pixel[0]);
                    pixel[1] = self.backlight_channel(pixel[1]);
                    pixel[2] = self.backlight_channel(pixel[2]);
                }
            }
            return;
        }
        out.resize(image.width as usize * image.height as usize * 4, 0);
        for (source, dest) in image.pixels.chunks_exact(2).zip(out.chunks_exact_mut(4)) {
            let color = rgb565_to_rgba8888(u16::from_le_bytes([source[0], source[1]]));
            let [red, green, blue, alpha] = color.to_le_bytes();
            dest.copy_from_slice(&[
                self.backlight_channel(red),
                self.backlight_channel(green),
                self.backlight_channel(blue),
                alpha,
            ]);
        }
    }

    pub fn is_all_black(&self) -> bool {
        self.pixels.iter().all(|&pixel| pixel == 0)
    }

    /// Fill the entire framebuffer with one RGB565 value.
    pub fn fill(&mut self, color: u16) {
        // The framebuffer is canonically two-byte-aligned (we always
        // allocate `width * height * 2` bytes), so reinterpreting the
        // backing storage as `[u16]` is sound and lets the LLVM
        // backend compile the body down to a single SIMD store loop
        // instead of the per-byte path the safe `chunks_exact_mut`
        // version expanded to in release builds.
        let words: &mut [u16] = bytemuck::cast_slice_mut(&mut self.pixels);
        words.fill(color.to_le());
        self.mark_dirty();
    }

    /// Plot one pixel. Out-of-bounds is silently ignored to match
    /// GDI's clipping semantics.
    pub fn put_pixel(&mut self, x: i32, y: i32, color: u16) {
        if x < 0 || y < 0 || (x as u32) >= self.width || (y as u32) >= self.height {
            return;
        }
        let off = ((y as u32) * self.width + x as u32) as usize * 2;
        let bytes = color.to_le_bytes();
        self.pixels[off] = bytes[0];
        self.pixels[off + 1] = bytes[1];
        // Caller is expected to call mark_dirty() once per logical
        // operation; we don't bump it per pixel for performance.
    }

    /// Fill an axis-aligned rectangle. `(x, y, w, h)` is clipped to
    /// the framebuffer.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u16) {
        if w <= 0 || h <= 0 {
            return;
        }
        let (cx0, cy0, cx1, cy1) = clip_rect(x, y, w, h, self.width, self.height);
        if cx0 >= cx1 || cy0 >= cy1 {
            return;
        }
        // Treat the framebuffer as `[u16]` so each row collapses into
        // a single `slice::fill` (one SIMD store per 8/16 pixels) —
        // the previous per-byte loop showed up high in the perf
        // profile when games like Derby do many `FillRect` calls per
        // frame to clear the background.
        let stride = self.width as usize;
        let words: &mut [u16] = bytemuck::cast_slice_mut(&mut self.pixels);
        let row_len = (cx1 - cx0) as usize;
        let color_le = color.to_le();
        for row in cy0..cy1 {
            let off = row as usize * stride + cx0 as usize;
            words[off..off + row_len].fill(color_le);
        }
        self.mark_dirty();
    }

    /// Draw a one-pixel-thick rectangle outline. Used by `Rectangle`
    /// after the interior is already filled.
    pub fn stroke_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u16) {
        if w <= 0 || h <= 0 {
            return;
        }
        for i in 0..w {
            self.put_pixel(x + i, y, color);
            self.put_pixel(x + i, y + h - 1, color);
        }
        for i in 0..h {
            self.put_pixel(x, y + i, color);
            self.put_pixel(x + w - 1, y + i, color);
        }
        self.mark_dirty();
    }

    /// Copy a sub-region of `src` into `(dx, dy)`. Clipping is applied
    /// independently on both source and destination axes.
    #[allow(clippy::too_many_arguments)]
    pub fn blit_from(
        &mut self,
        dx: i32,
        dy: i32,
        sx: i32,
        sy: i32,
        w: i32,
        h: i32,
        src: &[u8],
        src_w: u32,
        src_h: u32,
    ) {
        if w <= 0 || h <= 0 || src_w == 0 || src_h == 0 {
            return;
        }
        let (sx0, sy0, sx1, sy1) = clip_rect(sx, sy, w, h, src_w, src_h);
        if sx0 >= sx1 || sy0 >= sy1 {
            return;
        }
        let dst_x0 = dx + (sx0 as i32 - sx);
        let dst_y0 = dy + (sy0 as i32 - sy);
        let copy_w = (sx1 - sx0) as i32;
        let copy_h = (sy1 - sy0) as i32;
        let (dx0, dy0, dx1, dy1) =
            clip_rect(dst_x0, dst_y0, copy_w, copy_h, self.width, self.height);
        if dx0 >= dx1 || dy0 >= dy1 {
            return;
        }
        let row_bytes = (dx1 - dx0) as usize * 2;
        let src_skip_x = (dx0 as i32 - dst_x0) as u32;
        let src_skip_y = (dy0 as i32 - dst_y0) as u32;
        let src_x0 = sx0 + src_skip_x;
        let src_y0 = sy0 + src_skip_y;
        for row in 0..(dy1 - dy0) {
            let dst_off = ((dy0 + row) * self.width + dx0) as usize * 2;
            let src_off = ((src_y0 + row) * src_w + src_x0) as usize * 2;
            if src_off + row_bytes > src.len() || dst_off + row_bytes > self.pixels.len() {
                continue;
            }
            self.pixels[dst_off..dst_off + row_bytes]
                .copy_from_slice(&src[src_off..src_off + row_bytes]);
        }
        self.mark_dirty();
    }

    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    /// Convert the framebuffer to a flat 0xRR,GG,BB,AA buffer for
    /// host-side display. Each pixel is decoded from RGB565 to
    /// 8-bit-per-channel and given full opacity.
    pub fn snapshot_rgba8888(&self) -> Vec<u8> {
        let mut out = vec![0u8; (self.width * self.height * 4) as usize];
        self.snapshot_rgba8888_into(&mut out);
        out
    }

    /// Same as [`Framebuffer::snapshot_rgba8888`] but writes into a
    /// caller-owned buffer so the host display loop can amortise the
    /// 300 KiB-per-frame allocation across the whole emulator
    /// session. `out` is resized to fit if it is too small.
    pub fn snapshot_rgba8888_into(&self, out: &mut Vec<u8>) {
        let needed = (self.width * self.height * 4) as usize;
        if out.len() != needed {
            out.resize(needed, 0);
        }
        // Reinterpret the destination as `[u32]` so each pixel write
        // is one store instead of four `Vec::push` calls. The source
        // is reinterpreted as `[u16]` so we skip the per-byte
        // `u16::from_le_bytes` decode for every pixel.
        let src: &[u16] = bytemuck::cast_slice(&self.pixels);
        let dst: &mut [u32] = bytemuck::cast_slice_mut(out.as_mut_slice());
        for (s, d) in src.iter().zip(dst.iter_mut()) {
            let color = rgb565_to_rgba8888(u16::from_le(*s));
            *d = if self.backlight_level == 11 {
                color
            } else {
                let [red, green, blue, alpha] = color.to_le_bytes();
                u32::from_le_bytes([
                    self.backlight_channel(red),
                    self.backlight_channel(green),
                    self.backlight_channel(blue),
                    alpha,
                ])
            };
        }
    }

    /// Pack the framebuffer as a P6 PPM image. P6 is binary, one
    /// triplet per pixel, no compression — perfect for headless
    /// proof-of-rendering captures without pulling in `png`.
    /// Display capture, including same-size RGBA or higher-density presentation.
    /// snapshot_ppm retains its existing logical RGB565 diagnostic contract.
    pub fn snapshot_presentation_ppm(&self) -> Vec<u8> {
        let (width, height) = self.presentation_size();
        let mut rgba = Vec::new();
        self.snapshot_presentation_rgba8888_into(&mut rgba);
        let mut out = format!("P6\n{width} {height}\n255\n").into_bytes();
        out.reserve(rgba.len() / 4 * 3);
        for pixel in rgba.chunks_exact(4) {
            out.extend_from_slice(&pixel[..3]);
        }
        out
    }

    pub fn snapshot_ppm(&self) -> Vec<u8> {
        let header = format!("P6\n{} {}\n255\n", self.width, self.height);
        let mut out = Vec::with_capacity(header.len() + (self.width * self.height * 3) as usize);
        out.extend_from_slice(header.as_bytes());
        let src: &[u16] = bytemuck::cast_slice(&self.pixels);
        for &s in src {
            let p = u16::from_le(s);
            let r5 = ((p >> 11) & 0x1f) as u8;
            let g6 = ((p >> 5) & 0x3f) as u8;
            let b5 = (p & 0x1f) as u8;
            out.push(self.backlight_channel((r5 << 3) | (r5 >> 2)));
            out.push(self.backlight_channel((g6 << 2) | (g6 >> 4)));
            out.push(self.backlight_channel((b5 << 3) | (b5 >> 2)));
        }
        out
    }
}

/// Decode a single RGB565 pixel into a packed `0xAA_BB_GG_RR` `u32`
/// — i.e. the byte order an `RGBA8888` slice expects in little-endian
/// memory: `[R, G, B, A]`. Inlined hot path: this runs once per
/// framebuffer pixel during every host display update.
#[inline(always)]
pub fn rgb565_to_rgba8888(p: u16) -> u32 {
    let r5 = ((p >> 11) & 0x1f) as u32;
    let g6 = ((p >> 5) & 0x3f) as u32;
    let b5 = (p & 0x1f) as u32;
    let r = (r5 << 3) | (r5 >> 2);
    let g = (g6 << 2) | (g6 >> 4);
    let b = (b5 << 3) | (b5 >> 2);
    // Memory layout in little-endian: byte0=R, byte1=G, byte2=B, byte3=A.
    r | (g << 8) | (b << 16) | 0xff00_0000
}

/// Pack an `(R, G, B)` triple into RGB565.
pub fn pack_rgb565(r: u8, g: u8, b: u8) -> u16 {
    let r5 = (r as u16 >> 3) & 0x1f;
    let g6 = (g as u16 >> 2) & 0x3f;
    let b5 = (b as u16 >> 3) & 0x1f;
    (r5 << 11) | (g6 << 5) | b5
}

/// Convert a Win32 `COLORREF` (`0x00BBGGRR`) to RGB565.
pub fn colorref_to_rgb565(colorref: u32) -> u16 {
    let r = (colorref & 0xff) as u8;
    let g = ((colorref >> 8) & 0xff) as u8;
    let b = ((colorref >> 16) & 0xff) as u8;
    pack_rgb565(r, g, b)
}

/// Intersect `[x, x+w) × [y, y+h)` with `[0, max_w) × [0, max_h)`.
/// Returns `(x0, y0, x1, y1)` in `u32`.
fn clip_rect(x: i32, y: i32, w: i32, h: i32, max_w: u32, max_h: u32) -> (u32, u32, u32, u32) {
    let x0 = x.max(0).min(max_w as i32) as u32;
    let y0 = y.max(0).min(max_h as i32) as u32;
    let x1 = (x + w).max(0).min(max_w as i32) as u32;
    let y1 = (y + h).max(0).min(max_h as i32) as u32;
    (x0, y0, x1, y1)
}

#[cfg(test)]
mod tests {
    #[test]
    fn raw_rgb565_keeps_backlight_metadata_and_preserves_neutral_only_legacy_view() {
        let mut fb = Framebuffer::new(3, 1);
        fb.pixels.copy_from_slice(&[0, 248, 224, 7, 31, 0]);
        assert_eq!(fb.presentation_rgb565(), Some(&fb.pixels[..]));
        fb.set_backlight_level(21);
        assert!(fb.presentation_rgb565().is_none());
        assert_eq!(
            fb.presentation_rgb565_unadjusted(),
            Some(&[0, 248, 224, 7, 31, 0][..])
        );
        fb.set_render_presentation(6, 2, vec![0; 24]);
        assert!(fb.presentation_rgb565_unadjusted().is_none());
    }
    #[test]
    fn rgba_presentation_matches_rgb565_for_every_color_and_backlight() {
        let mut old = Framebuffer::new(128, 128);
        let mut new = old.clone();
        let pixels: Vec<u8> = (0..=u16::MAX).flat_map(u16::to_le_bytes).collect();
        old.set_render_presentation(256, 256, pixels);
        let mut expected = Vec::new();
        old.snapshot_presentation_rgba8888_into(&mut expected);
        new.set_render_presentation_rgba(256, 256, expected.clone());
        let mut actual = Vec::new();
        for level in 1..=31 {
            old.set_backlight_level(level);
            new.set_backlight_level(level);
            old.snapshot_presentation_rgba8888_into(&mut expected);
            new.snapshot_presentation_rgba8888_into(&mut actual);
            assert_eq!(actual, expected, "backlight {level}");
            assert_eq!(new.pixels, old.pixels);
        }
        new.mark_dirty();
        assert_eq!(new.presentation_size(), (128, 128));
        new.set_render_presentation_rgba(256, 256, vec![0; 256 * 256 * 2]);
        assert_eq!(new.presentation_size(), (128, 128));
        new.set_render_presentation_rgba(256, 384, vec![0; 256 * 384 * 4]);
        assert_eq!(new.presentation_size(), (128, 128));
    }

    #[test]
    fn render_presentation_is_host_only_and_invalidated_by_guest_writes() {
        let mut fb = Framebuffer::new(2, 1);
        fb.pixels.copy_from_slice(&[0, 0, 255, 255]);
        let native = fb.snapshot_rgba8888();
        let image = [0x00, 0xf8, 0xe0, 0x07, 0x1f, 0, 0xff, 0xff].repeat(2);
        fb.set_render_presentation(4, 2, image);
        assert_eq!(
            (fb.width, fb.height, fb.byte_size(), fb.stride_bytes()),
            (2, 1, 4, 4)
        );
        assert_eq!(fb.snapshot_rgba8888(), native);
        assert_eq!(fb.presentation_size(), (4, 2));
        let mut high = Vec::new();
        fb.snapshot_presentation_rgba8888_into(&mut high);
        assert_eq!(high.len(), 4 * 2 * 4);
        assert_eq!(&high[..4], &[255, 0, 0, 255]);
        fb.set_backlight_level(1);
        fb.snapshot_presentation_rgba8888_into(&mut high);
        assert!(high[0] < 255);
        assert_eq!(fb.presentation_size(), (4, 2));
        fb.mark_dirty();
        assert_eq!(fb.presentation_size(), (2, 1));
        fb.snapshot_presentation_rgba8888_into(&mut high);
        assert_eq!(high, fb.snapshot_rgba8888());
        fb.set_render_presentation(3, 2, vec![0; 12]);
        assert_eq!(fb.presentation_size(), (2, 1));
    }

    #[test]
    fn host_presentation_accepts_every_uniform_render_scale() {
        for (w, h) in [(320, 240), (240, 320), (640, 480)] {
            let mut fb = Framebuffer::new(w, h);
            let guest = fb.pixels.clone();
            for scale in [2, 3, 4] {
                let (rw, rh) = (w * scale, h * scale);
                fb.set_render_presentation(rw, rh, vec![0; (rw * rh * 2) as usize]);
                assert_eq!(fb.presentation_size(), (rw, rh));
                assert_eq!((fb.width, fb.height), (w, h));
                assert_eq!(fb.pixels, guest);
                let mut pixels = Vec::new();
                fb.snapshot_presentation_rgba8888_into(&mut pixels);
                assert_eq!(pixels.len(), (rw * rh * 4) as usize);
                fb.mark_dirty();
                assert_eq!(fb.presentation_size(), (w, h));
            }
            for (rw, rh, len) in [(w * 3, h * 2, w * h * 12), (w * 4, h * 4, 2), (0, h, 0)] {
                fb.set_render_presentation(rw, rh, vec![0; len as usize]);
                assert_eq!(fb.presentation_size(), (w, h));
            }
        }
    }

    #[test]
    fn backlight_changes_host_snapshots_without_modifying_guest_pixels() {
        let mut fb = super::Framebuffer::new(1, 1);
        fb.pixels = 0x7befu16.to_le_bytes().to_vec();
        let original = fb.pixels.clone();
        let normal = fb.snapshot_rgba8888();
        let counter = fb.frame_counter;
        fb.set_backlight_level(11);
        assert_eq!(fb.frame_counter, counter);
        fb.set_backlight_level(6);
        let dim = fb.snapshot_rgba8888();
        assert!(dim[0] < normal[0] && dim[1] < normal[1]);
        assert_eq!(dim[3], 255);
        assert_eq!(fb.pixels, original);
        assert_eq!(fb.frame_counter, counter + 1);
        fb.set_backlight_level(31);
        assert!(fb.snapshot_rgba8888()[0] > normal[0]);
        fb.set_backlight_level(0);
        assert_eq!(fb.backlight_level(), 11);
        fb.set_backlight_level(u32::MAX);
        assert_eq!(fb.backlight_level(), 11);
        fb.set_backlight_level(11);
        assert_eq!(fb.snapshot_rgba8888(), normal);
    }

    #[test]
    fn backlight_extremes_preserve_visible_monotonic_contrast() {
        let mut fb = super::Framebuffer::new(1, 1);
        for level in 1..=31 {
            fb.set_backlight_level(level);
            let mut previous = 0;
            for channel in 0..=255u8 {
                let shown = fb.backlight_channel(channel);
                assert!(shown >= previous);
                assert!(shown as u32 >= channel as u32 / 2);
                if channel > 0 {
                    assert!(shown > 0);
                }
                if channel < 255 {
                    assert!(shown < 255);
                }
                previous = shown;
            }
            if level <= 11 {
                assert_eq!(fb.backlight_channel(0), 0);
            }
            assert!(fb.backlight_channel(255) > fb.backlight_channel(0));
        }
        fb.set_backlight_level(1);
        assert_eq!(fb.backlight_channel(128), 103);
        fb.set_backlight_level(31);
        assert_eq!(fb.backlight_channel(128), 138);
        let maximum = fb.snapshot_rgba8888();
        fb.set_backlight_level(21);
        assert_ne!(fb.snapshot_rgba8888(), maximum);
        assert_eq!(fb.backlight_channel(128), 133);
        let middle = fb.backlight_channel(128);
        fb.set_backlight_level(11);
        assert!(fb.backlight_channel(128) < middle);
        assert!(middle < 138);
        fb.set_backlight_level(11);
        for channel in 0..=255u8 {
            assert_eq!(fb.backlight_channel(channel), channel);
        }
    }

    use super::*;

    #[test]
    fn fill_then_count_changes() {
        let mut fb = Framebuffer::new(4, 4);
        let c0 = fb.frame_counter;
        fb.fill(pack_rgb565(255, 0, 0));
        assert_ne!(fb.frame_counter, c0);
        let snap = fb.snapshot_rgba8888();
        assert_eq!(snap[0], 0xff);
        assert_eq!(snap[1], 0x00);
        assert_eq!(snap[2], 0x00);
        assert_eq!(snap[3], 0xff);
    }

    #[test]
    fn fill_rect_clips_to_bounds() {
        let mut fb = Framebuffer::new(4, 4);
        let red = pack_rgb565(255, 0, 0);
        fb.fill_rect(2, 2, 100, 100, red);
        // Only the bottom-right 2x2 should be red.
        let rgba = fb.snapshot_rgba8888();
        let pix = |x: u32, y: u32| -> [u8; 4] {
            let off = ((y * 4 + x) * 4) as usize;
            [rgba[off], rgba[off + 1], rgba[off + 2], rgba[off + 3]]
        };
        assert_eq!(pix(0, 0), [0, 0, 0, 0xff]);
        assert_eq!(pix(2, 2)[0], 0xff);
        assert_eq!(pix(3, 3)[0], 0xff);
    }

    #[test]
    fn blit_copies_subregion() {
        let mut fb = Framebuffer::new(4, 4);
        let blue = pack_rgb565(0, 0, 255);
        let src: Vec<u8> = (0..(2 * 2)).flat_map(|_| blue.to_le_bytes()).collect();
        fb.blit_from(1, 1, 0, 0, 2, 2, &src, 2, 2);
        let rgba = fb.snapshot_rgba8888();
        let pix = |x: u32, y: u32| -> [u8; 4] {
            let off = ((y * 4 + x) * 4) as usize;
            [rgba[off], rgba[off + 1], rgba[off + 2], rgba[off + 3]]
        };
        assert_eq!(pix(0, 0)[2], 0x00);
        assert_eq!(pix(1, 1)[2], 0xff);
        assert_eq!(pix(2, 2)[2], 0xff);
    }

    #[test]
    fn ppm_snapshot_starts_with_header() {
        let fb = Framebuffer::new(2, 2);
        let ppm = fb.snapshot_ppm();
        assert!(ppm.starts_with(b"P6\n2 2\n255\n"));
        assert_eq!(ppm.len(), 11 + 12);
    }
}

#[cfg(test)]
mod display_timing_tests {
    use super::DirectDrawTiming;
    #[test]
    fn blank_waits_cross_an_edge_and_end_waits_cross_the_blank_interval() {
        let clock = DirectDrawTiming::default();
        let period = DirectDrawTiming::PERIOD;
        assert_eq!(
            clock.blank_deadline(clock.epoch, false),
            clock.epoch + period
        );
        let during = clock.epoch + std::time::Duration::from_micros(500);
        assert!(clock.in_blank(during));
        assert_eq!(
            clock.blank_deadline(during, true),
            clock.epoch + DirectDrawTiming::BLANK
        );
        assert_eq!(clock.blank_deadline(during, false), clock.epoch + period);
        let active = clock.epoch + std::time::Duration::from_millis(2);
        assert!(!clock.in_blank(active));
        assert_eq!(
            clock.blank_deadline(active, true),
            clock.epoch + period + DirectDrawTiming::BLANK
        );
    }
    #[test]
    fn primary_presentations_do_not_catch_up_or_depend_on_pixel_changes() {
        let mut clock = DirectDrawTiming::default();
        let start = clock.epoch;
        assert_eq!(clock.present_deadline(start), start);
        clock.last_present = Some(start);
        assert_eq!(
            clock.present_deadline(start),
            start + DirectDrawTiming::PERIOD
        );
        let late = start + std::time::Duration::from_secs(2);
        assert_eq!(clock.present_deadline(late), late);
        clock.last_present = Some(late);
        assert_eq!(
            clock.present_deadline(late),
            late + DirectDrawTiming::PERIOD
        );
    }

    #[test]
    fn user_frame_limits_leave_vblank_unchanged_and_do_not_catch_up() {
        let mut clock = DirectDrawTiming::default();
        let start = clock.epoch;
        let blank = clock.blank_deadline(start, true);
        for fps in [10, 15, 20, 30, 45, 60, 120] {
            clock.frame_rate_limit = Some(fps);
            clock.last_present = None;
            assert_eq!(clock.present_deadline(start), start);
            clock.last_present = Some(start);
            let period = std::time::Duration::from_nanos(1_000_000_000u64.div_ceil(fps as u64));
            assert_eq!(clock.frame_limit_deadline(start), start + period);
            assert_eq!(clock.present_deadline(start), start + period);
            assert_eq!(clock.blank_deadline(start, true), blank);
            let late = start + std::time::Duration::from_secs(2);
            assert_eq!(clock.present_deadline(late), late);
            clock.last_present = Some(late);
            assert_eq!(clock.present_deadline(late), late + period);
        }
        clock.frame_rate_limit = Some(0);
        clock.last_present = Some(start);
        assert_eq!(clock.present_deadline(start), start);
        assert_eq!(clock.frame_limit_deadline(start), start);
        assert_eq!(clock.blank_deadline(start, true), blank);
        // Unconfigured/invalid values keep the native LCD pacing.
        clock.last_present = Some(start);
        for value in [None, Some(17), Some(u32::MAX)] {
            clock.frame_rate_limit = value;
            assert_eq!(clock.frame_limit_deadline(start), start);
            assert_eq!(
                clock.present_deadline(start),
                start + DirectDrawTiming::PERIOD
            );
        }
    }
}

#[cfg(test)]
mod presentation_stats_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn published_image_rate_is_independent_of_completed_presents() {
        let stats = PresentationStats::default();
        let start = Instant::now();
        for frame in 0..=60 {
            let now = start + Duration::from_nanos(frame * 1_000_000_000 / 60);
            stats.record_at(now);
            if frame % 2 == 0 {
                stats.record_image_at(now);
            }
        }
        let end = start + Duration::from_secs(1);
        assert_eq!(stats.fps_at(end), Some(60.0));
        assert_eq!(stats.ips_at(end), Some(30.0));
        assert_eq!(stats.ips_at(end + Duration::from_secs(1)), Some(0.0));
        stats.record_image_at(end + Duration::from_secs(2));
        assert_eq!(stats.ips_at(end + Duration::from_secs(2)), Some(1.0));
        assert_eq!(stats.completed(), 61);
        stats.reset();
        assert_eq!(stats.ips(), None);
    }

    #[test]
    fn completed_presentations_do_not_dirty_pixels_and_survive_snapshot_clones() {
        let fb = Framebuffer::new(2, 2);
        let snapshot = fb.clone();
        let start = Instant::now();
        assert_eq!(fb.presentation_stats.fps_at(start), None);
        for frame in 0..=60 {
            fb.presentation_stats
                .record_at(start + Duration::from_nanos(frame * 1_000_000_000 / 60));
        }
        assert_eq!(fb.frame_counter, 0);
        assert_eq!(fb.pixels, vec![0; 8]);
        assert_eq!(snapshot.presentation_stats.completed(), 61);
        assert!(
            (snapshot
                .presentation_stats
                .fps_at(start + Duration::from_secs(1))
                .unwrap()
                - 60.0)
                .abs()
                < 0.01
        );
        assert_eq!(
            snapshot
                .presentation_stats
                .fps_at(start + Duration::from_secs(2)),
            Some(0.0)
        );
        fb.presentation_stats.reset();
        assert_eq!(
            snapshot
                .presentation_stats
                .fps_at(start + Duration::from_secs(2)),
            None
        );
    }
}

/// Checked GPU/CPU authority facade, not yet installed in KernelState.
pub mod deferred;
pub mod gapi_memory;
