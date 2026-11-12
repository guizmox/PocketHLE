//! Native GLES calls and bounded copies from the guest address space.
use std::collections::{HashMap, HashSet};
use std::ffi::{c_char, c_void, CString};

use pocket_gles::gpu::{
    configured_shared_context_source, ProgrammableFrameExport, ProgrammableHost,
    SharedContextSource, SharedFrame,
};

use pocket_gles::GL_TEXTURE0;

use super::{error, CallCtx, Result};

const FRAMEBUFFER: u32 = 0x8d40;
const RENDERBUFFER: u32 = 0x8d41;
const TEXTURE_2D: u32 = 0x0de1;
const RGBA: u32 = 0x1908;
const UNSIGNED_BYTE: u32 = 0x1401;
const ARRAY_BUFFER: u32 = 0x8892;
const ELEMENT_BUFFER: u32 = 0x8893;
const MAX_TRANSFER: usize = 64 * 1024 * 1024;

macro_rules! invoke {
    ($renderer:expr, $name:expr, ($($arg:expr => $ty:ty),* $(,)?), $ret:ty) => {{
        let function: unsafe extern "system" fn($($ty),*) -> $ret =
            unsafe { std::mem::transmute($renderer.symbol($name)?) };
        unsafe { function($($arg),*) }
    }};
}

#[derive(Clone, Copy)]
struct ClientAttribute {
    address: u32,
    size: i32,
    kind: u32,
    normalized: u8,
    stride: i32,
    bytes_per_vertex: usize,
}

#[derive(Default)]
struct StagedBuffer {
    name: u32,
    uploaded: Vec<u8>,
}

/// One contiguous upload minimizes both bytes and GL calls for a reused buffer.
fn changed_range(previous: &[u8], current: &[u8]) -> Option<std::ops::Range<usize>> {
    let previous = &previous[..current.len()];
    let start = previous.iter().zip(current).position(|(a, b)| a != b)?;
    let end = previous.iter().zip(current).rposition(|(a, b)| a != b)? + 1;
    Some(start..end)
}

pub(super) struct Renderer {
    host: ProgrammableHost,
    symbols: HashMap<String, *const c_void>,
    width: u32,
    height: u32,
    default_fbo: u32,
    window_color: u32,
    window_depth_stencil: u32,
    framebuffer: u32,
    array: u32,
    elements: u32,
    client_indices: StagedBuffer,
    client_vertices: HashMap<u32, StagedBuffer>,
    guest_buffers: HashSet<u32>,
    index_data: HashMap<u32, Vec<u8>>,
    client_attributes: HashMap<u32, ClientAttribute>,
    enabled_attributes: HashSet<u32>,
    max_attributes: u32,
    active_texture: u32,
    texture_units: u32,
    configured_attributes: HashSet<u32>,
    unpack: usize,
    pack: usize,
    guest_error: u32,
    native_compressed_formats: Vec<i32>,
    guest_textures: HashSet<u32>,
    guest_renderbuffers: HashSet<u32>,
    guest_framebuffers: HashSet<u32>,
    desktop_shader_sources: HashMap<u32, Vec<u8>>,
    deleted_shaders: HashSet<u32>,
    export: Option<ProgrammableFrameExport>,
    export_failed: bool,
    readback: Vec<u8>,
}

impl Renderer {
    fn symbol(&mut self, name: &str) -> Result<*const c_void> {
        if let Some(&address) = self.symbols.get(name) {
            return Ok(address);
        }
        let c_name = CString::new(name).map_err(|_| error("Invalid GL symbol"))?;
        let address = self.host.load(&c_name).cast_const();
        if address.is_null() {
            return Err(error(format!("Host GL has no {name}")));
        }
        self.symbols.insert(name.to_owned(), address);
        Ok(address)
    }

    pub(super) fn new(width: u32, height: u32) -> Result<Self> {
        let bytes = width as usize * height as usize * 4;
        if width == 0 || height == 0 || bytes > MAX_TRANSFER {
            return Err(error("Invalid GLES2 framebuffer size"));
        }
        let host = configured_shared_context_source()
            .and_then(|source| match ProgrammableHost::shared(&source) {
                Ok(host) => Some(host),
                Err(error) => {
                    log::warn!("GLES2 shared context unavailable: {error}");
                    None
                }
            })
            .map_or_else(ProgrammableHost::new_programmable, Ok)
            .map_err(error)?;
        let mut renderer = Self {
            host,
            symbols: HashMap::new(),
            width,
            height,
            default_fbo: 0,
            window_color: 0,
            window_depth_stencil: 0,
            framebuffer: 0,
            array: 0,
            elements: 0,
            client_indices: StagedBuffer::default(),
            client_vertices: HashMap::new(),
            guest_buffers: HashSet::new(),
            index_data: HashMap::new(),
            client_attributes: HashMap::new(),
            enabled_attributes: HashSet::new(),
            max_attributes: 0,
            active_texture: GL_TEXTURE0,
            texture_units: 0,
            configured_attributes: HashSet::new(),
            unpack: 4,
            pack: 4,
            guest_error: 0,
            native_compressed_formats: Vec::new(),
            guest_textures: HashSet::new(),
            guest_renderbuffers: HashSet::new(),
            guest_framebuffers: HashSet::new(),
            desktop_shader_sources: HashMap::new(),
            deleted_shaders: HashSet::new(),
            export: None,
            export_failed: false,
            readback: Vec::new(),
        };
        let mut fbo = 0;
        let mut color = 0;
        let mut depth_stencil = 0;
        let r = &mut renderer;
        invoke!(r, "glGenFramebuffers", (1 => i32, &mut fbo => *mut u32), ());
        invoke!(r, "glBindFramebuffer", (FRAMEBUFFER => u32, fbo => u32), ());
        invoke!(r, "glGenTextures", (1 => i32, &mut color => *mut u32), ());
        invoke!(r, "glBindTexture", (TEXTURE_2D => u32, color => u32), ());
        invoke!(r, "glTexImage2D", (TEXTURE_2D => u32, 0 => i32, RGBA as i32 => i32,
            width as i32 => i32, height as i32 => i32, 0 => i32, RGBA => u32,
            UNSIGNED_BYTE => u32, std::ptr::null() => *const c_void), ());
        invoke!(r, "glFramebufferTexture2D", (FRAMEBUFFER => u32, 0x8ce0 => u32,
            TEXTURE_2D => u32, color => u32, 0 => i32), ());
        invoke!(r, "glGenRenderbuffers", (1 => i32, &mut depth_stencil => *mut u32), ());
        invoke!(r, "glBindRenderbuffer", (RENDERBUFFER => u32, depth_stencil => u32), ());
        invoke!(r, "glRenderbufferStorage", (RENDERBUFFER => u32, 0x88f0 => u32,
            width as i32 => i32, height as i32 => i32), ());
        for attachment in [0x8d00, 0x8d20] {
            invoke!(r, "glFramebufferRenderbuffer", (FRAMEBUFFER => u32, attachment => u32,
                RENDERBUFFER => u32, depth_stencil => u32), ());
        }
        let status = invoke!(r, "glCheckFramebufferStatus", (FRAMEBUFFER => u32), u32);
        if status != 0x8cd5 {
            return Err(error(format!(
                "GLES2 window framebuffer incomplete ({status:#x})"
            )));
        }
        invoke!(r, "glBindTexture", (TEXTURE_2D => u32, 0 => u32), ());
        invoke!(r, "glBindRenderbuffer", (RENDERBUFFER => u32, 0 => u32), ());
        invoke!(r, "glViewport", (0 => i32, 0 => i32, width as i32 => i32, height as i32 => i32), ());
        r.default_fbo = fbo;
        r.window_color = color;
        r.window_depth_stencil = depth_stencil;
        let mut max_attributes = 0i32;
        let mut texture_units = 0i32;
        invoke!(r, "glGetIntegerv", (0x8869 => u32, &mut max_attributes => *mut i32), ());
        invoke!(r, "glGetIntegerv", (0x8b4d => u32, &mut texture_units => *mut i32), ());
        r.max_attributes = max_attributes.clamp(0, 32) as u32;
        r.texture_units = texture_units.max(0) as u32;
        let mut count = 0i32;
        invoke!(r, "glGetIntegerv", (0x86a2 => u32, &mut count => *mut i32), ());
        if (1..=4096).contains(&count) {
            let mut formats = vec![0; count as usize];
            invoke!(r, "glGetIntegerv", (0x86a3 => u32, formats.as_mut_ptr() => *mut i32), ());
            r.native_compressed_formats = formats;
        }
        Ok(renderer)
    }

    fn snapshot(&mut self, source: &SharedContextSource) -> Option<SharedFrame> {
        if self.export_failed {
            return None;
        }
        let snapshot = (|| -> Result<Option<SharedFrame>> {
            // The transport checks native errors after copying. Preserve a
            // pending guest error before it can be mistaken for a copy failure.
            for _ in 0..8 {
                let pending = invoke!(self, "glGetError", (), u32);
                if pending == 0 {
                    break;
                }
                self.reject(pending);
            }
            let export = match self.export.as_mut() {
                Some(export) => export,
                None => self
                    .export
                    .insert(ProgrammableFrameExport::new(&self.host, source).map_err(error)?),
            };
            let fbo = self.default_fbo;
            let restore = if self.framebuffer == 0 {
                fbo
            } else {
                self.framebuffer
            };
            export
                .snapshot(&self.host, fbo, restore, (self.width, self.height))
                .map_err(error)
        })();
        match snapshot {
            Ok(frame) => frame,
            Err(error) => {
                log::warn!("GLES2 shared frame export unavailable: {error}");
                self.export_failed = true;
                None
            }
        }
    }

