//! The OpenGL ES 1.1 context: state machine, vertex assembly, drawing.
//!
//! This is the layer the DLL entry points call into. It owns every piece
//! of GL state (matrix stacks, enables, texture objects, array
//! pointers), assembles primitives, and hands triangles to
//! [`crate::raster`].
//!
//! Vertex and texture data live in *guest* memory, which this crate has
//! no way to read directly. Callers therefore supply a [`GuestMemory`]
//! implementation; the emulator backs it with the CPU's address space
//! and the tests back it with a plain byte vector.

mod diagnostic;
mod es11;
mod sharing;
pub use es11::StateValue;
pub use sharing::{ObjectReferences, SharedObjects};

use crate::consts::*;
use crate::fixed;
use crate::matrix::{self, Matrix4, MatrixMode, MatrixStack};
use crate::raster::{
    self, BlendFactor, CompareFunc, CullMode, FogMode, FrontFace, PipelineState, RenderTarget,
    TexEnvMode, Vertex,
};
use crate::texture::{self, Texture};
use std::collections::HashMap;

type SoftwareMipLevels = Vec<(u32, u32, Vec<u8>)>;
struct SoftwareSampler<'a> {
    combiner: Option<&'a crate::combiner_program::BasicProgram>,
    combiner_constants: [[f32; 4]; 4],
    textures: [Option<&'a Texture>; raster::MAX_TEXTURE_STAGES],
    cache: &'a HashMap<u32, (u64, bool, SoftwareMipLevels)>,
    names: [u32; raster::MAX_TEXTURE_STAGES],
}
impl raster::TextureSampler for SoftwareSampler<'_> {
    fn combiner_program(&self) -> Option<&crate::combiner_program::BasicProgram> {
        self.combiner
    }

    fn combiner_constants(&self) -> [[f32; 4]; 4] {
        self.combiner_constants
    }
    fn sample(&self, unit: usize, s: f32, t: f32) -> Option<[u8; 4]> {
        let tex = self.textures[unit]?;
        let levels = self
            .cache
            .get(&self.names[unit])
            .map_or(&[][..], |c| c.2.as_slice());
        Some(tex.sample_lod_complete(levels, s, t, 0.))
    }
    fn needs_gradients(&self) -> bool {
        self.textures
            .iter()
            .flatten()
            .any(|t| t.min_filter.uses_mipmaps() || t.min_filter != t.mag_filter)
    }
    fn sample_grad(&self, unit: usize, s: f32, t: f32, g: [f32; 4], bias: f32) -> Option<[u8; 4]> {
        let tex = self.textures[unit]?;
        if !tex.min_filter.uses_mipmaps() && tex.min_filter == tex.mag_filter {
            return self.sample(unit, s, t);
        }
        let (w, h) = tex.sampling_size();
        let x = (g[0] * w as f32).powi(2) + (g[1] * h as f32).powi(2);
        let y = (g[2] * w as f32).powi(2) + (g[3] * h as f32).powi(2);
        let lod = 0.5 * x.max(y).max(1e-30).log2() + bias.clamp(-4., 4.);
        let levels = self
            .cache
            .get(&self.names[unit])
            .map_or(&[][..], |c| c.2.as_slice());
        Some(tex.sample_lod_complete(levels, s, t, lod))
    }
}

/// Read access to the guest's address space.
pub trait GuestMemory {
    /// Copy `len` bytes starting at `addr` into `out`. Returns `false`
    /// if any part of the range is unmapped, in which case the caller
    /// skips the draw rather than rendering garbage.
    fn read(&mut self, addr: u32, len: usize, out: &mut Vec<u8>) -> bool;
}

/// A `gl*Pointer` client array binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArrayPointer {
    pub enabled: bool,
    /// Components per element (2, 3 or 4).
    pub size: u32,
    /// One of `GL_BYTE`, `GL_SHORT`, `GL_FIXED`, `GL_FLOAT`,
    /// `GL_UNSIGNED_BYTE`.
    pub ty: u32,
    /// Byte stride between elements. Zero means tightly packed.
    pub stride: u32,
    /// Guest virtual address of the first element — or, when `buffer`
    /// is non-zero, a byte offset into that buffer object.
    pub pointer: u32,
    /// The `GL_ARRAY_BUFFER` bound when this pointer was set. GL ES 1.1
    /// captures the binding at `gl*Pointer` time, not at draw time, so
    /// it has to be remembered per array: a game may bind a VBO, set
    /// the vertex pointer, then bind a different VBO for texcoords.
    pub buffer: u32,
}

impl Default for ArrayPointer {
    fn default() -> Self {
        Self {
            enabled: false,
            size: 4,
            ty: GL_FLOAT,
            stride: 0,
            pointer: 0,
            buffer: 0,
        }
    }
}

impl ArrayPointer {
    /// Bytes one component occupies, or `None` for an unsupported type.
    fn component_bytes(&self) -> Option<u32> {
        Some(match self.ty {
            GL_BYTE | GL_UNSIGNED_BYTE => 1,
            GL_SHORT | GL_UNSIGNED_SHORT => 2,
            GL_FLOAT | GL_FIXED => 4,
            _ => return None,
        })
    }

    /// Distance in bytes between consecutive elements.
    fn element_stride(&self) -> Option<u32> {
        let packed = self.component_bytes()?.checked_mul(self.size)?;
        Some(if self.stride == 0 {
            packed
        } else {
            self.stride
        })
    }
}

/// Decode one component out of a raw little-endian buffer.
///
/// `normalize` maps integer types into `[0, 1]`, which is what GL does
/// for colours but *not* for positions or texture coordinates.
fn decode_component(bytes: &[u8], ty: u32, normalize: bool) -> f32 {
    match ty {
        GL_FLOAT => f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        GL_FIXED => fixed::to_f32(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
        GL_BYTE => {
            let v = bytes[0] as i8 as f32;
            if normalize {
                (v / 127.0).clamp(-1.0, 1.0)
            } else {
                v
            }
        }
        GL_UNSIGNED_BYTE => {
            let v = bytes[0] as f32;
            if normalize {
                v / 255.0
            } else {
                v
            }
        }
        GL_SHORT => {
            let v = i16::from_le_bytes([bytes[0], bytes[1]]) as f32;
            if normalize {
                (v / 32767.0).clamp(-1.0, 1.0)
            } else {
                v
            }
        }
        GL_UNSIGNED_SHORT => {
            let v = u16::from_le_bytes([bytes[0], bytes[1]]) as f32;
            if normalize {
                v / 65535.0
            } else {
                v
            }
        }
        _ => 0.0,
    }
}

/// How many texture stages keep state, which is deliberately more than
/// the `GL_MAX_TEXTURE_UNITS` we advertise.
///
/// `glActiveTexture` past the advertised maximum is an error that leaves
/// the selected unit alone, so an engine that walks more stages than it
/// asked about aims its per-unit calls at the last stage it *did*
/// select. Sticky Balls (X-Forge) does exactly that: it sets up stage 1,
/// then either binds a second layer to stage 2 or resets stage 2 before
/// the draw. With two stages tracked, that stage-2 traffic landed on
/// stage 1 — the reset unbound the texture that had just been bound, and
/// the game's sky came out as an opaque white sheet. Tracking the stages
/// the engine actually touches, plus one spare, keeps stage 2 harmless.
// Ordinary clients retain four selectable units. NV clients additionally
// need coordinate sets four/five; storage is shared, API limits are checked
// by client_texture_unit_capacity and the native shader resource limits.
const MAX_TEXTURE_UNITS: usize = raster::MAX_TEXTURE_STAGES;

/// A buffer object: the host-side copy of what `glBufferData` uploaded.
///
/// GL ES 1.1 buffer objects hold vertex attributes (`GL_ARRAY_BUFFER`)
/// or indices (`GL_ELEMENT_ARRAY_BUFFER`). The guest uploads once and
/// then draws from the buffer many times, so keeping the bytes on the
/// host side means a draw does not re-read guest memory at all.
#[derive(Debug, Clone)]
pub struct Buffer {
    pub data: Vec<u8>,
    /// The `GL_STATIC_DRAW` / `GL_DYNAMIC_DRAW` hint. Recorded for
    /// debugging; a software rasterizer has no use for it.
    pub usage: u32,
}

impl Default for Buffer {
    fn default() -> Self {
        Self {
            data: Vec::new(),
            usage: GL_STATIC_DRAW,
        }
    }
}

impl Buffer {
    /// Borrow `len` bytes at `offset`, or `None` if that range runs off
    /// the end. A short read must not be padded with zeroes — that
    /// silently renders degenerate geometry instead of surfacing the
    /// out-of-range access.
    pub fn slice(&self, offset: u32, len: usize) -> Option<&[u8]> {
        let start = offset as usize;
        let end = start.checked_add(len)?;
        self.data.get(start..end)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TextureUnitState {
    pub bound_texture: u32,
    pub texture_enabled: bool,
    pub texcoord_array: ArrayPointer,
    pub current_texcoord: [f32; 4],
    /// `glTexEnv(GL_TEXTURE_ENV_MODE)` is per-unit state in GL, so a
    /// game that configures two stages leaves two different modes
    /// behind. Keeping one global would mean the stage we actually
    /// sample is combined with whichever stage the guest happened to
    /// touch last.
    pub tex_env: TexEnvMode,
    /// `GL_TEXTURE_ENV_COLOR`, per-unit for the same reason as the mode
    /// above. Only `GL_BLEND` reads it.
    pub tex_env_color: [f32; 4],
    pub lod_bias: f32,
}

impl Default for TextureUnitState {
    fn default() -> Self {
        Self {
            bound_texture: 0,
            texture_enabled: false,
            texcoord_array: ArrayPointer::default(),
            current_texcoord: [0.0, 0.0, 0.0, 1.0],
            tex_env: TexEnvMode::Modulate,
            tex_env_color: [0.0; 4],
            lod_bias: 0.0,
        }
    }
}

/// The full GL ES 1.1 context.
/// Colour/depth and native renderer ownership for an EGL surface. GL state
/// and guest texture objects remain in Context when switching surfaces.
pub struct SurfaceBuffers {
    pub target: RenderTarget,
    gpu: Option<crate::gpu::Renderer>,
    failed: bool,
}
impl SurfaceBuffers {
    pub fn new_rgb565(width: u32, height: u32) -> Self {
        let mut surface = Self::new(width, height);
        surface.target = RenderTarget::with_format(width, height, true);
        surface
    }
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            target: RenderTarget::new(width, height),
            gpu: None,
            failed: false,
        }
    }
    pub fn release_gpu(&mut self) {
        self.gpu = None;
        self.failed = false;
    }
    pub fn write_rgb565(&mut self, pixels: &[u8]) -> bool {
        if !write_rgb565_target(&mut self.target, pixels) {
            return false;
        }
        if let Some(gpu) = self.gpu.as_mut() {
            if let Err(e) = gpu.write_rgb565(self.target.width, self.target.height, pixels) {
                log::warn!("GLES native pixmap upload: {e}");
                self.failed = true;
                return false;
            }
        }
        true
    }
    pub fn retain_shared_texture_cache(&mut self, live: &std::collections::HashSet<u32>) {
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.retain_shared_texture_cache(live);
        }
    }
    pub fn invalidate_texture_cache(&mut self) {
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.invalidate_texture_cache();
        }
    }
    pub fn delete_textures(&mut self, names: &[u32]) {
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.delete_textures(names);
        }
    }
    pub fn disable_gpu(&mut self) {
        if let Some(gpu) = self.gpu.as_mut() {
            if let Err(e) = gpu.read(&mut self.target) {
                log::warn!("GLES offscreen readback: {e}");
            }
        }
        self.release_gpu();
    }
}

fn write_rgb565_target(target: &mut RenderTarget, pixels: &[u8]) -> bool {
    if pixels.len() != target.width as usize * target.height as usize * 2 {
        return false;
    }
    for (dst, src) in target.color.chunks_exact_mut(4).zip(pixels.chunks_exact(2)) {
        let v = u16::from_le_bytes([src[0], src[1]]);
        let (r, g, b) = (
            ((v >> 11) & 31) as u8,
            ((v >> 5) & 63) as u8,
            (v & 31) as u8,
        );
        dst.copy_from_slice(&[
            (r << 3) | (r >> 2),
            (g << 2) | (g >> 4),
            (b << 3) | (b >> 2),
            255,
        ]);
    }
    true
}

pub struct Context {
    es11: es11::Es11State,
    gpu: Option<crate::gpu::Renderer>,
    gpu_requested: bool,
    internal_resolution: u8,
    gpu_failed: bool,
    // ---- transform state ----
    pub matrix_mode: MatrixMode,
    pub modelview: MatrixStack,
    pub projection: MatrixStack,
    /// GL ES keeps one texture matrix stack per server texture unit.
    pub texture_matrix: [MatrixStack; MAX_TEXTURE_UNITS],

    // ---- per-vertex defaults, used when the array is disabled ----
    pub current_color: [f32; 4],
    pub current_texcoord: [f32; 4],
    pub current_normal: [f32; 3],
    pub lighting: crate::lighting::LightingState,

    // ---- client arrays ----
    pub vertex_array: ArrayPointer,
    pub point_size_array: ArrayPointer,
    pub color_array: ArrayPointer,
    /// Texture coordinate arrays live on [`TextureUnitState`], one per
    /// unit — there is deliberately no unit-0 copy here, because a stale
    /// mirror of per-unit state is what made multitextured draws read
    /// the wrong stage.
    pub normal_array: ArrayPointer,
    /// Texture unit `glTexCoordPointer` / `glEnableClientState` affect.
    pub client_active_texture: u32,
    /// Texture unit `glTexEnv` / `glBindTexture` affect.
    pub active_texture: u32,

    // ---- textures ----
    pub textures: HashMap<u32, Texture>,
    software_mips: HashMap<u32, (u64, bool, SoftwareMipLevels)>,
    pub texture_units: [TextureUnitState; MAX_TEXTURE_UNITS],
    pub bound_texture: u32,
    next_texture_name: u32,

    // ---- buffer objects ----
    pub buffers: HashMap<u32, Buffer>,
    /// Currently bound `GL_ARRAY_BUFFER`; captured by `gl*Pointer`.
    pub array_buffer: u32,
    /// Currently bound `GL_ELEMENT_ARRAY_BUFFER`; consulted by
    /// `glDrawElements` at draw time, unlike the array binding.
    pub element_array_buffer: u32,
    next_buffer_name: u32,
    names: sharing::ObjectNames,
    external_references: ObjectReferences,

    // ---- fragment pipeline ----
    pub state: PipelineState,
    diagnostic_combiners: diagnostic::CombinerTrace,
    pub combiner_programs: crate::combiner_program::ProgramObjects,
    pub clear_color: [f32; 4],
    pub clear_depth: f32,
    /// Driver behavior observed in Gizmondo libgles_cm_20051013.dll, not
    /// a hardware claim or an ES-wide relaxation. Standard profile is default.
    pub gizmondo_goforce_20051013: bool,
    pub nv_combiner_client: bool,
    /// Session capability gate, independent of client detection and driver profile.
    pub nv_combiner_allowed: bool,
    /// `GL_CULL_FACE` enable, tracked separately from the face mode so
    /// `glCullFace` before `glEnable` is not lost.
    cull_enabled: bool,
    // The rectangle survives disabling GL_SCISSOR_TEST. PipelineState holds
    // only the effective clip shared by software and native rendering.
    scissor_enabled: bool,
    scissor_box: Option<(i32, i32, i32, i32)>,
    /// The face `glCullFace` selected; GL's initial value is `GL_BACK`.
    cull_mode: CullMode,

    // ---- framebuffer ----
    pub target: RenderTarget,

    /// Sticky error code, returned and reset by `glGetError`.
    error: u32,

    /// Scratch buffer for guest array reads, reused across draws so a
    /// 60 fps render loop does not allocate per call.
    prepared_arrays: Vec<(u32, Vec<u8>)>,
    array_read_pool: Vec<Vec<u8>>,
    vertex_stream: crate::gpu::VertexStream,
    flat_vertex_stream: crate::gpu::VertexStream,
    // Per-draw scratch only: never cache decoded guest memory between calls.
    assembled_vertices: Vec<Vertex>,
    vertex_remap: HashMap<u32, usize>,
    dense_indices: Vec<u16>,
    expanded_indices: Vec<usize>,
    decoded_indices: Vec<u32>,
    scratch: Vec<u8>,
    /// Scratch for index buffer reads.
    index_scratch: Vec<u8>,
}

impl Context {
    pub fn new_rgb565(width: u32, height: u32) -> Self {
        let mut context = Self::new(width, height);
        context.target = RenderTarget::with_format(width, height, true);
        context
    }

    pub fn new(width: u32, height: u32) -> Self {
        let mut state = PipelineState {
            viewport: (0, 0, width as i32, height as i32),
            ..Default::default()
        };
        // GL's initial depth func is LESS, but the test is off until
        // the app enables it.
        state.depth_func = CompareFunc::Less;
        Self {
            es11: Default::default(),
            gpu: None,
            gpu_requested: false,
            internal_resolution: 1,
            gpu_failed: false,
            matrix_mode: MatrixMode::Modelview,
            modelview: MatrixStack::new(16),
            projection: MatrixStack::new(16),
            texture_matrix: std::array::from_fn(|_| MatrixStack::new(16)),
            current_color: [1.0, 1.0, 1.0, 1.0],
            current_texcoord: [0.0, 0.0, 0.0, 1.0],
            current_normal: [0.0, 0.0, 1.0],
            lighting: Default::default(),
            vertex_array: ArrayPointer::default(),
            point_size_array: ArrayPointer {
                size: 1,
                ..Default::default()
            },
            color_array: ArrayPointer::default(),
            normal_array: ArrayPointer::default(),
            client_active_texture: 0,
            active_texture: 0,
            textures: HashMap::new(),
            software_mips: HashMap::new(),
            texture_units: [TextureUnitState::default(); MAX_TEXTURE_UNITS],
            bound_texture: 0,
            next_texture_name: 1,
            buffers: HashMap::new(),
            array_buffer: 0,
            element_array_buffer: 0,
            next_buffer_name: 1,
            names: Default::default(),
            external_references: Default::default(),
            state,
            diagnostic_combiners: Default::default(),
            combiner_programs: Default::default(),
            clear_color: [0.0, 0.0, 0.0, 0.0],
            clear_depth: 1.0,
            gizmondo_goforce_20051013: false,
            nv_combiner_client: false,
            nv_combiner_allowed: true,
            cull_enabled: false,
            scissor_enabled: false,
            scissor_box: None,
            cull_mode: CullMode::Back,
            target: RenderTarget::new(width, height),
            error: GL_NO_ERROR,
            prepared_arrays: Vec::new(),
            array_read_pool: Vec::new(),
            vertex_stream: Default::default(),
            flat_vertex_stream: Default::default(),
            assembled_vertices: Vec::new(),
            vertex_remap: HashMap::new(),
            dense_indices: Vec::new(),
            expanded_indices: Vec::new(),
            decoded_indices: Vec::new(),
            scratch: Vec::new(),
            index_scratch: Vec::new(),
        }
    }

    pub fn park_surface(&mut self, surface: &mut SurfaceBuffers) {
        std::mem::swap(&mut self.target, &mut surface.target);
        std::mem::swap(&mut self.gpu, &mut surface.gpu);
        std::mem::swap(&mut self.gpu_failed, &mut surface.failed);
    }

