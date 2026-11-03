//! Adapter between an immutable GLES swap image and the checked kernel facade.
//! KernelState uses the checked facade; GPU publication remains opt-in until
//! frontend validation and the swap handoff are complete.
use pocket_gles::gpu::{SharedContextSource, SharedFrame, SharedReadback};
use pocket_kernel::framebuffer::deferred::{CpuPixelSource, MaterializedPixels};
use std::cell::RefCell;
thread_local! {static READER: RefCell<Option<SharedReadback>>=const{RefCell::new(None)};}
#[derive(Debug)]
pub struct GlesCpuPixelSource {
    frame: SharedFrame,
    source: SharedContextSource,
    logical: (u32, u32),
}
impl GlesCpuPixelSource {
    pub fn new(
        frame: SharedFrame,
        source: SharedContextSource,
        logical: (u32, u32),
    ) -> Result<Self, String> {
        let (w, h) = frame.size();
        let (lw, lh) = logical;
        if lw == 0
            || lh == 0
            || w % lw != 0
            || h % lh != 0
            || w / lw != h / lh
            || !(1..=4).contains(&(w / lw))
            || !frame.belongs_to_source(&source)
        {
            return Err("deferred GLES source geometry/group mismatch".into());
        }
        Ok(Self {
            frame,
            source,
            logical,
        })
    }
    fn read_with(&self, reader: &mut SharedReadback) -> Result<MaterializedPixels, String> {
        let result = (|| {
            let mut rgb565 = Vec::new();
            reader.read_rgb565_into(&self.frame, self.logical, &mut rgb565)?;
            let native_rgba = if self.native_size() != self.logical {
                let mut rgba = Vec::new();
                reader.read_display_rgba_into(&self.frame, &mut rgba)?;
                Some(rgba)
            } else {
                None
            };
            Ok(MaterializedPixels {
                logical_size: self.logical,
                native_size: self.native_size(),
                rgb565,
                native_rgba,
            })
        })();
        reader.clear_cache();
        result
    }
}
impl CpuPixelSource for GlesCpuPixelSource {
    fn logical_size(&self) -> (u32, u32) {
        self.logical
    }
    fn native_size(&self) -> (u32, u32) {
        self.frame.size()
    }
    fn materialize_transient(&self) -> Result<MaterializedPixels, String> {
        let mut reader = SharedReadback::new(&self.source)?;
        self.read_with(&mut reader)
    }
    fn materialize(&self) -> Result<MaterializedPixels, String> {
        READER.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot
                .as_ref()
                .is_none_or(|r| !r.matches_source(&self.source))
            {
                *slot = Some(SharedReadback::new(&self.source)?);
            }
            let reader = slot.as_mut().unwrap();
            self.read_with(reader)
        })
    }
}
/// Must run on the owning execution thread before its UI display is destroyed.
pub fn release_reader() {
    READER.with(|r| r.borrow_mut().take());
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_gles::{
        context::Context,
        gpu::{with_shared_context_source, Renderer},
        GL_COLOR_BUFFER_BIT,
    };
    use pocket_kernel::framebuffer::{deferred::DeferredFramebuffer, Framebuffer};
    use std::sync::Arc;
    #[test]
    #[ignore = "requires native shared OpenGL"]
    fn kernel_facade_materializes_the_retained_swap_and_preserves_partial_cpu_writes() {
        let ui = Renderer::new(8, 8).unwrap();
        let source = ui.shared_context_source();
        with_shared_context_source(Some(source.clone()), || {
            for policy in [1, 3, 4, 5] {
                let mut c = Context::new_rgb565(320, 240);
                c.set_internal_resolution(policy);
                c.enable_gpu().unwrap();
                c.clear_color = [0.15, 0.7, 0.35, 1.];
                c.clear(GL_COLOR_BUFFER_BIT);
                let frame = c.snapshot_shared_frame().unwrap();
                let mut reference = Framebuffer::new(320, 240);
                c.present_rgb565(&mut reference.pixels);
                let mut rgba = Vec::new();
                if let Some((w, h)) = c.scaled_display_rgba_into(&mut rgba) {
                    reference.set_render_presentation_rgba(w, h, rgba);
                }
                reference.set_backlight_level(21);
                let mut expected = Vec::new();
                reference.snapshot_presentation_rgba8888_into(&mut expected);
                // A later clear must not affect the retained snapshot.
                c.clear_color = [1., 0., 0., 1.];
                c.clear(GL_COLOR_BUFFER_BIT);
                drop(c);
                let provider =
                    Arc::new(GlesCpuPixelSource::new(frame, source.clone(), (320, 240)).unwrap());
                let (_, mut kernel, _) = crate::bluetooth::tests::setup();
                kernel.framebuffer = DeferredFramebuffer::new(Framebuffer::new(320, 240)).unwrap();
                let checked = &mut kernel.framebuffer;
                checked.set_backlight_level(21);
                checked.publish_gpu(provider).unwrap();
                assert!(!checked.cpu_resident());
                let revision = checked.revision();
                assert_eq!(checked.cpu().unwrap().pixels, reference.pixels);
                assert_eq!(checked.revision(), revision);
                assert!(checked.gpu_source().is_some());
                let mut actual = Vec::new();
                checked
                    .cpu()
                    .unwrap()
                    .snapshot_presentation_rgba8888_into(&mut actual);
                assert_eq!(actual, expected, "policy={policy}");
                let mut active = Vec::new();
                kernel
                    .presentation_framebuffer()
                    .unwrap()
                    .snapshot_presentation_rgba8888_into(&mut active);
                assert_eq!(active, expected, "active kernel policy={policy}");
                let checked = &mut kernel.framebuffer;
                let original = checked.cpu().unwrap().pixels.clone();
                let cpu = checked.cpu_mut().unwrap();
                cpu.put_pixel(0, 0, 0xffff);
                cpu.mark_dirty();
                assert_eq!(&checked.cpu().unwrap().pixels[2..], &original[2..]);
                assert!(checked.gpu_source().is_none());
                assert_eq!(checked.presentation_size(), (320, 240));
            }
        });
        release_reader();
        drop(ui);
    }
}
