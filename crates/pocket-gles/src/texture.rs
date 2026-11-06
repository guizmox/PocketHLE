//! Texture objects and pixel-format conversion.
//!
//! OpenGL ES 1.1 accepts textures in five base formats crossed with
//! three packed 16-bit types. Windows Mobile games lean on the packed
//! types heavily because a 16-bit texel halves both the file size and
//! the memory bandwidth — Call of Duty 2 uploads 85 textures, most of
//! them `GL_UNSIGNED_SHORT_5_6_5` or `GL_UNSIGNED_SHORT_4_4_4_4`.
//!
//! The software rasterizer samples RGBA8888. GPU-supported block formats
//! retain their compressed payload and defer decoding until software sampling
//! or a partial pixel update requires it. Unsupported formats decode at upload.

use crate::consts::*;

/// Texture filtering mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    Nearest,
    Linear,
    NearestMipmapNearest,
    LinearMipmapNearest,
    NearestMipmapLinear,
    LinearMipmapLinear,
}

impl Filter {
    pub fn uses_mipmaps(self) -> bool {
        !matches!(self, Self::Nearest | Self::Linear)
    }
}

/// Texture coordinate wrap mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrap {
    Repeat,
    ClampToEdge,
    /// NVIDIA accepts GL_CLAMP but emits the same hardware wrap as EDGE.
    GizmondoClamp,
}

#[derive(Debug, Clone)]
pub struct CompressedLevel {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub bytes: std::sync::Arc<[u8]>,
}

/// A single texture object, as created by `glGenTextures`.
#[derive(Debug, Clone)]
pub struct Texture {
    /// Incremented on pixel updates; used by the host GPU upload cache.
    pub revision: u64,
    pub(crate) gpu_copy: Option<crate::gpu::SharedFrame>,
    pub formats: std::collections::BTreeMap<u32, u32>,
    pub generate_mipmap: bool,
    pub width: u32,
    pub height: u32,
    /// Decoded RGBA8888 texels, row-major, `width * height * 4` bytes.
    /// Empty before upload or while a compressed native upload is deferred.
    pub rgba: Vec<u8>,
    /// Defined levels only: explicit uploads or requested automatic generation.
    pub mipmaps: std::collections::BTreeMap<u32, (u32, u32, Vec<u8>)>,
    pub compressed: std::collections::BTreeMap<u32, CompressedLevel>,
    pub max_level: u32,
    pub base_level: u32,
    pub min_lod: f32,
    pub max_lod: f32,
    pub goforce_lod: bool,
    pub min_filter: Filter,
    pub mag_filter: Filter,
    pub wrap_s: Wrap,
    pub wrap_t: Wrap,
}

impl Default for Texture {
    fn default() -> Self {
        Self {
            revision: 0,
            gpu_copy: None,
            formats: Default::default(),
            generate_mipmap: false,
            width: 0,
            height: 0,
            rgba: Vec::new(),
            mipmaps: Default::default(),
            compressed: Default::default(),
            max_level: 1000,
            base_level: 0,
            min_lod: -1000.,
            max_lod: 1000.,
            goforce_lod: false,
            min_filter: Filter::NearestMipmapLinear,
            mag_filter: Filter::Linear,
            wrap_s: Wrap::Repeat,
            wrap_t: Wrap::Repeat,
        }
    }
}