    pub(super) fn present(&mut self, ctx: &mut CallCtx<'_>) -> Result<()> {
        self.host.bind_client_api().map_err(error)?;
        self.host.make_current().map_err(error)?;
        ctx.kernel.framebuffer.resize(self.width, self.height);
        let shared = crate::gles::publish_shared_window(ctx, |source| self.snapshot(source))?;
        if !shared {
            self.readback_window(ctx)?;
        }
        crate::directshow::service(ctx)?;
        if ctx.kernel.fb_mapped
            && !ctx
                .cpu
                .deferred_memory_pending(pocket_kernel::SYNTHETIC_FRAMEBUFFER_BASE)
        {
            ctx.cpu.write_mem(
                pocket_kernel::SYNTHETIC_FRAMEBUFFER_BASE,
                &ctx.kernel.framebuffer.cpu().map_err(error)?.pixels,
            )?;
        }
        ctx.kernel.gx_last_pushed_counter = ctx.kernel.framebuffer.frame_counter;
        ctx.kernel.framebuffer.directdraw.last_present = Some(std::time::Instant::now());
        ctx.kernel.framebuffer.presentation_stats.record_present();
        Ok(())
    }

    fn readback_window(&mut self, ctx: &mut CallCtx<'_>) -> Result<()> {
        let default_fbo = self.default_fbo;
        let previous = if self.framebuffer == 0 {
            default_fbo
        } else {
            self.framebuffer
        };
        let (width, height) = (self.width, self.height);
        invoke!(self, "glBindFramebuffer", (FRAMEBUFFER => u32, default_fbo => u32), ());
        self.readback
            .resize(width as usize * height as usize * 4, 0);
        let pixels = self.readback.as_mut_ptr().cast();
        let pack = self.pack as i32;
        if pack > 4 {
            invoke!(self, "glPixelStorei", (0x0d05 => u32, 4 => i32), ());
        }
        invoke!(self, "glReadPixels", (0 => i32, 0 => i32, width as i32 => i32,
            height as i32 => i32, RGBA => u32, UNSIGNED_BYTE => u32, pixels => *mut c_void), ());
        if pack > 4 {
            invoke!(self, "glPixelStorei", (0x0d05 => u32, pack => i32), ());
        }
        invoke!(self, "glBindFramebuffer", (FRAMEBUFFER => u32, previous => u32), ());
        pocket_kernel::framebuffer::gapi_memory::discard(ctx.cpu)?;
        ctx.kernel.framebuffer.overwrite_rgb565(|rgb, native| {
            native.resize(self.readback.len(), 0);
            let stride = width as usize * 4;
            for (dest, source) in native
                .chunks_exact_mut(stride)
                .zip(self.readback.rchunks_exact(stride))
            {
                dest.copy_from_slice(source);
            }
            for (pixel, dest) in native.chunks_exact_mut(4).zip(rgb.chunks_exact_mut(2)) {
                pixel[3] = 255;
                let packed = ((pixel[0] as u16 & 0xf8) << 8)
                    | ((pixel[1] as u16 & 0xfc) << 3)
                    | (pixel[2] as u16 >> 3);
                dest.copy_from_slice(&packed.to_le_bytes());
            }
            Some((width, height))
        });
        ctx.kernel.framebuffer.observe_ips();
        Ok(())
    }

    /// Bind reusable storage and upload only the bytes that changed.
    fn upload_client_buffer(
        &mut self,
        target: u32,
        staging: &mut StagedBuffer,
        data: Vec<u8>,
    ) -> Result<()> {
        if staging.name == 0 {
            invoke!(self, "glGenBuffers", (1 => i32, &mut staging.name => *mut u32), ());
            if staging.name == 0 {
                return Err(error("Host GL could not allocate a client staging buffer"));
            }
        }
        invoke!(self, "glBindBuffer", (target => u32, staging.name => u32), ());
        if data.len() > staging.uploaded.len() {
            invoke!(self, "glBufferData", (target => u32, data.len() as isize => isize,
                data.as_ptr().cast() => *const c_void, 0x88e0 => u32), ());
            staging.uploaded = data;
        } else if let Some(range) = changed_range(&staging.uploaded, &data) {
            let bytes = &data[range.clone()];
            invoke!(self, "glBufferSubData", (target => u32, range.start as isize => isize,
                bytes.len() as isize => isize, bytes.as_ptr().cast() => *const c_void), ());
            staging.uploaded[range].copy_from_slice(bytes);
        }
        Ok(())
    }

    fn stage_attributes(&mut self, ctx: &mut CallCtx<'_>, last_vertex: usize) -> Result<()> {
        let active: Vec<_> = self
            .client_attributes
            .iter()
            .filter(|(index, _)| self.enabled_attributes.contains(index))
            .map(|(&index, &attribute)| (index, attribute))
            .collect();
        if active.is_empty() {
            return Ok(());
        }
        // Read guest memory before changing host bindings. Attribute buffers
        // remain alive until replaced or the context closes.
        let arrays = active
            .into_iter()
            .map(|(index, attribute)| {
                let stride = if attribute.stride == 0 {
                    attribute.bytes_per_vertex
                } else {
                    attribute.stride as usize
                };
                let size = last_vertex
                    .checked_mul(stride)
                    .and_then(|offset| offset.checked_add(attribute.bytes_per_vertex))
                    .filter(|&n| n <= MAX_TRANSFER)
                    .ok_or_else(|| error("GLES2 client attribute exceeds the supported size"))?;
                let mut data = ctx.cpu.read_mem(attribute.address, size as u32)?;
                // Upload only the bytes belonging to this attribute. Guest
                // interleaved layouts otherwise resend the same stride gaps
                // for every enabled attribute.
                if stride > attribute.bytes_per_vertex {
                    for vertex in 1..=last_vertex {
                        let source = vertex * stride;
                        let destination = vertex * attribute.bytes_per_vertex;
                        data.copy_within(source..source + attribute.bytes_per_vertex, destination);
                    }
                    data.truncate((last_vertex + 1) * attribute.bytes_per_vertex);
                } else if stride < attribute.bytes_per_vertex {
                    let packed = (last_vertex + 1)
                        .checked_mul(attribute.bytes_per_vertex)
                        .filter(|&bytes| bytes <= MAX_TRANSFER)
                        .ok_or_else(|| {
                            error("GLES2 packed vertex data exceeds the supported size")
                        })?;
                    let mut expanded = Vec::with_capacity(packed);
                    for vertex in 0..=last_vertex {
                        let source = vertex * stride;
                        expanded
                            .extend_from_slice(&data[source..source + attribute.bytes_per_vertex]);
                    }
                    data = expanded;
                }
                Ok((index, attribute, data))
            })
            .collect::<Result<Vec<_>>>()?;
        let upload = (|| {
            for (index, attribute, data) in arrays {
                let mut staging = self.client_vertices.remove(&index).unwrap_or_default();
                let result = self.upload_client_buffer(ARRAY_BUFFER, &mut staging, data);
                self.client_vertices.insert(index, staging);
                result?;
                invoke!(self, "glVertexAttribPointer", (index => u32, attribute.size => i32,
                    attribute.kind => u32, attribute.normalized => u8, 0 => i32,
                    std::ptr::null() => *const c_void), ());
            }
            Ok(())
        })();
        let bound = self.array;
        invoke!(self, "glBindBuffer", (ARRAY_BUFFER => u32, bound => u32), ());
        upload
    }

