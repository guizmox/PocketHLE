extern crate self as eframe;
pub use glow;
pub mod runner {use std::sync::Arc;
#[derive(Debug,Clone)]
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
}
#[path="/workspace/scratch/1a0f598492f0/gpu-x1-work/frontends/pocket-desktop/src/smaa.rs"] pub mod smaa;
#[path="/workspace/scratch/1a0f598492f0/gpu-x1-work/frontends/pocket-desktop/src/reconstruction.rs"] pub mod reconstruction;
pub mod app {use crate::runner::FrameSnapshot;
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
enum UpscaleFilter {
    None,
    Reconstruction,
    Smaa,
    SmaaSoft,
    Xbrz,
    Bicubic,
    Lanczos,
    Bilinear,
    Nearest,
}
impl UpscaleFilter {
    const ALL: [Self; 9] = [
        Self::None,
        Self::Reconstruction,
        Self::Smaa,
        Self::SmaaSoft,
        Self::Xbrz,
        Self::Bicubic,
        Self::Lanczos,
        Self::Bilinear,
        Self::Nearest,
    ];
    fn id(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Reconstruction => "reconstruction",
            Self::Smaa => "smaa",
            Self::SmaaSoft => "smaa_soft",
            Self::Xbrz => "xbrz",
            Self::Bicubic => "bicubic",
            Self::Lanczos => "lanczos",
            Self::Bilinear => "bilinear",
            Self::Nearest => "nearest",
        }
    }
    fn from_id(id: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|f| f.id() == id)
            .unwrap_or(Self::Reconstruction)
    }
    fn label(self) -> &'static str {
        match self {
            Self::None => "No filter",
            Self::Reconstruction => "GPU reconstruction (sharp contours; GPU)",
            Self::Smaa => "SMAA + reconstruction (anti-aliasing; GPU)",
            Self::SmaaSoft => "SMAA + bilinear (very soft; GPU)",
            Self::Xbrz => "xBRZ ×3 (smooth 2D; CPU intensive)",
            Self::Bicubic => "Bicubic (balanced; moderate cost)",
            Self::Lanczos => "Lanczos (sharp; intensive)",
            Self::Bilinear => "Bilinear (soft; fast)",
            Self::Nearest => "Nearest (crisp pixels; fastest)",
        }
    }
    fn stepped(self, forward: bool) -> Self {
        let index = Self::ALL
            .iter()
            .position(|filter| *filter == self)
            .unwrap_or(0);
        let next = if forward {
            (index + 1) % Self::ALL.len()
        } else {
            (index + Self::ALL.len() - 1) % Self::ALL.len()
        };
        Self::ALL[next]
    }

    fn gpu_filter(self) -> i32 {
        match self {
            Self::None | Self::Nearest => 0,
            Self::Bilinear => 1,
            Self::Bicubic => 2,
            Self::Reconstruction => 3,
            Self::Lanczos => 4,
            Self::Smaa => 5,
            Self::Xbrz => 6,
            Self::SmaaSoft => 7,
        }
    }

    fn image_filter(self) -> image::imageops::FilterType {
        match self {
            Self::Reconstruction | Self::Smaa | Self::Xbrz | Self::Bicubic => {
                image::imageops::FilterType::CatmullRom
            }
            Self::Lanczos => image::imageops::FilterType::Lanczos3,
            Self::Bilinear | Self::SmaaSoft => image::imageops::FilterType::Triangle,
            Self::None | Self::Nearest => image::imageops::FilterType::Nearest,
        }
    }
}
pub(crate) fn filtered_snapshot_image(
    frame: &FrameSnapshot,
    factor: u32,
    filter: image::imageops::FilterType,
) -> Result<image::RgbaImage, String> {
    let frame = frame.cpu_only()?;
    let source = image::RgbaImage::from_raw(frame.texture_width, frame.texture_height, frame.rgba)
        .ok_or_else(|| "invalid RGBA framebuffer size".to_string())?;
    if factor == 1 {
        return Ok(source);
    }
    Ok(image::imageops::resize(
        &source,
        frame.texture_width * factor,
        frame.texture_height * factor,
        filter,
    ))
}
pub struct Capture {upscale_filter:UpscaleFilter}
impl Capture {pub fn nearest()->Self {Self {upscale_filter:UpscaleFilter::Nearest}}
pub fn filtered_frame_image(
        &self,
        frame: &FrameSnapshot,
        factor: u32,
    ) -> Result<image::RgbaImage, String> {
        filtered_snapshot_image(frame, factor, self.upscale_filter.image_filter())
    }
}
}
