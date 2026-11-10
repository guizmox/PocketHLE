//! Software triangle rasterizer.
//!
//! This is the backend that turns transformed vertices into pixels. It
//! is deliberately self-contained: no host GPU, no guest memory, no
//! kernel types. That keeps it unit-testable — every behaviour below is
//! pinned by a test that renders into a small buffer and inspects the
//! result — and it means the same code path runs on a PC and on a
//! device where no OpenGL ES driver is available.
//!
//! Conventions, all of which follow the OpenGL ES 1.1 specification:
//!
//! * The colour target is RGBA8888, row-major, top row first. GL's
//!   window origin is bottom-left, so the viewport transform flips Y
//!   when computing the target row.
//! * Depth is stored as a `f32` in `[0, 1]` after the `glDepthRange`
//!   mapping, with smaller meaning nearer.
//! * Vertices arrive in clip space; this module does the perspective
//!   divide, the viewport transform, and near/far clipping.

mod combine;
pub use combine::Combine;

/// How many texture stages a draw can cascade through.
pub const MAX_TEXTURE_STAGES: usize = 6;

/// A vertex after the modelview-projection transform, in clip space.
#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    /// Eye-space position retained for user clipping, including singular projections.
    pub eye: [f32; 4],
    /// Derived point width; negative means use pipeline point size.
    pub point_width: f32,
    /// Clip-space position, `w` not yet divided out.
    pub pos: [f32; 4],
    /// Object-space normal consumed by the native lighting shader.
    pub normal: [f32; 3],
    /// Straight (non-premultiplied) RGBA in `[0, 1]`.
    pub color: [f32; 4],
    pub back_color: [f32; 4],
    /// Texture coordinates, one set per texture unit.
    pub texcoord: [[f32; 4]; MAX_TEXTURE_STAGES],
    /// Eye-space Z used for fog. OpenGL defines fog distance from the
    /// eye-space depth, not clip-space or NDC Z.
    pub fog_depth: f32,
}

impl Default for Vertex {
    fn default() -> Self {
        Self {
            eye: [0., 0., 0., 1.],
            point_width: -1.,
            pos: [0.0, 0.0, 0.0, 1.0],
            normal: [0.0, 0.0, 1.0],
            color: [1.0, 1.0, 1.0, 1.0],
            back_color: [1.0; 4],
            texcoord: [[0.0, 0.0, 0.0, 1.0]; MAX_TEXTURE_STAGES],
            fog_depth: 0.0,
        }
    }
}

/// Comparison function for the depth and alpha tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareFunc {
    Never,
    Less,
    Equal,
    Lequal,
    Greater,
    Notequal,
    Gequal,
    Always,
}

impl CompareFunc {
    pub fn from_enum(value: u32) -> Option<Self> {
        use crate::consts::*;
        Some(match value {
            GL_NEVER => Self::Never,
            GL_LESS => Self::Less,
            GL_EQUAL => Self::Equal,
            GL_LEQUAL => Self::Lequal,
            GL_GREATER => Self::Greater,
            GL_NOTEQUAL => Self::Notequal,
            GL_GEQUAL => Self::Gequal,
            GL_ALWAYS => Self::Always,
            _ => return None,
        })
    }

    /// Evaluate `lhs <op> rhs`.
    #[inline]
    pub fn test(self, lhs: f32, rhs: f32) -> bool {
        match self {
            Self::Never => false,
            Self::Less => lhs < rhs,
            Self::Equal => lhs == rhs,
            Self::Lequal => lhs <= rhs,
            Self::Greater => lhs > rhs,
            Self::Notequal => lhs != rhs,
            Self::Gequal => lhs >= rhs,
            Self::Always => true,
        }
    }
}

/// Source or destination blend factor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendFactor {
    Zero,
    One,
    SrcColor,
    OneMinusSrcColor,
    SrcAlpha,
    OneMinusSrcAlpha,
    DstAlpha,
    OneMinusDstAlpha,
    DstColor,
    OneMinusDstColor,
    SrcAlphaSaturate,
}

impl BlendFactor {
    pub fn from_enum(value: u32) -> Option<Self> {
        use crate::consts::*;
        Some(match value {
            GL_ZERO => Self::Zero,
            GL_ONE => Self::One,
            GL_SRC_COLOR => Self::SrcColor,
            GL_ONE_MINUS_SRC_COLOR => Self::OneMinusSrcColor,
            GL_SRC_ALPHA => Self::SrcAlpha,
            GL_ONE_MINUS_SRC_ALPHA => Self::OneMinusSrcAlpha,
            GL_DST_ALPHA => Self::DstAlpha,
            GL_ONE_MINUS_DST_ALPHA => Self::OneMinusDstAlpha,
            GL_DST_COLOR => Self::DstColor,
            GL_ONE_MINUS_DST_COLOR => Self::OneMinusDstColor,
            GL_SRC_ALPHA_SATURATE => Self::SrcAlphaSaturate,
            _ => return None,
        })
    }

    /// Resolve the factor to a per-channel multiplier.
    #[inline]
    fn resolve(self, src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
        let splat = |v: f32| [v, v, v, v];
        match self {
            Self::Zero => splat(0.0),
            Self::One => splat(1.0),
            Self::SrcColor => src,
            Self::OneMinusSrcColor => [1.0 - src[0], 1.0 - src[1], 1.0 - src[2], 1.0 - src[3]],
            Self::SrcAlpha => splat(src[3]),
            Self::OneMinusSrcAlpha => splat(1.0 - src[3]),
            Self::DstAlpha => splat(dst[3]),
            Self::OneMinusDstAlpha => splat(1.0 - dst[3]),
            Self::DstColor => dst,
            Self::OneMinusDstColor => [1.0 - dst[0], 1.0 - dst[1], 1.0 - dst[2], 1.0 - dst[3]],
            // GL defines the alpha channel of SRC_ALPHA_SATURATE as 1.
            Self::SrcAlphaSaturate => {
                let f = src[3].min(1.0 - dst[3]);
                [f, f, f, 1.0]
            }
        }
    }
}

/// Which faces to discard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CullMode {
    Front,
    Back,
    FrontAndBack,
}

/// Front-face winding direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontFace {
    Cw,
    Ccw,
}

/// How the texture colour combines with the incoming fragment colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TexEnvMode {
    Combine(Combine),
    Modulate,
    Replace,
    Decal,
    Blend,
    Add,
}

/// Fog falloff curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FogMode {
    Linear,
    Exp,
    Exp2,
}

/// Everything the per-fragment stage needs. Assembled by the state
/// machine once per draw call rather than read through a lock per pixel.
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineState {
    pub viewport: (i32, i32, i32, i32),
    pub depth_range: (f32, f32),
    pub scissor: Option<(i32, i32, i32, i32)>,
    pub depth_test: bool,
    pub depth_func: CompareFunc,
    pub depth_write: bool,
    pub alpha_test: bool,
    pub alpha_func: CompareFunc,
    pub alpha_ref: f32,
    pub logic_op: bool,
    pub logic_mode: u32,
    pub blend: bool,
    pub blend_src: BlendFactor,
    pub blend_dst: BlendFactor,
    pub cull: Option<CullMode>,
    pub front_face: FrontFace,
    pub color_mask: [bool; 4],
    pub fog: bool,
    pub fog_mode: FogMode,
    pub fog_color: [f32; 4],
    pub fog_start: f32,
    pub fog_end: f32,
    pub fog_density: f32,
    /// `false` selects flat shading, which uses the last vertex's colour
    /// across the whole triangle.
    pub smooth_shading: bool,
    pub two_sided_lighting: bool,
    pub point_size: f32,
    pub point_size_array: bool,
    pub point_sprite: bool,
    pub point_coord_replace: [bool; MAX_TEXTURE_STAGES],
    pub line_width: f32,
    pub polygon_offset: bool,
    pub polygon_factor: f32,
    pub polygon_units: f32,
    /// Original Intel 2700G WinCE driver: integer constant bias, no slope term.
    /// Opt-in driver compatibility; standard ES behavior is the default.
    pub intel2700g_depth_bias: bool,
    /// GoForce 4500 approximation, not a measured hardware rule.
    pub goforce4500_depth_tolerance: bool,
}

impl Default for PipelineState {
    fn default() -> Self {
        Self {
            viewport: (0, 0, 0, 0),
            depth_range: (0.0, 1.0),
            scissor: None,
            depth_test: false,
            depth_func: CompareFunc::Less,
            depth_write: true,
            alpha_test: false,
            alpha_func: CompareFunc::Always,
            alpha_ref: 0.0,
            logic_op: false,
            logic_mode: crate::GL_COPY,
            blend: false,
            blend_src: BlendFactor::One,
            blend_dst: BlendFactor::Zero,
            cull: None,
            front_face: FrontFace::Ccw,
            color_mask: [true; 4],
            fog: false,
            fog_mode: FogMode::Exp,
            fog_color: [0.0, 0.0, 0.0, 0.0],
            fog_start: 0.0,
            fog_end: 1.0,
            fog_density: 1.0,
            smooth_shading: true,
            two_sided_lighting: false,
            point_size: 1.0,
            point_size_array: false,
            point_sprite: false,
            point_coord_replace: [false; MAX_TEXTURE_STAGES],
            line_width: 1.0,
            polygon_offset: false,
            polygon_factor: 0.,
            polygon_units: 0.,
            intel2700g_depth_bias: false,
            goforce4500_depth_tolerance: false,
        }
    }
}

pub(crate) const GOFORCE4500_DEPTH_TOLERANCE: f32 = 1. / 12.;
const INTEL2700G_DEPTH_TOLERANCE_UNITS: f32 = 16.;

impl PipelineState {
    /// Empirical 2700G tolerance for nearly coplanar opaque fragments, including
    /// reversed depth. THPS2 submits fixed-point projected decals whose depth
    /// crosses their backing surface as the camera moves. This is a profile
    /// approximation, not a measured MBX register value. Preserve the nearest
    /// real depth and reject translucent fragments to avoid blending twice.
    pub fn intel2700g_depth_color_pass(&self) -> Option<Self> {
        if !self.intel2700g_depth_bias
            || !self.depth_test
            || !self.depth_write
            || self.depth_range != (0., 1.)
            || !matches!(self.depth_func, CompareFunc::Lequal | CompareFunc::Gequal)
            || self.logic_op
            || self.polygon_offset
            || (self.blend
                && (self.blend_src != BlendFactor::SrcAlpha
                    || self.blend_dst != BlendFactor::OneMinusSrcAlpha))
            || (self.alpha_test
                && !self
                    .alpha_func
                    .test(255., (self.alpha_ref.clamp(0., 1.) * 255. + 0.5).floor()))
            || !self.color_mask.iter().any(|enabled| *enabled)
        {
            return None;
        }
        let mut pass = self.clone();
        pass.depth_write = false;
        pass.alpha_test = true;
        pass.alpha_func = CompareFunc::Equal;
        pass.alpha_ref = 1.;
        pass.polygon_offset = true;
        pass.polygon_factor = 0.;
        pass.polygon_units = if self.depth_func == CompareFunc::Gequal {
            INTEL2700G_DEPTH_TOLERANCE_UNITS
        } else {
            -INTEL2700G_DEPTH_TOLERANCE_UNITS
        };
        Some(pass)
    }

    /// A second opaque color pass expands clip depth away from the far plane.
    /// Keep the original depth: tolerance must not accumulate with draw order
    /// or pull subsequent geometry towards the camera.
    pub fn goforce_depth_color_pass(&self) -> Option<Self> {
        if !self.goforce4500_depth_tolerance
            || !self.depth_test
            || !self.depth_write
            || self.depth_range != (0., 1.)
            || !matches!(self.depth_func, CompareFunc::Less | CompareFunc::Lequal)
            || self.blend
            || self.logic_op
            || self.polygon_offset
            || self.intel2700g_depth_bias
            || !self.color_mask.iter().any(|enabled| *enabled)
        {
            return None;
        }
        let mut pass = self.clone();
        pass.goforce4500_depth_tolerance = false;
        pass.depth_write = false;
        pass.depth_range = (-GOFORCE4500_DEPTH_TOLERANCE, 1.);
        Some(pass)
    }

    /// The reference WinCE CL driver ignores factor and arithmetic-shifts its
    /// signed 16.16 units before encoding a sign + 16-bit magnitude register.
    pub fn polygon_offset_parameters(&self) -> (f32, f32) {
        if self.intel2700g_depth_bias {
            (0., self.polygon_units.floor().clamp(-65535., 65535.))
        } else {
            (self.polygon_factor, self.polygon_units)
        }
    }

    /// Model the bias in the 16.16 domain of the 2700G primitive input,
    /// independently of the precision advertised for its depth attachment.
    pub fn polygon_offset_resolution(&self) -> f32 {
        if self.intel2700g_depth_bias {
            65536.
        } else {
            65535.
        }
    }

    pub fn depth_bits(&self) -> u32 {
        if self.intel2700g_depth_bias {
            24
        } else {
            16
        }
    }
}

/// Colour and depth attachments.
pub struct RenderTarget {
    /// RGB565 virtual surface; RGBA bytes are only its expanded CPU cache.
    pub rgb565: bool,
    pub width: u32,
    pub height: u32,
    /// RGBA8888, row-major, top row first.
    pub color: Vec<u8>,
    /// Window-space depth in `[0, 1]`, smaller is nearer.
    pub depth: Vec<f32>,
}

impl RenderTarget {
    pub fn new(width: u32, height: u32) -> Self {
        let px = (width as usize) * (height as usize);
        Self {
            width,
            height,
            rgb565: false,
            color: vec![0u8; px * 4],
            depth: vec![1.0f32; px],
        }
    }