    pub(super) fn gl(&mut self, name: &str, ctx: &mut CallCtx<'_>) -> Result<u32> {
        self.host.make_programmable_current().map_err(error)?;
        macro_rules! u {
            ($index:expr) => {
                ctx.arg_u32($index)?
            };
        }
        macro_rules! i {
            ($index:expr) => {
                u!($index) as i32
            };
        }
        macro_rules! f {
            ($index:expr) => {
                f32::from_bits(u!($index))
            };
        }
        macro_rules! gl {
            ($($arg:expr => $ty:ty),* $(,)?) => {
                invoke!(self, name, ($($arg => $ty),*), ())
            };
        }
        match name {
            "glCreateProgram" => return Ok(invoke!(self, name, (), u32)),
            "glIsProgram" | "glIsShader" | "glIsEnabled" => {
                return Ok(u32::from(invoke!(self, name, (u!(0) => u32), u8)));
            }
            "glCreateShader" | "glCheckFramebufferStatus" => {
                return Ok(invoke!(self, name, (u!(0) => u32), u32));
            }
            "glIsBuffer" | "glIsFramebuffer" | "glIsRenderbuffer" | "glIsTexture" => {
                let id = u!(0);
                let objects = match name {
                    "glIsBuffer" => &self.guest_buffers,
                    "glIsFramebuffer" => &self.guest_framebuffers,
                    "glIsRenderbuffer" => &self.guest_renderbuffers,
                    _ => &self.guest_textures,
                };
                if !objects.contains(&id) {
                    return Ok(0);
                }
                return Ok(u32::from(invoke!(self, name, (id => u32), u8)));
            }
            "glGetString" => {
                let value = match u!(0) {
                    0x1f00 => "PocketHLE",
                    0x1f01 => "Adreno 200 (PocketHLE)",
                    0x1f02 => "OpenGL ES 2.0 PocketHLE",
                    0x1f03 => "",
                    0x8b8c => "OpenGL ES GLSL ES 1.00",
                    _ => {
                        self.reject(0x0500);
                        return Ok(0);
                    }
                };
                return crate::gles::intern_string(ctx, value);
            }
            "glGetError" => {
                let pending = std::mem::take(&mut self.guest_error);
                return Ok(if pending != 0 {
                    pending
                } else {
                    invoke!(self, name, (), u32)
                });
            }
            "glFinish" | "glFlush" => gl!(),
            "glClear" | "glGenerateMipmap" | "glCompileShader" | "glCullFace" | "glDepthFunc"
            | "glDisable" | "glEnable" | "glLinkProgram" | "glValidateProgram" | "glFrontFace"
            | "glBlendEquation" | "glStencilMask" => {
                gl!(u!(0) => u32)
            }
            "glActiveTexture" => {
                let unit = u!(0);
                if unit.wrapping_sub(GL_TEXTURE0) >= self.texture_units {
                    return Ok(self.reject(0x0500));
                }
                if self.active_texture != unit {
                    gl!(unit => u32);
                    self.active_texture = unit;
                }
            }
            "glEnableVertexAttribArray" | "glDisableVertexAttribArray" => {
                let index = u!(0);
                if index >= self.max_attributes {
                    return Ok(self.reject(0x0501));
                }
                let changed = if name == "glEnableVertexAttribArray" {
                    self.enabled_attributes.insert(index)
                } else {
                    self.enabled_attributes.remove(&index)
                };
                if changed {
                    gl!(index => u32);
                }
            }
            "glAttachShader"
            | "glBlendFunc"
            | "glBlendEquationSeparate"
            | "glStencilMaskSeparate" => {
                gl!(u!(0) => u32, u!(1) => u32);
            }
            "glDeleteShader" => {
                let shader = u!(0);
                gl!(shader => u32);
                if self.desktop_shader_sources.contains_key(&shader) {
                    self.deleted_shaders.insert(shader);
                    self.reap_shader_sources()?;
                }
            }
            "glDeleteProgram" | "glUseProgram" => {
                gl!(u!(0) => u32);
                self.reap_shader_sources()?;
            }
            "glDetachShader" => {
                gl!(u!(0) => u32, u!(1) => u32);
                self.reap_shader_sources()?;
            }
            "glBindTexture" => {
                let (target, texture) = (u!(0), u!(1));
                if texture == self.window_color {
                    return Ok(self.reject(0x0502));
                }
                gl!(target => u32, texture => u32);
                if texture != 0 && matches!(target, TEXTURE_2D | 0x8513) {
                    self.guest_textures.insert(texture);
                }
            }
            "glBindRenderbuffer" => {
                let (target, buffer) = (u!(0), u!(1));
                if buffer == self.window_depth_stencil {
                    return Ok(self.reject(0x0502));
                }
                gl!(target => u32, buffer => u32);
                if target == RENDERBUFFER && buffer != 0 {
                    self.guest_renderbuffers.insert(buffer);
                }
            }
            "glBlendColor" => gl!(f!(0) => f32, f!(1) => f32, f!(2) => f32, f!(3) => f32),
            "glBlendFuncSeparate" | "glStencilOpSeparate" => {
                gl!(u!(0) => u32, u!(1) => u32, u!(2) => u32, u!(3) => u32);
            }
            "glClearStencil" => gl!(i!(0) => i32),
            "glStencilFunc" => gl!(u!(0) => u32, i!(1) => i32, u!(2) => u32),
            "glStencilFuncSeparate" => gl!(u!(0) => u32, u!(1) => u32, i!(2) => i32, u!(3) => u32),
            "glStencilOp" => gl!(u!(0) => u32, u!(1) => u32, u!(2) => u32),
            "glHint" => {
                let target = u!(0);
                if target != 0x8192 {
                    // GL_GENERATE_MIPMAP_HINT
                    return Ok(self.reject(0x0500));
                }
                gl!(target => u32, u!(1) => u32);
            }
            "glLineWidth" => gl!(f!(0) => f32),
            "glPolygonOffset" => gl!(f!(0) => f32, f!(1) => f32),
            "glSampleCoverage" => gl!(f!(0) => f32, u!(1) as u8 => u8),
            "glBindBuffer" => {
                let (target, buffer) = (u!(0), u!(1));
                if !matches!(target, ARRAY_BUFFER | ELEMENT_BUFFER) {
                    return Ok(self.reject(0x0500));
                }
                if buffer != 0 && !self.guest_buffers.contains(&buffer) {
                    return Ok(self.reject(0x0502));
                }
                let previous = if target == ARRAY_BUFFER {
                    self.array
                } else {
                    self.elements
                };
                if previous != buffer {
                    gl!(target => u32, buffer => u32);
                    if target == ARRAY_BUFFER {
                        self.array = buffer;
                    } else {
                        self.elements = buffer;
                    }
                }
            }
            "glBindFramebuffer" => {
                let (target, framebuffer) = (u!(0), u!(1));
                if target == FRAMEBUFFER {
                    if framebuffer == self.default_fbo {
                        return Ok(self.reject(0x0502));
                    }
                    self.framebuffer = framebuffer;
                    if framebuffer != 0 {
                        self.guest_framebuffers.insert(framebuffer);
                    }
                }
                let default_fbo = self.default_fbo;
                gl!(target => u32, if framebuffer == 0 { default_fbo } else { framebuffer } => u32);
            }
            "glDepthMask" => gl!(u!(0) as u8 => u8),
            "glColorMask" => {
                gl!(u!(0) as u8 => u8, u!(1) as u8 => u8, u!(2) as u8 => u8, u!(3) as u8 => u8)
            }
            "glClearDepthf" if !self.host.es => {
                invoke!(self, "glClearDepth", (f!(0) as f64 => f64), ());
            }
            "glDepthRangef" if !self.host.es => {
                invoke!(self, "glDepthRange", (f!(0) as f64 => f64, f!(1) as f64 => f64), ());
            }
            "glClearDepthf" => gl!(f!(0) => f32),
            "glDepthRangef" => gl!(f!(0) => f32, f!(1) => f32),
            "glClearColor" => gl!(f!(0) => f32, f!(1) => f32, f!(2) => f32, f!(3) => f32),
            "glScissor" | "glViewport" => {
                gl!(i!(0) => i32, i!(1) => i32, i!(2) => i32, i!(3) => i32)
            }
            "glTexParameteri" => gl!(u!(0) => u32, u!(1) => u32, i!(2) => i32),
            "glTexParameterf" => gl!(u!(0) => u32, u!(1) => u32, f!(2) => f32),
            "glTexParameterfv"
            | "glTexParameteriv"
            | "glGetTexParameterfv"
            | "glGetTexParameteriv" => {
                let (target, pname, address) = (u!(0), u!(1), u!(2));
                if !matches!(target, TEXTURE_2D | 0x8513) || !matches!(pname, 0x2800..=0x2803) {
                    return Ok(self.reject(0x0500));
                }
                match name {
                    "glTexParameterfv" => {
                        let value = f32::from_bits(ctx.cpu.read_u32_le(address)?);
                        gl!(target => u32, pname => u32, &value => *const f32);
                    }
                    "glTexParameteriv" => {
                        let value = ctx.cpu.read_u32_le(address)? as i32;
                        gl!(target => u32, pname => u32, &value => *const i32);
                    }
                    "glGetTexParameterfv" => {
                        let mut value = 0.0f32;
                        gl!(target => u32, pname => u32, &mut value => *mut f32);
                        ctx.cpu.write_mem(address, &value.to_le_bytes())?;
                    }
                    _ => {
                        let mut value = 0i32;
                        gl!(target => u32, pname => u32, &mut value => *mut i32);
                        ctx.cpu.write_mem(address, &value.to_le_bytes())?;
                    }
                }
            }
            "glPixelStorei" => {
                let (parameter, alignment) = (u!(0), i!(1));
                if !matches!(alignment, 1 | 2 | 4 | 8) {
                    return Ok(self.reject(0x0501));
                }
                match parameter {
                    0x0d05 => self.pack = alignment as usize,
                    0x0cf5 => self.unpack = alignment as usize,
                    _ => return Ok(self.reject(0x0500)),
                }
                gl!(parameter => u32, alignment => i32);
            }
            "glRenderbufferStorage" => gl!(u!(0) => u32, u!(1) => u32, i!(2) => i32, i!(3) => i32),
            "glGetFramebufferAttachmentParameteriv" => {
                let (target, attachment, pname, address) = (u!(0), u!(1), u!(2), u!(3));
                if target != FRAMEBUFFER
                    || !matches!(attachment, 0x8ce0 | 0x8d00 | 0x8d20)
                    || !matches!(pname, 0x8cd0..=0x8cd3)
                {
                    return Ok(self.reject(0x0500));
                }
                if self.framebuffer == 0 {
                    return Ok(self.reject(0x0502));
                }
                let mut value = -1i32;
                gl!(target => u32, attachment => u32, pname => u32, &mut value => *mut i32);
                if value >= 0 {
                    ctx.cpu.write_mem(address, &value.to_le_bytes())?;
                }
            }
            "glGetRenderbufferParameteriv" => {
                let (target, pname, address) = (u!(0), u!(1), u!(2));
                if target != RENDERBUFFER || !matches!(pname, 0x8d42..=0x8d44 | 0x8d50..=0x8d55) {
                    return Ok(self.reject(0x0500));
                }
                let mut value = -1i32;
                gl!(target => u32, pname => u32, &mut value => *mut i32);
                if value >= 0 {
                    ctx.cpu.write_mem(address, &value.to_le_bytes())?;
                }
            }
            "glFramebufferTexture2D" => {
                if self.framebuffer == 0 {
                    return Ok(self.reject(0x0502));
                }
                gl!(u!(0) => u32, u!(1) => u32, u!(2) => u32, u!(3) => u32, i!(4) => i32);
            }
            "glFramebufferRenderbuffer" => {
                if self.framebuffer == 0 {
                    return Ok(self.reject(0x0502));
                }
                gl!(u!(0) => u32, u!(1) => u32, u!(2) => u32, u!(3) => u32);
            }
            "glGenBuffers" | "glGenFramebuffers" | "glGenRenderbuffers" | "glGenTextures" => {
                let count = i!(0);
                let mut names = vec![0u32; transfer_size(count, 4)? / 4];
                gl!(count => i32, names.as_mut_ptr() => *mut u32);
                let out = u!(1);
                write_words(ctx, out, &names)?;
                let objects = match name {
                    "glGenBuffers" => &mut self.guest_buffers,
                    "glGenFramebuffers" => &mut self.guest_framebuffers,
                    "glGenRenderbuffers" => &mut self.guest_renderbuffers,
                    _ => &mut self.guest_textures,
                };
                objects.extend(names.into_iter().filter(|&id| id != 0));
            }
            "glDeleteBuffers"
            | "glDeleteFramebuffers"
            | "glDeleteRenderbuffers"
            | "glDeleteTextures" => {
                let count = i!(0);
                let address = u!(1);
                let mut names = read_words(ctx, address, transfer_size(count, 4)? / 4)?;
                let objects = match name {
                    "glDeleteBuffers" => &mut self.guest_buffers,
                    "glDeleteFramebuffers" => &mut self.guest_framebuffers,
                    "glDeleteRenderbuffers" => &mut self.guest_renderbuffers,
                    _ => &mut self.guest_textures,
                };
                names.retain(|id| objects.remove(id));
                gl!(names.len() as i32 => i32, names.as_ptr() => *const u32);
                if name == "glDeleteBuffers" {
                    for id in &names {
                        self.index_data.remove(id);
                    }
                    if names.contains(&self.array) {
                        self.array = 0;
                    }
                    if names.contains(&self.elements) {
                        self.elements = 0;
                    }
                }
                if name == "glDeleteFramebuffers" && names.contains(&self.framebuffer) {
                    self.framebuffer = 0;
                    let default_fbo = self.default_fbo;
                    invoke!(self, "glBindFramebuffer", (FRAMEBUFFER => u32, default_fbo => u32), ());
                }
            }
            "glBufferData" => {
                let (target, size, address, usage) = (u!(0), i!(1), u!(2), u!(3));
                let bytes = transfer_size(size, 1)?;
                let data = optional_bytes(ctx, address, bytes)?;
                gl!(target => u32, size as isize => isize, data_pointer(&data) => *const c_void, usage => u32);
                let index = if target == ELEMENT_BUFFER {
                    self.elements
                } else if target == ARRAY_BUFFER && self.index_data.contains_key(&self.array) {
                    self.array
                } else {
                    0
                };
                if index != 0 {
                    self.index_data
                        .insert(index, data.unwrap_or_else(|| vec![0; bytes]));
                }
            }
            "glGetBufferParameteriv" => {
                let (target, pname, out) = (u!(0), u!(1), u!(2));
                let bound = match target {
                    ARRAY_BUFFER => self.array,
                    ELEMENT_BUFFER => self.elements,
                    _ => return Ok(self.reject(0x0500)),
                };
                if !matches!(pname, 0x8764 | 0x8765) {
                    return Ok(self.reject(0x0500));
                }
                if bound == 0 {
                    return Ok(self.reject(0x0502));
                }
                let mut value = 0i32;
                gl!(target => u32, pname => u32, &mut value => *mut i32);
                ctx.cpu.write_mem(out, &value.to_le_bytes())?;
            }
            "glBufferSubData" => {
                let (target, offset, size, address) = (u!(0), i!(1), i!(2), u!(3));
                let bytes = transfer_size(size, 1)?;
                let data = ctx.cpu.read_mem(address, bytes as u32)?;
                gl!(target => u32, offset as isize => isize, size as isize => isize, data.as_ptr().cast() => *const c_void);
                let index = if target == ELEMENT_BUFFER {
                    self.elements
                } else {
                    self.array
                };
                if let Some(shadow) = self.index_data.get_mut(&index) {
                    if let Some(dest) = usize::try_from(offset).ok().and_then(|start| {
                        start
                            .checked_add(bytes)
                            .and_then(|end| shadow.get_mut(start..end))
                    }) {
                        dest.copy_from_slice(&data);
                    }
                }
            }
            "glVertexAttrib1f" => gl!(u!(0) => u32, f!(1) => f32),
            "glVertexAttrib2f" => gl!(u!(0) => u32, f!(1) => f32, f!(2) => f32),
            "glVertexAttrib3f" => gl!(u!(0) => u32, f!(1) => f32, f!(2) => f32, f!(3) => f32),
            "glVertexAttrib4f" => {
                gl!(u!(0) => u32, f!(1) => f32, f!(2) => f32, f!(3) => f32, f!(4) => f32);
            }
            "glVertexAttrib1fv" | "glVertexAttrib2fv" | "glVertexAttrib3fv"
            | "glVertexAttrib4fv" => {
                let (index, address) = (u!(0), u!(1));
                if index >= self.max_attributes {
                    return Ok(self.reject(0x0501));
                }
                let components = match name {
                    "glVertexAttrib2fv" => 2,
                    "glVertexAttrib3fv" => 3,
                    "glVertexAttrib4fv" => 4,
                    _ => 1,
                };
                let values = read_words(ctx, address, components)?;
                gl!(index => u32, values.as_ptr().cast() => *const f32);
            }
            "glVertexAttribPointer" => {
                let (index, size, kind, normalized, stride, address) =
                    (u!(0), i!(1), u!(2), u!(3) as u8, i!(4), u!(5));
                let component = match kind {
                    0x1400 | 0x1401 => 1,
                    0x1402 | 0x1403 => 2,
                    0x1406 | 0x140c => 4,
                    _ => return Ok(self.reject(0x0500)),
                };
                if index >= self.max_attributes || !(1..=4).contains(&size) || stride < 0 {
                    return Ok(self.reject(0x0501));
                }
                self.configured_attributes.insert(index);
                if self.array == 0 {
                    self.client_attributes.insert(
                        index,
                        ClientAttribute {
                            address,
                            size,
                            kind,
                            normalized,
                            stride,
                            bytes_per_vertex: size as usize * component,
                        },
                    );
                } else {
                    gl!(index => u32, size => i32, kind => u32, normalized => u8, stride => i32,
                        address as usize as *const c_void => *const c_void);
                    self.client_attributes.remove(&index);
                    // Keep its staging buffer available if this slot switches
                    // back from a guest VBO to client memory.
                }
            }
            "glGetVertexAttribPointerv" => {
                let (index, pname, out) = (u!(0), u!(1), u!(2));
                if index >= self.max_attributes {
                    return Ok(self.reject(0x0501));
                }
                if pname != 0x8645 {
                    return Ok(self.reject(0x0500));
                }
                let address = if let Some(attribute) = self.client_attributes.get(&index) {
                    attribute.address
                } else {
                    let mut pointer = std::ptr::null_mut();
                    gl!(index => u32, pname => u32, &mut pointer => *mut *mut c_void);
                    pointer as usize as u32
                };
                ctx.cpu.write_mem(out, &address.to_le_bytes())?;
            }
            "glGetVertexAttribfv" | "glGetVertexAttribiv" => {
                let (index, pname, out) = (u!(0), u!(1), u!(2));
                if index >= self.max_attributes {
                    return Ok(self.reject(0x0501));
                }
                if !matches!(pname, 0x8622..=0x8626 | 0x886a | 0x889f) {
                    return Ok(self.reject(0x0500));
                }
                let float = name == "glGetVertexAttribfv";
                let client_value =
                    self.client_attributes
                        .get(&index)
                        .and_then(|attribute| match pname {
                            0x8623 => Some(attribute.size),
                            0x8624 => Some(attribute.stride),
                            0x8625 => Some(attribute.kind as i32),
                            0x886a => Some(i32::from(attribute.normalized != 0)),
                            0x889f => Some(0),
                            _ => None,
                        });
                if let Some(value) = client_value {
                    let bits = if float {
                        (value as f32).to_bits()
                    } else {
                        value as u32
                    };
                    ctx.cpu.write_mem(out, &bits.to_le_bytes())?;
                } else {
                    let count = if pname == 0x8626 { 4 } else { 1 };
                    let mut values = [0u32; 4];
                    if float {
                        gl!(index => u32, pname => u32, values.as_mut_ptr().cast() => *mut f32);
                    } else {
                        gl!(index => u32, pname => u32, values.as_mut_ptr().cast() => *mut i32);
                    }
                    write_words(ctx, out, &values[..count])?;
                }
            }
            "glDrawArrays" => {
                let (mode, first, count) = (u!(0), i!(1), i!(2));
                if !self
                    .enabled_attributes
                    .is_subset(&self.configured_attributes)
                {
                    return Ok(self.reject(0x0502));
                }
                if count > 0 && first >= 0 {
                    self.stage_attributes(ctx, first as usize + count as usize - 1)?;
                }
                gl!(mode => u32, first => i32, count => i32);
            }
            "glDrawElements" => {
                let (mode, count, kind, address) = (u!(0), i!(1), u!(2), u!(3));
                let component = match kind {
                    0x1401 => 1,
                    0x1403 => 2,
                    0x1405 => 4,
                    _ => return Ok(self.reject(0x0500)),
                };
                if count <= 0 {
                    return Ok(if count == 0 { 0 } else { self.reject(0x0501) });
                }
                if !self
                    .enabled_attributes
                    .is_subset(&self.configured_attributes)
                {
                    return Ok(self.reject(0x0502));
                }
                let bytes = transfer_size(count, component)?;
                let client = if self.elements == 0 {
                    Some(ctx.cpu.read_mem(address, bytes as u32)?)
                } else {
                    None
                };
                if self
                    .client_attributes
                    .keys()
                    .any(|index| self.enabled_attributes.contains(index))
                {
                    let data = if let Some(data) = client.as_deref() {
                        data
                    } else {
                        let start = address as usize;
                        let end = start
                            .checked_add(bytes)
                            .ok_or_else(|| error("GLES2 index offset overflow"))?;
                        self.index_data
                            .get(&self.elements)
                            .and_then(|data| data.get(start..end))
                            .ok_or_else(|| {
                                error(
                                    "GLES2 index buffer has no guest shadow for client attributes",
                                )
                            })?
                    };
                    let last = data
                        .chunks_exact(component)
                        .map(|index| {
                            let mut bytes = [0; 4];
                            bytes[..component].copy_from_slice(index);
                            u32::from_le_bytes(bytes) as usize
                        })
                        .max()
                        .unwrap_or(0);
                    self.stage_attributes(ctx, last)?;
                }
                if let Some(data) = client {
                    let mut staging = std::mem::take(&mut self.client_indices);
                    let upload = self.upload_client_buffer(ELEMENT_BUFFER, &mut staging, data);
                    self.client_indices = staging;
                    let draw = upload.and_then(|()| {
                        gl!(mode => u32, count => i32, kind => u32, std::ptr::null() => *const c_void);
                        Ok(())
                    });
                    invoke!(self, "glBindBuffer", (ELEMENT_BUFFER => u32, 0 => u32), ());
                    draw?;
                } else {
                    gl!(mode => u32, count => i32, kind => u32, address as usize as *const c_void => *const c_void);
                }
            }
            "glTexImage2D" => {
                let (width, height, format, kind, address) = (i!(3), i!(4), u!(6), u!(7), u!(8));
                let Some(bytes) = texture_size(width, height, format, kind, self.unpack) else {
                    return Ok(self.reject(0x0501));
                };
                let pixels = optional_bytes(ctx, address, bytes)?;
                gl!(u!(0) => u32, i!(1) => i32, i!(2) => i32, width => i32, height => i32,
                    i!(5) => i32, format => u32, kind => u32, data_pointer(&pixels) => *const c_void);
            }
            "glTexSubImage2D" => {
                let (width, height, format, kind, address) = (i!(4), i!(5), u!(6), u!(7), u!(8));
                if width < 0 || height < 0 {
                    return Ok(self.reject(0x0501));
                }
                let Some(bytes) = texture_size(width, height, format, kind, self.unpack) else {
                    return Ok(self.reject(0x0500));
                };
                let pixels = if bytes == 0 {
                    Vec::new()
                } else {
                    ctx.cpu.read_mem(address, bytes as u32)?
                };
                gl!(u!(0) => u32, i!(1) => i32, i!(2) => i32, i!(3) => i32,
                    width => i32, height => i32, format => u32, kind => u32,
                    pixels.as_ptr().cast() => *const c_void);
            }
            "glCopyTexImage2D" => {
                gl!(u!(0) => u32, i!(1) => i32, u!(2) => u32, i!(3) => i32,
                    i!(4) => i32, i!(5) => i32, i!(6) => i32, i!(7) => i32);
            }
            "glCopyTexSubImage2D" => {
                gl!(u!(0) => u32, i!(1) => i32, i!(2) => i32, i!(3) => i32,
                    i!(4) => i32, i!(5) => i32, i!(6) => i32, i!(7) => i32);
            }
            "glCompressedTexSubImage2D" => {
                let (target, level, x, y, width, height, format, size, address) = (
                    u!(0),
                    i!(1),
                    i!(2),
                    i!(3),
                    i!(4),
                    i!(5),
                    u!(6),
                    i!(7),
                    u!(8),
                );
                if !self.native_compressed_formats.contains(&(format as i32)) {
                    return Ok(self.reject(0x0500));
                }
                if size < 0 || width < 0 || height < 0 {
                    return Ok(self.reject(0x0501));
                }
                let bytes = transfer_size(size, 1)?;
                let data = if bytes == 0 {
                    Vec::new()
                } else {
                    ctx.cpu.read_mem(address, bytes as u32)?
                };
                gl!(target => u32, level => i32, x => i32, y => i32, width => i32, height => i32,
                    format => u32, size => i32, data.as_ptr().cast() => *const c_void);
            }
            "glCompressedTexImage2D" => {
                let (target, level, format, width, height, border, size, address) =
                    (u!(0), i!(1), u!(2), i!(3), i!(4), i!(5), i!(6), u!(7));
                let pixels = optional_bytes(ctx, address, transfer_size(size, 1)?)?;
                use pocket_gles::{
                    GL_ATC_RGBA_EXPLICIT_ALPHA_AMD, GL_ATC_RGBA_INTERPOLATED_ALPHA_AMD,
                    GL_ATC_RGB_AMD,
                };
                if matches!(
                    format,
                    GL_ATC_RGB_AMD
                        | GL_ATC_RGBA_EXPLICIT_ALPHA_AMD
                        | GL_ATC_RGBA_INTERPOLATED_ALPHA_AMD
                ) && !self.native_compressed_formats.contains(&(format as i32))
                {
                    let Some(bytes) = texture_size(width, height, RGBA, UNSIGNED_BYTE, 1) else {
                        return Ok(self.reject(0x0501));
                    };
                    if bytes == 0 {
                        return Ok(self.reject(0x0501));
                    }
                    let Some(decoded) = pixels.as_deref().and_then(|pixels| {
                        pocket_gles::texture::decode_compressed_to_rgba(
                            pixels,
                            width as u32,
                            height as u32,
                            format,
                        )
                    }) else {
                        return Ok(self.reject(0x0501));
                    };
                    invoke!(self, "glPixelStorei", (0x0cf5 => u32, 1 => i32), ());
                    invoke!(self, "glTexImage2D", (target => u32, level => i32, RGBA as i32 => i32,
                        width => i32, height => i32, border => i32, RGBA => u32, UNSIGNED_BYTE => u32,
                        decoded.as_ptr().cast() => *const c_void), ());
                    let align = self.unpack as i32;
                    invoke!(self, "glPixelStorei", (0x0cf5 => u32, align => i32), ());
                } else {
                    gl!(target => u32, level => i32, format => u32, width => i32, height => i32,
                        border => i32, size => i32, data_pointer(&pixels) => *const c_void);
                }
            }
            "glShaderSource" => {
                let (shader, count, strings, lengths) = (u!(0), i!(1), u!(2), u!(3));
                let count = usize::try_from(count)
                    .ok()
                    .filter(|&n| n <= 256)
                    .ok_or_else(|| error("Invalid shader source count"))?;
                let pointers = read_words(ctx, strings, count)?;
                let sizes = (lengths != 0)
                    .then(|| read_words(ctx, lengths, count))
                    .transpose()?;
                let mut source = Vec::new();
                for (index, &address) in pointers.iter().enumerate() {
                    let size = sizes.as_ref().map_or(-1, |sizes| sizes[index] as i32);
                    if size >= 0 {
                        if size as usize > 1024 * 1024 - source.len() {
                            return Err(error("Shader source exceeds 1 MiB"));
                        }
                        source.extend(ctx.cpu.read_mem(address, size as u32)?);
                    } else {
                        source.extend(read_string(ctx, address, 1024 * 1024)?);
                    }
                    if source.len() > 1024 * 1024 {
                        return Err(error("Shader source exceeds 1 MiB"));
                    }
                }
                if !self.host.es && invoke!(self, "glIsShader", (shader => u32), u8) != 0 {
                    let adapted = desktop_shader(&source);
                    let pointer = adapted.as_ptr().cast();
                    let length = adapted.len() as i32;
                    gl!(shader => u32, 1 => i32, &pointer => *const *const c_char, &length => *const i32);
                    self.desktop_shader_sources.insert(shader, source);
                } else {
                    let pointer = source.as_ptr().cast();
                    let length = source.len() as i32;
                    gl!(shader => u32, 1 => i32, &pointer => *const *const c_char, &length => *const i32);
                }
            }
            "glReleaseShaderCompiler" => {
                if self.host.es {
                    gl!();
                }
            }
            "glShaderBinary" => return Ok(self.reject(0x0500)),
            "glGetShaderPrecisionFormat" => {
                let (shader, precision, range_out, precision_out) = (u!(0), u!(1), u!(2), u!(3));
                if !matches!(shader, 0x8b30 | 0x8b31) || !matches!(precision, 0x8df0..=0x8df5) {
                    return Ok(self.reject(0x0500));
                }
                let (mut range, mut bits) = if precision <= 0x8df2 {
                    ([127i32, 127], 23i32)
                } else {
                    ([31i32, 30], 0i32)
                };
                if self.host.es {
                    gl!(shader => u32, precision => u32, range.as_mut_ptr() => *mut i32, &mut bits => *mut i32);
                }
                write_words(ctx, range_out, &range.map(|n| n as u32))?;
                ctx.cpu.write_mem(precision_out, &bits.to_le_bytes())?;
            }
            "glGetShaderSource" => {
                let (shader, capacity, length_out, source_out) = (u!(0), i!(1), u!(2), u!(3));
                if capacity < 0 {
                    return Ok(self.reject(0x0501));
                }
                let mut length = 0i32;
                if self.host.es {
                    let mut source = vec![0u8; transfer_size(capacity, 1)?];
                    gl!(shader => u32, capacity => i32, &mut length => *mut i32,
                        source.as_mut_ptr().cast() => *mut c_char);
                    if capacity > 0 {
                        ctx.cpu.write_mem(
                            source_out,
                            &source[..length.clamp(0, capacity - 1) as usize + 1],
                        )?;
                    }
                } else {
                    if invoke!(self, "glIsShader", (shader => u32), u8) == 0 {
                        return Ok(self.reject(0x0501));
                    }
                    if capacity > 0 {
                        let source = self
                            .desktop_shader_sources
                            .get(&shader)
                            .map_or(&[][..], Vec::as_slice);
                        let source = &source[..source.len().min((capacity - 1) as usize)];
                        length = source.len() as i32;
                        ctx.cpu.write_mem(source_out, source)?;
                        let terminator = source_out
                            .checked_add(length as u32)
                            .ok_or_else(|| error("Guest shader source address overflow"))?;
                        ctx.cpu.write_mem(terminator, &[0])?;
                    }
                }
                if length_out != 0 {
                    ctx.cpu.write_mem(length_out, &length.to_le_bytes())?;
                }
            }
            "glGetActiveAttrib" | "glGetActiveUniform" => {
                let (program, index, capacity, length_out, size_out, type_out, name_out) =
                    (u!(0), u!(1), i!(2), u!(3), u!(4), u!(5), u!(6));
                if capacity < 0 {
                    return Ok(self.reject(0x0501));
                }
                let mut symbol = vec![0u8; transfer_size(capacity, 1)?];
                let (mut length, mut size, mut kind) = (0i32, 0i32, 0u32);
                gl!(program => u32, index => u32, capacity => i32, &mut length => *mut i32,
                    &mut size => *mut i32, &mut kind => *mut u32, symbol.as_mut_ptr().cast() => *mut c_char);
                if kind != 0 {
                    if length_out != 0 {
                        ctx.cpu.write_mem(length_out, &length.to_le_bytes())?;
                    }
                    ctx.cpu.write_mem(size_out, &size.to_le_bytes())?;
                    ctx.cpu.write_mem(type_out, &kind.to_le_bytes())?;
                    if capacity > 0 {
                        ctx.cpu.write_mem(
                            name_out,
                            &symbol[..length.clamp(0, capacity - 1) as usize + 1],
                        )?;
                    }
                }
            }
            "glGetUniformfv" | "glGetUniformiv" => {
                let (program, location, out) = (u!(0), i!(1), u!(2));
                // ES2 writes one uniform element, with up to 16 components.
                // Different initial bit patterns identify the written prefix even
                // when a legitimate value matches one of the patterns.
                let mut values = [0u32; 16];
                let mut check = [u32::MAX; 16];
                for storage in [&mut values, &mut check] {
                    if name == "glGetUniformfv" {
                        gl!(program => u32, location => i32, storage.as_mut_ptr().cast() => *mut f32);
                    } else {
                        gl!(program => u32, location => i32, storage.as_mut_ptr().cast() => *mut i32);
                    }
                }
                let count = values
                    .iter()
                    .zip(check)
                    .take_while(|(a, b)| **a == *b)
                    .count();
                write_words(ctx, out, &values[..count])?;
            }
            "glGetProgramiv" | "glGetShaderiv" => {
                let mut value = 0i32;
                let (object, pname, out) = (u!(0), u!(1), u!(2));
                if name == "glGetShaderiv"
                    && pname == 0x8b88
                    && !self.host.es
                    && invoke!(self, "glIsShader", (object => u32), u8) != 0
                {
                    value = self
                        .desktop_shader_sources
                        .get(&object)
                        .map_or(0, |source| source.len() as i32 + 1);
                } else {
                    gl!(object => u32, pname => u32, &mut value => *mut i32);
                }
                ctx.cpu.write_mem(out, &value.to_le_bytes())?;
            }
            "glGetProgramInfoLog" | "glGetShaderInfoLog" => {
                let (object, size, length_out, log_out) = (u!(0), i!(1), u!(2), u!(3));
                let mut log = vec![0u8; transfer_size(size, 1)?];
                let mut length = 0i32;
                gl!(object => u32, size => i32, &mut length => *mut i32, log.as_mut_ptr().cast() => *mut c_char);
                if length_out != 0 {
                    ctx.cpu.write_mem(length_out, &length.to_le_bytes())?;
                }
                if size > 0 {
                    let len = (length.max(0) as usize + 1).min(log.len());
                    ctx.cpu.write_mem(log_out, &log[..len])?;
                }
            }
            "glGetUniformLocation" | "glBindAttribLocation" | "glGetAttribLocation" => {
                let object = u!(0);
                let bind = name == "glBindAttribLocation";
                let address = u!(if bind { 2 } else { 1 });
                let symbol = CString::new(read_string(ctx, address, 4096)?)
                    .map_err(|_| error("Invalid shader symbol"))?;
                if bind {
                    gl!(object => u32, u!(1) => u32, symbol.as_ptr() => *const c_char);
                } else {
                    return Ok(
                        invoke!(self, name, (object => u32, symbol.as_ptr() => *const c_char), i32)
                            as u32,
                    );
                }
            }
            "glGetAttachedShaders" => {
                let (program, capacity, count_out, shaders_out) = (u!(0), i!(1), u!(2), u!(3));
                if capacity < 0 {
                    return Ok(self.reject(0x0501));
                }
                let mut shaders = vec![0u32; transfer_size(capacity, 4)? / 4];
                let mut count = 0i32;
                gl!(program => u32, capacity => i32, &mut count => *mut i32, shaders.as_mut_ptr() => *mut u32);
                if count_out != 0 {
                    ctx.cpu.write_mem(count_out, &count.to_le_bytes())?;
                }
                let count = count.clamp(0, capacity) as usize;
                if count != 0 {
                    write_words(ctx, shaders_out, &shaders[..count])?;
                }
            }
            "glReadPixels" => {
                let (x, y, width, height) = (i!(0), i!(1), i!(2), i!(3));
                let (format, kind, address) = (u!(4), u!(5), u!(6));
                if width < 0 || height < 0 {
                    return Ok(self.reject(0x0501));
                }
                let Some(bytes) = texture_size(width, height, format, kind, self.pack) else {
                    return Ok(self.reject(0x0500));
                };
                if bytes == 0 {
                    return Ok(0);
                }
                address
                    .checked_add(bytes as u32 - 1)
                    .ok_or_else(|| error("GLES2 readback address overflow"))?;
                let row = texture_size(width, 1, format, kind, 1).unwrap();
                let stride = (row + self.pack - 1) & !(self.pack - 1);
                let mut pixels = vec![0u8; bytes];
                // Preserve an earlier GL error while detecting a failed read.
                let previous = invoke!(self, "glGetError", (), u32);
                if previous != 0 {
                    self.reject(previous);
                }
                gl!(x => i32, y => i32, width => i32, height => i32, format => u32, kind => u32,
                    pixels.as_mut_ptr().cast() => *mut c_void);
                let read_error = invoke!(self, "glGetError", (), u32);
                if read_error != 0 {
                    return Ok(self.reject(read_error));
                }
                if stride == row {
                    ctx.cpu.write_mem(address, &pixels)?;
                } else {
                    for (index, data) in pixels.chunks(stride).enumerate() {
                        let start = address + (index * stride) as u32;
                        ctx.cpu.write_mem(start, &data[..row])?;
                    }
                }
            }
            "glGetIntegerv" | "glGetBooleanv" | "glGetFloatv" => {
                let (parameter, address) = (u!(0), u!(1));
                let boolean = name == "glGetBooleanv";
                let float = name == "glGetFloatv";
                let count = match parameter {
                    0x0ba2 | 0x0c10 | 0x0c22 | 0x0c23 | 0x8005 => 4,
                    0x0b70 | 0x0d3a | 0x846d | 0x846e => 2,
                    0x8df8 => return Ok(0), // Shader binary formats: none.
                    0x8869 | 0x8b9a | 0x8b9b | 0x8ca6 | 0x8df9 | 0x8dfa => {
                        let value = match parameter {
                            0x8869 => self.max_attributes,
                            0x8b9a => UNSIGNED_BYTE,
                            0x8b9b => RGBA,
                            0x8ca6 => self.framebuffer,
                            0x8dfa => 1, // Source shaders are compiled on either host API.
                            _ => 0,      // Shader binaries are not exposed.
                        };
                        if boolean {
                            ctx.cpu.write_mem(address, &[u8::from(value != 0)])?;
                        } else if float {
                            ctx.cpu.write_mem(address, &(value as f32).to_le_bytes())?;
                        } else {
                            ctx.cpu.write_mem(address, &value.to_le_bytes())?;
                        }
                        return Ok(0);
                    }
                    0x86a3 => self.native_compressed_formats.len(),
                    // Scalar ES2 queries.
                    0x0b21
                    | 0x0b44
                    | 0x0b45
                    | 0x0b46
                    | 0x0b71
                    | 0x0b72
                    | 0x0b73
                    | 0x0b74
                    | 0x0b90..=0x0b98
                    | 0x8009
                    | 0x8037..=0x8038
                    | 0x80c8..=0x80cb
                    | 0x8192
                    | 0x883d
                    | 0x8800..=0x8803
                    | 0x8ca3..=0x8ca5
                    | 0x0bd0
                    | 0x0be2
                    | 0x0c11
                    | 0x0cf5
                    | 0x0d05
                    | 0x0d33
                    | 0x0d50..=0x0d57
                    | 0x2a00
                    | 0x8069
                    | 0x809e
                    | 0x80a0
                    | 0x80a8..=0x80ab
                    | 0x84e0
                    | 0x84e8
                    | 0x8514
                    | 0x851c
                    | 0x86a2
                    | 0x8894
                    | 0x8895
                    | 0x8b49..=0x8b4d
                    | 0x8b8d
                    | 0x8ca7
                    | 0x8dfb..=0x8dfd => 1,
                    _ => return Ok(self.reject(0x0500)),
                };
                if count != 0 && boolean {
                    let mut values = vec![0u8; count];
                    gl!(parameter => u32, values.as_mut_ptr() => *mut u8);
                    ctx.cpu.write_mem(address, &values)?;
                } else if count != 0 {
                    let mut values = vec![0u32; count];
                    if float {
                        gl!(parameter => u32, values.as_mut_ptr().cast() => *mut f32);
                    } else {
                        gl!(parameter => u32, values.as_mut_ptr().cast() => *mut i32);
                    }
                    write_words(ctx, address, &values)?;
                }
            }
            "glUniform1i" => gl!(i!(0) => i32, i!(1) => i32),
            "glUniform2i" => gl!(i!(0) => i32, i!(1) => i32, i!(2) => i32),
            "glUniform3i" => gl!(i!(0) => i32, i!(1) => i32, i!(2) => i32, i!(3) => i32),
            "glUniform4i" => {
                gl!(i!(0) => i32, i!(1) => i32, i!(2) => i32, i!(3) => i32, i!(4) => i32)
            }
            "glUniform1f" => gl!(i!(0) => i32, f!(1) => f32),
            "glUniform2f" => gl!(i!(0) => i32, f!(1) => f32, f!(2) => f32),
            "glUniform3f" => gl!(i!(0) => i32, f!(1) => f32, f!(2) => f32, f!(3) => f32),
            "glUniform4f" => {
                gl!(i!(0) => i32, f!(1) => f32, f!(2) => f32, f!(3) => f32, f!(4) => f32)
            }
            "glUniform1fv" | "glUniform2fv" | "glUniform3fv" | "glUniform4fv" | "glUniform1iv"
            | "glUniform2iv" | "glUniform3iv" | "glUniform4iv" => {
                let (location, count, address) = (i!(0), i!(1), u!(2));
                let components = match name {
                    "glUniform2fv" | "glUniform2iv" => 2,
                    "glUniform3fv" | "glUniform3iv" => 3,
                    "glUniform4fv" | "glUniform4iv" => 4,
                    _ => 1,
                };
                let values = read_words(ctx, address, transfer_size(count, components * 4)? / 4)?;
                if name.ends_with("iv") {
                    gl!(location => i32, count => i32, values.as_ptr().cast() => *const i32);
                } else {
                    gl!(location => i32, count => i32, values.as_ptr().cast() => *const f32);
                }
            }
            "glUniformMatrix2fv" | "glUniformMatrix3fv" | "glUniformMatrix4fv" => {
                let (location, count, transpose, address) = (i!(0), i!(1), u!(2), u!(3));
                let components = match name {
                    "glUniformMatrix2fv" => 4,
                    "glUniformMatrix3fv" => 9,
                    _ => 16,
                };
                let values = read_words(ctx, address, transfer_size(count, components * 4)? / 4)?;
                gl!(location => i32, count => i32, transpose as u8 => u8, values.as_ptr().cast() => *const f32);
            }
            _ => {
                log::warn!("Unsupported guest GLES2 call {name}");
                return Ok(self.reject(0x0502));
            }
        }
        Ok(0)
    }

