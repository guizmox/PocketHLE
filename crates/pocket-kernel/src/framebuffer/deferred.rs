//! Checked CPU access to a GPU-authoritative image. Only metadata can be
//! dereferenced; the eager image and its pixel Vec remain inaccessible.
use super::Framebuffer;
use std::sync::Arc;

#[derive(Debug)]
pub struct MaterializedPixels {
    pub logical_size: (u32, u32),
    pub native_size: (u32, u32),
    pub rgb565: Vec<u8>,
    /// Top-down, RGB565-quantized, opaque RGBA, before host brightness.
    pub native_rgba: Option<Vec<u8>>,
}
/// Immutable snapshot provider. Implemented above the kernel, so the kernel
/// neither owns a GL context nor depends on the OpenGLES implementation.
pub trait CpuPixelSource: std::fmt::Debug + Send + Sync {
    fn logical_size(&self) -> (u32, u32);
    fn native_size(&self) -> (u32, u32);
    fn materialize(&self) -> Result<MaterializedPixels, String>;
    /// A presentation consumer may run on another host thread. Implementations
    /// must release native readers before returning, rather than retaining a
    /// thread-local context past the consumer's display lifetime.
    fn materialize_transient(&self) -> Result<MaterializedPixels, String> {
        self.materialize()
    }
}
/// Immutable CPU fallback for a displayed GPU snapshot. Successful conversion
/// is cached across clones/captures; failures remain retryable. No native
/// context is stored here, and normal GPU presentation never requests bytes.
#[derive(Debug)]
pub struct DeferredPresentation {
    source: Arc<dyn CpuPixelSource>,
    logical: (u32, u32),
    native: (u32, u32),
    backlight: u32,
    cached: std::sync::Mutex<Option<Arc<Vec<u8>>>>,
}
impl DeferredPresentation {
    pub fn new(source: Arc<dyn CpuPixelSource>, backlight: u32) -> Result<Self, String> {
        let logical = source.logical_size();
        let native = source.native_size();
        if logical.0 == 0
            || logical.1 == 0
            || native.0 == 0
            || native.1 == 0
            || native.0 % logical.0 != 0
            || native.1 % logical.1 != 0
            || native.0 / logical.0 != native.1 / logical.1
            || !(1..=4).contains(&(native.0 / logical.0))
        {
            return Err("invalid deferred presentation geometry".into());
        }
        bytes(logical, 2)?;
        bytes(native, 4)?;
        Ok(Self {
            source,
            logical,
            native,
            backlight,
            cached: std::sync::Mutex::new(None),
        })
    }
    pub fn size(&self) -> (u32, u32) {
        self.native
    }
    pub fn cached_rgba(&self) -> Option<Arc<Vec<u8>>> {
        self.cached.lock().ok()?.clone()
    }
    pub fn rgba(&self) -> Result<Arc<Vec<u8>>, String> {
        let mut cache = self
            .cached
            .lock()
            .map_err(|_| "deferred presentation cache poisoned")?;
        if let Some(pixels) = cache.as_ref() {
            return Ok(pixels.clone());
        }
        let pixels = self.source.materialize_transient()?;
        pixels.validate_for(self.logical, self.native)?;
        let mut image = Framebuffer::new(self.logical.0, self.logical.1);
        image.pixels = pixels.rgb565;
        if self.native != self.logical {
            image.set_render_presentation_rgba(
                self.native.0,
                self.native.1,
                pixels.native_rgba.unwrap(),
            );
        }
        image.set_backlight_level(self.backlight);
        let mut rgba = Vec::new();
        image.snapshot_presentation_rgba8888_into(&mut rgba);
        let rgba = Arc::new(rgba);
        *cache = Some(rgba.clone());
        Ok(rgba)
    }
}
impl MaterializedPixels {
    fn validate_for(&self, logical: (u32, u32), native: (u32, u32)) -> Result<(), String> {
        if self.logical_size != logical
            || self.native_size != native
            || self.rgb565.len() != bytes(logical, 2)?
        {
            return Err("GPU materialization metadata or pixel length mismatch".into());
        }
        match &self.native_rgba {
            Some(rgba) if rgba.len() != bytes(native, 4)? => {
                Err("GPU native image length mismatch".into())
            }
            None if native != logical => Err("GPU native presentation pixels missing".into()),
            _ => Ok(()),
        }
    }
}
#[derive(Debug)]
struct GpuAuthority {
    source: Arc<dyn CpuPixelSource>,
    logical: (u32, u32),
    native: (u32, u32),
}
#[derive(Debug)]
pub struct DeferredFramebuffer {
    cpu: Framebuffer,
    gpu: Option<GpuAuthority>,
    resident: bool,
    epoch: u64,
    ips_image: Option<IpsImage>,
    system_video: Option<Framebuffer>,
}

