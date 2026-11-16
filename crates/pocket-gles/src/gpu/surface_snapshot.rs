//! Capture native attachments only when a checkpoint is requested.

use super::*;

const FRAMEBUFFER: u32 = 0x8d40;
const READ_FRAMEBUFFER: u32 = 0x8ca8;
const DRAW_FRAMEBUFFER: u32 = 0x8ca9;
const COLOR_ATTACHMENT: u32 = 0x8ce0;
const DEPTH_ATTACHMENT: u32 = 0x8d00;
const DEPTH_COMPONENT: u32 = 0x1902;
const GL_UNSIGNED_INT: u32 = 0x1405;
const DEPTH16: u32 = 0x81a5;
const DEPTH24: u32 = 0x81a6;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct NativeSurfaceSnapshot {
    width: u32,
    height: u32,
    rgb565: bool,
    depth_format: u32,
    color: Vec<u8>,
    depth: Vec<u32>,
}

impl Renderer {
    /// Capture the full-size native attachments in GL bottom-up order.
    /// Readback runs in the surface's own context and preserves the context
    /// previously current on this thread.
    pub fn capture_native_surface(&self) -> Result<NativeSurfaceSnapshot, String> {
        if self.host.es {
            return Err("native GLES depth readback is unavailable".into());
        }
        let mut previous = platform::SavedCurrent::capture()?;
        let captured = (|| {
            self.host.make_current()?;
            let mut frame = NativeSurfaceSnapshot {
                width: self.width,
                height: self.height,
                rgb565: self.rgb565,
                depth_format: self.depth_format,
                color: vec![0; self.width as usize * self.height as usize * 4],
                depth: vec![0; self.width as usize * self.height as usize],
            };
            unsafe {
                (self.gl.BindFramebuffer)(FRAMEBUFFER, self.fbo);
                (self.gl.PixelStorei)(0x0d05, 1);
                (self.gl.ReadPixels)(
                    0,
                    0,
                    self.width as i32,
                    self.height as i32,
                    GL_RGBA,
                    GL_UNSIGNED_BYTE,
                    frame.color.as_mut_ptr().cast(),
                );
                (self.gl.ReadPixels)(
                    0,
                    0,
                    self.width as i32,
                    self.height as i32,
                    DEPTH_COMPONENT,
                    GL_UNSIGNED_INT,
                    frame.depth.as_mut_ptr().cast(),
                );
            }
            unsafe { self.check()? };
            Ok(frame)
        })();
        let restored = previous.restore();
        restored?;
        captured
    }

    /// Upload color and depth to a fresh renderer at its saved physical size.
    /// Return an error if the receiving context cannot provide that depth format.
    pub fn restore_native_surface(&mut self, frame: &NativeSurfaceSnapshot) -> Result<(), String> {
        frame.validate()?;
        if self.host.es
            || self.width != frame.width
            || self.height != frame.height
            || self.rgb565 != frame.rgb565
            || self.depth_format != frame.depth_format
        {
            return Err("native snapshot attachment format or size mismatch".into());
        }
        let blit = self.host.load(c"glBlitFramebuffer");
        let blit = if blit.is_null() {
            self.host.load(c"glBlitFramebufferEXT")
        } else {
            blit
        };
        if blit.is_null() {
            return Err("native snapshot depth transfer is unavailable".into());
        }
        let blit: unsafe extern "system" fn(i32, i32, i32, i32, i32, i32, i32, i32, u32, u32) =
            unsafe { std::mem::transmute(blit) };

        let mut previous = platform::SavedCurrent::capture()?;
        let uploaded = (|| {
            self.host.make_current()?;
            unsafe { self.check()? };
            let color565 = frame.rgb565.then(|| {
                frame
                    .color
                    .chunks_exact(4)
                    .map(|rgba| {
                        u16::from(rgba[0] >> 3) << 11
                            | u16::from(rgba[1] >> 2) << 5
                            | u16::from(rgba[2] >> 3)
                    })
                    .collect::<Vec<_>>()
            });
            let (format, ty, color) = match &color565 {
                Some(packed) => (GL_RGB, GL_UNSIGNED_SHORT_5_6_5, packed.as_ptr().cast()),
                None => (GL_RGBA, GL_UNSIGNED_BYTE, frame.color.as_ptr().cast()),
            };
            let mut depth_texture = 0;
            let mut source = 0;
            let width = frame.width as i32;
            let height = frame.height as i32;
            let result = unsafe {
                (self.gl.ActiveTexture)(GL_TEXTURE0);
                (self.gl.BindTexture)(GL_TEXTURE_2D, self.color);
                (self.gl.PixelStorei)(0x0cf5, 1);
                (self.gl.TexImage2D)(
                    GL_TEXTURE_2D,
                    0,
                    if frame.rgb565 { 0x8d62 } else { GL_RGBA as i32 },
                    width,
                    height,
                    0,
                    format,
                    ty,
                    color,
                );

                (self.gl.GenTextures)(1, &mut depth_texture);
                (self.gl.BindTexture)(GL_TEXTURE_2D, depth_texture);
                (self.gl.TexImage2D)(
                    GL_TEXTURE_2D,
                    0,
                    frame.depth_format as i32,
                    width,
                    height,
                    0,
                    DEPTH_COMPONENT,
                    GL_UNSIGNED_INT,
                    frame.depth.as_ptr().cast(),
                );
                self.params(
                    Filter::Nearest,
                    Filter::Nearest,
                    Wrap::ClampToEdge,
                    Wrap::ClampToEdge,
                );
                (self.gl.GenFramebuffers)(1, &mut source);
                (self.gl.BindFramebuffer)(FRAMEBUFFER, source);
                (self.gl.FramebufferTexture2D)(
                    FRAMEBUFFER,
                    COLOR_ATTACHMENT,
                    GL_TEXTURE_2D,
                    self.color,
                    0,
                );
                (self.gl.FramebufferTexture2D)(
                    FRAMEBUFFER,
                    DEPTH_ATTACHMENT,
                    GL_TEXTURE_2D,
                    depth_texture,
                    0,
                );
                let complete = (self.gl.CheckFramebufferStatus)(FRAMEBUFFER) == 0x8cd5;
                if complete && source != 0 && depth_texture != 0 {
                    (self.gl.BindFramebuffer)(DRAW_FRAMEBUFFER, self.fbo);
                    (self.gl.BindFramebuffer)(READ_FRAMEBUFFER, source);
                    self.scissor(None);
                    blit(
                        0,
                        0,
                        width,
                        height,
                        0,
                        0,
                        width,
                        height,
                        GL_DEPTH_BUFFER_BIT,
                        GL_NEAREST,
                    );
                    self.check()
                } else {
                    Err("native snapshot depth framebuffer incomplete".into())
                }
            };
            unsafe {
                (self.gl.BindFramebuffer)(FRAMEBUFFER, self.fbo);
                (self.gl.DeleteFramebuffers)(1, &source);
                (self.gl.DeleteTextures)(1, &depth_texture);
            }
            self.pipeline = None;
            self.dirty = true;
            self.display_prepared = false;
            self.logical_packed.clear();
            result
        })();
        let restored = previous.restore();
        restored?;
        uploaded
    }
}