impl Texture {
    pub fn rgba_level(&self, level: u32) -> Option<std::borrow::Cow<'_, [u8]>> {
        let pixels = if level == 0 {
            Some(self.rgba.as_slice())
        } else {
            self.mipmaps.get(&level).map(|l| l.2.as_slice())
        };
        if let Some(p) = pixels.filter(|p| !p.is_empty()) {
            return Some(std::borrow::Cow::Borrowed(p));
        }
        if level == 0 {
            if let Some(frame) = &self.gpu_copy {
                let source = crate::gpu::configured_shared_context_source()?;
                let mut reader = crate::gpu::SharedReadback::new(&source).ok()?;
                return reader
                    .read_guest_rgb_texture(frame)
                    .ok()
                    .map(std::borrow::Cow::Owned);
            }
        }
        let c = self.compressed.get(&level)?;
        decode_compressed_to_rgba(&c.bytes, c.width, c.height, c.format)
            .map(std::borrow::Cow::Owned)
    }
    /// Decode deferred native uploads only when software/subimage needs RGBA.
    pub fn ensure_rgba(&mut self) {
        if self.gpu_copy.is_some() {
            if let Some(std::borrow::Cow::Owned(bytes)) = self.rgba_level(0) {
                self.rgba = bytes;
                self.gpu_copy = None;
            }
        }
        let levels: Vec<_> = self
            .compressed
            .keys()
            .copied()
            .filter(|l| {
                if *l == 0 {
                    self.rgba.is_empty()
                } else {
                    self.mipmaps.get(l).is_some_and(|m| m.2.is_empty())
                }
            })
            .collect();
        for level in levels {
            let decoded = self.rgba_level(level).and_then(|p| match p {
                std::borrow::Cow::Owned(v) => Some(v),
                _ => None,
            });
            if let Some(v) = decoded {
                if level == 0 {
                    self.rgba = v;
                } else if let Some(m) = self.mipmaps.get_mut(&level) {
                    m.2 = v;
                }
            }
        }
    }
    /// Internal formats survive RGBA decoding and automatic mip generation.
    pub fn set_format(&mut self, level: u32, format: u32) {
        self.formats.insert(level, format);
        if level == self.base_level
            && self.generate_mipmap
            && !self.compressed.contains_key(&self.base_level)
        {
            let q = self
                .sampling_size()
                .0
                .max(self.sampling_size().1)
                .checked_ilog2()
                .unwrap_or(0);
            for level in 1..=q {
                self.formats.insert(level + self.base_level, format);
            }
        }
    }

    pub fn set_compressed(
        &mut self,
        level: u32,
        width: u32,
        height: u32,
        format: u32,
        bytes: &[u8],
        rgba: Vec<u8>,
    ) {
        let generate = self.generate_mipmap;
        self.generate_mipmap = false;
        self.set_level(level, width, height, rgba);
        self.generate_mipmap = generate;
        self.formats.insert(level, format);
        self.compressed.insert(
            level,
            CompressedLevel {
                width,
                height,
                format,
                bytes: bytes.into(),
            },
        );
        if level == 0 {
            self.regenerate_mipmaps();
        }
    }

    pub fn level_size(&self, level: u32) -> Option<(u32, u32)> {
        if level == 0 {
            Some((self.width, self.height))
        } else {
            self.mipmaps.get(&level).map(|m| (m.0, m.1))
        }
    }
    pub fn sampling_size(&self) -> (u32, u32) {
        self.level_size(self.base_level).unwrap_or((0, 0))
    }
    pub fn last_level(&self) -> u32 {
        let (w, h) = self.sampling_size();
        self.base_level
            .saturating_add(w.max(h).checked_ilog2().unwrap_or(0))
            .min(self.max_level)
    }
    pub fn lod_bounds(&self) -> (f32, f32) {
        if self.goforce_lod && (self.min_lod != -1000. || self.max_lod != 1000.) {
            // The driver first stores 16.16, then adds 0x8000 and shifts
            // arithmetically when programming the sampler's level bounds.
            let rounded = |v: f32| {
                let fixed = (f64::from(v) * 65536.)
                    .round()
                    .clamp(i32::MIN as f64, i32::MAX as f64) as i64;
                ((fixed + 0x8000) >> 16) as f32
            };
            (rounded(self.min_lod), rounded(self.max_lod))
        } else {
            (self.min_lod, self.max_lod)
        }
    }

    /// A host texture starts at level zero. Rebase only on upload, keeping
    /// original guest images and shared native compressed payloads intact.
    pub(crate) fn host_view(&self) -> Self {
        let (w, h) = self.sampling_size();
        let b = self.base_level;
        Self {
            width: w,
            height: h,
            rgba: if b == 0 {
                self.rgba.clone()
            } else {
                self.mipmaps
                    .get(&b)
                    .map(|m| m.2.clone())
                    .unwrap_or_default()
            },
            mipmaps: self
                .mipmaps
                .iter()
                .filter(|(l, _)| **l > b && **l <= self.last_level())
                .map(|(l, m)| (*l - b, m.clone()))
                .collect(),
            compressed: self
                .compressed
                .iter()
                .filter(|(l, _)| **l >= b && **l <= self.last_level())
                .map(|(l, m)| (*l - b, m.clone()))
                .collect(),
            formats: self
                .formats
                .iter()
                .filter(|(l, _)| **l >= b)
                .map(|(l, f)| (*l - b, *f))
                .collect(),
            max_level: self.max_level.saturating_sub(b),
            base_level: 0,
            min_filter: self.min_filter,
            mag_filter: self.mag_filter,
            wrap_s: self.wrap_s,
            wrap_t: self.wrap_t,
            ..Self::default()
        }
    }

    /// Snapshot defined images without silently inventing missing mip levels.
    pub fn upload_levels(&self) -> Vec<(u32, u32, Vec<u8>)> {
        if !self.is_complete() {
            return Vec::new();
        }
        let last = if self.min_filter.uses_mipmaps() {
            self.last_level()
        } else {
            self.base_level
        };
        (self.base_level..=last)
            .map(|level| {
                let (w, h) = self.level_size(level).unwrap();
                (w, h, self.rgba_level(level).unwrap().into_owned())
            })
            .collect()
    }

    fn generated_levels(&self) -> Vec<(u32, u32, Vec<u8>)> {
        if !self.image_defined(
            self.base_level,
            self.sampling_size().0,
            self.sampling_size().1,
        ) {
            return Vec::new();
        }
        let mut levels = vec![(
            self.sampling_size().0,
            self.sampling_size().1,
            self.rgba_level(self.base_level).unwrap().into_owned(),
        )];
        while levels.last().is_some_and(|(w, h, _)| *w > 1 || *h > 1) {
            let (w, h, src) = levels.last().unwrap();
            let (nw, nh) = ((*w / 2).max(1), (*h / 2).max(1));
            let mut dst = vec![0; (nw * nh * 4) as usize];
            for y in 0..nh {
                for x in 0..nw {
                    for ch in 0..4 {
                        let mut sum = 0u32;
                        for dy in 0..2 {
                            for dx in 0..2 {
                                let sx = (x * 2 + dx).min(*w - 1);
                                let sy = (y * 2 + dy).min(*h - 1);
                                sum += src[((sy * *w + sx) * 4) as usize + ch] as u32;
                            }
                        }
                        dst[((y * nw + x) * 4) as usize + ch] = ((sum + 2) / 4) as u8;
                    }
                }
            }
            levels.push((nw, nh, dst));
        }
        levels
    }

    pub fn set_level(&mut self, level: u32, width: u32, height: u32, rgba: Vec<u8>) {
        self.compressed.remove(&level);
        self.formats.insert(level, GL_RGBA);
        if level == 0 {
            self.gpu_copy = None;
            self.width = width;
            self.height = height;
            self.rgba = rgba;
        } else {
            self.mipmaps.insert(level, (width, height, rgba));
        }
        if level == self.base_level {
            self.regenerate_mipmaps();
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// GL_GENERATE_MIPMAP creates actual level storage, not only a sampler
    /// approximation: subsequent glTexSubImage2D can address the new levels.
    pub fn regenerate_mipmaps(&mut self) {
        if !self.generate_mipmap
            || self.compressed.contains_key(&self.base_level)
            || !matches!(
                self.formats
                    .get(&self.base_level)
                    .copied()
                    .unwrap_or(GL_RGBA),
                GL_RGBA | GL_RGB | GL_ALPHA | GL_LUMINANCE | GL_LUMINANCE_ALPHA
            )
        {
            return;
        }
        let levels = self.generated_levels();
        for (level, (width, height, pixels)) in levels.into_iter().enumerate().skip(1) {
            let level = level as u32 + self.base_level;
            self.compressed.remove(&level);
            self.mipmaps.insert(level, (width, height, pixels));
            self.formats.insert(
                level,
                self.formats
                    .get(&self.base_level)
                    .copied()
                    .unwrap_or(GL_RGBA),
            );
        }
    }

    fn image_defined(&self, level: u32, width: u32, height: u32) -> bool {
        if width == 0 || height == 0 {
            return false;
        }
        let Some(size) = width.checked_mul(height).and_then(|n| n.checked_mul(4)) else {
            return false;
        };
        let pixels = if level == 0 {
            Some(self.rgba.as_slice())
        } else {
            self.mipmaps
                .get(&level)
                .filter(|m| m.0 == width && m.1 == height)
                .map(|m| m.2.as_slice())
        };
        (level == 0
            && self
                .gpu_copy
                .as_ref()
                .is_some_and(|frame| frame.size() == (width, height)))
            || pixels.is_some_and(|p| p.len() >= size as usize)
            || self.compressed.get(&level).is_some_and(|c| {
                c.width == width
                    && c.height == height
                    && compressed_image_size(c.format, width, height)
                        .is_some_and(|n| c.bytes.len() >= n)
            })
    }

    /// ES 1.1 section 3.7.9 with SGIS_texture_lod: mip filters require images
    /// through min(MAX_LEVEL, log2(max(width,height))),
    /// with consistent dimensions and internal formats. Nonmip filters need base only.
    pub fn is_complete(&self) -> bool {
        if self.base_level > self.max_level {
            return false;
        }
        let (w, h) = self.sampling_size();
        if !self.image_defined(self.base_level, w, h) {
            return false;
        }
        if !self.min_filter.uses_mipmaps() {
            return true;
        }
        let format = self
            .formats
            .get(&self.base_level)
            .copied()
            .unwrap_or(GL_RGBA);
        (self.base_level + 1..=self.last_level()).all(|level| {
            let shift = level - self.base_level;
            self.formats.get(&level).copied().unwrap_or(GL_RGBA) == format
                && self.image_defined(level, (w >> shift).max(1), (h >> shift).max(1))
        })
    }

    /// Sample a texel with nearest-neighbour filtering.
    ///
    /// Returns premultiplied-by-nothing straight RGBA. Out-of-range
    /// coordinates are resolved through the wrap modes.
    pub fn sample_nearest(&self, s: f32, t: f32) -> [u8; 4] {
        if !self.is_complete() {
            return [255; 4];
        }
        let (w, h) = self.sampling_size();
        let pixels = self.rgba_level(self.base_level).unwrap();
        let x = Self::wrap_coord(s, w, self.wrap_s);
        let y = Self::wrap_coord(t, h, self.wrap_t);
        let i = ((y * w + x) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
    }
    pub fn sample_linear(&self, s: f32, t: f32) -> [u8; 4] {
        if !self.is_complete() {
            return [255; 4];
        }
        let (w, h) = self.sampling_size();
        let pixels = self.rgba_level(self.base_level).unwrap();
        Self::sample_pixels(&pixels, w, h, self.wrap_s, self.wrap_t, s, t, true)
    }

    /// Legacy base-level adapter. Rendering with a known footprint uses
    /// sample_lod so minification and magnification remain distinct.
    pub fn sample(&self, s: f32, t: f32) -> [u8; 4] {
        match self.mag_filter {
            Filter::Nearest => self.sample_nearest(s, t),
            _ => self.sample_linear(s, t),
        }
    }

    /// Select minification/magnification and explicit/generated mip levels.
    /// `levels` is prepared once per revision, never generated per fragment.
    pub fn sample_lod(
        &self,
        levels: &[(u32, u32, Vec<u8>)],
        s: f32,
        t: f32,
        lambda: f32,
    ) -> [u8; 4] {
        if !self.is_complete() {
            return [255; 4];
        }
        self.sample_lod_complete(levels, s, t, lambda)
    }

    /// Caller has checked completeness once when constructing the active stages.
    pub(crate) fn sample_lod_complete(
        &self,
        levels: &[(u32, u32, Vec<u8>)],
        s: f32,
        t: f32,
        lambda: f32,
    ) -> [u8; 4] {
        let threshold = if self.mag_filter == Filter::Linear
            && matches!(
                self.min_filter,
                Filter::NearestMipmapNearest | Filter::NearestMipmapLinear
            ) {
            0.5
        } else {
            0.0
        };
        let lambda = if lambda.is_nan() { 0.0 } else { lambda };
        let (min_lod, max_lod) = self.lod_bounds();
        let lambda = lambda.max(min_lod).min(max_lod);
        let filter = if lambda <= threshold {
            self.mag_filter
        } else {
            self.min_filter
        };
        let linear = matches!(
            filter,
            Filter::Linear | Filter::LinearMipmapNearest | Filter::LinearMipmapLinear
        );
        let max_level = if filter.uses_mipmaps() {
            levels
                .len()
                .saturating_sub(1)
                .min(self.max_level.saturating_sub(self.base_level) as usize)
        } else {
            0
        };
        let lod = lambda.max(0.0).min(max_level as f32);
        let mip_linear = matches!(
            filter,
            Filter::NearestMipmapLinear | Filter::LinearMipmapLinear
        );
        let low = if mip_linear {
            lod.floor() as usize
        } else {
            (lod + 0.5).floor() as usize
        };
        let at = |level: usize| {
            if level == 0 && self.base_level == 0 && !self.rgba.is_empty() {
                Self::sample_pixels(
                    &self.rgba,
                    self.width,
                    self.height,
                    self.wrap_s,
                    self.wrap_t,
                    s,
                    t,
                    linear,
                )
            } else if let Some((w, h, pixels)) = levels.get(level) {
                Self::sample_pixels(pixels, *w, *h, self.wrap_s, self.wrap_t, s, t, linear)
            } else {
                Self::sample_pixels(
                    &self.rgba,
                    self.width,
                    self.height,
                    self.wrap_s,
                    self.wrap_t,
                    s,
                    t,
                    linear,
                )
            }
        };
        let a = at(low);
        if !mip_linear || low >= max_level {
            return a;
        }
        let b = at(low + 1);
        let weight = lod - low as f32;
        std::array::from_fn(|i| {
            (a[i] as f32 + (b[i] as f32 - a[i] as f32) * weight)
                .round()
                .clamp(0., 255.) as u8
        })
    }

    fn sample_pixels(
        pixels: &[u8],
        w: u32,
        h: u32,
        ws: Wrap,
        wt: Wrap,
        s: f32,
        t: f32,
        linear: bool,
    ) -> [u8; 4] {
        if w == 0
            || h == 0
            || pixels.len() < (w as usize).saturating_mul(h as usize).saturating_mul(4)
        {
            return [255; 4];
        }
        let texel = |x: i64, y: i64| {
            let x = Self::wrap_texel(x, w, ws);
            let y = Self::wrap_texel(y, h, wt);
            let i = ((y * w + x) * 4) as usize;
            [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
        };
        if !linear {
            return texel((s * w as f32).floor() as i64, (t * h as f32).floor() as i64);
        }
        let x = s * w as f32 - 0.5;
        let y = t * h as f32 - 0.5;
        let (ix, iy) = (x.floor(), y.floor());
        let (fx, fy) = (x - ix, y - iy);
        let a = texel(ix as i64, iy as i64);
        let b = texel(ix as i64 + 1, iy as i64);
        let c = texel(ix as i64, iy as i64 + 1);
        let d = texel(ix as i64 + 1, iy as i64 + 1);
        std::array::from_fn(|i| {
            let top = a[i] as f32 + (b[i] as f32 - a[i] as f32) * fx;
            let bottom = c[i] as f32 + (d[i] as f32 - c[i] as f32) * fx;
            (top + (bottom - top) * fy).round().clamp(0., 255.) as u8
        })
    }

    /// Resolve an integer texel coordinate through a wrap mode.
    fn wrap_texel(v: i64, size: u32, wrap: Wrap) -> u32 {
        if size == 0 {
            return 0;
        }
        match wrap {
            Wrap::Repeat => v.rem_euclid(size as i64) as u32,
            Wrap::ClampToEdge | Wrap::GizmondoClamp => v.clamp(0, size as i64 - 1) as u32,
        }
    }

    fn wrap_coord(v: f32, size: u32, wrap: Wrap) -> u32 {
        if size == 0 {
            return 0;
        }
        let scaled = v * size as f32;
        match wrap {
            Wrap::Repeat => {
                let m = (scaled.floor() as i64).rem_euclid(size as i64);
                m as u32
            }
            Wrap::ClampToEdge | Wrap::GizmondoClamp => {
                let c = scaled.floor();
                if c < 0.0 {
                    0
                } else if c >= size as f32 {
                    size - 1
                } else {
                    c as u32
                }
            }
        }
    }

    /// Apply a `glTexParameter*` value. Returns `false` if the
    /// enumerant is not one we recognise, which the caller reports as
    /// `GL_INVALID_ENUM`.
    pub fn set_parameter(&mut self, pname: u32, value: u32) -> bool {
        match pname {
            0x8191 => {
                self.generate_mipmap = value != 0;
                true
            }
            GL_TEXTURE_MIN_FILTER => {
                self.min_filter = match value {
                    GL_NEAREST => Filter::Nearest,
                    GL_LINEAR => Filter::Linear,
                    GL_NEAREST_MIPMAP_NEAREST => Filter::NearestMipmapNearest,
                    GL_LINEAR_MIPMAP_NEAREST => Filter::LinearMipmapNearest,
                    GL_NEAREST_MIPMAP_LINEAR => Filter::NearestMipmapLinear,
                    GL_LINEAR_MIPMAP_LINEAR => Filter::LinearMipmapLinear,
                    _ => return false,
                };
                true
            }
            GL_TEXTURE_MAG_FILTER => {
                self.mag_filter = match value {
                    GL_NEAREST => Filter::Nearest,
                    GL_LINEAR => Filter::Linear,
                    _ => return false,
                };
                true
            }
            GL_TEXTURE_WRAP_S => match wrap_from_enum(value) {
                Some(w) => {
                    self.wrap_s = w;
                    true
                }
                None => false,
            },
            GL_TEXTURE_WRAP_T => match wrap_from_enum(value) {
                Some(w) => {
                    self.wrap_t = w;
                    true
                }
                None => false,
            },
            GL_TEXTURE_MAX_LEVEL_SGIS => {
                if self.max_level != value {
                    self.max_level = value;
                    self.revision = self.revision.wrapping_add(1);
                }
                true
            }
            GL_TEXTURE_BASE_LEVEL_SGIS => {
                if self.base_level != value {
                    self.base_level = value;
                    self.revision = self.revision.wrapping_add(1);
                }
                true
            }
            GL_TEXTURE_MIN_LOD_SGIS => {
                self.min_lod = value as i32 as f32;
                true
            }
            GL_TEXTURE_MAX_LOD_SGIS => {
                self.max_lod = value as i32 as f32;
                true
            }
            GL_TEXTURE_LOD_BIAS_EXT => true,
            _ => false,
        }
    }
}

fn wrap_from_enum(value: u32) -> Option<Wrap> {
    match value {
        GL_REPEAT => Some(Wrap::Repeat),
        GL_CLAMP_TO_EDGE => Some(Wrap::ClampToEdge),
        _ => None,
    }
}

/// Number of bytes one texel occupies in the guest's buffer.
/// Validate enum membership separately from packed format/type compatibility.
pub fn pixel_layout(format: u32, ty: u32) -> Result<usize, u32> {
    let components = match format {
        GL_ALPHA | GL_LUMINANCE => 1,
        GL_LUMINANCE_ALPHA => 2,
        GL_RGB => 3,
        GL_RGBA => 4,
        _ => return Err(GL_INVALID_ENUM),
    };
    match ty {
        GL_UNSIGNED_BYTE => Ok(components),
        GL_UNSIGNED_SHORT_5_6_5 if format == GL_RGB => Ok(2),
        GL_UNSIGNED_SHORT_4_4_4_4 | GL_UNSIGNED_SHORT_5_5_5_1 if format == GL_RGBA => Ok(2),
        GL_UNSIGNED_SHORT_5_6_5 | GL_UNSIGNED_SHORT_4_4_4_4 | GL_UNSIGNED_SHORT_5_5_5_1 => {
            Err(GL_INVALID_OPERATION)
        }
        _ => Err(GL_INVALID_ENUM),
    }
}
pub fn bytes_per_texel(format: u32, ty: u32) -> Option<usize> {
    pixel_layout(format, ty).ok()
}

/// Expand a 5-bit channel to 8 bits, replicating the high bits so that
/// `0b11111` maps to `255` rather than `248`.
#[inline]
fn expand5(v: u16) -> u8 {
    let v = (v & 0x1F) as u8;
    (v << 3) | (v >> 2)
}

#[inline]
fn expand6(v: u16) -> u8 {
    let v = (v & 0x3F) as u8;
    (v << 2) | (v >> 4)
}

#[inline]
fn expand4(v: u16) -> u8 {
    let v = (v & 0x0F) as u8;
    (v << 4) | v
}

/// Decode a guest texel buffer into RGBA8888.
///
/// Returns `None` for an unsupported format/type pair or a short snapshot.
/// Callers use `pixel_layout` first to distinguish the GL error categories.
pub fn decode_to_rgba(
    data: &[u8],
    width: u32,
    height: u32,
    format: u32,
    ty: u32,
) -> Option<Vec<u8>> {
    let texel_bytes = bytes_per_texel(format, ty)?;
    let count = (width as usize).checked_mul(height as usize)?;
    let needed = count.checked_mul(texel_bytes)?;
    if data.len() < needed {
        return None;
    }
    let mut out = Vec::with_capacity(count * 4);
    match ty {
        GL_UNSIGNED_SHORT_5_6_5 => {
            for i in 0..count {
                let p = u16::from_le_bytes([data[i * 2], data[i * 2 + 1]]);
                out.extend_from_slice(&[expand5(p >> 11), expand6(p >> 5), expand5(p), 255]);
            }
        }
        GL_UNSIGNED_SHORT_4_4_4_4 => {
            for i in 0..count {
                let p = u16::from_le_bytes([data[i * 2], data[i * 2 + 1]]);
                out.extend_from_slice(&[
                    expand4(p >> 12),
                    expand4(p >> 8),
                    expand4(p >> 4),
                    expand4(p),
                ]);
            }
        }
        GL_UNSIGNED_SHORT_5_5_5_1 => {
            for i in 0..count {
                let p = u16::from_le_bytes([data[i * 2], data[i * 2 + 1]]);
                let a = if p & 1 != 0 { 255 } else { 0 };
                out.extend_from_slice(&[expand5(p >> 11), expand5(p >> 6), expand5(p >> 1), a]);
            }
        }
        GL_UNSIGNED_BYTE => match format {
            GL_RGBA => out.extend_from_slice(&data[..needed]),
            GL_RGB => {
                for i in 0..count {
                    let s = i * 3;
                    out.extend_from_slice(&[data[s], data[s + 1], data[s + 2], 255]);
                }
            }
            GL_LUMINANCE => {
                for &l in data.iter().take(count) {
                    out.extend_from_slice(&[l, l, l, 255]);
                }
            }
            GL_LUMINANCE_ALPHA => {
                for i in 0..count {
                    let l = data[i * 2];
                    out.extend_from_slice(&[l, l, l, data[i * 2 + 1]]);
                }
            }
            GL_ALPHA => {
                for &a in data.iter().take(count) {
                    out.extend_from_slice(&[255, 255, 255, a]);
                }
            }
            _ => return None,
        },
        _ => return None,
    }
    Some(out)
}

// ---- ATITC (AMD ATC) ------------------------------------------------------
//
// ATC stores 4x4 blocks. The RGB block is eight bytes laid out like
// DXT1 — two endpoint colours then sixteen two-bit selectors — but the
// endpoints are derived differently, and the top bit of the first
// endpoint picks between two interpolation modes rather than signalling
// punch-through alpha. `EXPLICIT_ALPHA` prefixes each block with eight
// bytes of DXT3-style 4-bit alpha; `INTERPOLATED_ALPHA` prefixes it
// with a DXT5-style alpha block.

/// Blocks needed to cover `width` x `height`, rounding up on both axes.
#[inline]
fn block_counts(width: u32, height: u32) -> (usize, usize) {
    (width.div_ceil(4) as usize, height.div_ceil(4) as usize)
}

/// Whether the source contains a shared OES color palette.
pub fn is_paletted(format: u32) -> bool {
    (GL_PALETTE4_RGB8_OES..=GL_PALETTE8_RGB5_A1_OES).contains(&format)
}
pub fn palette_bytes(format: u32) -> Option<usize> {
    if !is_paletted(format) {
        return None;
    }
    let entries = if format >= GL_PALETTE8_RGB8_OES {
        256
    } else {
        16
    };
    let bytes = match format {
        GL_PALETTE4_RGB8_OES | GL_PALETTE8_RGB8_OES => 3,
        GL_PALETTE4_RGBA8_OES | GL_PALETTE8_RGBA8_OES => 4,
        _ => 2,
    };
    Some(entries * bytes)
}

/// A paletted chain contains one shared palette, not one palette per level.
pub fn compressed_upload_size(format: u32, level: i32, width: u32, height: u32) -> Option<usize> {
    if !(-31..=31).contains(&level)
        || (is_paletted(format) && level > 0)
        || (!is_paletted(format) && level < 0)
    {
        return None;
    }
    // PVRTC retains its format-specific minimum allocation, even for an
    // empty image; block formats instead have zero blocks on an empty axis.
    if (GL_COMPRESSED_RGB_PVRTC_4BPPV1_IMG..=GL_COMPRESSED_RGBA_PVRTC_2BPPV1_IMG).contains(&format)
    {
        return compressed_image_size(format, width.max(1), height.max(1));
    }
    if let Some(palette) = palette_bytes(format) {
        let mut size = palette;
        for l in 0..=level.unsigned_abs() {
            let (w, h) = ((width >> l).max(1), (height >> l).max(1));
            if width != 0 && height != 0 {
                size =
                    size.checked_add(compressed_image_size(format, w, h)?.checked_sub(palette)?)?;
            }
        }
        Some(size)
    } else if width == 0 || height == 0 {
        compressed_image_size(format, 1, 1)?;
        Some(0)
    } else {
        compressed_image_size(format, width, height)
    }
}

/// S3TC formats accepted by EXT_dxt1 and ANGLE_dxt3/dxt5 uploads.
pub fn is_s3tc(format: u32) -> bool {
    (GL_COMPRESSED_RGB_S3TC_DXT1_EXT..=GL_COMPRESSED_RGBA_S3TC_DXT5_EXT).contains(&format)
}

/// Bytes for a single encoded image, including its palette when present.
pub fn compressed_image_size(format: u32, width: u32, height: u32) -> Option<usize> {
    if (GL_COMPRESSED_RGB_PVRTC_4BPPV1_IMG..=GL_COMPRESSED_RGBA_PVRTC_2BPPV1_IMG).contains(&format)
    {
        if !width.is_power_of_two() || !height.is_power_of_two() {
            return None;
        }
        let two = format & 1 != 0;
        let w = width.max(if two { 16 } else { 8 }) as usize;
        let h = height.max(8) as usize;
        return w.checked_mul(h)?.checked_div(if two { 4 } else { 2 });
    }

    let per_block = match format {
        GL_ATC_RGB_AMD | GL_COMPRESSED_RGB_S3TC_DXT1_EXT | GL_COMPRESSED_RGBA_S3TC_DXT1_EXT => 8,
        GL_ATC_RGBA_EXPLICIT_ALPHA_AMD
        | GL_ATC_RGBA_INTERPOLATED_ALPHA_AMD
        | GL_COMPRESSED_RGBA_S3TC_DXT3_EXT
        | GL_COMPRESSED_RGBA_S3TC_DXT5_EXT => 16,
        GL_PALETTE4_RGB8_OES
        | GL_PALETTE4_RGBA8_OES
        | GL_PALETTE4_R5_G6_B5_OES
        | GL_PALETTE4_RGBA4_OES
        | GL_PALETTE4_RGB5_A1_OES => 0,
        GL_PALETTE8_RGB8_OES
        | GL_PALETTE8_RGBA8_OES
        | GL_PALETTE8_R5_G6_B5_OES
        | GL_PALETTE8_RGBA4_OES
        | GL_PALETTE8_RGB5_A1_OES => 0,
        _ => return None,
    };
    if matches!(
        format,
        GL_PALETTE4_RGB8_OES
            | GL_PALETTE4_RGBA8_OES
            | GL_PALETTE4_R5_G6_B5_OES
            | GL_PALETTE4_RGBA4_OES
            | GL_PALETTE4_RGB5_A1_OES
            | GL_PALETTE8_RGB8_OES
            | GL_PALETTE8_RGBA8_OES
            | GL_PALETTE8_R5_G6_B5_OES
            | GL_PALETTE8_RGBA4_OES
            | GL_PALETTE8_RGB5_A1_OES
    ) {
        let entries: usize = if format >= GL_PALETTE8_RGB8_OES {
            256
        } else {
            16
        };
        let entry_bytes = if matches!(format, GL_PALETTE4_RGB8_OES | GL_PALETTE8_RGB8_OES) {
            3
        } else if matches!(format, GL_PALETTE4_RGBA8_OES | GL_PALETTE8_RGBA8_OES) {
            4
        } else {
            2
        };
        let index_bytes = if format >= GL_PALETTE8_RGB8_OES {
            (width as usize).checked_mul(height as usize)?
        } else {
            (width as usize).checked_mul(height as usize)?.div_ceil(2)
        };
        return entries.checked_mul(entry_bytes)?.checked_add(index_bytes);
    }
    let (bw, bh) = block_counts(width, height);
    bw.checked_mul(bh)?.checked_mul(per_block)
}

/// Decode one eight-byte DXT1 RGB block into sixteen RGB triples in raster order within the block.
fn dxt_rgb_block(src: &[u8], force_four: bool) -> ([[u8; 3]; 16], [u8; 16]) {
    let c0 = u16::from_le_bytes([src[0], src[1]]);
    let c1 = u16::from_le_bytes([src[2], src[3]]);
    let bits = u32::from_le_bytes([src[4], src[5], src[6], src[7]]);
    let e0 = [expand5(c0 >> 11), expand6(c0 >> 5), expand5(c0)];
    let e1 = [expand5(c1 >> 11), expand6(c1 >> 5), expand5(c1)];
    let mut palette = [[0u8; 3]; 4];
    palette[0] = e0;
    palette[1] = e1;
    if force_four || c0 > c1 {
        let [e0, e1, p2, p3] = &mut palette;
        for ((p2, p3), (p0, p1)) in p2
            .iter_mut()
            .zip(p3.iter_mut())
            .zip(e0.iter().zip(e1.iter()))
        {
            *p2 = ((2 * u32::from(*p0) + u32::from(*p1)) / 3) as u8;
            *p3 = ((u32::from(*p0) + 2 * u32::from(*p1)) / 3) as u8;
        }
    } else {
        let [e0, e1, p2, _] = &mut palette;
        for (p2, (p0, p1)) in p2.iter_mut().zip(e0.iter().zip(e1.iter())) {
            *p2 = ((u32::from(*p0) + u32::from(*p1)) / 2) as u8;
        }
        palette[3] = [0, 0, 0];
    }
    let mut rgb = [[0u8; 3]; 16];
    let mut alpha = [255u8; 16];
    for i in 0..16 {
        let selector = ((bits >> (i * 2)) & 3) as usize;
        rgb[i] = palette[selector];
        if !force_four && c0 <= c1 && selector == 3 {
            alpha[i] = 0;
        }
    }
    (rgb, alpha)
}

/// Decode one eight-byte ATC RGB block into sixteen RGB triples in
/// raster order within the block.
fn atc_rgb_block(src: &[u8]) -> [[u8; 3]; 16] {
    let c0 = u16::from_le_bytes([src[0], src[1]]);
    let c1 = u16::from_le_bytes([src[2], src[3]]);
    let bits = u32::from_le_bytes([src[4], src[5], src[6], src[7]]);

    // The first endpoint is RGB555 with the mode in bit 15; the second
    // is always RGB565.
    let e0 = [expand5(c0 >> 10), expand5(c0 >> 5), expand5(c0)];
    let e1 = [expand5(c1 >> 11), expand6(c1 >> 5), expand5(c1)];

    let mut pal = [[0u8; 3]; 4];
    if c0 & 0x8000 == 0 {
        // Mode 0: both endpoints are used, with the two interior
        // colours at 3/8 and 5/8 — the same split DXT1 uses at 1/3.
        pal[0] = e0;
        pal[3] = e1;
        for ch in 0..3 {
            let (a, b) = (e0[ch] as u32, e1[ch] as u32);
            pal[1][ch] = ((a * 5 + b * 3) / 8) as u8;
            pal[2][ch] = ((a * 3 + b * 5) / 8) as u8;
        }
    } else {
        // Mode 1: selector 0 is forced to black, which is how ATC
        // encodes blocks that need a hard dark edge without spending an
        // endpoint on it.
        pal[0] = [0, 0, 0];
        pal[2] = e0;
        pal[3] = e1;
        for ch in 0..3 {
            pal[1][ch] = (e0[ch] as u32).saturating_sub(e1[ch] as u32 / 4) as u8;
        }
    }

    let mut out = [[0u8; 3]; 16];
    for (i, texel) in out.iter_mut().enumerate() {
        *texel = pal[((bits >> (i * 2)) & 3) as usize];
    }
    out
}

/// Decode one eight-byte DXT5-style interpolated alpha block into
/// sixteen alpha values.
fn interpolated_alpha_block(src: &[u8]) -> [u8; 16] {
    let (a0, a1) = (src[0], src[1]);
    let mut pal = [0u8; 8];
    pal[0] = a0;
    pal[1] = a1;
    if a0 > a1 {
        for (i, slot) in pal.iter_mut().enumerate().take(8).skip(2) {
            let (w0, w1) = ((8 - i) as u32, (i - 1) as u32);
            *slot = ((a0 as u32 * w0 + a1 as u32 * w1) / 7) as u8;
        }
    } else {
        for (i, slot) in pal.iter_mut().enumerate().take(6).skip(2) {
            let (w0, w1) = ((6 - i) as u32, (i - 1) as u32);
            *slot = ((a0 as u32 * w0 + a1 as u32 * w1) / 5) as u8;
        }
        pal[6] = 0;
        pal[7] = 255;
    }
    // Sixteen three-bit selectors packed into the remaining six bytes.
    let bits = src[2..8]
        .iter()
        .enumerate()
        .fold(0u64, |acc, (i, &b)| acc | ((b as u64) << (i * 8)));
    let mut out = [0u8; 16];
    for (i, a) in out.iter_mut().enumerate() {
        *a = pal[((bits >> (i * 3)) & 7) as usize];
    }
    out
}

fn decode_paletted(data: &[u8], width: u32, height: u32, format: u32) -> Option<Vec<u8>> {
    let is8 = format >= GL_PALETTE8_RGB8_OES;
    let entries = if is8 { 256usize } else { 16usize };
    let rgba_palette = matches!(format, GL_PALETTE4_RGBA8_OES | GL_PALETTE8_RGBA8_OES);
    let rgb8_palette = matches!(format, GL_PALETTE4_RGB8_OES | GL_PALETTE8_RGB8_OES);
    let entry_bytes = if rgba_palette {
        4
    } else if rgb8_palette {
        3
    } else {
        2
    };
    let palette_len = entries.checked_mul(entry_bytes)?;
    let indices_len = if is8 {
        (width as usize).checked_mul(height as usize)?
    } else {
        (width as usize).checked_mul(height as usize)?.div_ceil(2)
    };
    if data.len() < palette_len + indices_len {
        return None;
    }
    let mut out = vec![
        0u8;
        (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?
    ];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let idx = if is8 {
                data[palette_len + y * width as usize + x] as usize
            } else {
                let pixel = y * width as usize + x;
                let b = data[palette_len + pixel / 2];
                if pixel & 1 == 0 {
                    (b >> 4) as usize
                } else {
                    (b & 0x0f) as usize
                }
            };
            let po = idx.checked_mul(entry_bytes)?;
            let (r, g, b, a) = if rgba_palette {
                (data[po], data[po + 1], data[po + 2], data[po + 3])
            } else if rgb8_palette {
                (data[po], data[po + 1], data[po + 2], 255)
            } else {
                let v = u16::from_le_bytes([data[po], data[po + 1]]);
                match format {
                    GL_PALETTE4_R5_G6_B5_OES | GL_PALETTE8_R5_G6_B5_OES => {
                        (expand5(v >> 11), expand6(v >> 5), expand5(v), 255)
                    }
                    GL_PALETTE4_RGBA4_OES | GL_PALETTE8_RGBA4_OES => (
                        expand4(v >> 12),
                        expand4(v >> 8),
                        expand4(v >> 4),
                        expand4(v),
                    ),
                    GL_PALETTE4_RGB5_A1_OES | GL_PALETTE8_RGB5_A1_OES => (
                        expand5(v >> 11),
                        expand5(v >> 6),
                        expand5(v >> 1),
                        if v & 1 == 0 { 0 } else { 255 },
                    ),
                    _ => return None,
                }
            };
            let o = (y * width as usize + x) * 4;
            out[o..o + 4].copy_from_slice(&[r, g, b, a]);
        }
    }
    Some(out)
}

/// Decode a supported block-compressed or paletted image into RGBA8888.
///
/// Returns `None` if the format is unsupported or the guest handed us fewer
/// bytes than the block layout needs.
pub fn decode_compressed_to_rgba(
    data: &[u8],
    width: u32,
    height: u32,
    format: u32,
) -> Option<Vec<u8>> {
    if width == 0 || height == 0 {
        return Some(Vec::new());
    }
    let needed = compressed_image_size(format, width, height)?;
    if data.len() < needed {
        return None;
    }

    if (GL_COMPRESSED_RGB_PVRTC_4BPPV1_IMG..=GL_COMPRESSED_RGBA_PVRTC_2BPPV1_IMG).contains(&format)
    {
        let two = format & 1 != 0;
        let w = width.max(if two { 16 } else { 8 }) as usize;
        let h = height.max(8) as usize;
        let mut pixels = vec![0u32; w.checked_mul(h)?];
        texture2ddecoder::decode_pvrtc(&data[..needed], w, h, &mut pixels, two).ok()?;
        let rgb = matches!(
            format,
            GL_COMPRESSED_RGB_PVRTC_4BPPV1_IMG | GL_COMPRESSED_RGB_PVRTC_2BPPV1_IMG
        );
        let mut out = Vec::with_capacity(
            (width as usize)
                .checked_mul(height as usize)?
                .checked_mul(4)?,
        );
        for y in 0..height as usize {
            for x in 0..width as usize {
                // The decoder returns BGRA byte order; PocketHLE stores RGBA.
                let [b, g, r, a] = pixels[y * w + x].to_le_bytes();
                out.extend_from_slice(&[r, g, b, if rgb { 255 } else { a }]);
            }
        }
        return Some(out);
    }
    if matches!(
        format,
        GL_PALETTE4_RGB8_OES
            | GL_PALETTE4_RGBA8_OES
            | GL_PALETTE4_R5_G6_B5_OES
            | GL_PALETTE4_RGBA4_OES
            | GL_PALETTE4_RGB5_A1_OES
            | GL_PALETTE8_RGB8_OES
            | GL_PALETTE8_RGBA8_OES
            | GL_PALETTE8_R5_G6_B5_OES
            | GL_PALETTE8_RGBA4_OES
            | GL_PALETTE8_RGB5_A1_OES
    ) {
        return decode_paletted(data, width, height, format);
    }
    let stride = match format {
        GL_ATC_RGB_AMD | GL_COMPRESSED_RGB_S3TC_DXT1_EXT | GL_COMPRESSED_RGBA_S3TC_DXT1_EXT => 8,
        _ => 16,
    };
    let (bw, bh) = block_counts(width, height);
    let (w, h) = (width as usize, height as usize);
    let mut out = vec![0u8; w.checked_mul(h)?.checked_mul(4)?];

    for by in 0..bh {
        for bx in 0..bw {
            let block = &data[(by * bw + bx) * stride..];
            let (rgb, alpha) = match format {
                GL_ATC_RGB_AMD => (atc_rgb_block(block), [255u8; 16]),
                GL_COMPRESSED_RGB_S3TC_DXT1_EXT | GL_COMPRESSED_RGBA_S3TC_DXT1_EXT => {
                    let (rgb, alpha) = dxt_rgb_block(block, false);
                    (
                        rgb,
                        if format == GL_COMPRESSED_RGB_S3TC_DXT1_EXT {
                            [255; 16]
                        } else {
                            alpha
                        },
                    )
                }
                GL_COMPRESSED_RGBA_S3TC_DXT3_EXT => {
                    let mut alpha = [0; 16];
                    for (i, a) in alpha.iter_mut().enumerate() {
                        *a = expand4((block[i / 2] >> (4 * (i % 2))) as u16);
                    }
                    (dxt_rgb_block(&block[8..], true).0, alpha)
                }
                GL_COMPRESSED_RGBA_S3TC_DXT5_EXT => (
                    dxt_rgb_block(&block[8..], true).0,
                    interpolated_alpha_block(block),
                ),
                GL_ATC_RGBA_EXPLICIT_ALPHA_AMD => {
                    let mut a = [0u8; 16];
                    for (i, slot) in a.iter_mut().enumerate() {
                        let nibble = block[i / 2] >> (4 * (i % 2));
                        *slot = expand4(nibble as u16);
                    }
                    (atc_rgb_block(&block[8..]), a)
                }
                _ => (atc_rgb_block(&block[8..]), interpolated_alpha_block(block)),
            };
            // Blocks at the right and bottom edges hang over the image
            // when the dimensions are not multiples of four; the
            // overhanging texels are simply dropped.
            for ty in 0..4 {
                let y = by * 4 + ty;
                if y >= h {
                    break;
                }
                for tx in 0..4 {
                    let x = bx * 4 + tx;
                    if x >= w {
                        break;
                    }
                    let i = ty * 4 + tx;
                    let o = (y * w + x) * 4;
                    out[o..o + 3].copy_from_slice(&rgb[i]);
                    out[o + 3] = alpha[i];
                }
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dxt3_explicit_alpha_and_four_colour_reverse_endpoints() {
        let mut block = [0u8; 16];
        for i in 0..8 {
            block[i] = (i * 2) as u8 | (((i * 2 + 1) as u8) << 4);
        }
        block[10] = 255;
        block[11] = 255; // c0=black < c1=white
        block[12..].fill(255); // selector 3 must be 2/3 white, never transparent
        let rgba =
            decode_compressed_to_rgba(&block, 4, 4, GL_COMPRESSED_RGBA_S3TC_DXT3_EXT).unwrap();
        for i in 0..16 {
            assert_eq!(&rgba[i * 4..i * 4 + 4], &[170, 170, 170, i as u8 * 17]);
        }
        assert_eq!(
            decode_compressed_to_rgba(&block, 3, 2, GL_COMPRESSED_RGBA_S3TC_DXT3_EXT)
                .unwrap()
                .len(),
            24
        );
        assert!(
            decode_compressed_to_rgba(&block[..15], 4, 4, GL_COMPRESSED_RGBA_S3TC_DXT3_EXT)
                .is_none()
        );
    }
    #[test]
    fn mip_chain_requires_defined_children_and_preserves_independent_images() {
        let mut t = Texture::default();
        t.set_level(0, 4, 4, [255, 0, 0, 255].repeat(16));
        t.set_level(1, 2, 2, [0, 255, 0, 255].repeat(4));
        assert!(!t.is_complete());
        assert!(t.upload_levels().is_empty());
        t.set_level(2, 1, 1, vec![0, 0, 255, 255]);
        let chain = t.upload_levels();
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[1].2, [0, 255, 0, 255].repeat(4));
        assert_eq!(chain[2].2, [0, 0, 255, 255]);
        t.set_level(0, 2, 2, [255; 4].repeat(4));
        assert_eq!(t.mipmaps.len(), 2);
        assert!(!t.is_complete()); // former 2x2 level 1 no longer matches the base.
        t.set_level(1, 1, 1, vec![255; 4]);
        assert!(t.is_complete()); // unused level 2 does not affect completeness.
    }

    #[test]
    fn paletted_rgba8_4bit_decodes_palette_before_indices() {
        let mut data = vec![0u8; 16 * 4 + 1];
        data[0..4].copy_from_slice(&[255, 0, 0, 255]);
        data[4..8].copy_from_slice(&[0, 255, 0, 255]);
        data[64] = 0x10;
        let out = decode_compressed_to_rgba(&data, 2, 1, GL_PALETTE4_RGBA8_OES).unwrap();
        assert_eq!(out, vec![0, 255, 0, 255, 255, 0, 0, 255]);
    }

    #[test]
    fn paletted_rgb565_8bit_decodes_packed_palette() {
        let mut data = vec![0u8; 256 * 2 + 2];
        data[0..2].copy_from_slice(&0xF800u16.to_le_bytes());
        data[2..4].copy_from_slice(&0x001Fu16.to_le_bytes());
        data[512] = 0;
        data[513] = 1;
        let out = decode_compressed_to_rgba(&data, 2, 1, GL_PALETTE8_R5_G6_B5_OES).unwrap();
        assert_eq!(out, vec![255, 0, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn rgb565_expands_to_full_range() {
        // Pure white in 5:6:5 must decode to 255,255,255 — not
        // 248,252,248, which is what a naive `<< 3` shift produces and
        // which makes every bright surface visibly dingy.
        let white = 0xFFFFu16.to_le_bytes();
        let out = decode_to_rgba(&white, 1, 1, GL_RGB, GL_UNSIGNED_SHORT_5_6_5).unwrap();
        assert_eq!(out, vec![255, 255, 255, 255]);
    }

    #[test]
    fn rgb565_channel_order_is_r_g_b() {
        // 0xF800 = red only.
        let red = 0xF800u16.to_le_bytes();
        let out = decode_to_rgba(&red, 1, 1, GL_RGB, GL_UNSIGNED_SHORT_5_6_5).unwrap();
        assert_eq!(out, vec![255, 0, 0, 255]);
        // 0x001F = blue only.
        let blue = 0x001Fu16.to_le_bytes();
        let out = decode_to_rgba(&blue, 1, 1, GL_RGB, GL_UNSIGNED_SHORT_5_6_5).unwrap();
        assert_eq!(out, vec![0, 0, 255, 255]);
    }

    #[test]
    fn rgba4444_decodes_alpha() {
        // 0xF00F: red=F, g=0, b=0, a=F
        let px = 0xF00Fu16.to_le_bytes();
        let out = decode_to_rgba(&px, 1, 1, GL_RGBA, GL_UNSIGNED_SHORT_4_4_4_4).unwrap();
        assert_eq!(out, vec![255, 0, 0, 255]);
    }

    #[test]
    fn rgba5551_alpha_is_one_bit() {
        let opaque = 0x0001u16.to_le_bytes();
        let out = decode_to_rgba(&opaque, 1, 1, GL_RGBA, GL_UNSIGNED_SHORT_5_5_5_1).unwrap();
        assert_eq!(out.len(), 4);
        assert_eq!(out[3], 255);
        let transparent = 0x0000u16.to_le_bytes();
        let out = decode_to_rgba(&transparent, 1, 1, GL_RGBA, GL_UNSIGNED_SHORT_5_5_5_1).unwrap();
        assert_eq!(out[3], 0);
    }

    #[test]
    fn rgb_ubyte_gains_opaque_alpha() {
        let out = decode_to_rgba(&[10, 20, 30], 1, 1, GL_RGB, GL_UNSIGNED_BYTE).unwrap();
        assert_eq!(out, vec![10, 20, 30, 255]);
    }

    #[test]
    fn truncated_buffer_is_rejected() {
        // A 2×2 RGBA texture needs 16 bytes; 8 must not be read past.
        assert!(decode_to_rgba(&[0u8; 8], 2, 2, GL_RGBA, GL_UNSIGNED_BYTE).is_none());
    }

    #[test]
    fn unsupported_format_is_rejected() {
        assert!(decode_to_rgba(&[0u8; 4], 1, 1, 0x9999, GL_UNSIGNED_BYTE).is_none());
    }

    #[test]
    fn repeat_wraps_negative_coordinates() {
        let mut t = Texture {
            width: 4,
            height: 1,
            rgba: vec![0; 16],
            ..Default::default()
        };
        t.wrap_s = Wrap::Repeat;
        // -0.25 in a 4-texel row is texel 3, not a clamp to 0.
        assert_eq!(Texture::wrap_coord(-0.25, 4, Wrap::Repeat), 3);
        assert_eq!(Texture::wrap_coord(1.25, 4, Wrap::Repeat), 1);
    }

    #[test]
    fn clamp_to_edge_saturates() {
        assert_eq!(Texture::wrap_coord(-5.0, 4, Wrap::ClampToEdge), 0);
        assert_eq!(Texture::wrap_coord(99.0, 4, Wrap::ClampToEdge), 3);
    }

    #[test]
    fn incomplete_texture_samples_white() {
        let t = Texture::default();
        assert_eq!(t.sample_nearest(0.5, 0.5), [255, 255, 255, 255]);
        assert_eq!(t.sample_linear(0.5, 0.5), [255, 255, 255, 255]);
    }

    /// A 2x1 texture, black then white, clamped so the edges do not
    /// wrap around into each other.
    fn ramp_2x1() -> Texture {
        Texture {
            width: 2,
            height: 1,
            rgba: vec![0, 0, 0, 0, 255, 255, 255, 255],
            min_filter: Filter::Linear,
            wrap_s: Wrap::ClampToEdge,
            wrap_t: Wrap::ClampToEdge,
            ..Default::default()
        }
    }

    #[test]
    fn linear_blends_halfway_between_texel_centres() {
        let t = ramp_2x1();
        // Texel centres sit at s = 0.25 and 0.75; sampling exactly
        // between them must return the midpoint of the two colours.
        assert_eq!(t.sample_linear(0.5, 0.5), [128, 128, 128, 128]);
        // And a quarter of the way across is a quarter of the blend.
        assert_eq!(t.sample_linear(0.375, 0.5), [64, 64, 64, 64]);
    }

    #[test]
    fn linear_reproduces_texel_colours_at_their_centres() {
        let t = ramp_2x1();
        assert_eq!(t.sample_linear(0.25, 0.5), [0, 0, 0, 0]);
        assert_eq!(t.sample_linear(0.75, 0.5), [255, 255, 255, 255]);
    }

    #[test]
    fn linear_clamps_rather_than_reading_out_of_bounds() {
        let t = ramp_2x1();
        // Left of the first texel centre there is nothing to blend
        // with, so ClampToEdge repeats the edge texel.
        assert_eq!(t.sample_linear(0.0, 0.5), [0, 0, 0, 0]);
        assert_eq!(t.sample_linear(1.0, 0.5), [255, 255, 255, 255]);
    }

    #[test]
    fn linear_wraps_across_the_seam_when_repeating() {
        let mut t = ramp_2x1();
        t.wrap_s = Wrap::Repeat;
        // At s = 0 the sample straddles the seam, blending texel 1
        // (white, wrapped from the right edge) with texel 0 (black).
        assert_eq!(t.sample_linear(0.0, 0.5), [128, 128, 128, 128]);
    }

    #[test]
    fn sample_dispatches_on_the_requested_filter() {
        let mut t = ramp_2x1();
        t.mag_filter = Filter::Nearest;
        assert_eq!(t.sample(0.5, 0.5), [255, 255, 255, 255]);
        t.mag_filter = Filter::Linear;
        assert_eq!(t.sample(0.5, 0.5), [128, 128, 128, 128]);
    }

    #[test]
    fn mipmap_min_filters_preserve_level_selection() {
        let mut t = Texture::default();
        assert!(t.set_parameter(GL_TEXTURE_MIN_FILTER, GL_LINEAR_MIPMAP_LINEAR));
        assert_eq!(t.min_filter, Filter::LinearMipmapLinear);
        assert!(t.set_parameter(GL_TEXTURE_MIN_FILTER, GL_NEAREST_MIPMAP_NEAREST));
        assert_eq!(t.min_filter, Filter::NearestMipmapNearest);
    }

    #[test]
    fn unknown_parameter_is_rejected() {
        let mut t = Texture::default();
        assert!(!t.set_parameter(0x9999, GL_LINEAR));
        assert!(!t.set_parameter(GL_TEXTURE_MAG_FILTER, 0x9999));
    }

    /// Build one ATC RGB block: two endpoints and sixteen selectors.
    fn atc_block(c0: u16, c1: u16, sel: u32) -> [u8; 8] {
        let mut b = [0u8; 8];
        b[0..2].copy_from_slice(&c0.to_le_bytes());
        b[2..4].copy_from_slice(&c1.to_le_bytes());
        b[4..8].copy_from_slice(&sel.to_le_bytes());
        b
    }

    #[test]
    fn atc_image_size_follows_the_block_layout() {
        // 8 bytes per 4x4 block for RGB, 16 with an alpha block.
        assert_eq!(compressed_image_size(GL_ATC_RGB_AMD, 8, 8), Some(4 * 8));
        assert_eq!(
            compressed_image_size(GL_ATC_RGBA_EXPLICIT_ALPHA_AMD, 8, 8),
            Some(4 * 16)
        );
        // Non-multiples of four round up to whole blocks.
        assert_eq!(compressed_image_size(GL_ATC_RGB_AMD, 5, 1), Some(2 * 8));
        // Anything else is not ours to decode.
        assert_eq!(compressed_image_size(GL_RGBA, 8, 8), None);
    }

    #[test]
    fn atc_mode0_interpolates_between_both_endpoints() {
        // Mode 0 (top bit of c0 clear): white RGB555 endpoint and black
        // RGB565 endpoint, with the four selectors 0..3 in texels 0..3.
        let block = atc_block(0x7FFF, 0x0000, 0b11_10_01_00);
        let out = decode_compressed_to_rgba(&block, 4, 1, GL_ATC_RGB_AMD).unwrap();
        // Selector 0 is endpoint 0, selector 3 is endpoint 1, and the
        // interior colours sit at 5/8 and 3/8 rather than DXT1's 2/3.
        assert_eq!(&out[0..4], &[255, 255, 255, 255]);
        assert_eq!(&out[4..8], &[159, 159, 159, 255]);
        assert_eq!(&out[8..12], &[95, 95, 95, 255]);
        assert_eq!(&out[12..16], &[0, 0, 0, 255]);
    }

    #[test]
    fn atc_mode1_forces_selector_zero_to_black() {
        // Mode 1 (top bit of c0 set) spends no endpoint on black, so
        // selector 0 is black regardless of what the endpoints hold.
        let block = atc_block(0x8000 | 0x7FFF, 0xFFFF, 0b00_00_00_00);
        let out = decode_compressed_to_rgba(&block, 4, 1, GL_ATC_RGB_AMD).unwrap();
        assert_eq!(&out[0..4], &[0, 0, 0, 255]);
    }

    #[test]
    fn atc_explicit_alpha_reads_one_nibble_per_texel() {
        let mut block = [0u8; 16];
        // Texel 0 alpha = 0x0, texel 1 = 0xF, texel 2 = 0x8.
        block[0] = 0xF0;
        block[1] = 0x08;
        block[8..16].copy_from_slice(&atc_block(0x7FFF, 0x7FFF, 0));
        let out = decode_compressed_to_rgba(&block, 4, 1, GL_ATC_RGBA_EXPLICIT_ALPHA_AMD).unwrap();
        assert_eq!(out[3], 0x00);
        assert_eq!(out[7], 0xFF);
        // 0x8 replicated to eight bits is 0x88, not 0x80 — the same
        // reasoning as `expand5`.
        assert_eq!(out[11], 0x88);
    }

    #[test]
    fn atc_rejects_short_data_rather_than_panicking() {
        // A guest that passes a wrong `imageSize` must get None, not an
        // out-of-bounds slice.
        let short = [0u8; 4];
        assert!(decode_compressed_to_rgba(&short, 4, 4, GL_ATC_RGB_AMD).is_none());
    }

    #[test]
    fn atc_drops_texels_that_hang_past_the_edge() {
        // A 2x2 image still costs one whole block; the twelve
        // overhanging texels must not be written anywhere.
        let block = atc_block(0x7FFF, 0x7FFF, 0);
        let out = decode_compressed_to_rgba(&block, 2, 2, GL_ATC_RGB_AMD).unwrap();
        assert_eq!(out.len(), 2 * 2 * 4);
        assert!(out.chunks(4).all(|p| p == [255, 255, 255, 255]));
    }

    #[test]
    fn dxt1_opaque_block_decodes_rgb() {
        let mut block = [0u8; 8];
        block[0..2].copy_from_slice(&0xF800u16.to_le_bytes());
        block[2..4].copy_from_slice(&0x001Fu16.to_le_bytes());
        block[4..8].fill(0);
        let out =
            decode_compressed_to_rgba(&block, 4, 4, GL_COMPRESSED_RGBA_S3TC_DXT1_EXT).unwrap();
        assert_eq!(&out[0..4], &[255, 0, 0, 255]);
        assert_eq!(&out[4..8], &[255, 0, 0, 255]);
    }

    #[test]
    fn dxt1_transparent_selector_has_zero_alpha() {
        let mut block = [0u8; 8];
        block[0..2].copy_from_slice(&0x001Fu16.to_le_bytes());
        block[2..4].copy_from_slice(&0x001Fu16.to_le_bytes());
        block[4..8].fill(0xFF);
        let out =
            decode_compressed_to_rgba(&block, 4, 4, GL_COMPRESSED_RGBA_S3TC_DXT1_EXT).unwrap();
        assert_eq!(&out[0..4], &[0, 0, 0, 0]);
    }
}

#[cfg(test)]
mod pvrtc_tests {
    use super::*;
    #[test]
    fn pvrtc_size_and_invalid_dimensions() {
        for format in 0x8c00..=0x8c03 {
            assert_eq!(compressed_image_size(format, 1, 1), Some(32));
            assert_eq!(compressed_image_size(format, 8, 8), Some(32));
            assert_eq!(compressed_image_size(format, 3, 8), None);
            assert_eq!(compressed_image_size(format, 0, 8), None);
            assert!(decode_compressed_to_rgba(&[0; 31], 8, 8, format).is_none());
        }
        assert_eq!(compressed_image_size(0x8c02, 32, 16), Some(256));
        assert_eq!(compressed_image_size(0x8c03, 32, 16), Some(128));
    }
    #[test]
    fn pvrtc_rgb_alpha_channel_order_and_small_mips() {
        // Every word has identical opaque red endpoints. Modulation cannot
        // alter the color: catches BGRA/RGBA conversion and cropped mip rows.
        let mut red = Vec::new();
        for _ in 0..4 {
            red.extend_from_slice(&[0, 0, 0, 0, 0, 0xfc, 0, 0xfc]);
        }
        for format in 0x8c00..=0x8c03 {
            let out = decode_compressed_to_rgba(&red, 1, 1, format).unwrap();
            assert_eq!(out, vec![255, 0, 0, 255]);
            let transparent = decode_compressed_to_rgba(&[0; 32], 1, 1, format).unwrap();
            assert_eq!(
                transparent,
                vec![0, 0, 0, if format < 0x8c02 { 255 } else { 0 }]
            );
        }
    }
    #[test]
    fn mip_filters_select_levels_blend_and_respect_max_level() {
        let mut tex = Texture::default();
        tex.set_level(0, 4, 4, [255, 0, 0, 255].repeat(16));
        tex.set_level(1, 2, 2, [0, 255, 0, 255].repeat(4));
        tex.set_level(2, 1, 1, vec![0, 0, 255, 255]);
        let levels = tex.upload_levels();
        for filter in [Filter::NearestMipmapNearest, Filter::LinearMipmapNearest] {
            tex.min_filter = filter;
            assert_eq!(tex.sample_lod(&levels, 0.5, 0.5, 1.), [0, 255, 0, 255]);
            assert_eq!(tex.sample_lod(&levels, 0.5, 0.5, 2.), [0, 0, 255, 255]);
        }
        for filter in [Filter::NearestMipmapLinear, Filter::LinearMipmapLinear] {
            tex.min_filter = filter;
            assert_eq!(tex.sample_lod(&levels, 0.5, 0.5, 1.5), [0, 128, 128, 255]);
        }
        tex.max_level = 1;
        assert_eq!(tex.sample_lod(&levels, 0.5, 0.5, 8.), [0, 255, 0, 255]);
        assert_eq!(tex.sample_lod(&levels, 0.5, 0.5, -2.), [255, 0, 0, 255]);
    }
    #[test]
    fn min_and_mag_filters_remain_distinct_and_generation_requires_a_request() {
        let mut tex = Texture::default();
        tex.set_level(
            0,
            2,
            2,
            vec![
                255, 0, 0, 255, 0, 0, 255, 255, 255, 0, 0, 255, 0, 0, 255, 255,
            ],
        );
        tex.mag_filter = Filter::Nearest;
        tex.min_filter = Filter::Linear;
        assert_eq!(tex.sample_lod(&[], 0.5, 0.5, -1.), [0, 0, 255, 255]);
        assert_eq!(tex.sample_lod(&[], 0.5, 0.5, 1.), [128, 0, 128, 255]);
        tex.min_filter = Filter::LinearMipmapLinear;
        assert!(!tex.is_complete());
        assert!(tex.upload_levels().is_empty());
        tex.set_parameter(0x8191, 1);
        assert!(!tex.is_complete()); // toggling the flag alone changes no image.
        let base = tex.rgba.clone();
        tex.set_level(0, 2, 2, base);
        let levels = tex.upload_levels();
        assert_eq!(levels.len(), 2);
        assert_eq!(tex.sample_lod(&levels, 0.5, 0.5, 1.), [128, 0, 128, 255]);
    }
    #[test]
    fn automatic_mipmaps_allocate_levels_and_replace_old_explicit_children() {
        let mut t = Texture::default();
        t.set_parameter(0x8191, 1);
        t.set_level(0, 4, 4, [255, 0, 0, 255].repeat(16));
        assert_eq!(
            t.mipmaps.get(&1).unwrap(),
            &(2, 2, [255, 0, 0, 255].repeat(4))
        );
        t.set_level(1, 2, 2, [0, 0, 255, 255].repeat(4));
        t.set_level(0, 4, 4, [0, 255, 0, 255].repeat(16));
        assert_eq!(
            t.mipmaps.get(&1).unwrap(),
            &(2, 2, [0, 255, 0, 255].repeat(4))
        );
        assert_eq!(t.mipmaps.get(&2).unwrap(), &(1, 1, vec![0, 255, 0, 255]));
    }
}

#[cfg(test)]
mod completeness_tests {
    use super::*;
    #[test]
    fn initial_state_and_all_minification_modes_require_the_correct_images() {
        let mut t = Texture::default();
        assert_eq!(t.min_filter, Filter::NearestMipmapLinear);
        assert_eq!(t.mag_filter, Filter::Linear);
        assert_eq!((t.wrap_s, t.wrap_t), (Wrap::Repeat, Wrap::Repeat));
        assert!(!t.generate_mipmap);
        t.set_level(0, 8, 2, [255; 4].repeat(16));
        for f in [
            Filter::Nearest,
            Filter::Linear,
            Filter::NearestMipmapNearest,
            Filter::NearestMipmapLinear,
            Filter::LinearMipmapNearest,
            Filter::LinearMipmapLinear,
        ] {
            t.min_filter = f;
            assert_eq!(t.is_complete(), !f.uses_mipmaps());
        }
        t.set_level(1, 4, 1, [0; 4].repeat(4));
        t.set_level(3, 1, 1, vec![0; 4]);
        assert!(!t.is_complete());
        t.set_level(2, 2, 1, [0; 4].repeat(2));
        assert!(t.is_complete());
        t.set_format(2, GL_RGB);
        assert!(!t.is_complete());
        t.set_format(2, GL_RGBA);
        assert!(t.is_complete());
        t.set_level(1, 2, 2, [0; 4].repeat(4));
        assert!(!t.is_complete());
        t.min_filter = Filter::Linear;
        assert!(t.is_complete());
    }
    #[test]
    fn compressed_images_do_not_trigger_automatic_generation() {
        let mut t = Texture::default();
        t.generate_mipmap = true;
        let bytes = [0u8; 8];
        t.set_compressed(
            0,
            4,
            4,
            GL_COMPRESSED_RGBA_S3TC_DXT1_EXT,
            &bytes,
            Vec::new(),
        );
        assert!(t.mipmaps.is_empty());
        assert!(!t.is_complete());
        for (level, side) in [(1, 2), (2, 1)] {
            t.set_compressed(
                level,
                side,
                side,
                GL_COMPRESSED_RGBA_S3TC_DXT1_EXT,
                &bytes,
                Vec::new(),
            );
        }
        assert!(t.is_complete());
        assert_eq!(t.upload_levels().len(), 3);
        t.set_compressed(0, 4, 4, GL_COMPRESSED_RGB_S3TC_DXT1_EXT, &bytes, Vec::new());
        assert!(!t.is_complete()); // independent RGBA child formats were preserved.
    }
    #[test]
    fn generated_levels_replace_required_children_without_touching_higher_images() {
        let mut t = Texture::default();
        t.set_level(5, 1, 1, vec![12; 4]);
        t.generate_mipmap = true;
        t.set_level(0, 4, 2, [255, 0, 0, 255].repeat(8));
        t.set_format(0, GL_RGB);
        assert!(t.is_complete());
        assert_eq!(t.mipmaps[&1], (2, 1, [255, 0, 0, 255].repeat(2)));
        assert_eq!(t.mipmaps[&2], (1, 1, vec![255, 0, 0, 255]));
        assert_eq!(t.mipmaps[&5].2, vec![12; 4]);
        assert_eq!(t.formats[&1], GL_RGB);
        t.set_level(1, 2, 1, [0; 4].repeat(2));
        assert!(!t.is_complete()); // this explicit child is RGBA.
        t.set_level(0, 4, 2, [0, 255, 0, 255].repeat(8));
        t.set_format(0, GL_RGB);
        assert!(t.is_complete());
        assert_eq!(t.mipmaps[&1].2, [0, 255, 0, 255].repeat(2));
    }
}

#[cfg(test)]
mod truncated_mipmap_tests {
    use super::*;
    #[test]
    fn max_level_controls_completeness_without_inventing_guest_images() {
        let mut t = Texture::default();
        t.set_level(0, 8, 4, [255; 4].repeat(32));
        assert!(!t.is_complete());
        t.set_parameter(GL_TEXTURE_MAX_LEVEL_SGIS, 0);
        assert!(t.is_complete());
        assert_eq!(t.upload_levels().len(), 1);
        assert!(t.mipmaps.is_empty());
        t.set_parameter(GL_TEXTURE_MAX_LEVEL_SGIS, 1);
        assert!(!t.is_complete());
        t.set_level(1, 4, 2, [0, 255, 0, 255].repeat(8));
        assert!(t.is_complete());
        assert_eq!(t.upload_levels().len(), 2);
        assert_eq!(
            t.sample_lod(&t.upload_levels(), 0.5, 0.5, 100.),
            [0, 255, 0, 255]
        );
        t.set_level(2, 3, 1, [0; 4].repeat(3));
        assert!(t.is_complete()); // outside active range.
        t.set_parameter(GL_TEXTURE_MAX_LEVEL_SGIS, 2);
        assert!(!t.is_complete());
    }
}

#[cfg(test)]
mod compressed_upload_tests {
    use super::*;
    #[test]
    fn palette_sizes_match_driver_shared_chain_and_linear_nibbles() {
        assert_eq!(
            compressed_upload_size(GL_PALETTE4_RGBA8_OES, -2, 4, 4),
            Some(75)
        );
        assert_eq!(
            compressed_upload_size(GL_PALETTE8_RGBA8_OES, -2, 4, 4),
            Some(1045)
        );
        assert_eq!(compressed_image_size(GL_PALETTE4_RGBA8_OES, 1, 4), Some(66));
        assert_eq!(
            compressed_upload_size(GL_COMPRESSED_RGBA_PVRTC_4BPPV1_IMG, 0, 0, 0),
            Some(32)
        );
        assert_eq!(
            compressed_upload_size(GL_COMPRESSED_RGB_S3TC_DXT1_EXT, 0, 0, 4),
            Some(0)
        );

        let mut data = vec![0; 64];
        for i in 1..=4 {
            data[i * 4..i * 4 + 4].copy_from_slice(&[i as u8, 20, 30, 255]);
        }
        data.extend_from_slice(&[0x12, 0x34]);
        let decoded = decode_compressed_to_rgba(&data, 1, 4, GL_PALETTE4_RGBA8_OES).unwrap();
        assert_eq!(
            decoded,
            [1, 20, 30, 255, 2, 20, 30, 255, 3, 20, 30, 255, 4, 20, 30, 255]
        );
        for level in [i32::MIN, -32, 1, 32, i32::MAX] {
            assert_eq!(
                compressed_upload_size(GL_PALETTE4_RGBA8_OES, level, 4, 4),
                None
            );
        }
    }
}

#[cfg(test)]
mod dxt5_tests {
    use super::*;
    #[test]
    fn both_alpha_palettes_and_opaque_colour_mode_match_the_block_spec() {
        for (endpoints, expected) in [
            ([210, 0], [210, 0, 180, 150, 120, 90, 60, 30]),
            ([0, 200], [0, 200, 40, 80, 120, 160, 0, 255]),
        ] {
            let mut block = [0u8; 16];
            block[..2].copy_from_slice(&endpoints);
            let selectors = (0..16).fold(0u64, |v, i| v | (((i % 8) as u64) << (i * 3)));
            block[2..8].copy_from_slice(&selectors.to_le_bytes()[..6]);
            // c0 <= c1 must still use four opaque colour entries in DXT5.
            block[10..12].copy_from_slice(&0xffffu16.to_le_bytes());
            block[12..16].fill(0xff);
            let pixels =
                decode_compressed_to_rgba(&block, 4, 4, GL_COMPRESSED_RGBA_S3TC_DXT5_EXT).unwrap();
            for (i, p) in pixels.chunks_exact(4).enumerate() {
                assert_eq!(p, [170, 170, 170, expected[i % 8]]);
            }
            for (w, h) in [(1, 1), (2, 2), (1, 4)] {
                let small =
                    decode_compressed_to_rgba(&block, w, h, GL_COMPRESSED_RGBA_S3TC_DXT5_EXT)
                        .unwrap();
                for y in 0..h as usize {
                    for x in 0..w as usize {
                        assert_eq!(
                            &small[(y * w as usize + x) * 4..(y * w as usize + x + 1) * 4],
                            &pixels[(y * 4 + x) * 4..(y * 4 + x + 1) * 4]
                        );
                    }
                }
            }
            assert!(decode_compressed_to_rgba(
                &block[..15],
                4,
                4,
                GL_COMPRESSED_RGBA_S3TC_DXT5_EXT
            )
            .is_none());
        }
        assert_eq!(
            compressed_image_size(GL_COMPRESSED_RGBA_S3TC_DXT5_EXT, 8, 4),
            Some(32)
        );
    }
}