    pub fn with_format(width: u32, height: u32, rgb565: bool) -> Self {
        let mut target = Self::new(width, height);
        target.rgb565 = rgb565;
        if rgb565 {
            for p in target.color.chunks_exact_mut(4) {
                p[3] = 255;
            }
        }
        target
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == self.width && height == self.height {
            return;
        }
        *self = Self::with_format(width, height, self.rgb565);
    }

    /// `glClear(GL_COLOR_BUFFER_BIT)` honouring the scissor box.
    pub fn clear_color(&mut self, rgba: [u8; 4], scissor: Option<(i32, i32, i32, i32)>) {
        let rgba = if self.rgb565 {
            rgb565_pixel(rgba)
        } else {
            rgba
        };
        match self.scissor_bounds(scissor) {
            None => {}
            Some((x0, y0, x1, y1)) if self.covers_all(x0, y0, x1, y1) => {
                for px in self.color.chunks_exact_mut(4) {
                    px.copy_from_slice(&rgba);
                }
            }
            Some((x0, y0, x1, y1)) => {
                for y in y0..y1 {
                    let row = (y as usize) * (self.width as usize);
                    for x in x0..x1 {
                        let i = (row + x as usize) * 4;
                        self.color[i..i + 4].copy_from_slice(&rgba);
                    }
                }
            }
        }
    }

    /// Clear only enabled color components; keep the full-write fast path.
    pub fn clear_color_masked(
        &mut self,
        rgba: [u8; 4],
        scissor: Option<(i32, i32, i32, i32)>,
        mask: [bool; 4],
    ) {
        let rgba = if self.rgb565 {
            rgb565_pixel(rgba)
        } else {
            rgba
        };
        let mask = [mask[0], mask[1], mask[2], mask[3] && !self.rgb565];
        if mask == [true; 4] || self.rgb565 && mask[..3] == [true; 3] {
            self.clear_color(rgba, scissor);
            return;
        }
        if mask == [false; 4] {
            return;
        }
        if let Some((x0, y0, x1, y1)) = self.scissor_bounds(scissor) {
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = (y as usize * self.width as usize + x as usize) * 4;
                    for c in 0..4 {
                        if mask[c] {
                            self.color[i + c] = rgba[c];
                        }
                    }
                }
            }
        }
    }

    /// `glClear(GL_DEPTH_BUFFER_BIT)` honouring the scissor box.
    pub fn clear_depth(&mut self, value: f32, scissor: Option<(i32, i32, i32, i32)>) {
        match self.scissor_bounds(scissor) {
            None => {}
            Some((x0, y0, x1, y1)) if self.covers_all(x0, y0, x1, y1) => {
                self.depth.fill(value);
            }
            Some((x0, y0, x1, y1)) => {
                for y in y0..y1 {
                    let row = (y as usize) * (self.width as usize);
                    for x in x0..x1 {
                        self.depth[row + x as usize] = value;
                    }
                }
            }
        }
    }

    fn covers_all(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
        x0 == 0 && y0 == 0 && x1 == self.width as i32 && y1 == self.height as i32
    }

    /// Intersect a GL scissor box (bottom-left origin) with the target,
    /// returning top-left-origin half-open bounds. `None` means the
    /// intersection is empty and the caller should do nothing.
    fn scissor_bounds(
        &self,
        scissor: Option<(i32, i32, i32, i32)>,
    ) -> Option<(i32, i32, i32, i32)> {
        let (w, h) = (i64::from(self.width), i64::from(self.height));
        let Some((sx, sy, sw, sh)) = scissor else {
            return Some((0, 0, w as i32, h as i32));
        };
        if sw <= 0 || sh <= 0 {
            return None;
        }
        let (sx, sy, sw, sh) = (i64::from(sx), i64::from(sy), i64::from(sw), i64::from(sh));
        let x0 = sx.max(0);
        let x1 = (sx + sw).min(w);
        // GL's scissor Y is measured from the bottom; flip to rows.
        let top = h - (sy + sh);
        let y0 = top.max(0);
        let y1 = (h - sy).min(h);
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        Some((x0 as i32, y0 as i32, x1 as i32, y1 as i32))
    }

    /// Convert the colour buffer to RGB565 little-endian, the format the
    /// PocketHLE framebuffer holds. Alpha is dropped: the window surface
    /// is opaque.
    pub fn to_rgb565(&self, out: &mut [u8]) {
        for (i, px) in self.color.chunks_exact(4).enumerate() {
            let o = i * 2;
            if o + 1 >= out.len() {
                break;
            }
            let v = (((px[0] as u16) >> 3) << 11)
                | (((px[1] as u16) >> 2) << 5)
                | ((px[2] as u16) >> 3);
            out[o] = (v & 0xFF) as u8;
            out[o + 1] = (v >> 8) as u8;
        }
    }
}

/// Texture sampling callback, called with the texture unit a stage
/// draws from. Returning `None` means that unit has no usable texture,
/// in which case the stage leaves the fragment colour alone.
pub trait TextureSampler {
    fn combiner_program(&self) -> Option<&crate::combiner_program::BasicProgram> {
        None
    }

    fn combiner_constants(&self) -> [[f32; 4]; 4] {
        [[0.0; 4]; 4]
    }
    fn sample(&self, unit: usize, s: f32, t: f32) -> Option<[u8; 4]>;
    fn needs_gradients(&self) -> bool {
        false
    }
    fn sample_grad(
        &self,
        unit: usize,
        s: f32,
        t: f32,
        _grad: [f32; 4],
        _bias: f32,
    ) -> Option<[u8; 4]> {
        self.sample(unit, s, t)
    }
}
impl<F: Fn(usize, f32, f32) -> Option<[u8; 4]>> TextureSampler for F {
    fn sample(&self, unit: usize, s: f32, t: f32) -> Option<[u8; 4]> {
        self(unit, s, t)
    }
}
pub type SampleFn<'a> = &'a dyn TextureSampler;

fn coordinate_units(sample: SampleFn<'_>, stages: &[TextureStage]) -> u8 {
    sample.combiner_program().map_or_else(
        || {
            stages
                .iter()
                .fold(0, |mask, stage| mask | (1 << stage.unit))
        },
        |program| program.coordinate_units(),
    )
}

fn framebuffer_color(target: &RenderTarget, idx: usize) -> [f32; 4] {
    let offset = idx * 4;
    [
        to_unit(target.color[offset]),
        to_unit(target.color[offset + 1]),
        to_unit(target.color[offset + 2]),
        if target.rgb565 {
            1.
        } else {
            to_unit(target.color[offset + 3])
        },
    ]
}

fn evaluate_combiner(
    program: &crate::combiner_program::BasicProgram,
    sample: SampleFn<'_>,
    stages: &[TextureStage],
    primary: [f32; 4],
    constants: [[f32; 4]; 4],
    framebuffer: [f32; 4],
    coordinate: impl Fn(usize) -> (f32, f32, [f32; 4]),
) -> Option<[f32; 4]> {
    program.evaluate_fragment(primary, constants, framebuffer, |route| {
        let Some(stage) = stages.iter().find(|stage| stage.unit == route.texture) else {
            return [0., 0., 0., 1.];
        };
        let (s, t, grad) = if route.s == route.t && route.s_component == 0 && route.t_component == 1
        {
            coordinate(route.s)
        } else {
            let (sx, sy, sg) = coordinate(route.s);
            let (tx, ty, tg) = coordinate(route.t);
            let s = [sx, sy][route.s_component];
            let t = [tx, ty][route.t_component];
            (
                s,
                t,
                [
                    sg[route.s_component],
                    tg[route.t_component],
                    sg[route.s_component + 2],
                    tg[route.t_component + 2],
                ],
            )
        };
        sample
            .sample_grad(route.texture, s, t, grad, stage.lod_bias)
            .map_or([0., 0., 0., 1.], |texel| texel.map(to_unit))
    })
}

/// One enabled texture unit, as the rasterizer sees it.
///
/// GL ES 1.1 multitexturing is a cascade: the stages are applied in
/// ascending unit order and each one combines its texel with the colour
/// the stage before it produced, starting from the interpolated vertex
/// colour. Games lean on that. Sticky Balls stores a ball as a DXT1
/// colour map — a format with no alpha channel at all — on one stage
/// and a white-RGB circular alpha mask on the next, so only running
/// both, in order, cuts the ball out of its square.
#[derive(Debug, Clone, Copy)]
pub struct TextureStage {
    /// Which texture unit this is. Selects both the texture
    /// [`SampleFn`] reads and the vertex coordinate set it reads with.
    pub unit: usize,
    /// This unit's `glTexEnv(GL_TEXTURE_ENV_MODE)`.
    pub env: TexEnvMode,
    /// This unit's `GL_TEXTURE_ENV_COLOR`; only `GL_BLEND` reads it.
    pub env_color: [f32; 4],
    pub lod_bias: f32,
}

#[inline]
fn to_unit(v: u8) -> f32 {
    v as f32 / 255.0
}

#[inline]
fn to_byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Window-space vertex after the perspective divide.
#[derive(Debug, Clone, Copy)]
struct ScreenVertex {
    x: f32,
    y: f32,
    z: f32,
    /// Reciprocal of clip `w`, for perspective-correct interpolation.
    inv_w: f32,
    color: [f32; 4],
    texcoord: [[f32; 4]; MAX_TEXTURE_STAGES],
    fog_depth: f32,
}

/// Rasterize one triangle. Returns the number of fragments written,
/// which the tests use to distinguish "culled" from "drawn but
/// invisible".
pub fn draw_triangle(
    target: &mut RenderTarget,
    state: &PipelineState,
    sample: SampleFn<'_>,
    stages: &[TextureStage],
    tri: [Vertex; 3],
) -> usize {
    // Small primitives are dominated by clipping/allocation overhead. Keep the
    // established path for larger triangles: its pixel loop benchmarks better.
    if tri.iter().all(|v| {
        let [x, y, z, w] = v.pos;
        w.is_finite() && w >= 1e-9 && x.abs() <= w && y.abs() <= w && z.abs() <= w
    }) {
        let xs = tri.map(|v| v.pos[0] / v.pos[3]);
        let ys = tri.map(|v| v.pos[1] / v.pos[3]);
        let span = |v: [f32; 3]| v[0].max(v[1]).max(v[2]) - v[0].min(v[1]).min(v[2]);
        if span(xs) * state.viewport.2 as f32 <= 8.0 && span(ys) * state.viewport.3 as f32 <= 8.0 {
            return raster_clipped(target, state, sample, stages, tri);
        }
    }
    draw_triangle_clipped(target, state, sample, stages, tri)
}

fn draw_triangle_clipped(
    target: &mut RenderTarget,
    state: &PipelineState,
    sample: SampleFn<'_>,
    stages: &[TextureStage],
    tri: [Vertex; 3],
) -> usize {
    // OpenGL clips primitives in homogeneous clip space before the
    // perspective divide and viewport transform.  Clipping only against
    // the framebuffer bounds is not equivalent: with a sub-viewport,
    // vertices outside +/-W can otherwise spill into pixels outside that
    // viewport.  Clip against all six canonical planes.
    let mut polygon: Vec<Vertex> = tri.to_vec();
    let mut scratch: Vec<Vertex> = Vec::with_capacity(9);
    for plane in 0..6 {
        clip_plane(&polygon, &mut scratch, plane);
        std::mem::swap(&mut polygon, &mut scratch);
        scratch.clear();
        if polygon.len() < 3 {
            return 0;
        }
    }
    let mut written = 0;
    for i in 1..polygon.len() - 1 {
        written += raster_clipped(
            target,
            state,
            sample,
            stages,
            [polygon[0], polygon[i], polygon[i + 1]],
        );
    }
    written
}

/// Sutherland-Hodgman clipping against one canonical OpenGL clip plane.
/// Inside is: -w <= x,y,z <= w.
fn clip_distance(v: &Vertex, plane: usize) -> f32 {
    match plane {
        0 => v.pos[0] + v.pos[3],
        1 => v.pos[3] - v.pos[0],
        2 => v.pos[1] + v.pos[3],
        3 => v.pos[3] - v.pos[1],
        4 => v.pos[2] + v.pos[3],
        5 => v.pos[3] - v.pos[2],
        _ => unreachable!(),
    }
}

fn clip_plane(input: &[Vertex], out: &mut Vec<Vertex>, plane: usize) {
    if input.is_empty() {
        return;
    }
    for i in 0..input.len() {
        let a = &input[i];
        let b = &input[(i + 1) % input.len()];
        let da = clip_distance(a, plane);
        let db = clip_distance(b, plane);
        let a_in = da >= 0.0;
        let b_in = db >= 0.0;
        if a_in {
            out.push(*a);
        }
        if a_in != b_in {
            let denom = da - db;
            if denom != 0.0 && denom.is_finite() {
                let t = da / denom;
                out.push(lerp_vertex(a, b, t));
            }
        }
    }
}

fn lerp_vertex(a: &Vertex, b: &Vertex, t: f32) -> Vertex {
    let mix = |x: f32, y: f32| x + (y - x) * t;
    Vertex {
        eye: std::array::from_fn(|i| mix(a.eye[i], b.eye[i])),
        point_width: mix(a.point_width, b.point_width),
        pos: [
            mix(a.pos[0], b.pos[0]),
            mix(a.pos[1], b.pos[1]),
            mix(a.pos[2], b.pos[2]),
            mix(a.pos[3], b.pos[3]),
        ],
        color: [
            mix(a.color[0], b.color[0]),
            mix(a.color[1], b.color[1]),
            mix(a.color[2], b.color[2]),
            mix(a.color[3], b.color[3]),
        ],
        texcoord: std::array::from_fn(|u| {
            std::array::from_fn(|c| mix(a.texcoord[u][c], b.texcoord[u][c]))
        }),
        back_color: std::array::from_fn(|i| mix(a.back_color[i], b.back_color[i])),
        normal: std::array::from_fn(|i| mix(a.normal[i], b.normal[i])),
        fog_depth: mix(a.fog_depth, b.fog_depth),
    }
}