    /// Reset client state after the EGL context is destroyed, retaining the
    /// surface attachments and host rendering policy. Other EGL surfaces must
    /// also have their cached textures invalidated by the owner.
    pub fn write_rgb565(&mut self, pixels: &[u8]) -> bool {
        if !write_rgb565_target(&mut self.target, pixels) {
            return false;
        }
        if let Some(gpu) = self.gpu.as_mut() {
            if let Err(e) = gpu.write_rgb565(self.target.width, self.target.height, pixels) {
                self.gpu_failure("native pixmap upload", e);
                return false;
            }
        }
        true
    }
    pub fn reset_client_state(&mut self) {
        let names: Vec<_> = self.textures.keys().copied().collect();
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.delete_textures(&names);
        }
        let mut fresh = Self::new(self.target.width, self.target.height);
        fresh.gpu_requested = self.gpu_requested;
        fresh.internal_resolution = self.internal_resolution;
        fresh.gizmondo_goforce_20051013 = self.gizmondo_goforce_20051013;
        fresh.nv_combiner_client = self.nv_combiner_client;
        fresh.nv_combiner_allowed = self.nv_combiner_allowed;
        fresh.state.intel2700g_depth_bias = self.state.intel2700g_depth_bias;
        std::mem::swap(&mut fresh.target, &mut self.target);
        std::mem::swap(&mut fresh.gpu, &mut self.gpu);
        fresh.gpu_failed = self.gpu_failed;
        if let Some(gpu) = fresh.gpu.as_ref() {
            fresh.es11.point_max = gpu.logical_point_range()[1];
        }
        *self = fresh;
    }
    pub fn exchange_surface(&mut self, surface: &mut SurfaceBuffers) {
        self.park_surface(surface);
        if let Some(gpu) = &mut self.gpu {
            if let Err(e) = gpu.set_render_scale(self.internal_resolution) {
                self.gpu_failure("surface resolution", e);
            } else if !self.es11.point_max_changed {
                self.es11.point_max = gpu.logical_point_range()[1];
            }
        }
        if self.gpu_requested && self.gpu.is_none() {
            if let Err(e) = self.enable_gpu() {
                self.gpu_failure("surface", e);
            }
        }
    }

    /// Host-only GPU resolution policy: 1=native, 2=all 2x, 3=QVGA-only 2x, 4=QVGA-only 3x, 5=QVGA-only 4x.
    /// Guest surfaces, queries and reads stay logical.
    pub fn set_internal_resolution(&mut self, scale: u8) {
        self.internal_resolution = match scale {
            2 | 3 | 4 | 5 => scale,
            _ => 1,
        };
        if let Some(gpu) = self.gpu.as_mut() {
            if let Err(e) = gpu.set_render_scale(self.internal_resolution) {
                self.gpu_failure("internal resolution", e);
            } else if !self.es11.point_max_changed {
                self.es11.point_max = gpu.logical_point_range()[1];
            }
        }
    }

    pub fn scaled_rgb565_into(&self, out: &mut Vec<u8>) -> Option<(u32, u32)> {
        if self.gpu_failed {
            return None;
        }
        self.gpu.as_ref()?.scaled_rgb565_into(out)
    }

    /// Host presentation at source precision; guest display memory remains RGB565.
    pub fn scaled_display_rgba_into(&self, out: &mut Vec<u8>) -> Option<(u32, u32)> {
        if self.gpu_failed {
            return None;
        }
        if let Some(gpu) = self.gpu.as_ref() {
            return gpu.scaled_display_rgba_into(out);
        }
        if self.target.rgb565 {
            return None;
        }
        out.clear();
        out.extend_from_slice(&self.target.color);
        for pixel in out.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        Some((self.target.width, self.target.height))
    }

    /// Select before guest rendering begins. Initialization failure keeps software.
    pub fn enable_gpu(&mut self) -> Result<(), String> {
        self.gpu_requested = true;
        if self.target.width == 0 || self.target.height == 0 {
            return Ok(());
        }
        if self.gpu.is_none() {
            let mut gpu = crate::gpu::Renderer::new(self.target.width, self.target.height)?;
            gpu.set_surface_rgb565(self.target.rgb565)?;
            gpu.set_render_scale(self.internal_resolution)?;
            if !self.es11.point_max_changed {
                self.es11.point_max = gpu.logical_point_range()[1];
            }
            if self.target.rgb565
                && self
                    .target
                    .color
                    .chunks_exact(4)
                    .any(|p| p[..3] != [0, 0, 0])
            {
                let mut pixels =
                    vec![0; self.target.width as usize * self.target.height as usize * 2];
                for (dst, src) in pixels
                    .chunks_exact_mut(2)
                    .zip(self.target.color.chunks_exact(4))
                {
                    let v = ((src[0] as u16 >> 3) << 11)
                        | ((src[1] as u16 >> 2) << 5)
                        | (src[2] as u16 >> 3);
                    dst.copy_from_slice(&v.to_le_bytes());
                }
                gpu.write_rgb565(self.target.width, self.target.height, &pixels)?;
            }
            self.gpu = Some(gpu);
        }
        Ok(())
    }
    pub fn release_gpu(&mut self) {
        self.gpu = None;
        self.gpu_failed = false;
    }
    pub fn disable_gpu(&mut self) {
        self.sync_gpu();
        for tex in self.textures.values_mut() {
            tex.ensure_rgba();
        }
        self.release_gpu();
        self.gpu_requested = false;
    }
    fn gpu_failure(&mut self, action: &str, error: String) {
        if !self.gpu_failed {
            log::error!("GLES GPU {action}: {error}; restart with software rendering if needed");
        }
        self.gpu_failed = true;
        self.set_error(GL_OUT_OF_MEMORY);
    }
    pub fn gpu_requested(&self) -> bool {
        self.gpu_requested
    }
    fn client_texture_unit_capacity(&self) -> usize {
        if self.nv_combiner_client && self.nv_combiner_allowed {
            self.nv_texture_coordinate_capacity()
        } else {
            4
        }
    }

    pub fn nv_texture_coordinate_capacity(&self) -> usize {
        self.gpu
            .as_ref()
            .map_or(MAX_TEXTURE_UNITS, |g| g.texture_coordinate_capacity())
    }

    /// Querying identity must not create, bind or synchronize a host context.
    pub fn renderer_name(&self) -> &str {
        self.gpu
            .as_ref()
            .map_or("PocketHLE Software Rasterizer", |gpu| gpu.renderer_name())
    }

    pub fn gpu_active(&self) -> bool {
        self.gpu.is_some()
    }
    pub fn gpu_supports_combiner(&self) -> bool {
        !self.gpu_failed && self.gpu.as_ref().is_some_and(|gpu| gpu.supports_combiner())
    }

    /// Software rendering is synchronous; a GPU context must explicitly wait.
    /// Completion does not update the CPU framebuffer or present a surface.
    pub fn finish(&mut self) {
        if self.gpu_failed {
            return;
        }
        if let Some(gpu) = self.gpu.as_mut() {
            if let Err(e) = gpu.finish() {
                self.gpu_failure("finish", e);
            }
        }
    }

    /// Flush submission without imposing glFinish's completion barrier.
    pub fn flush(&mut self) {
        if self.gpu_failed {
            return;
        }
        if let Some(gpu) = self.gpu.as_mut() {
            if let Err(e) = gpu.flush() {
                self.gpu_failure("flush", e);
            }
        }
    }

    /// Read back only for presentation or glReadPixels, never per draw.
    pub fn sync_gpu(&mut self) {
        if self.gpu_failed {
            return;
        }
        if let Some(gpu) = self.gpu.as_mut() {
            if let Err(e) = gpu.read(&mut self.target) {
                self.gpu_failure("readback", e);
            }
        }
    }

    /// Preserve the RGBA readback cache and create the display framebuffer
    /// in the same native-readback traversal. Software uses the existing converter.
    pub fn present_rgb565(&mut self, out: &mut [u8]) {
        if !self.gpu_failed {
            if let Some(gpu) = self.gpu.as_mut() {
                match gpu.read_present(&mut self.target, out) {
                    Ok(()) => return,
                    Err(e) => self.gpu_failure("readback", e),
                }
            }
        }
        self.target.to_rgb565(out);
    }

    pub fn snapshot_shared_frame(&mut self) -> Option<crate::gpu::SharedFrame> {
        if self.gpu_failed {
            return None;
        }
        let gpu = self.gpu.as_mut()?;
        // Match read_present's geometry preparation even when no readback runs.
        if let Err(error) = gpu.resize(self.target.width, self.target.height) {
            self.gpu_failure("shared snapshot geometry", error);
            return None;
        }
        match self.gpu.as_mut()?.snapshot_configured_shared_frame() {
            Ok(frame) => frame,
            Err(error) => {
                self.gpu_failure("shared snapshot batch", error);
                None
            }
        }
    }

    /// Window presentation does not preserve ancillary depth storage. Do not
    /// use this for pbuffers, pixmaps, reads or a retried/failed presentation.
    /// Guest clear values, masks, scissor and all other GL state stay unchanged.
    pub fn discard_window_depth(&mut self) {
        if let Some(gpu) = self.gpu.as_mut() {
            if let Err(error) =
                gpu.discard_window_depth(self.target.width, self.target.height, &self.state)
            {
                self.gpu_failure("window depth discard", error);
                return;
            }
        }
        self.target.clear_depth(1., None);
    }

    // ---- error handling -------------------------------------------------

    /// Record an error. GL keeps the *first* error until it is read, so
    /// a later failure does not mask an earlier one.
    pub fn set_error(&mut self, code: u32) {
        if self.error == GL_NO_ERROR {
            // `glGetError` reports the *first* error since the last
            // check, so by the time a guest sees a code the offending
            // call is long gone. Log at the point of blame instead.
            log::debug!("GLES set error 0x{code:04x}");
            self.error = code;
        }
    }

    /// `glGetError`: return and clear the sticky error code.
    pub fn take_error(&mut self) -> u32 {
        if !self.gpu_failed {
            if let Some(gpu) = self.gpu.as_ref() {
                if let Err(error) = gpu.poll_error() {
                    self.gpu_failure("get_error", error);
                }
            }
        }
        std::mem::replace(&mut self.error, GL_NO_ERROR)
    }

    // ---- matrix stacks --------------------------------------------------

    fn stack_mut(&mut self) -> &mut MatrixStack {
        match self.matrix_mode {
            MatrixMode::Modelview => &mut self.modelview,
            MatrixMode::Projection => &mut self.projection,
            MatrixMode::Texture => &mut self.texture_matrix[self.active_texture as usize],
        }
    }

    pub fn set_matrix_mode(&mut self, mode: u32) {
        match MatrixMode::from_enum(mode) {
            Some(m) => self.matrix_mode = m,
            None => self.set_error(GL_INVALID_ENUM),
        }
    }

    pub fn load_identity(&mut self) {
        self.stack_mut().load_identity();
    }

    pub fn load_matrix(&mut self, m: Matrix4) {
        self.stack_mut().load(m);
    }

    pub fn mult_matrix(&mut self, m: Matrix4) {
        self.stack_mut().multiply_by(&m);
    }

    pub fn push_matrix(&mut self) {
        if !self.stack_mut().push() {
            self.set_error(GL_STACK_OVERFLOW);
        }
    }

    pub fn pop_matrix(&mut self) {
        if !self.stack_mut().pop() {
            self.set_error(GL_STACK_UNDERFLOW);
        }
    }

    pub fn translate(&mut self, x: f32, y: f32, z: f32) {
        let m = matrix::translate(x, y, z);
        self.stack_mut().multiply_by(&m);
    }

    pub fn scale(&mut self, x: f32, y: f32, z: f32) {
        let m = matrix::scale(x, y, z);
        self.stack_mut().multiply_by(&m);
    }

    pub fn rotate(&mut self, angle: f32, x: f32, y: f32, z: f32) {
        let m = matrix::rotate(angle, x, y, z);
        self.stack_mut().multiply_by(&m);
    }

    pub fn frustum(&mut self, l: f32, r: f32, b: f32, t: f32, n: f32, f: f32) {
        // GL rejects a degenerate frustum rather than producing NaNs.
        if n <= 0.0 || f <= 0.0 || l == r || b == t || n == f {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        let m = matrix::frustum(l, r, b, t, n, f);
        self.stack_mut().multiply_by(&m);
    }

    pub fn ortho(&mut self, l: f32, r: f32, b: f32, t: f32, n: f32, f: f32) {
        if l == r || b == t || n == f {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        let m = matrix::ortho(l, r, b, t, n, f);
        self.stack_mut().multiply_by(&m);
    }

    // ---- enables --------------------------------------------------------

    pub fn set_capability(&mut self, cap: u32, on: bool) {
        match cap {
            GL_DEPTH_TEST => self.state.depth_test = on,
            GL_POINT_SPRITE_OES => self.state.point_sprite = on,
            GL_ALPHA_TEST => self.state.alpha_test = on,
            GL_BLEND => self.state.blend = on,
            GL_COLOR_LOGIC_OP => self.state.logic_op = on,
            GL_CULL_FACE => {
                self.cull_enabled = on;
                self.sync_cull();
            }
            GL_SCISSOR_TEST => {
                self.scissor_enabled = on;
                self.state.scissor = if on { self.scissor_box } else { None };
            }
            GL_TEXTURE_2D => {
                let unit = self.active_texture as usize;
                if unit >= self.client_texture_unit_capacity() {
                    self.set_error(GL_INVALID_ENUM);
                    return;
                }
                self.texture_units[unit].texture_enabled = on;
                // No mirror into `self.state`: the rasterizer's single
                // texturing switch is derived per draw from the stage
                // `texturing_unit` picks, which is not always stage 0.
            }
            GL_FOG => self.state.fog = on,
            GL_LIGHTING => self.lighting.enabled = on,
            GL_NORMALIZE => self.lighting.normalize = on,
            GL_COLOR_MATERIAL => self.lighting.color_material = on,
            crate::lighting::RESCALE_NORMAL => self.lighting.rescale = on,
            GL_POLYGON_OFFSET_FILL => self.state.polygon_offset = on,
            0x0b10 | 0x0b20 | 0x809d | 0x809e | 0x809f | 0x80a0 => {
                if on {
                    self.es11.extra_enables.insert(cap);
                } else {
                    self.es11.extra_enables.remove(&cap);
                }
            }
            GL_DITHER => self.es11.dither = on,
            GL_STENCIL_TEST => self.es11.stencil = on,
            0x3000..=0x3005 => self.es11.clip_enabled[(cap - 0x3000) as usize] = on,
            0x4000..=0x4007 => self.lighting.lights[(cap - 0x4000) as usize].enabled = on,
            _ => self.set_error(GL_INVALID_ENUM),
        }
    }

    pub fn set_client_state(&mut self, array: u32, on: bool) {
        match array {
            GL_VERTEX_ARRAY => self.vertex_array.enabled = on,
            GL_COLOR_ARRAY => self.color_array.enabled = on,
            GL_TEXTURE_COORD_ARRAY => {
                let unit = self.client_active_texture as usize;
                if unit >= self.client_texture_unit_capacity() {
                    self.set_error(GL_INVALID_ENUM);
                    return;
                }
                self.texture_units[unit].texcoord_array.enabled = on;
            }
            GL_NORMAL_ARRAY => self.normal_array.enabled = on,
            GL_POINT_SIZE_ARRAY_OES => {
                self.point_size_array.enabled = on;
                self.state.point_size_array = on;
            }
            _ => self.set_error(GL_INVALID_ENUM),
        }
    }

    // ---- textures -------------------------------------------------------

    /// `glGenTextures`: allocate `n` unused names.
    pub fn gen_textures(&mut self, n: usize) -> Vec<u32> {
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            while self.next_texture_name == 0
                || self.names.textures.contains_key(&self.next_texture_name)
            {
                self.next_texture_name = self.next_texture_name.wrapping_add(1);
            }
            let name = self.next_texture_name;
            self.next_texture_name = self.next_texture_name.wrapping_add(1);
            let id = self.ensure_texture_name(name);
            self.textures.insert(id, Texture::default());
            out.push(name);
        }
        out
    }

    pub fn bind_texture(&mut self, target: u32, name: u32) {
        if target != GL_TEXTURE_2D {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        let unit = self.active_texture as usize;
        if unit >= self.client_texture_unit_capacity() {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        // Bindings retain the object, independently of namespace deletion.
        self.next_texture_name = self.next_texture_name.max(name.saturating_add(1));
        let name = self.ensure_texture_name(name);
        if name != 0 {
            self.es11.texture_objects.insert(name);
            self.textures.entry(name).or_default();
        }
        self.texture_units[unit].bound_texture = name;
        if unit == 0 {
            self.bound_texture = name;
        }
        log::debug!(
            "GLES bind texture target=0x{target:04x} name={name} active_unit={} objects={}",
            self.active_texture,
            self.textures.len()
        );
    }

    /// `glGenBuffers`. Names are handed out densely from 1; GL only
    /// requires that they be unused, not that they be contiguous.
    pub fn gen_buffers(&mut self, n: usize) -> Vec<u32> {
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            while self.next_buffer_name == 0
                || self.names.buffers.contains_key(&self.next_buffer_name)
            {
                self.next_buffer_name = self.next_buffer_name.wrapping_add(1);
            }
            let name = self.next_buffer_name;
            self.next_buffer_name = self.next_buffer_name.wrapping_add(1);
            let id = self.ensure_buffer_name(name);
            self.buffers.insert(id, Buffer::default());
            out.push(name);
        }
        out
    }

    /// `glBindBuffer`. Binding an unseen name creates the object, and
    /// binding 0 means "no buffer" — client arrays go back to reading
    /// guest memory directly.
    pub fn bind_buffer(&mut self, target: u32, name: u32) {
        if !matches!(target, GL_ARRAY_BUFFER | GL_ELEMENT_ARRAY_BUFFER) {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        self.next_buffer_name = self.next_buffer_name.max(name.saturating_add(1));
        let name = self.ensure_buffer_name(name);
        if name != 0 {
            self.es11.buffer_objects.insert(name);
            self.buffers.entry(name).or_default();
        }
        match target {
            GL_ARRAY_BUFFER => self.array_buffer = name,
            GL_ELEMENT_ARRAY_BUFFER => self.element_array_buffer = name,
            _ => {
                self.set_error(GL_INVALID_ENUM);
                return;
            }
        }
        log::debug!(
            "GLES bind buffer target=0x{target:04x} name={name} objects={}",
            self.buffers.len()
        );
    }

    /// `glDeleteBuffers`. Deleting the bound buffer reverts the binding
    /// to 0, as GL requires.
    pub fn delete_buffers(&mut self, names: &[u32]) {
        for &name in names {
            if name == 0 {
                continue;
            }
            let Some(name) = self.names.buffers.remove(&name) else {
                continue;
            };
            for array in [
                &mut self.vertex_array,
                &mut self.normal_array,
                &mut self.color_array,
                &mut self.point_size_array,
            ] {
                if array.buffer == name {
                    array.buffer = 0;
                }
            }
            for unit in &mut self.texture_units {
                if unit.texcoord_array.buffer == name {
                    unit.texcoord_array.buffer = 0;
                }
            }
            if self.array_buffer == name {
                self.array_buffer = 0;
            }
            if self.element_array_buffer == name {
                self.element_array_buffer = 0;
            }
            self.release_unlinked_buffer(name);
        }
    }

    /// The buffer currently bound to `target`, or `None` for 0.
    fn bound_buffer_name(&self, target: u32) -> Option<u32> {
        let name = match target {
            GL_ARRAY_BUFFER => self.array_buffer,
            GL_ELEMENT_ARRAY_BUFFER => self.element_array_buffer,
            _ => return None,
        };
        (name != 0).then_some(name)
    }

    /// `glBufferData`: (re)allocate the store and fill it with `data`.
    /// A null `data` pointer allocates without initialising, which is
    /// legal and common — the guest follows up with `glBufferSubData`.
    pub fn buffer_data(&mut self, target: u32, size: usize, data: Option<&[u8]>, usage: u32) {
        if !self.validate_buffer_data(target, size, usage) {
            return;
        }
        if data.is_some_and(|src| src.len() < size) {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        let name = self.bound_buffer_name(target).unwrap();
        let buf = self.buffers.get_mut(&name).unwrap();
        if buf
            .data
            .try_reserve_exact(size.saturating_sub(buf.data.len()))
            .is_err()
        {
            self.set_error(GL_OUT_OF_MEMORY);
            return;
        }
        buf.data.clear();
        buf.data.resize(size, 0);
        if let Some(src) = data {
            buf.data.copy_from_slice(&src[..size]);
        }
        buf.usage = usage;
        log::debug!(
            "GLES buffer data target=0x{target:04x} name={name} size={size} usage=0x{usage:04x} initialised={}",
            data.is_some()
        );
    }

    /// `glBufferSubData`: overwrite part of an existing store. A range
    /// that runs past the end is an error and writes nothing.
    pub fn buffer_sub_data(&mut self, target: u32, offset: u32, data: &[u8]) {
        if !self.validate_buffer_sub_data(target, offset, data.len()) {
            return;
        }
        let name = self.bound_buffer_name(target).unwrap();
        let buf = self.buffers.get_mut(&name).unwrap();
        let start = offset as usize;
        buf.data[start..start + data.len()].copy_from_slice(data);
    }

    /// Validate before the bridge reads guest data or mutates the store.
    pub fn validate_buffer_data(&mut self, target: u32, size: usize, usage: u32) -> bool {
        if !matches!(target, GL_ARRAY_BUFFER | GL_ELEMENT_ARRAY_BUFFER)
            || !matches!(usage, GL_STATIC_DRAW | GL_DYNAMIC_DRAW)
        {
            self.set_error(GL_INVALID_ENUM);
            return false;
        }
        if size > i32::MAX as usize {
            self.set_error(GL_INVALID_VALUE);
            return false;
        }
        if self.bound_buffer_name(target).is_none() {
            self.set_error(GL_INVALID_OPERATION);
            return false;
        }
        true
    }

    pub fn buffer_sub_data_read(&mut self, target: u32, offset: u32, size: usize) -> Option<&[u8]> {
        if !self.validate_buffer_sub_data(target, offset, size) {
            return None;
        }
        let name = self.bound_buffer_name(target).unwrap();
        let start = offset as usize;
        Some(&self.buffers[&name].data[start..start + size])
    }

    pub fn validate_buffer_sub_data(&mut self, target: u32, offset: u32, size: usize) -> bool {
        if !matches!(target, GL_ARRAY_BUFFER | GL_ELEMENT_ARRAY_BUFFER) {
            self.set_error(GL_INVALID_ENUM);
            return false;
        }
        if (offset as i32) < 0 || size > i32::MAX as usize {
            self.set_error(GL_INVALID_VALUE);
            return false;
        }
        let Some(name) = self.bound_buffer_name(target) else {
            self.set_error(GL_INVALID_OPERATION);
            return false;
        };
        let valid = self.buffers.get(&name).is_some_and(|buf| {
            (offset as usize)
                .checked_add(size)
                .is_some_and(|end| end <= buf.data.len())
        });
        if !valid {
            self.set_error(GL_INVALID_VALUE);
        }
        valid
    }

    pub fn set_active_texture(&mut self, unit: u32) {
        if unit as usize >= self.client_texture_unit_capacity() {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        self.active_texture = unit;
    }

    pub fn set_client_active_texture(&mut self, unit: u32) {
        if unit as usize >= self.client_texture_unit_capacity() {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        self.client_active_texture = unit;
    }

    /// OES_point_size_array captures the ARRAY_BUFFER binding at specification time.
    pub fn set_point_size_pointer(&mut self, ty: u32, stride: u32, pointer: u32) {
        if !matches!(ty, GL_FLOAT | GL_FIXED) {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        if (stride as i32) < 0 {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        self.point_size_array = ArrayPointer {
            enabled: self.point_size_array.enabled,
            size: 1,
            ty,
            stride,
            pointer,
            buffer: self.array_buffer,
        };
    }

    /// `glVertexPointer` preserves the enable flag owned by `glEnableClientState`.
    pub fn set_vertex_pointer(&mut self, size: u32, ty: u32, stride: u32, pointer: u32) {
        if !(2..=4).contains(&size) || (stride as i32) < 0 {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        if !matches!(ty, GL_BYTE | GL_SHORT | GL_FIXED | GL_FLOAT) {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        self.vertex_array = ArrayPointer {
            enabled: self.vertex_array.enabled,
            size,
            ty,
            stride,
            pointer,
            buffer: self.array_buffer,
        };
    }

    /// `glColorPointer`.
    pub fn set_color_pointer(&mut self, size: u32, ty: u32, stride: u32, pointer: u32) {
        if !(size == 4 || (self.gizmondo_goforce_20051013 && size == 3)) || (stride as i32) < 0 {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        if !matches!(ty, GL_UNSIGNED_BYTE | GL_FIXED | GL_FLOAT) {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        self.color_array = ArrayPointer {
            enabled: self.color_array.enabled,
            size,
            ty,
            stride,
            pointer,
            buffer: self.array_buffer,
        };
    }

    /// `glNormalPointer`. Normals are always three components.
    pub fn set_normal_pointer(&mut self, ty: u32, stride: u32, pointer: u32) {
        if (stride as i32) < 0 {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        if !matches!(ty, GL_BYTE | GL_SHORT | GL_FIXED | GL_FLOAT) {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        self.normal_array = ArrayPointer {
            enabled: self.normal_array.enabled,
            size: 3,
            ty,
            stride,
            pointer,
            buffer: self.array_buffer,
        };
    }

    pub fn set_texcoord_pointer(&mut self, size: u32, ty: u32, stride: u32, pointer: u32) {
        if !((2..=4).contains(&size) || (self.gizmondo_goforce_20051013 && size == 1))
            || (stride as i32) < 0
        {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        if !matches!(ty, GL_BYTE | GL_SHORT | GL_FIXED | GL_FLOAT) {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        let unit = self.client_active_texture as usize;
        if unit >= self.client_texture_unit_capacity() {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        let enabled = self.texture_units[unit].texcoord_array.enabled;
        self.texture_units[unit].texcoord_array = ArrayPointer {
            enabled,
            size,
            ty,
            stride,
            pointer,
            buffer: self.array_buffer,
        };
    }

    pub fn set_multi_texcoord(&mut self, unit: u32, s: f32, t: f32) {
        let Some(unit) = unit.checked_sub(GL_TEXTURE0).map(|u| u as usize) else {
            self.set_error(GL_INVALID_ENUM);
            return;
        };
        if unit >= self.client_texture_unit_capacity() {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        self.es11.current_texcoords[unit] = [s, t, 0., 1.];
        self.texture_units[unit].current_texcoord = [s, t, 0., 1.];
        if unit == 0 {
            self.current_texcoord = [s, t, 0., 1.];
        }
    }

    pub fn set_multi_texcoord4(&mut self, unit: u32, values: [f32; 4]) {
        self.set_multi_texcoord(unit, values[0], values[1]);
        if let Some(index) = unit
            .checked_sub(GL_TEXTURE0)
            .filter(|i| (*i as usize) < self.client_texture_unit_capacity())
        {
            self.es11.current_texcoords[index as usize] = values;
            self.texture_units[index as usize].current_texcoord = values;
            if index == 0 {
                self.current_texcoord = values;
            }
        }
    }

    pub fn delete_textures(&mut self, names: &[u32]) {
        if let Some(gpu) = self.gpu.as_mut() {
            let ids: Vec<_> = names
                .iter()
                .filter_map(|n| self.names.textures.get(n).copied())
                .collect();
            gpu.delete_textures(&ids);
        }
        for &name in names {
            if name == 0 {
                continue;
            }
            let Some(name) = self.names.textures.remove(&name) else {
                continue;
            };
            for (unit, state) in self.texture_units.iter_mut().enumerate() {
                if state.bound_texture == name {
                    state.bound_texture = 0;
                    if unit == 0 {
                        self.bound_texture = 0;
                    }
                }
            }
            self.release_unlinked_texture(name);
        }
    }

    /// `glTexImage2D`. `data` is the already-copied guest buffer; an
    /// empty slice means a null pointer, which allocates storage
    /// without initialising it.
    // The parameter list mirrors the GL entry point one-for-one.
    // Bundling them into a struct would only move the unpacking to
    // every call site in the dispatch layer.
    #[allow(clippy::too_many_arguments)]
    /// Copy framebuffer pixels into the canonical texture image. GL's source
    /// origin is bottom-left, while the framebuffer cache is stored top-down.
    /// ES 1.1 section 4.1.7. The initial operation is COPY.
    pub fn logic_op(&mut self, op: u32) {
        if (0x1500..=0x150f).contains(&op) {
            self.state.logic_mode = op;
        } else {
            self.set_error(GL_INVALID_ENUM);
        }
    }

    /// ES 1.1 section 3.7.2. Source coordinates use GL's lower-left origin.
    #[allow(clippy::too_many_arguments)]
    pub fn copy_tex_sub_image_2d(
        &mut self,
        target: u32,
        level: i32,
        xo: i32,
        yo: i32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) {
        if target != GL_TEXTURE_2D {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        if !(0..=10).contains(&level) || xo < 0 || yo < 0 || width < 0 || height < 0 {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        let name = self.texture_units[self.active_texture as usize].bound_texture;
        let Some(tex) = self.textures.get(&name) else {
            self.set_error(GL_INVALID_OPERATION);
            return;
        };
        let Some(&format) = tex.formats.get(&(level as u32)) else {
            self.set_error(GL_INVALID_OPERATION);
            return;
        };
        let (tw, th) = if level == 0 {
            (tex.width, tex.height)
        } else if let Some((w, h, _)) = tex.mipmaps.get(&(level as u32)) {
            (*w, *h)
        } else {
            self.set_error(GL_INVALID_OPERATION);
            return;
        };
        if i64::from(xo) + i64::from(width) > i64::from(tw)
            || i64::from(yo) + i64::from(height) > i64::from(th)
        {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        // Compressed formats announced here do not provide framebuffer compression.
        if !matches!(
            format,
            GL_ALPHA | GL_RGB | GL_RGBA | GL_LUMINANCE | GL_LUMINANCE_ALPHA
        ) {
            self.set_error(GL_INVALID_OPERATION);
            return;
        }
        if self.target.rgb565 && matches!(format, GL_ALPHA | GL_LUMINANCE_ALPHA | GL_RGBA) {
            self.set_error(GL_INVALID_OPERATION);
            return;
        }
        if width == 0 || height == 0 {
            return;
        }
        if level == 0
            && format == GL_RGB
            && !self.gpu_failed
            && !self.textures[&name].generate_mipmap
        {
            if let Some(gpu) = self.gpu.as_mut() {
                let copied = (|| {
                    let Some(source) =
                        gpu.copy_guest_rgb_texture(x, y, width as u32, height as u32)?
                    else {
                        return Ok(None);
                    };
                    gpu.patch_guest_rgb_texture(&self.textures[&name], &source, xo, yo)
                })();
                match copied {
                    Ok(Some(frame)) => {
                        let texture = self.textures.get_mut(&name).unwrap();
                        texture.gpu_copy = Some(frame);
                        texture.rgba.clear();
                        texture.compressed.remove(&0);
                        texture.revision = texture.revision.wrapping_add(1);
                        return;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        self.gpu_failure("copy texture subimage", error);
                        return;
                    }
                }
            }
        }
        self.sync_gpu();
        if self.gpu_failed {
            return;
        }
        let mut patch = Vec::with_capacity(width as usize * height as usize * 4);
        for row in 0..height {
            for col in 0..width {
                let (sx, sy) = (i64::from(x) + i64::from(col), i64::from(y) + i64::from(row));
                let mut p = [0; 4]; // Out-of-framebuffer source values are undefined by GL.
                if sx >= 0
                    && sy >= 0
                    && sx < i64::from(self.target.width)
                    && sy < i64::from(self.target.height)
                {
                    let i = ((i64::from(self.target.height) - 1 - sy)
                        * i64::from(self.target.width)
                        + sx) as usize
                        * 4;
                    p.copy_from_slice(&self.target.color[i..i + 4]);
                }
                patch.extend(match format {
                    GL_ALPHA => [255, 255, 255, p[3]],
                    GL_RGB => [p[0], p[1], p[2], 255],
                    GL_LUMINANCE => [p[0], p[0], p[0], 255],
                    GL_LUMINANCE_ALPHA => [p[0], p[0], p[0], p[3]],
                    _ => p,
                });
            }
        }
        self.apply_texture_patch(
            level,
            xo as u32,
            yo as u32,
            width as u32,
            height as u32,
            &patch,
        );
    }

    /// ES 1.1 plus IMG PVRTC, EXT/ANGLE DXT and AMD ATC restrictions.
    /// Validate the complete update before reading the guest payload.
    #[allow(clippy::too_many_arguments)]
    pub fn validate_compressed_tex_sub_image_2d(
        &mut self,
        target: u32,
        level: i32,
        xo: i32,
        yo: i32,
        width: i32,
        height: i32,
        format: u32,
    ) -> Option<usize> {
        if target != GL_TEXTURE_2D || texture::compressed_image_size(format, 4, 4).is_none() {
            self.set_error(GL_INVALID_ENUM);
            return None;
        }
        if !(0..=10).contains(&level) || xo < 0 || yo < 0 || width < 0 || height < 0 {
            self.set_error(GL_INVALID_VALUE);
            return None;
        }
        // Paletted subimages and all ATC subimages are explicitly forbidden.
        if (GL_PALETTE4_RGB8_OES..=GL_PALETTE8_RGB5_A1_OES).contains(&format)
            || matches!(
                format,
                GL_ATC_RGB_AMD
                    | GL_ATC_RGBA_EXPLICIT_ALPHA_AMD
                    | GL_ATC_RGBA_INTERPOLATED_ALPHA_AMD
            )
        {
            self.set_error(GL_INVALID_OPERATION);
            return None;
        }
        let name = self.texture_units[self.active_texture as usize].bound_texture;
        let Some(old) = self
            .textures
            .get(&name)
            .and_then(|t| t.compressed.get(&(level as u32)))
        else {
            self.set_error(GL_INVALID_OPERATION);
            return None;
        };
        if old.format != format {
            self.set_error(GL_INVALID_OPERATION);
            return None;
        }
        let (xo, yo, w, h) = (xo as u32, yo as u32, width as u32, height as u32);
        if u64::from(xo) + u64::from(w) > u64::from(old.width)
            || u64::from(yo) + u64::from(h) > u64::from(old.height)
        {
            self.set_error(GL_INVALID_VALUE);
            return None;
        }
        let pvrtc = (GL_COMPRESSED_RGB_PVRTC_4BPPV1_IMG..=GL_COMPRESSED_RGBA_PVRTC_2BPPV1_IMG)
            .contains(&format);
        if pvrtc && (xo != 0 || yo != 0 || w != old.width || h != old.height)
            || !pvrtc
                && (xo % 4 != 0
                    || yo % 4 != 0
                    || w % 4 != 0 && xo + w != old.width
                    || h % 4 != 0 && yo + h != old.height)
        {
            self.set_error(GL_INVALID_OPERATION);
            return None;
        }
        if w == 0 || h == 0 {
            return Some(0);
        }
        texture::compressed_image_size(format, w, h)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn compressed_tex_sub_image_2d(
        &mut self,
        target: u32,
        level: i32,
        xo: i32,
        yo: i32,
        width: i32,
        height: i32,
        format: u32,
        data: &[u8],
    ) {
        let Some(needed) =
            self.validate_compressed_tex_sub_image_2d(target, level, xo, yo, width, height, format)
        else {
            return;
        };
        if data.len() != needed {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        if width == 0 || height == 0 {
            return;
        }
        let name = self.texture_units[self.active_texture as usize].bound_texture;
        let old = self.textures[&name].compressed[&(level as u32)].clone();
        let (xo, yo, w, h) = (xo as u32, yo as u32, width as u32, height as u32);
        let pvrtc = (GL_COMPRESSED_RGB_PVRTC_4BPPV1_IMG..=GL_COMPRESSED_RGBA_PVRTC_2BPPV1_IMG)
            .contains(&format);
        let mut packed = old.bytes.to_vec();
        if pvrtc {
            packed.copy_from_slice(data);
        } else {
            let block = if matches!(
                format,
                GL_COMPRESSED_RGB_S3TC_DXT1_EXT | GL_COMPRESSED_RGBA_S3TC_DXT1_EXT
            ) {
                8
            } else {
                16
            };
            let stride = old.width.div_ceil(4) as usize * block;
            let count = w.div_ceil(4) as usize * block;
            for row in 0..h.div_ceil(4) as usize {
                let dst = (yo as usize / 4 + row) * stride + xo as usize / 4 * block;
                packed[dst..dst + count].copy_from_slice(&data[row * count..(row + 1) * count]);
            }
        }
        let Some(rgba) = texture::decode_compressed_to_rgba(&packed, old.width, old.height, format)
        else {
            self.set_error(GL_INVALID_VALUE);
            return;
        };
        self.textures.get_mut(&name).unwrap().set_compressed(
            level as u32,
            old.width,
            old.height,
            format,
            &packed,
            rgba,
        );
    }

    pub fn copy_tex_image_2d(
        &mut self,
        target: u32,
        level: i32,
        format: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        border: i32,
    ) {
        if target == GL_TEXTURE_2D && texture::is_s3tc(format) {
            self.set_error(
                if self.gizmondo_goforce_20051013 && format > GL_COMPRESSED_RGBA_S3TC_DXT1_EXT {
                    GL_INVALID_VALUE
                } else {
                    GL_INVALID_OPERATION
                },
            );
            return;
        }
        if target != GL_TEXTURE_2D
            || !matches!(
                format,
                GL_ALPHA | GL_RGB | GL_RGBA | GL_LUMINANCE | GL_LUMINANCE_ALPHA
            )
        {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        if level < 0 || width < 0 || height < 0 || width > 1024 || height > 1024 || border != 0 {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        if level > 10 || width > (1024 >> level) || height > (1024 >> level) {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        if (width != 0 && !(width as u32).is_power_of_two())
            || (height != 0 && !(height as u32).is_power_of_two())
        {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        if self.target.rgb565 && matches!(format, GL_ALPHA | GL_LUMINANCE_ALPHA | GL_RGBA) {
            self.set_error(GL_INVALID_OPERATION);
            return;
        }
        if self.gpu_failed {
            return;
        }
        let name = self.texture_units[self.active_texture as usize].bound_texture;
        if level == 0
            && format == GL_RGB
            && width > 0
            && height > 0
            && self
                .textures
                .get(&name)
                .is_none_or(|tex| !tex.generate_mipmap)
        {
            if let Some(gpu) = self.gpu.as_mut() {
                match gpu.copy_guest_rgb_texture(x, y, width as u32, height as u32) {
                    Ok(Some(frame)) => {
                        let tex = self.textures.entry(name).or_default();
                        tex.set_level(0, width as u32, height as u32, Vec::new());
                        tex.set_format(0, GL_RGB);
                        tex.gpu_copy = Some(frame);
                        return;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        self.gpu_failure("copy texture", error);
                        return;
                    }
                }
            }
        }
        self.sync_gpu();
        if self.gpu_failed {
            return;
        }
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for row in 0..height {
            for col in 0..width {
                let sx = i64::from(x) + i64::from(col);
                let sy = i64::from(y) + i64::from(row);
                let mut pixel = [0; 4];
                if sx >= 0
                    && sy >= 0
                    && sx < i64::from(self.target.width)
                    && sy < i64::from(self.target.height)
                {
                    let offset = ((i64::from(self.target.height) - 1 - sy)
                        * i64::from(self.target.width)
                        + sx) as usize
                        * 4;
                    pixel.copy_from_slice(&self.target.color[offset..offset + 4]);
                }
                pixel = match format {
                    GL_ALPHA => [255, 255, 255, pixel[3]],
                    GL_RGB => [pixel[0], pixel[1], pixel[2], 255],
                    GL_LUMINANCE => [pixel[0], pixel[0], pixel[0], 255],
                    GL_LUMINANCE_ALPHA => [pixel[0], pixel[0], pixel[0], pixel[3]],
                    _ => pixel,
                };
                rgba.extend(pixel);
            }
        }
        self.tex_image_2d(
            target,
            level,
            width as u32,
            height as u32,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &rgba,
        );
        let name = self.texture_units[self.active_texture as usize].bound_texture;
        if let Some(tex) = self.textures.get_mut(&name) {
            tex.set_format(level as u32, format);
        }
    }

    /// NVIDIA's Gizmondo driver accepts BGR/BGRA source uploads. Keep the
    /// standard ES format validation unchanged outside that device profile.
    fn upload_format(&self, format: u32) -> u32 {
        if self.gizmondo_goforce_20051013 {
            match format {
                0x80E0 => GL_RGB,
                0x80E1 => GL_RGBA,
                _ => format,
            }
        } else {
            format
        }
    }

    pub fn upload_pixel_layout(&self, format: u32, ty: u32) -> Result<usize, u32> {
        texture::pixel_layout(self.upload_format(format), ty)
    }

    fn decode_upload(
        &self,
        data: &[u8],
        width: u32,
        height: u32,
        format: u32,
        ty: u32,
    ) -> Option<Vec<u8>> {
        let canonical = self.upload_format(format);
        let mut rgba = texture::decode_to_rgba(data, width, height, canonical, ty)?;
        if canonical != format {
            for pixel in rgba.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }
        Some(rgba)
    }

    /// Validate before the bridge touches guest pixel memory, including NULL uploads.
    #[allow(clippy::too_many_arguments)]
    pub fn validate_tex_image_2d(
        &mut self,
        target: u32,
        level: i32,
        internalformat: u32,
        width: u32,
        height: u32,
        border: i32,
        format: u32,
        ty: u32,
    ) -> bool {
        let error = if target != GL_TEXTURE_2D {
            Some(GL_INVALID_ENUM)
        } else if texture::is_s3tc(internalformat) {
            Some(
                if self.gizmondo_goforce_20051013
                    && internalformat > GL_COMPRESSED_RGBA_S3TC_DXT1_EXT
                {
                    GL_INVALID_VALUE
                } else {
                    GL_INVALID_OPERATION
                },
            )
        } else if !matches!(
            internalformat,
            GL_ALPHA | GL_LUMINANCE | GL_LUMINANCE_ALPHA | GL_RGB | GL_RGBA
        ) && !(self.gizmondo_goforce_20051013 && (1..=4).contains(&internalformat))
        {
            // The dated NVIDIA driver also accepts legacy component counts.
            // Source format/type still determine decoding and retained alpha.
            Some(GL_INVALID_VALUE)
        } else if !(0..=10).contains(&level)
            || border != 0
            || width > (1024u32 >> level)
            || height > (1024u32 >> level)
            || (width != 0 && !width.is_power_of_two())
            || (height != 0 && !height.is_power_of_two())
        {
            Some(GL_INVALID_VALUE)
        } else if let Err(error) = self.upload_pixel_layout(format, ty) {
            Some(error)
        } else if internalformat != format && !self.gizmondo_goforce_20051013 {
            Some(GL_INVALID_OPERATION)
        } else {
            None
        };
        if let Some(error) = error {
            self.set_error(error);
            false
        } else {
            true
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn validate_tex_sub_image_2d(
        &mut self,
        target: u32,
        level: i32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        format: u32,
        ty: u32,
    ) -> bool {
        if target != GL_TEXTURE_2D {
            self.set_error(GL_INVALID_ENUM);
            return false;
        }
        if !(0..=10).contains(&level)
            || width > 1024
            || height > 1024
            || x > i32::MAX as u32
            || y > i32::MAX as u32
        {
            self.set_error(GL_INVALID_VALUE);
            return false;
        }
        if let Err(error) = self.upload_pixel_layout(format, ty) {
            self.set_error(error);
            return false;
        }
        let unit = self.active_texture as usize;
        let image = self
            .texture_units
            .get(unit)
            .and_then(|u| self.textures.get(&u.bound_texture));
        let Some(tex) = image else {
            self.set_error(GL_INVALID_OPERATION);
            return false;
        };
        let dims = if level == 0 && tex.formats.contains_key(&0) {
            Some((tex.width, tex.height))
        } else {
            tex.mipmaps.get(&(level as u32)).map(|m| (m.0, m.1))
        };
        let Some((tw, th)) = dims else {
            self.set_error(GL_INVALID_OPERATION);
            return false;
        };
        if x.checked_add(width).is_none_or(|n| n > tw)
            || y.checked_add(height).is_none_or(|n| n > th)
        {
            self.set_error(GL_INVALID_VALUE);
            return false;
        }
        let internal = tex.formats.get(&(level as u32)).copied().unwrap_or(GL_RGBA);
        if texture::compressed_image_size(internal, 4, 4).is_some() {
            self.set_error(GL_INVALID_OPERATION);
            return false;
        }
        // Compressed-image update restrictions belong to their format-specific path.
        if matches!(
            internal,
            GL_ALPHA | GL_LUMINANCE | GL_LUMINANCE_ALPHA | GL_RGB | GL_RGBA
        ) && internal != self.upload_format(format)
        {
            self.set_error(GL_INVALID_OPERATION);
            return false;
        }
        true
    }

    pub fn tex_image_2d(
        &mut self,
        target: u32,
        level: i32,
        width: u32,
        height: u32,
        format: u32,
        ty: u32,
        data: &[u8],
    ) {
        if !self.validate_tex_image_2d(
            target,
            level,
            self.upload_format(format),
            width,
            height,
            0,
            format,
            ty,
        ) {
            return;
        }
        let unit = self.active_texture as usize;
        if unit >= self.client_texture_unit_capacity() {
            self.set_error(GL_INVALID_OPERATION);
            return;
        }
        let rgba = if data.is_empty() {
            vec![0u8; (width as usize) * (height as usize) * 4]
        } else {
            match self.decode_upload(data, width, height, format, ty) {
                Some(v) => v,
                None => {
                    self.set_error(GL_INVALID_OPERATION);
                    return;
                }
            }
        };
        let canonical_format = self.upload_format(format);
        let name = self.texture_units[self.active_texture as usize].bound_texture;
        let tex = self.textures.entry(name).or_default();
        tex.set_level(level as u32, width, height, rgba);
        tex.set_format(level as u32, canonical_format);
        log::debug!(
            "GLES tex image texture={} level={} {}x{} format=0x{format:04x} type=0x{ty:04x} bytes={} complete={} first={:02x?}",
            name,
            level,
            width,
            height,
            data.len(),
            tex.is_complete(),
            &tex.rgba[..tex.rgba.len().min(4)],
        );
    }

    /// Validate the complete compressed image before the bridge reads guest memory.
    #[allow(clippy::too_many_arguments)]
    pub fn validate_compressed_tex_image_2d(
        &mut self,
        target: u32,
        level: i32,
        width: u32,
        height: u32,
        format: u32,
        border: i32,
    ) -> Option<usize> {
        if target != GL_TEXTURE_2D || texture::compressed_image_size(format, 1, 1).is_none() {
            self.set_error(GL_INVALID_ENUM);
            return None;
        }
        let paletted = texture::is_paletted(format);
        // NVIDIA returns INVALID_VALUE for a DXT border; retain that measured
        // legacy result while standard EXT/ANGLE compression uses INVALID_OPERATION.
        if texture::is_s3tc(format)
            && border != 0
            && !(self.gizmondo_goforce_20051013 && format <= GL_COMPRESSED_RGBA_S3TC_DXT1_EXT)
        {
            self.set_error(GL_INVALID_OPERATION);
            return None;
        }
        if paletted && border != 0 {
            self.set_error(GL_INVALID_OPERATION);
            return None;
        }
        if !(-10..=10).contains(&level)
            || (paletted && level > 0)
            || (!paletted && level < 0)
            || border != 0
            || width > (1024u32 >> level.max(0))
            || height > (1024u32 >> level.max(0))
            || (width != 0 && !width.is_power_of_two())
            || (height != 0 && !height.is_power_of_two())
            || (level < 0 && level.unsigned_abs() > width.max(height).max(1).ilog2())
        {
            self.set_error(GL_INVALID_VALUE);
            return None;
        }
        let size = texture::compressed_upload_size(format, level, width, height);
        if size.is_none() {
            self.set_error(GL_INVALID_VALUE);
        }
        size
    }

    /// Complete block-compressed image or a shared-palette mipmap chain.
    pub fn compressed_tex_image_2d(
        &mut self,
        target: u32,
        level: i32,
        width: u32,
        height: u32,
        format: u32,
        data: &[u8],
    ) {
        let Some(needed) =
            self.validate_compressed_tex_image_2d(target, level, width, height, format, 0)
        else {
            return;
        };
        // Empty is the direct-call representation of a NULL client pointer.
        // The ABI bridge independently checks imageSize, including for NULL.
        if !data.is_empty() && data.len() != needed {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        let unit = self.active_texture as usize;
        if unit >= self.client_texture_unit_capacity() {
            self.set_error(GL_INVALID_OPERATION);
            return;
        }
        let name = self.texture_units[unit].bound_texture;
        if let Some(palette_len) = texture::palette_bytes(format) {
            let mut images = Vec::new();
            let mut offset = palette_len;
            for l in 0..=level.unsigned_abs() {
                let (w, h) = if width == 0 || height == 0 {
                    (width, height)
                } else {
                    ((width >> l).max(1), (height >> l).max(1))
                };
                let rgba = if data.is_empty() || w == 0 || h == 0 {
                    vec![0; (w * h * 4) as usize]
                } else {
                    let size = texture::compressed_image_size(format, w, h).unwrap() - palette_len;
                    let mut packed = data[..palette_len].to_vec();
                    packed.extend_from_slice(&data[offset..offset + size]);
                    offset += size;
                    let Some(rgba) = texture::decode_compressed_to_rgba(&packed, w, h, format)
                    else {
                        self.set_error(GL_INVALID_VALUE);
                        return;
                    };
                    rgba
                };
                images.push((l, w, h, rgba));
            }
            // Decode every image before committing any. Compressed uploads do
            // not trigger GENERATE_MIPMAP, even when decoded for host storage.
            let tex = self.textures.entry(name).or_default();
            let generate = tex.generate_mipmap;
            tex.generate_mipmap = false;
            for (l, w, h, rgba) in images {
                tex.set_level(l, w, h, rgba);
                tex.formats.insert(l, format);
            }
            tex.generate_mipmap = generate;
        } else {
            let defer = !data.is_empty()
                && self
                    .gpu
                    .as_ref()
                    .is_some_and(|g| g.supports_compressed(format));
            let rgba = if defer {
                Vec::new()
            } else if data.is_empty() || needed == 0 {
                vec![0; (width * height * 4) as usize]
            } else {
                let Some(rgba) = texture::decode_compressed_to_rgba(data, width, height, format)
                else {
                    self.set_error(GL_INVALID_VALUE);
                    return;
                };
                rgba
            };
            let bytes = if data.is_empty() {
                vec![0; needed]
            } else {
                data.to_vec()
            };
            self.textures.entry(name).or_default().set_compressed(
                level as u32,
                width,
                height,
                format,
                &bytes,
                rgba,
            );
        }
    }

    /// `glTexSubImage2D`: patch a rectangle of the bound texture.
    // The parameter list mirrors the GL entry point one-for-one.
    #[allow(clippy::too_many_arguments)]
    pub fn tex_sub_image_2d(
        &mut self,
        target: u32,
        level: i32,
        xoffset: u32,
        yoffset: u32,
        width: u32,
        height: u32,
        format: u32,
        ty: u32,
        data: &[u8],
    ) {
        if !self
            .validate_tex_sub_image_2d(target, level, xoffset, yoffset, width, height, format, ty)
        {
            return;
        }
        if width == 0 || height == 0 || data.is_empty() {
            return;
        }
        let Some(patch) = self.decode_upload(data, width, height, format, ty) else {
            self.set_error(GL_INVALID_OPERATION);
            return;
        };
        self.apply_texture_patch(level, xoffset, yoffset, width, height, &patch);
    }

    /// Canonical RGBA patch from a validated upload or framebuffer copy.
    fn apply_texture_patch(
        &mut self,
        level: i32,
        xoffset: u32,
        yoffset: u32,
        width: u32,
        height: u32,
        patch: &[u8],
    ) {
        let unit = self.active_texture as usize;
        if unit >= self.client_texture_unit_capacity() {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        let name = self.texture_units[unit].bound_texture;
        let Some(tex) = self.textures.get_mut(&name) else {
            self.set_error(GL_INVALID_OPERATION);
            return;
        };
        tex.ensure_rgba();
        if level == 0 && tex.gpu_copy.is_some() {
            self.set_error(GL_INVALID_OPERATION);
            return;
        }
        let (tw, th, pixels) = if level == 0 {
            (tex.width, tex.height, &mut tex.rgba)
        } else if let Some((w, h, p)) = tex.mipmaps.get_mut(&(level as u32)) {
            (*w, *h, p)
        } else {
            self.set_error(GL_INVALID_OPERATION);
            return;
        };
        if xoffset.checked_add(width).is_none_or(|v| v > tw)
            || yoffset.checked_add(height).is_none_or(|v| v > th)
        {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        tex.compressed.remove(&(level as u32));
        tex.revision = tex.revision.wrapping_add(1);
        for row in 0..height {
            let src = (row * width * 4) as usize;
            let dst = (((yoffset + row) * tw + xoffset) * 4) as usize;
            let len = (width * 4) as usize;
            pixels[dst..dst + len].copy_from_slice(&patch[src..src + len]);
        }
        if level as u32 == tex.base_level {
            tex.regenerate_mipmaps();
        }
    }

    pub fn tex_parameter(&mut self, target: u32, pname: u32, value: u32) {
        if target != GL_TEXTURE_2D {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        if matches!(
            pname,
            GL_TEXTURE_MAX_LEVEL_SGIS | GL_TEXTURE_BASE_LEVEL_SGIS
        ) && (value as i32) < 0
        {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        let unit = self.active_texture as usize;
        if unit >= self.client_texture_unit_capacity() {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        let name = self.texture_units[unit].bound_texture;
        let tex = self.textures.entry(name).or_default();
        tex.goforce_lod = self.gizmondo_goforce_20051013;
        if self.gizmondo_goforce_20051013
            && matches!(pname, GL_TEXTURE_WRAP_S | GL_TEXTURE_WRAP_T)
            && value == 0x2900
        {
            if pname == GL_TEXTURE_WRAP_S {
                tex.wrap_s = texture::Wrap::GizmondoClamp;
            } else {
                tex.wrap_t = texture::Wrap::GizmondoClamp;
            }
            return;
        }
        if !tex.set_parameter(pname, value) {
            self.set_error(GL_INVALID_ENUM);
        }
    }

    /// Float-valued SGIS LOD parameters are not enum bit patterns.
    pub fn tex_parameter_float(&mut self, target: u32, pname: u32, value: f32) {
        if target != GL_TEXTURE_2D {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        if !value.is_finite() {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        if matches!(pname, GL_TEXTURE_MIN_LOD_SGIS | GL_TEXTURE_MAX_LOD_SGIS) {
            let name = self.texture_units[self.active_texture as usize].bound_texture;
            let tex = self.textures.entry(name).or_default();
            tex.goforce_lod = self.gizmondo_goforce_20051013;
            if pname == GL_TEXTURE_MIN_LOD_SGIS {
                tex.min_lod = value;
            } else {
                tex.max_lod = value;
            }
        } else {
            if matches!(
                pname,
                GL_TEXTURE_BASE_LEVEL_SGIS | GL_TEXTURE_MAX_LEVEL_SGIS
            ) && (value < 0. || value > i32::MAX as f32)
            {
                self.set_error(GL_INVALID_VALUE);
                return;
            }
            self.tex_parameter(
                target,
                pname,
                if pname == 0x8191 {
                    u32::from(value != 0.)
                } else {
                    value.round() as u32
                },
            );
        }
    }

    pub fn set_lod_bias(&mut self, value: f32) {
        if !value.is_finite() {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        self.texture_units[self.active_texture as usize].lod_bias = value.clamp(-4., 4.);
    }

    pub fn tex_env(&mut self, pname: u32, value: u32) {
        if pname == GL_COORD_REPLACE_OES {
            if value > 1 {
                self.set_error(GL_INVALID_VALUE);
                return;
            }
            self.state.point_coord_replace[self.active_texture as usize] = value != 0;
            return;
        }
        if pname != GL_TEXTURE_ENV_MODE {
            let unit = self.active_texture as usize;
            match self.es11.combiners[unit].set(pname, value) {
                Ok(()) => {
                    if matches!(self.texture_units[unit].tex_env, TexEnvMode::Combine(_)) {
                        self.texture_units[unit].tex_env =
                            TexEnvMode::Combine(self.es11.combiners[unit]);
                    }
                }
                Err(error) => self.set_error(error),
            }
            return;
        }
        let mode = match value {
            GL_MODULATE => TexEnvMode::Modulate,
            GL_REPLACE => TexEnvMode::Replace,
            GL_DECAL => TexEnvMode::Decal,
            GL_ADD => TexEnvMode::Add,
            GL_COMBINE => TexEnvMode::Combine(self.es11.combiners[self.active_texture as usize]),
            // GL_BLEND shares its value with the blend enable enum.
            0x0BE2 => TexEnvMode::Blend,
            _ => {
                self.set_error(GL_INVALID_ENUM);
                return;
            }
        };
        let unit = self.active_texture as usize;
        if unit >= self.client_texture_unit_capacity() {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        self.texture_units[unit].tex_env = mode;
    }

    /// `glTexEnv(GL_TEXTURE_ENV_COLOR)`, which like the mode belongs to
    /// the active unit.
    pub fn set_tex_env_color(&mut self, color: [f32; 4]) {
        let unit = self.active_texture as usize;
        if unit >= self.client_texture_unit_capacity() {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        self.texture_units[unit].tex_env_color = color.map(|v| v.clamp(0., 1.));
    }

    // ---- fragment state -------------------------------------------------

    pub fn set_point_size(&mut self, size: f32) {
        if !size.is_finite() || size <= 0. {
            self.set_error(GL_INVALID_VALUE);
        } else {
            self.state.point_size = size;
        }
    }
    pub fn set_line_width(&mut self, width: f32) {
        if !width.is_finite() || width <= 0. {
            self.set_error(GL_INVALID_VALUE);
        } else {
            self.state.line_width = width;
        }
    }
    /// Software implementation limit, not a hardware RAM/display claim.
    pub fn max_viewport_dims(&self) -> [i32; 2] {
        self.gpu.as_ref().map_or([16384; 2], |g| g.max_viewport)
    }
    pub fn subpixel_bits(&self) -> i32 {
        self.gpu.as_ref().map_or(4, |g| g.subpixel_bits)
    }
    pub fn primitive_size_range(&self, point: bool) -> [f32; 2] {
        self.gpu.as_ref().map_or([1., 64.], |g| {
            if point {
                g.logical_point_range()
            } else {
                g.line_range
            }
        })
    }

    pub fn set_depth_func(&mut self, func: u32) {
        match CompareFunc::from_enum(func) {
            Some(f) => self.state.depth_func = f,
            None => self.set_error(GL_INVALID_ENUM),
        }
    }

    pub fn set_alpha_func(&mut self, func: u32, reference: f32) {
        match CompareFunc::from_enum(func) {
            Some(f) => {
                self.state.alpha_func = f;
                self.state.alpha_ref = reference.clamp(0.0, 1.0);
            }
            None => self.set_error(GL_INVALID_ENUM),
        }
    }

    pub fn set_blend_func(&mut self, src: u32, dst: u32) {
        match (BlendFactor::from_enum(src), BlendFactor::from_enum(dst)) {
            (Some(s), Some(d)) => {
                self.state.blend_src = s;
                self.state.blend_dst = d;
            }
            _ => self.set_error(GL_INVALID_ENUM),
        }
    }

    pub fn set_cull_face(&mut self, mode: u32) {
        let cull = match mode {
            GL_FRONT => CullMode::Front,
            GL_BACK => CullMode::Back,
            GL_FRONT_AND_BACK => CullMode::FrontAndBack,
            _ => {
                self.set_error(GL_INVALID_ENUM);
                return;
            }
        };
        self.cull_mode = cull;
        self.sync_cull();
    }

    /// `GL_CULL_FACE` and `glCullFace` are independent pieces of state;
    /// the rasterizer only sees the combination, so recompute it
    /// whenever either changes.
    fn sync_cull(&mut self) {
        self.state.cull = if self.cull_enabled {
            Some(self.cull_mode)
        } else {
            None
        };
    }

    pub fn set_front_face(&mut self, mode: u32) {
        match mode {
            GL_CW => self.state.front_face = FrontFace::Cw,
            GL_CCW => self.state.front_face = FrontFace::Ccw,
            _ => self.set_error(GL_INVALID_ENUM),
        }
    }

    pub fn set_shade_model(&mut self, mode: u32) {
        match mode {
            GL_FLAT => self.state.smooth_shading = false,
            GL_SMOOTH => self.state.smooth_shading = true,
            _ => self.set_error(GL_INVALID_ENUM),
        }
    }

    pub fn set_fog(&mut self, pname: u32, value: f32) {
        log::debug!("GLES fog scalar pname=0x{pname:04x} value={value:.6}",);
        match pname {
            GL_FOG_MODE => {
                // GL_LINEAR and GL_EXP share numeric space with other
                // enums, so compare against the fog values directly.
                self.state.fog_mode = match value as u32 {
                    GL_EXP => FogMode::Exp,
                    GL_EXP2 => FogMode::Exp2,
                    GL_LINEAR => FogMode::Linear,
                    _ => {
                        self.set_error(GL_INVALID_ENUM);
                        return;
                    }
                };
            }
            GL_FOG_DENSITY => self.state.fog_density = value,
            GL_FOG_START => self.state.fog_start = value,
            GL_FOG_END => self.state.fog_end = value,
            _ => self.set_error(GL_INVALID_ENUM),
        }
        log::debug!(
            "GLES fog state mode={:?} density={:.6} start={:.6} end={:.6} color={:?} enabled={}",
            self.state.fog_mode,
            self.state.fog_density,
            self.state.fog_start,
            self.state.fog_end,
            self.state.fog_color,
            self.state.fog,
        );
    }

    pub fn set_viewport(&mut self, x: i32, y: i32, w: i32, h: i32) {
        if w < 0 || h < 0 {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        let limit = self.max_viewport_dims();
        self.state.viewport = (x, y, w.min(limit[0]), h.min(limit[1]));
    }

    pub fn set_scissor(&mut self, x: i32, y: i32, w: i32, h: i32) {
        if w < 0 || h < 0 {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        // SkyStriker sets a 138x99 aircraft-preview box, but never enables
        // GL_SCISSOR_TEST. glScissor changes the box, not its enable state;
        // clipping immediately confined every later gameplay clear/draw.
        self.scissor_box = Some((x, y, w, h));
        if self.scissor_enabled {
            self.state.scissor = self.scissor_box;
        }
    }

    pub fn set_depth_range(&mut self, near: f32, far: f32) {
        self.state.depth_range = (near.clamp(0.0, 1.0), far.clamp(0.0, 1.0));
    }

    // ---- clearing -------------------------------------------------------

    pub fn clear(&mut self, mask: u32) {
        let mask = if self.target.depth.is_empty() {
            mask & !GL_DEPTH_BUFFER_BIT
        } else {
            mask
        };
        // ES 1.1 section 4.2.3: reject the whole command before changing buffers.
        if mask & !(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT | GL_STENCIL_BUFFER_BIT) != 0 {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        // This NVIDIA window/depth path emits the same depth command for a
        // stencil clear as for a depth clear, independent of stencil state.
        let mask = if self.gizmondo_goforce_20051013 && mask & GL_STENCIL_BUFFER_BIT != 0 {
            mask | GL_DEPTH_BUFFER_BIT
        } else {
            mask
        };
        // Standard ES has no stencil attachment in this configuration.
        if mask & (GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT) == 0 {
            return;
        }
        if self.gpu_failed {
            return;
        }
        // The dated GoForce driver submits a full depth clear even when
        // its guest depth-write state is false. Other drivers keep ES semantics.
        let driver_depth_write = self.state.depth_write || self.gizmondo_goforce_20051013;
        let mut clear_state;
        let state = if driver_depth_write != self.state.depth_write {
            clear_state = self.state.clone();
            clear_state.depth_write = true;
            &clear_state
        } else {
            &self.state
        };
        if let Some(gpu) = self.gpu.as_mut() {
            if let Err(e) = gpu.clear(
                self.target.width,
                self.target.height,
                state,
                self.clear_color,
                self.clear_depth,
                mask,
            ) {
                self.gpu_failure("clear", e);
            }
            return;
        }
        // Ordering against the draws is the whole point: a frame that
        // clears *after* drawing looks exactly like a frame that never
        // drew, and only an interleaved log tells the two apart.
        log::trace!(
            "GLES clear mask=0x{mask:04x} color={:?} depth={}",
            self.clear_color,
            self.clear_depth
        );
        if mask & GL_COLOR_BUFFER_BIT != 0 {
            let c = [
                (self.clear_color[0].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
                (self.clear_color[1].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
                (self.clear_color[2].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
                (self.clear_color[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
            ];
            self.target
                .clear_color_masked(c, self.state.scissor, self.state.color_mask);
        }
        if mask & GL_DEPTH_BUFFER_BIT != 0 && driver_depth_write {
            let d = self.clear_depth;
            self.target.clear_depth(d, self.state.scissor);
        }
    }

    // ---- drawing --------------------------------------------------------

    /// The combined projection × modelview transform applied to every
    /// incoming position.
    fn mvp(&self) -> Matrix4 {
        matrix::multiply(self.projection.current(), self.modelview.current())
    }

    /// Fetch element `index` of `array`, writing up to 4 components.
    /// Missing components take their GL defaults: `(0, 0, 0, 1)`.
    fn fetch(
        &mut self,
        mem: &mut dyn GuestMemory,
        array: ArrayPointer,
        index: u32,
        normalize: bool,
    ) -> Option<[f32; 4]> {
        let comp = array.component_bytes()?;
        let stride = array.element_stride()?;
        let addr = array.pointer.checked_add(stride.checked_mul(index)?)?;
        let len = (comp * array.size) as usize;

        if let Some((base, bytes)) = self.prepared_arrays.iter().find(|(base, bytes)| {
            array.buffer == 0
                && addr.checked_sub(*base).is_some_and(|off| {
                    (off as usize)
                        .checked_add(len)
                        .is_some_and(|end| end <= bytes.len())
                })
        }) {
            let offset = addr.checked_sub(*base)? as usize;
            let bytes = bytes.get(offset..offset.checked_add(len)?)?;
            let mut out = [0., 0., 0., 1.];
            for (c, slot) in out.iter_mut().enumerate().take(array.size as usize) {
                *slot = decode_component(&bytes[c * comp as usize..], array.ty, normalize);
            }
            return Some(out);
        }

        // A pointer captured while a VBO was bound is an offset into
        // that buffer's host-side copy, not a guest address. Reading it
        // as a guest address would dereference a small integer and, on
        // an unlucky mapping, silently render garbage.
        if array.buffer != 0 {
            let buf = self.buffers.get(&array.buffer)?;
            let bytes = buf.slice(addr, len)?;
            let mut out = [0.0, 0.0, 0.0, 1.0];
            for (c, slot) in out.iter_mut().enumerate().take(array.size as usize) {
                let off = c * comp as usize;
                *slot = decode_component(&bytes[off..], array.ty, normalize);
            }
            return Some(out);
        }

        let mut buf = std::mem::take(&mut self.scratch);
        let ok = mem.read(addr, len, &mut buf) && buf.len() >= len;
        let mut out = [0.0, 0.0, 0.0, 1.0];
        if ok {
            for (c, slot) in out.iter_mut().enumerate().take(array.size as usize) {
                let off = c * comp as usize;
                *slot = decode_component(&buf[off..], array.ty, normalize);
            }
        }
        self.scratch = buf;
        if ok {
            Some(out)
        } else {
            None
        }
    }

    fn texture_coordinate_units(&self, stages: &[raster::TextureStage]) -> u8 {
        self.combiner_programs.active().map_or_else(
            || {
                stages
                    .iter()
                    .fold(0, |mask, stage| mask | (1 << stage.unit))
            },
            |program| program.coordinate_units(),
        )
    }

    /// The texture stages this draw cascades through, in unit order.
    ///
    /// Unit 0 is *not* a safe assumption, and neither is a single stage:
    /// nothing in GL says a textured draw has to use the first unit, and
    /// engines built for multitexturing hardware routinely leave stage 0
    /// disabled and drive the diffuse map through a later stage.
    /// X-Forge does exactly that — Sticky Balls sets up every draw as
    ///
    /// ```text
    /// glActiveTexture(GL_TEXTURE0); glDisable(GL_TEXTURE_2D); glBindTexture(2D, 0);
    /// glActiveTexture(GL_TEXTURE1); glEnable(GL_TEXTURE_2D);  glBindTexture(2D, colour);
    /// glActiveTexture(GL_TEXTURE2); glEnable(GL_TEXTURE_2D);  glBindTexture(2D, alpha);
    /// ```
    ///
    /// and it needs *both* of those stages: the colour map is DXT1, a
    /// format with no alpha channel, so the shape's cut-out lives in a
    /// separate white-RGB mask on the stage above. Sampling either one
    /// alone is visibly wrong — the mask alone paints white over the
    /// scene, the colour map alone leaves every ball and cloud as an
    /// opaque square.
    ///
    /// A unit whose texture has never been given pixels is skipped
    /// rather than sampled: GL ES treats an incomplete texture as if
    /// texturing were off for that unit, and a stage bound to a
    /// just-generated name must not shadow a later stage that has real
    /// pixels.
    fn texture_stages(&self) -> ([raster::TextureStage; MAX_TEXTURE_UNITS], usize) {
        let mut stages = [raster::TextureStage {
            unit: 0,
            env: TexEnvMode::Modulate,
            env_color: [0.; 4],
            lod_bias: 0.,
        }; MAX_TEXTURE_UNITS];
        let mut count = 0;
        for u in 0..MAX_TEXTURE_UNITS {
            let unit = &self.texture_units[u];
            let enabled = self
                .combiner_programs
                .active()
                .map_or(unit.texture_enabled, |p| p.texture_units() & (1 << u) != 0);
            let coordinate_only = self.gpu.is_some()
                && self
                    .combiner_programs
                    .active()
                    .is_some_and(|p| p.coordinate_units() & (1 << u) != 0);
            if coordinate_only
                || enabled
                    && self
                        .textures
                        .get(&unit.bound_texture)
                        .is_some_and(Texture::is_complete)
            {
                stages[count] = raster::TextureStage {
                    unit: u,
                    env: unit.tex_env,
                    env_color: unit.tex_env_color,
                    lod_bias: unit.lod_bias,
                };
                count += 1;
            }
        }
        (stages, count)
    }

    /// Assemble one vertex from the enabled client arrays and the
    /// coordinate sets needed by this draw.
    fn assemble(
        &mut self,
        mem: &mut dyn GuestMemory,
        index: u32,
        mvp: &Matrix4,
        coordinate_units: u8,
        normal_matrix: &Matrix4,
    ) -> Option<Vertex> {
        let obj = self.fetch(mem, self.vertex_array, index, false)?;
        let clip = if self.gpu.is_some() && !self.es11.pretransform {
            obj
        } else {
            matrix::transform(mvp, obj)
        };

        let color = if self.color_array.enabled {
            // Colours from integer arrays are normalized to [0, 1];
            // this is the one place GL treats a UNSIGNED_BYTE array as
            // a fraction rather than a raw value.
            self.fetch(mem, self.color_array, index, true)
                .unwrap_or(self.current_color)
        } else {
            self.current_color
        };

        let mut texcoord = [[0., 0., 0., 1.]; raster::MAX_TEXTURE_STAGES];
        for coordinate_unit in 0..MAX_TEXTURE_UNITS {
            if coordinate_units & (1 << coordinate_unit) == 0 {
                continue;
            }
            let unit = self.texture_units[coordinate_unit];
            let input = if unit.texcoord_array.enabled {
                self.fetch(mem, unit.texcoord_array, index, false)
                    .unwrap_or([0., 0., 0., 1.])
            } else {
                unit.current_texcoord
            };
            texcoord[coordinate_unit] = if self.gpu.is_some() && !self.es11.pretransform {
                input
            } else {
                matrix::transform(self.texture_matrix[coordinate_unit].current(), input)
            };
        }

        // Eye-space Z drives fog. The modelview alone takes us to eye
        // space; GL fogs on |z_eye|.
        let eye = if self.gpu.is_some() && !self.es11.pretransform {
            [0.0; 4]
        } else {
            matrix::transform(self.modelview.current(), obj)
        };
        let normal = if self.lighting.enabled && self.normal_array.enabled {
            let n = self.fetch(mem, self.normal_array, index, true)?;
            [n[0], n[1], n[2]]
        } else {
            self.current_normal
        };
        let (color, back_color) =
            if (self.gpu.is_none() || self.es11.pretransform) && self.lighting.enabled {
                let n = self.lighting.transform_normal(normal_matrix, normal);
                let point = std::array::from_fn(|i| {
                    if eye[3] == 0. {
                        eye[i]
                    } else {
                        eye[i] / eye[3]
                    }
                });
                let front = self.lighting.shade(n, point, color);
                let back = if self.lighting.two_sided {
                    self.lighting.shade(n.map(|x| -x), point, color)
                } else {
                    front
                };
                (front, back)
            } else {
                (color, color)
            };
        let array_size = if self.es11.point_primitive && self.point_size_array.enabled {
            Some(self.fetch(mem, self.point_size_array, index, false)?[0])
        } else {
            None
        };
        Some(Vertex {
            eye,
            point_width: if self.es11.point_draw {
                if let Some(size) = array_size {
                    self.derived_point_size_from(eye, size)
                } else {
                    self.derived_point_size(eye)
                }
            } else {
                array_size.unwrap_or(-1.)
            },
            normal,
            back_color,
            pos: clip,
            color,
            texcoord,
            fog_depth: eye[2].abs(),
        })
    }

    /// Is there vertex data to draw from? A pointer of 0 is legitimate
    /// when it is an offset into a bound VBO, so the null check only
    /// applies to client-side arrays.
    fn vertex_source_ready(&self) -> bool {
        self.vertex_array.enabled
            && (self.vertex_array.buffer != 0 || self.vertex_array.pointer != 0)
    }

    /// `glDrawArrays`.
    pub fn draw_arrays(&mut self, mem: &mut dyn GuestMemory, mode: u32, first: u32, count: u32) {
        if !self.validate_draw(mode, count) {
            return;
        }
        if (first as i32) < 0 || first.checked_add(count).is_none() {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        if count == 0 || !self.vertex_source_ready() {
            return;
        }
        let end = first + count;
        self.draw_indexed(mem, mode, first..end, (end - first) as usize, true);
    }

    /// `glDrawElements`. `indices` has already been read out of guest
    /// memory and widened to `u32`.
    pub fn draw_elements(&mut self, mem: &mut dyn GuestMemory, mode: u32, indices: &[u32]) {
        if !self.validate_draw(mode, indices.len() as u32) || !self.vertex_source_ready() {
            return;
        }
        self.draw_indexed(mem, mode, indices.iter().copied(), indices.len(), false);
    }

    fn validate_draw(&mut self, mode: u32, count: u32) -> bool {
        if mode > GL_TRIANGLE_FAN {
            self.set_error(GL_INVALID_ENUM);
            return false;
        }
        if (count as i32) < 0 {
            self.set_error(GL_INVALID_VALUE);
            return false;
        }
        true
    }

    /// Read an index buffer out of guest memory and draw with it.
    pub fn draw_elements_from_guest(
        &mut self,
        mem: &mut dyn GuestMemory,
        mode: u32,
        count: u32,
        ty: u32,
        pointer: u32,
    ) {
        if !self.validate_draw(mode, count) {
            return;
        }
        let width = match ty {
            GL_UNSIGNED_BYTE => 1usize,
            GL_UNSIGNED_SHORT => 2usize,
            _ => {
                self.set_error(GL_INVALID_ENUM);
                return;
            }
        };
        if count == 0 || !self.vertex_source_ready() {
            return;
        }
        let Some(len) = (count as usize).checked_mul(width) else {
            self.set_error(GL_INVALID_VALUE);
            return;
        };

        let mut indices = std::mem::take(&mut self.decoded_indices);
        indices.clear();
        if self.element_array_buffer != 0 {
            if let Some(bytes) = self
                .buffers
                .get(&self.element_array_buffer)
                .and_then(|b| b.slice(pointer, len))
            {
                if width == 1 {
                    indices.extend(bytes.iter().map(|b| *b as u32));
                } else {
                    indices.extend(
                        bytes
                            .chunks_exact(2)
                            .map(|b| u16::from_le_bytes([b[0], b[1]]) as u32),
                    );
                }
            } else {
                self.set_error(GL_INVALID_VALUE);
            }
        } else {
            let mut bytes = std::mem::take(&mut self.index_scratch);
            if mem.read(pointer, len, &mut bytes) && bytes.len() >= len {
                if width == 1 {
                    indices.extend(bytes[..len].iter().map(|b| *b as u32));
                } else {
                    indices.extend(
                        bytes[..len]
                            .chunks_exact(2)
                            .map(|b| u16::from_le_bytes([b[0], b[1]]) as u32),
                    );
                }
            }
            self.index_scratch = bytes;
        }
        if !indices.is_empty() {
            self.draw_elements(mem, mode, &indices);
        }
        self.decoded_indices = indices;
    }

    fn recycle_prepared_arrays(&mut self) {
        for (_, mut bytes) in self.prepared_arrays.drain(..) {
            bytes.clear();
            self.array_read_pool.push(bytes);
        }
    }

    fn stream_array(
        &self,
        stream: &mut crate::gpu::VertexStream,
        slot: usize,
        array: ArrayPointer,
        lo: u32,
        count: usize,
        normalize: bool,
        components: u32,
        native_fixed: bool,
    ) -> Option<()> {
        let comp = array.component_bytes()?;
        let stride = array.element_stride()?;
        let start = array.pointer.checked_add(lo.checked_mul(stride)?)?;
        let len = (count.checked_sub(1)? as u32)
            .checked_mul(stride)?
            .checked_add(array.size.checked_mul(comp)?)? as usize;
        start.checked_add(len as u32)?;
        if len > 1024 * 1024 {
            return None;
        }
        let source = if array.buffer != 0 {
            self.buffers.get(&array.buffer)?.slice(start, len)?
        } else {
            let (base, bytes) = self.prepared_arrays.iter().find(|(base, bytes)| {
                start.checked_sub(*base).is_some_and(|off| {
                    (off as usize)
                        .checked_add(len)
                        .is_some_and(|end| end <= bytes.len())
                })
            })?;
            &bytes[(start - *base) as usize..(start - *base) as usize + len]
        };
        let size = array.size.min(components);
        if size == 0 || size > 4 || stride > i32::MAX as u32 {
            return None;
        }
        let offset = (stream.bytes.len() + 3) & !3;
        stream.bytes.resize(offset, 0);
        let convert = (array.ty == GL_FIXED && !native_fixed) || stride % comp != 0;
        let (ty, output_stride, normalized) = if convert {
            for i in 0..count {
                for c in 0..size as usize {
                    let at = i * stride as usize + c * comp as usize;
                    stream.bytes.extend_from_slice(
                        &decode_component(&source[at..], array.ty, normalize).to_le_bytes(),
                    );
                }
            }
            (GL_FLOAT, (size * 4) as i32, 0)
        } else {
            let packed = (size * comp) as usize;
            if stride as usize == packed {
                stream.bytes.extend_from_slice(&source[..count * packed]);
            } else {
                // Keep native formats, but omit interleaved padding/other attributes.
                for i in 0..count {
                    let start = i * stride as usize;
                    stream
                        .bytes
                        .extend_from_slice(&source[start..start + packed]);
                }
            }
            (
                array.ty,
                packed as i32,
                (normalize && !matches!(array.ty, GL_FLOAT | GL_FIXED)) as u8,
            )
        };
        stream.attributes[slot] = Some(crate::gpu::StreamAttribute {
            size: size as i32,
            ty,
            normalized,
            stride: output_stride,
            offset,
        });
        Some(())
    }

    fn prepare_vertex_stream(
        &self,
        stream: &mut crate::gpu::VertexStream,
        indices: impl Iterator<Item = u32> + Clone,
        count: usize,
        sequential: bool,
        stages: &[raster::TextureStage],
        point_sizes: bool,
    ) -> bool {
        stream.flat_triangles = false;
        stream.bytes.clear();
        stream.indices.clear();
        stream.attributes.fill(None);
        stream.constants = [[0., 0., 0., 1.]; 10];
        stream.constants[1] = self.current_color;
        stream.constants[6] = [
            self.current_normal[0],
            self.current_normal[1],
            self.current_normal[2],
            0.,
        ];
        let Some(lo) = indices.clone().min() else {
            return false;
        };
        let Some(hi) = indices.clone().max() else {
            return false;
        };
        let span = hi - lo;
        if span >= 65536 || span > (count as u32).saturating_mul(4) {
            return false;
        }
        stream.count = span as usize + 1;
        if !sequential {
            stream.coverage.resize(stream.count, 0);
            stream.coverage.fill(0);
            for i in indices {
                let relative = (i - lo) as u16;
                stream.indices.push(relative);
                stream.coverage[relative as usize] = 1;
            }
            // Hole-heavy draws keep the existing unique-vertex path rather than
            // uploading unused vertices and inflating native transfer size.
            if stream.coverage.contains(&0) {
                stream.indices.clear();
                return false;
            }
        }
        let native_fixed = self
            .gpu
            .as_ref()
            .is_some_and(|g| g.supports_fixed_vertices());
        let ready = (|| {
            self.stream_array(
                stream,
                0,
                self.vertex_array,
                lo,
                stream.count,
                false,
                4,
                native_fixed,
            )?;
            if self.color_array.enabled {
                self.stream_array(
                    stream,
                    1,
                    self.color_array,
                    lo,
                    stream.count,
                    true,
                    4,
                    native_fixed,
                )?;
            }
            for stage in stages {
                let unit = self.texture_units[stage.unit];
                if unit.texcoord_array.enabled {
                    self.stream_array(
                        stream,
                        crate::gpu::texture_attribute(stage.unit),
                        unit.texcoord_array,
                        lo,
                        stream.count,
                        false,
                        4,
                        native_fixed,
                    )?;
                } else {
                    stream.constants[crate::gpu::texture_attribute(stage.unit)] =
                        unit.current_texcoord;
                }
            }
            if self.lighting.enabled {
                if self.normal_array.enabled {
                    self.stream_array(
                        stream,
                        6,
                        self.normal_array,
                        lo,
                        stream.count,
                        true,
                        3,
                        native_fixed,
                    )?;
                }
                stream.attributes[7] = stream.attributes[0];
            }
            if point_sizes {
                self.stream_array(
                    stream,
                    7,
                    self.point_size_array,
                    lo,
                    stream.count,
                    false,
                    1,
                    native_fixed,
                )?;
            }
            Some(())
        })()
        .is_some();
        if !ready {
            stream.bytes.clear();
            stream.indices.clear();
        }
        ready
    }

    fn draw_indexed(
        &mut self,
        mem: &mut dyn GuestMemory,
        mut mode: u32,
        indices: impl Iterator<Item = u32> + Clone,
        count: usize,
        sequential: bool,
    ) {
        if self.gpu_failed {
            return;
        }
        if crate::graphics_diagnostic::enabled() {
            self.trace_combiner_draw(mode, count);
        }
        self.es11.point_primitive = mode == GL_POINTS;
        self.es11.point_draw = mode == GL_POINTS
            && ((self.point_size_array.enabled && self.gpu.is_none())
                || self.es11.attenuation != [1., 0., 0.]
                || self.es11.point_min != 0.
                || self.es11.point_max != self.primitive_size_range(true)[1]);
        self.es11.pretransform = self.es11.clip_enabled.iter().any(|v| *v) || self.es11.point_draw;
        if self.gpu.is_none() {
            for tex in self.textures.values_mut() {
                tex.ensure_rgba();
                if tex.gpu_copy.is_some() {
                    self.set_error(GL_INVALID_OPERATION);
                    return;
                }
            }
        }
        let mvp = if self.gpu.is_some() && !self.es11.pretransform {
            matrix::IDENTITY
        } else {
            self.mvp()
        };
        // The stages own the coordinate arrays to read, so they have to
        // be resolved before assembling vertices.
        let normal_matrix = if self.lighting.enabled {
            crate::lighting::normal_matrix(self.modelview.current())
        } else {
            matrix::IDENTITY
        };
        self.state.two_sided_lighting = self.lighting.enabled && self.lighting.two_sided;
        let combiner = self.combiner_programs.active_shared();
        let mut feedback_writes = self.state.color_mask;
        if self.target.rgb565 {
            feedback_writes[3] = false;
        }
        let framebuffer_fetch =
            combiner.is_some_and(|p| p.framebuffer_needs_primitive_order(feedback_writes));
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.set_combiner(combiner, self.combiner_programs.constants());
        }
        let (stage_storage, stage_count) = self.texture_stages();
        let stages = &stage_storage[..stage_count];
        let coordinate_units = self.texture_coordinate_units(stages);
        self.recycle_prepared_arrays();
        if let (Some(lo), Some(hi)) = (indices.clone().min(), indices.clone().max()) {
            let mut arrays = [ArrayPointer::default(); MAX_TEXTURE_UNITS + 4];
            arrays[0] = self.vertex_array;
            let mut array_count = 1;
            if self.color_array.enabled {
                arrays[array_count] = self.color_array;
                array_count += 1;
            }
            if self.lighting.enabled && self.normal_array.enabled {
                arrays[array_count] = self.normal_array;
                array_count += 1;
            }
            if mode == GL_POINTS && self.point_size_array.enabled {
                arrays[array_count] = self.point_size_array;
                array_count += 1;
            }
            for (unit, state) in self.texture_units.iter().enumerate() {
                let array = state.texcoord_array;
                if coordinate_units & (1 << unit) != 0 && array.enabled {
                    arrays[array_count] = array;
                    array_count += 1;
                }
            }
            let mut spans = [(0u32, 0u32); MAX_TEXTURE_UNITS + 4];
            let mut span_count = 0;
            for &array in &arrays[..array_count] {
                if array.buffer != 0 {
                    continue;
                }
                let span = (|| {
                    let stride = array.element_stride()?;
                    let start = array.pointer.checked_add(lo.checked_mul(stride)?)?;
                    let len = hi
                        .checked_sub(lo)?
                        .checked_mul(stride)?
                        .checked_add(array.component_bytes()?.checked_mul(array.size)?)?;
                    start.checked_add(len)?;
                    // Sparse indices and enormous strides keep the element-by-element path.
                    if len > 1024 * 1024 || hi - lo > (count as u32).saturating_mul(4) {
                        return None;
                    }
                    Some((start, len as usize))
                })();
                if let Some((start, len)) = span {
                    spans[span_count] = (start, start + len as u32);
                    span_count += 1;
                }
            }
            spans[..span_count].sort_unstable_by_key(|s| s.0);
            let mut merged = 0;
            for i in 0..span_count {
                let (start, end) = spans[i];
                if merged > 0
                    && start <= spans[merged - 1].1
                    && end.max(spans[merged - 1].1) - spans[merged - 1].0 <= 1024 * 1024
                {
                    spans[merged - 1].1 = end.max(spans[merged - 1].1);
                } else {
                    spans[merged] = (start, end);
                    merged += 1;
                }
            }
            for &(start, end) in &spans[..merged] {
                let len = (end - start) as usize;
                let mut bytes = self.array_read_pool.pop().unwrap_or_default();
                bytes.clear();
                if mem.read(start, len, &mut bytes) && bytes.len() >= len {
                    bytes.truncate(len);
                    self.prepared_arrays.push((start, bytes));
                } else {
                    bytes.clear();
                    self.array_read_pool.push(bytes);
                }
            }
        }

        let transforms = crate::gpu::VertexTransforms {
            lighting: self.lighting,
            normal_matrix,
            projection: *self.projection.current(),
            modelview: *self.modelview.current(),
            textures: std::array::from_fn(|i| *self.texture_matrix[i].current()),
        };
        if self.gpu.is_some()
            && !framebuffer_fetch
            && !self.state.logic_op
            && !self.es11.pretransform
            && ((self.state.smooth_shading
                && matches!(
                    mode,
                    GL_TRIANGLES | GL_TRIANGLE_STRIP | GL_TRIANGLE_FAN | GL_POINTS | GL_LINES
                ))
                || (!self.state.smooth_shading && mode == GL_TRIANGLES))
        {
            let mut stream = std::mem::take(&mut self.vertex_stream);
            let mut flat = std::mem::take(&mut self.flat_vertex_stream);
            let mut ready = self.prepare_vertex_stream(
                &mut stream,
                indices.clone(),
                count,
                sequential,
                stages,
                mode == GL_POINTS && self.point_size_array.enabled,
            );
            if ready && !self.state.smooth_shading {
                ready = stream.expand_flat_triangles(&mut flat);
            }
            if ready {
                let prepared = if self.state.smooth_shading {
                    &stream
                } else {
                    &flat
                };
                let names = std::array::from_fn(|i| self.texture_units[i].bound_texture);
                let result = self.gpu.as_mut().unwrap().draw_stream_object_space(
                    &self.target,
                    &self.state,
                    mode,
                    prepared,
                    stages,
                    &self.textures,
                    names,
                    &transforms,
                );
                self.vertex_stream = stream;
                self.flat_vertex_stream = flat;
                self.recycle_prepared_arrays();
                if let Err(e) = result {
                    self.gpu_failure("vertex stream", e);
                }
                return;
            }
            self.vertex_stream = stream;
            self.flat_vertex_stream = flat;
        }
        // Native smooth draws retain the topology and prepare each unique
        // guest vertex directly, instead of expanding then hashing float records.
        let native_indices = !framebuffer_fetch
            && !self.state.logic_op
            && !sequential
            && self.gpu.is_some()
            && !self.es11.pretransform
            && self.state.smooth_shading
            && matches!(
                mode,
                GL_TRIANGLES | GL_TRIANGLE_STRIP | GL_TRIANGLE_FAN | GL_POINTS | GL_LINES
            );
        let mut remap = std::mem::take(&mut self.vertex_remap);
        let mut verts = std::mem::take(&mut self.assembled_vertices);
        let mut dense = std::mem::take(&mut self.dense_indices);
        let mut expanded = std::mem::take(&mut self.expanded_indices);
        // All exits (including malformed guest arrays and GPU failure) return here
        // before recycling. No guest pointers or previous vertex values survive.
        (|| {
            for i in indices {
                let index = if !sequential {
                    remap.get(&i).copied()
                } else {
                    None
                };
                let index = if let Some(index) = index {
                    index
                } else {
                    let Some(v) = self.assemble(mem, i, &mvp, coordinate_units, &normal_matrix)
                    else {
                        self.recycle_prepared_arrays();
                        return;
                    };
                    let index = verts.len();
                    verts.push(v);
                    if !sequential {
                        remap.insert(i, index);
                    }
                    index
                };
                if !sequential {
                    expanded.push(index);
                }
            }
            let direct_indexed = native_indices && verts.len() <= 65536;
            if direct_indexed {
                dense.extend(expanded.iter().map(|i| *i as u16));
            } else if !sequential {
                verts = expanded.iter().map(|i| verts[*i]).collect();
            }
            self.recycle_prepared_arrays();

            match mode {
                GL_TRIANGLES | GL_TRIANGLE_STRIP | GL_TRIANGLE_FAN | GL_POINTS | GL_LINES
                | GL_LINE_STRIP | GL_LINE_LOOP => {}
                _ => {
                    self.set_error(GL_INVALID_ENUM);
                    return;
                }
            }

            if self.es11.pretransform {
                let planes = self.clip_draw_planes();
                (mode, verts) =
                    raster::clip_user_primitives(mode, &verts, &planes, self.state.smooth_shading);
            }
            if let Some(gpu) = self.gpu.as_mut() {
                if self.es11.pretransform {
                    let names = std::array::from_fn(|i| self.texture_units[i].bound_texture);
                    let result = if mode == GL_POINTS {
                        let mut result = Ok(());
                        for v in &verts {
                            let mut state = self.state.clone();
                            state.point_size = if v.point_width >= 0. {
                                v.point_width
                            } else {
                                self.state.point_size
                            };
                            if let Err(error) = gpu.draw(
                                &self.target,
                                &state,
                                mode,
                                &[*v],
                                stages,
                                &self.textures,
                                names,
                            ) {
                                result = Err(error);
                                break;
                            }
                        }
                        result
                    } else {
                        gpu.draw(
                            &self.target,
                            &self.state,
                            mode,
                            &verts,
                            stages,
                            &self.textures,
                            names,
                        )
                    };
                    if let Err(error) = result {
                        self.gpu_failure("clipped draw", error);
                    }
                    return;
                }
                let names = std::array::from_fn(|i| self.texture_units[i].bound_texture);
                let result = if direct_indexed {
                    gpu.draw_indexed_object_space(
                        &self.target,
                        &self.state,
                        mode,
                        &verts,
                        &dense,
                        &stages,
                        &self.textures,
                        names,
                        &transforms,
                    )
                } else {
                    let draw = if sequential {
                        crate::gpu::Renderer::draw_arrays_object_space
                    } else {
                        crate::gpu::Renderer::draw_object_space
                    };
                    draw(
                        gpu,
                        &self.target,
                        &self.state,
                        mode,
                        &verts,
                        &stages,
                        &self.textures,
                        names,
                        &transforms,
                    )
                };
                if let Err(e) = result {
                    self.gpu_failure("draw", e);
                }
                return;
            }
            // Decode/generate only once per texture revision, and only in software.
            for stage in stages {
                let name = self.texture_units[stage.unit].bound_texture;
                if let Some(tex) = self.textures.get(&name) {
                    let mips = tex.min_filter.uses_mipmaps();
                    if mips || tex.rgba.is_empty() || tex.base_level != 0 {
                        if !self
                            .software_mips
                            .get(&name)
                            .is_some_and(|c| c.0 == tex.revision && c.1 == mips)
                        {
                            let mut levels = tex.upload_levels();
                            // Base zero is already owned by Texture; other bases
                            // and deferred compressed data must retain their snapshot.
                            if tex.base_level == 0 && !tex.rgba.is_empty() {
                                if let Some(base) = levels.first_mut() {
                                    base.2 = Vec::new();
                                }
                            }
                            self.software_mips
                                .insert(name, (tex.revision, mips, levels));
                        }
                    } else {
                        self.software_mips.remove(&name);
                    }
                }
            }
            // Resolve the bound textures once per draw, not per fragment.
            let mut texs: [Option<&Texture>; raster::MAX_TEXTURE_STAGES] = [None; _];
            for stage in stages {
                texs[stage.unit] = self
                    .textures
                    .get(&self.texture_units[stage.unit].bound_texture);
            }
            log::debug!(
            "GLES draw mode=0x{mode:04x} indices={} stages={:?} active_unit={} vertex_array={} blend={} src={:?} dst={:?} alpha_test={} func={:?} ref={}",
            count,
            stages
                .iter()
                .map(|s| (s.unit, self.texture_units[s.unit].bound_texture, s.env))
                .collect::<Vec<_>>(),
            self.active_texture,
            self.vertex_array.enabled,
            self.state.blend,
            self.state.blend_src,
            self.state.blend_dst,
            self.state.alpha_test,
            self.state.alpha_func,
            self.state.alpha_ref,
        );
            let sample = SoftwareSampler {
                combiner: self.combiner_programs.active(),
                combiner_constants: self.combiner_programs.constants(),
                textures: texs,
                cache: &self.software_mips,
                names: std::array::from_fn(|i| self.texture_units[i].bound_texture),
            };

            match mode {
                GL_POINTS => {
                    for v in &verts {
                        raster::draw_point(&mut self.target, &self.state, &sample, &stages, *v);
                    }
                    return;
                }
                GL_LINES | GL_LINE_STRIP | GL_LINE_LOOP => {
                    if mode == GL_LINES {
                        for v in verts.chunks_exact(2) {
                            raster::draw_line(
                                &mut self.target,
                                &self.state,
                                &sample,
                                &stages,
                                [v[0], v[1]],
                            );
                        }
                    } else {
                        for v in verts.windows(2) {
                            raster::draw_line(
                                &mut self.target,
                                &self.state,
                                &sample,
                                &stages,
                                [v[0], v[1]],
                            );
                        }
                        if mode == GL_LINE_LOOP && verts.len() > 1 {
                            raster::draw_line(
                                &mut self.target,
                                &self.state,
                                &sample,
                                &stages,
                                [*verts.last().unwrap(), verts[0]],
                            );
                        }
                    }
                    return;
                }
                _ => {}
            }

            let compatibility_pass = self
                .combiner_programs
                .active()
                .is_none()
                .then(|| self.state.intel2700g_depth_color_pass())
                .flatten();
            for pass in std::iter::once(&self.state).chain(compatibility_pass.as_ref()) {
                // Stream primitives in their original order without allocating a triangle list.
                let mut draw = |tri: [Vertex; 3]| {
                    raster::draw_triangle(&mut self.target, pass, &sample, stages, tri);
                };
                match mode {
                    GL_TRIANGLES => {
                        for c in verts.chunks_exact(3) {
                            draw([c[0], c[1], c[2]]);
                        }
                    }
                    GL_TRIANGLE_STRIP => {
                        for i in 2..verts.len() {
                            // Preserve alternating winding and the provoking vertex.
                            if i % 2 == 0 {
                                draw([verts[i - 2], verts[i - 1], verts[i]]);
                            } else {
                                draw([verts[i - 1], verts[i - 2], verts[i]]);
                            }
                        }
                    }
                    GL_TRIANGLE_FAN => {
                        for i in 2..verts.len() {
                            draw([verts[0], verts[i - 1], verts[i]]);
                        }
                    }
                    _ => unreachable!(),
                }
            }
        })();
        verts.clear();
        dense.clear();
        expanded.clear();
        remap.clear();
        // Bound retained storage after exceptional large/sparse guest submissions.
        if verts.capacity() <= 65536 {
            self.assembled_vertices = verts;
        }
        if dense.capacity() <= 131072 {
            self.dense_indices = dense;
        }
        if expanded.capacity() <= 131072 {
            self.expanded_indices = expanded;
        }
        if remap.capacity() <= 131072 {
            self.vertex_remap = remap;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires host OpenGL; context configuration and full resolution presentation"]
    fn internal_resolution_context_exports_sharp_host_image_and_native_guest_image() {
        let mut c = Context::new(16, 16);
        c.set_internal_resolution(2);
        c.enable_gpu().unwrap();
        c.clear_color = [1., 0., 0., 1.];
        c.clear(GL_COLOR_BUFFER_BIT);
        let mut guest = vec![0; 16 * 16 * 2];
        c.present_rgb565(&mut guest);
        assert_eq!((c.target.width, c.target.height), (16, 16));
        assert_eq!(&guest[..2], &0xf800u16.to_le_bytes());
        let mut host = Vec::new();
        assert_eq!(c.scaled_rgb565_into(&mut host), Some((32, 32)));
        assert_eq!(host.len(), 32 * 32 * 2);
        c.set_internal_resolution(1);
        c.clear_color = [0., 0., 1., 1.];
        c.clear(GL_COLOR_BUFFER_BIT);
        c.present_rgb565(&mut guest);
        assert_eq!(&guest[..2], &0x001fu16.to_le_bytes());
        assert_eq!(c.scaled_rgb565_into(&mut host), None);
    }

    #[test]
    #[ignore = "requires native OpenGL; restored EGL surface policy"]
    fn internal_resolution_policy_reapplies_to_parked_surface_renderers() {
        let mut c = Context::new(320, 240);
        c.set_internal_resolution(3);
        c.enable_gpu().unwrap();
        c.clear(GL_COLOR_BUFFER_BIT);
        let mut qvga = SurfaceBuffers::new(640, 480);
        c.exchange_surface(&mut qvga);
        c.clear(GL_COLOR_BUFFER_BIT);
        let mut guest = vec![0; 640 * 480 * 2];
        c.present_rgb565(&mut guest);
        assert_eq!(c.scaled_rgb565_into(&mut Vec::new()), None);
        let mut parked = SurfaceBuffers::new(1, 1);
        c.park_surface(&mut parked);
        c.set_internal_resolution(2);
        c.exchange_surface(&mut parked);
        c.clear(GL_COLOR_BUFFER_BIT);
        c.present_rgb565(&mut guest);
        assert_eq!(c.scaled_rgb565_into(&mut Vec::new()), Some((1280, 960)));
        c.set_internal_resolution(3);
        c.exchange_surface(&mut qvga);
        c.clear(GL_COLOR_BUFFER_BIT);
        let mut guest = vec![0; 320 * 240 * 2];
        c.present_rgb565(&mut guest);
        assert_eq!(c.scaled_rgb565_into(&mut Vec::new()), Some((640, 480)));
    }

    #[test]
    #[ignore = "requires native OpenGL"]
    fn higher_qvga_resolution_keeps_guest_coordinates_and_leaves_vga_native() {
        for (policy, scale) in [(4, 3), (5, 4)] {
            for (w, h) in [(320, 240), (240, 320), (640, 480), (480, 640)] {
                let expected = if w * h == 320 * 240 {
                    Some((w * scale, h * scale))
                } else {
                    None
                };
                let mut c = Context::new(w, h);
                c.set_internal_resolution(policy);
                c.enable_gpu().unwrap();
                c.clear_color = [1., 0., 0., 1.];
                c.clear(GL_COLOR_BUFFER_BIT);
                let mut guest = vec![0; (w * h * 2) as usize];
                c.present_rgb565(&mut guest);
                assert_eq!((c.target.width, c.target.height), (w, h));
                assert_eq!(c.state.viewport, (0, 0, w as i32, h as i32));
                assert_eq!(c.scaled_rgb565_into(&mut Vec::new()), expected);
                assert_eq!(&guest[..2], &0xf800u16.to_le_bytes());
            }
        }
    }

    #[test]
    fn internal_resolution_never_changes_software_geometry_or_creates_a_gpu() {
        let mut c = Context::new(240, 320);
        c.set_internal_resolution(2);
        assert!(!c.gpu_active());
        assert_eq!((c.target.width, c.target.height), (240, 320));
        assert_eq!(c.state.viewport, (0, 0, 240, 320));
        assert_eq!(c.scaled_rgb565_into(&mut Vec::new()), None);
        c.set_internal_resolution(3);
        assert_eq!(c.internal_resolution, 3);
        assert!(!c.gpu_active());
        c.set_internal_resolution(4);
        assert_eq!(c.internal_resolution, 4);
        assert!(!c.gpu_active());
        c.set_internal_resolution(99);
        assert_eq!(c.internal_resolution, 1);
    }

    #[test]
    fn finish_and_flush_leave_synchronous_software_state_intact() {
        let mut c = Context::new(16, 16);
        c.target.color.fill(123);
        c.set_error(GL_INVALID_ENUM);
        c.flush();
        c.finish();
        assert!(!c.gpu_active(), "software completion must not create a GPU");
        assert!(c.target.color.iter().all(|&v| v == 123));
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }

    fn scissor_enable_sequence(c: &mut Context) {
        let mut mem = FakeMem {
            base: BASE,
            bytes: quad_floats(),
        };
        c.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: BASE,
            buffer: 0,
        };
        c.set_capability(GL_SCISSOR_TEST, true);
        c.clear_color = [1., 0., 0., 1.];
        c.clear(GL_COLOR_BUFFER_BIT);
        c.sync_gpu();
        assert_eq!(pixel(c, 1, 1), [255, 0, 0, 255]);
        c.set_capability(GL_SCISSOR_TEST, false);
        c.set_scissor(16, 16, 16, 16);
        c.clear_color = [0., 0., 0., 1.];
        c.clear(GL_COLOR_BUFFER_BIT);
        c.current_color = [0., 1., 0., 1.];
        c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
        c.sync_gpu();
        // Setting a box while disabled must not clip either clear or draw.
        assert_eq!(pixel(c, 1, 1), [0, 255, 0, 255]);
        c.set_capability(GL_SCISSOR_TEST, true);
        c.clear_color = [1., 0., 0., 1.];
        c.clear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);
        c.sync_gpu();
        assert_eq!(pixel(c, 20, 40), [255, 0, 0, 255]);
        assert_eq!(pixel(c, 1, 1), [0, 255, 0, 255]);
        c.current_color = [0., 0., 1., 1.];
        c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
        c.sync_gpu();
        assert_eq!(pixel(c, 20, 40), [0, 0, 255, 255]);
        assert_eq!(pixel(c, 1, 1), [0, 255, 0, 255]);
        c.set_capability(GL_SCISSOR_TEST, false);
        c.set_scissor(0, 0, 8, 8);
        c.set_capability(GL_SCISSOR_TEST, true);
        c.set_scissor(0, 0, -1, 8);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        c.clear_color = [1., 1., 0., 1.];
        c.clear(GL_COLOR_BUFFER_BIT);
        c.sync_gpu();
        assert_eq!(pixel(c, 2, 60), [255, 255, 0, 255]);
        assert_eq!(pixel(c, 20, 40), [0, 0, 255, 255]);
        // Disable/re-enable preserves the most recently specified rectangle.
        c.set_capability(GL_SCISSOR_TEST, false);
        c.set_capability(GL_SCISSOR_TEST, true);
        c.clear_color = [1., 0., 1., 1.];
        c.clear(GL_COLOR_BUFFER_BIT);
        c.sync_gpu();
        assert_eq!(pixel(c, 2, 60), [255, 0, 255, 255]);
        assert_eq!(pixel(c, 20, 40), [0, 0, 255, 255]);
        c.set_scissor(0, 0, 0, 0);
        c.clear_color = [1., 1., 1., 1.];
        c.clear(GL_COLOR_BUFFER_BIT);
        c.sync_gpu();
        assert_eq!(pixel(c, 2, 60), [255, 0, 255, 255]);
        c.set_capability(GL_SCISSOR_TEST, false);
        c.clear(GL_COLOR_BUFFER_BIT);
        c.sync_gpu();
        assert_eq!(pixel(c, 2, 60), [255, 255, 255, 255]);
        assert_eq!(pixel(c, 20, 40), [255, 255, 255, 255]);
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }

    #[test]
    fn scissor_box_does_not_enable_clipping_and_survives_disable() {
        scissor_enable_sequence(&mut ctx());
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native GL/GLES2; scissor enable and rectangle are independent"]
    fn gpu_scissor_box_does_not_enable_clipping_and_survives_disable() {
        for es in [false, true] {
            scissor_enable_sequence(&mut native_ctx(es));
        }
    }

    use super::*;

    #[test]
    fn explicit_mip_upload_subimage_copy_and_bounds() {
        let mut c = Context::new(4, 4);
        c.bind_texture(GL_TEXTURE_2D, 1);
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            4,
            4,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[255, 0, 0, 255].repeat(16),
        );
        c.tex_image_2d(
            GL_TEXTURE_2D,
            1,
            2,
            2,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[0, 255, 0, 255].repeat(4),
        );
        c.tex_sub_image_2d(
            GL_TEXTURE_2D,
            1,
            1,
            1,
            1,
            1,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[0, 0, 255, 255],
        );
        assert_eq!(&c.textures[&1].mipmaps[&1].2[12..], &[0, 0, 255, 255]);
        c.target.color.fill(255);
        c.copy_tex_image_2d(GL_TEXTURE_2D, 2, GL_RGBA, 0, 0, 1, 1, 0);
        assert_eq!(c.textures[&1].mipmaps[&2].2, [255; 4]);
        assert_eq!(c.textures[&1].width, 4);
        c.tex_image_2d(GL_TEXTURE_2D, 11, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, &[0; 4]);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        assert_eq!(c.textures[&1].max_level, 1000);
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MAX_LEVEL_SGIS, 0);
        assert_eq!(c.textures[&1].max_level, 0);
        let revision = c.textures[&1].revision;
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MAX_LEVEL_SGIS, u32::MAX);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        assert_eq!(c.textures[&1].revision, revision);
        assert_eq!(c.textures[&1].max_level, 0);
        c.set_lod_bias(-0.5);
        assert_eq!(c.texture_units[0].lod_bias, -0.5);
        c.set_lod_bias(f32::NAN);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        assert_eq!(c.texture_units[0].lod_bias, -0.5);
    }

    #[test]
    #[ignore = "requires host OpenGL; framebuffer copy synchronizes native pixels"]
    fn gpu_framebuffer_copy_reads_native_color() {
        let mut c = Context::new(4, 4);
        c.enable_gpu().unwrap();
        c.clear_color = [1., 0.5, 0., 1.];
        c.clear(GL_COLOR_BUFFER_BIT);
        c.bind_texture(GL_TEXTURE_2D, 3);
        c.copy_tex_image_2d(GL_TEXTURE_2D, 0, GL_RGBA, 1, 1, 2, 2, 0);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        for p in c.textures[&3].rgba.chunks_exact(4) {
            assert_eq!(p[0], 255);
            assert!((127..=128).contains(&p[1]));
            assert_eq!(&p[2..], &[0, 255]);
        }
    }

    #[cfg(unix)]
    fn native_ctx(es: bool) -> Context {
        let mut c = ctx();
        c.gpu = Some(if es {
            crate::gpu::Renderer::new_es_for_test(64, 64).unwrap()
        } else {
            crate::gpu::Renderer::new(64, 64).unwrap()
        });
        c.gpu_requested = true;
        c
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native GL/GLES; exact fixed clip boundaries and stream format transitions"]
    fn gpu_fixed_clip_boundaries_match_float_with_client_arrays_and_vbos() {
        for es in [false, true] {
            for rgb565 in [false, true] {
                for scale in [1, 2] {
                    let mut c = native_ctx(es);
                    c.target = raster::RenderTarget::with_format(64, 64, rgb565);
                    c.gpu.as_mut().unwrap().set_surface_rgb565(rgb565).unwrap();
                    c.set_internal_resolution(scale);
                    c.state.intel2700g_depth_bias = true;
                    c.set_client_state(GL_VERTEX_ARRAY, true);
                    let buffer = c.gen_buffers(1)[0];
                    for ty in [GL_FIXED, GL_FLOAT, GL_FIXED] {
                        for size in [2, 3, 4] {
                            for z in [-1., 1.] {
                                let mut bytes = Vec::new();
                                for (x, y) in [(-1., -1.), (1., -1.), (-1., 1.), (1., 1.)] {
                                    for value in [x, y, z, 1.].into_iter().take(size as usize) {
                                        bytes.extend(if ty == GL_FIXED {
                                            fixed::from_f32(value).to_le_bytes()
                                        } else {
                                            value.to_le_bytes()
                                        });
                                    }
                                }
                                let mut mem = FakeMem { base: BASE, bytes };
                                for vbo in [false, true] {
                                    c.bind_buffer(GL_ARRAY_BUFFER, if vbo { buffer } else { 0 });
                                    if vbo {
                                        c.buffer_data(
                                            GL_ARRAY_BUFFER,
                                            mem.bytes.len(),
                                            Some(&mem.bytes),
                                            GL_DYNAMIC_DRAW,
                                        );
                                    }
                                    c.set_vertex_pointer(size, ty, 0, if vbo { 0 } else { BASE });
                                    c.current_color = if vbo {
                                        [0., 1., 0., 1.]
                                    } else {
                                        [1., 0., 0., 1.]
                                    };
                                    c.clear_color = [0., 0., 0., 1.];
                                    c.clear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);
                                    c.draw_arrays(&mut mem, GL_TRIANGLE_STRIP, 0, 4);
                                    c.sync_gpu();
                                    let expected = if vbo {
                                        [0, 255, 0, 255]
                                    } else {
                                        [255, 0, 0, 255]
                                    };
                                    assert!(c.target.color.chunks_exact(4).all(|p| p == expected), "es={es}, rgb565={rgb565}, scale={scale}, ty={ty}, size={size}, z={z}, vbo={vbo}");
                                    assert!(c.gpu_active());
                                    assert_eq!(c.take_error(), GL_NO_ERROR);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native GL/GLES2; Colors snapshots, indices and stream/fallback switching"]
    fn gpu_colors_stream_refreshes_mutable_guest_data_and_reuses_storage() {
        let indices = [
            0, 2, 1, 2, 0, 3, 1, 5, 4, 5, 1, 2, 5, 3, 6, 3, 5, 2, 3, 7, 6, 7, 3, 0, 6, 4, 5, 4, 6,
            7, 1, 7, 0, 7, 1, 4,
        ];
        for es in [false, true] {
            let mut c = native_ctx(es);
            // Three-component colors are a measured GoForce driver variant.
            // Keep exercising their typed fast path under the appropriate profile.
            c.gizmondo_goforce_20051013 = true;
            let mut bytes = Vec::new();
            for i in 0..8 {
                bytes.extend(fixeds(&[
                    if i & 1 == 0 { -0.5 } else { 0.5 },
                    if i & 2 == 0 { -0.5 } else { 0.5 },
                    0.,
                ]));
            }
            for _ in 0..8 {
                bytes.extend(fixeds(&[1., 0., 0.]));
            }
            let mut mem = CountMem {
                inner: FakeMem { base: BASE, bytes },
                reads: 0,
                reject_bulk: false,
            };
            c.set_vertex_pointer(3, GL_FIXED, 0, BASE);
            c.set_color_pointer(3, GL_FIXED, 0, BASE + 96);
            c.set_client_state(GL_VERTEX_ARRAY, true);
            c.set_client_state(GL_COLOR_ARRAY, true);
            c.draw_elements(&mut mem, GL_TRIANGLES, &indices);
            c.sync_gpu();
            assert_eq!(mem.reads, 1);
            assert_eq!(c.vertex_stream.bytes.len(), 192);
            assert_eq!(c.vertex_stream.indices.len() * 2, 72);
            assert_eq!(pixel(&c, 32, 32), [255, 0, 0, 255]);
            let ptr = c.vertex_stream.bytes.as_ptr();
            let pool_ptr = c.array_read_pool[0].as_ptr();
            for i in 0..8 {
                mem.inner.bytes[96 + i * 12..108 + i * 12].copy_from_slice(&fixeds(&[0., 1., 0.]));
            }
            c.draw_elements(&mut mem, GL_TRIANGLES, &indices);
            c.sync_gpu();
            assert_eq!(mem.reads, 2);
            assert_eq!(pixel(&c, 32, 32), [0, 255, 0, 255]);
            assert_eq!(c.vertex_stream.bytes.as_ptr(), ptr);
            assert_eq!(c.array_read_pool[0].as_ptr(), pool_ptr);
            // Failed coalesced reads must retain the elementwise fallback.
            mem.reject_bulk = true;
            c.draw_elements(&mut mem, GL_TRIANGLES, &indices);
            c.sync_gpu();
            assert_eq!(mem.reads, 19);
            assert_eq!(pixel(&c, 32, 32), [0, 255, 0, 255]);
            mem.reject_bulk = false;
            // Alternate packed/flat and typed/smooth attribute layouts.
            c.set_shade_model(GL_FLAT);
            c.draw_elements(&mut mem, GL_TRIANGLES, &indices);
            c.sync_gpu();
            assert_eq!(pixel(&c, 32, 32), [0, 255, 0, 255]);
            c.set_shade_model(GL_SMOOTH);
            c.set_client_state(GL_COLOR_ARRAY, false);
            c.current_color = [0., 0., 1., 1.];
            c.draw_elements(&mut mem, GL_TRIANGLES, &indices);
            c.sync_gpu();
            assert_eq!(pixel(&c, 32, 32), [0, 0, 255, 255]);
            assert_eq!(c.take_error(), GL_NO_ERROR);
            c.disable_gpu();
            c.current_color = [1., 1., 0., 1.];
            c.draw_elements(&mut mem, GL_TRIANGLES, &indices);
            assert_eq!(pixel(&c, 32, 32), [255, 255, 0, 255]);
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native GL/GLES2; captured VBOs, strides, indices and update semantics"]
    fn gpu_stream_preserves_vbo_capture_and_first_with_normalized_colours() {
        for es in [false, true] {
            let mut c = native_ctx(es);
            let mut positions = Vec::new();
            for pos in [[0i16, 0, 0], [-1, -1, 0], [1, -1, 0], [0, 1, 0]] {
                for n in pos {
                    positions.extend(n.to_le_bytes());
                }
                positions.extend([0x5a, 0x5a]);
            }
            let names = c.gen_buffers(3);
            c.bind_buffer(GL_ARRAY_BUFFER, names[0]);
            c.buffer_data(
                GL_ARRAY_BUFFER,
                positions.len(),
                Some(&positions),
                GL_DYNAMIC_DRAW,
            );
            c.set_vertex_pointer(3, GL_SHORT, 8, 0);
            c.set_client_state(GL_VERTEX_ARRAY, true);
            c.bind_buffer(GL_ARRAY_BUFFER, names[1]);
            c.buffer_data(
                GL_ARRAY_BUFFER,
                16,
                Some(&[255, 0, 0, 255].repeat(4)),
                GL_DYNAMIC_DRAW,
            );
            c.set_color_pointer(4, GL_UNSIGNED_BYTE, 4, 0);
            c.set_client_state(GL_COLOR_ARRAY, true);
            c.bind_buffer(GL_ELEMENT_ARRAY_BUFFER, names[2]);
            c.buffer_data(
                GL_ELEMENT_ARRAY_BUFFER,
                3,
                Some(&[1, 2, 3]),
                GL_DYNAMIC_DRAW,
            );
            let mut mem = NoMem;
            c.draw_elements_from_guest(&mut mem, GL_TRIANGLES, 3, GL_UNSIGNED_BYTE, 0);
            c.sync_gpu();
            assert_eq!(c.vertex_stream.count, 3);
            assert_eq!(c.vertex_stream.indices, [0, 1, 2]);
            assert_eq!(c.vertex_stream.attributes[1].unwrap().normalized, 1);
            assert_eq!(pixel(&c, 32, 32), [255, 0, 0, 255]);
            let index_ptr = c.decoded_indices.as_ptr();
            c.bind_buffer(GL_ARRAY_BUFFER, names[1]);
            c.buffer_sub_data(GL_ARRAY_BUFFER, 0, &[0, 255, 0, 255].repeat(4));
            // ARRAY_BUFFER at draw time must not replace the captured position binding.
            c.draw_elements_from_guest(&mut mem, GL_TRIANGLES, 3, GL_UNSIGNED_BYTE, 0);
            c.sync_gpu();
            assert_eq!(c.decoded_indices.as_ptr(), index_ptr);
            assert_eq!(pixel(&c, 32, 32), [0, 255, 0, 255]);
            c.draw_arrays(&mut mem, GL_TRIANGLES, 1, 3);
            c.sync_gpu();
            assert_eq!(pixel(&c, 32, 32), [0, 255, 0, 255]);
            c.bind_buffer(GL_ELEMENT_ARRAY_BUFFER, names[2]);
            c.buffer_data(
                GL_ELEMENT_ARRAY_BUFFER,
                6,
                Some(&[1, 0, 2, 0, 3, 0]),
                GL_DYNAMIC_DRAW,
            );
            c.draw_elements_from_guest(&mut mem, GL_TRIANGLES, 3, GL_UNSIGNED_SHORT, 0);
            c.sync_gpu();
            assert_eq!(pixel(&c, 32, 32), [0, 255, 0, 255]);
            assert_eq!(c.take_error(), GL_NO_ERROR);
        }
    }

    struct CountMem {
        inner: FakeMem,
        reads: usize,
        reject_bulk: bool,
    }
    impl GuestMemory for CountMem {
        fn read(&mut self, addr: u32, len: usize, out: &mut Vec<u8>) -> bool {
            self.reads += 1;
            if self.reject_bulk && len > 16 {
                return false;
            }
            self.inner.read(addr, len, out)
        }
    }
    #[test]
    fn colors_shared_vertices_share_a_bulk_read_and_refresh_each_draw() {
        let mut c = ctx();
        let mut bytes = Vec::new();
        for i in 0..8 {
            bytes.extend(floats(&[
                if i & 1 == 0 { -0.5 } else { 0.5 },
                if i & 2 == 0 { -0.5 } else { 0.5 },
                0.,
            ]));
        }
        for _ in 0..8 {
            bytes.extend(floats(&[1., 0., 0., 1.]));
        }
        let mut mem = CountMem {
            inner: FakeMem { base: BASE, bytes },
            reads: 0,
            reject_bulk: false,
        };
        c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
        c.set_color_pointer(4, GL_FLOAT, 0, BASE + 96);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        c.set_client_state(GL_COLOR_ARRAY, true);
        let indices = [
            0, 2, 1, 2, 0, 3, 1, 5, 4, 5, 1, 2, 5, 3, 6, 3, 5, 2, 3, 7, 6, 7, 3, 0, 6, 4, 5, 4, 6,
            7, 1, 7, 0, 7, 1, 4,
        ];
        c.draw_elements(&mut mem, GL_TRIANGLES, &indices);
        assert_eq!(mem.reads, 1); // Adjacent positions/colours share one snapshot.
        for i in 0..8 {
            mem.inner.bytes[96 + i * 16..112 + i * 16].copy_from_slice(&floats(&[0., 1., 0., 1.]));
        }
        c.draw_elements(&mut mem, GL_TRIANGLES, &indices);
        assert_eq!(mem.reads, 2);
        assert_eq!(pixel(&c, 32, 32), [0, 255, 0, 255]);
        mem.reject_bulk = true;
        c.draw_elements(&mut mem, GL_TRIANGLES, &indices);
        assert_eq!(mem.reads, 19); // One failed merged span, sixteen valid elements.
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }
    #[test]
    fn sparse_and_overflowing_arrays_do_not_prefetch_large_spans() {
        let mut c = ctx();
        let mut mem = CountMem {
            inner: FakeMem {
                base: BASE,
                bytes: floats(&[0., 0., 0.]),
            },
            reads: 0,
            reject_bulk: true,
        };
        c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        c.draw_elements(&mut mem, GL_TRIANGLES, &[0, 65535, 0]);
        assert_eq!(mem.reads, 2); // Only requested elements, no sparse-span read.
        c.set_vertex_pointer(3, GL_FLOAT, 0, u32::MAX - 4);
        c.draw_elements(&mut mem, GL_POINTS, &[1]);
        assert_eq!(mem.reads, 2); // Checked offset overflow never reaches memory.
        assert!(c.prepared_arrays.is_empty());
    }

    #[test]
    fn framebuffer_copy_has_gl_origin_formats_and_errors() {
        let mut c = Context::new(2, 2);
        c.target.color = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        c.bind_texture(GL_TEXTURE_2D, 7);
        c.copy_tex_image_2d(GL_TEXTURE_2D, 0, GL_RGBA, 0, 0, 2, 2, 0);
        assert_eq!(
            c.textures[&7].rgba,
            [9, 10, 11, 12, 13, 14, 15, 16, 1, 2, 3, 4, 5, 6, 7, 8]
        );
        c.copy_tex_image_2d(GL_TEXTURE_2D, 0, GL_RGB, 1, 1, 1, 1, 0);
        assert_eq!(c.textures[&7].rgba, [5, 6, 7, 255]);
        c.copy_tex_image_2d(GL_TEXTURE_2D, 0, GL_LUMINANCE_ALPHA, 0, 0, 1, 1, 0);
        assert_eq!(c.textures[&7].rgba, [9, 9, 9, 12]);
        c.copy_tex_image_2d(GL_TEXTURE_2D, 0, GL_RGBA, 0, 0, -1, 2, 0);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        c.copy_tex_image_2d(GL_TEXTURE_2D, 0, GL_RGBA, 0, 0, 1, 2, 1);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        c.copy_tex_image_2d(GL_TEXTURE_2D, 0, 0, 0, 0, 1, 2, 0);
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
    }

    /// Guest memory backed by a flat buffer based at `base`.
    struct FakeMem {
        base: u32,
        bytes: Vec<u8>,
    }

    impl GuestMemory for FakeMem {
        fn read(&mut self, addr: u32, len: usize, out: &mut Vec<u8>) -> bool {
            let Some(off) = addr.checked_sub(self.base) else {
                return false;
            };
            let off = off as usize;
            if off + len > self.bytes.len() {
                return false;
            }
            out.clear();
            out.extend_from_slice(&self.bytes[off..off + len]);
            true
        }
    }

    const BASE: u32 = 0x1000;
    fn lighting_sample(gpu: bool, scenario: usize) -> [u8; 4] {
        let mut c = ctx();
        c.lighting.enabled = true;
        c.lighting.lights[0].enabled = true;
        c.lighting.model_ambient = [0.; 4];
        c.lighting.diffuse = [0.8, 0.4, 0.2, 0.5];
        c.lighting.normalize = true;
        let mut bytes = floats(&[-0.9, -0.9, 0., 0.9, -0.9, 0., 0., 0.9, 0.]);
        bytes.extend([0u8, 0, 127, 0, 0, 127, 0, 0, 127]);
        match scenario {
            1 => {
                c.lighting.two_sided = true;
                c.state.front_face = raster::FrontFace::Cw;
                for i in [38, 41, 44] {
                    bytes[i] = 128;
                }
            }
            2 => {
                c.state.smooth_shading = false;
                bytes[44] = 128;
            }
            3 => {
                c.modelview.load(matrix::scale(0.75, 0.5, 2.));
                c.lighting.normalize = false;
                c.lighting.rescale = true;
            }
            4 => {
                c.lighting.lights[0].position = [0., 0., 2., 1.];
                c.lighting.lights[0].attenuation = [1., 0.25, 0.1];
            }
            5 => {
                c.lighting.lights[0].position = [0., 0., 2., 1.];
                c.lighting.lights[0].spot_cutoff = 40.;
                c.lighting.lights[0].spot_exponent = 4.;
            }
            6 => {
                c.lighting.specular = [0.3, 0.2, 0.1, 1.];
                c.lighting.shininess = 16.;
            }
            7 => {
                c.lighting.color_material = true;
                c.current_color = [0.2, 0.4, 0.8, 0.25];
            }
            _ => {}
        }
        let mut mem = FakeMem { base: BASE, bytes };
        c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        c.set_normal_pointer(GL_BYTE, 0, BASE + 36);
        c.set_client_state(GL_NORMAL_ARRAY, true);
        if gpu {
            c.enable_gpu().unwrap();
        }
        c.draw_arrays(&mut mem, GL_TRIANGLES, 0, 3);
        c.sync_gpu();
        assert_eq!(c.take_error(), GL_NO_ERROR);
        pixel(&c, 32, 32)
    }

    #[test]
    fn software_lighting_reads_normals_two_sides_flat_and_rescale() {
        for scenario in [0, 1, 3] {
            let p = lighting_sample(false, scenario);
            for (a, b) in p.into_iter().zip([204u8, 102, 51, 128]) {
                assert!(a.abs_diff(b) <= 1, "scenario {scenario}: {p:?}");
            }
        }
        assert_eq!(&lighting_sample(false, 2)[..3], &[0, 0, 0]);
        assert!(lighting_sample(false, 4)[0] < 204);
        assert!(lighting_sample(false, 5)[0] > 0);
        assert!(lighting_sample(false, 6)[0] > 204);
        assert_eq!(lighting_sample(false, 7), [51, 102, 204, 64]);
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires host OpenGL; compares software and native fixed lighting"]
    fn software_and_gpu_fixed_lighting_match() {
        for scenario in 0..8 {
            let cpu = lighting_sample(false, scenario);
            let gpu = lighting_sample(true, scenario);
            for (a, b) in cpu.into_iter().zip(gpu) {
                assert!(
                    a.abs_diff(b) <= 2,
                    "scenario {scenario}: CPU {cpu:?}, GPU {gpu:?}"
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires host OpenGL; signed guest normal arrays drive lighting"]
    fn gpu_signed_normal_arrays_change_fixed_lighting() {
        let mut c = ctx();
        c.lighting.enabled = true;
        c.lighting.lights[0].enabled = true;
        c.lighting.model_ambient = [0.; 4];
        c.lighting.diffuse = [0.8, 0.4, 0.2, 0.5];
        let mut bytes = floats(&[-0.9, -0.9, 0., 0.9, -0.9, 0., 0., 0.9, 0.]);
        bytes.extend([0u8, 0, 127, 0, 0, 127, 0, 0, 127]);
        let mut mem = FakeMem { base: BASE, bytes };
        c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        c.set_normal_pointer(GL_BYTE, 0, BASE + 36);
        c.set_client_state(GL_NORMAL_ARRAY, true);
        c.enable_gpu().unwrap();
        c.clear(GL_COLOR_BUFFER_BIT);
        c.draw_arrays(&mut mem, GL_TRIANGLES, 0, 3);
        c.sync_gpu();
        let shown = pixel(&c, 32, 32);
        for (actual, expected) in shown.into_iter().zip([204u8, 102, 51, 128]) {
            assert!(actual.abs_diff(expected) <= 1);
        }
        for offset in [38, 41, 44] {
            mem.bytes[offset] = 128;
        }
        c.clear(GL_COLOR_BUFFER_BIT);
        c.draw_arrays(&mut mem, GL_TRIANGLES, 0, 3);
        c.sync_gpu();
        assert_eq!(&pixel(&c, 32, 32)[..3], &[0, 0, 0]);
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires host OpenGL; validates guest arrays through the vertex shader"]
    fn gpu_guest_arrays_keep_object_coordinates_and_match_cpu_reference() {
        let mut c = ctx();
        c.modelview.load(matrix::multiply(
            &matrix::translate(0.125, 0., -0.25),
            &matrix::rotate(17., 0., 0., 1.),
        ));
        c.projection.load(matrix::scale(0.75, 0.625, 1.));
        c.state.fog = true;
        c.state.fog_start = 0.;
        c.state.fog_end = 1.;
        c.state.fog_color = [0.1, 0.2, 0.3, 1.];
        c.current_color = [1., 0.5, 0.25, 1.];
        let coordinates = [-0.75, -0.75, 0., 0.75, -0.75, 0., 0., 0.75, 0.];
        let mut mem = FakeMem {
            base: BASE,
            bytes: floats(&coordinates),
        };
        c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        let mvp = c.mvp();
        let reference: Vec<_> = (0..3)
            .map(|i| c.assemble(&mut mem, i, &mvp, 0, &matrix::IDENTITY).unwrap())
            .collect();
        let mut renderer = crate::gpu::Renderer::new(c.target.width, c.target.height).unwrap();
        renderer
            .clear(
                c.target.width,
                c.target.height,
                &c.state,
                c.clear_color,
                1.,
                GL_COLOR_BUFFER_BIT,
            )
            .unwrap();
        renderer
            .draw(
                &c.target,
                &c.state,
                GL_TRIANGLES,
                &reference,
                &[],
                &c.textures,
                [0; 6],
            )
            .unwrap();
        let mut expected = raster::RenderTarget::new(c.target.width, c.target.height);
        renderer.read(&mut expected).unwrap();
        c.enable_gpu().unwrap();
        let raw = c.assemble(&mut mem, 0, &mvp, 0, &matrix::IDENTITY).unwrap();
        assert_eq!(raw.pos, [-0.75, -0.75, 0., 1.]);
        assert_ne!(raw.pos, reference[0].pos);
        c.clear(GL_COLOR_BUFFER_BIT);
        c.draw_arrays(&mut mem, GL_TRIANGLES, 0, 3);
        c.sync_gpu();
        assert_eq!(c.take_error(), GL_NO_ERROR);
        let differences = expected
            .color
            .iter()
            .zip(&c.target.color)
            .filter(|(a, b)| a.abs_diff(**b) > 2)
            .count();
        assert!(
            differences <= 12,
            "guest draw differs in {differences} channels"
        );
        c.disable_gpu();
        let restored = c.assemble(&mut mem, 0, &mvp, 0, &matrix::IDENTITY).unwrap();
        assert_eq!(restored.pos, reference[0].pos);
    }

    fn floats(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    fn fixeds(v: &[f32]) -> Vec<u8> {
        v.iter()
            .flat_map(|f| fixed::from_f32(*f).to_le_bytes())
            .collect()
    }

    fn ctx() -> Context {
        let mut c = Context::new(64, 64);
        c.state.viewport = (0, 0, 64, 64);
        c
    }

    fn pixel(c: &Context, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * c.target.width + x) * 4) as usize;
        [
            c.target.color[i],
            c.target.color[i + 1],
            c.target.color[i + 2],
            c.target.color[i + 3],
        ]
    }

    fn covered(c: &Context) -> usize {
        c.target
            .color
            .chunks_exact(4)
            .filter(|p| p[..3] != [0, 0, 0])
            .count()
    }

    /// A full-viewport quad in NDC, as two triangles worth of vertices.
    fn quad_floats() -> Vec<u8> {
        floats(&[
            -1.0, -1.0, 0.0, //
            1.0, -1.0, 0.0, //
            1.0, 1.0, 0.0, //
            -1.0, 1.0, 0.0,
        ])
    }

    #[test]
    #[ignore = "requires native OpenGL/GLES; client-state lifetime and texture caches"]
    fn client_state_reset_preserves_surface_policy_and_invalidates_reused_texture_names() {
        for es in [false, true] {
            let mut c = Context::new_rgb565(8, 8);
            if es {
                let mut renderer = crate::gpu::Renderer::new_es_for_test(8, 8).unwrap();
                renderer.set_surface_rgb565(true).unwrap();
                c.gpu = Some(renderer);
                c.gpu_requested = true;
            } else {
                c.enable_gpu().unwrap();
            }
            c.set_internal_resolution(2);
            c.gizmondo_goforce_20051013 = true;
            c.state.intel2700g_depth_bias = true;
            let mut mem = FakeMem {
                base: BASE,
                bytes: quad_floats(),
            };
            for rgba in [[255, 0, 0, 255], [0, 0, 255, 255]] {
                c.bind_texture(GL_TEXTURE_2D, 1);
                c.tex_image_2d(GL_TEXTURE_2D, 0, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, &rgba);
                c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
                c.set_capability(GL_TEXTURE_2D, true);
                c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
                c.set_client_state(GL_VERTEX_ARRAY, true);
                c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
                c.sync_gpu();
                assert_eq!(
                    &c.target.color[(4 * 8 + 4) * 4..(4 * 8 + 4) * 4 + 4],
                    &rgba,
                    "ES={es}"
                );
                c.current_color = [0.; 4];
                c.set_error(GL_INVALID_ENUM);
                c.reset_client_state();
                assert_eq!(&c.target.color[(4 * 8 + 4) * 4..(4 * 8 + 4) * 4 + 4], &rgba);
                assert!(c.textures.is_empty());
                assert!(c.buffers.is_empty());
                assert_eq!(c.current_color, [1.; 4]);
                assert!(!c.vertex_array.enabled);
                assert_eq!(c.take_error(), GL_NO_ERROR);
                assert!(c.target.rgb565);
                assert!(c.gpu_active());
                assert_eq!(c.internal_resolution, 2);
                assert!(c.gizmondo_goforce_20051013);
                assert!(c.state.intel2700g_depth_bias);
            }
        }
    }

    #[test]
    #[ignore = "requires native OpenGL/GLES; native pixmap interop"]
    fn native_rgb565_upload_preserves_depth_odd_rows_and_internal_scale() {
        for es in [false, true] {
            let mut c = Context::new_rgb565(3, 2);
            let mut renderer = if es {
                crate::gpu::Renderer::new_es_for_test(3, 2).unwrap()
            } else {
                crate::gpu::Renderer::new(3, 2).unwrap()
            };
            renderer.set_surface_rgb565(true).unwrap();
            c.gpu = Some(renderer);
            c.gpu_requested = true;
            c.set_internal_resolution(2);
            c.clear_depth = 0.25;
            c.clear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);
            let values = [0xf800u16, 0x07e0, 0x001f, 0xffff, 0, 0xffe0];
            let pixels: Vec<_> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
            assert!(c.write_rgb565(&pixels));
            c.sync_gpu();
            let before = c.target.color.clone();
            let mut presented = vec![0; 12];
            c.present_rgb565(&mut presented);
            assert_eq!(presented, pixels, "ES={es}");
            c.set_capability(GL_DEPTH_TEST, true);
            c.current_color = [1., 0., 1., 1.];
            c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
            c.set_client_state(GL_VERTEX_ARRAY, true);
            let mut mem = FakeMem {
                base: BASE,
                bytes: quad_floats(),
            };
            c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
            c.sync_gpu();
            assert_eq!(
                c.target.color, before,
                "depth attachment changed on ES={es}"
            );
            assert_eq!(c.scaled_rgb565_into(&mut Vec::new()), Some((6, 4)));
        }
    }

    #[test]
    fn mipmapped_triangle_rendering_refreshes_cache_on_level_updates() {
        let mut c = Context::new(8, 8);
        let mut m = FakeMem {
            base: BASE,
            bytes: quad_floats(),
        };
        c.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: BASE,
            buffer: 0,
        };
        let uv = [[0., 0.], [4., 0.], [4., 4.], [0., 4.]];
        let uv_bytes: Vec<u8> = uv
            .iter()
            .flatten()
            .flat_map(|v: &f32| v.to_le_bytes())
            .collect();
        let uv_ptr = BASE + m.bytes.len() as u32;
        m.bytes.extend(uv_bytes);
        c.texture_units[0].texcoord_array = ArrayPointer {
            enabled: true,
            size: 2,
            ty: GL_FLOAT,
            stride: 0,
            pointer: uv_ptr,
            buffer: 0,
        };
        c.texture_units[0].texture_enabled = true;
        c.texture_units[0].bound_texture = 1;
        let mut t = Texture::default();
        t.set_level(0, 4, 4, [255, 0, 0, 255].repeat(16));
        t.set_level(1, 2, 2, [0, 255, 0, 255].repeat(4));
        t.set_level(2, 1, 1, vec![0, 0, 255, 255]);
        t.min_filter = texture::Filter::LinearMipmapLinear;
        c.bind_texture(GL_TEXTURE_2D, 1);
        c.textures.insert(1, t);
        c.draw_arrays(&mut m, GL_TRIANGLE_FAN, 0, 4);
        assert_eq!(
            &c.target.color[(4 * 8 + 4) * 4..(4 * 8 + 4) * 4 + 4],
            &[0, 255, 0, 255]
        );
        c.textures
            .get_mut(&1)
            .unwrap()
            .set_level(1, 2, 2, [255, 255, 0, 255].repeat(4));
        c.draw_arrays(&mut m, GL_TRIANGLE_FAN, 0, 4);
        assert_eq!(
            &c.target.color[(4 * 8 + 4) * 4..(4 * 8 + 4) * 4 + 4],
            &[255, 255, 0, 255]
        );
        c.delete_textures(&[1]);
        assert!(!c.software_mips.contains_key(&1));
    }

    #[test]
    fn flat_stream_uses_fresh_host_snapshots_and_sparse_indices_fall_back() {
        let mut c = Context::new(8, 8);
        let mut bytes = Vec::new();
        for i in 0..4 {
            for v in [i as f32, 0., 0.] {
                bytes.extend(v.to_le_bytes());
            }
        }
        let colors = bytes.len();
        for i in 0..4 {
            bytes.extend([i, 10, 20, 255]);
        }
        c.prepared_arrays.push((BASE, bytes));
        c.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: BASE,
            buffer: 0,
        };
        c.color_array = ArrayPointer {
            enabled: true,
            size: 4,
            ty: GL_UNSIGNED_BYTE,
            stride: 0,
            pointer: BASE + colors as u32,
            buffer: 0,
        };
        let indices = [2, 0, 3, 3, 0, 1];
        let mut source = crate::gpu::VertexStream::default();
        let mut out = crate::gpu::VertexStream::default();
        assert!(c.prepare_vertex_stream(&mut source, indices.into_iter(), 6, false, &[], false));
        assert!(source.expand_flat_triangles(&mut out));
        let offset = out.attributes[1].unwrap().offset;
        assert_eq!(out.bytes[offset], 3);
        c.prepared_arrays[0].1[colors + 3 * 4] = 42;
        assert!(c.prepare_vertex_stream(&mut source, indices.into_iter(), 6, false, &[], false));
        assert!(source.expand_flat_triangles(&mut out));
        assert_eq!(out.bytes[offset], 42);
        assert!(!c.prepare_vertex_stream(&mut source, [0, 0, 3].into_iter(), 3, false, &[], false));
        assert!(!source.flat_triangles);
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native GL/GLES2; indexed fallback storage and mutable guest data"]
    fn gpu_indexed_fallback_reuses_scratch_and_refreshes_guest_data() {
        for es in [false, true] {
            let mut c = native_ctx(es);
            let mut mem = CountMem {
                inner: FakeMem {
                    base: BASE,
                    bytes: quad_floats(),
                },
                reads: 0,
                reject_bulk: true,
            };
            c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
            c.set_client_state(GL_VERTEX_ARRAY, true);
            let indices = [0, 1, 2, 0, 2, 3];
            c.current_color = [1., 0., 0., 1.];
            c.draw_elements(&mut mem, GL_TRIANGLES, &indices);
            c.sync_gpu();
            assert_eq!(pixel(&c, 32, 32), [255, 0, 0, 255]);
            let storage = (
                c.assembled_vertices.as_ptr(),
                c.dense_indices.as_ptr(),
                c.expanded_indices.as_ptr(),
                c.vertex_remap.capacity(),
            );
            assert!(c.assembled_vertices.capacity() >= 4);
            assert!(c.dense_indices.capacity() >= 6);
            c.clear_color = [0., 0., 0., 1.];
            c.clear(GL_COLOR_BUFFER_BIT);
            mem.inner.bytes.fill(0);
            c.draw_elements(&mut mem, GL_TRIANGLES, &indices);
            c.sync_gpu();
            assert_eq!(pixel(&c, 32, 32), [0, 0, 0, 255]);
            assert_eq!(
                storage,
                (
                    c.assembled_vertices.as_ptr(),
                    c.dense_indices.as_ptr(),
                    c.expanded_indices.as_ptr(),
                    c.vertex_remap.capacity()
                )
            );
            assert!(c.assembled_vertices.is_empty());
            assert!(c.vertex_remap.is_empty());
            assert_eq!(c.take_error(), GL_NO_ERROR);
        }
    }

    #[test]
    fn fallback_scratch_reuses_storage_but_refreshes_guest_vertices() {
        let mut c = ctx();
        let original = quad_floats();
        let mut mem = FakeMem {
            base: BASE,
            bytes: original.clone(),
        };
        c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
        let ptr = c.assembled_vertices.as_ptr();
        let capacity = c.assembled_vertices.capacity();
        assert!(capacity >= 4);
        assert!(c.assembled_vertices.is_empty());
        c.target.color.fill(0);
        // All vertices at the same point must not reuse the previous quad.
        mem.bytes.fill(0);
        c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
        assert_eq!(covered(&c), 0);
        assert_eq!(c.assembled_vertices.as_ptr(), ptr);
        assert_eq!(c.assembled_vertices.capacity(), capacity);
        // Invalid memory and then recovery must also recycle on the early exit.
        mem.bytes.clear();
        c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
        assert_eq!(c.assembled_vertices.as_ptr(), ptr);
        c.take_error();
        mem.bytes = original;
        c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
        assert!(covered(&c) > 3000);
        assert_eq!(c.assembled_vertices.as_ptr(), ptr);
    }

    #[test]
    fn draw_arrays_renders_float_positions() {
        let mut c = ctx();
        let mut m = FakeMem {
            base: BASE,
            bytes: quad_floats(),
        };
        c.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: BASE,
            buffer: 0,
        };
        c.current_color = [1.0, 0.0, 0.0, 1.0];
        c.draw_arrays(&mut m, GL_TRIANGLE_FAN, 0, 4);
        assert!(covered(&c) > 3000, "covered only {}", covered(&c));
        assert_eq!(pixel(&c, 32, 32), [255, 0, 0, 255]);
    }

    #[test]
    fn fixed_point_positions_render_identically_to_float() {
        // This is the whole reason the crate carries a fixed-point
        // module: Call of Duty 2 feeds GL_FIXED vertex arrays, which
        // desktop GL 2.1 cannot consume at all.
        let mut a = ctx();
        let mut ma = FakeMem {
            base: BASE,
            bytes: quad_floats(),
        };
        a.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: BASE,
            buffer: 0,
        };
        a.draw_arrays(&mut ma, GL_TRIANGLE_FAN, 0, 4);

        let mut b = ctx();
        let mut mb = FakeMem {
            base: BASE,
            bytes: fixeds(&[
                -1.0, -1.0, 0.0, //
                1.0, -1.0, 0.0, //
                1.0, 1.0, 0.0, //
                -1.0, 1.0, 0.0,
            ]),
        };
        b.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FIXED,
            stride: 0,
            pointer: BASE,
            buffer: 0,
        };
        b.draw_arrays(&mut mb, GL_TRIANGLE_FAN, 0, 4);

        assert_eq!(a.target.color, b.target.color);
        assert!(covered(&a) > 3000);
    }

    #[test]
    fn interleaved_stride_skips_padding() {
        // Position at offset 0 of a 20-byte struct, the classic
        // interleaved layout. A stride bug renders scrambled geometry.
        let mut bytes = Vec::new();
        for pos in [
            [-1.0f32, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        ] {
            bytes.extend(floats(&pos));
            bytes.extend([0u8; 8]); // padding: uv, colour, whatever
        }
        let mut c = ctx();
        let mut m = FakeMem { base: BASE, bytes };
        c.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 20,
            pointer: BASE,
            buffer: 0,
        };
        c.draw_arrays(&mut m, GL_TRIANGLE_FAN, 0, 4);
        assert!(covered(&c) > 3000, "stride handling dropped geometry");
    }

    #[test]
    fn unmapped_array_pointer_aborts_the_draw() {
        let mut c = ctx();
        let mut m = FakeMem {
            base: BASE,
            bytes: vec![0u8; 8],
        };
        c.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            // Far outside the fake mapping.
            pointer: 0xDEAD_0000,
            buffer: 0,
        };
        c.draw_arrays(&mut m, GL_TRIANGLE_FAN, 0, 4);
        assert_eq!(covered(&c), 0);
    }

    #[test]
    fn disabled_vertex_array_draws_nothing() {
        let mut c = ctx();
        let mut m = FakeMem {
            base: BASE,
            bytes: quad_floats(),
        };
        c.vertex_array = ArrayPointer {
            enabled: false,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: BASE,
            buffer: 0,
        };
        c.draw_arrays(&mut m, GL_TRIANGLE_FAN, 0, 4);
        assert_eq!(covered(&c), 0);
    }

    #[test]
    fn draw_elements_reads_ushort_indices_from_guest_memory() {
        let mut bytes = quad_floats();
        let index_off = bytes.len() as u32;
        for i in [0u16, 1, 2, 0, 2, 3] {
            bytes.extend(i.to_le_bytes());
        }
        let mut c = ctx();
        let mut m = FakeMem { base: BASE, bytes };
        c.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: BASE,
            buffer: 0,
        };
        c.draw_elements_from_guest(&mut m, GL_TRIANGLES, 6, GL_UNSIGNED_SHORT, BASE + index_off);
        assert!(covered(&c) > 3000, "indexed quad covered {}", covered(&c));
    }

    #[test]
    fn draw_elements_reads_ubyte_indices() {
        let mut bytes = quad_floats();
        let index_off = bytes.len() as u32;
        bytes.extend([0u8, 1, 2, 0, 2, 3]);
        let mut c = ctx();
        let mut m = FakeMem { base: BASE, bytes };
        c.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: BASE,
            buffer: 0,
        };
        c.draw_elements_from_guest(&mut m, GL_TRIANGLES, 6, GL_UNSIGNED_BYTE, BASE + index_off);
        assert!(covered(&c) > 3000);
    }

    #[test]
    fn color_array_bytes_are_normalized() {
        // A UNSIGNED_BYTE colour array holds 0..255 meaning 0.0..1.0.
        // Treating it as a raw value would saturate every vertex white.
        let mut bytes = quad_floats();
        let color_off = bytes.len() as u32;
        for _ in 0..4 {
            bytes.extend([0u8, 0, 255, 255]); // pure blue
        }
        let mut c = ctx();
        let mut m = FakeMem { base: BASE, bytes };
        c.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: BASE,
            buffer: 0,
        };
        c.color_array = ArrayPointer {
            enabled: true,
            size: 4,
            ty: GL_UNSIGNED_BYTE,
            stride: 0,
            pointer: BASE + color_off,
            buffer: 0,
        };
        c.draw_arrays(&mut m, GL_TRIANGLE_FAN, 0, 4);
        assert_eq!(pixel(&c, 32, 32), [0, 0, 255, 255]);
    }

    fn six_texture_cycle(c: &mut Context) {
        let mut bytes = quad_floats();
        let uv_off = bytes.len() as u32;
        bytes.extend(floats(&[0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]));

        let mut m = FakeMem { base: BASE, bytes };
        for name in 0..6 {
            c.bind_texture(GL_TEXTURE_2D, name);
            c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
            c.compressed_tex_image_2d(
                GL_TEXTURE_2D,
                0,
                4,
                4,
                GL_COMPRESSED_RGB_S3TC_DXT1_EXT,
                &[0xe0, 0x07, 0, 0, 0, 0, 0, 0],
            );
            assert_eq!(c.take_error(), GL_NO_ERROR);
        }
        c.set_capability(GL_TEXTURE_2D, true);
        c.texture_units[0].tex_env = TexEnvMode::Replace;
        c.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: BASE,
            buffer: 0,
        };
        c.texture_units[0].texcoord_array = ArrayPointer {
            enabled: true,
            size: 2,
            ty: GL_FLOAT,
            stride: 0,
            pointer: BASE + uv_off,
            buffer: 0,
        };
        for name in (0..6).cycle().take(18) {
            c.bind_texture(GL_TEXTURE_2D, name);
            c.delete_textures(&[0]);
            c.draw_arrays(&mut m, GL_TRIANGLE_FAN, 0, 4);
            c.sync_gpu();
            assert_eq!(pixel(&c, 32, 32), [0, 255, 0, 255], "name={name}");
            assert_eq!(c.take_error(), GL_NO_ERROR);
        }
    }

    #[test]
    fn default_texture_compressed_cycle_software() {
        six_texture_cycle(&mut ctx());
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires a native GL driver"]
    fn default_texture_compressed_cycle_native() {
        for es in [false, true] {
            six_texture_cycle(&mut native_ctx(es));
        }
    }

    #[test]
    fn texture_coords_sample_the_bound_texture() {
        let mut bytes = quad_floats();
        let uv_off = bytes.len() as u32;
        bytes.extend(floats(&[0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]));

        let mut c = ctx();
        let mut m = FakeMem { base: BASE, bytes };
        let names = c.gen_textures(1);
        c.bind_texture(GL_TEXTURE_2D, names[0]);
        // 1x1 green texture.
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            1,
            1,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[0, 255, 0, 255],
        );
        c.set_capability(GL_TEXTURE_2D, true);
        c.texture_units[0].tex_env = TexEnvMode::Replace;
        c.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: BASE,
            buffer: 0,
        };
        c.texture_units[0].texcoord_array = ArrayPointer {
            enabled: true,
            size: 2,
            ty: GL_FLOAT,
            stride: 0,
            pointer: BASE + uv_off,
            buffer: 0,
        };
        c.draw_arrays(&mut m, GL_TRIANGLE_FAN, 0, 4);
        assert_eq!(pixel(&c, 32, 32), [0, 255, 0, 255]);
    }

    fn combiner_discard_preserves_color_and_depth(gpu: bool) {
        use crate::combiner_program::{Architecture, Program, Stage};
        let mut c = ctx();
        if gpu {
            c.enable_gpu().unwrap();
        }
        c.clear_color = [0., 0., 1., 1.];
        c.clear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);
        c.set_capability(GL_DEPTH_TEST, true);
        let mut mem = FakeMem {
            base: BASE,
            bytes: quad_floats(),
        };
        c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        c.current_color = [1., 0., 0., 0.];
        let program = Program {
            architecture: Architecture::Ar11,
            flags: 0,
            stages: vec![Stage {
                alpha_output: 0x70,
                alpha_inputs: 0x8304_0004,
                flags: 0x68,
                color_output: 0,
                color_input_modifiers: 0,
                color_inputs: 0,
                color_input_modes: 0,
                texture_routing: 0xffff_ffff,
            }],
        }
        .compile_basic()
        .unwrap();
        c.combiner_programs
            .load_bound(program, MAX_TEXTURE_UNITS)
            .unwrap();
        c.combiner_programs.set_enabled(true);
        c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
        c.sync_gpu();
        assert_eq!(pixel(&c, 32, 32), [0, 0, 255, 255]);
        c.combiner_programs.set_enabled(false);
        c.current_color = [0., 1., 0., 1.];
        // Farther geometry remains visible: the rejected fragment wrote no Z.
        for vertex in mem.bytes.chunks_exact_mut(12) {
            vertex[8..12].copy_from_slice(&0.5f32.to_le_bytes());
        }
        c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
        c.sync_gpu();
        assert_eq!(pixel(&c, 32, 32), [0, 255, 0, 255]);
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }

    #[test]
    fn software_combiner_discard_preserves_color_and_depth() {
        combiner_discard_preserves_color_and_depth(false);
    }

    #[test]
    #[ignore = "requires native desktop GL and GLES"]
    fn gpu_combiner_discard_preserves_color_and_depth() {
        for es in [false, true] {
            let root = if es {
                crate::gpu::Renderer::new_es_for_test(8, 8).unwrap()
            } else {
                crate::gpu::Renderer::new(8, 8).unwrap()
            };
            crate::gpu::with_shared_context_source(Some(root.shared_context_source()), || {
                combiner_discard_preserves_color_and_depth(true);
            });
        }
    }

    fn feedback_alpha_case(gpu: bool, write_alpha: bool) -> [u8; 4] {
        use crate::combiner_program::{Architecture, Program, Stage};
        let mut c = ctx();
        if gpu {
            c.enable_gpu().unwrap();
        }
        c.clear_color = [0., 0., 0., 0.5];
        c.clear(GL_COLOR_BUFFER_BIT);
        c.state.color_mask = [true, true, true, write_alpha];
        c.set_capability(GL_BLEND, true);
        c.set_blend_func(GL_ONE, GL_ONE);
        let positions = [[-1f32, -1., 0.], [1., -1., 0.], [0., 1., 0.]];
        let bytes = positions
            .into_iter()
            .cycle()
            .take(6)
            .flatten()
            .flat_map(f32::to_le_bytes)
            .collect();
        let mut mem = FakeMem { base: BASE, bytes };
        c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        let p = Program {
            architecture: Architecture::Ar11,
            flags: 0,
            stages: vec![Stage {
                alpha_output: 0x0670,
                alpha_inputs: 0xc340_0000,
                flags: 0xec,
                color_output: 0x8070,
                color_input_modifiers: 0,
                color_inputs: 0xff40_0000,
                color_input_modes: 0x8800,
                texture_routing: 0x7fff_ffff,
            }],
        }
        .compile_basic()
        .unwrap();
        assert_eq!(
            p.framebuffer_needs_primitive_order([true, true, true, write_alpha]),
            write_alpha
        );
        c.combiner_programs.set_constant(0, [16384; 4]);
        c.combiner_programs
            .load_bound(p, MAX_TEXTURE_UNITS)
            .unwrap();
        c.combiner_programs.set_enabled(true);
        c.draw_arrays(&mut mem, GL_TRIANGLES, 0, 6);
        c.sync_gpu();
        assert_eq!(c.take_error(), GL_NO_ERROR);
        let first = pixel(&c, 32, 32);
        if !write_alpha && gpu {
            assert!(c.vertex_stream.count > 0);
        }
        first
    }
    #[test]
    fn software_feedback_distinguishes_alpha_reads_from_alpha_writes() {
        assert_eq!(feedback_alpha_case(false, false), [64, 64, 64, 128]);
        assert_eq!(feedback_alpha_case(false, true), [72, 72, 72, 200]);
    }
    #[test]
    #[ignore = "requires native desktop GL and GLES"]
    fn gpu_feedback_stable_alpha_batches_without_losing_overlapping_order() {
        for es in [false, true] {
            let root = if es {
                crate::gpu::Renderer::new_es_for_test(8, 8).unwrap()
            } else {
                crate::gpu::Renderer::new(8, 8).unwrap()
            };
            crate::gpu::with_shared_context_source(Some(root.shared_context_source()), || {
                for alpha in [false, true] {
                    let expected = feedback_alpha_case(false, alpha);
                    let actual = feedback_alpha_case(true, alpha);
                    assert!(
                        actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1),
                        "write_alpha={alpha}: {actual:?} vs {expected:?}"
                    );
                }
            });
        }
    }

    #[test]
    #[ignore = "requires native desktop GL and GLES; sparse high NV sampler"]
    fn gpu_nv_sparse_sampler_four_keeps_coordinate_one_and_fixed_function_transitions() {
        use crate::combiner_program::{Architecture, Program, Stage};
        for es in [false, true] {
            let root = if es {
                crate::gpu::Renderer::new_es_for_test(8, 8).unwrap()
            } else {
                crate::gpu::Renderer::new(8, 8).unwrap()
            };
            crate::gpu::with_shared_context_source(Some(root.shared_context_source()), || {
                let mut c = ctx();
                c.nv_combiner_client = true;
                c.enable_gpu().unwrap();
                let mut mem = FakeMem {
                    base: BASE,
                    bytes: quad_floats(),
                };
                c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
                c.set_client_state(GL_VERTEX_ARRAY, true);
                c.set_active_texture(1);
                c.bind_texture(GL_TEXTURE_2D, 2);
                c.tex_image_2d(
                    GL_TEXTURE_2D,
                    0,
                    1,
                    1,
                    GL_RGBA,
                    GL_UNSIGNED_BYTE,
                    &[0, 0, 255, 255],
                );
                c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
                c.set_capability(GL_TEXTURE_2D, true);
                c.tex_env(GL_TEXTURE_ENV_MODE, GL_REPLACE);
                c.set_multi_texcoord4(GL_TEXTURE0 + 1, [0.75, 0., 0., 1.]);
                c.set_active_texture(4);
                c.bind_texture(GL_TEXTURE_2D, 1);
                c.tex_image_2d(
                    GL_TEXTURE_2D,
                    0,
                    2,
                    1,
                    GL_RGBA,
                    GL_UNSIGNED_BYTE,
                    &[255, 0, 0, 255, 0, 255, 0, 255],
                );
                c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
                c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
                c.set_capability(GL_TEXTURE_2D, true);
                c.tex_env(GL_TEXTURE_ENV_MODE, GL_MODULATE);
                let p = Program {
                    architecture: Architecture::Ar11,
                    flags: 0,
                    stages: vec![Stage {
                        alpha_output: 0x0670,
                        alpha_inputs: 0xc304_0000,
                        flags: 0xe8,
                        color_output: 0x8070,
                        color_input_modifiers: 0x0400,
                        color_inputs: 0xe408_0000,
                        color_input_modes: 0x8100,
                        texture_routing: 0x3fff_fc21,
                    }],
                }
                .compile_basic()
                .unwrap();
                assert_eq!(p.texture_units(), 1 << 4);
                assert_eq!(p.coordinate_units(), 1 << 1);
                c.combiner_programs.load_bound(p, 6).unwrap();
                for nv in [true, false, true] {
                    c.combiner_programs.set_enabled(nv);
                    c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
                    c.sync_gpu();
                    assert_eq!(
                        pixel(&c, 32, 32),
                        if nv { [0, 255, 0, 255] } else { [0, 0, 0, 255] },
                        "es={es} nv={nv}"
                    );
                    assert_eq!(c.take_error(), GL_NO_ERROR);
                }
                // An incomplete high sampler is absent from enabled stages,
                // but must still replace the prior binding with its fallback.
                c.set_active_texture(4);
                c.bind_texture(GL_TEXTURE_2D, 99);
                c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
                c.sync_gpu();
                assert_eq!(pixel(&c, 32, 32), [255, 255, 255, 255]);
                assert_eq!(c.take_error(), GL_NO_ERROR);
            });
        }
    }

    fn literal_normalization(gpu: bool) {
        use crate::combiner_program::{Architecture, Program, Stage};
        let mut c = ctx();
        if gpu {
            c.enable_gpu().unwrap();
        }
        let mut mem = FakeMem {
            base: BASE,
            bytes: quad_floats(),
        };
        c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        c.combiner_programs.set_constant(0, [8192; 4]);
        for code in 0..32u16 {
            let packed = code | code << 5 | code << 10;
            let literal = Stage {
                alpha_output: 0x0670,
                alpha_inputs: u32::from(code) << 24 | 4 << 16,
                flags: 0xe0,
                color_output: 0x8070,
                color_input_modifiers: (packed & 15) << 12 | 0x0400,
                color_inputs: u32::from((packed >> 4) & 255) << 24 | 0x0008_0000,
                color_input_modes: ((packed >> 12) & 7) << 12 | 0x0100,
                texture_routing: 0xffff_ffff,
            };
            // Map the complete signed literal range into [0,1], retaining its
            // distinction after framebuffer clamping: output = literal/8+1/2.
            let normalize = Stage {
                alpha_output: 0x0670,
                alpha_inputs: 0x0040_0204,
                flags: 0xec,
                color_output: 0x8070,
                color_input_modifiers: 0x0024,
                color_inputs: 0x0040_8408,
                color_input_modes: 0x8801,
                texture_routing: 0xffff_ffff,
            };
            let program = Program {
                architecture: Architecture::Ar11,
                flags: 0,
                stages: vec![literal, normalize],
            }
            .compile_basic()
            .unwrap();
            c.combiner_programs
                .load_bound(program, MAX_TEXTURE_UNITS)
                .unwrap();
            c.combiner_programs.set_enabled(true);
            c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
            c.sync_gpu();
            let value = (code & 15) as f32 * 0.25 * if code & 16 == 0 { 1. } else { -1. };
            let expected = ((value * 0.125 + 0.5) * 255.).round() as u8;
            let pixel = pixel(&c, 32, 32);
            assert!(
                pixel.iter().all(|&v| v.abs_diff(expected) <= 1),
                "code={code}: {pixel:?}, expected={expected}"
            );
            assert_eq!(c.take_error(), GL_NO_ERROR);
        }
    }

    #[test]
    fn software_ar11_literals_preserve_sign_and_fraction_through_arithmetic() {
        literal_normalization(false);
    }

    #[test]
    #[ignore = "requires native desktop GL and GLES"]
    fn gpu_ar11_literals_preserve_sign_and_fraction_through_arithmetic() {
        for es in [false, true] {
            let root = if es {
                crate::gpu::Renderer::new_es_for_test(8, 8).unwrap()
            } else {
                crate::gpu::Renderer::new(8, 8).unwrap()
            };
            crate::gpu::with_shared_context_source(Some(root.shared_context_source()), || {
                literal_normalization(true);
            });
        }
    }

    fn fixed_sixth_texture_stage(gpu: bool) {
        let mut c = ctx();
        c.nv_combiner_client = true;
        if gpu {
            c.enable_gpu().unwrap();
        }
        let mut mem = FakeMem {
            base: BASE,
            bytes: quad_floats(),
        };
        c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        c.set_active_texture(5);
        c.bind_texture(GL_TEXTURE_2D, 1);
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            1,
            1,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[0, 255, 0, 255],
        );
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
        c.set_capability(GL_TEXTURE_2D, true);
        c.tex_env(GL_TEXTURE_ENV_MODE, GL_REPLACE);
        // Populate the NV cache before the wide fixed-function cache. Program
        // ID zero is a real NV identity and must not alias the fixed variant.
        use crate::combiner_program::{Architecture, Program, Stage};
        let blue = Program {
            architecture: Architecture::Ar11,
            flags: 0,
            stages: vec![Stage {
                alpha_output: 0x0670,
                alpha_inputs: 0x4004_0000,
                flags: 0xe8,
                color_output: 0x8070,
                color_input_modifiers: 0x0400,
                color_inputs: 0x4008_0000,
                color_input_modes: 0x8100,
                texture_routing: 0xffff_ffff,
            }],
        }
        .compile_basic()
        .unwrap();
        c.combiner_programs.set_constant(0, [0, 0, 65536, 65536]);
        c.combiner_programs
            .load_bound(blue, MAX_TEXTURE_UNITS)
            .unwrap();
        c.combiner_programs.set_enabled(true);
        c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
        c.sync_gpu();
        assert_eq!(pixel(&c, 32, 32), [0, 0, 255, 255]);
        c.combiner_programs.set_enabled(false);
        for bounded in [false, true] {
            if bounded {
                c.tex_parameter_float(GL_TEXTURE_2D, GL_TEXTURE_MAX_LOD_SGIS, 0.);
            }
            c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
            c.sync_gpu();
            assert_eq!(pixel(&c, 32, 32), [0, 255, 0, 255]);
            assert_eq!(c.take_error(), GL_NO_ERROR);
        }
        c.set_capability(GL_TEXTURE_2D, false);
        c.current_color = [1., 0., 0., 1.];
        c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
        c.sync_gpu();
        assert_eq!(pixel(&c, 32, 32), [255, 0, 0, 255]);
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }
    #[test]
    fn software_nv_coordinate_capacity_preserves_regular_limits() {
        let mut c = ctx();
        c.set_active_texture(4);
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
        assert_eq!(c.active_texture, 0);
        c.nv_combiner_client = true;
        c.set_active_texture(5);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        c.set_client_active_texture(5);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        c.set_multi_texcoord4(GL_TEXTURE0 + 5, [0.25, 0.5, 0., 1.]);
        assert_eq!(c.es11.current_texcoords[5], [0.25, 0.5, 0., 1.]);
        c.set_active_texture(6);
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
        assert_eq!(c.active_texture, 5);
        fixed_sixth_texture_stage(false);
    }
    #[test]
    #[ignore = "requires native desktop GL and GLES"]
    fn gpu_nv_sixth_stage_and_return_to_four_stage_variants() {
        for es in [false, true] {
            let root = if es {
                crate::gpu::Renderer::new_es_for_test(8, 8).unwrap()
            } else {
                crate::gpu::Renderer::new(8, 8).unwrap()
            };
            crate::gpu::with_shared_context_source(Some(root.shared_context_source()), || {
                fixed_sixth_texture_stage(true)
            });
        }
    }

    fn swizzled_combiner_coordinates(gpu: bool) {
        use crate::combiner_program::{Architecture, Program, Stage};
        let mut c = ctx();
        c.nv_combiner_client = true;
        if gpu {
            c.enable_gpu().unwrap();
            assert_eq!(c.nv_texture_coordinate_capacity(), 6);
        }
        let mut mem = FakeMem {
            base: BASE,
            bytes: quad_floats(),
        };
        c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        c.bind_texture(GL_TEXTURE_2D, 1);
        // Top-right is white; all other quadrants have different colours.
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            2,
            2,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ],
        );
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
        for (s, t) in [(10u32, 1u32), (5, 12)] {
            let si = (s & 7) as usize;
            let ti = (t & 7) as usize;
            c.texture_units[si].current_texcoord = [0.25, 0.25, 0., 1.];
            c.texture_units[ti].current_texcoord = [0.25, 0.25, 0., 1.];
            c.texture_matrix[si].load(matrix::translate(
                if s < 8 { 0.5 } else { 0. },
                if s >= 8 { 0.5 } else { 0. },
                0.,
            ));
            c.texture_matrix[ti].load(matrix::translate(
                if t < 8 { 0.5 } else { 0. },
                if t >= 8 { 0.5 } else { 0. },
                0.,
            ));
            let program = Program {
                architecture: Architecture::Ar11,
                flags: 0,
                stages: vec![Stage {
                    alpha_output: 0x0670,
                    alpha_inputs: 0xc304_0000,
                    flags: 0xc8,
                    color_output: 0x8070,
                    color_input_modifiers: 0x0400,
                    color_inputs: 0xe408_0000,
                    color_input_modes: 0x8100,
                    texture_routing: 0x3fff_fc00 | s << 5 | t,
                }],
            }
            .compile_basic()
            .unwrap();
            assert!(program.supports_texture_units(6));
            if si >= 4 || ti >= 4 {
                assert!(!program.supports_texture_units(4));
            }
            c.combiner_programs
                .load_bound(program, MAX_TEXTURE_UNITS)
                .unwrap();
            c.combiner_programs.set_enabled(true);
            for arrays in [false, true] {
                if arrays {
                    for unit in [si, ti] {
                        let pointer = BASE + mem.bytes.len() as u32;
                        for _ in 0..4 {
                            mem.bytes.extend(
                                [0.25f32, 0.25, 0., 2.]
                                    .into_iter()
                                    .flat_map(f32::to_le_bytes),
                            );
                        }
                        c.set_client_active_texture(unit as u32);
                        c.set_texcoord_pointer(4, GL_FLOAT, 16, pointer);
                        c.set_client_state(GL_TEXTURE_COORD_ARRAY, true);
                    }
                }
                for pretransform in [false, true] {
                    c.es11.pretransform = pretransform;
                    c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
                    c.sync_gpu();
                    assert_eq!(
                        pixel(&c, 32, 32),
                        [255; 4],
                        "s={s} t={t} arrays={arrays} pretransform={pretransform}"
                    );
                    assert_eq!(c.take_error(), GL_NO_ERROR);
                }
            }
            // The ordinary four-coordinate shader must still work afterwards.
            c.combiner_programs.set_enabled(false);
            c.current_color = [0., 1., 0., 1.];
            c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
            c.sync_gpu();
            assert_eq!(pixel(&c, 32, 32), [0, 255, 0, 255]);
        }
    }

    #[test]
    fn software_combiner_swizzles_axes_and_uses_six_coordinate_sets() {
        swizzled_combiner_coordinates(false);
    }

    #[test]
    #[ignore = "requires native desktop GL and GLES"]
    fn gpu_combiner_swizzles_axes_and_uses_six_coordinate_sets() {
        for es in [false, true] {
            let root = if es {
                crate::gpu::Renderer::new_es_for_test(8, 8).unwrap()
            } else {
                crate::gpu::Renderer::new(8, 8).unwrap()
            };
            crate::gpu::with_shared_context_source(Some(root.shared_context_source()), || {
                swizzled_combiner_coordinates(true);
            });
        }
    }

    #[test]
    fn software_combiner_samples_coordinates_from_the_program_route() {
        combiner_samples_coordinates_from_the_program_route(false);
    }

    #[test]
    #[ignore = "requires native OpenGL"]
    fn gpu_combiner_samples_coordinates_from_the_program_route() {
        for es in [false, true] {
            let root = if es {
                crate::gpu::Renderer::new_es_for_test(8, 8).unwrap()
            } else {
                crate::gpu::Renderer::new(8, 8).unwrap()
            };
            crate::gpu::with_shared_context_source(Some(root.shared_context_source()), || {
                combiner_samples_coordinates_from_the_program_route(true);
            });
        }
    }

    fn combiner_samples_coordinates_from_the_program_route(gpu: bool) {
        let mut c = ctx();
        if gpu {
            c.enable_gpu().unwrap();
        }
        let mut mem = FakeMem {
            base: BASE,
            bytes: quad_floats(),
        };
        c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        c.bind_texture(GL_TEXTURE_2D, 1);
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            2,
            1,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[255, 0, 0, 255, 32, 160, 64, 255],
        );
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
        c.texture_units[1].current_texcoord = [0.75, 0.0, 0.0, 1.0];
        c.current_color = [0.5, 0.5, 1.0, 1.0];

        // One stage multiplies the texture sample by the primary RGBA.
        let program = crate::combiner_program::Program::decode(&[
            b'A', b'R', b'1', b'0', 1, 0x70, 0x06, 0, 0, 0x83, 0xc3, 0xcc, 0x70, 0x80, 0, 0, 0, 0,
            0xa4, 0xe4, 0, 0x88, 0x29, 0x5c, 0x5b, 0x29,
        ])
        .unwrap()
        .compile_basic()
        .unwrap();
        c.combiner_programs
            .load_bound(program, MAX_TEXTURE_UNITS)
            .unwrap();
        c.combiner_programs.set_enabled(true);
        c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
        c.sync_gpu();
        assert_eq!(pixel(&c, 32, 32), [16, 80, 64, 255]);
        if gpu {
            // Force the explicit-LOD shader variant too (GLSL ES 3 on GLES).
            // Restricting these nonmipmapped inputs to level zero must not
            // change their colors or the program's coordinate routing.
            for texture in c.textures.values_mut() {
                texture.max_lod = 0.;
            }
            c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
            c.sync_gpu();
            assert_eq!(pixel(&c, 32, 32), [16, 80, 64, 255]);
        }

        c.combiner_programs.set_enabled(false);
        c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
        c.sync_gpu();
        assert_eq!(pixel(&c, 32, 32), [128, 128, 255, 255]);
    }

    #[test]
    #[ignore = "requires native OpenGL"]
    fn gpu_combiner_reads_overlapping_primitives_in_draw_order() {
        for es in [false, true] {
            let root = if es {
                crate::gpu::Renderer::new_es_for_test(8, 8).unwrap()
            } else {
                crate::gpu::Renderer::new(8, 8).unwrap()
            };
            crate::gpu::with_shared_context_source(Some(root.shared_context_source()), || {
                use crate::combiner_program::{Architecture, Program, Stage};

                let mut c = ctx();
                c.enable_gpu().unwrap();
                c.clear_color = [1., 0.5, 0.25, 1.];
                c.clear(GL_COLOR_BUFFER_BIT);
                let mut mem = FakeMem {
                    base: BASE,
                    bytes: floats(&[-1., -1., 0., 1., -1., 0., 0., 1., 0.].repeat(2)),
                };
                c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
                c.set_client_state(GL_VERTEX_ARRAY, true);
                let stage = Stage {
                    alpha_output: 0x0670,
                    alpha_inputs: 0x0404_0000,
                    flags: 0xe0,
                    color_output: 0x8070,
                    color_input_modifiers: 0x0200,
                    color_inputs: 0xe484_0000,
                    color_input_modes: 0x8000,
                    texture_routing: 0x7fff_ffff,
                };
                let program = Program {
                    architecture: Architecture::Ar11,
                    flags: 0,
                    stages: vec![stage],
                }
                .compile_basic()
                .unwrap();
                let expected = program.evaluate_sampled(
                    [1.; 4],
                    [[0.; 4]; 4],
                    program.evaluate_sampled(
                        [1.; 4],
                        [[0.; 4]; 4],
                        [1., 0.5, 0.25, 1.],
                        |_| unreachable!(),
                    ),
                    |_| unreachable!(),
                );
                c.combiner_programs
                    .load_bound(program, MAX_TEXTURE_UNITS)
                    .unwrap();
                c.combiner_programs.set_enabled(true);
                c.draw_arrays(&mut mem, GL_TRIANGLES, 0, 6);
                c.sync_gpu();
                let actual = pixel(&c, 32, 32);
                for (actual, expected) in actual.into_iter().zip(expected) {
                    assert!(actual.abs_diff((expected * 255.).round() as u8) <= 1);
                }
            });
        }
    }

    #[test]
    fn software_combiner_accumulates_samples_with_independent_s_and_t() {
        use crate::combiner_program::{Architecture, Program, Stage};
        let mut c = ctx();
        let mut mem = FakeMem {
            base: BASE,
            bytes: quad_floats(),
        };
        c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        c.bind_texture(GL_TEXTURE_2D, 1);
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            2,
            2,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[240, 0, 0, 255, 0, 120, 0, 255, 0, 0, 60, 255, 0, 0, 0, 255],
        );
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
        c.texture_units[0].current_texcoord = [0.25, 0.25, 0., 1.];
        c.texture_units[1].current_texcoord = [0.75, 0.75, 0., 1.];

        // Four quarter-weight samples from the same texture. The middle
        // stages choose S and T from different coordinate arrays.
        let stages = [0x08, 0x28, 0x09, 0x29]
            .into_iter()
            .enumerate()
            .map(|(i, route)| Stage {
                alpha_output: 0x0670,
                alpha_inputs: if i == 0 { 0x01c3_0000 } else { 0x01c3_0004 },
                flags: if i == 0 { 0xc4 } else { 0xc6 },
                color_output: 0x8070,
                color_input_modifiers: if i == 0 { 0x1000 } else { 0x1004 },
                color_inputs: if i == 0 { 0x42e4_0000 } else { 0x42e4_0008 },
                color_input_modes: if i == 0 { 0x0800 } else { 0x0881 },
                texture_routing: 0x3fff_fc00 | route,
            })
            .collect();
        let program = Program {
            architecture: Architecture::Ar10,
            flags: 0,
            stages,
        }
        .compile_basic()
        .unwrap();
        c.combiner_programs
            .load_bound(program, MAX_TEXTURE_UNITS)
            .unwrap();
        c.combiner_programs.set_enabled(true);
        c.draw_arrays(&mut mem, GL_TRIANGLE_FAN, 0, 4);
        assert_eq!(pixel(&c, 32, 32), [60, 30, 15, 255]);
    }

    #[test]
    fn texture_bound_to_unit_one_is_sampled_when_unit_zero_is_disabled() {
        // X-Forge (Sticky Balls, Ball Busters) sets every textured draw
        // up as: activeTexture(0) / disable(TEXTURE_2D) / bindTexture(0),
        // then activeTexture(1) / enable(TEXTURE_2D) / bindTexture(tex).
        // Sampling stage 0 — or gating texturing on stage 0's enable —
        // leaves each fragment with its raw vertex colour, which is how
        // the game's white-on-alpha UI atlases came out as solid white
        // rectangles.
        let mut bytes = quad_floats();
        let uv_off = bytes.len() as u32;
        bytes.extend(floats(&[0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]));

        let mut c = ctx();
        let mut m = FakeMem { base: BASE, bytes };
        let names = c.gen_textures(1);

        // A red vertex colour so that sampling the wrong stage, or
        // taking stage 0's combiner mode, is visible in the result:
        // stage 1 replaces, stage 0 would modulate red by blue to black.
        c.current_color = [1.0, 0.0, 0.0, 1.0];

        // Stage 0: explicitly off, with nothing bound.
        c.set_active_texture(0);
        c.set_capability(GL_TEXTURE_2D, false);
        c.bind_texture(GL_TEXTURE_2D, 0);

        // Stage 1: the only stage carrying a texture.
        c.set_active_texture(1);
        c.bind_texture(GL_TEXTURE_2D, names[0]);
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            1,
            1,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[0, 0, 255, 255],
        );
        c.set_capability(GL_TEXTURE_2D, true);
        c.tex_env(GL_TEXTURE_ENV_MODE, GL_REPLACE);

        c.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: BASE,
            buffer: 0,
        };
        c.set_client_active_texture(1);
        c.set_client_state(GL_TEXTURE_COORD_ARRAY, true);
        c.set_texcoord_pointer(2, GL_FLOAT, 0, BASE + uv_off);

        c.draw_arrays(&mut m, GL_TRIANGLE_FAN, 0, 4);
        assert_eq!(pixel(&c, 32, 32), [0, 0, 255, 255]);
    }

    #[test]
    fn resetting_a_stage_past_the_advertised_maximum_keeps_the_live_stage_bound() {
        // Sticky Balls walks one stage further than GL_MAX_TEXTURE_UNITS
        // says exists: it binds its texture to stage 1 and then clears
        // stage 2 before the draw. glActiveTexture past the last stage we
        // track is an error that leaves the selected unit alone, so that
        // clear used to land back on stage 1 and unbind the texture that
        // had just been bound there — which is how the game's sky came
        // out as an opaque white sheet.
        let mut c = ctx();
        let names = c.gen_textures(1);

        c.set_active_texture(1);
        c.set_capability(GL_TEXTURE_2D, true);
        c.bind_texture(GL_TEXTURE_2D, names[0]);

        c.set_active_texture(2);
        c.set_capability(GL_TEXTURE_2D, false);
        c.bind_texture(GL_TEXTURE_2D, 0);

        assert!(c.texture_units[1].texture_enabled, "stage 1 was disabled");
        assert_eq!(c.texture_units[1].bound_texture, names[0]);
    }

    #[test]
    fn a_second_stage_alpha_mask_cuts_out_a_colour_map_that_has_no_alpha() {
        // How Sticky Balls draws a ball: stage 1 carries the colour as
        // DXT1, a format with no alpha channel at all, and stage 2 a
        // white-RGB texture whose shape lives in its alpha. Modulating
        // across both cuts the ball out of its square. Sampling only one
        // stage loses either the colour or the cut-out — with the mask
        // alone the balls came out white, with the colour map alone an
        // opaque square stayed around each one.
        let mut bytes = quad_floats();
        let uv_off = bytes.len() as u32;
        bytes.extend(floats(&[0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]));

        let mut c = ctx();
        let mut m = FakeMem { base: BASE, bytes };
        let names = c.gen_textures(2);

        c.set_capability(GL_BLEND, true);
        c.set_blend_func(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);

        // Stage 1: the colour map, opaque green all over.
        c.set_active_texture(1);
        c.bind_texture(GL_TEXTURE_2D, names[0]);
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            1,
            1,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[0, 255, 0, 255],
        );
        c.set_capability(GL_TEXTURE_2D, true);
        c.tex_env(GL_TEXTURE_ENV_MODE, GL_MODULATE);

        // Stage 2: the mask, white RGB throughout with the left half
        // opaque and the right half clear.
        c.set_active_texture(2);
        c.bind_texture(GL_TEXTURE_2D, names[1]);
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            2,
            1,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[255, 255, 255, 255, 255, 255, 255, 0],
        );
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
        c.set_capability(GL_TEXTURE_2D, true);
        c.tex_env(GL_TEXTURE_ENV_MODE, GL_MODULATE);

        c.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: BASE,
            buffer: 0,
        };
        // Both stages read the same coordinates here; the game gives
        // each its own array, which is why they are per-unit state.
        for unit in [1, 2] {
            c.set_client_active_texture(unit);
            c.set_client_state(GL_TEXTURE_COORD_ARRAY, true);
            c.set_texcoord_pointer(2, GL_FLOAT, 0, BASE + uv_off);
        }

        c.draw_arrays(&mut m, GL_TRIANGLE_FAN, 0, 4);

        // Where the mask is opaque the colour map's green survives;
        // where it is clear the background shows through untouched.
        assert_eq!(pixel(&c, 8, 32), [0, 255, 0, 255]);
        assert_eq!(pixel(&c, 56, 32), [0, 0, 0, 0]);
    }

    #[test]
    fn projection_and_modelview_both_apply() {
        // Halving via the modelview and halving again via the
        // projection must compound: coverage should drop to ~1/16.
        let mut full = ctx();
        let mut m1 = FakeMem {
            base: BASE,
            bytes: quad_floats(),
        };
        let ptr = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: BASE,
            buffer: 0,
        };
        full.vertex_array = ptr;
        full.draw_arrays(&mut m1, GL_TRIANGLE_FAN, 0, 4);
        let full_area = covered(&full);

        let mut half = ctx();
        let mut m2 = FakeMem {
            base: BASE,
            bytes: quad_floats(),
        };
        half.vertex_array = ptr;
        half.set_matrix_mode(GL_MODELVIEW);
        half.scale(0.5, 0.5, 1.0);
        half.set_matrix_mode(GL_PROJECTION);
        half.scale(0.5, 0.5, 1.0);
        half.draw_arrays(&mut m2, GL_TRIANGLE_FAN, 0, 4);
        let quarter_area = covered(&half);

        let ratio = full_area as f32 / quarter_area.max(1) as f32;
        assert!(
            (12.0..20.0).contains(&ratio),
            "expected ~16x area ratio, got {ratio}"
        );
    }

    #[test]
    fn matrix_stack_push_pop_isolates_transforms() {
        let mut c = ctx();
        c.set_matrix_mode(GL_MODELVIEW);
        c.load_identity();
        c.push_matrix();
        c.scale(0.25, 0.25, 1.0);
        c.pop_matrix();
        assert_eq!(*c.modelview.current(), matrix::IDENTITY);
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }

    #[test]
    fn stack_underflow_is_reported() {
        let mut c = ctx();
        c.set_matrix_mode(GL_MODELVIEW);
        c.pop_matrix();
        assert_eq!(c.take_error(), GL_STACK_UNDERFLOW);
    }

    #[test]
    fn first_error_survives_until_read() {
        let mut c = ctx();
        c.set_matrix_mode(0x9999); // GL_INVALID_ENUM
        c.set_viewport(0, 0, -1, -1); // GL_INVALID_VALUE
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }

    #[test]
    fn clear_uses_the_clear_color() {
        let mut c = ctx();
        c.clear_color = [0.0, 0.0, 1.0, 1.0];
        c.clear(GL_COLOR_BUFFER_BIT);
        assert_eq!(pixel(&c, 0, 0), [0, 0, 255, 255]);
    }

    #[test]
    fn gen_textures_returns_distinct_names() {
        let mut c = ctx();
        let a = c.gen_textures(3);
        let b = c.gen_textures(2);
        assert_eq!(a.len(), 3);
        assert!(a.iter().all(|n| *n != 0), "0 is the reserved default name");
        assert!(b.iter().all(|n| !a.contains(n)), "names were reused");
    }

    #[test]
    fn deleting_the_bound_texture_unbinds_it() {
        let mut c = ctx();
        let n = c.gen_textures(1);
        c.bind_texture(GL_TEXTURE_2D, n[0]);
        c.delete_textures(&n);
        assert_eq!(c.bound_texture, 0);
        assert!(!c.textures.contains_key(&n[0]));
    }

    #[test]
    fn default_texture_accepts_uploads_and_is_not_deleted() {
        let mut c = ctx();
        c.bind_texture(GL_TEXTURE_2D, 0);
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            1,
            1,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[1, 2, 3, 4],
        );
        assert_eq!(c.take_error(), GL_NO_ERROR);
        assert_eq!(c.textures[&0].rgba, [1, 2, 3, 4]);
        c.set_capability(GL_TEXTURE_2D, true);
        assert_eq!(c.texture_stages().1, 1);
        c.delete_textures(&[0]);
        assert_eq!(c.textures[&0].rgba, [1, 2, 3, 4]);
        c.bind_texture(GL_TEXTURE_2D, 1);
        c.delete_textures(&[1]);
        assert_eq!(c.bound_texture, 0);
        assert_eq!(c.texture_stages().1, 1);
    }

    #[test]
    fn tex_sub_image_patches_a_rectangle() {
        let mut c = ctx();
        let n = c.gen_textures(1);
        c.bind_texture(GL_TEXTURE_2D, n[0]);
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            2,
            2,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[0u8; 16],
        );
        c.tex_sub_image_2d(
            GL_TEXTURE_2D,
            0,
            1,
            1,
            1,
            1,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[9, 9, 9, 9],
        );
        let t = &c.textures[&n[0]];
        assert_eq!(&t.rgba[12..16], &[9, 9, 9, 9]);
        assert_eq!(&t.rgba[0..4], &[0, 0, 0, 0]);
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }

    #[test]
    fn tex_sub_image_out_of_bounds_is_rejected() {
        let mut c = ctx();
        let n = c.gen_textures(1);
        c.bind_texture(GL_TEXTURE_2D, n[0]);
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            2,
            2,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[0u8; 16],
        );
        c.tex_sub_image_2d(
            GL_TEXTURE_2D,
            0,
            2,
            2,
            2,
            2,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[0u8; 16],
        );
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
    }

    #[test]
    fn triangle_strip_winding_stays_consistent_under_culling() {
        // Every second triangle of a strip has reversed winding. If the
        // assembler doesn't compensate, back-face culling erases half
        // the mesh — the classic "striped" rendering bug.
        //
        // Vertices run top, bottom, top, bottom left-to-right, which
        // makes the first triangle counter-clockwise in GL's frame and
        // therefore front-facing.
        let strip = || {
            let mut b = Vec::new();
            for p in [
                [-1.0f32, 1.0, 0.0],
                [-1.0, -1.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, -1.0, 0.0],
                [1.0, 1.0, 0.0],
                [1.0, -1.0, 0.0],
            ] {
                b.extend(floats(&p));
            }
            b
        };
        let ptr = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: BASE,
            buffer: 0,
        };

        let mut plain = ctx();
        let mut m1 = FakeMem {
            base: BASE,
            bytes: strip(),
        };
        plain.vertex_array = ptr;
        plain.draw_arrays(&mut m1, GL_TRIANGLE_STRIP, 0, 6);
        let no_cull = covered(&plain);

        let mut culled = ctx();
        let mut m2 = FakeMem {
            base: BASE,
            bytes: strip(),
        };
        culled.vertex_array = ptr;
        culled.set_cull_face(GL_BACK);
        culled.set_capability(GL_CULL_FACE, true);
        culled.draw_arrays(&mut m2, GL_TRIANGLE_STRIP, 0, 6);
        let with_cull = covered(&culled);

        assert!(no_cull > 3000, "unculled strip covered {no_cull}");
        assert_eq!(
            with_cull, no_cull,
            "culling removed part of a consistently-wound strip"
        );

        // And the converse: flipping the front face must erase the
        // whole strip, proving the strip really was uniformly wound
        // rather than the cull test being a no-op.
        let mut flipped = ctx();
        let mut m3 = FakeMem {
            base: BASE,
            bytes: strip(),
        };
        flipped.vertex_array = ptr;
        flipped.set_front_face(GL_CW);
        flipped.set_cull_face(GL_BACK);
        flipped.set_capability(GL_CULL_FACE, true);
        flipped.draw_arrays(&mut m3, GL_TRIANGLE_STRIP, 0, 6);
        assert_eq!(covered(&flipped), 0, "strip was not uniformly wound");
    }

    #[test]
    fn streamed_strip_and_fan_match_explicit_triangles() {
        for (mode, indices) in [
            (GL_TRIANGLE_STRIP, vec![0, 1, 2, 2, 1, 3]),
            (GL_TRIANGLE_FAN, vec![0, 1, 2, 0, 2, 3]),
        ] {
            let bytes: Vec<u8> = [
                [-0.8f32, -0.8, 0.0],
                [0.8, -0.8, 0.0],
                [-0.8, 0.8, 0.0],
                [0.8, 0.8, 0.0],
            ]
            .iter()
            .flat_map(|p| floats(p))
            .collect();
            let mut optimized = ctx();
            let mut explicit = ctx();
            let pointer = ArrayPointer {
                enabled: true,
                size: 3,
                ty: GL_FLOAT,
                stride: 0,
                pointer: BASE,
                buffer: 0,
            };
            optimized.vertex_array = pointer;
            explicit.vertex_array = pointer;
            // Blending makes primitive order and overlapping coverage observable.
            optimized.set_capability(GL_BLEND, true);
            explicit.set_capability(GL_BLEND, true);
            optimized.draw_arrays(
                &mut FakeMem {
                    base: BASE,
                    bytes: bytes.clone(),
                },
                mode,
                0,
                4,
            );
            explicit.draw_elements(&mut FakeMem { base: BASE, bytes }, GL_TRIANGLES, &indices);
            assert_eq!(optimized.target.color, explicit.target.color);
            assert_eq!(optimized.target.depth, explicit.target.depth);
        }
    }

    #[test]
    fn line_strip_loop_guest_arrays_match_explicit_indexed_segments() {
        for mode in [GL_LINE_STRIP, GL_LINE_LOOP] {
            let mut strip = ctx();
            let mut explicit = ctx();
            for c in [&mut strip, &mut explicit] {
                c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
                c.set_client_state(GL_VERTEX_ARRAY, true);
                c.current_color = [0.8, 0.4, 0.2, 0.5];
                c.state.blend = true;
                c.state.blend_src = BlendFactor::SrcAlpha;
                c.state.blend_dst = BlendFactor::OneMinusSrcAlpha;
            }
            let bytes = floats(&[-0.65, 0.4, 0., 0.3, 0.4, 0., 0.3, -0.6, 0., -0.65, -0.6, 0.]);
            strip.draw_arrays(
                &mut FakeMem {
                    base: BASE,
                    bytes: bytes.clone(),
                },
                mode,
                0,
                4,
            );
            let indices = if mode == GL_LINE_LOOP {
                vec![0, 1, 1, 2, 2, 3, 3, 0]
            } else {
                vec![0, 1, 1, 2, 2, 3]
            };
            explicit.draw_elements(&mut FakeMem { base: BASE, bytes }, GL_LINES, &indices);
            assert!(covered(&strip) > 50);
            assert_eq!(strip.target.color, explicit.target.color);
            assert_eq!(strip.take_error(), GL_NO_ERROR);
        }
    }

    #[test]
    fn points_guest_arrays_render_and_size_errors_preserve_previous_values() {
        let mut c = ctx();
        c.set_vertex_pointer(3, GL_FLOAT, 0, BASE);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        c.set_point_size(3.);
        c.set_line_width(2.);
        c.draw_arrays(
            &mut FakeMem {
                base: BASE,
                bytes: floats(&[-0.5, 0.25, 0., 0.25, -0.5, 0.]),
            },
            GL_POINTS,
            0,
            2,
        );
        assert_eq!(covered(&c), 18);
        c.set_point_size(0.);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        assert_eq!(c.state.point_size, 3.);
        c.set_line_width(f32::INFINITY);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        assert_eq!(c.state.line_width, 2.);
    }

    #[test]
    fn points_and_lines_render_without_error() {
        let mut c = ctx();
        let mut m = FakeMem {
            base: BASE,
            bytes: quad_floats(),
        };
        c.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: BASE,
            buffer: 0,
        };
        c.draw_arrays(&mut m, GL_LINES, 0, 4);
        c.draw_arrays(&mut m, GL_POINTS, 0, 4);
        assert!(covered(&c) > 0);
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }

    #[test]
    fn unknown_primitive_mode_is_an_error() {
        let mut c = ctx();
        let mut m = FakeMem {
            base: BASE,
            bytes: quad_floats(),
        };
        c.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: BASE,
            buffer: 0,
        };
        c.draw_arrays(&mut m, 0x99, 0, 4);
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
    }

    #[test]
    fn lighting_capabilities_are_retained_without_rejecting_existing_games() {
        // COD2 enables lighting; erroring here would make the game
        // think the driver is broken.
        let mut c = ctx();
        c.set_capability(GL_LIGHTING, true);
        c.set_capability(0x4000, true); // GL_LIGHT0
        assert!(c.lighting.enabled && c.lighting.lights[0].enabled);
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }

    #[test]
    fn cull_face_before_enable_is_not_lost() {
        // Engines commonly call glCullFace once at init and toggle
        // GL_CULL_FACE per material. If the mode were only stored while
        // culling happened to be enabled, the toggle would silently
        // revert to GL_BACK.
        let mut c = ctx();
        c.set_cull_face(GL_FRONT);
        assert!(c.state.cull.is_none(), "mode set should not enable culling");
        c.set_capability(GL_CULL_FACE, true);
        assert_eq!(c.state.cull, Some(CullMode::Front));
        c.set_capability(GL_CULL_FACE, false);
        assert!(c.state.cull.is_none());
        c.set_capability(GL_CULL_FACE, true);
        assert_eq!(c.state.cull, Some(CullMode::Front), "mode was forgotten");
    }

    #[test]
    fn degenerate_frustum_is_rejected_without_nans() {
        let mut c = ctx();
        c.set_matrix_mode(GL_PROJECTION);
        c.frustum(-1.0, 1.0, -1.0, 1.0, 0.0, 100.0);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        assert!(c.projection.current().iter().all(|v| v.is_finite()));
    }

    // ---- buffer objects -------------------------------------------------

    /// Guest memory that refuses every read.
    ///
    /// A VBO-backed draw must not touch guest memory at all, so any
    /// read is a bug the test should fail on rather than tolerate.
    struct NoMem;

    impl GuestMemory for NoMem {
        fn read(&mut self, _addr: u32, _len: usize, _out: &mut Vec<u8>) -> bool {
            panic!("VBO draw read guest memory");
        }
    }

    #[test]
    fn draw_arrays_reads_from_array_buffer() {
        let mut c = ctx();
        let names = c.gen_buffers(1);
        c.bind_buffer(GL_ARRAY_BUFFER, names[0]);
        let quad = quad_floats();
        c.buffer_data(GL_ARRAY_BUFFER, quad.len(), Some(&quad), GL_STATIC_DRAW);
        // Offset 0 into the buffer, which is also the null guest pointer:
        // the draw must still happen.
        c.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: 0,
            buffer: names[0],
        };
        c.draw_arrays(&mut NoMem, GL_TRIANGLE_FAN, 0, 4);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        assert!(covered(&c) > 3000, "VBO quad covered {}", covered(&c));
    }

    #[test]
    fn draw_elements_reads_indices_from_element_buffer() {
        let mut c = ctx();
        let names = c.gen_buffers(2);
        c.bind_buffer(GL_ARRAY_BUFFER, names[0]);
        let quad = quad_floats();
        c.buffer_data(GL_ARRAY_BUFFER, quad.len(), Some(&quad), GL_STATIC_DRAW);
        c.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: 0,
            buffer: names[0],
        };
        c.bind_buffer(GL_ELEMENT_ARRAY_BUFFER, names[1]);
        let indices: Vec<u8> = [0u16, 1, 2, 0, 2, 3]
            .iter()
            .flat_map(|i| i.to_le_bytes())
            .collect();
        c.buffer_data(
            GL_ELEMENT_ARRAY_BUFFER,
            indices.len(),
            Some(&indices),
            GL_STATIC_DRAW,
        );
        c.draw_elements_from_guest(&mut NoMem, GL_TRIANGLES, 6, GL_UNSIGNED_SHORT, 0);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        assert!(
            covered(&c) > 3000,
            "indexed VBO quad covered {}",
            covered(&c)
        );
    }

    #[test]
    fn pointer_captures_binding_at_call_time_not_draw_time() {
        // GL ES 1.1 latches GL_ARRAY_BUFFER when gl*Pointer runs. A game
        // that binds a VBO, sets the vertex pointer, then binds a
        // different VBO for another array must still draw from the first.
        let mut c = ctx();
        let names = c.gen_buffers(2);
        c.bind_buffer(GL_ARRAY_BUFFER, names[0]);
        let quad = quad_floats();
        c.buffer_data(GL_ARRAY_BUFFER, quad.len(), Some(&quad), GL_STATIC_DRAW);
        c.set_vertex_pointer(3, GL_FLOAT, 0, 0);
        assert_eq!(c.vertex_array.buffer, names[0]);

        c.bind_buffer(GL_ARRAY_BUFFER, names[1]);
        c.buffer_data(GL_ARRAY_BUFFER, 16, None, GL_DYNAMIC_DRAW);
        assert_eq!(
            c.vertex_array.buffer, names[0],
            "rebinding clobbered an already-set pointer"
        );

        c.vertex_array.enabled = true;
        c.draw_arrays(&mut NoMem, GL_TRIANGLE_FAN, 0, 4);
        assert!(covered(&c) > 3000, "covered {}", covered(&c));
    }

    #[test]
    fn buffer_sub_data_rejects_out_of_range_writes() {
        let mut c = ctx();
        let names = c.gen_buffers(1);
        c.bind_buffer(GL_ARRAY_BUFFER, names[0]);
        c.buffer_data(GL_ARRAY_BUFFER, 8, None, GL_STATIC_DRAW);
        c.buffer_sub_data(GL_ARRAY_BUFFER, 4, &[1, 2, 3, 4]);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        assert_eq!(c.buffers[&names[0]].data, [0, 0, 0, 0, 1, 2, 3, 4]);

        // One byte past the end must write nothing at all, not clip.
        c.buffer_sub_data(GL_ARRAY_BUFFER, 5, &[9, 9, 9, 9]);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        assert_eq!(c.buffers[&names[0]].data, [0, 0, 0, 0, 1, 2, 3, 4]);
    }

    #[test]
    fn upload_without_a_bound_buffer_is_an_error() {
        let mut c = ctx();
        c.buffer_data(GL_ARRAY_BUFFER, 16, None, GL_STATIC_DRAW);
        assert_eq!(c.take_error(), GL_INVALID_OPERATION);
        c.buffer_sub_data(GL_ELEMENT_ARRAY_BUFFER, 0, &[1, 2]);
        assert_eq!(c.take_error(), GL_INVALID_OPERATION);
    }

    #[test]
    fn deleting_a_bound_buffer_clears_the_binding() {
        let mut c = ctx();
        let names = c.gen_buffers(2);
        c.bind_buffer(GL_ARRAY_BUFFER, names[0]);
        c.bind_buffer(GL_ELEMENT_ARRAY_BUFFER, names[1]);
        c.delete_buffers(&names);
        assert_eq!(c.array_buffer, 0);
        assert_eq!(c.element_array_buffer, 0);
        assert!(c.buffers.is_empty());
    }

    #[test]
    fn draw_from_a_short_buffer_renders_nothing() {
        // An out-of-range VBO read must drop the draw. Padding it with
        // zeroes would render a degenerate triangle at the origin and
        // hide the guest's mistake.
        let mut c = ctx();
        let names = c.gen_buffers(1);
        c.bind_buffer(GL_ARRAY_BUFFER, names[0]);
        let quad = quad_floats();
        c.buffer_data(GL_ARRAY_BUFFER, quad.len() - 4, Some(&quad), GL_STATIC_DRAW);
        c.set_vertex_pointer(3, GL_FLOAT, 0, 0);
        c.vertex_array.enabled = true;
        c.draw_arrays(&mut NoMem, GL_TRIANGLE_FAN, 0, 4);
        assert_eq!(covered(&c), 0, "short buffer still rendered");
    }
}

#[cfg(test)]
mod pvrtc_upload_tests {
    use super::*;
    #[test]
    fn pvrtc_uploads_keep_small_explicit_mips_and_reject_bad_updates() {
        let mut c = Context::new(8, 8);
        c.bind_texture(GL_TEXTURE_2D, 7);
        for level in 0..=3 {
            let size = 8 >> level;
            c.compressed_tex_image_2d(
                GL_TEXTURE_2D,
                level,
                size,
                size,
                GL_COMPRESSED_RGBA_PVRTC_2BPPV1_IMG,
                &[0; 32],
            );
            assert_eq!(c.take_error(), GL_NO_ERROR);
        }
        assert_eq!((c.textures[&7].width, c.textures[&7].height), (8, 8));
        assert_eq!(c.textures[&7].mipmaps[&3].0, 1);
        let revision = c.textures[&7].revision;
        c.compressed_tex_image_2d(
            GL_TEXTURE_2D,
            0,
            3,
            8,
            GL_COMPRESSED_RGBA_PVRTC_2BPPV1_IMG,
            &[0; 32],
        );
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        c.compressed_tex_image_2d(
            GL_TEXTURE_2D,
            0,
            8,
            8,
            GL_COMPRESSED_RGBA_PVRTC_2BPPV1_IMG,
            &[0; 31],
        );
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        assert_eq!(c.textures[&7].revision, revision);
    }
    #[test]
    fn software_viewport_limits_are_reported_and_enforced() {
        let mut c = Context::new(8, 8);
        let dims = c.max_viewport_dims();
        assert!(dims[0] > 0 && dims[1] > 0);
        c.set_viewport(3, 4, i32::MAX, i32::MAX);
        assert_eq!(c.state.viewport, (3, 4, dims[0], dims[1]));
        assert!(c.subpixel_bits() > 0);
    }
}

#[cfg(test)]
mod subimage_and_logic_tests {
    use super::*;

    #[test]
    fn logic_state_defaults_queries_and_invalid_enum() {
        let mut c = Context::new(4, 4);
        assert_eq!(
            c.query_state(GL_LOGIC_OP_MODE).unwrap().fixed(),
            vec![GL_COPY as i32]
        );
        assert_eq!(c.is_enabled(GL_COLOR_LOGIC_OP), Some(false));
        c.logic_op(0x1506);
        c.set_capability(GL_COLOR_LOGIC_OP, true);
        assert_eq!(
            c.query_state(GL_LOGIC_OP_MODE).unwrap().fixed(),
            vec![0x1506]
        );
        assert_eq!(
            c.query_state(GL_COLOR_LOGIC_OP).unwrap().fixed(),
            vec![65536]
        );
        c.logic_op(0x1234);
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
        assert_eq!(c.state.logic_mode, 0x1506);
    }

    #[test]
    fn copy_subimage_origin_format_mip_and_atomic_errors() {
        let mut c = Context::new(2, 2);
        c.target.color = vec![
            10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160,
        ];
        c.tex_image_2d(
            GL_TEXTURE_2D,
            1,
            2,
            2,
            GL_LUMINANCE_ALPHA,
            GL_UNSIGNED_BYTE,
            &[0; 8],
        );
        c.copy_tex_sub_image_2d(GL_TEXTURE_2D, 1, 1, 0, 0, 0, 1, 2);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        let tex = &c.textures[&0];
        assert_eq!(
            tex.mipmaps[&1].2,
            vec![0, 0, 0, 0, 90, 90, 90, 120, 0, 0, 0, 0, 10, 10, 10, 40]
        );
        let saved = tex.mipmaps[&1].2.clone();
        let revision = tex.revision;
        for args in [(1, 0, 2, 1), (-1, 0, 1, 1), (i32::MAX, 0, 1, 1)] {
            c.copy_tex_sub_image_2d(GL_TEXTURE_2D, 1, args.0, args.1, 0, 0, args.2, args.3);
            assert_eq!(c.take_error(), GL_INVALID_VALUE);
            assert_eq!(c.textures[&0].mipmaps[&1].2, saved);
            assert_eq!(c.textures[&0].revision, revision);
        }
        c.copy_tex_sub_image_2d(GL_TEXTURE_2D, 2, 0, 0, 0, 0, 1, 1);
        assert_eq!(c.take_error(), GL_INVALID_OPERATION);
        c.copy_tex_sub_image_2d(GL_TEXTURE_2D, 1, 0, 0, 0, 0, 0, 0);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        assert_eq!(c.textures[&0].revision, revision);
        c.tex_parameter(GL_TEXTURE_2D, 0x8191, 1);
        c.tex_image_2d(GL_TEXTURE_2D, 0, 2, 2, GL_RGB, GL_UNSIGNED_BYTE, &[0; 12]);
        c.copy_tex_sub_image_2d(GL_TEXTURE_2D, 1, 0, 0, 0, 0, 1, 1);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        assert_eq!(c.textures[&0].mipmaps[&1].2, vec![90, 100, 110, 255]);
    }

    #[test]
    fn compressed_subimages_preserve_other_blocks_and_reject_bad_calls() {
        let mut c = Context::new(4, 4);
        let red = [0x00, 0xf8, 0, 0, 0, 0, 0, 0];
        let green = [0xe0, 0x07, 0, 0, 0, 0, 0, 0];
        let bytes = [red, red, red, red].concat();
        c.compressed_tex_image_2d(
            GL_TEXTURE_2D,
            0,
            8,
            8,
            GL_COMPRESSED_RGB_S3TC_DXT1_EXT,
            &bytes,
        );
        c.compressed_tex_sub_image_2d(
            GL_TEXTURE_2D,
            0,
            4,
            0,
            4,
            4,
            GL_COMPRESSED_RGB_S3TC_DXT1_EXT,
            &green,
        );
        assert_eq!(c.take_error(), GL_NO_ERROR);
        let tex = &c.textures[&0];
        assert_eq!(&tex.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&tex.rgba[16..20], &[0, 255, 0, 255]);
        assert_eq!(&tex.compressed[&0].bytes[8..16], &green);
        let saved = tex.compressed[&0].bytes.clone();
        let revision = tex.revision;
        for (xo, w, data, error) in [
            (1, 4, green.as_slice(), GL_INVALID_OPERATION),
            (4, 3, green.as_slice(), GL_INVALID_OPERATION),
            (4, 4, &green[..7], GL_INVALID_VALUE),
            (8, 4, green.as_slice(), GL_INVALID_VALUE),
        ] {
            c.compressed_tex_sub_image_2d(
                GL_TEXTURE_2D,
                0,
                xo,
                0,
                w,
                4,
                GL_COMPRESSED_RGB_S3TC_DXT1_EXT,
                data,
            );
            assert_eq!(c.take_error(), error);
            assert_eq!(c.textures[&0].compressed[&0].bytes, saved);
            assert_eq!(c.textures[&0].revision, revision);
        }
        c.compressed_tex_sub_image_2d(GL_TEXTURE_2D, 0, 0, 0, 8, 8, GL_ATC_RGB_AMD, &bytes);
        assert_eq!(c.take_error(), GL_INVALID_OPERATION);
        c.compressed_tex_sub_image_2d(GL_TEXTURE_2D, 0, 0, 0, 8, 8, GL_PALETTE4_RGB8_OES, &bytes);
        assert_eq!(c.take_error(), GL_INVALID_OPERATION);
        c.compressed_tex_sub_image_2d(
            GL_TEXTURE_2D,
            0,
            0,
            0,
            8,
            8,
            GL_COMPRESSED_RGBA_S3TC_DXT1_EXT,
            &bytes,
        );
        assert_eq!(c.take_error(), GL_INVALID_OPERATION);
    }

    #[test]
    #[ignore = "requires native OpenGL; framebuffer copy and compressed GPU cache refresh"]
    fn native_subimages_refresh_uploaded_textures_and_copy_rendered_pixels() {
        let mut c = Context::new(4, 4);
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
        c.enable_gpu().unwrap();
        c.set_internal_resolution(2);
        c.clear_color = [0.2, 0.4, 0.6, 0.8];
        c.clear(GL_COLOR_BUFFER_BIT);
        c.tex_image_2d(GL_TEXTURE_2D, 0, 2, 2, GL_RGB, GL_UNSIGNED_BYTE, &[0; 12]);
        c.copy_tex_sub_image_2d(GL_TEXTURE_2D, 0, 0, 0, 0, 0, 2, 2);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        assert_eq!(c.textures[&0].rgba, [51, 102, 153, 255].repeat(4));
        c.compressed_tex_image_2d(
            GL_TEXTURE_2D,
            0,
            4,
            4,
            GL_COMPRESSED_RGB_S3TC_DXT1_EXT,
            &[0, 248, 0, 0, 0, 0, 0, 0],
        );
        let stages = [raster::TextureStage {
            unit: 0,
            env: raster::TexEnvMode::Replace,
            env_color: [0.; 4],
            lod_bias: 0.,
        }];
        let tri =
            [[-1., -1., 0., 1.], [1., -1., 0., 1.], [0., 1., 0., 1.]].map(|pos| raster::Vertex {
                pos,
                color: [1.; 4],
                ..Default::default()
            });
        for (data, expected) in [
            ([0, 248, 0, 0, 0, 0, 0, 0], [255, 0, 0, 255]),
            ([224, 7, 0, 0, 0, 0, 0, 0], [0, 255, 0, 255]),
        ] {
            c.compressed_tex_sub_image_2d(
                GL_TEXTURE_2D,
                0,
                0,
                0,
                4,
                4,
                GL_COMPRESSED_RGB_S3TC_DXT1_EXT,
                &data,
            );
            c.gpu
                .as_mut()
                .unwrap()
                .draw(
                    &c.target,
                    &c.state,
                    GL_TRIANGLES,
                    &tri,
                    &stages,
                    &c.textures,
                    [0; 6],
                )
                .unwrap();
            c.sync_gpu();
            assert_eq!(&c.target.color[36..40], &expected);
        }
        // Explicit nonbase levels are also stored and may be replaced at small block edges.
        c.compressed_tex_image_2d(
            GL_TEXTURE_2D,
            1,
            2,
            2,
            GL_COMPRESSED_RGB_S3TC_DXT1_EXT,
            &[0; 8],
        );
        c.compressed_tex_sub_image_2d(
            GL_TEXTURE_2D,
            1,
            0,
            0,
            2,
            2,
            GL_COMPRESSED_RGB_S3TC_DXT1_EXT,
            &[0, 248, 0, 0, 0, 0, 0, 0],
        );
        assert_eq!(c.take_error(), GL_NO_ERROR);
        assert_eq!(
            c.textures[&0].rgba_level(1).unwrap().as_ref(),
            [255, 0, 0, 255].repeat(4)
        );
    }

    #[test]
    fn pvrtc_subimage_requires_whole_level_and_exact_size() {
        let mut c = Context::new(4, 4);
        let format = GL_COMPRESSED_RGB_PVRTC_4BPPV1_IMG;
        c.compressed_tex_image_2d(GL_TEXTURE_2D, 0, 8, 8, format, &[0; 32]);
        c.compressed_tex_sub_image_2d(GL_TEXTURE_2D, 0, 0, 0, 8, 8, format, &[255; 32]);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        assert_eq!(c.textures[&0].compressed[&0].bytes.as_ref(), &[255; 32]);
        c.compressed_tex_sub_image_2d(GL_TEXTURE_2D, 0, 0, 0, 4, 8, format, &[0; 32]);
        assert_eq!(c.take_error(), GL_INVALID_OPERATION);
        c.compressed_tex_sub_image_2d(GL_TEXTURE_2D, 0, 0, 0, 8, 8, format, &[0; 33]);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
    }
}

#[cfg(test)]
mod clear_conformance_tests {
    use super::*;
    fn context() -> Context {
        Context::new(4, 4)
    }
    fn color_cases(c: &mut Context) {
        let old = [31, 63, 95, 127];
        let new = [223, 191, 159, 255];
        for bits in 0..16 {
            c.state.color_mask = [true; 4];
            c.state.scissor = None;
            c.clear_color = old.map(|v| v as f32 / 255.);
            c.clear(GL_COLOR_BUFFER_BIT);
            c.state.color_mask = std::array::from_fn(|i| bits & (1 << i) != 0);
            c.state.scissor = Some((1, 1, 2, 2));
            c.clear_color = new.map(|v| v as f32 / 255.);
            c.clear(GL_COLOR_BUFFER_BIT);
            c.sync_gpu();
            for y in 0..4 {
                for x in 0..4 {
                    for channel in 0..4 {
                        let expected = if (1..3).contains(&x)
                            && (1..3).contains(&y)
                            && c.state.color_mask[channel]
                        {
                            new[channel]
                        } else {
                            old[channel]
                        };
                        assert_eq!(
                            c.target.color[(y * 4 + x) * 4 + channel],
                            expected,
                            "mask={bits:x}, x={x}, y={y}, channel={channel}"
                        );
                    }
                }
            }
            assert_eq!(c.take_error(), GL_NO_ERROR);
        }
        c.state.scissor = None;
        c.state.color_mask = [true; 4];
        let before = c.target.color.clone();
        c.clear_color = [1.; 4];
        c.clear(GL_COLOR_BUFFER_BIT | 0x80000000);
        c.sync_gpu();
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        assert_eq!(
            c.target.color, before,
            "invalid command must not partially clear"
        );
        c.clear(0);
        c.clear(GL_STENCIL_BUFFER_BIT);
        c.sync_gpu();
        assert_eq!(c.take_error(), GL_NO_ERROR);
        assert_eq!(c.target.color, before);
    }
    struct Memory(Vec<u8>);
    impl GuestMemory for Memory {
        fn read(&mut self, address: u32, length: usize, out: &mut Vec<u8>) -> bool {
            let Some(start) = address.checked_sub(4096) else {
                return false;
            };
            let Some(bytes) = self.0.get(start as usize..start as usize + length) else {
                return false;
            };
            out.clear();
            out.extend_from_slice(bytes);
            true
        }
    }
    fn depth_cases(c: &mut Context) {
        c.state.scissor = None;
        c.state.color_mask = [true; 4];
        c.state.depth_test = false;
        c.state.depth_write = true;
        c.clear_color = [0.; 4];
        c.clear_depth = 0.25;
        c.clear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);
        c.state.depth_write = false;
        c.clear_depth = 1.;
        c.clear(GL_DEPTH_BUFFER_BIT);
        c.state.depth_test = true;
        c.current_color = [0., 0., 1., 1.];
        c.set_vertex_pointer(3, GL_FLOAT, 0, 4096);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        let bytes = [-1f32, -1., 0., 1., -1., 0., 0., 1., 0.]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        let mut memory = Memory(bytes);
        c.draw_arrays(&mut memory, GL_TRIANGLES, 0, 3);
        c.sync_gpu();
        assert_eq!(
            &c.target.color[36..40],
            &[0; 4],
            "disabled depth writes protect depth during Clear"
        );
        c.state.depth_test = false;
        c.state.depth_write = true;
        c.clear(GL_DEPTH_BUFFER_BIT);
        c.state.depth_test = true;
        c.state.depth_write = false;
        c.draw_arrays(&mut memory, GL_TRIANGLES, 0, 3);
        c.sync_gpu();
        assert_eq!(
            &c.target.color[36..40],
            &[0, 0, 255, 255],
            "depth clear does not require DEPTH_TEST"
        );
        assert!(
            !c.state.depth_write,
            "Clear must not change the stored mask"
        );
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }
    #[test]
    fn software_clear_obeys_all_color_masks_scissor_depth_and_errors() {
        let mut c = context();
        color_cases(&mut c);
        depth_cases(&mut c);
    }
    #[test]
    fn default_clear_color_and_extreme_scissors_are_defined() {
        let mut c = context();
        assert_eq!(c.clear_color, [0.; 4]);
        c.clear_color = [1.; 4];
        c.state.scissor = Some((i32::MAX, i32::MAX, i32::MAX, i32::MAX));
        c.clear(GL_COLOR_BUFFER_BIT);
        assert!(c.target.color.iter().all(|v| *v == 0));
        c.state.scissor = Some((-1, -1, i32::MAX, i32::MAX));
        c.clear(GL_COLOR_BUFFER_BIT);
        assert!(c.target.color.iter().all(|v| *v == 255));
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires native OpenGL and GLES2; clear masks and scissor at both resolutions"]
    fn native_clear_obeys_all_masks_scissor_depth_and_errors() {
        for es in [false, true] {
            for scale in [1, 2] {
                let mut c = context();
                c.gpu = Some(if es {
                    crate::gpu::Renderer::new_es_for_test(4, 4).unwrap()
                } else {
                    crate::gpu::Renderer::new(4, 4).unwrap()
                });
                c.set_internal_resolution(scale);
                color_cases(&mut c);
                depth_cases(&mut c);
            }
        }
    }
}

#[cfg(test)]
mod rgb565_surface_tests {
    use super::*;
    struct Memory(Vec<u8>);
    impl GuestMemory for Memory {
        fn read(&mut self, address: u32, length: usize, out: &mut Vec<u8>) -> bool {
            let Some(offset) = address.checked_sub(4096) else {
                return false;
            };
            let Some(bytes) = self.0.get(offset as usize..offset as usize + length) else {
                return false;
            };
            out.clear();
            out.extend_from_slice(bytes);
            true
        }
    }
    fn draw(c: &mut Context) {
        let data = [-1f32, -1., 0., 1., -1., 0., 0., 1., 0.]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        c.set_vertex_pointer(3, GL_FLOAT, 0, 4096);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        c.draw_arrays(&mut Memory(data), GL_TRIANGLES, 0, 3);
        c.sync_gpu();
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }
    fn pixel(c: &Context) -> [u8; 4] {
        c.target.color[36..40].try_into().unwrap()
    }
    fn alpha_cases(c: &mut Context) {
        assert!(c.target.rgb565);
        for (src, expected) in [
            (GL_DST_ALPHA, [255, 0, 0, 255]),
            (GL_ONE_MINUS_DST_ALPHA, [0, 0, 0, 255]),
            (GL_SRC_ALPHA_SATURATE, [0, 0, 0, 255]),
        ] {
            c.clear_color = [0., 0., 1., 0.];
            c.clear(GL_COLOR_BUFFER_BIT);
            c.current_color = [1., 0., 0., 0.25];
            c.state.blend = true;
            c.set_blend_func(src, GL_ZERO);
            draw(c);
            assert_eq!(pixel(c), expected, "blend source={src:x}");
        }
        c.state.blend = false;
        c.state.alpha_test = true;
        c.state.alpha_ref = 0.5;
        c.state.alpha_func = raster::CompareFunc::Greater;
        c.clear_color = [0., 0., 1., 0.];
        c.clear(GL_COLOR_BUFFER_BIT);
        draw(c);
        assert_eq!(
            pixel(c),
            [0, 0, 255, 255],
            "fragment alpha remains available before storage"
        );
        c.state.alpha_test = false;
    }
    fn logic_cases(c: &mut Context) {
        let source = [10u8, 21, 7];
        let dest = [5u8, 34, 28];
        let levels = [31u32, 63, 31];
        c.state.logic_op = true;
        for op in 0x1500..=0x150f {
            c.clear_color = [
                dest[0] as f32 / 31.,
                dest[1] as f32 / 63.,
                dest[2] as f32 / 31.,
                0.,
            ];
            c.clear(GL_COLOR_BUFFER_BIT);
            c.current_color = [
                source[0] as f32 / 31.,
                source[1] as f32 / 63.,
                source[2] as f32 / 31.,
                0.1,
            ];
            c.logic_op(op);
            draw(c);
            let result = pixel(c);
            for i in 0..3 {
                let bits = raster::logic_byte(op, source[i], dest[i]) & levels[i] as u8;
                let expected = ((u32::from(bits) * 255 + levels[i] / 2) / levels[i]) as u8;
                assert!(
                    result[i].abs_diff(expected) <= 1,
                    "op={op:x} channel={i}: {result:?}, expected {expected}"
                );
            }
            assert_eq!(result[3], 255);
        }
        c.state.logic_op = false;
    }
    fn copies(c: &mut Context) {
        c.clear_color = [1., 0., 0., 0.];
        c.clear(GL_COLOR_BUFFER_BIT);
        c.copy_tex_image_2d(GL_TEXTURE_2D, 0, GL_RGB, 0, 0, 2, 2, 0);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        assert_eq!(c.textures[&0].rgba, [255, 0, 0, 255].repeat(4));
        let revision = c.textures[&0].revision;
        for format in [GL_RGBA, GL_ALPHA, GL_LUMINANCE_ALPHA] {
            c.copy_tex_image_2d(GL_TEXTURE_2D, 0, format, 0, 0, 2, 2, 0);
            assert_eq!(c.take_error(), GL_INVALID_OPERATION);
            assert_eq!(c.textures[&0].revision, revision);
        }
        c.tex_image_2d(
            GL_TEXTURE_2D,
            1,
            2,
            2,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[123; 16],
        );
        c.copy_tex_sub_image_2d(GL_TEXTURE_2D, 1, 0, 0, 0, 0, 1, 1);
        assert_eq!(c.take_error(), GL_INVALID_OPERATION);
        assert_eq!(c.textures[&0].mipmaps[&1].2, [123; 16]);
        c.tex_image_2d(GL_TEXTURE_2D, 1, 2, 2, GL_RGB, GL_UNSIGNED_BYTE, &[0; 12]);
        c.copy_tex_sub_image_2d(GL_TEXTURE_2D, 1, 0, 0, 0, 0, 1, 1);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        assert_eq!(&c.textures[&0].mipmaps[&1].2[..4], &[255, 0, 0, 255]);
    }
    #[test]
    fn software_rgb565_alpha_logic_and_copies() {
        let mut c = Context::new_rgb565(4, 4);
        alpha_cases(&mut c);
        logic_cases(&mut c);
        copies(&mut c);
        c.target.resize(8, 8);
        assert!(c.target.rgb565);
        assert!(c.target.color.chunks_exact(4).all(|p| p[3] == 255));
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires native OpenGL/GLES2; real RGB565 targets and fixed-function alpha"]
    fn native_rgb565_alpha_logic_and_copies() {
        for es in [false, true] {
            for scale in [1, 2] {
                let mut c = Context::new_rgb565(4, 4);
                let mut gpu = if es {
                    crate::gpu::Renderer::new_es_for_test(4, 4).unwrap()
                } else {
                    crate::gpu::Renderer::new(4, 4).unwrap()
                };
                gpu.set_surface_rgb565(true).unwrap();
                c.gpu = Some(gpu);
                c.set_internal_resolution(scale);
                alpha_cases(&mut c);
                logic_cases(&mut c);
                copies(&mut c);
            }
        }
    }
}

#[cfg(test)]
mod homogeneous_texture_tests {
    use super::*;
    struct Memory(Vec<u8>);
    impl GuestMemory for Memory {
        fn read(&mut self, address: u32, len: usize, out: &mut Vec<u8>) -> bool {
            let Some(offset) = address.checked_sub(4096) else {
                return false;
            };
            let Some(bytes) = self.0.get(offset as usize..offset as usize + len) else {
                return false;
            };
            out.clear();
            out.extend_from_slice(bytes);
            true
        }
    }
    fn setup() -> (Context, Memory) {
        let mut c = Context::new(32, 32);
        c.bind_texture(GL_TEXTURE_2D, 1);
        let pixels = (0..4)
            .flat_map(|y| (0..4).flat_map(move |x| [x * 60, y * 60, 0, 255]))
            .collect::<Vec<u8>>();
        c.tex_image_2d(GL_TEXTURE_2D, 0, 4, 4, GL_RGBA, GL_UNSIGNED_BYTE, &pixels);
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
        c.set_capability(GL_TEXTURE_2D, true);
        c.tex_env(GL_TEXTURE_ENV_MODE, GL_REPLACE);
        let mut matrix = matrix::IDENTITY;
        matrix[8] = 0.75;
        matrix[11] = 0.5;
        c.texture_matrix[0].load(matrix);
        let positions = [-0.9f32, -0.9, 0., 0.9, -0.9, 0., 0., 0.9, 0.];
        let memory = Memory(positions.into_iter().flat_map(f32::to_le_bytes).collect());
        c.set_vertex_pointer(3, GL_FLOAT, 0, 4096);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        (c, memory)
    }
    fn render(c: &mut Context, memory: &mut Memory) {
        c.clear(GL_COLOR_BUFFER_BIT);
        c.draw_arrays(memory, GL_TRIANGLES, 0, 3);
        c.sync_gpu();
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }
    fn at(c: &Context, x: u32, y: u32) -> [u8; 4] {
        let i = (y * c.target.width + x) as usize * 4;
        c.target.color[i..i + 4].try_into().unwrap()
    }
    fn cases(c: &mut Context, memory: &mut Memory, scale: u32, unit: u32) {
        if unit != 0 {
            let texture_matrix = *c.texture_matrix[0].current();
            c.set_capability(GL_TEXTURE_2D, false);
            c.set_active_texture(unit);
            c.set_client_active_texture(unit);
            c.bind_texture(GL_TEXTURE_2D, 1);
            c.set_capability(GL_TEXTURE_2D, true);
            c.tex_env(GL_TEXTURE_ENV_MODE, GL_REPLACE);
            c.texture_matrix[unit as usize].load(texture_matrix);
        }
        let input = [0.25f32, 0.6, 2., 2.];
        c.set_multi_texcoord4(GL_TEXTURE0 + unit, input);
        render(c, memory);
        assert_eq!(
            at(c, 16, 16),
            [120, 0, 0, 255],
            "current coordinates and complete texture matrix"
        );
        assert_eq!(
            c.query_state(0x0b03).unwrap().numbers(),
            input.map(f64::from)
        );
        c.set_client_state(GL_TEXTURE_COORD_ARRAY, true);
        for size in [2, 3, 4] {
            memory.0.truncate(36);
            for _ in 0..3 {
                for v in &input[..size] {
                    memory.0.extend(v.to_le_bytes());
                }
            }
            c.set_texcoord_pointer(size as u32, GL_FLOAT, 0, 4096 + 36);
            render(c, memory);
            let expected = match size {
                2 => [60, 120, 0, 255],
                3 => [180, 60, 0, 255],
                _ => [120, 0, 0, 255],
            };
            assert_eq!(at(c, 16, 16), expected, "array size={size}");
        }
        // A native fixed-point VBO retains q; mutating it must refresh the draw.
        c.bind_buffer(GL_ARRAY_BUFFER, 9);
        for (q, expected) in [(2., [120, 0, 0, 255]), (4., [60, 0, 0, 255])] {
            let mut data = Vec::new();
            for _ in 0..3 {
                for v in [input[0], input[1], input[2], q] {
                    data.extend(((v * 65536.) as i32).to_le_bytes());
                }
            }
            c.buffer_data(GL_ARRAY_BUFFER, data.len(), Some(&data), GL_DYNAMIC_DRAW);
            c.set_texcoord_pointer(4, GL_FIXED, 0, 0);
            c.bind_buffer(GL_ARRAY_BUFFER, 0);
            render(c, memory);
            assert_eq!(at(c, 16, 16), expected, "fixed VBO q={q}");
            c.bind_buffer(GL_ARRAY_BUFFER, 9);
        }
        c.bind_buffer(GL_ARRAY_BUFFER, 0);
        let uv = [
            [0.1f32, 0.2, 0.5, 1.3],
            [1.4, 0.3, 1.1, 2.1],
            [0.8, 1.4, 2., 3.2],
        ];
        memory.0.truncate(36);
        for coord in uv {
            for v in coord {
                memory.0.extend(v.to_le_bytes());
            }
        }
        c.set_texcoord_pointer(4, GL_FLOAT, 0, 4096 + 36);
        c.clip_plane(0x3000, [1., 0., 0., 0.]);
        for clipped in [false, true] {
            c.set_capability(0x3000, clipped);
            for flat in [false, true] {
                c.state.smooth_shading = !flat;
                render(c, memory);
                for (x, y) in [(16, 16), (12, 18), (18, 22)] {
                    if clipped && x < 16 {
                        assert_eq!(at(c, x, y), [0; 4]);
                        continue;
                    }
                    // Independent window-space barycentrics, full matrix, then division by q.
                    let mut sum = [0u32; 4];
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let nx = 2. * (x as f64 + (dx as f64 + 0.5) / scale as f64) / 32. - 1.;
                            let ny = 1. - 2. * (y as f64 + (dy as f64 + 0.5) / scale as f64) / 32.;
                            let b2 = (ny + 0.9) / 1.8;
                            let b1 = (nx + 0.9 - 0.9 * b2) / 1.8;
                            let weights = [1. - b1 - b2, b1, b2];
                            let s = (0..3)
                                .map(|i| weights[i] * (uv[i][0] as f64 + 0.75 * uv[i][2] as f64))
                                .sum::<f64>();
                            let t = (0..3).map(|i| weights[i] * uv[i][1] as f64).sum::<f64>();
                            let q = (0..3)
                                .map(|i| weights[i] * (uv[i][3] as f64 + 0.5 * uv[i][2] as f64))
                                .sum::<f64>();
                            let tx = ((s / q * 4.).floor() as i32).clamp(0, 3) as u32;
                            let ty = ((t / q * 4.).floor() as i32).clamp(0, 3) as u32;
                            sum[0] += tx * 60;
                            sum[1] += ty * 60;
                            sum[3] += 255;
                        }
                    }
                    assert_eq!(
                        at(c, x, y),
                        sum.map(|v| (v / (scale * scale)) as u8),
                        "variable q x={x} y={y} flat={flat}"
                    );
                }
            }
        }
    }
    #[test]
    fn software_current_arrays_fixed_vbo_and_varying_q() {
        for unit in [0, 3] {
            let (mut c, mut memory) = setup();
            cases(&mut c, &mut memory, 1, unit);
        }
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires native OpenGL/GLES2; projective arrays, constants, VBOs and flat streams"]
    fn native_current_arrays_fixed_vbo_and_varying_q() {
        for es in [false, true] {
            for scale in [1, 2] {
                for unit in [0, 3] {
                    let (mut c, mut memory) = setup();
                    c.gpu = Some(if es {
                        crate::gpu::Renderer::new_es_for_test(32, 32).unwrap()
                    } else {
                        crate::gpu::Renderer::new(32, 32).unwrap()
                    });
                    c.set_internal_resolution(scale as u8);
                    cases(&mut c, &mut memory, scale, unit);
                }
            }
        }
    }
}

#[cfg(test)]
mod point_extension_tests {
    use super::*;
    struct Memory(Vec<u8>);
    impl GuestMemory for Memory {
        fn read(&mut self, address: u32, len: usize, out: &mut Vec<u8>) -> bool {
            let Some(offset) = address.checked_sub(4096) else {
                return false;
            };
            let Some(bytes) = self.0.get(offset as usize..offset as usize + len) else {
                return false;
            };
            out.clear();
            out.extend_from_slice(bytes);
            true
        }
    }
    fn positions(values: &[f32]) -> Memory {
        Memory(values.iter().flat_map(|v| v.to_le_bytes()).collect())
    }
    fn pixel(c: &Context, x: u32, y: u32) -> [u8; 4] {
        let i = (y * 32 + x) as usize * 4;
        c.target.color[i..i + 4].try_into().unwrap()
    }
    fn count(c: &Context) -> usize {
        c.target.color.chunks_exact(4).filter(|p| p[0] > 0).count()
    }
    fn draw(c: &mut Context, memory: &mut Memory, mode: u32, count: u32) {
        c.clear(GL_COLOR_BUFFER_BIT);
        c.draw_arrays(memory, mode, 0, count);
        c.sync_gpu();
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }
    fn sprite_cases(c: &mut Context, scale: u32) {
        c.bind_texture(GL_TEXTURE_2D, 1);
        let data = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
        ];
        c.tex_image_2d(GL_TEXTURE_2D, 0, 2, 2, GL_RGBA, GL_UNSIGNED_BYTE, &data);
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
        let mut memory = positions(&[0., 0., 0., -0.9, -0.9, 0., 0.9, -0.9, 0., 0., 0.9, 0.]);
        c.set_vertex_pointer(3, GL_FLOAT, 0, 4096);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        c.set_point_size(8.);
        for unit in [0, 3] {
            c.set_capability(GL_TEXTURE_2D, false);
            c.set_active_texture(unit);
            c.bind_texture(GL_TEXTURE_2D, 1);
            c.set_capability(GL_TEXTURE_2D, true);
            c.tex_env(GL_TEXTURE_ENV_MODE, GL_REPLACE);
            c.set_capability(GL_POINT_SPRITE_OES, true);
            c.tex_env(GL_COORD_REPLACE_OES, 1);
            // Replacement is after the texture matrix, and t grows top to bottom.
            c.texture_matrix[unit as usize].load(matrix::translate(0.75, 0.75, 0.));
            draw(c, &mut memory, GL_POINTS, 1);
            for (x, y, expected) in [
                (13, 13, [255, 0, 0, 255]),
                (18, 13, [0, 255, 0, 255]),
                (13, 18, [0, 0, 255, 255]),
                (18, 18, [255, 255, 0, 255]),
            ] {
                assert_eq!(pixel(c, x, y), expected, "sprite unit={unit} at {x},{y}");
            }
            c.tex_env(GL_COORD_REPLACE_OES, 0);
            draw(c, &mut memory, GL_POINTS, 1);
            assert_eq!(pixel(c, 13, 13), [255, 255, 0, 255]);
            assert_eq!(pixel(c, 18, 18), [255, 255, 0, 255]);
            c.tex_env(GL_COORD_REPLACE_OES, 1);
            c.set_capability(GL_POINT_SPRITE_OES, false);
            draw(c, &mut memory, GL_POINTS, 1);
            assert_eq!(pixel(c, 13, 13), [255, 255, 0, 255]);
            c.set_capability(GL_POINT_SPRITE_OES, true);
            // The same state must not activate sprite replacement for triangles.
            c.clear(GL_COLOR_BUFFER_BIT);
            c.draw_arrays(&mut memory, GL_TRIANGLES, 1, 3);
            c.sync_gpu();
            assert_eq!(pixel(c, 16, 16), [255, 255, 0, 255]);
            draw(c, &mut memory, GL_POINTS, 1);
            assert_eq!(pixel(c, 13, 13), [255, 0, 0, 255]);
        }
        // Explicit mip levels: replacement coordinates must provide useful derivatives.
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            8,
            8,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[255, 0, 0, 255].repeat(64),
        );
        c.tex_image_2d(
            GL_TEXTURE_2D,
            1,
            4,
            4,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[0, 255, 0, 255].repeat(16),
        );
        c.tex_image_2d(
            GL_TEXTURE_2D,
            2,
            2,
            2,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[0, 0, 255, 255].repeat(4),
        );
        c.tex_image_2d(
            GL_TEXTURE_2D,
            3,
            1,
            1,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[255, 255, 0, 255],
        );
        c.tex_parameter(
            GL_TEXTURE_2D,
            GL_TEXTURE_MIN_FILTER,
            GL_NEAREST_MIPMAP_NEAREST,
        );
        c.set_point_size(4.);
        draw(c, &mut memory, GL_POINTS, 1);
        assert_eq!(
            pixel(c, 16, 16),
            if scale == 1 {
                [0, 255, 0, 255]
            } else {
                [255, 0, 0, 255]
            },
            "sprite mipmap derivatives"
        );
        c.set_lod_bias(1.);
        draw(c, &mut memory, GL_POINTS, 1);
        assert_eq!(
            pixel(c, 16, 16),
            if scale == 1 {
                [0, 0, 255, 255]
            } else {
                [0, 255, 0, 255]
            },
            "sprite LOD bias"
        );
        c.set_lod_bias(0.);
        if scale == 1 {
            let pixels = (0..8)
                .flat_map(|y| (0..8).flat_map(move |x| [x * 32, y * 32, 0, 255]))
                .collect::<Vec<u8>>();
            c.tex_image_2d(GL_TEXTURE_2D, 0, 8, 8, GL_RGBA, GL_UNSIGNED_BYTE, &pixels);
            c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
            for (i, value) in [0.015625f32, 0.046875, 0.].into_iter().enumerate() {
                memory.0[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
            }
            draw(c, &mut memory, GL_POINTS, 1);
            assert_eq!(
                pixel(c, 15, 14),
                if c.target.rgb565 {
                    raster::rgb565_pixel([64, 64, 0, 255])
                } else {
                    [64, 64, 0, 255]
                },
                "sprite texture coordinates use the unrounded center"
            );
        }
        c.set_capability(GL_TEXTURE_2D, false);
        c.set_active_texture(0);
        c.set_capability(GL_POINT_SPRITE_OES, false);
    }
    fn size_cases(c: &mut Context) {
        let mut memory = positions(&[-0.5, 0., 0., 0.5, 0., 0.]);
        for v in [2f32, 91., 6., 92.] {
            memory.0.extend(v.to_le_bytes());
        }
        c.set_vertex_pointer(3, GL_FLOAT, 0, 4096);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        c.set_point_size(19.);
        c.set_point_size_pointer(GL_FLOAT, 8, 4096 + 24);
        c.set_client_state(GL_POINT_SIZE_ARRAY_OES, true);
        c.lighting.enabled = true;
        c.lighting.model_ambient = [0.; 4];
        c.lighting.ambient = [0.; 4];
        c.lighting.diffuse = [1.; 4];
        c.lighting.lights[0].enabled = true;
        c.lighting.lights[0].position = [0., 0., 1., 1.];
        c.lighting.lights[0].attenuation = [1., 0., 1.];
        draw(c, &mut memory, GL_POINTS, 2);
        assert_eq!(count(c), 40);
        for x in [8, 24] {
            assert!(
                pixel(c, x, 16)[0].abs_diff(if c.target.rgb565 { 99 } else { 101 }) <= 1,
                "point-size attribute must not replace lighting position"
            );
        }
        if c.gpu.is_some() {
            assert!(!c.es11.pretransform, "native array points stay batched");
            assert_eq!(c.vertex_stream.attributes[7].unwrap().size, 1);
            assert_eq!(c.vertex_stream.count, 2);
        }
        c.state.smooth_shading = false;
        draw(c, &mut memory, GL_POINTS, 2);
        assert_eq!(
            count(c),
            40,
            "flat points retain individual sizes and lighting"
        );
        c.state.smooth_shading = true;
        c.set_client_state(GL_POINT_SIZE_ARRAY_OES, false);
        c.set_point_size(4.);
        draw(c, &mut memory, GL_POINTS, 2);
        assert_eq!(count(c), 32);
        c.set_client_state(GL_POINT_SIZE_ARRAY_OES, true);
        c.bind_buffer(GL_ARRAY_BUFFER, 9);
        let data = [4i32 * 65536, 8 * 65536]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>();
        c.buffer_data(GL_ARRAY_BUFFER, data.len(), Some(&data), GL_DYNAMIC_DRAW);
        c.set_point_size_pointer(GL_FIXED, 0, 0);
        c.bind_buffer(GL_ARRAY_BUFFER, 0);
        draw(c, &mut memory, GL_POINTS, 2);
        assert_eq!(count(c), 80);
        assert_eq!(
            c.query_state(GL_POINT_SIZE_ARRAY_BUFFER_BINDING_OES)
                .unwrap()
                .numbers(),
            vec![9.]
        );
        c.point_parameter(0x8129, &[4., 0., 0.]);
        draw(c, &mut memory, GL_POINTS, 2);
        assert_eq!(count(c), 20, "attenuation scales array values");
        c.point_parameter(0x8129, &[1., 0., 0.]);
        let maximum = c.primitive_size_range(true)[1];
        c.point_parameter(0x8126, &[2.]);
        c.point_parameter(0x8127, &[4.]);
        draw(c, &mut memory, GL_POINTS, 2);
        assert_eq!(count(c), 32, "point array sizes respect clamps");
        c.point_parameter(0x8126, &[0.]);
        c.point_parameter(0x8127, &[maximum]);
        c.bind_buffer(GL_ARRAY_BUFFER, 9);
        let updated = [6i32 * 65536, 2 * 65536]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>();
        c.buffer_sub_data(GL_ARRAY_BUFFER, 0, &updated);
        c.bind_buffer(GL_ARRAY_BUFFER, 0);
        draw(c, &mut memory, GL_POINTS, 2);
        assert_eq!(count(c), 40);
        assert!(pixel(c, 5, 16)[0] > 0);
        assert_eq!(
            pixel(c, 26, 16),
            [0, 0, 0, if c.target.rgb565 { 255 } else { 0 }]
        );
        c.clear(GL_COLOR_BUFFER_BIT);
        c.draw_elements(&mut memory, GL_POINTS, &[1]);
        c.sync_gpu();
        assert_eq!(count(c), 4);
        assert_eq!(
            pixel(c, 8, 16),
            [0, 0, 0, if c.target.rgb565 { 255 } else { 0 }]
        );
        assert!(pixel(c, 24, 16)[0] > 0);
        c.delete_buffers(&[9]);
        assert_eq!(c.point_size_array.buffer, 0);
        c.set_point_size_pointer(GL_FLOAT, 0, 0xdead0000);
        memory = positions(&[-0.9, -0.9, 0., 0.9, -0.9, 0., 0., 0.9, 0.]);
        draw(c, &mut memory, GL_TRIANGLES, 3);
        assert!(
            pixel(c, 16, 16)[0] > 0,
            "triangles ignore point-size arrays"
        );
    }
    #[test]
    fn pointer_state_validation_and_per_unit_replacement_defaults() {
        let mut c = Context::new(32, 32);
        let before = c.point_size_array;
        assert_eq!(c.is_enabled(GL_POINT_SIZE_ARRAY_OES), Some(false));
        assert_eq!(c.is_enabled(GL_POINT_SPRITE_OES), Some(false));
        assert_eq!(c.get_pointer(GL_POINT_SIZE_ARRAY_POINTER_OES), Some(0));
        c.set_point_size_pointer(GL_UNSIGNED_BYTE, 0, 4096);
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
        assert_eq!(c.point_size_array, before);
        c.set_point_size_pointer(GL_FLOAT, u32::MAX, 4096);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        assert_eq!(c.point_size_array, before);
        c.tex_env(GL_COORD_REPLACE_OES, 1);
        c.set_active_texture(3);
        assert_eq!(
            c.get_tex_env(GL_POINT_SPRITE_OES, GL_COORD_REPLACE_OES)
                .unwrap()
                .numbers(),
            vec![0.]
        );
        c.tex_env(GL_COORD_REPLACE_OES, 2);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
    }
    #[test]
    fn software_point_arrays_and_sprite_coordinates() {
        for rgb565 in [false, true] {
            let mut c = if rgb565 {
                Context::new_rgb565(32, 32)
            } else {
                Context::new(32, 32)
            };
            sprite_cases(&mut c, 1);
            size_cases(&mut c);
        }
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires native OpenGL/GLES2; point sprites and batched point-size arrays"]
    fn native_point_arrays_and_sprite_coordinates() {
        for es in [false, true] {
            for scale in [1, 2] {
                for rgb565 in [false, true] {
                    let mut c = if rgb565 {
                        Context::new_rgb565(32, 32)
                    } else {
                        Context::new(32, 32)
                    };
                    c.gpu = Some(if es {
                        crate::gpu::Renderer::new_es_for_test(32, 32).unwrap()
                    } else {
                        crate::gpu::Renderer::new(32, 32).unwrap()
                    });
                    c.gpu.as_mut().unwrap().set_surface_rgb565(rgb565).unwrap();
                    c.es11.point_max = c.primitive_size_range(true)[1];
                    c.set_internal_resolution(scale as u8);
                    sprite_cases(&mut c, scale);
                    size_cases(&mut c);
                }
            }
        }
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires native OpenGL/GLES2; logical point limits at internal 2x"]
    fn native_point_limit_queries_match_scaled_rasterization() {
        for es in [false, true] {
            let mut c = Context::new(64, 64);
            let mut gpu = if es {
                crate::gpu::Renderer::new_es_for_test(64, 64).unwrap()
            } else {
                crate::gpu::Renderer::new(64, 64).unwrap()
            };
            let physical = (gpu.point_range[1].min(64.) / 4.).floor() * 4.;
            assert!(physical >= 4.);
            gpu.point_range[1] = physical;
            c.gpu = Some(gpu);
            c.set_internal_resolution(2);
            assert_eq!(c.primitive_size_range(true), [1., physical / 2.]);
            let mut memory = positions(&[0., 0., 0., 1000.]);
            c.set_vertex_pointer(3, GL_FLOAT, 0, 4096);
            c.set_client_state(GL_VERTEX_ARRAY, true);
            c.set_point_size(128.);
            for array in [false, true] {
                c.set_point_size_pointer(GL_FLOAT, 0, 4096 + 12);
                c.set_client_state(GL_POINT_SIZE_ARRAY_OES, array);
                draw(&mut c, &mut memory, GL_POINTS, 1);
                assert_eq!(
                    count(&c),
                    (physical as usize / 2).pow(2),
                    "array={array}: clamp to the advertised logical maximum"
                );
                assert_eq!(
                    c.state.point_size, 128.,
                    "raster clamping does not modify requested state"
                );
            }
        }
    }
}

#[cfg(test)]
mod texture_completeness_tests {
    use super::*;
    struct Memory(Vec<u8>);
    impl GuestMemory for Memory {
        fn read(&mut self, addr: u32, len: usize, out: &mut Vec<u8>) -> bool {
            if let Some(p) = self.0.get(addr as usize..addr as usize + len) {
                out.clear();
                out.extend_from_slice(p);
                true
            } else {
                false
            }
        }
    }
    fn exercise(c: &mut Context) {
        let mut bytes = vec![0; 16];
        for v in [-1f32, -1., 1., -1., 1., 1., -1., 1.] {
            bytes.extend(v.to_le_bytes());
        }
        let mut memory = Memory(bytes);
        c.set_vertex_pointer(2, GL_FLOAT, 0, 16);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        c.current_color = [0., 1., 0., 1.];
        c.set_capability(GL_TEXTURE_2D, true);
        c.tex_env(GL_TEXTURE_ENV_MODE, GL_REPLACE);
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            4,
            2,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[255, 0, 0, 255].repeat(8),
        );
        let draw = |c: &mut Context, m: &mut Memory, expected: [u8; 4]| {
            c.clear(GL_COLOR_BUFFER_BIT);
            c.draw_arrays(m, GL_TRIANGLE_FAN, 0, 4);
            c.sync_gpu();
            let offset = (8 * 16 + 8) * 4;
            assert_eq!(&c.target.color[offset..offset + 4], &expected);
        };
        assert_eq!(c.texture_stages().1, 0);
        draw(c, &mut memory, [0, 255, 0, 255]);
        c.tex_image_2d(
            GL_TEXTURE_2D,
            1,
            2,
            1,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[255, 0, 0, 255].repeat(2),
        );
        draw(c, &mut memory, [0, 255, 0, 255]);
        c.tex_image_2d(
            GL_TEXTURE_2D,
            2,
            1,
            1,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[255, 0, 0, 255],
        );
        assert_eq!(c.texture_stages().1, 1);
        draw(c, &mut memory, [255, 0, 0, 255]);
        c.tex_image_2d(
            GL_TEXTURE_2D,
            1,
            2,
            1,
            GL_RGB,
            GL_UNSIGNED_BYTE,
            &[255, 0, 0].repeat(2),
        );
        draw(c, &mut memory, [0, 255, 0, 255]);
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
        draw(c, &mut memory, [255, 0, 0, 255]);
        c.tex_parameter(
            GL_TEXTURE_2D,
            GL_TEXTURE_MIN_FILTER,
            GL_NEAREST_MIPMAP_LINEAR,
        );
        draw(c, &mut memory, [0, 255, 0, 255]);
        c.tex_parameter(GL_TEXTURE_2D, 0x8191, 1);
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            4,
            2,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[0, 0, 255, 255].repeat(8),
        );
        draw(c, &mut memory, [0, 0, 255, 255]);
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }
    #[test]
    fn software_incomplete_units_and_filter_transitions() {
        exercise(&mut Context::new_rgb565(16, 16));
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires native desktop GL and GLES2; completeness and cache transitions"]
    fn native_incomplete_units_and_filter_transitions() {
        for es in [false, true] {
            for scale in [1, 2] {
                let mut c = Context::new_rgb565(16, 16);
                if es {
                    let mut gpu = crate::gpu::Renderer::new_es_for_test(16, 16).unwrap();
                    gpu.set_surface_rgb565(true).unwrap();
                    c.gpu = Some(gpu);
                } else {
                    c.enable_gpu().unwrap();
                }
                c.set_internal_resolution(scale);
                exercise(&mut c);
            }
        }
    }
}

#[cfg(test)]
mod image_validation_tests {
    use super::*;
    #[test]
    fn null_uploads_validate_all_parameters_and_invalid_calls_are_atomic() {
        let mut c = Context::new(4, 4);
        c.tex_image_2d(GL_TEXTURE_2D, 0, 2, 2, GL_RGBA, GL_UNSIGNED_BYTE, &[23; 16]);
        let saved = c.textures[&0].clone();
        for (target, level, internal, w, h, border, format, ty, error) in [
            (
                0,
                0,
                GL_RGBA,
                2,
                2,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                GL_INVALID_ENUM,
            ),
            (
                GL_TEXTURE_2D,
                0,
                4,
                2,
                2,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                GL_INVALID_VALUE,
            ),
            (
                GL_TEXTURE_2D,
                0,
                GL_RGBA,
                2,
                2,
                1,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                GL_INVALID_VALUE,
            ),
            (
                GL_TEXTURE_2D,
                0,
                GL_RGBA,
                3,
                2,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                GL_INVALID_VALUE,
            ),
            (
                GL_TEXTURE_2D,
                -1,
                GL_RGBA,
                2,
                2,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                GL_INVALID_VALUE,
            ),
            (
                GL_TEXTURE_2D,
                11,
                GL_RGBA,
                1,
                1,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                GL_INVALID_VALUE,
            ),
            (
                GL_TEXTURE_2D,
                1,
                GL_RGBA,
                1024,
                1,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                GL_INVALID_VALUE,
            ),
            (
                GL_TEXTURE_2D,
                0,
                GL_RGBA,
                2,
                2,
                0,
                0xdead,
                GL_UNSIGNED_BYTE,
                GL_INVALID_ENUM,
            ),
            (
                GL_TEXTURE_2D,
                0,
                GL_RGBA,
                2,
                2,
                0,
                GL_RGBA,
                0xdead,
                GL_INVALID_ENUM,
            ),
            (
                GL_TEXTURE_2D,
                0,
                GL_RGBA,
                2,
                2,
                0,
                GL_RGBA,
                GL_UNSIGNED_SHORT_5_6_5,
                GL_INVALID_OPERATION,
            ),
            (
                GL_TEXTURE_2D,
                0,
                GL_RGB,
                2,
                2,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                GL_INVALID_OPERATION,
            ),
        ] {
            assert!(!c.validate_tex_image_2d(target, level, internal, w, h, border, format, ty));
            assert_eq!(c.take_error(), error);
            assert_eq!(c.textures[&0].revision, saved.revision);
            assert_eq!(c.textures[&0].rgba, saved.rgba);
        }
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            2,
            2,
            GL_RGBA,
            GL_UNSIGNED_SHORT_5_6_5,
            &[],
        );
        assert_eq!(c.take_error(), GL_INVALID_OPERATION);
        assert_eq!(c.textures[&0].rgba, saved.rgba);
        c.tex_image_2d(GL_TEXTURE_2D, 0, 0, 4, GL_RGBA, GL_UNSIGNED_BYTE, &[]);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        assert_eq!((c.textures[&0].width, c.textures[&0].height), (0, 4));
        assert!(!c.textures[&0].is_complete());
    }
    #[test]
    fn subimage_bounds_format_and_empty_rectangles_preserve_storage() {
        let mut c = Context::new(4, 4);
        c.tex_sub_image_2d(GL_TEXTURE_2D, 0, 0, 0, 0, 0, GL_RGBA, GL_UNSIGNED_BYTE, &[]);
        assert_eq!(c.take_error(), GL_INVALID_OPERATION);
        c.tex_image_2d(GL_TEXTURE_2D, 0, 4, 2, GL_RGB, GL_UNSIGNED_BYTE, &[20; 24]);
        let saved = c.textures[&0].clone();
        for (x, y, w, h, format, ty, error) in [
            (0, 0, 0, 0, 0xdead, GL_UNSIGNED_BYTE, GL_INVALID_ENUM),
            (
                0,
                0,
                0,
                0,
                GL_RGB,
                GL_UNSIGNED_SHORT_4_4_4_4,
                GL_INVALID_OPERATION,
            ),
            (0, 0, 0, 0, GL_RGBA, GL_UNSIGNED_BYTE, GL_INVALID_OPERATION),
            (
                u32::MAX,
                0,
                1,
                1,
                GL_RGB,
                GL_UNSIGNED_BYTE,
                GL_INVALID_VALUE,
            ),
            (3, 0, 2, 1, GL_RGB, GL_UNSIGNED_BYTE, GL_INVALID_VALUE),
            (0, 2, 1, 1, GL_RGB, GL_UNSIGNED_BYTE, GL_INVALID_VALUE),
        ] {
            c.tex_sub_image_2d(GL_TEXTURE_2D, 0, x, y, w, h, format, ty, &[]);
            assert_eq!(c.take_error(), error);
            assert_eq!(c.textures[&0].rgba, saved.rgba);
            assert_eq!(c.textures[&0].revision, saved.revision);
        }
        c.tex_sub_image_2d(GL_TEXTURE_2D, 0, 4, 2, 0, 0, GL_RGB, GL_UNSIGNED_BYTE, &[]);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        assert_eq!(c.textures[&0].revision, saved.revision);
        // Subimages may be non-power-of-two even though complete images may not.
        c.tex_sub_image_2d(
            GL_TEXTURE_2D,
            0,
            1,
            0,
            3,
            1,
            GL_RGB,
            GL_UNSIGNED_BYTE,
            &[100; 9],
        );
        assert_eq!(c.take_error(), GL_NO_ERROR);
        assert_eq!(&c.textures[&0].rgba[4..16], &[100, 100, 100, 255].repeat(3));
    }
    #[test]
    fn invalid_copy_image_does_not_read_back_or_change_texture() {
        let mut c = Context::new(4, 4);
        c.tex_image_2d(GL_TEXTURE_2D, 0, 2, 2, GL_RGBA, GL_UNSIGNED_BYTE, &[71; 16]);
        let revision = c.textures[&0].revision;
        c.copy_tex_image_2d(GL_TEXTURE_2D, 0, GL_RGBA, 0, 0, 3, 2, 0);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        assert_eq!(c.textures[&0].revision, revision);
        assert_eq!(c.textures[&0].rgba, [71; 16]);
    }
}

#[cfg(test)]
mod window_depth_discard_tests {
    use super::*;
    fn exercise(c: &mut Context) {
        let vertices = |z: f32, color: [f32; 4]| {
            [[-1., -1., z, 1.], [1., -1., z, 1.], [0., 1., z, 1.]].map(|pos| raster::Vertex {
                pos,
                color,
                back_color: color,
                ..Default::default()
            })
        };
        c.state.depth_test = true;
        c.state.depth_func = CompareFunc::Lequal;
        c.clear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);
        let near = vertices(-0.5, [1., 0., 0., 1.]);
        if let Some(g) = c.gpu.as_mut() {
            g.draw(
                &c.target,
                &c.state,
                GL_TRIANGLES,
                &near,
                &[],
                &HashMap::new(),
                [0; 6],
            )
            .unwrap();
        } else {
            raster::draw_triangle(&mut c.target, &c.state, &|_, _, _| None, &[], near);
        }
        c.sync_gpu();
        let original = c.target.color.clone();
        c.state.depth_write = false;
        c.state.scissor = Some((0, 0, 1, 1));
        c.state.color_mask = [false; 4];
        c.clear_depth = 0.25;
        let masks = c.state.color_mask;
        let scissor = c.state.scissor;
        c.discard_window_depth();
        c.sync_gpu();
        assert_eq!(c.target.color, original);
        assert_eq!(c.state.color_mask, masks);
        assert_eq!(c.state.scissor, scissor);
        assert!(!c.state.depth_write);
        assert_eq!(c.clear_depth, 0.25);
        // A guest clear remains masked. The fresh depth is from presentation,
        // not from overriding the semantics of that subsequent GL command.
        c.clear(GL_DEPTH_BUFFER_BIT);
        c.state.scissor = None;
        c.state.color_mask = [true; 4];
        let far = vertices(0.5, [0., 1., 0., 1.]);
        if let Some(g) = c.gpu.as_mut() {
            g.draw(
                &c.target,
                &c.state,
                GL_TRIANGLES,
                &far,
                &[],
                &HashMap::new(),
                [0; 6],
            )
            .unwrap();
        } else {
            raster::draw_triangle(&mut c.target, &c.state, &|_, _, _| None, &[], far);
        }
        c.sync_gpu();
        let at = (8 * 16 + 8) * 4;
        assert_eq!(&c.target.color[at..at + 4], &[0, 255, 0, 255]);
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }
    #[test]
    fn software_window_depth_discard_keeps_state_and_color() {
        exercise(&mut Context::new_rgb565(16, 16));
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires native desktop GL/GLES2; ancillary discard across window frames"]
    fn native_window_depth_discard_keeps_state_and_color() {
        for es in [false, true] {
            for scale in [1, 2] {
                let mut c = Context::new_rgb565(16, 16);
                if es {
                    let mut gpu = crate::gpu::Renderer::new_es_for_test(16, 16).unwrap();
                    gpu.set_surface_rgb565(true).unwrap();
                    c.gpu = Some(gpu);
                } else {
                    c.enable_gpu().unwrap();
                }
                c.set_internal_resolution(scale);
                exercise(&mut c);
            }
        }
    }
}

#[cfg(test)]
mod goforce_driver_tests {
    use super::*;
    fn exercise(c: &mut Context) {
        let tri = |z, color| {
            [[-1., -1., z, 1.], [1., -1., z, 1.], [0., 1., z, 1.]].map(|pos| raster::Vertex {
                pos,
                color,
                back_color: color,
                ..Default::default()
            })
        };
        let near = tri(-0.5, [1., 0., 0., 1.]);
        let far = tri(0.5, [0., 1., 0., 1.]);
        let draw = |c: &mut Context, v: [raster::Vertex; 3]| {
            if let Some(g) = c.gpu.as_mut() {
                g.draw(
                    &c.target,
                    &c.state,
                    GL_TRIANGLES,
                    &v,
                    &[],
                    &HashMap::new(),
                    [0; 6],
                )
                .unwrap();
            } else {
                raster::draw_triangle(&mut c.target, &c.state, &|_, _, _| None, &[], v);
            }
        };
        for (profile, clear_mask) in [false, true, false]
            .into_iter()
            .flat_map(|p| [GL_DEPTH_BUFFER_BIT, GL_STENCIL_BUFFER_BIT].map(|m| (p, m)))
        {
            c.gizmondo_goforce_20051013 = profile;
            c.state.depth_test = true;
            c.state.depth_func = CompareFunc::Lequal;
            c.state.depth_write = true;
            c.state.scissor = None;
            c.clear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);
            draw(c, near);
            c.state.depth_write = false;
            c.state.scissor = Some((0, 0, 8, 16));
            c.clear(clear_mask);
            assert!(!c.state.depth_write);
            assert_eq!(c.state.scissor, Some((0, 0, 8, 16)));
            c.state.scissor = None;
            draw(c, far);
            c.sync_gpu();
            let at = |x: usize| &c.target.color[(8 * 16 + x) * 4..(8 * 16 + x) * 4 + 4];
            assert_eq!(
                at(6),
                if profile {
                    &[0, 255, 0, 255]
                } else {
                    &[255, 0, 0, 255]
                }
            );
            assert_eq!(at(9), &[255, 0, 0, 255]);
            c.clear(GL_DEPTH_BUFFER_BIT | 1);
            assert_eq!(c.take_error(), GL_INVALID_VALUE);
        }
    }
    #[test]
    fn software_dated_driver_clear_is_distinct_from_standard_es() {
        exercise(&mut Context::new_rgb565(16, 16));
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires native desktop GL/GLES2; NVIDIA masked depth clear and scissor"]
    fn native_dated_driver_clear_is_distinct_from_standard_es() {
        for es in [false, true] {
            for scale in [1, 2] {
                let mut c = Context::new_rgb565(16, 16);
                if es {
                    let mut gpu = crate::gpu::Renderer::new_es_for_test(16, 16).unwrap();
                    gpu.set_surface_rgb565(true).unwrap();
                    c.gpu = Some(gpu);
                } else {
                    c.enable_gpu().unwrap();
                }
                c.set_internal_resolution(scale);
                exercise(&mut c);
            }
        }
    }
}

#[cfg(test)]
mod compressed_validation_tests {
    use super::*;
    fn palette() -> Vec<u8> {
        let mut data = vec![0; 64];
        data[4..8].copy_from_slice(&[255, 0, 0, 255]);
        data[8..12].copy_from_slice(&[0, 255, 0, 255]);
        data[12..16].copy_from_slice(&[0, 0, 255, 255]);
        data.extend_from_slice(&[0x11; 8]);
        data.extend_from_slice(&[0x22; 2]);
        data.push(0x30);
        data
    }
    #[test]
    fn shared_palette_chain_is_atomic_and_preserves_explicit_mips() {
        for profile in [false, true] {
            let mut c = Context::new(8, 8);
            c.gizmondo_goforce_20051013 = profile;
            c.tex_image_2d(GL_TEXTURE_2D, 0, 4, 4, GL_RGBA, GL_UNSIGNED_BYTE, &[7; 64]);
            let revision = c.textures[&0].revision;
            let data = palette();
            for len in [74, 76] {
                let mut bad = data.clone();
                bad.resize(len, 0);
                c.compressed_tex_image_2d(GL_TEXTURE_2D, -2, 4, 4, GL_PALETTE4_RGBA8_OES, &bad);
                assert_eq!(c.take_error(), GL_INVALID_VALUE);
                assert_eq!(c.textures[&0].revision, revision);
                assert_eq!(c.textures[&0].rgba, vec![7; 64]);
            }
            c.tex_parameter(GL_TEXTURE_2D, 0x8191, 1);
            c.compressed_tex_image_2d(GL_TEXTURE_2D, -2, 4, 4, GL_PALETTE4_RGBA8_OES, &data);
            assert_eq!(c.take_error(), GL_NO_ERROR);
            let t = &c.textures[&0];
            assert!(t.is_complete());
            assert!(t.generate_mipmap);
            assert_eq!(t.rgba, [255, 0, 0, 255].repeat(16));
            assert_eq!(t.mipmaps[&1].2, [0, 255, 0, 255].repeat(4));
            assert_eq!(t.mipmaps[&2].2, [0, 0, 255, 255]);
            assert_eq!(t.formats[&0], GL_PALETTE4_RGBA8_OES);
            c.tex_parameter(GL_TEXTURE_2D, 0x8191, 1);
            assert_eq!(c.textures[&0].mipmaps[&2].2, [0, 0, 255, 255]);
            c.copy_tex_sub_image_2d(GL_TEXTURE_2D, 0, 0, 0, 0, 0, 1, 1);
            assert_eq!(c.take_error(), GL_INVALID_OPERATION);
        }
    }
    #[test]
    fn compressed_validation_handles_levels_dimensions_and_storage_only() {
        let mut c = Context::new(8, 8);
        for (level, w, h, format, border, error) in [
            (1, 4, 4, GL_PALETTE4_RGBA8_OES, 0, GL_INVALID_VALUE),
            (
                -1,
                4,
                4,
                GL_COMPRESSED_RGB_S3TC_DXT1_EXT,
                0,
                GL_INVALID_VALUE,
            ),
            (-3, 4, 4, GL_PALETTE4_RGBA8_OES, 0, GL_INVALID_VALUE),
            (
                0,
                3,
                4,
                GL_COMPRESSED_RGB_S3TC_DXT1_EXT,
                0,
                GL_INVALID_VALUE,
            ),
            (0, 4, 4, GL_PALETTE4_RGBA8_OES, 1, GL_INVALID_OPERATION),
            (
                0,
                4,
                4,
                GL_COMPRESSED_RGB_S3TC_DXT1_EXT,
                1,
                GL_INVALID_OPERATION,
            ),
            (i32::MIN, 4, 4, GL_PALETTE4_RGBA8_OES, 0, GL_INVALID_VALUE),
        ] {
            assert_eq!(
                c.validate_compressed_tex_image_2d(GL_TEXTURE_2D, level, w, h, format, border),
                None
            );
            assert_eq!(c.take_error(), error);
        }
        c.compressed_tex_image_2d(GL_TEXTURE_2D, -2, 4, 4, GL_PALETTE4_RGBA8_OES, &[]);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        assert!(c.textures[&0].is_complete());
        assert_eq!(
            c.validate_compressed_tex_image_2d(
                GL_TEXTURE_2D,
                0,
                0,
                4,
                GL_COMPRESSED_RGB_S3TC_DXT1_EXT,
                0
            ),
            Some(0)
        );
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires native GL/GLES2; shared palette mips and cache replacement"]
    fn native_palette_chain_survives_failed_replacement_and_respects_max_level() {
        for es in [false, true] {
            for scale in [1, 2] {
                let mut c = Context::new(8, 8);
                if es {
                    c.gpu = Some(crate::gpu::Renderer::new_es_for_test(8, 8).unwrap());
                } else {
                    c.enable_gpu().unwrap();
                }
                c.set_internal_resolution(scale);
                c.tex_parameter(
                    GL_TEXTURE_2D,
                    GL_TEXTURE_MIN_FILTER,
                    GL_NEAREST_MIPMAP_NEAREST,
                );
                c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
                c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MAX_LEVEL_SGIS, 1);
                let data = palette();
                c.compressed_tex_image_2d(GL_TEXTURE_2D, -2, 4, 4, GL_PALETTE4_RGBA8_OES, &data);
                let stages = [raster::TextureStage {
                    unit: 0,
                    env: TexEnvMode::Replace,
                    env_color: [0.; 4],
                    lod_bias: 0.,
                }];
                let tri = [[-1., -1., 0., 1.], [1., -1., 0., 1.], [0., 1., 0., 1.]].map(|pos| {
                    raster::Vertex {
                        pos,
                        ..Default::default()
                    }
                });
                for (minify, bad) in [(false, false), (true, false), (false, true), (true, true)] {
                    let mut tri = tri;
                    if minify {
                        for v in &mut tri {
                            v.texcoord[0] = [(v.pos[0] + 1.) * 64., (v.pos[1] + 1.) * 64., 0., 1.];
                        }
                    }
                    if bad {
                        c.compressed_tex_image_2d(
                            GL_TEXTURE_2D,
                            -2,
                            4,
                            4,
                            GL_PALETTE4_RGBA8_OES,
                            &data[..74],
                        );
                        assert_eq!(c.take_error(), GL_INVALID_VALUE);
                    }
                    c.gpu
                        .as_mut()
                        .unwrap()
                        .draw(
                            &c.target,
                            &c.state,
                            GL_TRIANGLES,
                            &tri,
                            &stages,
                            &c.textures,
                            [0; 6],
                        )
                        .unwrap();
                    c.sync_gpu();
                    assert_eq!(
                        &c.target.color[(4 * 8 + 3) * 4..(4 * 8 + 3) * 4 + 4],
                        if minify {
                            &[0, 255, 0, 255]
                        } else {
                            &[255, 0, 0, 255]
                        }
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod retrospective_texture_tests {
    use super::*;
    fn mip_colors(c: &mut Context) {
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_BASE_LEVEL_SGIS, 2);
        c.tex_parameter(
            GL_TEXTURE_2D,
            GL_TEXTURE_MIN_FILTER,
            GL_LINEAR_MIPMAP_LINEAR,
        );
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
        for (level, w, col) in [
            (2, 4, [255, 0, 0, 255]),
            (3, 2, [0, 255, 0, 255]),
            (4, 1, [0, 0, 255, 255]),
        ] {
            c.tex_image_2d(
                GL_TEXTURE_2D,
                level,
                w,
                w,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                &col.repeat((w * w) as usize),
            );
        }
        assert_eq!(c.take_error(), GL_NO_ERROR);
        assert!(c.textures[&0].is_complete());
    }
    #[test]
    fn base_levels_and_fractional_bounds_select_real_images_without_level_zero() {
        for profile in [false, true] {
            let mut c = Context::new(16, 16);
            c.gizmondo_goforce_20051013 = profile;
            mip_colors(&mut c);
            c.tex_parameter_float(GL_TEXTURE_2D, GL_TEXTURE_MIN_LOD_SGIS, 1.25);
            let t = &c.textures[&0];
            let levels = t.upload_levels();
            assert_eq!(
                levels.iter().map(|l| (l.0, l.1)).collect::<Vec<_>>(),
                [(4, 4), (2, 2), (1, 1)]
            );
            assert_eq!(
                t.sample_lod(&levels, 0.5, 0.5, -20.),
                if profile {
                    [0, 255, 0, 255]
                } else {
                    [0, 191, 64, 255]
                }
            );
            c.tex_parameter_float(GL_TEXTURE_2D, GL_TEXTURE_MAX_LOD_SGIS, 0.);
            assert_eq!(
                c.textures[&0].sample_lod(&levels, 0.5, 0.5, 20.),
                [255, 0, 0, 255]
            );
            c.tex_parameter_float(GL_TEXTURE_2D, GL_TEXTURE_BASE_LEVEL_SGIS, 1.9);
            assert_eq!(c.textures[&0].base_level, 2);
            if profile {
                c.tex_parameter_float(GL_TEXTURE_2D, GL_TEXTURE_MIN_LOD_SGIS, 0.4999999);
                assert_eq!(c.textures[&0].lod_bounds().0, 1.);
                c.tex_parameter_float(GL_TEXTURE_2D, GL_TEXTURE_MIN_LOD_SGIS, 1.25);
            }
            let before = c.textures[&0].base_level;
            c.tex_parameter_float(GL_TEXTURE_2D, GL_TEXTURE_BASE_LEVEL_SGIS, -1.);
            assert_eq!(c.take_error(), GL_INVALID_VALUE);
            assert_eq!(c.textures[&0].base_level, before);
            c.tex_parameter_float(GL_TEXTURE_2D, GL_TEXTURE_MIN_LOD_SGIS, f32::NAN);
            assert_eq!(c.take_error(), GL_INVALID_VALUE);
            assert_eq!(c.textures[&0].min_lod, 1.25);
        }
    }
    #[test]
    fn legacy_clamp_is_driver_only_and_keeps_its_query_identity() {
        let mut c = Context::new(4, 4);
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, 0x2900);
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
        c.gizmondo_goforce_20051013 = true;
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, 0x2900);
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            2,
            1,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[255, 0, 0, 255, 0, 255, 0, 255],
        );
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
        assert_eq!(
            c.get_tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S)
                .unwrap()
                .numbers(),
            [0x2900 as f64]
        );
        assert_eq!(c.textures[&0].sample_linear(-5., 0.5), [255, 0, 0, 255]);
        assert_eq!(c.textures[&0].sample_linear(5., 0.5), [0, 255, 0, 255]);
    }
    #[test]
    fn generated_mipmaps_follow_the_selected_base_and_keep_internal_formats() {
        let mut c = Context::new(4, 4);
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_BASE_LEVEL_SGIS, 2);
        c.tex_parameter(GL_TEXTURE_2D, 0x8191, 1);
        c.tex_image_2d(
            GL_TEXTURE_2D,
            2,
            4,
            4,
            GL_RGB,
            GL_UNSIGNED_BYTE,
            &[255, 0, 0].repeat(16),
        );
        assert_eq!(c.take_error(), GL_NO_ERROR);
        let t = &c.textures[&0];
        assert!(t.is_complete());
        assert!(t.rgba.is_empty());
        assert_eq!(t.mipmaps[&4].2, [255, 0, 0, 255]);
        assert_eq!(t.formats[&4], GL_RGB);
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            1,
            1,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[0, 255, 0, 255],
        );
        assert_eq!(c.textures[&0].mipmaps[&4].2, [255, 0, 0, 255]);
    }
    #[test]
    fn software_draw_uses_nonzero_base_cache_and_live_lod_limits() {
        struct Memory(Vec<u8>);
        impl GuestMemory for Memory {
            fn read(&mut self, addr: u32, len: usize, out: &mut Vec<u8>) -> bool {
                let Some(bytes) = self.0.get(addr as usize..addr as usize + len) else {
                    return false;
                };
                out.clear();
                out.extend_from_slice(bytes);
                true
            }
        }
        let mut c = Context::new(16, 16);
        mip_colors(&mut c);
        c.texture_units[0].texture_enabled = true;
        c.texture_units[0].tex_env = TexEnvMode::Replace;
        c.vertex_array = ArrayPointer {
            enabled: true,
            size: 3,
            ty: GL_FLOAT,
            stride: 0,
            pointer: 16,
            buffer: 0,
        };
        let mut bytes = vec![0; 16];
        for v in [-1f32, -1., 0., 1., -1., 0., 0., 1., 0.] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let mut memory = Memory(bytes);
        c.tex_parameter_float(GL_TEXTURE_2D, GL_TEXTURE_MIN_LOD_SGIS, 1.25);
        c.draw_arrays(&mut memory, GL_TRIANGLES, 0, 3);
        assert_eq!(
            &c.target.color[(8 * 16 + 7) * 4..(8 * 16 + 7) * 4 + 4],
            &[0, 191, 64, 255]
        );
        c.tex_parameter_float(GL_TEXTURE_2D, GL_TEXTURE_MAX_LOD_SGIS, 0.);
        c.draw_arrays(&mut memory, GL_TRIANGLES, 0, 3);
        assert_eq!(
            &c.target.color[(8 * 16 + 7) * 4..(8 * 16 + 7) * 4 + 4],
            &[255, 0, 0, 255]
        );
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires native GL/GLES2; base remapping, exact fractional LOD and shader restoration"]
    fn native_base_bounds_clamp_and_dxt_alpha_match_software() {
        for es in [false, true] {
            for scale in [1, 2] {
                let mut c = Context::new(16, 16);
                if es {
                    c.gpu = Some(crate::gpu::Renderer::new_es_for_test(16, 16).unwrap());
                } else {
                    c.enable_gpu().unwrap();
                }
                c.set_internal_resolution(scale);
                mip_colors(&mut c);
                let stage = [raster::TextureStage {
                    unit: 0,
                    env: TexEnvMode::Replace,
                    env_color: [0.; 4],
                    lod_bias: 0.,
                }];
                let draw = |c: &mut Context, footprint: f32, expected: [u8; 4]| {
                    let tri =
                        [[-1., -1., 0., 1.], [1., -1., 0., 1.], [0., 1., 0., 1.]].map(|pos| {
                            raster::Vertex {
                                pos,
                                texcoord: [[
                                    (pos[0] + 1.) * footprint,
                                    (pos[1] + 1.) * footprint,
                                    0.,
                                    1.,
                                ]; 6],
                                ..Default::default()
                            }
                        });
                    c.gpu
                        .as_mut()
                        .unwrap()
                        .draw(
                            &c.target,
                            &c.state,
                            GL_TRIANGLES,
                            &tri,
                            &stage,
                            &c.textures,
                            [0; 6],
                        )
                        .unwrap();
                    c.sync_gpu();
                    let pixel = &c.target.color[(8 * 16 + 7) * 4..(8 * 16 + 7) * 4 + 4];
                    for (a, b) in pixel.iter().zip(expected) {
                        assert!(
                            (*a as i32 - b as i32).abs() <= 1,
                            "es={es} scale={scale} {pixel:?} expected={expected:?}"
                        );
                    }
                };
                draw(&mut c, 0., [255, 0, 0, 255]);
                c.tex_parameter_float(GL_TEXTURE_2D, GL_TEXTURE_MIN_LOD_SGIS, 1.25);
                draw(&mut c, 0., [0, 191, 64, 255]);
                c.tex_parameter_float(GL_TEXTURE_2D, GL_TEXTURE_MIN_LOD_SGIS, -1000.);
                c.tex_parameter_float(GL_TEXTURE_2D, GL_TEXTURE_MAX_LOD_SGIS, 0.);
                draw(&mut c, 64., [255, 0, 0, 255]);
                c.tex_parameter_float(GL_TEXTURE_2D, GL_TEXTURE_MAX_LOD_SGIS, 1000.);
                draw(&mut c, 64., [0, 0, 255, 255]);
                c.gizmondo_goforce_20051013 = true;
                c.tex_parameter_float(GL_TEXTURE_2D, GL_TEXTURE_MIN_LOD_SGIS, 1.25);
                draw(&mut c, 0., [0, 255, 0, 255]);
                c.tex_parameter_float(GL_TEXTURE_2D, GL_TEXTURE_MIN_LOD_SGIS, -1000.);
                c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_BASE_LEVEL_SGIS, 0);
                c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
                c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, 0x2900);
                for (format, alpha) in [
                    (GL_COMPRESSED_RGB_S3TC_DXT1_EXT, 255),
                    (GL_COMPRESSED_RGBA_S3TC_DXT1_EXT, 0),
                ] {
                    c.compressed_tex_image_2d(
                        GL_TEXTURE_2D,
                        0,
                        4,
                        4,
                        format,
                        &[0, 0, 255, 255, 255, 255, 255, 255],
                    );
                    assert_eq!(c.textures[&0].rgba_level(0).unwrap()[3], alpha);
                    draw(&mut c, 0., [0, 0, 0, alpha]);
                }
                c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_BASE_LEVEL_SGIS, 2);
                c.compressed_tex_image_2d(
                    GL_TEXTURE_2D,
                    2,
                    4,
                    4,
                    GL_COMPRESSED_RGB_S3TC_DXT1_EXT,
                    &[0, 248, 0, 0, 0, 0, 0, 0],
                );
                draw(&mut c, 0., [255, 0, 0, 255]);
            }
        }
    }
}

#[cfg(test)]
mod array_validation_tests {
    use super::*;

    #[test]
    fn invalid_array_descriptors_preserve_captured_state() {
        let mut c = Context::new(8, 8);
        c.bind_buffer(GL_ARRAY_BUFFER, 7);
        c.vertex_array.enabled = true;
        c.set_vertex_pointer(3, GL_FLOAT, 12, 16);
        let saved = c.vertex_array;
        c.bind_buffer(GL_ARRAY_BUFFER, 8);
        for (size, ty, stride, error) in [
            (1, GL_FLOAT, 0, GL_INVALID_VALUE),
            (5, GL_FLOAT, 0, GL_INVALID_VALUE),
            (3, GL_UNSIGNED_BYTE, 0, GL_INVALID_ENUM),
            (3, GL_FLOAT, u32::MAX, GL_INVALID_VALUE),
        ] {
            c.set_vertex_pointer(size, ty, stride, 99);
            assert_eq!(c.take_error(), error);
            assert_eq!(c.vertex_array, saved);
        }
        let normal = c.normal_array;
        c.set_normal_pointer(GL_UNSIGNED_BYTE, 0, 99);
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
        assert_eq!(c.normal_array, normal);
        c.set_color_pointer(4, GL_UNSIGNED_BYTE, 0, 20);
        let color = c.color_array;
        for ty in [GL_BYTE, GL_SHORT, 0xdead] {
            c.set_color_pointer(4, ty, 0, 99);
            assert_eq!(c.take_error(), GL_INVALID_ENUM);
            assert_eq!(c.color_array, color);
        }
    }

    #[test]
    fn native_goforce_array_exceptions_are_profile_scoped() {
        let mut c = Context::new(8, 8);
        for profile in [false, true, false] {
            c.gizmondo_goforce_20051013 = profile;
            c.set_color_pointer(3, GL_FLOAT, 0, 16);
            assert_eq!(
                c.take_error(),
                if profile {
                    GL_NO_ERROR
                } else {
                    GL_INVALID_VALUE
                }
            );
            c.set_client_active_texture(2);
            c.set_texcoord_pointer(1, GL_SHORT, 0, 32);
            assert_eq!(
                c.take_error(),
                if profile {
                    GL_NO_ERROR
                } else {
                    GL_INVALID_VALUE
                }
            );
            if profile {
                assert_eq!(c.color_array.size, 3);
                assert_eq!(c.texture_units[2].texcoord_array.size, 1);
            }
            c.set_texcoord_pointer(2, GL_UNSIGNED_BYTE, 0, 32);
            assert_eq!(c.take_error(), GL_INVALID_ENUM);
        }
    }

    #[test]
    fn deletion_detaches_all_arrays_without_resetting_descriptors() {
        let mut c = Context::new(8, 8);
        c.bind_buffer(GL_ARRAY_BUFFER, 9);
        c.bind_buffer(GL_ELEMENT_ARRAY_BUFFER, 9);
        c.set_vertex_pointer(3, GL_FLOAT, 12, 24);
        c.set_color_pointer(4, GL_UNSIGNED_BYTE, 4, 48);
        c.set_normal_pointer(GL_FIXED, 12, 64);
        c.set_point_size_pointer(GL_FLOAT, 4, 80);
        for unit in 0..MAX_TEXTURE_UNITS as u32 {
            c.set_client_active_texture(unit);
            c.set_texcoord_pointer(2, GL_FLOAT, 8, 96 + unit * 8);
        }
        c.delete_buffers(&[0, 99, 9, 9]);
        assert_eq!(c.array_buffer, 0);
        assert_eq!(c.element_array_buffer, 0);
        for a in [
            c.vertex_array,
            c.normal_array,
            c.color_array,
            c.point_size_array,
        ] {
            assert_eq!(a.buffer, 0);
            assert_ne!(a.pointer, 0);
        }
        for unit in &c.texture_units {
            assert_eq!(unit.texcoord_array.buffer, 0);
        }
        c.bind_buffer(GL_ARRAY_BUFFER, 9);
        assert!(c.buffers[&9].data.is_empty());
        assert_eq!(
            c.vertex_array.buffer, 0,
            "name reuse must not recapture old arrays"
        );
    }

    #[test]
    fn invalid_buffer_updates_preserve_store_and_usage() {
        let mut c = Context::new(8, 8);
        c.bind_buffer(GL_ARRAY_BUFFER, 1);
        c.buffer_data(GL_ARRAY_BUFFER, 4, Some(&[1, 2, 3, 4]), GL_DYNAMIC_DRAW);
        c.buffer_data(GL_ARRAY_BUFFER, 8, None, 0x88e0);
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
        c.buffer_sub_data(0xdead, 0, &[9]);
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
        for (offset, bytes) in [(u32::MAX, &[9][..]), (3, &[9, 8][..])] {
            c.buffer_sub_data(GL_ARRAY_BUFFER, offset, bytes);
            assert_eq!(c.take_error(), GL_INVALID_VALUE);
        }
        assert_eq!(c.buffers[&1].data, [1, 2, 3, 4]);
        assert_eq!(c.buffers[&1].usage, GL_DYNAMIC_DRAW);
        c.buffer_sub_data(GL_ARRAY_BUFFER, 4, &[]);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        c.buffer_sub_data(GL_ARRAY_BUFFER, 5, &[]);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
    }

    struct NoRead;
    impl GuestMemory for NoRead {
        fn read(&mut self, _: u32, _: usize, _: &mut Vec<u8>) -> bool {
            panic!("invalid or empty draw must not read guest memory")
        }
    }
    #[test]
    fn invalid_and_empty_draws_are_checked_before_memory_access() {
        let mut c = Context::new(8, 8);
        c.vertex_array.enabled = true;
        c.set_vertex_pointer(3, GL_FLOAT, 0, 16);
        c.draw_arrays(&mut NoRead, GL_TRIANGLES, 0, u32::MAX);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        c.draw_arrays(&mut NoRead, 0xdead, 0, 0);
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
        c.draw_arrays(&mut NoRead, GL_TRIANGLES, u32::MAX, 1);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        c.draw_elements_from_guest(&mut NoRead, GL_TRIANGLES, u32::MAX, GL_UNSIGNED_SHORT, 0);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        c.draw_elements_from_guest(&mut NoRead, 0xdead, 1, GL_UNSIGNED_SHORT, 0);
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
        c.draw_elements_from_guest(&mut NoRead, GL_TRIANGLES, 0, GL_UNSIGNED_SHORT, 0);
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }
}

#[cfg(test)]
mod dxt5_update_tests {
    use super::*;
    #[test]
    fn block_update_small_mips_and_rejected_calls_preserve_storage() {
        let mut c = Context::new(8, 8);
        let fmt = GL_COMPRESSED_RGBA_S3TC_DXT5_EXT;
        let mut block = [0u8; 16];
        block[0] = 255;
        block[8..10].copy_from_slice(&0xf800u16.to_le_bytes());
        c.compressed_tex_image_2d(GL_TEXTURE_2D, 0, 8, 4, fmt, &[block, block].concat());
        let mut green = block;
        green[8..10].copy_from_slice(&0x07e0u16.to_le_bytes());
        c.compressed_tex_sub_image_2d(GL_TEXTURE_2D, 0, 4, 0, 4, 4, fmt, &green);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        let rgba = c.textures[&0].rgba_level(0).unwrap();
        assert_eq!(&rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&rgba[16..20], &[0, 255, 0, 255]);
        let revision = c.textures[&0].revision;
        for (x, w, format, data, error) in [
            (1, 4, fmt, &green[..], GL_INVALID_OPERATION),
            (4, 8, fmt, &green[..], GL_INVALID_VALUE),
            (
                0,
                4,
                GL_COMPRESSED_RGBA_S3TC_DXT3_EXT,
                &green[..],
                GL_INVALID_OPERATION,
            ),
            (0, 4, fmt, &green[..15], GL_INVALID_VALUE),
        ] {
            c.compressed_tex_sub_image_2d(GL_TEXTURE_2D, 0, x, 0, w, 4, format, data);
            assert_eq!(c.take_error(), error);
            assert_eq!(c.textures[&0].revision, revision);
        }
        c.compressed_tex_image_2d(GL_TEXTURE_2D, 1, 2, 2, fmt, &block);
        c.compressed_tex_sub_image_2d(GL_TEXTURE_2D, 1, 0, 0, 2, 2, fmt, &green);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        assert_eq!(
            c.textures[&0].rgba_level(1).unwrap().as_ref(),
            [0, 255, 0, 255].repeat(4)
        );
    }

    #[test]
    fn standard_dxt_errors_and_native_dxt1_border_are_distinct() {
        let mut c = Context::new(8, 8);
        for profile in [false, true, false] {
            c.gizmondo_goforce_20051013 = profile;
            assert_eq!(
                c.validate_compressed_tex_image_2d(
                    GL_TEXTURE_2D,
                    0,
                    4,
                    4,
                    GL_COMPRESSED_RGB_S3TC_DXT1_EXT,
                    1
                ),
                None
            );
            assert_eq!(
                c.take_error(),
                if profile {
                    GL_INVALID_VALUE
                } else {
                    GL_INVALID_OPERATION
                }
            );
            for fmt in GL_COMPRESSED_RGB_S3TC_DXT1_EXT..=GL_COMPRESSED_RGBA_S3TC_DXT5_EXT {
                let error = if profile && fmt > GL_COMPRESSED_RGBA_S3TC_DXT1_EXT {
                    GL_INVALID_VALUE
                } else {
                    GL_INVALID_OPERATION
                };
                assert!(!c.validate_tex_image_2d(
                    GL_TEXTURE_2D,
                    0,
                    fmt,
                    4,
                    4,
                    0,
                    GL_RGBA,
                    GL_UNSIGNED_BYTE
                ));
                assert_eq!(c.take_error(), error);
                c.copy_tex_image_2d(GL_TEXTURE_2D, 0, fmt, 0, 0, 4, 4, 0);
                assert_eq!(c.take_error(), error);
            }
        }
    }
}