#[derive(Debug)]
struct IpsImage {
    revision: u64,
    size: (u32, u32),
    pixels: Vec<u8>,
}
// Only metadata is exposed. In particular, dereferencing this wrapper can
// never expose the eager image, its pixel Vec or its snapshot methods.
impl std::ops::Deref for DeferredFramebuffer {
    type Target = super::FramebufferMetadata;
    fn deref(&self) -> &Self::Target {
        &self.cpu
    }
}
impl std::ops::DerefMut for DeferredFramebuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.cpu
    }
}
impl Default for DeferredFramebuffer {
    fn default() -> Self {
        Self::new(Framebuffer::default()).expect("valid default framebuffer")
    }
}
fn bytes(size: (u32, u32), components: usize) -> Result<usize, String> {
    (size.0 as usize)
        .checked_mul(size.1 as usize)
        .and_then(|n| n.checked_mul(components))
        .ok_or_else(|| "framebuffer size overflow".into())
}
impl DeferredFramebuffer {
    pub fn new(cpu: Framebuffer) -> Result<Self, String> {
        if cpu.width == 0
            || cpu.height == 0
            || cpu.bpp != 16
            || cpu.pixels.len() != bytes((cpu.width, cpu.height), 2)?
        {
            return Err("invalid CPU framebuffer".into());
        }
        Ok(Self {
            cpu,
            gpu: None,
            resident: true,
            epoch: 0,
            ips_image: None,
            system_video: None,
        })
    }
    pub fn has_system_video(&self) -> bool {
        self.system_video.is_some()
    }

    /// Replace the host-only video image without modifying guest pixels.
    pub fn replace_system_video(&mut self, frame: Option<Framebuffer>) -> bool {
        let changed = frame.is_some() || self.system_video.is_some();
        self.system_video = frame;
        changed
    }

    pub(crate) fn presentation_cpu(&mut self) -> Result<&Framebuffer, String> {
        if self.system_video.is_some() {
            let frame = self.system_video.as_mut().unwrap();
            frame.frame_counter = self.cpu.frame_counter;
            frame.presentation_stats = self.cpu.presentation_stats.clone();
            frame.set_backlight_level(self.cpu.backlight_level());
            return Ok(frame);
        }
        self.cpu()
    }

    pub fn enable_ips(&mut self) {
        self.ips_image = Some(IpsImage {
            revision: self.cpu.frame_counter,
            size: self.logical_size(),
            pixels: self.cpu.pixels.clone(),
        });
    }

    pub fn ips_enabled(&self) -> bool {
        self.ips_image.is_some()
    }

    /// Compare resident pixels against the last measured image.
    pub fn observe_ips(&mut self) {
        let size = self.logical_size();
        let Some(image) = self.ips_image.as_mut() else {
            return;
        };
        if !self.resident || image.revision == self.cpu.frame_counter {
            return;
        }
        image.revision = self.cpu.frame_counter;
        if image.size != size || image.pixels != self.cpu.pixels {
            image.size = size;
            image.pixels.clone_from(&self.cpu.pixels);
            self.cpu.presentation_stats.record_image_update();
        }
    }