fn raster_clipped(
    target: &mut RenderTarget,
    state: &PipelineState,
    sample: SampleFn<'_>,
    stages: &[TextureStage],
    tri: [Vertex; 3],
) -> usize {
    let (vx, vy, vw, vh) = state.viewport;
    if vw <= 0 || vh <= 0 {
        return 0;
    }
    let (dn, df) = state.depth_range;

    // Perspective divide and viewport transform. GL's viewport has a
    // bottom-left origin; our colour buffer's first row is the top, so
    // Y is negated here rather than at every pixel.
    let mut sv = [ScreenVertex {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        inv_w: 1.0,
        color: [0.0; 4],
        texcoord: [[0.0, 0.0, 0.0, 1.0]; MAX_TEXTURE_STAGES],
        fog_depth: 0.0,
    }; 3];
    for (i, v) in tri.iter().enumerate() {
        let w = v.pos[3];
        if w.abs() < 1e-9 || !w.is_finite() {
            return 0;
        }
        let inv_w = 1.0 / w;
        let ndc = [v.pos[0] * inv_w, v.pos[1] * inv_w, v.pos[2] * inv_w];
        if !ndc[0].is_finite() || !ndc[1].is_finite() || !ndc[2].is_finite() {
            return 0;
        }
        sv[i] = ScreenVertex {
            x: vx as f32 + (ndc[0] * 0.5 + 0.5) * vw as f32,
            y: target.height as f32 - (vy as f32 + (ndc[1] * 0.5 + 0.5) * vh as f32),
            z: (dn + (df - dn) * (ndc[2] * 0.5 + 0.5)).clamp(0.0, 1.0),
            inv_w,
            color: v.color,
            texcoord: v.texcoord,
            fog_depth: v.fog_depth,
        };
    }

    // Signed area in window space. Y was flipped above, so a
    // counter-clockwise triangle in GL's frame has a negative area here.
    let area =
        (sv[1].x - sv[0].x) * (sv[2].y - sv[0].y) - (sv[2].x - sv[0].x) * (sv[1].y - sv[0].y);
    if area == 0.0 || !area.is_finite() {
        return 0;
    }
    let is_front = match state.front_face {
        FrontFace::Ccw => area < 0.0,
        FrontFace::Cw => area > 0.0,
    };
    if let Some(cull) = state.cull {
        let discard = match cull {
            CullMode::FrontAndBack => true,
            CullMode::Front => is_front,
            CullMode::Back => !is_front,
        };
        if discard {
            return 0;
        }
    }

    if state.two_sided_lighting && !is_front {
        for i in 0..3 {
            sv[i].color = tri[i].back_color;
        }
    }

    // Bounding box, clipped to the target and the scissor box.
    let Some((cx0, cy0, cx1, cy1)) = target.scissor_bounds(state.scissor) else {
        return 0;
    };
    let min_x = sv.iter().fold(f32::INFINITY, |m, v| m.min(v.x)).floor() as i32;
    let max_x = sv.iter().fold(f32::NEG_INFINITY, |m, v| m.max(v.x)).ceil() as i32;
    let min_y = sv.iter().fold(f32::INFINITY, |m, v| m.min(v.y)).floor() as i32;
    let max_y = sv.iter().fold(f32::NEG_INFINITY, |m, v| m.max(v.y)).ceil() as i32;
    let x0 = min_x.max(cx0);
    let x1 = (max_x + 1).min(cx1);
    let y0 = min_y.max(cy0);
    let y1 = (max_y + 1).min(cy1);
    if x0 >= x1 || y0 >= y1 {
        return 0;
    }

    let offset = if state.polygon_offset {
        let dx = ((sv[1].z - sv[0].z) * (sv[2].y - sv[0].y)
            - (sv[2].z - sv[0].z) * (sv[1].y - sv[0].y))
            / area;
        let dy = ((sv[1].x - sv[0].x) * (sv[2].z - sv[0].z)
            - (sv[2].x - sv[0].x) * (sv[1].z - sv[0].z))
            / area;
        let (factor, units) = state.polygon_offset_parameters();
        let resolution = state.polygon_offset_resolution();
        factor * dx.abs().max(dy.abs()) + units / resolution
    } else {
        0.
    };
    let inv_area = 1.0 / area;
    let gradients = sample.needs_gradients();
    let coordinates = coordinate_units(sample, stages);
    let projective_units = (0..MAX_TEXTURE_STAGES).fold(0u8, |mask, unit| {
        if coordinates & (1 << unit) != 0 && sv.iter().any(|v| v.texcoord[unit][3] != 1.) {
            mask | (1 << unit)
        } else {
            mask
        }
    });
    let (q_grad, uv_grad) = if gradients {
        let dx = [
            -(sv[2].y - sv[1].y) * inv_area,
            -(sv[0].y - sv[2].y) * inv_area,
            (sv[0].y - sv[1].y) * inv_area,
        ];
        let dy = [
            (sv[2].x - sv[1].x) * inv_area,
            (sv[0].x - sv[2].x) * inv_area,
            (sv[1].x - sv[0].x) * inv_area,
        ];
        let mut q_grad = [[0.; 2]; MAX_TEXTURE_STAGES];
        let mut uv_grad = [[0.; 4]; MAX_TEXTURE_STAGES];
        for unit in 0..MAX_TEXTURE_STAGES {
            if coordinates & (1 << unit) == 0 {
                continue;
            }
            for (axis, weights) in [dx, dy].iter().enumerate() {
                q_grad[unit][axis] = (0..3)
                    .map(|i| weights[i] * sv[i].inv_w * sv[i].texcoord[unit][3])
                    .sum();
                for c in 0..2 {
                    uv_grad[unit][axis * 2 + c] = (0..3)
                        .map(|i| weights[i] * sv[i].inv_w * sv[i].texcoord[unit][c])
                        .sum();
                }
            }
        }
        (q_grad, uv_grad)
    } else {
        ([[0.; 2]; MAX_TEXTURE_STAGES], [[0.; 4]; MAX_TEXTURE_STAGES])
    };
    // Flat shading takes the provoking vertex, which for a triangle is
    // the last one.
    let flat_color = sv[2].color;
    let combiner = sample.combiner_program();
    let constants = combiner.map(|_| sample.combiner_constants());
    let mut written = 0usize;
    let stride = target.width as usize;

    for py in y0..y1 {
        for px in x0..x1 {
            // Sample at the pixel centre.
            let sx = px as f32 + 0.5;
            let sy = py as f32 + 0.5;

            // Barycentric coordinates via edge functions.
            let w0 = ((sv[2].x - sv[1].x) * (sy - sv[1].y) - (sv[2].y - sv[1].y) * (sx - sv[1].x))
                * inv_area;
            let w1 = ((sv[0].x - sv[2].x) * (sy - sv[2].y) - (sv[0].y - sv[2].y) * (sx - sv[2].x))
                * inv_area;
            let w2 = 1.0 - w0 - w1;
            // Negative weights are outside the triangle. The tiny
            // epsilon keeps shared edges from dropping a pixel column
            // between two triangles of a strip.
            const EDGE_EPS: f32 = -1e-6;
            if w0 < EDGE_EPS || w1 < EDGE_EPS || w2 < EDGE_EPS {
                continue;
            }

            let idx = py as usize * stride + px as usize;

            // Depth interpolates linearly in window space.
            // Anchor at one vertex: equal endpoint depths must remain
            // exactly equal regardless of rounded barycentric weights.
            // Coplanar UI layers otherwise fail LEQUAL at scattered pixels.
            let z = (sv[2].z + w0 * (sv[0].z - sv[2].z) + w1 * (sv[1].z - sv[2].z) + offset)
                .clamp(0., 1.);
            if state.depth_test && !state.depth_func.test(z, target.depth[idx]) {
                continue;
            }

            // Perspective-correct attribute interpolation: weight each
            // attribute by 1/w, interpolate, then divide by the
            // interpolated 1/w.
            let iw = w0 * sv[0].inv_w + w1 * sv[1].inv_w + w2 * sv[2].inv_w;
            if iw <= 0.0 || !iw.is_finite() {
                continue;
            }
            let rec = 1.0 / iw;
            let p0 = w0 * sv[0].inv_w * rec;
            let p1 = w1 * sv[1].inv_w * rec;
            let p2 = w2 * sv[2].inv_w * rec;

            let mut frag = if state.smooth_shading {
                [
                    p0 * sv[0].color[0] + p1 * sv[1].color[0] + p2 * sv[2].color[0],
                    p0 * sv[0].color[1] + p1 * sv[1].color[1] + p2 * sv[2].color[1],
                    p0 * sv[0].color[2] + p1 * sv[1].color[2] + p2 * sv[2].color[2],
                    p0 * sv[0].color[3] + p1 * sv[1].color[3] + p2 * sv[2].color[3],
                ]
            } else {
                flat_color
            };

            // The multitexture cascade: each stage's combiner takes the
            // colour the previous stage produced, so the loop order is
            // the stage order. Coordinates are interpolated per stage
            // rather than up front, because an unused unit's coordinate
            // set must not cost anything per fragment.
            let primary = frag;
            let coordinate = |unit: usize| {
                let uv = |c: usize| {
                    p0 * sv[0].texcoord[unit][c]
                        + p1 * sv[1].texcoord[unit][c]
                        + p2 * sv[2].texcoord[unit][c]
                };
                let inv_q = if projective_units & (1 << unit) == 0 {
                    1.
                } else {
                    let q = uv(3);
                    if q != 0. && q.is_finite() {
                        1. / q
                    } else {
                        0.
                    }
                };
                let (u, v) = (uv(0) * inv_q, uv(1) * inv_q);
                let grad = if gradients {
                    let g = uv_grad[unit];
                    let [qx, qy] = q_grad[unit];
                    let rec = rec * inv_q;
                    [
                        (g[0] - u * qx) * rec,
                        (g[1] - v * qx) * rec,
                        (g[2] - u * qy) * rec,
                        (g[3] - v * qy) * rec,
                    ]
                } else {
                    [0.; 4]
                };
                (u, v, grad)
            };
            if let (Some(program), Some(constants)) = (combiner, constants) {
                let dst = if program.reads_framebuffer() {
                    framebuffer_color(target, idx)
                } else {
                    [0.; 4]
                };
                let Some(color) =
                    evaluate_combiner(program, sample, stages, primary, constants, dst, coordinate)
                else {
                    continue;
                };
                frag = color;
            } else {
                for stage in stages {
                    let (u, v, grad) = coordinate(stage.unit);
                    let texel = if gradients {
                        sample.sample_grad(stage.unit, u, v, grad, stage.lod_bias)
                    } else {
                        sample.sample(stage.unit, u, v)
                    };
                    if let Some(texel) = texel {
                        frag = apply_tex_env(stage, frag, texel.map(to_unit), primary);
                    }
                }
            }

            let depth = p0 * sv[0].fog_depth + p1 * sv[1].fog_depth + p2 * sv[2].fog_depth;
            if !write_fragment(target, state, idx, z, frag, depth) {
                continue;
            }
            written += 1;
        }
    }
    written
}

/// Common fragment operations, shared by triangles, lines and points.
#[inline]
fn write_fragment(
    target: &mut RenderTarget,
    state: &PipelineState,
    idx: usize,
    z: f32,
    mut frag: [f32; 4],
    depth: f32,
) -> bool {
    // ES 1.1 requires rounding fragment alpha and converting the
    // reference to the same fixed-point precision before testing.
    // Comparing raw interpolated floats rejects opaque fragments at
    // GL_EQUAL/1 when perspective weights sum to 0.99999994.
    // Alpha has eight bits in our RGBA8888 fragment pipeline, even
    // when the presentation surface is RGB565 without stored alpha.
    if state.alpha_test
        && !state.alpha_func.test(
            f32::from(to_byte(frag[3])),
            f32::from(to_byte(state.alpha_ref)),
        )
    {
        return false;
    }

    if state.fog {
        let f = fog_factor(state, depth);
        for (c, fc) in frag.iter_mut().zip(state.fog_color).take(3) {
            *c = f * *c + (1.0 - f) * fc;
        }
    }

    let ci = idx * 4;
    let dst = framebuffer_color(target, idx);
    let out = if state.blend && !state.logic_op {
        let sf = state.blend_src.resolve(frag, dst);
        let df = state.blend_dst.resolve(frag, dst);
        [
            (frag[0] * sf[0] + dst[0] * df[0]).clamp(0.0, 1.0),
            (frag[1] * sf[1] + dst[1] * df[1]).clamp(0.0, 1.0),
            (frag[2] * sf[2] + dst[2] * df[2]).clamp(0.0, 1.0),
            (frag[3] * sf[3] + dst[3] * df[3]).clamp(0.0, 1.0),
        ]
    } else {
        frag
    };

    for (c, (&value, &masked)) in out.iter().zip(&state.color_mask).enumerate() {
        if masked && !(target.rgb565 && c == 3) {
            target.color[ci + c] = if target.rgb565 {
                let max = if c == 1 { 63 } else { 31 };
                let source = (value.clamp(0., 1.) * max as f32 + 0.5) as u8;
                let bits = if state.logic_op {
                    let dest = ((u32::from(target.color[ci + c]) * max + 127) / 255) as u8;
                    logic_byte(state.logic_mode, source, dest) & max as u8
                } else {
                    source
                };
                ((u32::from(bits) * 255 + max / 2) / max) as u8
            } else if state.logic_op {
                logic_byte(state.logic_mode, to_byte(value), target.color[ci + c])
            } else {
                to_byte(value)
            };
        }
    }
    if target.rgb565 {
        target.color[ci + 3] = 255;
    }
    if state.depth_test && state.depth_write {
        target.depth[idx] = z;
    }
    true
}