    fn reap_shader_sources(&mut self) -> Result<()> {
        if self.deleted_shaders.is_empty() {
            return Ok(());
        }
        for shader in std::mem::take(&mut self.deleted_shaders) {
            if invoke!(self, "glIsShader", (shader => u32), u8) == 0 {
                self.desktop_shader_sources.remove(&shader);
            } else {
                self.deleted_shaders.insert(shader);
            }
        }
        Ok(())
    }

    fn reject(&mut self, code: u32) -> u32 {
        if self.guest_error == 0 {
            self.guest_error = code;
        }
        0
    }
}

fn transfer_size(count: i32, size: usize) -> Result<usize> {
    usize::try_from(count)
        .ok()
        .and_then(|n| n.checked_mul(size))
        .filter(|&n| n <= MAX_TRANSFER)
        .ok_or_else(|| error("GLES2 transfer exceeds the supported size"))
}

fn read_words(ctx: &mut CallCtx<'_>, address: u32, count: usize) -> Result<Vec<u32>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    Ok(ctx
        .cpu
        .read_mem(address, (count * 4) as u32)?
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect())
}

fn write_words(ctx: &mut CallCtx<'_>, address: u32, values: &[u32]) -> Result<()> {
    if !values.is_empty() {
        let bytes: Vec<_> = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        ctx.cpu.write_mem(address, &bytes)?;
    }
    Ok(())
}