    /// Diagnostic snapshots also obey the checked CPU read boundary.
    pub fn write_ppm(&mut self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        let frame = self.cpu().map_err(std::io::Error::other)?;
        std::fs::write(path, frame.snapshot_ppm())
    }
    pub fn byte_size(&self) -> u32 {
        self.cpu.byte_size()
    }
    pub fn stride_bytes(&self) -> u32 {
        self.cpu.stride_bytes()
    }
    pub fn backlight_level(&self) -> u32 {
        self.cpu.backlight_level()
    }
    pub fn mark_dirty(&mut self) {
        self.cpu.mark_dirty();
    }
    pub fn logical_size(&self) -> (u32, u32) {
        (self.cpu.width, self.cpu.height)
    }
    pub fn presentation_size(&self) -> (u32, u32) {
        self.gpu
            .as_ref()
            .map_or_else(|| self.cpu.presentation_size(), |g| g.native)
    }
    pub fn revision(&self) -> u64 {
        self.cpu.frame_counter
    }
    /// Changes on a potential CPU mutation even when a compositor deliberately
    /// keeps the guest frame counter. GPU publication must check this epoch too.
    pub fn authority_epoch(&self) -> u64 {
        self.epoch
    }
    /// Borrow already materialized bytes without initiating any GPU work.
    /// None means callers must use cpu() or defer a CPU-only operation.
    pub fn resident_cpu(&self) -> Option<&Framebuffer> {
        self.resident.then_some(&self.cpu)
    }
    pub fn cpu_resident(&self) -> bool {
        self.resident
    }
    pub fn gpu_source(&self) -> Option<&Arc<dyn CpuPixelSource>> {
        self.gpu.as_ref().map(|g| &g.source)
    }
    pub fn publish_gpu(&mut self, source: Arc<dyn CpuPixelSource>) -> Result<(), String> {
        let logical = source.logical_size();
        let native = source.native_size();
        if logical.0 == 0
            || logical.1 == 0
            || self.cpu.bpp != 16
            || logical != self.logical_size()
            || native.0 == 0
            || native.1 == 0
            || native.0 % logical.0 != 0
            || native.1 % logical.1 != 0
            || native.0 / logical.0 != native.1 / logical.1
            || !(1..=4).contains(&(native.0 / logical.0))
        {
            return Err("GPU snapshot geometry mismatch".into());
        }
        bytes(native, 4)?;
        self.cpu.mark_dirty();
        self.epoch = self.epoch.wrapping_add(1);
        self.gpu = Some(GpuAuthority {
            source,
            logical,
            native,
        });
        self.resident = false;
        Ok(())
    }
    fn materialize(&mut self) -> Result<(), String> {
        if self.resident {
            return Ok(());
        }
        let gpu = self.gpu.as_ref().ok_or("missing GPU authority")?;
        if gpu.logical != self.logical_size() || self.cpu.bpp != 16 {
            return Err("GPU authority no longer matches framebuffer metadata".into());
        }
        let pixels = gpu.source.materialize()?;
        pixels.validate_for(gpu.logical, gpu.native)?;
        // Commit only after every fallible operation. A failure keeps the old
        // storage private, the same GPU authority, revision and retryability.
        self.cpu.pixels = pixels.rgb565;
        self.cpu.clear_render_presentation();
        if gpu.native != gpu.logical {
            self.cpu.set_render_presentation_rgba(
                gpu.native.0,
                gpu.native.1,
                pixels.native_rgba.unwrap(),
            );
        }
        self.resident = true;
        Ok(())
    }
    /// CPU reads do not invalidate the matching GPU presentation or advance it.
    pub fn cpu(&mut self) -> Result<&Framebuffer, String> {
        self.materialize()?;
        Ok(&self.cpu)
    }
    /// Preserves untouched pixels before a partial write. The caller retains
    /// Framebuffer's existing obligation to mark_dirty after raw pixel writes.
    pub fn cpu_mut(&mut self) -> Result<&mut Framebuffer, String> {
        self.materialize()?;
        // Preserve the eager path's cache until a real write calls mark_dirty.
        // Acquiring a surface that is fully clipped must not lose native detail.
        if self.gpu.take().is_some() {
            self.cpu.clear_render_presentation();
        }
        self.epoch = self.epoch.wrapping_add(1);
        Ok(&mut self.cpu)
    }
    /// Publish a complete software/GL readback image without reading the old
    /// GPU authority first. The writer must replace every logical pixel. Its
    /// optional native RGBA scratch follows the existing presentation contract.
    pub fn overwrite_rgb565(
        &mut self,
        writer: impl FnOnce(&mut [u8], &mut Vec<u8>) -> Option<(u32, u32)>,
    ) {
        let mut native = self.cpu.take_render_pixels();
        self.gpu = None;
        self.resident = true;
        self.epoch = self.epoch.wrapping_add(1);
        // Invalidate before exposing mutable bytes, including during unwinding.
        self.cpu.mark_dirty();
        if let Some((w, h)) = writer(&mut self.cpu.pixels, &mut native) {
            self.cpu.set_render_presentation_rgba(w, h, native);
        }
    }
    /// A complete overwrite does not need the previous GPU pixels.
    pub fn fill(&mut self, color: u16) {
        self.cpu.fill(color);
        self.gpu = None;
        self.resident = true;
        self.epoch = self.epoch.wrapping_add(1);
    }
    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 || self.logical_size() == (w, h) {
            return;
        }
        self.cpu.resize(w, h);
        self.gpu = None;
        self.resident = true;
        self.epoch = self.epoch.wrapping_add(1);
    }
    pub fn set_backlight_level(&mut self, level: u32) {
        self.cpu.set_backlight_level(level);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn ips_counts_pixel_changes_once_per_publication() {
        let mut fb = DeferredFramebuffer::new(Framebuffer::new(2, 2)).unwrap();
        fb.enable_ips();
        fb.mark_dirty();
        fb.observe_ips();
        assert_eq!(fb.presentation_stats.ips(), None);

        fb.fill(0xffff);
        fb.observe_ips();
        fb.observe_ips();
        assert_eq!(fb.presentation_stats.ips(), Some(1.0));

        fb.fill(0xffff);
        fb.observe_ips();
        assert_eq!(fb.presentation_stats.ips(), Some(1.0));

        fb.fill(0x0000);
        fb.observe_ips();
        assert_eq!(fb.presentation_stats.ips(), Some(2.0));
        assert_eq!(fb.presentation_stats.fps(), None);
    }
    #[derive(Debug)]
    struct Source {
        reads: AtomicUsize,
        fail: AtomicBool,
        malformed: AtomicBool,
    }
    impl CpuPixelSource for Source {
        fn logical_size(&self) -> (u32, u32) {
            (2, 1)
        }
        fn native_size(&self) -> (u32, u32) {
            (4, 2)
        }
        fn materialize(&self) -> Result<MaterializedPixels, String> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            if self.fail.load(Ordering::Relaxed) {
                return Err("lost reader".into());
            }
            Ok(MaterializedPixels {
                logical_size: (2, 1),
                native_size: (4, 2),
                rgb565: if self.malformed.load(Ordering::Relaxed) {
                    vec![1]
                } else {
                    vec![0, 0xf8, 0xe0, 7]
                },
                native_rgba: Some(vec![255; 32]),
            })
        }
    }
    fn fixture() -> (DeferredFramebuffer, Arc<Source>) {
        let mut fb = Framebuffer::new(2, 1);
        fb.fill(0x1f);
        let s = Arc::new(Source {
            reads: AtomicUsize::new(0),
            fail: AtomicBool::new(false),
            malformed: AtomicBool::new(false),
        });
        (DeferredFramebuffer::new(fb).unwrap(), s)
    }
    #[test]
    fn reads_are_lazy_cached_and_partial_writes_keep_untouched_pixels() {
        let (mut f, s) = fixture();
        f.publish_gpu(s.clone()).unwrap();
        let revision = f.revision();
        let epoch = f.authority_epoch();
        assert!(!f.cpu_resident());
        assert_eq!(f.presentation_size(), (4, 2));
        assert_eq!(s.reads.load(Ordering::Relaxed), 0);
        assert_eq!(f.cpu().unwrap().pixels, [0, 0xf8, 0xe0, 7]);
        f.cpu().unwrap();
        assert_eq!(s.reads.load(Ordering::Relaxed), 1);
        assert_eq!(f.revision(), revision);
        assert_eq!(f.authority_epoch(), epoch);
        assert!(f.gpu_source().is_some());
        let cpu = f.cpu_mut().unwrap();
        cpu.put_pixel(0, 0, 0x1f);
        cpu.mark_dirty();
        assert_eq!(f.cpu().unwrap().pixels, [0x1f, 0, 0xe0, 7]);
        assert!(f.gpu_source().is_none());
        assert_ne!(f.authority_epoch(), epoch);
        assert_eq!(f.presentation_size(), (2, 1));
    }
    #[test]
    fn failed_or_malformed_reads_do_not_commit_and_can_retry() {
        let (mut f, s) = fixture();
        f.publish_gpu(s.clone()).unwrap();
        let revision = f.revision();
        let epoch = f.authority_epoch();
        s.fail.store(true, Ordering::Relaxed);
        assert!(f.cpu_mut().is_err());
        assert_eq!(f.cpu.pixels, [0x1f, 0, 0x1f, 0]);
        assert!(f.gpu_source().is_some());
        s.fail.store(false, Ordering::Relaxed);
        s.malformed.store(true, Ordering::Relaxed);
        assert!(f.cpu().is_err());
        assert_eq!(f.revision(), revision);
        assert_eq!(f.authority_epoch(), epoch);
        s.malformed.store(false, Ordering::Relaxed);
        assert_eq!(f.cpu().unwrap().pixels, [0, 0xf8, 0xe0, 7]);
        assert_eq!(s.reads.load(Ordering::Relaxed), 3);
    }
    #[test]
    fn presentation_fallback_is_lazy_cached_and_errors_remain_retryable() {
        let (_, source) = fixture();
        let deferred = Arc::new(DeferredPresentation::new(source.clone(), 21).unwrap());
        assert!(deferred.cached_rgba().is_none());
        assert_eq!(source.reads.load(Ordering::Relaxed), 0);
        source.fail.store(true, Ordering::Relaxed);
        assert!(deferred.rgba().is_err());
        assert!(deferred.cached_rgba().is_none());
        source.fail.store(false, Ordering::Relaxed);
        source.malformed.store(true, Ordering::Relaxed);
        assert!(deferred.rgba().is_err());
        assert!(deferred.cached_rgba().is_none());
        source.malformed.store(false, Ordering::Relaxed);
        let first = deferred.rgba().unwrap();
        assert_eq!(first.len(), 32);
        assert!(Arc::ptr_eq(&first, &deferred.rgba().unwrap()));
        assert_eq!(source.reads.load(Ordering::Relaxed), 3);
        let second = std::thread::spawn(move || deferred.rgba().unwrap())
            .join()
            .unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(source.reads.load(Ordering::Relaxed), 3);
        let mut expected = Framebuffer::new(2, 1);
        expected.set_render_presentation_rgba(4, 2, vec![255; 32]);
        expected.set_backlight_level(21);
        let mut rgba = Vec::new();
        expected.snapshot_presentation_rgba8888_into(&mut rgba);
        assert_eq!(first.as_slice(), rgba);
    }
    #[test]
    fn complete_cpu_replacement_never_reads_a_previous_gpu_image() {
        let (mut f, source) = fixture();
        f.publish_gpu(source.clone()).unwrap();
        source.fail.store(true, Ordering::Relaxed);
        let revision = f.revision();
        f.overwrite_rgb565(|logical, native| {
            logical.copy_from_slice(&[0xe0, 7, 0x1f, 0]);
            native.resize(32, 255);
            Some((4, 2))
        });
        assert_eq!(source.reads.load(Ordering::Relaxed), 0);
        assert!(f.cpu_resident());
        assert!(f.gpu_source().is_none());
        assert_eq!(f.cpu().unwrap().pixels, [0xe0, 7, 0x1f, 0]);
        assert_eq!(f.presentation_size(), (4, 2));
        assert_eq!(f.revision(), revision.wrapping_add(1));
    }
    #[test]
    fn acquiring_an_eager_surface_preserves_native_detail_until_an_actual_write() {
        let (mut f, source) = fixture();
        f.cpu_mut()
            .unwrap()
            .set_render_presentation_rgba(4, 2, vec![255; 32]);
        assert_eq!(f.presentation_size(), (4, 2));
        let revision = f.revision();
        // A GDI surface may be fully clipped or do nothing. Preserve the old
        // eager contract: mark_dirty is what invalidates its presentation.
        f.cpu_mut().unwrap();
        assert_eq!(f.presentation_size(), (4, 2));
        assert_eq!(f.revision(), revision);
        let cpu = f.cpu_mut().unwrap();
        cpu.put_pixel(0, 0, 0xffff);
        cpu.mark_dirty();
        assert_eq!(f.presentation_size(), (2, 1));
        assert_ne!(f.revision(), revision);
        assert_eq!(source.reads.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn full_overwrite_resize_and_brightness_do_not_read_gpu() {
        let (mut f, s) = fixture();
        f.publish_gpu(s.clone()).unwrap();
        f.set_backlight_level(31);
        assert!(!f.cpu_resident());
        f.fill(0xffff);
        assert_eq!(f.cpu().unwrap().pixels, [255; 4]);
        assert_eq!(s.reads.load(Ordering::Relaxed), 0);
        f.publish_gpu(s.clone()).unwrap();
        f.resize(3, 2);
        assert_eq!(f.cpu().unwrap().pixels, [0; 12]);
        assert_eq!(s.reads.load(Ordering::Relaxed), 0);
        assert!(f.gpu_source().is_none());
    }
}