/// Expand a virtual RGB565 pixel to the shared RGBA byte cache.
pub fn rgb565_pixel(mut p: [u8; 4]) -> [u8; 4] {
    for (i, max) in [31u32, 63, 31].into_iter().enumerate() {
        let bits = (u32::from(p[i]) * max + 127) / 255;
        p[i] = ((bits * 255 + max / 2) / max) as u8;
    }
    p[3] = 255;
    p
}

/// ES 1.1 table 4.2. Operates on framebuffer component bits, before write masks.
pub(crate) fn logic_byte(op: u32, s: u8, d: u8) -> u8 {
    match op {
        0x1500 => 0,
        0x1501 => s & d,
        0x1502 => s & !d,
        0x1503 => s,
        0x1504 => !s & d,
        0x1505 => d,
        0x1506 => s ^ d,
        0x1507 => s | d,
        0x1508 => !(s | d),
        0x1509 => !(s ^ d),
        0x150a => !d,
        0x150b => s | !d,
        0x150c => !s,
        0x150d => !s | d,
        0x150e => !(s & d),
        0x150f => 255,
        _ => unreachable!(),
    }
}

fn primitive_fragment(
    target: &mut RenderTarget,
    state: &PipelineState,
    sample: SampleFn<'_>,
    stages: &[TextureStage],
    x: i32,
    y: i32,
    z: f32,
    v: &Vertex,
    gradients: &[[f32; 4]; MAX_TEXTURE_STAGES],
    sprite: Option<([f32; 2], f32)>,
) -> bool {
    let idx = y as usize * target.width as usize + x as usize;
    if state.depth_test && !state.depth_func.test(z, target.depth[idx]) {
        return false;
    }
    let coordinate = |unit: usize| {
        let uv = v.texcoord[unit];
        if state.point_coord_replace[unit] {
            if let Some((uv, inv_size)) = sprite {
                return (uv[0], uv[1], [inv_size, 0., 0., inv_size]);
            }
        }
        let q = uv[3];
        if q != 0. {
            (uv[0] / q, uv[1] / q, gradients[unit])
        } else {
            (0., 0., gradients[unit])
        }
    };
    let color = if let Some(program) = sample.combiner_program() {
        let Some(color) = evaluate_combiner(
            program,
            sample,
            stages,
            v.color,
            sample.combiner_constants(),
            if program.reads_framebuffer() {
                framebuffer_color(target, idx)
            } else {
                [0.; 4]
            },
            coordinate,
        ) else {
            return false;
        };
        color
    } else {
        let mut color = v.color;
        for stage in stages {
            let (u, v_coord, gradient) = coordinate(stage.unit);
            if let Some(texel) =
                sample.sample_grad(stage.unit, u, v_coord, gradient, stage.lod_bias)
            {
                color = apply_tex_env(stage, color, texel.map(to_unit), v.color);
            }
        }
        color
    };
    write_fragment(target, state, idx, z, color, v.fog_depth)
}

fn window_position(target: &RenderTarget, state: &PipelineState, v: &Vertex) -> Option<[f64; 3]> {
    if v.pos.iter().any(|x| !x.is_finite()) || v.pos[3] <= 0. {
        return None;
    }
    let (x, y, w, h) = state.viewport;
    if w <= 0 || h <= 0 {
        return None;
    }
    let p = v.pos.map(|x| f64::from(x) / f64::from(v.pos[3]));
    let (near, far) = state.depth_range;
    Some([
        f64::from(x) + (p[0] + 1.) * f64::from(w) / 2.,
        f64::from(target.height) - f64::from(y) - (p[1] + 1.) * f64::from(h) / 2.,
        f64::from(near) + (p[2] + 1.) * f64::from(far - near) / 2.,
    ])
}

/// Aliased square points: clip the center before expanding in window space.
pub fn draw_point(
    target: &mut RenderTarget,
    state: &PipelineState,
    sample: SampleFn<'_>,
    stages: &[TextureStage],
    v: Vertex,
) -> usize {
    if (0..6).any(|p| clip_distance(&v, p) < 0.) {
        return 0;
    }
    let Some(pos) = window_position(target, state, &v) else {
        return 0;
    };
    let Some((x0, y0, x1, y1)) = target.scissor_bounds(state.scissor) else {
        return 0;
    };
    let width = if v.point_width >= 0. {
        v.point_width
    } else {
        state.point_size
    };
    let size = width.clamp(1., 64.).round() as i32;
    let center = |p: f64| {
        if size % 2 == 0 {
            (p + 0.5).floor()
        } else {
            p.floor() + 0.5
        }
    };
    let left = if state.point_sprite {
        (pos[0] - f64::from(size) / 2. - 0.5).ceil() as i32
    } else {
        (center(pos[0]) - f64::from(size) / 2.) as i32
    };
    let top = if state.point_sprite {
        (pos[1] - f64::from(size) / 2. - 0.5).ceil() as i32
    } else {
        (f64::from(target.height)
            - center(f64::from(target.height) - pos[1])
            - f64::from(size) / 2.) as i32
    };
    let mut written = 0;
    for y in top.max(y0)..top.saturating_add(size).min(y1) {
        for x in left.max(x0)..left.saturating_add(size).min(x1) {
            written += primitive_fragment(
                target,
                state,
                sample,
                stages,
                x,
                y,
                pos[2] as f32,
                &v,
                &[[0.; 4]; MAX_TEXTURE_STAGES],
                if state.point_sprite {
                    Some((
                        [
                            (0.5 + (f64::from(x) + 0.5 - pos[0]) / f64::from(size)) as f32,
                            (0.5 + (f64::from(y) + 0.5 - pos[1]) / f64::from(size)) as f32,
                        ],
                        1. / size as f32,
                    ))
                } else {
                    None
                },
            ) as usize;
        }
    }
    written
}

/// Diamond-exit test in x+y / x-y coordinates. The terminal pixel is excluded,
/// so connected segments do not blend their shared endpoint twice.
fn diamond_exit(a: [f64; 2], b: [f64; 2], center: [f64; 2]) -> bool {
    let mut enter = 0f64;
    let mut exit = 1f64;
    for (start, end, c) in [
        (a[0] + a[1], b[0] + b[1], center[0] + center[1]),
        (a[0] - a[1], b[0] - b[1], center[0] - center[1]),
    ] {
        let d = end - start;
        if d == 0. {
            if (start - c).abs() >= 0.5 {
                return false;
            }
        } else {
            let t0 = (c - 0.5 - start) / d;
            let t1 = (c + 0.5 - start) / d;
            enter = enter.max(t0.min(t1));
            exit = exit.min(t0.max(t1));
        }
    }
    enter < exit && exit < 1. && exit > 0.
}

pub fn draw_line(
    target: &mut RenderTarget,
    state: &PipelineState,
    sample: SampleFn<'_>,
    stages: &[TextureStage],
    mut ends: [Vertex; 2],
) -> usize {
    let flat = ends[1].color;
    if ends.iter().any(|v| v.pos.iter().any(|x| !x.is_finite())) {
        return 0;
    }
    for plane in 0..6 {
        let a = clip_distance(&ends[0], plane);
        let b = clip_distance(&ends[1], plane);
        if a < 0. && b < 0. {
            return 0;
        }
        if (a < 0.) != (b < 0.) {
            let v = lerp_vertex(&ends[0], &ends[1], a / (a - b));
            if a < 0. {
                ends[0] = v;
            } else {
                ends[1] = v;
            }
        }
    }
    let Some(a) = window_position(target, state, &ends[0]) else {
        return 0;
    };
    let Some(b) = window_position(target, state, &ends[1]) else {
        return 0;
    };
    let delta = [b[0] - a[0], b[1] - a[1]];
    let length = delta[0] * delta[0] + delta[1] * delta[1];
    if length == 0. {
        return 0;
    }
    let Some((x0, y0, x1, y1)) = target.scissor_bounds(state.scissor) else {
        return 0;
    };
    let major = if delta[0].abs() >= delta[1].abs() {
        0
    } else {
        1
    };
    let minor = 1 - major;
    let width = state.line_width.clamp(1., 64.).round() as i32;
    let shift = if width % 2 == 0 { 0.5 } else { 0. };
    let mut start = [a[0] - 0.0001, a[1] + 0.00000001];
    let mut end = [b[0] - 0.0001, b[1] + 0.00000001];
    start[minor] += shift;
    end[minor] += shift;
    let bounds = if major == 0 {
        (x0, x1, y0, y1)
    } else {
        (y0, y1, x0, x1)
    };
    let lo = (start[major].min(end[major]).floor() as i32)
        .saturating_sub(1)
        .max(bounds.0);
    let hi = (start[major].max(end[major]).ceil() as i32)
        .saturating_add(1)
        .min(bounds.1);
    let coordinates = coordinate_units(sample, stages);
    let needs_gradients = sample.needs_gradients();
    let mut written = 0;
    for j in lo..hi {
        let t = (f64::from(j) + 0.5 - start[major]) / (end[major] - start[major]);
        let m = (start[minor] + t * (end[minor] - start[minor])).floor() as i32;
        for base in m.saturating_sub(1)..=m.saturating_add(1) {
            let mut center = [0.; 2];
            center[major] = f64::from(j) + 0.5;
            center[minor] = f64::from(base) + 0.5;
            if !diamond_exit(start, end, center) {
                continue;
            }
            for offset in 0..width {
                let k = base.saturating_add(offset).saturating_sub(width / 2);
                if k < bounds.2 || k >= bounds.3 {
                    continue;
                }
                let (x, y) = if major == 0 { (j, k) } else { (k, j) };
                let t = (((f64::from(x) + 0.5 - a[0]) * delta[0]
                    + (f64::from(y) + 0.5 - a[1]) * delta[1])
                    / length)
                    .clamp(0., 1.) as f32;
                let iw = (1. - t) / ends[0].pos[3] + t / ends[1].pos[3];
                if iw <= 0. || !iw.is_finite() {
                    continue;
                }
                let p = (t / ends[1].pos[3]) / iw;
                let mut v = lerp_vertex(&ends[0], &ends[1], p);
                if !state.smooth_shading {
                    v.color = flat;
                }
                let mut grad = [[0.; 4]; MAX_TEXTURE_STAGES];
                if needs_gradients {
                    let dpdt = 1. / (ends[0].pos[3] * ends[1].pos[3] * iw * iw);
                    for unit in 0..MAX_TEXTURE_STAGES {
                        if coordinates & (1 << unit) == 0 {
                            continue;
                        }
                        for axis in 0..2 {
                            for c in 0..2 {
                                let q = v.texcoord[unit][3];
                                let ds = ends[1].texcoord[unit][c] - ends[0].texcoord[unit][c];
                                let dq = ends[1].texcoord[unit][3] - ends[0].texcoord[unit][3];
                                grad[unit][axis * 2 + c] = if q != 0. {
                                    (ds * q - v.texcoord[unit][c] * dq) / (q * q)
                                        * dpdt
                                        * (delta[axis] / length) as f32
                                } else {
                                    0.
                                };
                            }
                        }
                    }
                }
                let z = a[2] + f64::from(t) * (b[2] - a[2]);
                written += primitive_fragment(
                    target, state, sample, stages, x, y, z as f32, &v, &grad, None,
                ) as usize;
            }
        }
    }
    written
}

/// The `glTexEnv` colour combiners GL ES 1.1 defines for a single unit.
/// `frag` is the incoming colour: the interpolated vertex colour for the
/// first stage, and the previous stage's result for every stage after.
fn apply_tex_env(
    stage: &TextureStage,
    frag: [f32; 4],
    tex: [f32; 4],
    primary: [f32; 4],
) -> [f32; 4] {
    match stage.env {
        TexEnvMode::Combine(c) => c.evaluate(frag, tex, stage.env_color, primary),
        TexEnvMode::Replace => tex,
        TexEnvMode::Modulate => [
            frag[0] * tex[0],
            frag[1] * tex[1],
            frag[2] * tex[2],
            frag[3] * tex[3],
        ],
        // DECAL blends RGB by the texture alpha and keeps fragment alpha.
        TexEnvMode::Decal => [
            frag[0] * (1.0 - tex[3]) + tex[0] * tex[3],
            frag[1] * (1.0 - tex[3]) + tex[1] * tex[3],
            frag[2] * (1.0 - tex[3]) + tex[2] * tex[3],
            frag[3],
        ],
        // BLEND interpolates towards the constant env colour.
        TexEnvMode::Blend => [
            frag[0] * (1.0 - tex[0]) + stage.env_color[0] * tex[0],
            frag[1] * (1.0 - tex[1]) + stage.env_color[1] * tex[1],
            frag[2] * (1.0 - tex[2]) + stage.env_color[2] * tex[2],
            frag[3] * tex[3],
        ],
        TexEnvMode::Add => [
            (frag[0] + tex[0]).min(1.0),
            (frag[1] + tex[1]).min(1.0),
            (frag[2] + tex[2]).min(1.0),
            frag[3] * tex[3],
        ],
    }
}