impl NativeSurfaceSnapshot {
    /// Recreate the receiving context without disturbing the caller's current
    /// native binding, including when allocation fails.
    pub(crate) fn build_renderer(
        &self,
        logical_width: u32,
        logical_height: u32,
        render_policy: u8,
    ) -> Result<Renderer, String> {
        self.validate()?;
        if logical_width == 0
            || logical_height == 0
            || logical_width > self.width
            || logical_height > self.height
        {
            return Err("invalid native snapshot logical size".into());
        }
        let mut previous = platform::SavedCurrent::capture()?;
        let result = (|| {
            let mut gpu = Renderer::new(logical_width, logical_height)?;
            if gpu.depth_format != self.depth_format {
                gpu.depth_format = self.depth_format;
                gpu.width = 0;
                gpu.height = 0;
                gpu.resize(logical_width, logical_height)?;
            }
            gpu.set_surface_rgb565(self.rgb565)?;
            gpu.set_render_scale(render_policy)?;
            gpu.restore_native_surface(self)?;
            Ok(gpu)
        })();
        let restored = previous.restore();
        restored?;
        result
    }

    fn validate(&self) -> Result<(), String> {
        let pixels = (self.width as usize)
            .checked_mul(self.height as usize)
            .ok_or("native snapshot surface size overflow")?;
        if self.width == 0
            || self.height == 0
            || self.depth.len() != pixels
            || pixels.checked_mul(4) != Some(self.color.len())
            || !matches!(self.depth_format, DEPTH16 | DEPTH24)
        {
            return Err("invalid native snapshot surface".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires native desktop OpenGL"]
    fn native_surface_snapshot_preserves_rgb565_and_depth() {
        let mut producer = Renderer::new(4, 4).unwrap();
        producer.set_surface_rgb565(true).unwrap();
        let mut state = PipelineState {
            viewport: (0, 0, 4, 4),
            ..Default::default()
        };
        producer
            .clear(
                4,
                4,
                &state,
                [1., 0., 0., 1.],
                0.25,
                GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
            )
            .unwrap();
        state.scissor = Some((0, 2, 4, 2));
        producer
            .clear(
                4,
                4,
                &state,
                [0., 1., 0., 1.],
                0.75,
                GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
            )
            .unwrap();
        let original = producer.capture_native_surface().unwrap();
        assert_ne!(original.depth[0], original.depth[15]);
        drop(producer);

        let mut restored = Renderer::new(4, 4).unwrap();
        restored.set_surface_rgb565(true).unwrap();
        restored.restore_native_surface(&original).unwrap();
        let new = restored.capture_native_surface().unwrap();
        assert_eq!(new.color, original.color);
        assert_eq!(new.depth, original.depth);
    }
}