fn read_string(ctx: &mut CallCtx<'_>, address: u32, max: u32) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    for offset in 0..max {
        let ptr = address
            .checked_add(offset)
            .ok_or_else(|| error("Guest string address overflow"))?;
        let mut byte = [0u8; 1];
        ctx.cpu.read_mem_into(ptr, &mut byte)?;
        if byte[0] == 0 {
            return Ok(output);
        }
        output.push(byte[0]);
    }
    Err(error("Unterminated guest GLES2 string"))
}

fn optional_bytes(ctx: &mut CallCtx<'_>, address: u32, size: usize) -> Result<Option<Vec<u8>>> {
    if address == 0 || size == 0 {
        Ok(None)
    } else {
        Ok(Some(ctx.cpu.read_mem(address, size as u32)?))
    }
}

fn data_pointer(bytes: &Option<Vec<u8>>) -> *const c_void {
    bytes
        .as_ref()
        .map_or(std::ptr::null(), |bytes| bytes.as_ptr().cast())
}

fn texture_size(width: i32, height: i32, format: u32, kind: u32, align: usize) -> Option<usize> {
    let width = usize::try_from(width).ok()?;
    let height = usize::try_from(height).ok()?;
    if width == 0 || height == 0 {
        return Some(0);
    }
    let components = match (format, kind) {
        (0x1906 | 0x1909, UNSIGNED_BYTE) => 1,
        (0x190a, UNSIGNED_BYTE) => 2,
        (0x1907, UNSIGNED_BYTE) => 3,
        (RGBA, UNSIGNED_BYTE) => 4,
        (0x1907, 0x8363) | (RGBA, 0x8033 | 0x8034) => 2,
        _ => return None,
    };
    let row = width.checked_mul(components)?;
    let stride = row.checked_add(align - 1)? & !(align - 1);
    stride
        .checked_mul(height - 1)?
        .checked_add(row)
        .filter(|&size| size <= MAX_TRANSFER)
}