/// Fog blend factor: 1.0 means fully unfogged.
fn fog_factor(state: &PipelineState, depth: f32) -> f32 {
    let depth = depth.max(0.0);
    let density = state.fog_density.max(0.0);
    let f = match state.fog_mode {
        FogMode::Linear => {
            let span = state.fog_end - state.fog_start;
            if span.abs() < 1e-9 {
                1.0
            } else {
                (state.fog_end - depth) / span
            }
        }
        FogMode::Exp => (-density * depth).exp(),
        FogMode::Exp2 => {
            let d = density * depth;
            (-(d * d)).exp()
        }
    };
    f.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target_240x320() -> RenderTarget {
        RenderTarget::new(240, 320)
    }

    fn full_viewport(t: &RenderTarget) -> PipelineState {
        PipelineState {
            viewport: (0, 0, t.width as i32, t.height as i32),
            ..Default::default()
        }
    }

    fn no_texture(_unit: usize, _s: f32, _t: f32) -> Option<[u8; 4]> {
        None
    }

    /// A single stage on unit 0 with the given combiner.
    fn stage(env: TexEnvMode) -> [TextureStage; 1] {
        [TextureStage {
            unit: 0,
            env,
            env_color: [0.0; 4],
            lod_bias: 0.0,
        }]
    }

    /// A triangle covering the whole lower-left half of clip space.
    fn primitive_vertex(x: f32, y: f32, z: f32, color: [f32; 4]) -> Vertex {
        Vertex {
            pos: [x, y, z, 1.],
            color,
            ..Default::default()
        }
    }

    #[test]
    fn intel_depth_tolerance_preserves_explicit_and_translucent_policies() {
        let base = PipelineState {
            depth_test: true,
            depth_func: CompareFunc::Gequal,
            intel2700g_depth_bias: true,
            blend: true,
            blend_src: BlendFactor::SrcAlpha,
            blend_dst: BlendFactor::OneMinusSrcAlpha,
            ..Default::default()
        };
        let pass = base.intel2700g_depth_color_pass().unwrap();
        assert!(!pass.depth_write);
        assert_eq!(
            pass.polygon_units / pass.polygon_offset_resolution(),
            1. / 4096.
        );
        assert_eq!(pass.alpha_func, CompareFunc::Equal);
        assert_eq!(pass.alpha_ref, 1.);
        assert!(pass.intel2700g_depth_color_pass().is_none());
        for change in 0..10 {
            let mut state = base.clone();
            match change {
                0 => state.intel2700g_depth_bias = false,
                1 => state.depth_test = false,
                2 => state.depth_write = false,
                3 => state.depth_range = (0.2, 0.8),
                4 => state.depth_func = CompareFunc::Greater,
                5 => state.logic_op = true,
                6 => state.polygon_offset = true,
                7 => state.blend_dst = BlendFactor::One,
                8 => {
                    state.alpha_test = true;
                    state.alpha_func = CompareFunc::Less;
                    state.alpha_ref = 1.;
                }
                9 => state.color_mask = [false; 4],
                _ => unreachable!(),
            }
            assert!(
                state.intel2700g_depth_color_pass().is_none(),
                "case {change}"
            );
        }
        let mut forward = base;
        forward.depth_func = CompareFunc::Lequal;
        assert_eq!(
            forward.intel2700g_depth_color_pass().unwrap().polygon_units,
            -16.
        );
    }

    #[test]
    fn goforce_depth_tolerance_excludes_blending_and_explicit_depth_policies() {
        let mut state = PipelineState {
            depth_test: true,
            goforce4500_depth_tolerance: true,
            ..Default::default()
        };
        assert!(state.goforce_depth_color_pass().is_some());
        state.blend = true;
        assert!(state.goforce_depth_color_pass().is_none());
        state.blend = false;
        state.polygon_offset = true;
        assert!(state.goforce_depth_color_pass().is_none());
        state.polygon_offset = false;
        state.depth_func = CompareFunc::Equal;
        assert!(state.goforce_depth_color_pass().is_none());
        state.depth_func = CompareFunc::Less;
        state.depth_write = false;
        assert!(state.goforce_depth_color_pass().is_none());
        state.depth_write = true;
        state.depth_range = (0.5, 1.);
        assert!(state.goforce_depth_color_pass().is_none());
        state.depth_range = (0., 1.);
        state.goforce4500_depth_tolerance = false;
        assert!(state.goforce_depth_color_pass().is_none());
    }

    #[test]
    fn intel2700g_depth_bias_matches_original_driver_integer_vectors() {
        let mut state = PipelineState::default();
        assert!(!state.intel2700g_depth_bias);
        state.polygon_factor = -512.;
        state.polygon_units = -512.;
        assert_eq!(state.polygon_offset_parameters(), (-512., -512.));
        assert_eq!(state.depth_bits(), 16);
        state.intel2700g_depth_bias = true;
        assert_eq!(state.depth_bits(), 24);
        // Observed by executing the original driver, including arithmetic
        // shift of negative fractional 16.16 values (not truncation to zero).
        for (factor, units, expected) in [
            (-512., -512., -512.),
            (0., -512., -512.),
            (1., -512., -512.),
            (256., -0.25, -1.),
            (0., 1.5, 1.),
            (0., -32768., -32768.),
        ] {
            state.polygon_factor = factor;
            state.polygon_units = units;
            assert_eq!(state.polygon_offset_parameters(), (0., expected));
        }
        state.intel2700g_depth_bias = false;
        assert_eq!(state.polygon_offset_parameters(), (0., -32768.));
    }

    #[test]
    fn intel2700g_constant_bias_keeps_projected_light_behind_foreground() {
        fn scene(legacy: bool) -> [u8; 4] {
            let mut target = RenderTarget::new(32, 32);
            let mut state = PipelineState {
                viewport: (0, 0, 32, 32),
                depth_test: true,
                intel2700g_depth_bias: legacy,
                ..Default::default()
            };
            let foreground = [(-0.9, -0.9), (0.9, -0.9), (0., 0.9)]
                .map(|(x, y)| primitive_vertex(x, y, -0.2, [0., 0., 1., 1.]));
            draw_triangle(&mut target, &state, &|_, _, _| None, &[], foreground);
            let light = [(-0.9, -0.9, 0.2), (0.9, -0.9, 0.4), (0., 0.9, 0.3)]
                .map(|(x, y, z)| primitive_vertex(x, y, z, [1., 0., 0., 1.]));
            state.polygon_offset = true;
            state.polygon_factor = -512.;
            state.polygon_units = -512.;
            state.blend = true;
            state.blend_src = BlendFactor::One;
            state.blend_dst = BlendFactor::One;
            draw_triangle(&mut target, &state, &|_, _, _| None, &[], light);
            target.color[(16 * 32 + 16) * 4..(16 * 32 + 16) * 4 + 4]
                .try_into()
                .unwrap()
        }
        assert_eq!(scene(true), [0, 0, 255, 255]);
        assert_eq!(scene(false), [255, 0, 255, 255]);
    }

    #[test]
    fn intel2700g_bias_uses_fixed_input_precision_not_depth_storage_precision() {
        let mut target = RenderTarget::new(32, 32);
        let mut state = PipelineState {
            viewport: (0, 0, 32, 32),
            depth_test: true,
            intel2700g_depth_bias: true,
            ..Default::default()
        };
        assert_eq!(state.depth_bits(), 24);
        assert_eq!(state.polygon_offset_resolution(), 65536.);
        let triangle = |z, color| {
            [(-0.9, -0.9), (0.9, -0.9), (0., 0.9)].map(|(x, y)| primitive_vertex(x, y, z, color))
        };
        draw_triangle(
            &mut target,
            &state,
            &|_, _, _| None,
            &[],
            triangle(0., [0., 0., 1., 1.]),
        );
        state.polygon_offset = true;
        state.polygon_factor = -512.;
        state.polygon_units = -32.;
        draw_triangle(
            &mut target,
            &state,
            &|_, _, _| None,
            &[],
            triangle(1. / 2048., [0., 1., 0., 1.]),
        );
        let pixel = (16 * 32 + 16) * 4;
        assert_eq!(&target.color[pixel..pixel + 4], &[0, 255, 0, 255]);
        state.intel2700g_depth_bias = false;
        assert_eq!(state.polygon_offset_resolution(), 65535.);
    }

    #[test]
    fn polygon_offset_moves_coplanar_triangle_depth() {
        let mut target = RenderTarget::new(16, 16);
        let mut state = PipelineState {
            viewport: (0, 0, 16, 16),
            depth_test: true,
            ..Default::default()
        };
        let tri =
            [[-0.8, -0.8, 0., 1.], [0.8, -0.8, 0., 1.], [0., 0.8, 0., 1.]].map(|pos| Vertex {
                pos,
                color: [1., 0., 0., 1.],
                ..Default::default()
            });
        draw_triangle(&mut target, &state, &|_, _, _| None, &[], tri);
        let green = tri.map(|v| Vertex {
            color: [0., 1., 0., 1.],
            ..v
        });
        assert_eq!(
            draw_triangle(&mut target, &state, &|_, _, _| None, &[], green),
            0
        );
        state.polygon_offset = true;
        state.polygon_units = -2.;
        assert!(draw_triangle(&mut target, &state, &|_, _, _| None, &[], green) > 0);
        assert_eq!(
            &target.color[(8 * 16 + 8) * 4..(8 * 16 + 8) * 4 + 4],
            &[0, 255, 0, 255]
        );
    }

    #[test]
    fn points_clip_center_ignore_culling_and_respect_size_scissor() {
        let mut target = RenderTarget::new(32, 32);
        let mut state = PipelineState {
            viewport: (0, 0, 32, 32),
            point_size: 3.,
            cull: Some(CullMode::FrontAndBack),
            ..Default::default()
        };
        let sample = |_: usize, _: f32, _: f32| None;
        let v = primitive_vertex(0., 0., 0., [1.; 4]);
        assert_eq!(draw_point(&mut target, &state, &sample, &[], v), 9);
        assert_eq!(
            draw_point(
                &mut target,
                &state,
                &sample,
                &[],
                primitive_vertex(1.01, 0., 0., [1.; 4])
            ),
            0
        );
        state.point_size = 4.;
        assert_eq!(draw_point(&mut target, &state, &sample, &[], v), 16);
        state.scissor = Some((16, 16, 1, 1));
        assert_eq!(draw_point(&mut target, &state, &sample, &[], v), 1);
        let invalid = Vertex {
            pos: [0., 0., 0., 0.],
            ..v
        };
        assert_eq!(draw_point(&mut target, &state, &sample, &[], invalid), 0);
    }

    #[test]
    fn line_clipping_flat_shading_and_width_keep_fragment_operations() {
        let mut target = RenderTarget::new(32, 32);
        let mut state = PipelineState {
            viewport: (0, 0, 32, 32),
            smooth_shading: false,
            cull: Some(CullMode::FrontAndBack),
            ..Default::default()
        };
        let sample = |_: usize, _: f32, _: f32| None;
        let a = primitive_vertex(-2., 0., 0., [1., 0., 0., 1.]);
        let b = primitive_vertex(2., 0., 0., [0., 1., 0., 1.]);
        assert_eq!(draw_line(&mut target, &state, &sample, &[], [a, b]), 32);
        assert!(target
            .color
            .chunks_exact(4)
            .filter(|p| p[3] > 0)
            .all(|p| p == [0, 255, 0, 255]));
        state.line_width = 3.;
        assert_eq!(draw_line(&mut target, &state, &sample, &[], [a, b]), 96);
        assert_eq!(draw_line(&mut target, &state, &sample, &[], [a, a]), 0);
        let outside = primitive_vertex(0., 2., 0., [1.; 4]);
        assert_eq!(
            draw_line(
                &mut target,
                &state,
                &sample,
                &[],
                [outside, primitive_vertex(0.5, 2., 0., [1.; 4])]
            ),
            0
        );
        state.depth_test = true;
        target.depth.fill(0.);
        assert_eq!(draw_line(&mut target, &state, &sample, &[], [a, b]), 0);
        state.depth_test = false;
        state.alpha_test = true;
        state.alpha_func = CompareFunc::Greater;
        state.alpha_ref = 1.;
        assert_eq!(draw_line(&mut target, &state, &sample, &[], [a, b]), 0);
    }

    #[test]
    fn joined_lines_blend_shared_endpoint_once() {
        let mut target = RenderTarget::new(32, 32);
        let state = PipelineState {
            viewport: (0, 0, 32, 32),
            blend: true,
            blend_src: BlendFactor::SrcAlpha,
            blend_dst: BlendFactor::OneMinusSrcAlpha,
            ..Default::default()
        };
        let sample = |_: usize, _: f32, _: f32| None;
        let color = [1., 0., 0., 0.5];
        let a = primitive_vertex(-0.75, 0.25, 0., color);
        let b = primitive_vertex(0., 0.25, 0., color);
        let c = primitive_vertex(0.75, 0.25, 0., color);
        let count = draw_line(&mut target, &state, &sample, &[], [a, b])
            + draw_line(&mut target, &state, &sample, &[], [b, c]);
        assert_eq!(count, 24);
        assert!(target
            .color
            .chunks_exact(4)
            .filter(|p| p[0] > 0)
            .all(|p| p[0] == 128));
    }

    #[test]
    fn point_uses_texture_fog_blend_color_mask_and_depth_write() {
        let mut target = RenderTarget::new(32, 32);
        let state = PipelineState {
            viewport: (0, 0, 32, 32),
            point_size: 3.,
            fog: true,
            fog_mode: FogMode::Linear,
            fog_start: 0.,
            fog_end: 1.,
            fog_color: [0., 0., 1., 1.],
            blend: true,
            blend_src: BlendFactor::SrcAlpha,
            blend_dst: BlendFactor::OneMinusSrcAlpha,
            color_mask: [true, false, true, true],
            depth_test: true,
            ..Default::default()
        };
        let sample = |_: usize, _: f32, _: f32| Some([255, 128, 0, 128]);
        let stages = [TextureStage {
            unit: 0,
            env: TexEnvMode::Modulate,
            env_color: [0.; 4],
            lod_bias: 0.0,
        }];
        let mut v = primitive_vertex(0.1, 0.1, 0., [1.; 4]);
        v.fog_depth = 0.5;
        assert_eq!(draw_point(&mut target, &state, &sample, &stages, v), 9);
        for (i, p) in target
            .color
            .chunks_exact(4)
            .enumerate()
            .filter(|(_, p)| p[3] > 0)
        {
            assert!((i32::from(p[0]) - 64).abs() <= 1);
            assert_eq!(p[1], 0);
            assert!((i32::from(p[2]) - 64).abs() <= 1);
            assert_eq!(target.depth[i], 0.5);
        }
    }

    fn big_tri(color: [f32; 4]) -> [Vertex; 3] {
        [
            Vertex {
                pos: [-1.0, -1.0, 0.0, 1.0],
                color,
                ..Default::default()
            },
            Vertex {
                pos: [1.0, -1.0, 0.0, 1.0],
                color,
                ..Default::default()
            },
            Vertex {
                pos: [-1.0, 1.0, 0.0, 1.0],
                color,
                ..Default::default()
            },
        ]
    }

    fn pixel(t: &RenderTarget, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * t.width + x) * 4) as usize;
        [t.color[i], t.color[i + 1], t.color[i + 2], t.color[i + 3]]
    }

    #[test]
    fn inside_fast_path_matches_reference_clipper_color_depth_and_count() {
        let mut seed = 12345u32;
        let mut random = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / 16777216.0
        };
        for case in 0..128 {
            let mut tri = big_tri([0.8, 0.2, 0.6, 0.5]);
            for v in &mut tri {
                let w = 0.25 + random() * 2.0;
                v.pos = [
                    (random() * 2.0 - 1.0) * w,
                    (random() * 2.0 - 1.0) * w,
                    (random() * 2.0 - 1.0) * w,
                    w,
                ];
                v.color = [random(), random(), random(), random()];
            }
            // Include exact clip-plane boundaries and an intersecting primitive.
            if case % 4 == 0 {
                tri = big_tri([0.8, 0.2, 0.6, 0.5]);
            }
            if case % 4 == 1 {
                tri[0].pos[0] = -2.0 * tri[0].pos[3];
            }
            let mut fast = RenderTarget::new(
                if case % 2 == 0 { 2 } else { 32 },
                if case % 2 == 0 { 2 } else { 24 },
            );
            let mut reference = RenderTarget::new(
                if case % 2 == 0 { 2 } else { 32 },
                if case % 2 == 0 { 2 } else { 24 },
            );
            fast.color.fill(73);
            reference.color.fill(73);
            let mut state = full_viewport(&fast);
            state.smooth_shading = case % 2 == 0;
            state.depth_test = true;
            state.blend = case % 3 == 0;
            let a = draw_triangle(&mut fast, &state, &no_texture, &[], tri);
            let b = draw_triangle_clipped(&mut reference, &state, &no_texture, &[], tri);
            assert_eq!(a, b, "fragment count case {case}");
            assert_eq!(fast.color, reference.color, "color case {case}");
            assert_eq!(fast.depth, reference.depth, "depth case {case}");
        }
    }

    #[test]
    #[ignore = "manual release benchmark; timing is not a pass/fail assertion"]
    fn benchmark_inside_triangle_fast_path() {
        use std::{hint::black_box, time::Instant};
        for size in [2, 8, 64] {
            let iterations = if size <= 8 { 100000 } else { 3000 };
            let mut target = RenderTarget::new(size, size);
            let state = full_viewport(&target);
            let sample: SampleFn<'_> = black_box(&no_texture as SampleFn<'_>);
            let stages: &[TextureStage] = black_box(&[] as &[TextureStage]);
            let tri = big_tri([0.8, 0.2, 0.6, 1.0]);
            let mut old = Vec::new();
            let mut fast = Vec::new();
            for _ in 0..5 {
                for reference in [false, true] {
                    let render = black_box(if reference {
                        draw_triangle_clipped
                    } else {
                        draw_triangle
                    });
                    let start = Instant::now();
                    for _ in 0..iterations {
                        black_box(render(
                            black_box(&mut target),
                            black_box(&state),
                            sample,
                            stages,
                            black_box(tri),
                        ));
                    }
                    if reference {
                        old.push(start.elapsed());
                    } else {
                        fast.push(start.elapsed());
                    }
                }
            }
            old.sort();
            fast.sort();
            println!("{size}x{size}, {iterations} triangles: reference={:?}, optimized={:?}, speedup={:.3}x",
                old[2], fast[2], old[2].as_secs_f64()/fast[2].as_secs_f64());
        }
    }

    #[test]
    fn triangle_covers_expected_pixels() {
        let mut t = target_240x320();
        let s = full_viewport(&t);
        let n = draw_triangle(&mut t, &s, &no_texture, &[], big_tri([1.0, 0.0, 0.0, 1.0]));
        assert!(n > 0, "triangle produced no fragments");
        // Bottom-left of the GL viewport is the bottom-left row of our
        // buffer: inside the triangle.
        assert_eq!(pixel(&t, 2, 317), [255, 0, 0, 255]);
        // Top-right corner is outside the lower-left half.
        assert_eq!(pixel(&t, 237, 2), [0, 0, 0, 0]);
    }

    #[test]
    fn y_axis_is_flipped_for_the_color_buffer() {
        // A triangle in the *upper* half of GL clip space must land in
        // the *top* rows of the buffer. Getting this backwards renders
        // every game upside down.
        let mut t = RenderTarget::new(8, 8);
        let s = full_viewport(&t);
        let tri = [
            Vertex {
                pos: [-1.0, 0.1, 0.0, 1.0],
                color: [0.0, 1.0, 0.0, 1.0],
                ..Default::default()
            },
            Vertex {
                pos: [1.0, 0.1, 0.0, 1.0],
                color: [0.0, 1.0, 0.0, 1.0],
                ..Default::default()
            },
            Vertex {
                pos: [0.0, 1.0, 0.0, 1.0],
                color: [0.0, 1.0, 0.0, 1.0],
                ..Default::default()
            },
        ];
        draw_triangle(&mut t, &s, &no_texture, &[], tri);
        assert_eq!(pixel(&t, 4, 0)[1], 255, "top row should be covered");
        assert_eq!(pixel(&t, 4, 7)[1], 0, "bottom row should be empty");
    }

    #[test]
    fn back_face_culling_discards_clockwise_triangles() {
        let mut t = target_240x320();
        let mut s = full_viewport(&t);
        s.cull = Some(CullMode::Back);
        s.front_face = FrontFace::Ccw;
        // big_tri is counter-clockwise in GL's frame, so it survives.
        assert!(draw_triangle(&mut t, &s, &no_texture, &[], big_tri([1.0; 4])) > 0);
        // Reversing the winding must make it vanish.
        let mut rev = big_tri([1.0; 4]);
        rev.swap(1, 2);
        let mut t2 = target_240x320();
        assert_eq!(draw_triangle(&mut t2, &s, &no_texture, &[], rev), 0);
    }

    #[test]
    fn front_face_setting_inverts_which_side_is_culled() {
        let mut t = target_240x320();
        let mut s = full_viewport(&t);
        s.cull = Some(CullMode::Back);
        s.front_face = FrontFace::Cw;
        // With CW declared front, the CCW triangle is now the back face.
        assert_eq!(
            draw_triangle(&mut t, &s, &no_texture, &[], big_tri([1.0; 4])),
            0
        );
    }

    #[test]
    fn depth_test_rejects_farther_fragments() {
        let mut t = target_240x320();
        let mut s = full_viewport(&t);
        s.depth_test = true;
        s.depth_func = CompareFunc::Less;

        let mut near = big_tri([1.0, 0.0, 0.0, 1.0]);
        for v in near.iter_mut() {
            v.pos[2] = -0.5;
        }
        let mut far = big_tri([0.0, 1.0, 0.0, 1.0]);
        for v in far.iter_mut() {
            v.pos[2] = 0.5;
        }

        draw_triangle(&mut t, &s, &no_texture, &[], near);
        let after_near = pixel(&t, 2, 317);
        assert_eq!(after_near, [255, 0, 0, 255]);
        // The farther triangle must not overwrite it.
        assert_eq!(draw_triangle(&mut t, &s, &no_texture, &[], far), 0);
        assert_eq!(pixel(&t, 2, 317), [255, 0, 0, 255]);
    }

    #[test]
    fn depth_write_disabled_leaves_buffer_untouched() {
        let mut t = target_240x320();
        let mut s = full_viewport(&t);
        s.depth_test = true;
        s.depth_write = false;
        let before = t.depth[317 * 240 + 2];
        draw_triangle(&mut t, &s, &no_texture, &[], big_tri([1.0; 4]));
        assert_eq!(t.depth[317 * 240 + 2], before);
    }

    #[test]
    fn alpha_test_discards_below_reference() {
        let mut t = target_240x320();
        let mut s = full_viewport(&t);
        s.alpha_test = true;
        s.alpha_func = CompareFunc::Greater;
        s.alpha_ref = 0.5;
        // Alpha 0.25 fails a `> 0.5` test everywhere.
        assert_eq!(
            draw_triangle(&mut t, &s, &no_texture, &[], big_tri([1.0, 1.0, 1.0, 0.25])),
            0
        );
        assert!(draw_triangle(&mut t, &s, &no_texture, &[], big_tri([1.0, 1.0, 1.0, 0.75])) > 0);
    }

    #[test]
    fn src_alpha_blending_halves_a_white_triangle_over_black() {
        let mut t = target_240x320();
        let mut s = full_viewport(&t);
        s.blend = true;
        s.blend_src = BlendFactor::SrcAlpha;
        s.blend_dst = BlendFactor::OneMinusSrcAlpha;
        draw_triangle(&mut t, &s, &no_texture, &[], big_tri([1.0, 1.0, 1.0, 0.5]));
        let px = pixel(&t, 2, 317);
        // 0.5 * 255 rounds to 128.
        assert!(
            (px[0] as i32 - 128).abs() <= 1,
            "expected ~128, got {}",
            px[0]
        );
    }

    #[test]
    fn modulate_multiplies_texture_by_vertex_color() {
        let mut t = target_240x320();
        let s = full_viewport(&t);
        let half_grey = |_u: usize, _s: f32, _t: f32| Some([128u8, 128, 128, 255]);
        let stages = stage(TexEnvMode::Modulate);
        draw_triangle(
            &mut t,
            &s,
            &half_grey,
            &stages,
            big_tri([1.0, 0.0, 0.0, 1.0]),
        );
        let px = pixel(&t, 2, 317);
        assert!((px[0] as i32 - 128).abs() <= 1);
        assert_eq!(px[1], 0);
    }

    #[test]
    fn replace_ignores_vertex_color() {
        let mut t = target_240x320();
        let s = full_viewport(&t);
        let blue = |_u: usize, _s: f32, _t: f32| Some([0u8, 0, 255, 255]);
        let stages = stage(TexEnvMode::Replace);
        draw_triangle(&mut t, &s, &blue, &stages, big_tri([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(pixel(&t, 2, 317), [0, 0, 255, 255]);
    }

    #[test]
    fn a_later_stage_combines_with_the_colour_the_earlier_one_produced() {
        // Sticky Balls draws a ball as a DXT1 colour map — a format with
        // no alpha channel at all — on one stage and a white-RGB texture
        // whose shape lives in its alpha on the next. Only cascading the
        // stages in order both colours the ball and cuts it out of its
        // square; either stage on its own loses one of the two.
        let mut t = target_240x320();
        let s = full_viewport(&t);
        let layers = |unit: usize, _s: f32, _t: f32| match unit {
            0 => Some([0u8, 0, 255, 255]),
            _ => Some([255u8, 255, 255, 64]),
        };
        let stages = [
            TextureStage {
                unit: 0,
                env: TexEnvMode::Replace,
                env_color: [0.0; 4],
                lod_bias: 0.0,
            },
            TextureStage {
                unit: 1,
                env: TexEnvMode::Modulate,
                env_color: [0.0; 4],
                lod_bias: 0.0,
            },
        ];
        draw_triangle(&mut t, &s, &layers, &stages, big_tri([1.0, 0.0, 0.0, 1.0]));
        // Stage 0 replaces the red vertex colour with the map's blue;
        // stage 1's white RGB keeps that blue and its alpha carries
        // through as the fragment's.
        let px = pixel(&t, 2, 317);
        assert_eq!(px[..3], [0, 0, 255]);
        assert!((px[3] as i32 - 64).abs() <= 1, "mask alpha lost: {px:?}");
    }

    #[test]
    fn color_mask_protects_channels() {
        let mut t = target_240x320();
        let mut s = full_viewport(&t);
        s.color_mask = [true, false, false, true];
        draw_triangle(&mut t, &s, &no_texture, &[], big_tri([1.0, 1.0, 1.0, 1.0]));
        let px = pixel(&t, 2, 317);
        assert_eq!(px[0], 255);
        assert_eq!(px[1], 0, "green was masked off but got written");
        assert_eq!(px[2], 0);
    }

    #[test]
    fn scissor_box_confines_drawing_and_uses_gl_origin() {
        let mut t = RenderTarget::new(8, 8);
        let mut s = full_viewport(&t);
        // Bottom-left 4x4 quadrant in GL coordinates.
        s.scissor = Some((0, 0, 4, 4));
        draw_triangle(&mut t, &s, &no_texture, &[], big_tri([1.0; 4]));
        // Bottom-left rows (high row indices) are inside.
        assert_eq!(pixel(&t, 1, 7)[0], 255);
        // Row 3 is in the top half, outside the scissor.
        assert_eq!(pixel(&t, 1, 3)[0], 0);
    }

    #[test]
    fn empty_scissor_box_draws_nothing() {
        let mut t = RenderTarget::new(8, 8);
        let mut s = full_viewport(&t);
        s.scissor = Some((0, 0, 0, 0));
        assert_eq!(
            draw_triangle(&mut t, &s, &no_texture, &[], big_tri([1.0; 4])),
            0
        );
    }

    #[test]
    fn near_plane_clipping_keeps_the_visible_part() {
        let mut t = target_240x320();
        let s = full_viewport(&t);
        // One vertex behind the eye (z < -w) must be clipped, not
        // produce a garbage projection or a divide-by-zero.
        let tri = [
            Vertex {
                pos: [-1.0, -1.0, -2.0, 1.0],
                color: [1.0, 1.0, 1.0, 1.0],
                ..Default::default()
            },
            Vertex {
                pos: [1.0, -1.0, 0.0, 1.0],
                color: [1.0, 1.0, 1.0, 1.0],
                ..Default::default()
            },
            Vertex {
                pos: [-1.0, 1.0, 0.0, 1.0],
                color: [1.0, 1.0, 1.0, 1.0],
                ..Default::default()
            },
        ];
        let n = draw_triangle(&mut t, &s, &no_texture, &[], tri);
        assert!(n > 0, "fully clipped a partially visible triangle");
    }

    #[test]
    fn geometry_on_the_near_plane_is_visible() {
        let mut t = target_240x320();
        let s = full_viewport(&t);
        let tri = [
            Vertex {
                pos: [-1.0, -1.0, -1.0, 1.0],
                color: [1.0, 1.0, 1.0, 1.0],
                ..Default::default()
            },
            Vertex {
                pos: [1.0, -1.0, -1.0, 1.0],
                color: [1.0, 1.0, 1.0, 1.0],
                ..Default::default()
            },
            Vertex {
                pos: [-1.0, 1.0, -1.0, 1.0],
                color: [1.0, 1.0, 1.0, 1.0],
                ..Default::default()
            },
        ];
        assert!(draw_triangle(&mut t, &s, &no_texture, &[], tri) > 0);
    }

    #[test]
    fn fully_behind_the_eye_is_rejected() {
        let mut t = target_240x320();
        let s = full_viewport(&t);
        let mut tri = big_tri([1.0; 4]);
        for v in tri.iter_mut() {
            v.pos[2] = -5.0;
        }
        assert_eq!(draw_triangle(&mut t, &s, &no_texture, &[], tri), 0);
    }

    #[test]
    fn degenerate_triangle_is_rejected() {
        let mut t = target_240x320();
        let s = full_viewport(&t);
        let v = Vertex {
            pos: [0.0, 0.0, 0.0, 1.0],
            ..Default::default()
        };
        assert_eq!(draw_triangle(&mut t, &s, &no_texture, &[], [v, v, v]), 0);
    }

    #[test]
    fn flat_shading_uses_the_last_vertex_color() {
        let mut t = target_240x320();
        let mut s = full_viewport(&t);
        s.smooth_shading = false;
        let mut tri = big_tri([1.0; 4]);
        tri[0].color = [1.0, 0.0, 0.0, 1.0];
        tri[1].color = [0.0, 1.0, 0.0, 1.0];
        tri[2].color = [0.0, 0.0, 1.0, 1.0];
        draw_triangle(&mut t, &s, &no_texture, &[], tri);
        // Every covered pixel must be the provoking vertex's blue.
        assert_eq!(pixel(&t, 2, 317), [0, 0, 255, 255]);
    }

    #[test]
    fn smooth_shading_interpolates_between_vertices() {
        let mut t = RenderTarget::new(64, 64);
        let mut s = full_viewport(&t);
        s.smooth_shading = true;
        let tri = [
            Vertex {
                pos: [-1.0, -1.0, 0.0, 1.0],
                color: [1.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
            Vertex {
                pos: [1.0, -1.0, 0.0, 1.0],
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
            Vertex {
                pos: [-1.0, 1.0, 0.0, 1.0],
                color: [0.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        ];
        draw_triangle(&mut t, &s, &no_texture, &[], tri);
        // Near the red vertex (bottom-left) red is strong; far from it
        // (towards the black vertices) it decays.
        let near_red = pixel(&t, 1, 62)[0];
        let far_red = pixel(&t, 30, 33)[0];
        assert!(near_red > far_red, "no gradient: {near_red} vs {far_red}");
    }

    #[test]
    fn linear_fog_fades_to_the_fog_color() {
        let mut t = target_240x320();
        let mut s = full_viewport(&t);
        s.fog = true;
        s.fog_mode = FogMode::Linear;
        s.fog_start = 0.0;
        s.fog_end = 10.0;
        s.fog_color = [0.0, 0.0, 1.0, 1.0];
        let mut tri = big_tri([1.0, 0.0, 0.0, 1.0]);
        // Fully fogged: at fog_end the factor is 0, so the fragment
        // becomes pure fog colour.
        for v in tri.iter_mut() {
            v.fog_depth = 10.0;
        }
        draw_triangle(&mut t, &s, &no_texture, &[], tri);
        assert_eq!(pixel(&t, 2, 317), [0, 0, 255, 255]);
    }

    #[test]
    fn exponential_fog_is_unfogged_at_zero_depth() {
        let s = PipelineState {
            fog_mode: FogMode::Exp,
            fog_density: 0.25,
            ..PipelineState::default()
        };
        assert_eq!(fog_factor(&s, 0.0), 1.0);
        assert_eq!(fog_factor(&s, -10.0), 1.0);
        assert!((fog_factor(&s, 4.0) - (-1.0f32).exp()).abs() < 1e-6);
    }

    #[test]
    fn exp2_fog_uses_density_squared_in_the_exponent() {
        let s = PipelineState {
            fog_mode: FogMode::Exp2,
            fog_density: 0.5,
            ..PipelineState::default()
        };
        assert!((fog_factor(&s, 2.0) - (-1.0f32).exp()).abs() < 1e-6);
    }

    #[test]
    fn unfogged_geometry_keeps_its_color() {
        let mut t = target_240x320();
        let mut s = full_viewport(&t);
        s.fog = true;
        s.fog_mode = FogMode::Linear;
        s.fog_start = 5.0;
        s.fog_end = 10.0;
        s.fog_color = [0.0, 0.0, 1.0, 1.0];
        let mut tri = big_tri([1.0, 0.0, 0.0, 1.0]);
        for v in tri.iter_mut() {
            v.fog_depth = 0.0;
        }
        draw_triangle(&mut t, &s, &no_texture, &[], tri);
        assert_eq!(pixel(&t, 2, 317), [255, 0, 0, 255]);
    }

    #[test]
    fn clear_color_fills_the_whole_target() {
        let mut t = RenderTarget::new(4, 4);
        t.clear_color([10, 20, 30, 40], None);
        assert_eq!(pixel(&t, 0, 0), [10, 20, 30, 40]);
        assert_eq!(pixel(&t, 3, 3), [10, 20, 30, 40]);
    }

    #[test]
    fn clear_respects_the_scissor_box() {
        let mut t = RenderTarget::new(4, 4);
        t.clear_color([255, 255, 255, 255], Some((0, 0, 2, 2)));
        // Bottom-left 2x2 in GL space = rows 2..4, cols 0..2.
        assert_eq!(pixel(&t, 0, 3), [255, 255, 255, 255]);
        assert_eq!(pixel(&t, 3, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn clear_depth_resets_to_the_far_value() {
        let mut t = RenderTarget::new(2, 2);
        t.depth.fill(0.0);
        t.clear_depth(1.0, None);
        assert!(t.depth.iter().all(|&d| d == 1.0));
    }

    #[test]
    fn rgb565_conversion_preserves_pure_channels() {
        let mut t = RenderTarget::new(2, 1);
        t.color = vec![255, 0, 0, 255, 0, 0, 255, 255];
        let mut out = vec![0u8; 4];
        t.to_rgb565(&mut out);
        assert_eq!(u16::from_le_bytes([out[0], out[1]]), 0xF800);
        assert_eq!(u16::from_le_bytes([out[2], out[3]]), 0x001F);
    }

    #[test]
    fn perspective_divide_shrinks_distant_geometry() {
        // Scaling position and w together leaves NDC unchanged, so
        // coverage must be identical — that is what proves the divide
        // happens at all. Scaling position alone must shrink coverage.
        let s = PipelineState {
            viewport: (0, 0, 64, 64),
            ..Default::default()
        };
        let mut near = RenderTarget::new(64, 64);
        let mut far = RenderTarget::new(64, 64);
        let make = |w: f32| {
            [
                Vertex {
                    pos: [-w, -w, 0.0, w],
                    ..Default::default()
                },
                Vertex {
                    pos: [w, -w, 0.0, w],
                    ..Default::default()
                },
                Vertex {
                    pos: [-w, w, 0.0, w],
                    ..Default::default()
                },
            ]
        };
        // Same NDC coordinates, so identical coverage — this confirms
        // the divide happens at all.
        let a = draw_triangle(&mut near, &s, &no_texture, &[], make(1.0));
        let b = draw_triangle(&mut far, &s, &no_texture, &[], make(4.0));
        assert_eq!(a, b);
        // Now scale only the position, leaving w at 1: coverage must
        // shrink.
        let mut small = RenderTarget::new(64, 64);
        let shrunk = [
            Vertex {
                pos: [-0.25, -0.25, 0.0, 1.0],
                ..Default::default()
            },
            Vertex {
                pos: [0.25, -0.25, 0.0, 1.0],
                ..Default::default()
            },
            Vertex {
                pos: [-0.25, 0.25, 0.0, 1.0],
                ..Default::default()
            },
        ];
        let c = draw_triangle(&mut small, &s, &no_texture, &[], shrunk);
        assert!(c < a, "shrunk triangle covered {c}, full covered {a}");
    }

    #[test]
    fn zero_sized_viewport_draws_nothing() {
        let mut t = target_240x320();
        let s = PipelineState {
            viewport: (0, 0, 0, 0),
            ..Default::default()
        };
        assert_eq!(
            draw_triangle(&mut t, &s, &no_texture, &[], big_tri([1.0; 4])),
            0
        );
    }

    #[test]
    fn viewport_offsets_the_rendered_area() {
        let mut t = RenderTarget::new(16, 16);
        let s = PipelineState {
            // Right half only.
            viewport: (8, 0, 8, 16),
            ..Default::default()
        };
        draw_triangle(&mut t, &s, &no_texture, &[], big_tri([1.0; 4]));
        // Left half must be untouched.
        for y in 0..16 {
            assert_eq!(pixel(&t, 0, y)[0], 0, "row {y} leaked into the left half");
        }
        assert_eq!(pixel(&t, 9, 15)[0], 255);
    }

    #[test]
    fn disabled_depth_test_never_updates_depth_buffer() {
        let mut t = target_240x320();
        t.depth.fill(0.25);
        let s = PipelineState {
            depth_test: false,
            depth_write: true,
            ..Default::default()
        };
        draw_triangle(&mut t, &s, &no_texture, &[], big_tri([1.0; 4]));
        assert!(
            t.depth.iter().all(|&d| d == 0.25),
            "GL_DEPTH_TEST disabled must leave the depth buffer untouched"
        );
    }
    #[test]
    fn texture_gradients_follow_screen_axes_and_stage_bias() {
        use std::cell::RefCell;
        struct Probe(RefCell<Vec<([f32; 4], f32)>>);
        impl TextureSampler for Probe {
            fn sample(&self, _: usize, _: f32, _: f32) -> Option<[u8; 4]> {
                None
            }
            fn needs_gradients(&self) -> bool {
                true
            }
            fn sample_grad(
                &self,
                _: usize,
                _: f32,
                _: f32,
                g: [f32; 4],
                b: f32,
            ) -> Option<[u8; 4]> {
                self.0.borrow_mut().push((g, b));
                None
            }
        }
        let mut target = RenderTarget::new(8, 8);
        let state = PipelineState {
            viewport: (0, 0, 8, 8),
            ..Default::default()
        };
        let positions = [[-1., -1., 0., 1.], [1., -1., 0., 1.], [-1., 1., 0., 1.]];
        let uv = [[0., 0.], [8., 0.], [0., 8.]];
        let vertices = std::array::from_fn(|i| Vertex {
            pos: positions[i],
            texcoord: [[uv[i][0], uv[i][1], 0., 1.]; 6],
            ..Default::default()
        });
        let stage = TextureStage {
            unit: 0,
            env: TexEnvMode::Modulate,
            env_color: [0.; 4],
            lod_bias: 1.25,
        };
        let probe = Probe(RefCell::new(Vec::new()));
        assert!(draw_triangle(&mut target, &state, &probe, &[stage], vertices) > 0);
        assert!(!probe.0.borrow().is_empty());
        for (g, b) in probe.0.borrow().iter() {
            for (actual, expected) in g.iter().zip([1., 0., 0., -1.]) {
                assert!((actual - expected).abs() < 1e-5, "gradient {g:?}");
            }
            assert_eq!(*b, 1.25);
        }
    }

    #[test]
    fn perspective_texture_gradients_use_the_quotient_rule() {
        use std::cell::RefCell;
        struct Probe(RefCell<Vec<(f32, f32, [f32; 4])>>);
        impl TextureSampler for Probe {
            fn sample(&self, _: usize, _: f32, _: f32) -> Option<[u8; 4]> {
                None
            }
            fn needs_gradients(&self) -> bool {
                true
            }
            fn sample_grad(
                &self,
                _: usize,
                u: f32,
                v: f32,
                g: [f32; 4],
                _: f32,
            ) -> Option<[u8; 4]> {
                self.0.borrow_mut().push((u, v, g));
                None
            }
        }
        let mut target = RenderTarget::new(8, 8);
        let state = PipelineState {
            viewport: (0, 0, 8, 8),
            ..Default::default()
        };
        let positions = [[-1., -1., 0., 1.], [2., -2., 0., 2.], [-4., 4., 0., 4.]];
        let uv = [[0., 0.], [8., 0.], [0., 8.]];
        let vertices = std::array::from_fn(|i| Vertex {
            pos: positions[i],
            texcoord: [[uv[i][0], uv[i][1], 0., 1.]; 6],
            ..Default::default()
        });
        let stage = TextureStage {
            unit: 0,
            env: TexEnvMode::Modulate,
            env_color: [0.; 4],
            lod_bias: 0.,
        };
        let probe = Probe(RefCell::new(Vec::new()));
        draw_triangle(&mut target, &state, &probe, &[stage], vertices);
        // Pixel centre (0.5,1.5): barycentrics (0.125,0.0625,0.8125).
        let q = 0.359375;
        let u = 0.25 / q;
        let v = 1.625 / q;
        let expected = [
            (0.5 + u * 0.0625) / q,
            v * 0.0625 / q,
            -u * 0.09375 / q,
            (-0.25 - v * 0.09375) / q,
        ];
        let values = probe.0.borrow();
        let (_, _, g) = values
            .iter()
            .find(|(s, t, _)| (*s - u).abs() < 1e-5 && (*t - v).abs() < 1e-5)
            .expect("known covered pixel");
        for (actual, reference) in g.iter().zip(expected) {
            assert!((actual - reference).abs() < 1e-4, "{g:?} != {expected:?}");
        }
    }
}

/// Clip assembled, lit vertices before either rasterizer consumes them. Topology
/// is expanded only for draws which enable user planes, preserving fast draws.
pub fn clip_user_primitives(
    mode: u32,
    vertices: &[Vertex],
    planes: &[[f32; 4]],
    smooth: bool,
) -> (u32, Vec<Vertex>) {
    use crate::consts::*;
    let distance = |v: &Vertex, p: &[f32; 4]| (0..4).map(|i| v.eye[i] * p[i]).sum::<f32>();
    let mut out = Vec::new();
    if mode == GL_POINTS {
        out.extend(
            vertices
                .iter()
                .filter(|v| planes.iter().all(|p| distance(v, p) >= 0.))
                .copied(),
        );
        return (GL_POINTS, out);
    }
    let mut primitive = |mut v: Vec<Vertex>| {
        if !smooth {
            let last = *v.last().unwrap();
            for x in &mut v {
                x.color = last.color;
                x.back_color = last.back_color;
            }
        }
        if v.len() == 2 {
            for p in planes {
                let a = distance(&v[0], p);
                let b = distance(&v[1], p);
                if a < 0. && b < 0. {
                    return;
                }
                if (a < 0.) != (b < 0.) {
                    let x = lerp_vertex(&v[0], &v[1], a / (a - b));
                    if a < 0. {
                        v[0] = x;
                    } else {
                        v[1] = x;
                    }
                }
            }
            out.extend(v);
            return;
        }
        for p in planes {
            let input = std::mem::take(&mut v);
            if input.is_empty() {
                return;
            }
            for i in 0..input.len() {
                let a = &input[i];
                let b = &input[(i + 1) % input.len()];
                let da = distance(a, p);
                let db = distance(b, p);
                if da >= 0. {
                    v.push(*a);
                }
                if (da < 0.) != (db < 0.) {
                    v.push(lerp_vertex(a, b, da / (da - db)));
                }
            }
        }
        for i in 1..v.len().saturating_sub(1) {
            out.extend([v[0], v[i], v[i + 1]]);
        }
    };
    match mode {
        GL_LINES => {
            for v in vertices.chunks_exact(2) {
                primitive(v.to_vec());
            }
        }
        GL_LINE_STRIP | GL_LINE_LOOP => {
            for v in vertices.windows(2) {
                primitive(v.to_vec());
            }
            if mode == GL_LINE_LOOP && vertices.len() > 1 {
                primitive(vec![*vertices.last().unwrap(), vertices[0]]);
            }
        }
        GL_TRIANGLES => {
            for v in vertices.chunks_exact(3) {
                primitive(v.to_vec());
            }
        }
        GL_TRIANGLE_STRIP => {
            for i in 2..vertices.len() {
                primitive(if i % 2 == 0 {
                    vec![vertices[i - 2], vertices[i - 1], vertices[i]]
                } else {
                    vec![vertices[i - 1], vertices[i - 2], vertices[i]]
                });
            }
        }
        GL_TRIANGLE_FAN => {
            for i in 2..vertices.len() {
                primitive(vec![vertices[0], vertices[i - 1], vertices[i]]);
            }
        }
        _ => {}
    }
    (
        if matches!(mode, GL_LINES | GL_LINE_STRIP | GL_LINE_LOOP) {
            GL_LINES
        } else {
            GL_TRIANGLES
        },
        out,
    )
}

#[cfg(test)]
mod logical_truth_table_tests {
    #[test]
    fn all_operations_match_the_spec_truth_tables_for_every_byte_pair() {
        let tables = [
            0x0u8, 0x8, 0x4, 0xc, 0x2, 0xa, 0x6, 0xe, 0x1, 0x9, 0x5, 0xd, 0x3, 0xb, 0x7, 0xf,
        ];
        for (op, table) in tables.into_iter().enumerate() {
            for source in 0..=255u8 {
                for destination in 0..=255u8 {
                    let mut expected = 0;
                    for bit in 0..8 {
                        let index = (((source >> bit) & 1) << 1) | ((destination >> bit) & 1);
                        expected |= ((table >> index) & 1) << bit;
                    }
                    assert_eq!(
                        super::logic_byte(0x1500 + op as u32, source, destination),
                        expected
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod projective_coordinate_tests {
    use super::*;
    use std::cell::RefCell;
    struct Samples(RefCell<Vec<(f32, f32, [f32; 4])>>);
    impl TextureSampler for Samples {
        fn sample(&self, _: usize, u: f32, v: f32) -> Option<[u8; 4]> {
            Some([to_byte(u), to_byte(v), 255, 255])
        }
        fn needs_gradients(&self) -> bool {
            true
        }
        fn sample_grad(&self, unit: usize, u: f32, v: f32, g: [f32; 4], _: f32) -> Option<[u8; 4]> {
            self.0.borrow_mut().push((u, v, g));
            self.sample(unit, u, v)
        }
    }
    #[test]
    fn clipped_projective_triangles_match_independent_quotient_and_derivatives() {
        const N: u32 = 8;
        for clip_w in [[1., 1., 1.], [1., 2., 4.]] {
            let xy = [[-1., -1.], [3., -1.], [-1., 3.]];
            let uv = [[0., 1., 0.2, 1.], [2., 1., 0.4, 3.], [0., -1., 0.6, 1.]];
            let vertices = std::array::from_fn(|i| Vertex {
                pos: [xy[i][0] * clip_w[i], xy[i][1] * clip_w[i], 0., clip_w[i]],
                texcoord: [uv[i]; 6],
                ..Default::default()
            });
            let mut target = RenderTarget::new(N, N);
            // Unequal viewport dimensions keep triangulation edges off sample centers.
            let state = PipelineState {
                viewport: (0, 0, N as i32 + 1, N as i32),
                ..Default::default()
            };
            let stages = [TextureStage {
                unit: 0,
                env: TexEnvMode::Replace,
                env_color: [0.; 4],
                lod_bias: 0.,
            }];
            let samples = Samples(RefCell::new(Vec::new()));
            draw_triangle(&mut target, &state, &samples, &stages, vertices);
            let values = samples.0.borrow();
            assert_eq!(values.len(), (N * N) as usize);
            for y in 0..N {
                for x in 0..N {
                    let lx = (x as f32 + 0.5) / (2. * (N + 1) as f32);
                    let ly = (N as f32 - y as f32 - 0.5) / (2. * N as f32);
                    let l = [1. - lx - ly, lx, ly];
                    let mut a = [0.; 4];
                    let mut dx = [0.; 4];
                    let mut dy = [0.; 4];
                    let step_x = 1. / (2. * (N + 1) as f32);
                    let step_y = 1. / (2. * N as f32);
                    for component in 0..4 {
                        for i in 0..3 {
                            a[component] += l[i] * uv[i][component] / clip_w[i];
                            dx[component] +=
                                [-step_x, step_x, 0.][i] * uv[i][component] / clip_w[i];
                            dy[component] +=
                                [step_y, 0., -step_y][i] * uv[i][component] / clip_w[i];
                        }
                    }
                    let (u, v) = (a[0] / a[3], a[1] / a[3]);
                    let expected = [
                        (dx[0] * a[3] - a[0] * dx[3]) / (a[3] * a[3]),
                        (dx[1] * a[3] - a[1] * dx[3]) / (a[3] * a[3]),
                        (dy[0] * a[3] - a[0] * dy[3]) / (a[3] * a[3]),
                        (dy[1] * a[3] - a[1] * dy[3]) / (a[3] * a[3]),
                    ];
                    let found = values
                        .iter()
                        .find(|(s, t, _)| (*s - u).abs() < 1e-5 && (*t - v).abs() < 1e-5)
                        .expect("pixel sample follows s/q,t/q");
                    for i in 0..4 {
                        assert!(
                            (found.2[i] - expected[i]).abs() < 1e-5,
                            "x={x} y={y} derivative={i}: {:?} vs {expected:?}",
                            found.2
                        );
                    }
                    let offset = (y * N + x) as usize * 4;
                    assert!(target.color[offset].abs_diff(to_byte(u)) <= 1);
                    assert!(target.color[offset + 1].abs_diff(to_byte(v)) <= 1);
                }
            }
        }
    }
    #[test]
    fn clipping_interpolates_all_four_coordinates_before_projection() {
        let a = Vertex {
            texcoord: [[1., 2., 3., 4.]; 6],
            ..Default::default()
        };
        let b = Vertex {
            texcoord: [[5., 6., 7., 8.]; 6],
            ..Default::default()
        };
        assert_eq!(lerp_vertex(&a, &b, 0.25).texcoord, [[2., 3., 4., 5.]; 6]);
    }
    #[test]
    fn points_and_lines_preserve_homogeneous_coordinate_equivalence() {
        for line in [false, true] {
            let mut images = Vec::new();
            let state = PipelineState {
                viewport: (0, 0, 16, 16),
                point_size: 3.,
                ..Default::default()
            };
            let stages = [TextureStage {
                unit: 0,
                env: TexEnvMode::Replace,
                env_color: [0.; 4],
                lod_bias: 0.,
            }];
            for q in [1., 2., 4.] {
                let mut target = RenderTarget::new(16, 16);
                let sampler = Samples(RefCell::new(Vec::new()));
                let vertices = std::array::from_fn(|i| Vertex {
                    pos: [if i == 0 { -0.75 } else { 0.75 }, 0., 0., 1.],
                    texcoord: [[
                        if i == 0 { 0.25 * q } else { 0.75 * q },
                        0.25 * q,
                        0.5 * q,
                        q,
                    ]; 6],
                    ..Default::default()
                });
                let written = if line {
                    draw_line(&mut target, &state, &sampler, &stages, vertices)
                } else {
                    draw_point(&mut target, &state, &sampler, &stages, vertices[0])
                };
                assert!(written > 0);
                images.push(target.color);
            }
            assert_eq!(images[0], images[1]);
            assert_eq!(images[0], images[2]);
        }
    }
}

#[cfg(test)]
mod sprite_formula_tests {
    use super::*;
    use std::cell::RefCell;
    struct Capture(RefCell<Vec<[f32; 4]>>);
    impl TextureSampler for Capture {
        fn sample(&self, _: usize, s: f32, t: f32) -> Option<[u8; 4]> {
            Some([to_byte(s), to_byte(t), 0, 255])
        }
        fn needs_gradients(&self) -> bool {
            true
        }
        fn sample_grad(
            &self,
            unit: usize,
            s: f32,
            t: f32,
            gradient: [f32; 4],
            _: f32,
        ) -> Option<[u8; 4]> {
            self.0.borrow_mut().push(gradient);
            self.sample(unit, s, t)
        }
    }
    #[test]
    fn sprite_uses_unrounded_center_fixed_origin_and_unclipped_gradients() {
        let mut target = RenderTarget::new(16, 16);
        let mut state = PipelineState {
            viewport: (0, 0, 16, 16),
            point_sprite: true,
            point_coord_replace: [true; MAX_TEXTURE_STAGES],
            point_size: 4.,
            ..Default::default()
        };
        let stages = [TextureStage {
            unit: 0,
            env: TexEnvMode::Replace,
            env_color: [0.; 4],
            lod_bias: 0.,
        }];
        let samples = Capture(RefCell::new(Vec::new()));
        let mut point = Vertex {
            pos: [0.03125, 0.03125, 0., 1.],
            ..Default::default()
        };
        assert_eq!(
            draw_point(&mut target, &state, &samples, &stages, point),
            16
        );
        for (x, y, color) in [
            (6, 6, [16, 48, 0, 255]),
            (9, 6, [207, 48, 0, 255]),
            (6, 9, [16, 239, 0, 255]),
            (9, 9, [207, 239, 0, 255]),
        ] {
            let i = (y * 16 + x) * 4;
            assert_eq!(&target.color[i..i + 4], &color);
        }
        assert!(samples
            .0
            .borrow()
            .iter()
            .all(|g| *g == [0.25, 0., 0., 0.25]));
        target.clear_color([0; 4], None);
        point.pos = [-0.96875, 0.96875, 0., 1.];
        state.scissor = Some((0, 15, 1, 1));
        assert_eq!(draw_point(&mut target, &state, &samples, &stages, point), 1);
        assert_eq!(
            &target.color[..4],
            &[143, 143, 0, 255],
            "clipping must not stretch the visible part"
        );
        target.clear_color([0; 4], None);
        point.pos[0] = -1.01;
        assert_eq!(
            draw_point(&mut target, &state, &samples, &stages, point),
            0,
            "clip the point center before expanding its square"
        );
    }
}