fn desktop_shader(source: &[u8]) -> Vec<u8> {
    let mut output = b"#version 120\n#define lowp\n#define mediump\n#define highp\n".to_vec();
    for line in source.split_inclusive(|&byte| byte == b'\n') {
        let trimmed = line.trim_ascii_start();
        if trimmed.starts_with(b"precision ") || trimmed.starts_with(b"#version ") {
            output.push(b'\n');
        } else {
            output.extend_from_slice(line);
        }
    }
    output
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use pocket_gles::gpu::{with_shared_context_source, Renderer as NativeRenderer};
    use pocket_kernel::Thunk;
    use pocket_pe::ImportBinding;
    use std::sync::{atomic::AtomicBool, Arc};

    #[test]
    fn reused_client_buffers_skip_equal_bytes_and_limit_partial_uploads() {
        let previous = [1, 2, 3, 4, 5, 6];
        assert_eq!(changed_range(&previous, &previous), None);
        assert_eq!(changed_range(&previous, &[1, 2, 3]), None);
        assert_eq!(changed_range(&previous, &[1, 2, 8, 4, 5]), Some(2..3));
        assert_eq!(changed_range(&previous, &[1, 2, 8, 4, 9]), Some(2..5));
    }

    #[test]
    #[ignore = "requires native OpenGL"]
    fn programmable_queries_and_texture_copies_preserve_guest_data() -> Result<()> {
        use pocket_cpu::regs::ArmReg::{R0, R1, R2, R3};
        let (mut cpu, mut kernel, _) = crate::bluetooth::tests::setup();
        let thunk = Thunk {
            thunk_va: 0,
            iat_va: 0,
            dll: "libglesv2.dll".into(),
            binding: ImportBinding::Name("glTexSubImage2D".into()),
            friendly_name: None,
        };
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            thunk: &thunk,
            kernel: &mut kernel,
        };
        let mut renderer = Renderer::new(8, 8)?;
        let mut run = |ctx: &mut CallCtx<'_>, name, args: &[u32]| -> Result<u32> {
            for (reg, &value) in [R0, R1, R2, R3].into_iter().zip(args) {
                ctx.cpu.write_reg(reg, value)?;
            }
            write_words(ctx, 0x4000, args.get(4..).unwrap_or_default())?;
            renderer.gl(name, ctx)
        };

        run(
            &mut ctx,
            "glVertexAttribPointer",
            &[0, 2, 0x1406, 0, 12, 0x2000],
        )?;
        run(&mut ctx, "glGetVertexAttribPointerv", &[0, 0x8645, 0x1000])?;
        run(&mut ctx, "glGetVertexAttribiv", &[0, 0x8624, 0x1004])?;
        assert_eq!(read_words(&mut ctx, 0x1000, 2)?, [0x2000, 12]);

        let source = b"#version 100\nvoid main() { gl_Position = vec4(0.0); }";
        ctx.cpu.write_mem(0x2000, source)?;
        write_words(&mut ctx, 0x1000, &[0x2000, source.len() as u32])?;
        let shader = run(&mut ctx, "glCreateShader", &[0x8b31])?;
        run(&mut ctx, "glShaderSource", &[shader, 1, 0x1000, 0x1004])?;
        run(&mut ctx, "glGetShaderiv", &[shader, 0x8b88, 0x1010])?;
        run(
            &mut ctx,
            "glGetShaderSource",
            &[shader, 256, 0x1014, 0x3000],
        )?;
        assert_eq!(
            read_words(&mut ctx, 0x1010, 2)?,
            [source.len() as u32 + 1, source.len() as u32]
        );
        assert_eq!(ctx.cpu.read_mem(0x3000, source.len() as u32)?, source);
        run(&mut ctx, "glDeleteShader", &[shader])?;

        run(&mut ctx, "glGenTextures", &[1, 0x1000])?;
        let texture = read_words(&mut ctx, 0x1000, 1)?[0];
        run(&mut ctx, "glBindTexture", &[TEXTURE_2D, texture])?;
        run(
            &mut ctx,
            "glTexImage2D",
            &[TEXTURE_2D, 0, RGBA, 1, 2, 0, RGBA, UNSIGNED_BYTE, 0],
        )?;
        run(&mut ctx, "glPixelStorei", &[0x0cf5, 8])?;
        ctx.cpu.write_mem(
            0x2000,
            &[255, 0, 0, 255, 0x55, 0x55, 0x55, 0x55, 0, 255, 0, 255],
        )?;
        run(
            &mut ctx,
            "glTexSubImage2D",
            &[TEXTURE_2D, 0, 0, 0, 1, 2, RGBA, UNSIGNED_BYTE, 0x2000],
        )?;
        run(&mut ctx, "glGenFramebuffers", &[1, 0x1000])?;
        let framebuffer = read_words(&mut ctx, 0x1000, 1)?[0];
        run(&mut ctx, "glBindFramebuffer", &[FRAMEBUFFER, framebuffer])?;
        run(
            &mut ctx,
            "glFramebufferTexture2D",
            &[FRAMEBUFFER, 0x8ce0, TEXTURE_2D, texture, 0],
        )?;
        assert_eq!(
            run(&mut ctx, "glCheckFramebufferStatus", &[FRAMEBUFFER])?,
            0x8cd5
        );
        run(
            &mut ctx,
            "glReadPixels",
            &[0, 0, 1, 2, RGBA, UNSIGNED_BYTE, 0x3000],
        )?;
        assert_eq!(run(&mut ctx, "glGetError", &[])?, 0);
        assert_eq!(
            ctx.cpu.read_mem(0x3000, 8)?,
            [255, 0, 0, 255, 0, 255, 0, 255]
        );
        run(&mut ctx, "glGenTextures", &[1, 0x1000])?;
        let copied = read_words(&mut ctx, 0x1000, 1)?[0];
        run(&mut ctx, "glBindTexture", &[TEXTURE_2D, copied])?;
        run(
            &mut ctx,
            "glCopyTexImage2D",
            &[TEXTURE_2D, 0, RGBA, 0, 0, 1, 2, 0],
        )?;
        run(
            &mut ctx,
            "glCopyTexSubImage2D",
            &[TEXTURE_2D, 0, 0, 0, 0, 1, 1, 1],
        )?;
        run(
            &mut ctx,
            "glFramebufferTexture2D",
            &[FRAMEBUFFER, 0x8ce0, TEXTURE_2D, copied, 0],
        )?;
        run(
            &mut ctx,
            "glReadPixels",
            &[0, 0, 1, 2, RGBA, UNSIGNED_BYTE, 0x3000],
        )?;
        assert_eq!(run(&mut ctx, "glGetError", &[])?, 0);
        assert_eq!(
            ctx.cpu.read_mem(0x3000, 8)?,
            [0, 255, 0, 255, 0, 255, 0, 255]
        );

        Ok(())
    }

    #[test]
    #[ignore = "requires desktop OpenGL"]
    fn programmable_queries_and_copies_work_through_desktop_gl() -> Result<()> {
        let ui = NativeRenderer::new(8, 8).map_err(error)?;
        with_shared_context_source(Some(ui.shared_context_source()), || {
            programmable_queries_and_texture_copies_preserve_guest_data()
        })
    }

    #[test]
    #[ignore = "requires native OpenGL with shared-context fences"]
    fn programmable_swap_publishes_gpu_pixels_and_defers_readback() -> Result<()> {
        let ui = NativeRenderer::new(8, 8).map_err(error)?;
        let source = ui.shared_context_source();
        let (mut cpu, mut kernel, _) = crate::bluetooth::tests::setup();
        let thunk = Thunk {
            thunk_va: 0,
            iat_va: 0,
            dll: "libegl.dll".into(),
            binding: ImportBinding::Name("eglSwapBuffers".into()),
            friendly_name: None,
        };
        let mut call = CallCtx {
            cpu: &mut cpu,
            thunk: &thunk,
            kernel: &mut kernel,
        };
        with_shared_context_source(Some(source.clone()), || {
            crate::gles::with_shared_frame_export(Arc::new(AtomicBool::new(true)), || {
                let mut renderer = Renderer::new(8, 8)?;
                // A different native context can become current on the worker.
                // The GL entry point must rebind the renderer before drawing.
                let foreign = ProgrammableHost::new_programmable().map_err(error)?;
                use pocket_cpu::regs::ArmReg::{R0, R1, R2, R3};
                for (reg, value) in [(R0, 1.0f32.to_bits()), (R1, 0), (R2, 0), (R3, 0)] {
                    call.cpu.write_reg(reg, value)?;
                }
                renderer.gl("glClearColor", &mut call)?;
                drop(foreign);
                call.cpu.write_reg(R0, 0x4000)?;
                renderer.gl("glClear", &mut call)?;
                call.cpu.write_reg(R0, 0x0d05)?; // GL_PACK_ALIGNMENT
                call.cpu.write_reg(R1, 8)?;
                renderer.gl("glPixelStorei", &mut call)?;
                call.cpu.write_mem(0x2000, &[0x55; 12])?;
                for (reg, value) in [(R0, 0), (R1, 0), (R2, 1), (R3, 2)] {
                    call.cpu.write_reg(reg, value)?;
                }
                write_words(&mut call, 0x4000, &[RGBA, UNSIGNED_BYTE, 0x2000])?;
                renderer.gl("glReadPixels", &mut call)?;
                assert_eq!(
                    call.cpu.read_mem(0x2000, 12)?,
                    [255, 0, 0, 0, 0x55, 0x55, 0x55, 0x55, 255, 0, 0, 0],
                );
                renderer.present(&mut call)?;
                assert!(renderer.readback.is_empty());
                assert!(!call.kernel.framebuffer.cpu_resident());
                let frame = crate::gles::take_shared_window_frame(&call.kernel.framebuffer)
                    .expect("programmable swap must publish a shared frame");
                assert_eq!(frame.size(), (8, 8));
                assert!(frame.belongs_to_source(&source));

                // Changing the renderer's window must leave the snapshot intact.
                invoke!(&mut renderer, "glClearColor", (0.0 => f32, 1.0 => f32, 0.0 => f32, 1.0 => f32), ());
                invoke!(&mut renderer, "glClear", (0x4000 => u32), ());
                let pixels = &call.kernel.framebuffer.cpu().map_err(error)?.pixels;
                assert!(pixels.chunks_exact(2).all(|pixel| pixel == [0, 248]));
                Ok(())
            })
        })
    }
}
