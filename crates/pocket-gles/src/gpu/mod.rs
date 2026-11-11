//! OpenGL 2.1 / GLES 2 offscreen rasterizer. Guest pointers never reach a driver.
mod depth_bias;
mod diagnostic;
mod fixed_vertices;
mod platform;
mod readback;
mod resolve;
mod shared;
mod transfer;
#[cfg(unix)]
pub use platform::PersistentSharedContext;
pub use platform::SharedConsumer;
pub use readback::SharedReadback;
pub use shared::{SharedFrame, RGB565_DISPLAY_FRAGMENT, SHARED_DISPLAY_FRAGMENT};
#[cfg(all(test, unix))]
mod shared_display_tests;
#[cfg(all(test, unix))]
mod shared_present_ring_tests;
#[cfg(all(test, unix))]
mod shared_present_tests;
#[cfg(all(test, unix))]
mod shared_root_tests;
use crate::{
    consts::*,
    matrix::{Matrix4, IDENTITY},
    raster::{
        BlendFactor, CompareFunc, CullMode, FogMode, FrontFace, PipelineState, RenderTarget,
        TexEnvMode, TextureStage, Vertex,
    },
    texture::{Filter, Texture, Wrap},
};
pub use platform::ExternalContext as ExternalGpuContext;
#[cfg(unix)]
pub use platform::ExternalContext as ExternalEglContext;
use platform::Host;
use std::cell::RefCell;
thread_local! {
    static SHARED_CONTEXT_SOURCE: RefCell<Option<SharedContextSource>> = const { RefCell::new(None) };
}
/// Apply the UI's context-creation capability only on the calling emulation
/// thread. Nested child launches and unwinding restore the preceding source.
pub fn with_shared_context_source<R>(
    source: Option<SharedContextSource>,
    run: impl FnOnce() -> R,
) -> R {
    struct Restore(Option<SharedContextSource>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SHARED_CONTEXT_SOURCE.with(|s| {
                s.replace(self.0.take());
            });
        }
    }
    let _restore = Restore(SHARED_CONTEXT_SOURCE.with(|s| s.replace(source)));
    run()
}
/// Creation capability selected for this execution thread, if any.
/// It does not expose a context or permit issuing GL calls on another thread.
pub fn configured_shared_context_source() -> Option<SharedContextSource> {
    SHARED_CONTEXT_SOURCE.with(|source| source.borrow().clone())
}
pub use platform::SharedSource as SharedContextSource;
use std::{
    collections::{HashMap, HashSet},
    ffi::{c_char, c_void, CStr, CString},
};
pub struct VertexTransforms {
    pub lighting: crate::lighting::LightingState,
    pub normal_matrix: Matrix4,
    pub projection: Matrix4,
    pub modelview: Matrix4,
    pub textures: [Matrix4; 6],
}
/// Host-owned snapshots only. Guest addresses never reach the native driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StreamAttribute {
    pub size: i32,
    pub ty: u32,
    pub normalized: u8,
    pub stride: i32,
    pub offset: usize,
}
#[derive(Default)]
pub(crate) struct VertexStream {
    pub bytes: Vec<u8>,
    pub attributes: [Option<StreamAttribute>; 10],
    pub constants: [[f32; 4]; 10],
    pub count: usize,
    pub indices: Vec<u16>,
    pub coverage: Vec<u8>,
    /// Only set after per-primitive provoking attributes have been expanded.
    pub flat_triangles: bool,
}

impl VertexStream {
    /// Preserve typed snapshots and expand only attributes needed by GL_FLAT.
    /// Geometry/UVs use each corner; colour and lighting data use corner 3.
    /// No guest addresses or live guest buffers reach the driver.
    pub(crate) fn expand_flat_triangles(&self, out: &mut Self) -> bool {
        out.bytes.clear();
        out.indices.clear();
        out.attributes.fill(None);
        out.flat_triangles = false;
        out.count = 0;
        if self.flat_triangles
            || self.count == 0
            || self.count > 65536
            || self.attributes[0].is_none()
        {
            return false;
        }
        if self.indices.iter().any(|&i| i as usize >= self.count) {
            return false;
        }
        let references = if self.indices.is_empty() {
            self.count
        } else {
            self.indices.len()
        };
        let count = references / 3 * 3;
        if count == 0 || count > 65536 {
            return false;
        }
        let mut fields = [None; 8];
        let mut stride = 0usize;
        for (slot, attribute) in self.attributes.iter().enumerate() {
            let Some(attribute) = attribute else {
                continue;
            };
            let width = match attribute.ty {
                GL_BYTE | GL_UNSIGNED_BYTE => 1,
                GL_SHORT => 2,
                GL_FLOAT | GL_FIXED => 4,
                _ => return false,
            };
            if !(1..=4).contains(&attribute.size)
                || attribute.stride <= 0
                || attribute.normalized > 1
                || attribute.offset % width != 0
                || attribute.stride as usize % width != 0
            {
                return false;
            }
            let bytes = attribute.size as usize * width;
            let end = (self.count - 1)
                .checked_mul(attribute.stride as usize)
                .and_then(|n| n.checked_add(attribute.offset))
                .and_then(|n| n.checked_add(bytes));
            if end.is_none_or(|n| n > self.bytes.len()) {
                return false;
            }
            stride = (stride + width - 1) & !(width - 1);
            fields[slot] = Some((*attribute, stride, bytes, matches!(slot, 1 | 6 | 7)));
            stride += bytes;
        }
        stride = (stride + 3) & !3;
        let Some(length) = count.checked_mul(stride) else {
            return false;
        };
        out.bytes.resize(length, 0);
        out.constants = self.constants;
        for (slot, field) in fields.iter().enumerate() {
            if let Some((attribute, offset, _, _)) = field {
                out.attributes[slot] = Some(StreamAttribute {
                    offset: *offset,
                    stride: stride as i32,
                    ..*attribute
                });
            }
        }
        let index = |corner: usize| {
            if self.indices.is_empty() {
                corner
            } else {
                self.indices[corner] as usize
            }
        };
        for (corner, record) in out.bytes.chunks_exact_mut(stride).enumerate() {
            let geometry = index(corner);
            let provoking = index(corner / 3 * 3 + 2);
            for (attribute, offset, bytes, flat) in fields.iter().flatten() {
                let source = attribute.offset
                    + if *flat { provoking } else { geometry } * attribute.stride as usize;
                record[*offset..*offset + *bytes]
                    .copy_from_slice(&self.bytes[source..source + *bytes]);
            }
        }
        out.count = count;
        out.flat_triangles = true;
        true
    }
}

/// Per-draw attribute layout. Disabled stages carry no payload. Smooth
/// lighting aliases the position attribute instead of uploading it twice.
#[derive(Clone, Copy, PartialEq, Eq)]
struct VertexLayout {
    attributes: [Option<(i32, usize)>; 10],
    floats: usize,
    prelit: bool,
    point_sizes: bool,
}
impl VertexLayout {
    fn new(
        state: &PipelineState,
        stages: &[TextureStage],
        transforms: Option<&VertexTransforms>,
    ) -> Self {
        Self::with_projective_mask(state, stages, transforms, 0)
    }
    fn with_projective_mask(
        state: &PipelineState,
        stages: &[TextureStage],
        transforms: Option<&VertexTransforms>,
        mask: u8,
    ) -> Self {
        let lighting = transforms.is_some_and(|t| t.lighting.enabled);
        let mut layout = Self {
            attributes: [None; 10],
            floats: 8,
            prelit: transforms.is_none(),
            point_sizes: false,
        };
        layout.attributes[0] = Some((4, 0));
        layout.attributes[1] = Some((4, 4));
        for stage in stages {
            let size = if mask & (1 << stage.unit) != 0 { 4 } else { 2 };
            layout.attributes[texture_attribute(stage.unit)] = Some((size, layout.floats));
            layout.floats += size as usize;
        }
        if lighting || (transforms.is_none() && (state.fog || state.two_sided_lighting)) {
            layout.attributes[6] = Some((4, layout.floats));
            layout.floats += 4;
        }
        if lighting {
            if state.smooth_shading {
                layout.attributes[7] = Some((4, 0));
            } else {
                layout.attributes[7] = Some((4, layout.floats));
                layout.floats += 4;
            }
        }
        layout
    }
    fn with_point_sizes(mut self) -> Self {
        if self.attributes[7].is_some_and(|(_, offset)| offset != 0) {
            self.floats -= 4;
        }
        self.attributes[7] = Some((1, self.floats));
        self.floats += 1;
        self.point_sizes = true;
        self
    }
    fn pack(&self, out: &mut Vec<f32>, vertex: Vertex, provoking: Vertex, smooth: bool) {
        out.extend(vertex.pos);
        out.extend(if smooth {
            vertex.color
        } else {
            provoking.color
        });
        for unit in 0..6 {
            if let Some((size, _)) = self.attributes[texture_attribute(unit)] {
                out.extend_from_slice(&vertex.texcoord[unit][..size as usize]);
            }
        }
        if self.attributes[6].is_some() {
            let source = if smooth { vertex } else { provoking };
            out.extend(if self.prelit {
                [
                    source.back_color[0],
                    source.back_color[1],
                    source.back_color[2],
                ]
            } else {
                source.normal
            });
            out.push(vertex.fog_depth);
        }
        if self.point_sizes {
            out.push(vertex.point_width);
        } else if self.attributes[7].is_some_and(|(_, offset)| offset != 0) {
            out.extend(provoking.pos);
        }
    }
}
// Context has already expanded guest-indexed flat primitives before reaching
// the non-indexed submission path. Independent triangles can be sent directly
// after copying their provoking attributes, just like smooth array draws.
fn direct_flat_triangles(mode: u32, smooth: bool, has_guest_indices: bool) -> bool {
    mode == GL_TRIANGLES && !smooth && !has_guest_indices
}

fn pack_flat_triangles(layout: &VertexLayout, out: &mut Vec<f32>, vertices: &[Vertex]) {
    for triangle in vertices.chunks_exact(3) {
        let provoking = triangle[2];
        for &vertex in triangle {
            layout.pack(out, vertex, provoking, false);
        }
    }
}

fn unique_records<const N: usize>(
    source: &[f32],
    floats: usize,
    unique: &mut HashMap<[u32; N], u16>,
    packed: &mut Vec<f32>,
    indices: &mut Vec<u16>,
) -> bool {
    debug_assert!(floats <= N);
    unique.clear();
    for record in source.chunks_exact(floats) {
        let key = std::array::from_fn(|i| record.get(i).map_or(0, |v| v.to_bits()));
        let index = if let Some(&index) = unique.get(&key) {
            index
        } else {
            if unique.len() == 65536 {
                return false;
            }
            let index = unique.len() as u16;
            unique.insert(key, index);
            packed.extend_from_slice(record);
            index
        };
        indices.push(index);
    }
    true
}

// Preserve normal/fog and provoking/point attributes at locations 6/7.
// Additional coordinate sets are active only in a wide shader variant.
pub(crate) fn texture_attribute(unit: usize) -> usize {
    if unit < 4 {
        unit + 2
    } else {
        unit + 4
    }
}
const TEXTURE_MATRICES: [&str; 6] = [
    "textureMatrix0",
    "textureMatrix1",
    "textureMatrix2",
    "textureMatrix3",
    "textureMatrix4",
    "textureMatrix5",
];
const SAMPLERS: [&str; 6] = [
    "sampler0", "sampler1", "sampler2", "sampler3", "sampler4", "sampler5",
];
const ENVIRONMENTS: [&str; 6] = ["env0", "env1", "env2", "env3", "env4", "env5"];
const LOD_LIMITS: [&str; 6] = [
    "lodLimit0",
    "lodLimit1",
    "lodLimit2",
    "lodLimit3",
    "lodLimit4",
    "lodLimit5",
];
const LOD_BIASES: [&str; 6] = [
    "lodBias0", "lodBias1", "lodBias2", "lodBias3", "lodBias4", "lodBias5",
];
const ENV_COLORS: [&str; 6] = [
    "envColor0",
    "envColor1",
    "envColor2",
    "envColor3",
    "envColor4",
    "envColor5",
];
const COMBINE_UNIFORMS: [[&str; 6]; 5] = [
    [
        "combineOps0",
        "combineOps1",
        "combineOps2",
        "combineOps3",
        "combineOps4",
        "combineOps5",
    ],
    [
        "combineRgbSource0",
        "combineRgbSource1",
        "combineRgbSource2",
        "combineRgbSource3",
        "combineRgbSource4",
        "combineRgbSource5",
    ],
    [
        "combineAlphaSource0",
        "combineAlphaSource1",
        "combineAlphaSource2",
        "combineAlphaSource3",
        "combineAlphaSource4",
        "combineAlphaSource5",
    ],
    [
        "combineRgbOperand0",
        "combineRgbOperand1",
        "combineRgbOperand2",
        "combineRgbOperand3",
        "combineRgbOperand4",
        "combineRgbOperand5",
    ],
    [
        "combineAlphaOperand0",
        "combineAlphaOperand1",
        "combineAlphaOperand2",
        "combineAlphaOperand3",
        "combineAlphaOperand4",
        "combineAlphaOperand5",
    ],
];
#[allow(non_snake_case)]
struct Gl {
    CopyTexSubImage2D: Option<unsafe extern "system" fn(u32, i32, i32, i32, i32, i32, i32, i32)>,
    CopyTexImage2D: unsafe extern "system" fn(u32, i32, u32, i32, i32, i32, i32, i32),
    CompressedTexImage2D:
        Option<unsafe extern "system" fn(u32, i32, u32, i32, i32, i32, i32, *const c_void)>,
    GetString: unsafe extern "system" fn(u32) -> *const u8,
    GetError: unsafe extern "system" fn() -> u32,
    Finish: unsafe extern "system" fn(),
    Flush: unsafe extern "system" fn(),
    GetIntegerv: unsafe extern "system" fn(u32, *mut i32),
    GetFloatv: unsafe extern "system" fn(u32, *mut f32),
    LineWidth: unsafe extern "system" fn(f32),
    CreateShader: unsafe extern "system" fn(u32) -> u32,
    ShaderSource: unsafe extern "system" fn(u32, i32, *const *const c_char, *const i32),
    CompileShader: unsafe extern "system" fn(u32),
    GetShaderiv: unsafe extern "system" fn(u32, u32, *mut i32),
    GetShaderInfoLog: unsafe extern "system" fn(u32, i32, *mut i32, *mut c_char),
    DeleteShader: unsafe extern "system" fn(u32),
    CreateProgram: unsafe extern "system" fn() -> u32,
    AttachShader: unsafe extern "system" fn(u32, u32),
    BindAttribLocation: unsafe extern "system" fn(u32, u32, *const c_char),
    LinkProgram: unsafe extern "system" fn(u32),
    GetProgramiv: unsafe extern "system" fn(u32, u32, *mut i32),
    GetProgramInfoLog: unsafe extern "system" fn(u32, i32, *mut i32, *mut c_char),
    DeleteProgram: unsafe extern "system" fn(u32),
    UseProgram: unsafe extern "system" fn(u32),
    GetUniformLocation: unsafe extern "system" fn(u32, *const c_char) -> i32,
    Uniform1i: unsafe extern "system" fn(i32, i32),
    Uniform1f: unsafe extern "system" fn(i32, f32),
    Uniform2f: unsafe extern "system" fn(i32, f32, f32),
    UniformMatrix4fv: unsafe extern "system" fn(i32, i32, u8, *const f32),
    Uniform4fv: unsafe extern "system" fn(i32, i32, *const f32),
    GenBuffers: unsafe extern "system" fn(i32, *mut u32),
    DeleteBuffers: unsafe extern "system" fn(i32, *const u32),
    BindBuffer: unsafe extern "system" fn(u32, u32),
    BufferData: unsafe extern "system" fn(u32, isize, *const c_void, u32),
    DisableVertexAttribArray: unsafe extern "system" fn(u32),
    EnableVertexAttribArray: unsafe extern "system" fn(u32),
    VertexAttrib4fv: unsafe extern "system" fn(u32, *const f32),
    VertexAttribPointer: unsafe extern "system" fn(u32, i32, u32, u8, i32, *const c_void),
    PolygonOffset: unsafe extern "system" fn(f32, f32),
    DrawElements: unsafe extern "system" fn(u32, i32, u32, *const c_void),
    DrawArrays: unsafe extern "system" fn(u32, i32, i32),
    GenFramebuffers: unsafe extern "system" fn(i32, *mut u32),
    DeleteFramebuffers: unsafe extern "system" fn(i32, *const u32),
    BindFramebuffer: unsafe extern "system" fn(u32, u32),
    CheckFramebufferStatus: unsafe extern "system" fn(u32) -> u32,
    FramebufferTexture2D: unsafe extern "system" fn(u32, u32, u32, u32, i32),
    GenRenderbuffers: unsafe extern "system" fn(i32, *mut u32),
    DeleteRenderbuffers: unsafe extern "system" fn(i32, *const u32),
    BindRenderbuffer: unsafe extern "system" fn(u32, u32),
    RenderbufferStorage: unsafe extern "system" fn(u32, u32, i32, i32),
    FramebufferRenderbuffer: unsafe extern "system" fn(u32, u32, u32, u32),
    GenTextures: unsafe extern "system" fn(i32, *mut u32),
    DeleteTextures: unsafe extern "system" fn(i32, *const u32),
    BindTexture: unsafe extern "system" fn(u32, u32),
    TexImage2D: unsafe extern "system" fn(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void),
    TexParameteri: unsafe extern "system" fn(u32, u32, i32),
    ActiveTexture: unsafe extern "system" fn(u32),
    PixelStorei: unsafe extern "system" fn(u32, i32),
    Enable: unsafe extern "system" fn(u32),
    Disable: unsafe extern "system" fn(u32),
    Viewport: unsafe extern "system" fn(i32, i32, i32, i32),
    Scissor: unsafe extern "system" fn(i32, i32, i32, i32),
    DepthFunc: unsafe extern "system" fn(u32),
    DepthMask: unsafe extern "system" fn(u8),
    BlendFunc: unsafe extern "system" fn(u32, u32),
    CullFace: unsafe extern "system" fn(u32),
    FrontFace: unsafe extern "system" fn(u32),
    ColorMask: unsafe extern "system" fn(u8, u8, u8, u8),
    ClearColor: unsafe extern "system" fn(f32, f32, f32, f32),
    Clear: unsafe extern "system" fn(u32),
    ReadPixels: unsafe extern "system" fn(i32, i32, i32, i32, u32, u32, *mut c_void),
}
impl Gl {
    unsafe fn load(host: &Host) -> Result<Self, String> {
        Ok(Self {
            CopyTexSubImage2D: {
                let p = host.load(c"glCopyTexSubImage2D");
                (!p.is_null()).then(|| {
                    std::mem::transmute::<
                        *mut c_void,
                        unsafe extern "system" fn(u32, i32, i32, i32, i32, i32, i32, i32),
                    >(p)
                })
            },
            CopyTexImage2D: {
                let p = host.load(c"glCopyTexImage2D");
                if p.is_null() {
                    return Err("driver lacks glCopyTexImage2D".into());
                }
                std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "system" fn(u32, i32, u32, i32, i32, i32, i32, i32),
                >(p)
            },
            CompressedTexImage2D: {
                let mut p = host.load(c"glCompressedTexImage2D");
                if p.is_null() {
                    p = host.load(c"glCompressedTexImage2DARB");
                }
                if p.is_null() {
                    None
                } else {
                    Some(std::mem::transmute::<
                        *mut c_void,
                        unsafe extern "system" fn(u32, i32, u32, i32, i32, i32, i32, *const c_void),
                    >(p))
                }
            },
            GetString: {
                let mut p = host.load(c"glGetString");
                if p.is_null() {
                    p = host.load(c"glGetStringEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glGetString".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32) -> *const u8>(p)
            },
            GetError: {
                let mut p = host.load(c"glGetError");
                if p.is_null() {
                    p = host.load(c"glGetErrorEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glGetError".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn() -> u32>(p)
            },
            Finish: {
                let p = host.load(c"glFinish");
                if p.is_null() {
                    return Err("driver lacks glFinish".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn()>(p)
            },
            Flush: {
                let p = host.load(c"glFlush");
                if p.is_null() {
                    return Err("driver lacks glFlush".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn()>(p)
            },
            GetFloatv: {
                let p = host.load(c"glGetFloatv");
                if p.is_null() {
                    return Err("driver lacks glGetFloatv".into());
                }
                std::mem::transmute::<*const c_void, unsafe extern "system" fn(u32, *mut f32)>(p)
            },
            LineWidth: {
                let p = host.load(c"glLineWidth");
                if p.is_null() {
                    return Err("driver lacks glLineWidth".into());
                }
                std::mem::transmute::<*const c_void, unsafe extern "system" fn(f32)>(p)
            },
            GetIntegerv: {
                let mut p = host.load(c"glGetIntegerv");
                if p.is_null() {
                    p = host.load(c"glGetIntegervEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glGetIntegerv".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32, *mut i32)>(p)
            },
            CreateShader: {
                let mut p = host.load(c"glCreateShader");
                if p.is_null() {
                    p = host.load(c"glCreateShaderEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glCreateShader".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32) -> u32>(p)
            },
            ShaderSource: {
                let mut p = host.load(c"glShaderSource");
                if p.is_null() {
                    p = host.load(c"glShaderSourceEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glShaderSource".into());
                }
                std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "system" fn(u32, i32, *const *const c_char, *const i32),
                >(p)
            },
            CompileShader: {
                let mut p = host.load(c"glCompileShader");
                if p.is_null() {
                    p = host.load(c"glCompileShaderEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glCompileShader".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32)>(p)
            },
            GetShaderiv: {
                let mut p = host.load(c"glGetShaderiv");
                if p.is_null() {
                    p = host.load(c"glGetShaderivEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glGetShaderiv".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32, u32, *mut i32)>(p)
            },
            GetShaderInfoLog: {
                let mut p = host.load(c"glGetShaderInfoLog");
                if p.is_null() {
                    p = host.load(c"glGetShaderInfoLogEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glGetShaderInfoLog".into());
                }
                std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "system" fn(u32, i32, *mut i32, *mut c_char),
                >(p)
            },
            DeleteShader: {
                let mut p = host.load(c"glDeleteShader");
                if p.is_null() {
                    p = host.load(c"glDeleteShaderEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glDeleteShader".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32)>(p)
            },
            CreateProgram: {
                let mut p = host.load(c"glCreateProgram");
                if p.is_null() {
                    p = host.load(c"glCreateProgramEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glCreateProgram".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn() -> u32>(p)
            },
            AttachShader: {
                let mut p = host.load(c"glAttachShader");
                if p.is_null() {
                    p = host.load(c"glAttachShaderEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glAttachShader".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32, u32)>(p)
            },
            BindAttribLocation: {
                let mut p = host.load(c"glBindAttribLocation");
                if p.is_null() {
                    p = host.load(c"glBindAttribLocationEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glBindAttribLocation".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32, u32, *const c_char)>(
                    p,
                )
            },
            LinkProgram: {
                let mut p = host.load(c"glLinkProgram");
                if p.is_null() {
                    p = host.load(c"glLinkProgramEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glLinkProgram".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32)>(p)
            },
            GetProgramiv: {
                let mut p = host.load(c"glGetProgramiv");
                if p.is_null() {
                    p = host.load(c"glGetProgramivEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glGetProgramiv".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32, u32, *mut i32)>(p)
            },
            GetProgramInfoLog: {
                let mut p = host.load(c"glGetProgramInfoLog");
                if p.is_null() {
                    p = host.load(c"glGetProgramInfoLogEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glGetProgramInfoLog".into());
                }
                std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "system" fn(u32, i32, *mut i32, *mut c_char),
                >(p)
            },
            DeleteProgram: {
                let mut p = host.load(c"glDeleteProgram");
                if p.is_null() {
                    p = host.load(c"glDeleteProgramEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glDeleteProgram".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32)>(p)
            },
            UseProgram: {
                let mut p = host.load(c"glUseProgram");
                if p.is_null() {
                    p = host.load(c"glUseProgramEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glUseProgram".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32)>(p)
            },
            GetUniformLocation: {
                let mut p = host.load(c"glGetUniformLocation");
                if p.is_null() {
                    p = host.load(c"glGetUniformLocationEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glGetUniformLocation".into());
                }
                std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "system" fn(u32, *const c_char) -> i32,
                >(p)
            },
            Uniform1i: {
                let mut p = host.load(c"glUniform1i");
                if p.is_null() {
                    p = host.load(c"glUniform1iEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glUniform1i".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(i32, i32)>(p)
            },
            Uniform1f: {
                let mut p = host.load(c"glUniform1f");
                if p.is_null() {
                    p = host.load(c"glUniform1fEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glUniform1f".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(i32, f32)>(p)
            },
            Uniform2f: {
                let mut p = host.load(c"glUniform2f");
                if p.is_null() {
                    p = host.load(c"glUniform2fEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glUniform2f".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(i32, f32, f32)>(p)
            },
            UniformMatrix4fv: {
                let p = host.load(c"glUniformMatrix4fv");
                if p.is_null() {
                    return Err("driver lacks glUniformMatrix4fv".into());
                }
                std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "system" fn(i32, i32, u8, *const f32),
                >(p)
            },
            Uniform4fv: {
                let mut p = host.load(c"glUniform4fv");
                if p.is_null() {
                    p = host.load(c"glUniform4fvEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glUniform4fv".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(i32, i32, *const f32)>(
                    p,
                )
            },
            GenBuffers: {
                let mut p = host.load(c"glGenBuffers");
                if p.is_null() {
                    p = host.load(c"glGenBuffersEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glGenBuffers".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(i32, *mut u32)>(p)
            },
            DeleteBuffers: {
                let mut p = host.load(c"glDeleteBuffers");
                if p.is_null() {
                    p = host.load(c"glDeleteBuffersEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glDeleteBuffers".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(i32, *const u32)>(p)
            },
            BindBuffer: {
                let mut p = host.load(c"glBindBuffer");
                if p.is_null() {
                    p = host.load(c"glBindBufferEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glBindBuffer".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32, u32)>(p)
            },
            BufferData: {
                let mut p = host.load(c"glBufferData");
                if p.is_null() {
                    p = host.load(c"glBufferDataEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glBufferData".into());
                }
                std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "system" fn(u32, isize, *const c_void, u32),
                >(p)
            },
            DisableVertexAttribArray: {
                let p = host.load(c"glDisableVertexAttribArray");
                if p.is_null() {
                    return Err("driver lacks glDisableVertexAttribArray".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32)>(p)
            },
            EnableVertexAttribArray: {
                let mut p = host.load(c"glEnableVertexAttribArray");
                if p.is_null() {
                    p = host.load(c"glEnableVertexAttribArrayEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glEnableVertexAttribArray".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32)>(p)
            },
            VertexAttrib4fv: {
                let p = host.load(c"glVertexAttrib4fv");
                if p.is_null() {
                    return Err("driver lacks glVertexAttrib4fv".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32, *const f32)>(p)
            },
            VertexAttribPointer: {
                let mut p = host.load(c"glVertexAttribPointer");
                if p.is_null() {
                    p = host.load(c"glVertexAttribPointerEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glVertexAttribPointer".into());
                }
                std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "system" fn(u32, i32, u32, u8, i32, *const c_void),
                >(p)
            },
            PolygonOffset: {
                let p = host.load(c"glPolygonOffset");
                if p.is_null() {
                    return Err("driver lacks glPolygonOffset".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(f32, f32)>(p)
            },
            DrawElements: {
                let p = host.load(c"glDrawElements");
                if p.is_null() {
                    return Err("driver lacks glDrawElements".into());
                }
                std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "system" fn(u32, i32, u32, *const c_void),
                >(p)
            },
            DrawArrays: {
                let mut p = host.load(c"glDrawArrays");
                if p.is_null() {
                    p = host.load(c"glDrawArraysEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glDrawArrays".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32, i32, i32)>(p)
            },
            GenFramebuffers: {
                let mut p = host.load(c"glGenFramebuffers");
                if p.is_null() {
                    p = host.load(c"glGenFramebuffersEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glGenFramebuffers".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(i32, *mut u32)>(p)
            },
            DeleteFramebuffers: {
                let mut p = host.load(c"glDeleteFramebuffers");
                if p.is_null() {
                    p = host.load(c"glDeleteFramebuffersEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glDeleteFramebuffers".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(i32, *const u32)>(p)
            },
            BindFramebuffer: {
                let mut p = host.load(c"glBindFramebuffer");
                if p.is_null() {
                    p = host.load(c"glBindFramebufferEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glBindFramebuffer".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32, u32)>(p)
            },
            CheckFramebufferStatus: {
                let mut p = host.load(c"glCheckFramebufferStatus");
                if p.is_null() {
                    p = host.load(c"glCheckFramebufferStatusEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glCheckFramebufferStatus".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32) -> u32>(p)
            },
            FramebufferTexture2D: {
                let mut p = host.load(c"glFramebufferTexture2D");
                if p.is_null() {
                    p = host.load(c"glFramebufferTexture2DEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glFramebufferTexture2D".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32, u32, u32, u32, i32)>(
                    p,
                )
            },
            GenRenderbuffers: {
                let mut p = host.load(c"glGenRenderbuffers");
                if p.is_null() {
                    p = host.load(c"glGenRenderbuffersEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glGenRenderbuffers".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(i32, *mut u32)>(p)
            },
            DeleteRenderbuffers: {
                let mut p = host.load(c"glDeleteRenderbuffers");
                if p.is_null() {
                    p = host.load(c"glDeleteRenderbuffersEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glDeleteRenderbuffers".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(i32, *const u32)>(p)
            },
            BindRenderbuffer: {
                let mut p = host.load(c"glBindRenderbuffer");
                if p.is_null() {
                    p = host.load(c"glBindRenderbufferEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glBindRenderbuffer".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32, u32)>(p)
            },
            RenderbufferStorage: {
                let mut p = host.load(c"glRenderbufferStorage");
                if p.is_null() {
                    p = host.load(c"glRenderbufferStorageEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glRenderbufferStorage".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32, u32, i32, i32)>(p)
            },
            FramebufferRenderbuffer: {
                let mut p = host.load(c"glFramebufferRenderbuffer");
                if p.is_null() {
                    p = host.load(c"glFramebufferRenderbufferEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glFramebufferRenderbuffer".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32, u32, u32, u32)>(p)
            },
            GenTextures: {
                let mut p = host.load(c"glGenTextures");
                if p.is_null() {
                    p = host.load(c"glGenTexturesEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glGenTextures".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(i32, *mut u32)>(p)
            },
            DeleteTextures: {
                let mut p = host.load(c"glDeleteTextures");
                if p.is_null() {
                    p = host.load(c"glDeleteTexturesEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glDeleteTextures".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(i32, *const u32)>(p)
            },
            BindTexture: {
                let mut p = host.load(c"glBindTexture");
                if p.is_null() {
                    p = host.load(c"glBindTextureEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glBindTexture".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32, u32)>(p)
            },
            TexImage2D: {
                let mut p = host.load(c"glTexImage2D");
                if p.is_null() {
                    p = host.load(c"glTexImage2DEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glTexImage2D".into());
                }
                std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "system" fn(
                        u32,
                        i32,
                        i32,
                        i32,
                        i32,
                        i32,
                        u32,
                        u32,
                        *const c_void,
                    ),
                >(p)
            },
            TexParameteri: {
                let mut p = host.load(c"glTexParameteri");
                if p.is_null() {
                    p = host.load(c"glTexParameteriEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glTexParameteri".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32, u32, i32)>(p)
            },
            ActiveTexture: {
                let mut p = host.load(c"glActiveTexture");
                if p.is_null() {
                    p = host.load(c"glActiveTextureEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glActiveTexture".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32)>(p)
            },
            PixelStorei: {
                let mut p = host.load(c"glPixelStorei");
                if p.is_null() {
                    p = host.load(c"glPixelStoreiEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glPixelStorei".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32, i32)>(p)
            },
            Enable: {
                let mut p = host.load(c"glEnable");
                if p.is_null() {
                    p = host.load(c"glEnableEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glEnable".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32)>(p)
            },
            Disable: {
                let mut p = host.load(c"glDisable");
                if p.is_null() {
                    p = host.load(c"glDisableEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glDisable".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32)>(p)
            },
            Viewport: {
                let mut p = host.load(c"glViewport");
                if p.is_null() {
                    p = host.load(c"glViewportEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glViewport".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(i32, i32, i32, i32)>(p)
            },
            Scissor: {
                let mut p = host.load(c"glScissor");
                if p.is_null() {
                    p = host.load(c"glScissorEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glScissor".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(i32, i32, i32, i32)>(p)
            },
            DepthFunc: {
                let mut p = host.load(c"glDepthFunc");
                if p.is_null() {
                    p = host.load(c"glDepthFuncEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glDepthFunc".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32)>(p)
            },
            DepthMask: {
                let mut p = host.load(c"glDepthMask");
                if p.is_null() {
                    p = host.load(c"glDepthMaskEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glDepthMask".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u8)>(p)
            },
            BlendFunc: {
                let mut p = host.load(c"glBlendFunc");
                if p.is_null() {
                    p = host.load(c"glBlendFuncEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glBlendFunc".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32, u32)>(p)
            },
            CullFace: {
                let mut p = host.load(c"glCullFace");
                if p.is_null() {
                    p = host.load(c"glCullFaceEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glCullFace".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32)>(p)
            },
            FrontFace: {
                let mut p = host.load(c"glFrontFace");
                if p.is_null() {
                    p = host.load(c"glFrontFaceEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glFrontFace".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32)>(p)
            },
            ColorMask: {
                let mut p = host.load(c"glColorMask");
                if p.is_null() {
                    p = host.load(c"glColorMaskEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glColorMask".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u8, u8, u8, u8)>(p)
            },
            ClearColor: {
                let mut p = host.load(c"glClearColor");
                if p.is_null() {
                    p = host.load(c"glClearColorEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glClearColor".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(f32, f32, f32, f32)>(p)
            },
            Clear: {
                let mut p = host.load(c"glClear");
                if p.is_null() {
                    p = host.load(c"glClearEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glClear".into());
                }
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32)>(p)
            },
            ReadPixels: {
                let mut p = host.load(c"glReadPixels");
                if p.is_null() {
                    p = host.load(c"glReadPixelsEXT");
                }
                if p.is_null() {
                    return Err("driver lacks glReadPixels".into());
                }
                std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "system" fn(i32, i32, i32, i32, u32, u32, *mut c_void),
                >(p)
            },
        })
    }
}
fn native_compressed_format(format: u32) -> u32 {
    // RGB DXT1 has opaque alpha even for selector 3. Never reinterpret it
    // as RGBA DXT1 on the native path; unsupported hosts use the decoder.
    format
}

#[derive(Debug, PartialEq, Eq)]
enum UploadRoute {
    Compressed,
    Rgba,
}
unsafe fn check_native_error(gl: &Gl) -> Result<(), String> {
    let error = (gl.GetError)();
    if error == 0 {
        Ok(())
    } else {
        Err(format!("host OpenGL error 0x{error:04x}"))
    }
}

unsafe fn upload_error(gl: &Gl) -> Result<u32, String> {
    let mut first = 0;
    for _ in 0..16 {
        let error = (gl.GetError)();
        if error == 0 {
            break;
        }
        if error == GL_OUT_OF_MEMORY {
            return Err("native texture allocation failed".into());
        }
        if first == 0 {
            first = error;
        }
    }
    Ok(first)
}
unsafe fn upload_texture(
    gl: &Gl,
    formats: &HashSet<u32>,
    tex: &Texture,
    mips: bool,
    native_max_level: bool,
) -> Result<UploadRoute, String> {
    if !tex.is_complete() {
        return Err("incomplete guest texture must not reach GPU upload".into());
    }
    let rebased;
    let tex = if tex.base_level != 0 {
        rebased = tex.host_view();
        &rebased
    } else {
        tex
    };
    let full_levels = tex.width.max(tex.height).ilog2() + 1;
    let levels = full_levels.min(tex.max_level.saturating_add(1));

    if let Some(base) = tex.compressed.get(&0) {
        let format = native_compressed_format(base.format);
        let complete = (0..if mips { levels } else { 1 }).all(|l| {
            tex.compressed.get(&l).is_some_and(|c| {
                c.width == (tex.width >> l).max(1)
                    && c.height == (tex.height >> l).max(1)
                    && native_compressed_format(c.format) == format
            })
        });
        if formats.contains(&format)
            && (!mips || (complete && (levels == full_levels || native_max_level)))
        {
            if let Some(upload) = gl.CompressedTexImage2D {
                for l in 0..if mips && complete { levels } else { 1 } {
                    let c = &tex.compressed[&l];
                    upload(
                        GL_TEXTURE_2D,
                        l as i32,
                        format,
                        c.width as i32,
                        c.height as i32,
                        0,
                        c.bytes.len() as i32,
                        c.bytes.as_ptr().cast(),
                    );
                }
                let ok = upload_error(gl)? == 0;
                if ok {
                    return Ok(UploadRoute::Compressed);
                }
            }
        }
    }
    let base = tex
        .rgba_level(0)
        .ok_or("guest texture pixels are unavailable")?;
    (gl.TexImage2D)(
        GL_TEXTURE_2D,
        0,
        GL_RGBA as i32,
        tex.width as i32,
        tex.height as i32,
        0,
        GL_RGBA,
        GL_UNSIGNED_BYTE,
        base.as_ptr().cast(),
    );
    let error = upload_error(gl)?;
    if error != 0 {
        return Err(format!("native RGBA texture upload failed: 0x{error:04x}"));
    }
    if mips {
        let levels = if native_max_level {
            tex.upload_levels()
        } else {
            host_mipmap_levels(tex)
        };
        for (l, (w, h, pixels)) in levels.iter().enumerate().skip(1) {
            (gl.TexImage2D)(
                GL_TEXTURE_2D,
                l as i32,
                GL_RGBA as i32,
                *w as i32,
                *h as i32,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                pixels.as_ptr().cast(),
            );
        }
    }
    Ok(UploadRoute::Rgba)
}

/// ES2 needs a full host chain even when SGIS permits a shorter guest chain.
/// Tail images are host-only padding: LOD stays clamped to the guest MAX_LEVEL.
fn host_mipmap_levels(tex: &Texture) -> Vec<(u32, u32, Vec<u8>)> {
    let mut levels = tex.upload_levels();
    while levels.last().is_some_and(|(w, h, _)| *w > 1 || *h > 1) {
        let (w, h, src) = levels.last().unwrap();
        let (nw, nh) = ((*w / 2).max(1), (*h / 2).max(1));
        let mut dst = vec![0; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for channel in 0..4 {
                    let mut sum = 0u32;
                    for dy in 0..2 {
                        for dx in 0..2 {
                            let i = (((y * 2 + dy).min(*h - 1) * *w + (x * 2 + dx).min(*w - 1)) * 4)
                                as usize
                                + channel;
                            sum += src[i] as u32;
                        }
                    }
                    dst[((y * nw + x) * 4) as usize + channel] = ((sum + 2) / 4) as u8;
                }
            }
        }
        levels.push((nw, nh, dst));
    }
    levels
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum SpecializedShader {
    Combiner(u64),
    WideFixed,
}

type ProgramState = (u32, HashMap<String, i32>, HashMap<i32, Vec<u32>>);

pub struct Renderer {
    renderer_name: String,
    diagnostic: Option<std::rc::Rc<RefCell<diagnostic::Timer>>>,
    host: Host,
    gl: Gl,
    program: u32,
    alternate_program: Option<(u32, HashMap<String, i32>, HashMap<i32, Vec<u32>>)>,
    affine_program: bool,
    combiner_programs: HashMap<(SpecializedShader, bool), ProgramState>,
    combiner: Option<(
        std::sync::Arc<crate::combiner_program::BasicProgram>,
        [[f32; 4]; 4],
    )>,
    range_program: Option<(u32, HashMap<String, i32>, HashMap<i32, Vec<u32>>)>,
    range_active: bool,
    untextured_program: Option<(u32, HashMap<String, i32>, HashMap<i32, Vec<u32>>)>,
    untextured_active: bool,
    simple_program: Option<(u32, HashMap<String, i32>, HashMap<i32, Vec<u32>>)>,
    simple_active: bool,
    simple_available: bool,
    simple_run: u8,
    buffer: u32,
    index_buffer: u32,
    indices: Vec<u16>,
    packed_scratch: Vec<f32>,
    vertex_keys: HashMap<[u32; 32], u16>,
    wide_vertex_keys: HashMap<[u32; 48], u16>,
    uniform_values: HashMap<i32, Vec<u32>>,
    pipeline: Option<PipelineState>,
    point_sprite_mode: bool,
    point_drawing: bool,
    texture_params: HashMap<u32, (Filter, Filter, Wrap, Wrap)>,
    fbo: u32,
    upload_source: u32,
    upload_transfer_failed: bool,
    transfers: HashMap<(u32, bool, transfer::Quantization), transfer::TransferPass>,
    logic_destination: u32,
    destination_format: Option<(u32, u32, bool)>,
    rgb565: bool,
    color: u32,
    depth: u32,
    depth_format: u32,
    depth_bias_step: Option<(u32, f32)>,
    white: u32,
    width: u32,
    height: u32,
    render_scale: u32,
    render_policy: u8,
    textures: HashMap<u32, (u32, u64)>,
    uniforms: HashMap<String, i32>,
    scratch: Vec<f32>,
    readback: Vec<u8>,
    display_target: Option<(u32, u32, u32, u32)>,
    display_prepared: bool,
    packed_target: Option<(u32, u32, u32, u32)>,
    packed_readback: Vec<u8>,
    logical_packed: Vec<u8>,
    logical_resolve: Option<resolve::LogicalResolve>,
    shared_frames: Option<shared::FramePool>,
    shared_export_failed: bool,
    logical_resolve_disabled: bool,
    dirty: bool,
    full_npot: bool,
    native_max_level: bool,
    lod_derivatives: bool,
    compressed_formats: HashSet<u32>,
    attribute_layout: Option<VertexLayout>,
    stream_layout: Option<[Option<StreamAttribute>; 10]>,
    native_fixed: bool,
    combiner_available: bool,
    wide_textures: bool,
    last_wide_draw: bool,
    pub point_range: [f32; 2],
    pub line_range: [f32; 2],
    pub max_viewport: [i32; 2],
    pub subpixel_bits: i32,
}
fn render_scale_for(policy: u8, width: u32, height: u32) -> u32 {
    match policy {
        2 => 2,
        3 if matches!((width, height), (320, 240) | (240, 320)) => 2,
        4 if matches!((width, height), (320, 240) | (240, 320)) => 3,
        5 if matches!((width, height), (320, 240) | (240, 320)) => 4,
        _ => 1,
    }
}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        let source = SHARED_CONTEXT_SOURCE.with(|s| s.borrow().clone());
        let host = if let Some(source) = source {
            match Host::shared(&source) {
                Ok(host) => host,
                Err(error) => {
                    log::warn!(
                        "UI context sharing unavailable; retaining CPU presentation: {error}"
                    );
                    Host::new()?
                }
            }
        } else {
            Host::new()?
        };
        Self::with_host(host, width, height)
    }
    /// Create an independent renderer in an existing renderer's native share
    /// group. This does not share guest GL state or publish a presentation.
    /// A closed source returns an error; the non-Send renderer never moves.
    pub fn new_shared(
        source: &SharedContextSource,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        Self::with_host(Host::shared(source)?, width, height)
    }
    /// Sendable creation capability, invalidated when this renderer closes.
    /// Already-created children remain valid and can issue their own source.
    pub fn shared_context_source(&self) -> SharedContextSource {
        self.host.shared_source()
    }
    #[cfg(all(test, unix))]
    pub(crate) fn new_es_for_test(width: u32, height: u32) -> Result<Self, String> {
        Self::with_host(Host::es_for_test()?, width, height)
    }
    #[cfg(all(test, unix))]
    pub(crate) fn es2_for_test(width: u32, height: u32) -> Result<Self, String> {
        Self::with_host(Host::es_for_test()?, width, height)
    }
    fn with_host(host: Host, width: u32, height: u32) -> Result<Self, String> {
        unsafe {
            let gl = Gl::load(&host)?;
            let mut units = 0;
            let mut attributes = 0;
            (gl.GetIntegerv)(0x8872, &mut units);
            (gl.GetIntegerv)(0x8869, &mut attributes);
            if units < 4 || attributes < 8 {
                return Err("OpenGL driver lacks required texture/attribute units".into());
            }
            let mut varying = 0;
            (gl.GetIntegerv)(if host.es { 0x8dfc } else { 0x8b4b }, &mut varying);
            if !host.es {
                varying /= 4;
            }
            let wide_textures = attributes >= 10 && units >= 7 && varying >= 10;
            let mut max_viewport = [0; 2];
            let mut subpixel_bits = 0;
            (gl.GetIntegerv)(0x0d3a, max_viewport.as_mut_ptr());
            (gl.GetIntegerv)(0x0d50, &mut subpixel_bits);
            max_viewport = max_viewport.map(|v| v.max(1));
            let mut point_range = [1.; 2];
            let mut line_range = [1.; 2];
            (gl.GetFloatv)(0x846d, point_range.as_mut_ptr());
            (gl.GetFloatv)(0x846e, line_range.as_mut_ptr());
            for range in [&mut point_range, &mut line_range] {
                range[0] = range[0].max(1.);
                range[1] = range[1].clamp(range[0], 64.);
            }
            let full_npot = if host.es {
                let extensions = (gl.GetString)(0x1f03);
                let version = (gl.GetString)(0x1f02);
                (!extensions.is_null()
                    && CStr::from_ptr(extensions.cast())
                        .to_string_lossy()
                        .split_whitespace()
                        .any(|s| s == "GL_OES_texture_npot"))
                    || (!version.is_null()
                        && CStr::from_ptr(version.cast())
                            .to_string_lossy()
                            .starts_with("OpenGL ES 3."))
            } else {
                true
            };
            // Software retains float depth. A forced 16-bit attachment makes
            // nearby terrain/decal layers collapse at distance (RBR Android).
            let depth24 = if host.es {
                let extensions = (gl.GetString)(0x1f03);
                let version = (gl.GetString)(0x1f02);
                (!extensions.is_null()
                    && CStr::from_ptr(extensions.cast())
                        .to_string_lossy()
                        .split_whitespace()
                        .any(|s| s == "GL_OES_depth24"))
                    || (!version.is_null()
                        && CStr::from_ptr(version.cast())
                            .to_string_lossy()
                            .starts_with("OpenGL ES 3."))
            } else {
                true
            };
            let extensions_ptr = (gl.GetString)(0x1f03);
            let extensions = if extensions_ptr.is_null() {
                String::new()
            } else {
                CStr::from_ptr(extensions_ptr.cast())
                    .to_string_lossy()
                    .into_owned()
            };
            let version_ptr = (gl.GetString)(0x1f02);
            let es3 = !version_ptr.is_null()
                && CStr::from_ptr(version_ptr.cast())
                    .to_string_lossy()
                    .starts_with("OpenGL ES 3.");
            let native_max_level = !host.es
                || es3
                || extensions
                    .split_whitespace()
                    .any(|e| e == "GL_APPLE_texture_max_level");
            let lod_derivatives = !host.es
                || extensions
                    .split_whitespace()
                    .any(|e| e == "GL_OES_standard_derivatives");
            let native_fixed = host.es
                || extensions
                    .split_whitespace()
                    .any(|e| e == "GL_ARB_ES2_compatibility");
            let mut count = 0;
            (gl.GetIntegerv)(GL_NUM_COMPRESSED_TEXTURE_FORMATS, &mut count);
            let mut formats = vec![0i32; count.clamp(0, 128) as usize];
            if count > 0 && count <= 128 {
                (gl.GetIntegerv)(GL_COMPRESSED_TEXTURE_FORMATS, formats.as_mut_ptr());
            }
            let compressed_formats = formats.into_iter().map(|n| n as u32).collect();
            let diagnostic = diagnostic::Timer::new(&host, &gl);
            let mut r = Self {
                renderer_name: String::new(),
                diagnostic,
                host,
                gl,
                program: 0,
                alternate_program: None,
                affine_program: false,
                combiner_programs: HashMap::new(),
                combiner: None,
                range_program: None,
                range_active: false,
                untextured_program: None,
                untextured_active: false,
                simple_program: None,
                simple_active: false,
                simple_available: true,
                simple_run: 0,
                buffer: 0,
                index_buffer: 0,
                indices: Vec::new(),
                packed_scratch: Vec::new(),
                vertex_keys: HashMap::new(),
                wide_vertex_keys: HashMap::new(),
                uniform_values: HashMap::new(),
                pipeline: None,
                point_sprite_mode: false,
                point_drawing: false,
                texture_params: HashMap::new(),
                fbo: 0,
                upload_source: 0,
                upload_transfer_failed: false,
                transfers: HashMap::new(),
                logic_destination: 0,
                destination_format: None,
                rgb565: false,
                color: 0,
                depth: 0,
                depth_format: if depth24 { 0x81a6 } else { 0x81a5 },
                depth_bias_step: None,
                white: 0,
                width: 0,
                height: 0,
                textures: HashMap::new(),
                uniforms: HashMap::new(),
                scratch: Vec::new(),
                readback: Vec::new(),
                display_target: None,
                display_prepared: false,
                packed_target: None,
                packed_readback: Vec::new(),
                logical_packed: Vec::new(),
                logical_resolve: None,
                shared_frames: None,
                shared_export_failed: false,
                logical_resolve_disabled: false,
                render_scale: 1,
                render_policy: 1,
                dirty: true,
                full_npot,
                native_max_level,
                lod_derivatives,
                compressed_formats,
                attribute_layout: None,
                stream_layout: None,
                native_fixed,
                combiner_available: units >= 5,
                wide_textures,
                last_wide_draw: false,
                point_range,
                line_range,
                max_viewport,
                subpixel_bits,
            };
            r.program = r.compile_program(VERTEX, FRAGMENT)?;
            (r.gl.GenBuffers)(1, &mut r.buffer);
            (r.gl.GenBuffers)(1, &mut r.index_buffer);
            (r.gl.GenFramebuffers)(1, &mut r.fbo);
            (r.gl.GenTextures)(1, &mut r.color);
            (r.gl.GenRenderbuffers)(1, &mut r.depth);
            (r.gl.GenTextures)(1, &mut r.white);
            (r.gl.ActiveTexture)(GL_TEXTURE0);
            (r.gl.BindTexture)(GL_TEXTURE_2D, r.white);
            (r.gl.TexImage2D)(
                GL_TEXTURE_2D,
                0,
                GL_RGBA as i32,
                1,
                1,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                [255u8; 4].as_ptr().cast(),
            );
            r.params(
                Filter::Nearest,
                Filter::Nearest,
                Wrap::ClampToEdge,
                Wrap::ClampToEdge,
            );
            r.resize(width, height)?;
            for name in [
                "lightingEnabled",
                "normalMatrix",
                "normalMode",
                "normalRescale",
                "colorMaterial",
                "twoSided",
                "modelAmbient",
                "materialAmbient",
                "materialDiffuse",
                "materialSpecular",
                "materialEmission",
                "shininess",
                "lightAmbient[0]",
                "lightDiffuse[0]",
                "lightSpecular[0]",
                "lightPositions[0]",
                "lightSpot[0]",
                "lightAttenuation[0]",
                "projectionMatrix",
                "modelviewMatrix",
                "objectSpace",
                "textureMatrix0",
                "textureMatrix1",
                "textureMatrix2",
                "textureMatrix3",
                "textureMatrix4",
                "textureMatrix5",
                "depthRange",
                "pointSize",
                "pointArray",
                "pointArrayParams",
                "pointReplace",
                "pointReplaceHigh",
                "logicMode",
                "logicDestination",
                "logicSize",
                "logicLevels",
                "alphaMode",
                "alphaRef",
                "fogMode",
                "fogColor",
                "fogStart",
                "fogEnd",
                "fogDensity",
            ] {
                r.cache_uniform(name);
            }
            for i in 0..6 {
                for name in [
                    "sampler",
                    "env",
                    "envColor",
                    "lodBias",
                    "lodLimit",
                    "lodRange",
                    "combineOps",
                    "combineRgbSource",
                    "combineAlphaSource",
                    "combineRgbOperand",
                    "combineAlphaOperand",
                ] {
                    r.cache_uniform(&format!("{name}{i}"));
                }
            }
            // Compile once at startup; switching never compiles or reallocates shaders.
            let generic = r.program;
            let (vertex, fragment) = affine_shader_sources();
            let affine = r.compile_program(&vertex, &fragment)?;
            let locations = r
                .uniforms
                .keys()
                .map(|name| {
                    let location = (r.gl.GetUniformLocation)(
                        affine,
                        CString::new(name.as_str()).unwrap().as_ptr(),
                    );
                    (name.clone(), location)
                })
                .collect();
            r.alternate_program = Some((affine, locations, HashMap::new()));
            r.program = generic;
            // Prepare the no-texture variant now, avoiding a first-use hitch.
            r.select_affine_program(true);
            if let Err(error) = r.select_simple_program(true) {
                log::warn!("Simple GPU shader unavailable; keeping complete shaders: {error}");
                r.simple_available = false;
            }
            if r.simple_active {
                r.select_simple_program(false)?;
            }
            r.select_untextured_program(true)?;
            r.select_untextured_program(false)?;
            r.select_affine_program(false);
            r.point_drawing = false;
            let p = (r.gl.GetString)(0x1f01);
            let label = if p.is_null() {
                "unknown".into()
            } else {
                CStr::from_ptr(p.cast()).to_string_lossy().into_owned()
            };
            log::info!("GLES host OpenGL renderer: {label}");
            r.renderer_name = label;
            r.check()?;
            if r.native_fixed {
                // An advertised format is insufficient: some hosts move exact
                // 16.16 clip-boundary coordinates outside the visible volume.
                // Keep the existing float conversion when native fetch differs.
                r.native_fixed = match fixed_vertices::validate(&mut r) {
                    Ok(valid) => valid,
                    Err(error) => {
                        log::warn!("Native GL_FIXED validation unavailable: {error}");
                        false
                    }
                };
                log::info!(
                    "GLES GL_FIXED vertex conversion: {}",
                    if r.native_fixed {
                        "native (validated)"
                    } else {
                        "CPU float conversion (native validation failed)"
                    }
                );
            }
            Ok(r)
        }
    }
    /// Driver identity captured while this renderer's host context was current.
    pub fn renderer_name(&self) -> &str {
        &self.renderer_name
    }

    unsafe fn compile_program(&mut self, vertex: &str, fragment: &str) -> Result<u32, String> {
        let program = (self.gl.CreateProgram)();
        if program == 0 {
            return Err("OpenGL program allocation failed".into());
        }
        let prefix = if self.host.es {
            "#version 100\nprecision mediump float;\nprecision mediump int;\n#define VARYING varying mediump\n"
        } else {
            "#version 120\n#define VARYING varying\n"
        };
        let es3 = self.host.es
            && (fragment.contains("#define TEXTURE_LOD_RANGE")
                || fragment.contains("#define NV_COMBINER"))
            && {
                let p = (self.gl.GetString)(0x1f02);
                !p.is_null()
                    && CStr::from_ptr(p.cast())
                        .to_string_lossy()
                        .starts_with("OpenGL ES 3.")
            };
        // Multipass combiners project texture coordinates and sample previous
        // framebuffer passes. ES mediump can round those moving coordinates
        // into visible steps. ES3 guarantees fragment highp; retain the ES2
        // compatibility path and ordinary fixed-function shaders unchanged.
        let precise_combiner = es3 && fragment.contains("#define NV_COMBINER");
        let modern_range = es3 && fragment.contains("#define TEXTURE_LOD_RANGE");
        let vertex_source = if modern_range {
            vertex.replace("attribute ", "in ")
        } else {
            vertex.to_owned()
        };
        let vertex_prefix = if modern_range {
            "#version 300 es\nprecision highp float;\nprecision mediump int;\n#define VARYING out mediump\n".to_owned()
        } else {
            prefix.replace("precision mediump float", "precision highp float")
        };
        let vertex_prefix = format!(
            "{vertex_prefix}#define TEX_VARYING {}\n",
            if precise_combiner {
                if modern_range {
                    "out highp"
                } else {
                    "varying highp"
                }
            } else {
                "VARYING"
            }
        );
        let vs = match self.shader(0x8b31, &format!("{}{}", vertex_prefix, vertex_source)) {
            Ok(shader) => shader,
            Err(error) => {
                (self.gl.DeleteProgram)(program);
                return Err(error);
            }
        };
        let fragment_prefix = if self.lod_derivatives {
            if self.host.es {
                prefix.replace("#version 100\n", "#version 100\n#extension GL_OES_standard_derivatives : enable\n#define LOD_DERIVATIVES\n")
            } else {
                format!("{prefix}#define LOD_DERIVATIVES\n")
            }
        } else {
            prefix.to_owned()
        };
        let fragment_prefix = if modern_range {
            "#version 300 es\nprecision mediump float;\nprecision mediump int;\n#define VARYING in mediump\n#define LOD_DERIVATIVES\n#define RANGE_SAMPLE textureLod\n#define texture2D texture\nout vec4 rangeColor;\n#define gl_FragColor rangeColor\n".to_owned()
        } else if fragment.contains("#define TEXTURE_LOD_RANGE") {
            if self.host.es {
                fragment_prefix.replace("#version 100\n","#version 100\n#extension GL_EXT_shader_texture_lod : require\n#define RANGE_SAMPLE texture2DLodEXT\n")
            } else {
                fragment_prefix.replace("#version 120\n","#version 120\n#extension GL_ARB_shader_texture_lod : require\n#define RANGE_SAMPLE texture2DLod\n")
            }
        } else {
            fragment_prefix
        };
        let fragment_prefix = if precise_combiner {
            fragment_prefix.replace("precision mediump float", "precision highp float")
        } else {
            fragment_prefix
        };
        let fragment_prefix = format!(
            "{fragment_prefix}#define TEX_VARYING {}\n",
            if precise_combiner {
                if modern_range {
                    "in highp"
                } else {
                    "varying highp"
                }
            } else {
                "VARYING"
            }
        );
        let fs = match self.shader(0x8b30, &format!("{fragment_prefix}{fragment}")) {
            Ok(s) => s,
            Err(e) => {
                (self.gl.DeleteShader)(vs);
                (self.gl.DeleteProgram)(program);
                return Err(e);
            }
        };
        (self.gl.AttachShader)(program, vs);
        (self.gl.AttachShader)(program, fs);
        for (i, name) in [
            "position",
            "color",
            "tex0",
            "tex1",
            "tex2",
            "tex3",
            "normalAndFog",
            "lightPosition",
            "tex4",
            "tex5",
        ]
        .iter()
        .take(if vertex.contains("#define WIDE_TEXTURES") {
            10
        } else {
            8
        })
        .enumerate()
        {
            (self.gl.BindAttribLocation)(program, i as u32, CString::new(*name).unwrap().as_ptr());
        }
        (self.gl.LinkProgram)(program);
        (self.gl.DeleteShader)(vs);
        (self.gl.DeleteShader)(fs);
        let mut ok = 0;
        (self.gl.GetProgramiv)(program, 0x8b82, &mut ok);
        if ok == 0 {
            let mut msg = vec![0u8; 4096];
            (self.gl.GetProgramInfoLog)(
                program,
                4096,
                std::ptr::null_mut(),
                msg.as_mut_ptr().cast(),
            );
            (self.gl.DeleteProgram)(program);
            return Err(String::from_utf8_lossy(&msg).trim_end_matches('\0').into());
        }
        Ok(program)
    }
    /// No sampled stage means that texture-coordinate transforms and samplers
    /// have no effect. Keep their shader work out of this common draw path.
    fn select_untextured_program(&mut self, enabled: bool) -> Result<(), String> {
        if self.untextured_active == enabled {
            return Ok(());
        }
        if enabled && self.untextured_program.is_none() {
            unsafe {
                let program = self.compile_program(
                    &format!("#define AFFINE_TEXTURES\n#define UNTEXTURED_DRAW\n{VERTEX}"),
                    &format!("#define AFFINE_TEXTURES\n#define UNTEXTURED_DRAW\n{FRAGMENT}"),
                )?;
                let locations = self
                    .uniforms
                    .keys()
                    .map(|n| {
                        (
                            n.clone(),
                            (self.gl.GetUniformLocation)(
                                program,
                                CString::new(n.as_str()).unwrap().as_ptr(),
                            ),
                        )
                    })
                    .collect();
                self.untextured_program = Some((program, locations, HashMap::new()));
            }
        }
        let alternate = self.untextured_program.as_mut().unwrap();
        std::mem::swap(&mut self.program, &mut alternate.0);
        std::mem::swap(&mut self.uniforms, &mut alternate.1);
        std::mem::swap(&mut self.uniform_values, &mut alternate.2);
        self.untextured_active = enabled;
        self.point_drawing = true;
        Ok(())
    }

    /// Both untextured and unit-zero MODULATE draws use this program, so
    /// alternating between them does not cause a program switch.
    fn select_simple_program(&mut self, enabled: bool) -> Result<(), String> {
        if self.simple_active == enabled {
            return Ok(());
        }
        if enabled && self.simple_program.is_none() {
            unsafe {
                let program = self.compile_program(
                    &format!("#define AFFINE_TEXTURES\n{VERTEX}"),
                    SIMPLE_FRAGMENT,
                )?;
                let locations = self
                    .uniforms
                    .keys()
                    .map(|n| {
                        (
                            n.clone(),
                            (self.gl.GetUniformLocation)(
                                program,
                                CString::new(n.as_str()).unwrap().as_ptr(),
                            ),
                        )
                    })
                    .collect();
                self.simple_program = Some((program, locations, HashMap::new()));
            }
        }
        let alternate = self.simple_program.as_mut().unwrap();
        std::mem::swap(&mut self.program, &mut alternate.0);
        std::mem::swap(&mut self.uniforms, &mut alternate.1);
        std::mem::swap(&mut self.uniform_values, &mut alternate.2);
        self.simple_active = enabled;
        self.point_drawing = true;
        Ok(())
    }

    fn select_range_program(&mut self, enabled: bool) -> Result<(), String> {
        if self.range_active == enabled {
            return Ok(());
        }
        if enabled {
            self.select_affine_program(false);
            if self.range_program.is_none() {
                if !self.lod_derivatives {
                    return Err("LOD bounds require fragment derivatives on the host GPU".into());
                }
                unsafe {
                    let program = self.compile_program(
                        VERTEX,
                        &format!("#define TEXTURE_LOD_RANGE\n{FRAGMENT}"),
                    )?;
                    let locations = self
                        .uniforms
                        .keys()
                        .map(|n| {
                            (
                                n.clone(),
                                (self.gl.GetUniformLocation)(
                                    program,
                                    CString::new(n.as_str()).unwrap().as_ptr(),
                                ),
                            )
                        })
                        .collect();
                    self.range_program = Some((program, locations, HashMap::new()));
                }
            }
        }
        let range = self.range_program.as_mut().unwrap();
        std::mem::swap(&mut self.program, &mut range.0);
        std::mem::swap(&mut self.uniforms, &mut range.1);
        std::mem::swap(&mut self.uniform_values, &mut range.2);
        self.range_active = enabled;
        self.point_drawing = true;
        Ok(())
    }

    fn select_affine_program(&mut self, affine: bool) {
        if self.affine_program == affine {
            return;
        }
        let alternate = self.alternate_program.as_mut().unwrap();
        std::mem::swap(&mut self.program, &mut alternate.0);
        std::mem::swap(&mut self.uniforms, &mut alternate.1);
        std::mem::swap(&mut self.uniform_values, &mut alternate.2);
        self.affine_program = affine;
        // The other program may still hold point-sprite/array uniforms from
        // an earlier draw. Refresh them even if the last draw was a triangle.
        self.point_drawing = true;
    }
    unsafe fn shader(&self, ty: u32, source: &str) -> Result<u32, String> {
        let s = (self.gl.CreateShader)(ty);
        if s == 0 {
            return Err("shader allocation failed".into());
        }
        let source = CString::new(source).unwrap();
        (self.gl.ShaderSource)(s, 1, &source.as_ptr(), std::ptr::null());
        (self.gl.CompileShader)(s);
        let mut ok = 0;
        (self.gl.GetShaderiv)(s, 0x8b81, &mut ok);
        if ok == 0 {
            let mut msg = vec![0u8; 4096];
            (self.gl.GetShaderInfoLog)(s, 4096, std::ptr::null_mut(), msg.as_mut_ptr().cast());
            (self.gl.DeleteShader)(s);
            Err(String::from_utf8_lossy(&msg).trim_end_matches('\0').into())
        } else {
            Ok(s)
        }
    }
    unsafe fn cache_uniform(&mut self, n: &str) {
        let l = (self.gl.GetUniformLocation)(self.program, CString::new(n).unwrap().as_ptr());
        self.uniforms.insert(n.into(), l);
    }
    fn changed_uniform(&mut self, location: i32, value: &[u32]) -> bool {
        if location < 0 {
            return false;
        }
        if self
            .uniform_values
            .get(&location)
            .is_some_and(|old| old == value)
        {
            return false;
        }
        let cached = self.uniform_values.entry(location).or_default();
        cached.clear();
        cached.extend_from_slice(value);
        true
    }
    unsafe fn uniform_1i(&mut self, l: i32, v: i32) {
        if self.changed_uniform(l, &[v as u32]) {
            (self.gl.Uniform1i)(l, v);
        }
    }
    unsafe fn uniform_1f(&mut self, l: i32, v: f32) {
        if self.changed_uniform(l, &[v.to_bits()]) {
            (self.gl.Uniform1f)(l, v);
        }
    }
    unsafe fn uniform_2f(&mut self, l: i32, a: f32, b: f32) {
        if self.changed_uniform(l, &[a.to_bits(), b.to_bits()]) {
            (self.gl.Uniform2f)(l, a, b);
        }
    }
    unsafe fn uniform_4fv(&mut self, l: i32, n: i32, p: *const f32) {
        // An inactive uniform needs neither a driver call nor payload/cache work.
        if l < 0 {
            return;
        }
        let mut bits = [0u32; 32];
        let len = n as usize * 4;
        for (dst, v) in bits[..len]
            .iter_mut()
            .zip(std::slice::from_raw_parts(p, len))
        {
            *dst = v.to_bits();
        }
        if self.changed_uniform(l, &bits[..len]) {
            (self.gl.Uniform4fv)(l, n, p);
        }
    }
    unsafe fn uniform_matrix4fv(&mut self, l: i32, n: i32, transpose: u8, p: *const f32) {
        if l < 0 {
            return;
        }
        let mut bits = [0u32; 16];
        for (dst, v) in bits.iter_mut().zip(std::slice::from_raw_parts(p, 16)) {
            *dst = v.to_bits();
        }
        if self.changed_uniform(l, &bits) {
            (self.gl.UniformMatrix4fv)(l, n, transpose, p);
        }
    }
    fn loc(&self, n: &str) -> i32 {
        self.uniforms[n]
    }
    unsafe fn check(&self) -> Result<(), String> {
        check_native_error(&self.gl)
    }
    unsafe fn toggle(&self, cap: u32, on: bool) {
        if on {
            (self.gl.Enable)(cap)
        } else {
            (self.gl.Disable)(cap)
        }
    }
    unsafe fn scissor(&self, s: Option<(i32, i32, i32, i32)>) {
        self.toggle(GL_SCISSOR_TEST, s.is_some());
        if let Some((x, y, w, h)) = s {
            let k = self.render_scale as i32;
            (self.gl.Scissor)(
                x.saturating_mul(k),
                y.saturating_mul(k),
                w.saturating_mul(k),
                h.saturating_mul(k),
            );
        }
    }
    unsafe fn params(&self, min: Filter, mag: Filter, s: Wrap, t: Wrap) {
        for (p, v) in [
            (GL_TEXTURE_MIN_FILTER, filter(min)),
            (GL_TEXTURE_MAG_FILTER, filter(mag)),
            (GL_TEXTURE_WRAP_S, wrap(s)),
            (GL_TEXTURE_WRAP_T, wrap(t)),
        ] {
            (self.gl.TexParameteri)(GL_TEXTURE_2D, p, v as i32);
        }
    }
    /// Select at surface creation, before any rendering has been issued.
    pub fn set_surface_rgb565(&mut self, rgb565: bool) -> Result<(), String> {
        if self.rgb565 == rgb565 {
            return Ok(());
        }
        self.rgb565 = rgb565;
        let (w, h) = (
            self.width / self.render_scale,
            self.height / self.render_scale,
        );
        self.width = 0;
        self.height = 0;
        self.resize(w, h)?;
        if rgb565 {
            unsafe {
                let mut bits = [0; 4];
                for (i, pname) in [0x0d52, 0x0d53, 0x0d54, 0x0d55].into_iter().enumerate() {
                    (self.gl.GetIntegerv)(pname, &mut bits[i]);
                }
                self.check()?;
                if bits != [5, 6, 5, 0] {
                    return Err(format!("host cannot provide an RGB565 surface: {bits:?}"));
                }
            }
        }
        Ok(())
    }

    /// Logical point limits must fit the native target even at internal 2x.
    pub fn logical_point_range(&self) -> [f32; 2] {
        [
            1.,
            (self.point_range[1] / self.render_scale as f32)
                .floor()
                .max(1.),
        ]
    }

    /// Policy 1=native, 2=double all, 3=double QVGA only, 4=triple QVGA only, 5=quadruple QVGA only. Guest sizes stay logical.
    pub fn set_render_scale(&mut self, policy: u8) -> Result<(), String> {
        self.render_policy = policy;
        let (w, h) = (
            self.width / self.render_scale,
            self.height / self.render_scale,
        );
        let scale = render_scale_for(policy, w, h);
        if scale == self.render_scale {
            return Ok(());
        }
        self.render_scale = scale;
        self.resize(w, h)
    }

    pub fn resize(&mut self, logical_w: u32, logical_h: u32) -> Result<(), String> {
        let w = logical_w
            .checked_mul(self.render_scale)
            .ok_or("GPU width overflow")?;
        let h = logical_h
            .checked_mul(self.render_scale)
            .ok_or("GPU height overflow")?;
        // Keep the existing no-resize fast path; policy selection is only done
        // when surface dimensions or the configured policy actually change.
        if (w, h) == (self.width, self.height) {
            return Ok(());
        }
        let scale = render_scale_for(self.render_policy, logical_w, logical_h);
        let w = logical_w.checked_mul(scale).ok_or("GPU width overflow")?;
        let h = logical_h.checked_mul(scale).ok_or("GPU height overflow")?;
        if w == 0 || h == 0 || w > 8192 || h > 8192 {
            return Err("unsupported GPU target size".into());
        }
        self.host.make_current()?;
        unsafe {
            (self.gl.ActiveTexture)(GL_TEXTURE0);
            (self.gl.BindTexture)(GL_TEXTURE_2D, self.color);
            (self.gl.TexImage2D)(
                GL_TEXTURE_2D,
                0,
                if self.rgb565 {
                    if self.host.es {
                        GL_RGB as i32
                    } else {
                        0x8d62
                    }
                } else {
                    GL_RGBA as i32
                },
                w as i32,
                h as i32,
                0,
                if self.rgb565 { GL_RGB } else { GL_RGBA },
                if self.rgb565 {
                    GL_UNSIGNED_SHORT_5_6_5
                } else {
                    GL_UNSIGNED_BYTE
                },
                std::ptr::null(),
            );
            self.params(
                Filter::Nearest,
                Filter::Nearest,
                Wrap::ClampToEdge,
                Wrap::ClampToEdge,
            );
            self.check()?;
            (self.gl.BindRenderbuffer)(0x8d41, self.depth);
            (self.gl.BindFramebuffer)(0x8d40, self.fbo);
            (self.gl.FramebufferTexture2D)(0x8d40, 0x8ce0, GL_TEXTURE_2D, self.color, 0);
            (self.gl.FramebufferRenderbuffer)(0x8d40, 0x8d00, 0x8d41, self.depth);
            loop {
                (self.gl.RenderbufferStorage)(0x8d41, self.depth_format, w as i32, h as i32);
                let error = upload_error(&self.gl)?;
                let complete = (self.gl.CheckFramebufferStatus)(0x8d40) == 0x8cd5;
                if error == 0 && complete {
                    break;
                }
                if self.depth_format == 0x81a6 {
                    // Keep ES2 drivers lacking a usable 24-bit attachment usable.
                    self.depth_format = 0x81a5;
                } else {
                    return Err(format!("GPU depth attachment failed: 0x{error:04x}"));
                }
            }
            self.pipeline = None;
            self.scissor(None);
            (self.gl.ColorMask)(1, 1, 1, 1);
            (self.gl.DepthMask)(1);
            (self.gl.ClearColor)(0., 0., 0., 0.);
            (self.gl.Clear)(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);
            self.render_scale = scale;
            self.width = w;
            self.height = h;
            self.dirty = true;
            self.check()
        }
    }
    /// Update native pixmap color without touching its depth attachment.
    /// Source is top-down RGB565; the backing texture is bottom-up and may
    /// have an internal scale. This path is used only for native/GL interop.
    pub fn write_rgb565(&mut self, width: u32, height: u32, pixels: &[u8]) -> Result<(), String> {
        if !self.rgb565
            || width == 0
            || height == 0
            || pixels.len() != width as usize * height as usize * 2
            || self.width != width * self.render_scale
            || self.height != height * self.render_scale
        {
            return Err("incompatible native pixmap upload".into());
        }
        self.host.make_current()?;
        if !self.upload_transfer_failed {
            let uploaded = (|| unsafe {
                if self.upload_source == 0 {
                    (self.gl.GenTextures)(1, &mut self.upload_source);
                }
                (self.gl.ActiveTexture)(GL_TEXTURE0);
                (self.gl.BindTexture)(GL_TEXTURE_2D, self.upload_source);
                self.params(
                    Filter::Nearest,
                    Filter::Nearest,
                    Wrap::ClampToEdge,
                    Wrap::ClampToEdge,
                );
                (self.gl.PixelStorei)(0x0cf5, 1);
                (self.gl.TexImage2D)(
                    GL_TEXTURE_2D,
                    0,
                    if self.host.es { GL_RGB as i32 } else { 0x8d62 },
                    width as i32,
                    height as i32,
                    0,
                    GL_RGB,
                    GL_UNSIGNED_SHORT_5_6_5,
                    pixels.as_ptr().cast(),
                );
                self.check()?;
                self.transfer_pixels(
                    self.upload_source,
                    self.fbo,
                    self.width,
                    self.height,
                    1,
                    true,
                    transfer::Quantization::Preserve,
                    (width, height),
                )
            })();
            if uploaded.is_ok() {
                self.dirty = true;
                return Ok(());
            }
            self.upload_transfer_failed = true;
        }
        let mut upload = vec![0u8; self.width as usize * self.height as usize * 2];
        for y in 0..self.height as usize {
            let sy = height as usize - 1 - y / self.render_scale as usize;
            for x in 0..self.width as usize {
                let src = (sy * width as usize + x / self.render_scale as usize) * 2;
                let dst = (y * self.width as usize + x) * 2;
                upload[dst..dst + 2].copy_from_slice(&pixels[src..src + 2]);
            }
        }
        self.host.make_current()?;
        unsafe {
            (self.gl.ActiveTexture)(GL_TEXTURE0);
            (self.gl.BindTexture)(GL_TEXTURE_2D, self.color);
            (self.gl.PixelStorei)(0x0cf5, 1);
            (self.gl.TexImage2D)(
                GL_TEXTURE_2D,
                0,
                if self.host.es { GL_RGB as i32 } else { 0x8d62 },
                self.width as i32,
                self.height as i32,
                0,
                GL_RGB,
                GL_UNSIGNED_SHORT_5_6_5,
                upload.as_ptr().cast(),
            );
        }
        self.dirty = true;
        unsafe { self.check() }
    }

    pub fn clear(
        &mut self,
        w: u32,
        h: u32,
        state: &PipelineState,
        color: [f32; 4],
        depth: f32,
        mask: u32,
    ) -> Result<(), String> {
        self.resize(w, h)?;
        self.host.make_current()?;
        self.pipeline = None;
        unsafe {
            (self.gl.BindFramebuffer)(0x8d40, self.fbo);
            self.scissor(state.scissor);
            (self.gl.ColorMask)(
                state.color_mask[0] as u8,
                state.color_mask[1] as u8,
                state.color_mask[2] as u8,
                state.color_mask[3] as u8,
            );
            // Clear obeys the depth write mask, independently of DEPTH_TEST.
            (self.gl.DepthMask)(state.depth_write as u8);
            (self.gl.ClearColor)(color[0], color[1], color[2], color[3]);
            let p = self.host.load(if self.host.es {
                c"glClearDepthf"
            } else {
                c"glClearDepth"
            });
            if p.is_null() {
                return Err("driver lacks depth clear".into());
            }
            if self.host.es {
                let f: unsafe extern "system" fn(f32) = std::mem::transmute(p);
                f(depth);
            } else {
                let f: unsafe extern "system" fn(f64) = std::mem::transmute(p);
                f(depth as f64);
            }
            (self.gl.Clear)(mask & (GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT));
            (self.gl.DepthMask)(state.depth_write as u8);
            self.dirty = true;
            self.check()
        }
    }
    /// Begin the next window frame with fresh ancillary storage. This is an
    /// EGL surface operation, not a guest glClear: guest masks do not apply.
    pub fn discard_window_depth(
        &mut self,
        w: u32,
        h: u32,
        state: &PipelineState,
    ) -> Result<(), String> {
        let mut reset = state.clone();
        reset.scissor = None;
        reset.depth_write = true;
        let color_dirty = self.dirty;
        self.clear(w, h, &reset, [0.; 4], 1., GL_DEPTH_BUFFER_BIT)?;
        // Only depth changed: keep both presentation color caches valid.
        self.dirty = color_dirty;
        Ok(())
    }

    pub fn supports_compressed(&self, format: u32) -> bool {
        let format = native_compressed_format(format);
        self.gl.CompressedTexImage2D.is_some() && self.compressed_formats.contains(&format)
    }
    /// EGL cache maintenance, independent of guest glDeleteTextures semantics.
    /// Shared named objects retain identity/revision across contexts; texture
    /// zero is context-local and must never survive a surface context switch.
    pub fn retain_shared_texture_cache(&mut self, live: &std::collections::HashSet<u32>) {
        let stale: Vec<_> = self
            .textures
            .keys()
            .copied()
            .filter(|name| *name == 0 || !live.contains(name))
            .collect();
        self.invalidate_texture_cache_entries(&stale);
    }
    pub fn invalidate_texture_cache(&mut self) {
        let names: Vec<_> = self.textures.keys().copied().collect();
        self.invalidate_texture_cache_entries(&names);
    }
    fn invalidate_texture_cache_entries(&mut self, names: &[u32]) {
        if names.is_empty() || self.host.make_current().is_err() {
            return;
        }
        for name in names {
            if let Some((id, _)) = self.textures.remove(name) {
                self.texture_params.remove(&id);
                unsafe {
                    (self.gl.DeleteTextures)(1, &id);
                }
            }
        }
    }
    pub fn delete_textures(&mut self, names: &[u32]) {
        if self.host.make_current().is_err() {
            return;
        }
        for n in names {
            // Guest name zero is the persistent default texture, not native GL zero.
            if *n == 0 {
                continue;
            }
            if let Some((id, _)) = self.textures.remove(n) {
                self.texture_params.remove(&id);
                unsafe {
                    (self.gl.DeleteTextures)(1, &id);
                }
            }
        }
    }
    pub fn draw(
        &mut self,
        target: &RenderTarget,
        state: &PipelineState,
        mode: u32,
        vertices: &[Vertex],
        stages: &[TextureStage],
        textures: &HashMap<u32, Texture>,
        names: [u32; 6],
    ) -> Result<(), String> {
        self.draw_inner(
            target, state, mode, vertices, stages, textures, names, None, false, None, None,
        )
    }

    pub fn draw_object_space(
        &mut self,
        target: &RenderTarget,
        state: &PipelineState,
        mode: u32,
        vertices: &[Vertex],
        stages: &[TextureStage],
        textures: &HashMap<u32, Texture>,
        names: [u32; 6],
        transforms: &VertexTransforms,
    ) -> Result<(), String> {
        self.draw_inner(
            target,
            state,
            mode,
            vertices,
            stages,
            textures,
            names,
            Some(transforms),
            false,
            None,
            None,
        )
    }

    pub fn draw_arrays_object_space(
        &mut self,
        target: &RenderTarget,
        state: &PipelineState,
        mode: u32,
        vertices: &[Vertex],
        stages: &[TextureStage],
        textures: &HashMap<u32, Texture>,
        names: [u32; 6],
        transforms: &VertexTransforms,
    ) -> Result<(), String> {
        self.draw_inner(
            target,
            state,
            mode,
            vertices,
            stages,
            textures,
            names,
            Some(transforms),
            true,
            None,
            None,
        )
    }

    /// Guest indices are already remapped to unique, validated vertices by Context.
    pub fn draw_indexed_object_space(
        &mut self,
        target: &RenderTarget,
        state: &PipelineState,
        mode: u32,
        vertices: &[Vertex],
        indices: &[u16],
        stages: &[TextureStage],
        textures: &HashMap<u32, Texture>,
        names: [u32; 6],
        transforms: &VertexTransforms,
    ) -> Result<(), String> {
        if indices.iter().any(|i| *i as usize >= vertices.len()) {
            return Err("invalid native index".into());
        }
        self.draw_inner(
            target,
            state,
            mode,
            vertices,
            stages,
            textures,
            names,
            Some(transforms),
            true,
            Some(indices),
            None,
        )
    }

    pub(crate) fn supports_fixed_vertices(&self) -> bool {
        self.native_fixed
    }

    pub(crate) fn draw_stream_object_space(
        &mut self,
        target: &RenderTarget,
        state: &PipelineState,
        mode: u32,
        stream: &VertexStream,
        stages: &[TextureStage],
        textures: &HashMap<u32, Texture>,
        names: [u32; 6],
        transforms: &VertexTransforms,
    ) -> Result<(), String> {
        if (state.smooth_shading && stream.flat_triangles)
            || (!state.smooth_shading
                && (!stream.flat_triangles
                    || mode != GL_TRIANGLES
                    || !stream.indices.is_empty()
                    || stream.count % 3 != 0))
            || stream.count == 0
            || stream.count > 65536
            || stream.indices.len() > i32::MAX as usize
            || stream.indices.iter().any(|i| *i as usize >= stream.count)
        {
            return Err("invalid vertex stream".into());
        }
        for attr in stream.attributes.iter().flatten() {
            let width = match attr.ty {
                GL_BYTE | GL_UNSIGNED_BYTE => 1,
                GL_SHORT | GL_UNSIGNED_SHORT => 2,
                GL_FLOAT => 4,
                GL_FIXED if self.native_fixed => 4,
                _ => return Err("unsupported stream format".into()),
            };
            if !(1..=4).contains(&attr.size)
                || attr.stride <= 0
                || attr.normalized > 1
                || attr.offset % width != 0
                || attr.stride as usize % width != 0
            {
                return Err("invalid stream attribute".into());
            }
            let end = (stream.count - 1)
                .checked_mul(attr.stride as usize)
                .and_then(|n| n.checked_add(attr.offset))
                .and_then(|n| n.checked_add(attr.size as usize * width));
            if end.is_none_or(|n| n > stream.bytes.len()) {
                return Err("stream attribute outside host snapshot".into());
            }
        }
        if stream.attributes[0].is_none() {
            return Err("missing stream positions".into());
        }
        self.draw_inner(
            target,
            state,
            mode,
            &[],
            stages,
            textures,
            names,
            Some(transforms),
            true,
            None,
            Some(stream),
        )
    }

    pub(crate) fn texture_coordinate_capacity(&self) -> usize {
        if self.wide_textures {
            6
        } else {
            4
        }
    }

    pub(crate) fn supports_combiner(&self) -> bool {
        self.combiner_available
    }

    pub(crate) fn set_combiner(
        &mut self,
        program: Option<&std::sync::Arc<crate::combiner_program::BasicProgram>>,
        constants: [[f32; 4]; 4],
    ) {
        self.combiner = program.map(|program| (program.clone(), constants));
    }

    fn take_combiner_program(
        &mut self,
        program: &crate::combiner_program::BasicProgram,
        range: bool,
    ) -> Result<ProgramState, String> {
        if let Some(cached) = self
            .combiner_programs
            .remove(&(SpecializedShader::Combiner(program.id()), range))
        {
            return Ok(cached);
        }
        let wide = program.coordinate_units() & 0x30 != 0 || program.texture_units() & 0x30 != 0;
        if wide && !self.wide_textures {
            return Err("six texture coordinates exceed host shader limits".into());
        }
        let wide_define = if wide { "#define WIDE_TEXTURES\n" } else { "" };
        let vertex = format!("{wide_define}{VERTEX}");
        let fragment = FRAGMENT.replace("/* NV_COMBINER_FUNCTION */", &program.fragment_function());
        let range_define = if range {
            "#define TEXTURE_LOD_RANGE\n"
        } else {
            ""
        };
        unsafe {
            let id = self.compile_program(
                &vertex,
                &format!("#define NV_COMBINER\n{wide_define}{range_define}{fragment}"),
            )?;
            let locations = self
                .uniforms
                .keys()
                .map(String::as_str)
                .chain(NV_CONSTANTS)
                .map(|name| {
                    (
                        name.to_owned(),
                        (self.gl.GetUniformLocation)(id, CString::new(name).unwrap().as_ptr()),
                    )
                })
                .collect();
            Ok((id, locations, HashMap::new()))
        }
    }

    fn take_wide_fixed_program(&mut self, range: bool) -> Result<ProgramState, String> {
        if let Some(program) = self
            .combiner_programs
            .remove(&(SpecializedShader::WideFixed, range))
        {
            return Ok(program);
        }
        if !self.wide_textures {
            return Err("six texture coordinates exceed host shader limits".into());
        }
        let range_define = if range {
            "#define TEXTURE_LOD_RANGE\n"
        } else {
            ""
        };
        unsafe {
            let id = self.compile_program(
                &format!("#define WIDE_TEXTURES\n{VERTEX}"),
                &format!("#define WIDE_TEXTURES\n{range_define}{FRAGMENT}"),
            )?;
            let locations = self
                .uniforms
                .keys()
                .map(|name| {
                    (
                        name.clone(),
                        (self.gl.GetUniformLocation)(
                            id,
                            CString::new(name.as_str()).unwrap().as_ptr(),
                        ),
                    )
                })
                .collect();
            Ok((id, locations, HashMap::new()))
        }
    }

    fn swap_program_state(&mut self, other: &mut ProgramState) {
        std::mem::swap(&mut self.program, &mut other.0);
        std::mem::swap(&mut self.uniforms, &mut other.1);
        std::mem::swap(&mut self.uniform_values, &mut other.2);
        self.point_drawing = true;
    }

    unsafe fn copy_destination(&mut self, reuse: bool) {
        let format = (self.width, self.height, self.rgb565);
        if reuse && self.destination_format == Some(format) {
            if let Some(copy) = self.gl.CopyTexSubImage2D {
                copy(
                    GL_TEXTURE_2D,
                    0,
                    0,
                    0,
                    0,
                    0,
                    self.width as i32,
                    self.height as i32,
                );
                return;
            }
        }
        (self.gl.CopyTexImage2D)(
            GL_TEXTURE_2D,
            0,
            if self.rgb565 { GL_RGB } else { GL_RGBA },
            0,
            0,
            self.width as i32,
            self.height as i32,
            0,
        );
        self.destination_format = Some(format);
    }

    fn draw_inner(
        &mut self,
        target: &RenderTarget,
        state: &PipelineState,
        mode: u32,
        vertices: &[Vertex],
        stages: &[TextureStage],
        textures: &HashMap<u32, Texture>,
        names: [u32; 6],
        transforms: Option<&VertexTransforms>,
        sequential: bool,
        guest_indices: Option<&[u16]>,
        stream: Option<&VertexStream>,
    ) -> Result<(), String> {
        let _diagnostic =
            crate::graphics_diagnostic::Span::new(crate::graphics_diagnostic::Stage::Submit);
        // A sampled high unit still needs its white fallback when its image
        // is incomplete and therefore absent from the enabled stage list.
        let wide_draw = stages.iter().any(|s| s.unit >= 4)
            || self
                .combiner
                .as_ref()
                .is_some_and(|(p, _)| (p.texture_units() | p.coordinate_units()) & 0x30 != 0);
        if wide_draw && !self.wide_textures {
            return Err("six texture coordinates exceed host shader limits".into());
        }
        let attribute_limit = if wide_draw || self.last_wide_draw {
            10
        } else {
            8
        };
        if wide_draw != self.last_wide_draw {
            // Clear locations 8/9 once when returning to the compact path.
            self.attribute_layout = None;
            self.stream_layout = None;
        }
        self.last_wide_draw = wide_draw;
        self.resize(target.width, target.height)?;
        self.host.make_current()?;
        let (offset_factor, mut offset_units) = state.polygon_offset_parameters();
        let compatibility_pass = (self.combiner.is_none()
            && matches!(mode, GL_TRIANGLES | GL_TRIANGLE_STRIP | GL_TRIANGLE_FAN))
        .then(|| state.intel2700g_depth_color_pass())
        .flatten();
        let mut replay_offset_units = compatibility_pass
            .as_ref()
            .filter(|pass| pass.polygon_offset)
            .map_or(0., |pass| pass.polygon_offset_parameters().1);
        if state.intel2700g_depth_bias
            && ((state.polygon_offset && offset_units != 0.) || replay_offset_units != 0.)
        {
            let step = match self.depth_bias_step {
                Some((format, step)) if format == self.depth_format => step,
                _ => {
                    let step = unsafe { depth_bias::measure(self)? };
                    self.depth_bias_step = Some((self.depth_format, step));
                    step
                }
            };
            offset_units /= state.polygon_offset_resolution() * step;
            replay_offset_units /= state.polygon_offset_resolution() * step;
        }
        let _gpu_diagnostic = diagnostic::Timer::scope(&self.diagnostic);
        self.scratch.clear();
        // Packed guest streams already retain their native component count.
        // Only expanded vertices carrying nondefault r/q need a larger payload.
        let mask = if stream.is_none() {
            stages.iter().fold(0, |mask, stage| {
                if vertices
                    .iter()
                    .any(|v| v.texcoord[stage.unit][2] != 0. || v.texcoord[stage.unit][3] != 1.)
                {
                    mask | (1 << stage.unit)
                } else {
                    mask
                }
            })
        } else {
            0
        };
        let vertex_count = stream.map_or(vertices.len(), |s| s.count);
        let sampled_units = self.combiner.as_ref().map(|(p, _)| p.texture_units());
        let range = stages.iter().any(|stage| {
            if sampled_units.is_some_and(|mask| mask & (1 << stage.unit) == 0) {
                return false;
            }
            textures.get(&names[stage.unit]).is_some_and(|tex| {
                let (lo, hi) = tex.lod_bounds();
                let (w, h) = tex.sampling_size();
                (lo > 0. || hi < w.max(h).checked_ilog2().unwrap_or(0) as f32) && tex.is_complete()
            })
        });
        let combiner = self.combiner.clone();
        let framebuffer_fetch = combiner
            .as_ref()
            .is_some_and(|(program, _)| program.reads_framebuffer());
        let mut feedback_writes = state.color_mask;
        if self.rgb565 {
            feedback_writes[3] = false;
        }
        let ordered = state.logic_op
            || combiner.as_ref().is_some_and(|(program, _)| {
                program.framebuffer_needs_primitive_order(feedback_writes)
            });
        let affine = combiner.is_none()
            && !range
            && mode != GL_POINTS
            && !state.logic_op
            && affine_texture_coordinates(vertices, stages, transforms, stream);
        let simple_eligible = self.simple_available
            && replay_offset_units == 0.
            && affine
            && target.rgb565
            && !state.fog
            && !state.alpha_test
            && !state.logic_op
            && !state.two_sided_lighting
            && !transforms.is_some_and(|t| t.lighting.enabled && t.lighting.two_sided)
            && (stages.is_empty()
                || (stages.len() == 1
                    && stages[0].unit == 0
                    && matches!(stages[0].env, TexEnvMode::Modulate)
                    && stages[0].lod_bias == 0.
                    && textures.get(&names[0]).is_some_and(|t| {
                        t.is_complete()
                            && !t.min_filter.uses_mipmaps()
                            && t.min_lod == -1000.
                            && t.max_lod == 1000.
                    })));
        // Do not oscillate for isolated simple draws between complex states.
        // Texture ON/OFF is deliberately not a change of shader family here.
        self.simple_run = if simple_eligible {
            self.simple_run.saturating_add(1)
        } else {
            0
        };
        let simple = simple_eligible && (self.simple_active || self.simple_run >= 8);
        if self.simple_active && !simple {
            self.select_simple_program(false)?;
        }
        let untextured = combiner.is_none()
            && replay_offset_units == 0.
            && !simple
            && stages.is_empty()
            && target.rgb565
            && mode != GL_POINTS
            && !state.logic_op
            && !state.alpha_test
            && !state.blend
            && (vertex_count >= 128 || self.untextured_active);
        if self.untextured_active && !untextured {
            self.select_untextured_program(false)?;
        }
        self.select_range_program(range)?;
        if !range {
            self.select_affine_program(affine);
        }
        if simple && !self.simple_active {
            self.select_simple_program(true)?;
        }
        if untextured && !self.untextured_active {
            self.select_untextured_program(true)?;
        }
        let point_sizes = mode == GL_POINTS && state.point_size_array && transforms.is_some();
        let layout = if mask == 0 {
            VertexLayout::new(state, stages, transforms)
        } else {
            VertexLayout::with_projective_mask(state, stages, transforms, mask)
        };
        let layout = if point_sizes {
            layout.with_point_sizes()
        } else {
            layout
        };
        let flat_direct =
            !ordered && direct_flat_triangles(mode, state.smooth_shading, guest_indices.is_some());
        let direct = flat_direct
            || (!ordered
                && sequential
                && state.smooth_shading
                && matches!(
                    mode,
                    GL_TRIANGLES | GL_TRIANGLE_STRIP | GL_TRIANGLE_FAN | GL_POINTS | GL_LINES
                ));
        let draw_mode = if direct {
            mode
        } else {
            match mode {
                GL_POINTS => GL_POINTS,
                GL_LINES | GL_LINE_STRIP | GL_LINE_LOOP => GL_LINES,
                _ => GL_TRIANGLES,
            }
        };
        let mut indexed = stream.is_some_and(|s| !s.indices.is_empty());
        if stream.is_none() {
            let mut primitive = |v: &[Vertex]| {
                let provoking = *v.last().unwrap();
                for &vertex in v {
                    layout.pack(&mut self.scratch, vertex, provoking, state.smooth_shading);
                }
            };
            if flat_direct {
                pack_flat_triangles(&layout, &mut self.scratch, vertices);
            } else if direct {
                for vertex in vertices {
                    primitive(&[*vertex]);
                }
            } else {
                match mode {
                    GL_TRIANGLES => {
                        for v in vertices.chunks_exact(3) {
                            primitive(&[v[0], v[1], v[2]]);
                        }
                    }
                    GL_TRIANGLE_FAN => {
                        for i in 2..vertices.len() {
                            primitive(&[vertices[0], vertices[i - 1], vertices[i]]);
                        }
                    }
                    GL_TRIANGLE_STRIP => {
                        for i in 2..vertices.len() {
                            primitive(&if i % 2 == 0 {
                                [vertices[i - 2], vertices[i - 1], vertices[i]]
                            } else {
                                [vertices[i - 1], vertices[i - 2], vertices[i]]
                            });
                        }
                    }
                    GL_POINTS => {
                        for v in vertices {
                            primitive(&[*v]);
                        }
                    }
                    GL_LINES => {
                        for v in vertices.chunks_exact(2) {
                            primitive(v);
                        }
                    }
                    GL_LINE_STRIP | GL_LINE_LOOP => {
                        for v in vertices.windows(2) {
                            primitive(v);
                        }
                        if mode == GL_LINE_LOOP && vertices.len() > 1 {
                            primitive(&[*vertices.last().unwrap(), vertices[0]]);
                        }
                    }
                    _ => return Err("unsupported GPU primitive".into()),
                }
            }
            self.indices.clear();
            let mut packed = std::mem::take(&mut self.packed_scratch);
            packed.clear();
            indexed = !direct;
            if let Some(indices) = guest_indices {
                self.indices.extend_from_slice(indices);
                indexed = true;
            }
            if !direct {
                // Preserve the existing small-key path for ordinary draws.
                // Only a wide packed vertex needs the larger key/cache.
                indexed = if layout.floats <= 32 {
                    unique_records(
                        &self.scratch,
                        layout.floats,
                        &mut self.vertex_keys,
                        &mut packed,
                        &mut self.indices,
                    )
                } else {
                    unique_records(
                        &self.scratch,
                        layout.floats,
                        &mut self.wide_vertex_keys,
                        &mut packed,
                        &mut self.indices,
                    )
                };
            }
            // Do not increase transfer size for draws with little vertex reuse.
            if guest_indices.is_none() {
                indexed &= packed.len() * 4 + self.indices.len() * 2 < self.scratch.len() * 4;
            }
            if indexed && guest_indices.is_none() {
                std::mem::swap(&mut self.scratch, &mut packed);
            }
            self.packed_scratch = packed;
        }
        let combiner_state = if let Some((program, _)) = &combiner {
            let mut shader = self.take_combiner_program(program, range)?;
            self.swap_program_state(&mut shader);
            Some((SpecializedShader::Combiner(program.id()), shader))
        } else if stages.iter().any(|stage| stage.unit >= 4) {
            let mut shader = self.take_wide_fixed_program(range)?;
            self.swap_program_state(&mut shader);
            Some((SpecializedShader::WideFixed, shader))
        } else {
            None
        };
        let result = (|| unsafe {
            (self.gl.BindFramebuffer)(0x8d40, self.fbo);
            (self.gl.UseProgram)(self.program);
            if let Some((_, constants)) = &combiner {
                // The compiler may retain only the first few elements.
                for (name, value) in NV_CONSTANTS.into_iter().zip(constants) {
                    self.uniform_4fv(self.loc(name), 1, value.as_ptr());
                }
            }
            let sprite = mode == GL_POINTS && state.point_sprite;
            if mode == GL_POINTS || self.point_drawing {
                if !self.host.es && self.point_sprite_mode != sprite {
                    self.toggle(0x8861, sprite);
                    self.point_sprite_mode = sprite;
                }
                self.uniform_1i(self.loc("pointArray"), i32::from(point_sizes));
                let point_params = [
                    self.render_scale as f32,
                    self.point_range[0],
                    self.logical_point_range()[1] * self.render_scale as f32,
                    if sprite { 1. } else { 0. },
                ];
                self.uniform_4fv(self.loc("pointArrayParams"), 1, point_params.as_ptr());
                let replacements =
                    state
                        .point_coord_replace
                        .map(|enabled| if sprite && enabled { 1. } else { 0. });
                self.uniform_4fv(self.loc("pointReplace"), 1, replacements.as_ptr());
                let high = [replacements[4], replacements[5], 0., 0.];
                self.uniform_4fv(self.loc("pointReplaceHigh"), 1, high.as_ptr());
            }
            self.point_drawing = mode == GL_POINTS;
            self.uniform_1f(
                self.loc("pointSize"),
                ((if sprite {
                    state.point_size.round()
                } else {
                    state.point_size
                }) * self.render_scale as f32)
                    .clamp(
                        self.point_range[0],
                        if mode == GL_POINTS {
                            self.logical_point_range()[1] * self.render_scale as f32
                        } else {
                            self.point_range[1]
                        },
                    ),
            );
            if self.pipeline.as_ref() != Some(state) {
                (self.gl.LineWidth)(
                    (state.line_width * self.render_scale as f32)
                        .clamp(self.line_range[0], self.line_range[1]),
                );
                if !self.host.es {
                    self.toggle(0x8642, true);
                }
                let (x, y, w, h) = state.viewport;
                let k = self.render_scale as i32;
                (self.gl.Viewport)(
                    x.saturating_mul(k),
                    y.saturating_mul(k),
                    w.saturating_mul(k),
                    h.saturating_mul(k),
                );
                self.scissor(state.scissor);
                self.toggle(GL_DEPTH_TEST, state.depth_test && !target.depth.is_empty());
                (self.gl.DepthFunc)(compare(state.depth_func));
                (self.gl.DepthMask)(state.depth_write as u8);
                self.toggle(GL_BLEND, state.blend && !state.logic_op);
                (self.gl.BlendFunc)(blend(state.blend_src), blend(state.blend_dst));
                self.toggle(GL_CULL_FACE, state.cull.is_some());
                if let Some(c) = state.cull {
                    (self.gl.CullFace)(match c {
                        CullMode::Front => GL_FRONT,
                        CullMode::Back => GL_BACK,
                        CullMode::FrontAndBack => GL_FRONT_AND_BACK,
                    });
                }
                (self.gl.FrontFace)(if state.front_face == FrontFace::Ccw {
                    GL_CCW
                } else {
                    GL_CW
                });
                (self.gl.ColorMask)(
                    state.color_mask[0] as u8,
                    state.color_mask[1] as u8,
                    state.color_mask[2] as u8,
                    state.color_mask[3] as u8,
                );
                self.toggle(GL_POLYGON_OFFSET_FILL, state.polygon_offset);
                (self.gl.PolygonOffset)(offset_factor, offset_units);
                self.pipeline = Some(state.clone());
            }
            let lighting = transforms.filter(|t| t.lighting.enabled);
            self.uniform_1i(self.loc("lightingEnabled"), lighting.is_some() as i32);
            self.uniform_1i(
                self.loc("twoSided"),
                (matches!(
                    draw_mode,
                    GL_TRIANGLES | GL_TRIANGLE_STRIP | GL_TRIANGLE_FAN
                ) && (lighting.is_some_and(|t| t.lighting.two_sided)
                    || (transforms.is_none() && state.two_sided_lighting))) as i32,
            );
            if let Some(t) = lighting {
                let l = &t.lighting;
                self.uniform_matrix4fv(self.loc("normalMatrix"), 1, 0, t.normal_matrix.as_ptr());
                self.uniform_1i(
                    self.loc("normalMode"),
                    if l.normalize {
                        1
                    } else if l.rescale {
                        2
                    } else {
                        0
                    },
                );
                let scale = (t.normal_matrix[8] * t.normal_matrix[8]
                    + t.normal_matrix[9] * t.normal_matrix[9]
                    + t.normal_matrix[10] * t.normal_matrix[10])
                    .sqrt();
                self.uniform_1f(
                    self.loc("normalRescale"),
                    if scale > 0. { 1. / scale } else { 1. },
                );
                self.uniform_1i(self.loc("colorMaterial"), l.color_material as i32);
                self.uniform_1f(self.loc("shininess"), l.shininess);
                for (name, value) in [
                    ("modelAmbient", l.model_ambient),
                    ("materialAmbient", l.ambient),
                    ("materialDiffuse", l.diffuse),
                    ("materialSpecular", l.specular),
                    ("materialEmission", l.emission),
                ] {
                    self.uniform_4fv(self.loc(name), 1, value.as_ptr());
                }
                let ambient: [[f32; 4]; 8] = std::array::from_fn(|i| {
                    let mut c = l.lights[i].ambient;
                    c[3] = l.lights[i].enabled as u8 as f32;
                    c
                });
                let diffuse: [[f32; 4]; 8] = std::array::from_fn(|i| l.lights[i].diffuse);
                let specular: [[f32; 4]; 8] = std::array::from_fn(|i| l.lights[i].specular);
                let positions: [[f32; 4]; 8] = std::array::from_fn(|i| l.lights[i].position);
                let spot: [[f32; 4]; 8] = std::array::from_fn(|i| {
                    let a = l.lights[i];
                    [
                        a.spot_direction[0],
                        a.spot_direction[1],
                        a.spot_direction[2],
                        if a.spot_cutoff == 180. {
                            -1.
                        } else {
                            a.spot_cutoff.to_radians().cos().max(0.)
                        },
                    ]
                });
                let attenuation: [[f32; 4]; 8] = std::array::from_fn(|i| {
                    let a = l.lights[i];
                    [
                        a.attenuation[0],
                        a.attenuation[1],
                        a.attenuation[2],
                        a.spot_exponent,
                    ]
                });
                for (name, values) in [
                    ("lightAmbient[0]", &ambient),
                    ("lightDiffuse[0]", &diffuse),
                    ("lightSpecular[0]", &specular),
                    ("lightPositions[0]", &positions),
                    ("lightSpot[0]", &spot),
                    ("lightAttenuation[0]", &attenuation),
                ] {
                    self.uniform_4fv(self.loc(name), 8, values.as_ptr().cast());
                }
            }
            self.uniform_1i(self.loc("objectSpace"), transforms.is_some() as i32);
            let projection = transforms.map(|t| &t.projection).unwrap_or(&IDENTITY);
            let modelview = transforms.map(|t| &t.modelview).unwrap_or(&IDENTITY);
            self.uniform_matrix4fv(self.loc("projectionMatrix"), 1, 0, projection.as_ptr());
            self.uniform_matrix4fv(self.loc("modelviewMatrix"), 1, 0, modelview.as_ptr());
            if !self.untextured_active {
                for (i, name) in TEXTURE_MATRICES
                    .iter()
                    .take(if wide_draw { 6 } else { 4 })
                    .enumerate()
                {
                    let matrix = transforms.map(|t| &t.textures[i]).unwrap_or(&IDENTITY);
                    self.uniform_matrix4fv(self.loc(name), 1, 0, matrix.as_ptr());
                }
            }
            self.uniform_2f(
                self.loc("depthRange"),
                state.depth_range.0,
                state.depth_range.1,
            );
            self.uniform_1i(
                self.loc("alphaMode"),
                if state.alpha_test {
                    compare(state.alpha_func) as i32 - GL_NEVER as i32
                } else {
                    7
                },
            );
            // The software pipeline compares alpha/ref at 8-bit fixed precision.
            // Upload the rounded reference as an exactly representable integer.
            self.uniform_1f(
                self.loc("alphaRef"),
                (state.alpha_ref.clamp(0., 1.) * 255. + 0.5).floor(),
            );
            self.uniform_1i(
                self.loc("fogMode"),
                if state.fog {
                    match state.fog_mode {
                        FogMode::Linear => 1,
                        FogMode::Exp => 2,
                        FogMode::Exp2 => 3,
                    }
                } else {
                    0
                },
            );
            self.uniform_4fv(self.loc("fogColor"), 1, state.fog_color.as_ptr());
            for (n, v) in [
                ("fogStart", state.fog_start),
                ("fogEnd", state.fog_end),
                ("fogDensity", state.fog_density),
            ] {
                self.uniform_1f(self.loc(n), v);
            }
            (self.gl.PixelStorei)(0x0cf5, 1);
            for (i, name) in names
                .into_iter()
                .take(if wide_draw { 6 } else { 4 })
                .enumerate()
            {
                // NV coordinate routes and sampled textures are independent.
                // Keep all needed vertex coordinates, but do not bind/upload
                // textures or send sampler state for an unreferenced unit.
                if sampled_units.is_some_and(|mask| mask & (1 << i) == 0) {
                    continue;
                }
                (self.gl.ActiveTexture)(GL_TEXTURE0 + i as u32);
                let stage = stages.iter().find(|s| s.unit == i);
                let mut id = self.white;
                let mut lod_limit = [1., 1., 0., 0.];
                if stage.is_some() {
                    if let Some(tex) = textures.get(&name).filter(|t| t.is_complete()) {
                        let item = self.textures.entry(name).or_insert_with(|| {
                            let mut id = 0;
                            (self.gl.GenTextures)(1, &mut id);
                            (id, u64::MAX)
                        });
                        id = item.0;
                        (self.gl.BindTexture)(GL_TEXTURE_2D, id);
                        let (sample_w, sample_h) = tex.sampling_size();
                        let relative_max = tex.max_level.saturating_sub(tex.base_level);
                        let (min_lod, max_lod) = tex.lod_bounds();
                        let last_level = sample_w.max(sample_h).ilog2();
                        let restricted = relative_max < last_level
                            || min_lod > 0.
                            || max_lod < last_level as f32;
                        let base_only = relative_max == 0
                            || (restricted && !self.native_max_level && !self.lod_derivatives);
                        lod_limit = [
                            sample_w as f32,
                            sample_h as f32,
                            relative_max.min(last_level) as f32,
                            ((restricted && !self.native_max_level && !base_only)
                                || min_lod > 0.
                                || max_lod < last_level as f32) as u8
                                as f32,
                        ];
                        let needs_mips = !base_only
                            && tex.min_filter.uses_mipmaps()
                            && (self.full_npot
                                || (sample_w.is_power_of_two() && sample_h.is_power_of_two()));
                        let newly_mipped = needs_mips
                            && self
                                .texture_params
                                .get(&id)
                                .is_none_or(|p| !p.0.uses_mipmaps());
                        if item.1 != tex.revision || newly_mipped {
                            if self.native_max_level {
                                (self.gl.TexParameteri)(
                                    GL_TEXTURE_2D,
                                    GL_TEXTURE_MAX_LEVEL_SGIS,
                                    relative_max.min(last_level) as i32,
                                );
                            }
                            // Do not let upload fallback consume a deferred draw
                            // error and misclassify it as a texture-format failure.
                            check_native_error(&self.gl)?;
                            let native_copy = if tex.base_level == 0 && !needs_mips {
                                if let Some(frame) = &tex.gpu_copy {
                                    self.import_guest_rgb_texture(frame)?
                                } else {
                                    false
                                }
                            } else {
                                false
                            };
                            if !native_copy {
                                upload_texture(
                                    &self.gl,
                                    &self.compressed_formats,
                                    tex,
                                    needs_mips,
                                    self.native_max_level,
                                )?;
                            }
                            self.textures.get_mut(&name).unwrap().1 = tex.revision;
                        }
                        let params = if base_only {
                            (
                                match tex.min_filter {
                                    Filter::Nearest
                                    | Filter::NearestMipmapNearest
                                    | Filter::NearestMipmapLinear => Filter::Nearest,
                                    _ => Filter::Linear,
                                },
                                tex.mag_filter,
                                tex.wrap_s,
                                tex.wrap_t,
                            )
                        } else if !self.full_npot
                            && (!sample_w.is_power_of_two() || !sample_h.is_power_of_two())
                        {
                            (
                                match tex.min_filter {
                                    Filter::Nearest
                                    | Filter::NearestMipmapNearest
                                    | Filter::NearestMipmapLinear => Filter::Nearest,
                                    _ => Filter::Linear,
                                },
                                tex.mag_filter,
                                Wrap::ClampToEdge,
                                Wrap::ClampToEdge,
                            )
                        } else {
                            (tex.min_filter, tex.mag_filter, tex.wrap_s, tex.wrap_t)
                        };
                        if self.texture_params.get(&id) != Some(&params) {
                            self.params(params.0, params.1, params.2, params.3);
                            self.texture_params.insert(id, params);
                        }
                    }
                }
                (self.gl.BindTexture)(GL_TEXTURE_2D, id);
                self.uniform_1i(self.loc(SAMPLERS[i]), i as i32);
                self.uniform_1i(
                    self.loc(ENVIRONMENTS[i]),
                    stage.map_or(-1, |s| match s.env {
                        TexEnvMode::Modulate => 0,
                        TexEnvMode::Replace => 1,
                        TexEnvMode::Decal => 2,
                        TexEnvMode::Blend => 3,
                        TexEnvMode::Add => 4,
                        TexEnvMode::Combine(_) => 5,
                    }),
                );
                if self.range_active {
                    let (lo, hi) = textures
                        .get(&name)
                        .map_or((-1000., 1000.), |t| t.lod_bounds());
                    self.uniform_2f(
                        self.loc(
                            [
                                "lodRange0",
                                "lodRange1",
                                "lodRange2",
                                "lodRange3",
                                "lodRange4",
                                "lodRange5",
                            ][i],
                        ),
                        lo,
                        hi,
                    );
                }
                self.uniform_4fv(self.loc(LOD_LIMITS[i]), 1, lod_limit.as_ptr());
                self.uniform_1f(self.loc(LOD_BIASES[i]), stage.map_or(0.0, |s| s.lod_bias));
                if let Some(s) = stage {
                    self.uniform_4fv(self.loc(ENV_COLORS[i]), 1, s.env_color.as_ptr());
                    if let TexEnvMode::Combine(c) = s.env {
                        let op = |v| match v {
                            0x1e01 => 0.,
                            0x2100 => 1.,
                            0x104 => 2.,
                            0x8574 => 3.,
                            0x8575 => 4.,
                            0x84e7 => 5.,
                            0x86ae => 6.,
                            _ => 7.,
                        };
                        let source = |v| match v {
                            0x1702 => 0.,
                            0x8576 => 1.,
                            0x8577 => 2.,
                            _ => 3.,
                        };
                        let source_vector =
                            |a: [u32; 3]| [source(a[0]), source(a[1]), source(a[2]), 0.];
                        let operand = |a: [u32; 3], base: u32| {
                            [
                                (a[0] - base) as f32,
                                (a[1] - base) as f32,
                                (a[2] - base) as f32,
                                0.,
                            ]
                        };
                        for (parameter, (_, value)) in [
                            (
                                "combineOps",
                                [
                                    op(c.rgb),
                                    op(c.alpha),
                                    c.rgb_scale as f32,
                                    c.alpha_scale as f32,
                                ],
                            ),
                            ("combineRgbSource", source_vector(c.rgb_sources)),
                            ("combineAlphaSource", source_vector(c.alpha_sources)),
                            ("combineRgbOperand", operand(c.rgb_operands, 0x300)),
                            ("combineAlphaOperand", operand(c.alpha_operands, 0x302)),
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            self.uniform_4fv(
                                self.loc(COMBINE_UNIFORMS[parameter][i]),
                                1,
                                value.as_ptr(),
                            );
                        }
                    }
                }
            }
            (self.gl.BindBuffer)(GL_ARRAY_BUFFER, self.buffer);
            let (data, length) = stream
                .map(|s| (s.bytes.as_ptr().cast::<c_void>(), s.bytes.len()))
                .unwrap_or((self.scratch.as_ptr().cast(), self.scratch.len() * 4));
            (self.gl.BufferData)(GL_ARRAY_BUFFER, length as isize, data, 0x88e0);
            if let Some(stream) = stream {
                if self.stream_layout != Some(stream.attributes) {
                    for (i, attr) in stream.attributes.iter().take(attribute_limit).enumerate() {
                        if let Some(attr) = attr {
                            (self.gl.EnableVertexAttribArray)(i as u32);
                            (self.gl.VertexAttribPointer)(
                                i as u32,
                                attr.size,
                                attr.ty,
                                attr.normalized,
                                attr.stride,
                                attr.offset as *const c_void,
                            );
                        } else {
                            (self.gl.DisableVertexAttribArray)(i as u32);
                        }
                    }
                    self.stream_layout = Some(stream.attributes);
                }
                for (i, attr) in stream
                    .attributes
                    .iter()
                    .take(if wide_draw { 10 } else { 8 })
                    .enumerate()
                {
                    if attr.is_none() {
                        (self.gl.VertexAttrib4fv)(i as u32, stream.constants[i].as_ptr());
                    }
                }
                self.attribute_layout = None;
            } else {
                if self.attribute_layout != Some(layout) {
                    for (i, attribute) in layout.attributes.iter().take(attribute_limit).enumerate()
                    {
                        if let Some((size, offset)) = attribute {
                            (self.gl.EnableVertexAttribArray)(i as u32);
                            (self.gl.VertexAttribPointer)(
                                i as u32,
                                *size,
                                GL_FLOAT,
                                0,
                                (layout.floats * 4) as i32,
                                (*offset * 4) as *const c_void,
                            );
                        } else {
                            (self.gl.DisableVertexAttribArray)(i as u32);
                        }
                    }
                    self.attribute_layout = Some(layout);
                }
                self.stream_layout = None;
            }
            self.uniform_1i(
                self.loc("logicMode"),
                if state.logic_op {
                    (state.logic_mode - 0x1500) as i32
                } else {
                    -1
                },
            );
            if ordered || framebuffer_fetch {
                let levels = if self.rgb565 {
                    [31., 63., 31., 255.]
                } else {
                    [255.; 4]
                };
                self.uniform_4fv(self.loc("logicLevels"), 1, levels.as_ptr());
                self.uniform_1i(
                    self.loc("logicDestination"),
                    if self.wide_textures { 6 } else { 4 },
                );
                let loc = self.loc("logicSize");
                (self.gl.Uniform2f)(loc, self.width as f32, self.height as f32);
                (self.gl.ActiveTexture)(GL_TEXTURE0 + if self.wide_textures { 6 } else { 4 });
                if self.logic_destination == 0 {
                    (self.gl.GenTextures)(1, &mut self.logic_destination);
                }
                (self.gl.BindTexture)(GL_TEXTURE_2D, self.logic_destination);
                self.params(
                    Filter::Nearest,
                    Filter::Nearest,
                    Wrap::ClampToEdge,
                    Wrap::ClampToEdge,
                );
            }
            if framebuffer_fetch && !ordered {
                // The sampled channels are immutable for this entire draw.
                self.copy_destination(true);
            }
            if indexed {
                let indices = stream
                    .map(|s| s.indices.as_slice())
                    .unwrap_or(&self.indices);
                (self.gl.BindBuffer)(GL_ELEMENT_ARRAY_BUFFER, self.index_buffer);
                (self.gl.BufferData)(
                    GL_ELEMENT_ARRAY_BUFFER,
                    (indices.len() * 2) as isize,
                    indices.as_ptr().cast(),
                    0x88e0,
                );
                if ordered {
                    let step = match draw_mode {
                        GL_POINTS => 1,
                        GL_LINES => 2,
                        _ => 3,
                    };
                    for first in (0..indices.len() / step * step).step_by(step) {
                        self.copy_destination(framebuffer_fetch);
                        (self.gl.DrawElements)(
                            draw_mode,
                            step as i32,
                            GL_UNSIGNED_SHORT,
                            (first * 2) as *const c_void,
                        );
                    }
                } else {
                    (self.gl.DrawElements)(
                        draw_mode,
                        indices.len() as i32,
                        GL_UNSIGNED_SHORT,
                        std::ptr::null(),
                    );
                }
            } else {
                let count = stream.map_or(self.scratch.len() / layout.floats, |s| s.count);
                if ordered {
                    let step = match draw_mode {
                        GL_POINTS => 1,
                        GL_LINES => 2,
                        _ => 3,
                    };
                    for first in (0..count / step * step).step_by(step) {
                        self.copy_destination(framebuffer_fetch);
                        (self.gl.DrawArrays)(draw_mode, first as i32, step as i32);
                    }
                } else {
                    (self.gl.DrawArrays)(draw_mode, 0, count as i32);
                }
            }
            if compatibility_pass.is_some() {
                // Reuse uploaded vertices, indices, textures and shader uniforms.
                // The nearest unbiased depth from the first pass remains intact.
                (self.gl.DepthMask)(0);
                if replay_offset_units != 0. {
                    self.toggle(GL_POLYGON_OFFSET_FILL, true);
                    (self.gl.PolygonOffset)(0., replay_offset_units);
                    self.uniform_1i(self.loc("alphaMode"), 2);
                    self.uniform_1f(self.loc("alphaRef"), 255.);
                }
                if indexed {
                    let count = stream.map_or(self.indices.len(), |s| s.indices.len());
                    (self.gl.DrawElements)(
                        draw_mode,
                        count as i32,
                        GL_UNSIGNED_SHORT,
                        std::ptr::null(),
                    );
                } else {
                    let count = stream.map_or(self.scratch.len() / layout.floats, |s| s.count);
                    (self.gl.DrawArrays)(draw_mode, 0, count as i32);
                }
                (self.gl.DepthMask)(state.depth_write as u8);
                if replay_offset_units != 0. {
                    self.toggle(GL_POLYGON_OFFSET_FILL, state.polygon_offset);
                    (self.gl.PolygonOffset)(offset_factor, offset_units);
                    self.uniform_1i(
                        self.loc("alphaMode"),
                        if state.alpha_test {
                            compare(state.alpha_func) as i32 - GL_NEVER as i32
                        } else {
                            7
                        },
                    );
                    self.uniform_1f(
                        self.loc("alphaRef"),
                        (state.alpha_ref.clamp(0., 1.) * 255. + 0.5).floor(),
                    );
                }
            }
            self.dirty = true;
            // NVIDIA drivers can stall in glGetError after each draw. Check at
            // exports/reads/finish and explicit guest queries instead.
            Ok(())
        })();
        if let Some((id, mut shader)) = combiner_state {
            self.swap_program_state(&mut shader);
            if self.combiner_programs.len() >= 128 {
                for (_, (program, _, _)) in self.combiner_programs.drain() {
                    unsafe { (self.gl.DeleteProgram)(program) };
                }
            }
            self.combiner_programs.insert((id, range), shader);
        }
        result
    }
    /// Explicit batch-boundary driver check; does not read pixels or glFinish.
    pub fn poll_error(&self) -> Result<(), String> {
        self.host.make_current()?;
        unsafe { self.check() }
    }
    /// Wait for this context's queued GL work without reading back or presenting.
    pub fn finish(&mut self) -> Result<(), String> {
        self.host.make_current()?;
        unsafe {
            (self.gl.Finish)();
            self.check()
        }
    }

    /// Submit this context's queued work without waiting for its completion.
    pub fn flush(&mut self) -> Result<(), String> {
        self.host.make_current()?;
        unsafe {
            (self.gl.Flush)();
            Ok(())
        }
    }

    /// Export the already-read native image. No additional GPU readback.
    pub fn scaled_rgb565_into(&self, out: &mut Vec<u8>) -> Option<(u32, u32)> {
        if self.render_scale == 1
            || self.dirty
            || self.readback.len() != (self.width * self.height * 4) as usize
        {
            return None;
        }
        out.resize((self.width * self.height * 2) as usize, 0);
        if self.display_prepared {
            out.copy_from_slice(&self.packed_readback);
            return Some((self.width, self.height));
        }
        for y in 0..self.height as usize {
            let row = self.height as usize - 1 - y;
            for x in 0..self.width as usize {
                let src = (row * self.width as usize + x) * 4;
                let value = ((self.readback[src] as u16 >> 3) << 11)
                    | ((self.readback[src + 1] as u16 >> 2) << 5)
                    | (self.readback[src + 2] as u16 >> 3);
                let dst = (y * self.width as usize + x) * 2;
                out[dst..dst + 2].copy_from_slice(&value.to_le_bytes());
            }
        }
        Some((self.width, self.height))
    }

    /// Publish top-down opaque RGBA at the actual surface precision.
    /// Uses the existing readback; introduces no GL call or synchronization.
    pub fn scaled_display_rgba_into(&self, out: &mut Vec<u8>) -> Option<(u32, u32)> {
        if self.render_scale == 1 && self.rgb565
            || self.dirty
            || self.readback.len() != (self.width * self.height * 4) as usize
        {
            return None;
        }
        out.resize(self.readback.len(), 0);
        if self.display_prepared {
            out.copy_from_slice(&self.readback);
            return Some((self.width, self.height));
        }
        let stride = self.width as usize * 4;
        for (src, dst) in self
            .readback
            .chunks_exact(stride)
            .rev()
            .zip(out.chunks_exact_mut(stride))
        {
            for (src, dst) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
                if !self.rgb565 {
                    dst.copy_from_slice(&[src[0], src[1], src[2], 255]);
                    continue;
                }
                let r = src[0] >> 3;
                let g = src[1] >> 2;
                let b = src[2] >> 3;
                dst.copy_from_slice(&[
                    (r << 3) | (r >> 2),
                    (g << 2) | (g >> 4),
                    (b << 3) | (b >> 2),
                    255,
                ]);
            }
        }
        Some((self.width, self.height))
    }

    pub fn read(&mut self, target: &mut RenderTarget) -> Result<(), String> {
        self.read_inner(target, None)
    }
    pub fn read_present(
        &mut self,
        target: &mut RenderTarget,
        rgb565: &mut [u8],
    ) -> Result<(), String> {
        self.read_inner(target, Some(rgb565))
    }
    fn read_inner(
        &mut self,
        target: &mut RenderTarget,
        mut rgb565: Option<&mut [u8]>,
    ) -> Result<(), String> {
        self.resize(target.width, target.height)?;
        // Before optional resolve or a clean-cache early return, preserve a
        // pending native error rather than letting a fallback drain it.
        self.poll_error()?;
        if !self.dirty {
            if let Some(out) = rgb565 {
                if out.len() == self.logical_packed.len() && !self.logical_packed.is_empty() {
                    out.copy_from_slice(&self.logical_packed);
                } else {
                    target.to_rgb565(out);
                }
            }
            return Ok(());
        }
        self.host.make_current()?;
        // Queue/complete the reduction before the full display readback. Both
        // results then belong to the same untouched native color attachment.
        let gpu_resolved = self.render_scale > 1
            && unsafe { self.resolve_logical_gpu(target.width, target.height) };
        unsafe {
            self.display_prepared = false;
            self.logical_packed.clear();
            if gpu_resolved {
                let prepared = (|| {
                    let logical_texture = self.logical_resolve.as_ref().unwrap().texture;
                    self.pack_rgb565_readback(logical_texture, target.width, target.height)?;
                    self.logical_packed.extend_from_slice(&self.packed_readback);
                    self.prepare_display_readback()
                })();
                if prepared.is_ok() {
                    target
                        .color
                        .copy_from_slice(&self.logical_resolve.as_ref().unwrap().pixels);
                    if let Some(out) = rgb565 {
                        if out.len() == self.logical_packed.len() {
                            out.copy_from_slice(&self.logical_packed);
                        } else {
                            target.to_rgb565(out);
                        }
                    }
                    self.dirty = false;
                    return Ok(());
                }
                self.logical_packed.clear();
            }
            self.readback
                .resize((self.width * self.height * 4) as usize, 0);
            (self.gl.BindFramebuffer)(0x8d40, self.fbo);
            (self.gl.PixelStorei)(0x0d05, 1);
            {
                let _diagnostic = crate::graphics_diagnostic::Span::new(
                    crate::graphics_diagnostic::Stage::ReadPixels,
                );
                (self.gl.ReadPixels)(
                    0,
                    0,
                    self.width as i32,
                    self.height as i32,
                    GL_RGBA,
                    GL_UNSIGNED_BYTE,
                    self.readback.as_mut_ptr().cast(),
                );
            }
            self.check()?;
            if gpu_resolved {
                target
                    .color
                    .copy_from_slice(&self.logical_resolve.as_ref().unwrap().pixels);
                if let Some(out) = rgb565 {
                    target.to_rgb565(out);
                }
                self.dirty = false;
                return Ok(());
            }
            if self.render_scale > 1 {
                // Guest read/copy APIs still see the logical image. Resolve only
                // that cache; retain full native pixels for host presentation.
                let k = self.render_scale as usize;
                let native_w = self.width as usize;
                let native_h = self.height as usize;
                let logical_w = target.width as usize;
                for y in 0..target.height as usize {
                    for x in 0..logical_w {
                        let mut sum = [0u32; 4];
                        for dy in 0..k {
                            let row = native_h - 1 - (y * k + dy);
                            for dx in 0..k {
                                let offset = (row * native_w + x * k + dx) * 4;
                                for (channel, value) in sum.iter_mut().enumerate() {
                                    *value += self.readback[offset + channel] as u32;
                                }
                            }
                        }
                        let offset = (y * logical_w + x) * 4;
                        let p =
                            std::array::from_fn(|channel| (sum[channel] / (k * k) as u32) as u8);
                        let p = if target.rgb565 {
                            crate::raster::rgb565_pixel(p)
                        } else {
                            p
                        };
                        target.color[offset..offset + 4].copy_from_slice(&p);
                    }
                }
                if let Some(out) = rgb565 {
                    target.to_rgb565(out);
                }
                self.dirty = false;
                return Ok(());
            }
            let stride = self.width as usize * 4;
            for (row, out) in target.color.chunks_exact_mut(stride).enumerate() {
                let start = (self.height as usize - 1 - row) * stride;
                let source = &self.readback[start..start + stride];
                let rgb_row = rgb565.as_deref_mut().and_then(|pixels| {
                    let offset = row * self.width as usize * 2;
                    let end = (offset + self.width as usize * 2).min(pixels.len());
                    pixels.get_mut(offset..end)
                });
                if let Some(rgb_row) = rgb_row.filter(|row| row.len() == stride / 2) {
                    // One traversal produces both caches, preserving the RGBA image
                    // needed by glReadPixels and copy-to-texture calls after a swap.
                    for ((src, rgba), rgb) in source
                        .chunks_exact(4)
                        .zip(out.chunks_exact_mut(4))
                        .zip(rgb_row.chunks_exact_mut(2))
                    {
                        rgba.copy_from_slice(src);
                        let value = ((src[0] as u16 >> 3) << 11)
                            | ((src[1] as u16 >> 2) << 5)
                            | (src[2] as u16 >> 3);
                        rgb.copy_from_slice(&value.to_le_bytes());
                    }
                } else {
                    out.copy_from_slice(source);
                    // Match the existing converter when a caller supplies a short output.
                    if let Some(pixels) = rgb565.as_deref_mut() {
                        let offset = row * self.width as usize * 2;
                        if let Some(tail) = pixels.get_mut(offset..) {
                            for (src, rgb) in source.chunks_exact(4).zip(tail.chunks_exact_mut(2)) {
                                let value = ((src[0] as u16 >> 3) << 11)
                                    | ((src[1] as u16 >> 2) << 5)
                                    | (src[2] as u16 >> 3);
                                rgb.copy_from_slice(&value.to_le_bytes());
                            }
                        }
                    }
                }
            }
            self.dirty = false;
            Ok(())
        }
    }
}
impl Drop for Renderer {
    fn drop(&mut self) {
        if self.host.make_current().is_err() {
            return;
        }
        self.retire_shared_frames();
        unsafe {
            if let Some(timer) = self.diagnostic.take() {
                timer.borrow_mut().release();
            }
            self.delete_transfers();
            if let Some(resolve) = self.logical_resolve.take() {
                resolve.delete(&self.gl);
            }
            for (id, _) in self.textures.values() {
                (self.gl.DeleteTextures)(1, id);
            }
            for id in [
                self.color,
                self.white,
                self.logic_destination,
                self.upload_source,
            ] {
                (self.gl.DeleteTextures)(1, &id);
            }
            (self.gl.DeleteRenderbuffers)(1, &self.depth);
            (self.gl.DeleteFramebuffers)(1, &self.fbo);
            (self.gl.DeleteBuffers)(1, &self.buffer);
            (self.gl.DeleteBuffers)(1, &self.index_buffer);
            if let Some((program, _, _)) = &self.simple_program {
                (self.gl.DeleteProgram)(*program);
            }
            if let Some((program, _, _)) = &self.untextured_program {
                (self.gl.DeleteProgram)(*program);
            }
            if let Some((program, _, _)) = &self.range_program {
                (self.gl.DeleteProgram)(*program);
            }
            (self.gl.DeleteProgram)(self.program);
            for (program, _, _) in self.combiner_programs.values() {
                (self.gl.DeleteProgram)(*program);
            }
            if let Some((program, _, _)) = &self.alternate_program {
                (self.gl.DeleteProgram)(*program);
            }
        }
    }
}
fn compare(v: CompareFunc) -> u32 {
    GL_NEVER
        + match v {
            CompareFunc::Never => 0,
            CompareFunc::Less => 1,
            CompareFunc::Equal => 2,
            CompareFunc::Lequal => 3,
            CompareFunc::Greater => 4,
            CompareFunc::Notequal => 5,
            CompareFunc::Gequal => 6,
            CompareFunc::Always => 7,
        }
}
fn blend(v: BlendFactor) -> u32 {
    match v {
        BlendFactor::Zero => GL_ZERO,
        BlendFactor::One => GL_ONE,
        BlendFactor::SrcColor => GL_SRC_COLOR,
        BlendFactor::OneMinusSrcColor => GL_ONE_MINUS_SRC_COLOR,
        BlendFactor::SrcAlpha => GL_SRC_ALPHA,
        BlendFactor::OneMinusSrcAlpha => GL_ONE_MINUS_SRC_ALPHA,
        BlendFactor::DstAlpha => GL_DST_ALPHA,
        BlendFactor::OneMinusDstAlpha => GL_ONE_MINUS_DST_ALPHA,
        BlendFactor::DstColor => GL_DST_COLOR,
        BlendFactor::OneMinusDstColor => GL_ONE_MINUS_DST_COLOR,
        BlendFactor::SrcAlphaSaturate => GL_SRC_ALPHA_SATURATE,
    }
}
fn filter(v: Filter) -> u32 {
    match v {
        Filter::Nearest => GL_NEAREST,
        Filter::Linear => GL_LINEAR,
        Filter::NearestMipmapNearest => GL_NEAREST_MIPMAP_NEAREST,
        Filter::LinearMipmapNearest => GL_LINEAR_MIPMAP_NEAREST,
        Filter::NearestMipmapLinear => GL_NEAREST_MIPMAP_LINEAR,
        Filter::LinearMipmapLinear => GL_LINEAR_MIPMAP_LINEAR,
    }
}
fn wrap(v: Wrap) -> u32 {
    match v {
        Wrap::Repeat => GL_REPEAT,
        Wrap::ClampToEdge | Wrap::GizmondoClamp => GL_CLAMP_TO_EDGE,
    }
}
// Conservative, draw-level proof: no per-fragment branch is needed for q=1.
fn affine_texture_coordinates(
    vertices: &[Vertex],
    stages: &[TextureStage],
    transforms: Option<&VertexTransforms>,
    stream: Option<&VertexStream>,
) -> bool {
    stages.iter().all(|stage| {
        if let Some(t) = transforms {
            let m = &t.textures[stage.unit];
            if [m[3], m[7], m[11], m[15]] != [0., 0., 0., 1.] {
                return false;
            }
        }
        if let Some(stream) = stream {
            match stream.attributes[texture_attribute(stage.unit)] {
                Some(a) => a.size < 4,
                None => stream.constants[texture_attribute(stage.unit)][3] == 1.,
            }
        } else {
            vertices.iter().all(|v| v.texcoord[stage.unit][3] == 1.)
        }
    })
}
fn affine_shader_sources() -> (String, String) {
    (
        format!("#define AFFINE_TEXTURES\n{VERTEX}"),
        format!("#define AFFINE_TEXTURES\n{FRAGMENT}"),
    )
}
const VERTEX: &str = r#"
attribute vec4 position,color;attribute vec4 tex0,tex1,tex2,tex3;attribute vec4 normalAndFog,lightPosition;
VARYING vec4 vColor,vBackColor;
#ifdef AFFINE_TEXTURES
#define UV_TYPE vec2
#define UV_VALUE(v) (v).xy
#else
#define UV_TYPE vec4
#define UV_VALUE(v) (v)
#endif
TEX_VARYING UV_TYPE uv0,uv1,uv2,uv3;
#ifdef WIDE_TEXTURES
TEX_VARYING UV_TYPE uv4,uv5;
#endif
VARYING float vFog;uniform vec2 depthRange;
uniform mat4 projectionMatrix,modelviewMatrix,textureMatrix0,textureMatrix1,textureMatrix2,textureMatrix3;
#ifdef WIDE_TEXTURES
attribute vec4 tex4,tex5;uniform mat4 textureMatrix4,textureMatrix5;
#endif
uniform int objectSpace,lightingEnabled,normalMode,colorMaterial,twoSided;
uniform mat4 normalMatrix;
uniform float normalRescale,shininess,pointSize;uniform int pointArray;uniform vec4 pointArrayParams;
uniform vec4 modelAmbient,materialAmbient,materialDiffuse,materialSpecular,materialEmission;
uniform vec4 lightAmbient[8],lightDiffuse[8],lightSpecular[8],lightPositions[8],lightSpot[8],lightAttenuation[8];
vec3 unit(vec3 v){float l=length(v);return l>0.0?v/l:vec3(0.0);}
vec4 illuminate(vec3 n,vec3 point){
 vec4 ma=colorMaterial!=0?color:materialAmbient;
 vec4 md=colorMaterial!=0?color:materialDiffuse;
 vec3 result=materialEmission.rgb+modelAmbient.rgb*ma.rgb;
 for(int i=0;i<8;i++){
  if(lightAmbient[i].a>0.5){
   vec3 delta=lightPositions[i].w==0.0?lightPositions[i].xyz:lightPositions[i].xyz/lightPositions[i].w-point;
   float distance=length(delta);vec3 l=unit(delta);float attenuation=1.0;
   if(lightPositions[i].w!=0.0){vec3 k=lightAttenuation[i].xyz;attenuation=1.0/max(k.x+k.y*distance+k.z*distance*distance,0.000001);}
   float spot=dot(unit(lightSpot[i].xyz),-l);
   if(lightSpot[i].w>=0.0){if(spot<lightSpot[i].w){attenuation=0.0;}else if(lightAttenuation[i].w>0.0){attenuation*=pow(max(spot,0.0),lightAttenuation[i].w);}}
   float diffuse=max(dot(n,l),0.0);float spec=0.0;
   if(diffuse>0.0){float nh=max(dot(n,unit(l+vec3(0.0,0.0,1.0))),0.0);spec=shininess==0.0?1.0:pow(nh,shininess);}
   result+=attenuation*(lightAmbient[i].rgb*ma.rgb+diffuse*lightDiffuse[i].rgb*md.rgb+spec*lightSpecular[i].rgb*materialSpecular.rgb);
  }
 }
 return vec4(clamp(result,0.0,1.0),clamp(md.a,0.0,1.0));
}
void main(){
 gl_PointSize=pointSize;

#ifndef AFFINE_TEXTURES
 if(pointArray!=0){float size=lightPosition.x;if(pointArrayParams.w>0.5)size=floor(size+0.5);gl_PointSize=clamp(size*pointArrayParams.x,pointArrayParams.y,pointArrayParams.z);}

#endif
 vec4 eye=modelviewMatrix*position;
 vec4 clip=projectionMatrix*eye;
 gl_Position=clip;
 gl_Position.z=(depthRange.y-depthRange.x)*clip.z+(depthRange.x+depthRange.y-1.0)*clip.w;
 vColor=color;vBackColor=objectSpace!=0?color:vec4(normalAndFog.xyz,color.a);
 if(lightingEnabled!=0){
  vec3 n=(normalMatrix*vec4(normalAndFog.xyz,0.0)).xyz;
  if(normalMode==1){n=unit(n);}else if(normalMode==2){n*=normalRescale;}
#ifdef AFFINE_TEXTURES
  vec4 lightEye=modelviewMatrix*lightPosition;
#else
  vec4 lightEye=modelviewMatrix*(pointArray!=0?position:lightPosition);
#endif
  vec3 point=lightEye.w==0.0?lightEye.xyz:lightEye.xyz/lightEye.w;
  vColor=illuminate(n,point);vBackColor=twoSided!=0?illuminate(-n,point):vColor;
 }
#ifndef UNTEXTURED_DRAW
 uv0=UV_VALUE(textureMatrix0*tex0);
 uv1=UV_VALUE(textureMatrix1*tex1);
 uv2=UV_VALUE(textureMatrix2*tex2);
 uv3=UV_VALUE(textureMatrix3*tex3);
#ifdef WIDE_TEXTURES
 uv4=UV_VALUE(textureMatrix4*tex4);uv5=UV_VALUE(textureMatrix5*tex5);
#endif
#endif
 vFog=objectSpace!=0?abs(eye.z):normalAndFog.w;
}
"#;
// Only selected under simple_eligible. All transforms, lighting and fixed
// pipeline blending/depth/scissor stay on their original paths. The real
// RGB565 attachment still quantizes every write, including blended writes.
const SIMPLE_FRAGMENT: &str = r#"
VARYING vec4 vColor;VARYING vec2 uv0;
uniform sampler2D sampler0;uniform int env0;
void main(){vec4 c=vColor;if(env0>=0)c*=texture2D(sampler0,uv0);gl_FragColor=c;}
"#;
const NV_CONSTANTS: [&str; 4] = [
    "nvConstants[0]",
    "nvConstants[1]",
    "nvConstants[2]",
    "nvConstants[3]",
];

const FRAGMENT: &str = r#"
VARYING vec4 vColor,vBackColor;
#ifdef AFFINE_TEXTURES
#define UV_TYPE vec2
#define UV_VALUE(v) (v).xy
#else
#define UV_TYPE vec4
#define UV_VALUE(v) (v)
#endif
TEX_VARYING UV_TYPE uv0,uv1,uv2,uv3;
#ifdef WIDE_TEXTURES
TEX_VARYING UV_TYPE uv4,uv5;
#endif
VARYING float vFog;
uniform int logicMode;uniform sampler2D logicDestination;uniform vec2 logicSize;uniform vec4 logicLevels;
// ES2 has no integer bitwise operators. All intermediates are exact 8-bit integers.
#ifndef AFFINE_TEXTURES
vec4 logicalColor(vec4 source,vec4 destination){
 vec4 s=floor(clamp(source,0.0,1.0)*logicLevels+0.5),d=floor(destination*logicLevels+0.5),a=vec4(0.0);
 float bit=1.0;
 for(int i=0;i<8;i++){a+=mod(s,2.0)*mod(d,2.0)*bit;s=floor(s/2.0);d=floor(d/2.0);bit*=2.0;}
 s=floor(clamp(source,0.0,1.0)*logicLevels+0.5);d=floor(destination*logicLevels+0.5);
 vec4 r=vec4(0.0);
 if(logicMode==1)r=a;if(logicMode==2)r=s-a;if(logicMode==3)r=s;
 if(logicMode==4)r=d-a;if(logicMode==5)r=d;if(logicMode==6)r=s+d-2.0*a;
 if(logicMode==7)r=s+d-a;if(logicMode==8)r=logicLevels-(s+d-a);
 if(logicMode==9)r=logicLevels-(s+d-2.0*a);if(logicMode==10)r=logicLevels-d;
 if(logicMode==11)r=logicLevels-d+a;if(logicMode==12)r=logicLevels-s;
 if(logicMode==13)r=logicLevels-s+a;if(logicMode==14)r=logicLevels-a;if(logicMode==15)r=logicLevels;
 return r/logicLevels;
}
#endif
uniform vec4 lodLimit0,lodLimit1,lodLimit2,lodLimit3;
uniform vec2 lodRange0,lodRange1,lodRange2,lodRange3;
uniform float lodBias0,lodBias1,lodBias2,lodBias3;uniform sampler2D sampler0,sampler1,sampler2,sampler3;uniform int env0,env1,env2,env3;uniform vec4 envColor0,envColor1,envColor2,envColor3;
uniform vec4 pointReplace,pointReplaceHigh;uniform int alphaMode,fogMode,twoSided;uniform float alphaRef,fogStart,fogEnd,fogDensity;uniform vec4 fogColor;
uniform vec4 combineOps0,combineRgbSource0,combineAlphaSource0,combineRgbOperand0,combineAlphaOperand0;
uniform vec4 combineOps1,combineRgbSource1,combineAlphaSource1,combineRgbOperand1,combineAlphaOperand1;
uniform vec4 combineOps2,combineRgbSource2,combineAlphaSource2,combineRgbOperand2,combineAlphaOperand2;
uniform vec4 combineOps3,combineRgbSource3,combineAlphaSource3,combineRgbOperand3,combineAlphaOperand3;
#ifdef WIDE_TEXTURES
uniform sampler2D sampler4;uniform float lodBias4;uniform vec4 lodLimit4;uniform vec2 lodRange4;uniform int env4;uniform vec4 envColor4;
uniform vec4 combineOps4,combineRgbSource4,combineAlphaSource4,combineRgbOperand4,combineAlphaOperand4;
uniform sampler2D sampler5;uniform float lodBias5;uniform vec4 lodLimit5;uniform vec2 lodRange5;uniform int env5;uniform vec4 envColor5;
uniform vec4 combineOps5,combineRgbSource5,combineAlphaSource5,combineRgbOperand5,combineAlphaOperand5;
#endif


vec4 selectSource(float s,vec4 t,vec4 k,vec4 primary,vec4 previous){if(s<0.5)return t;if(s<1.5)return k;if(s<2.5)return primary;return previous;}
vec3 rgbOperand(vec4 v,float op){if(op<0.5)return v.rgb;if(op<1.5)return vec3(1.0)-v.rgb;if(op<2.5)return vec3(v.a);return vec3(1.0-v.a);}
vec3 rgbOperation(float f,vec3 a,vec3 b,vec3 c){if(f<0.5)return a;if(f<1.5)return a*b;if(f<2.5)return a+b;if(f<3.5)return a+b-vec3(0.5);if(f<4.5)return a*c+b*(vec3(1.0)-c);if(f<5.5)return a-b;return vec3(4.0*dot(a-vec3(0.5),b-vec3(0.5)));}
float alphaOperand(vec4 v,float op){return op<0.5?v.a:1.0-v.a;}
vec4 combineStage(vec4 previous,vec4 t,vec4 k,vec4 primary,vec4 ops,vec4 rs,vec4 asrc,vec4 ro,vec4 ao){
 vec3 a=rgbOperand(selectSource(rs.x,t,k,primary,previous),ro.x),b=rgbOperand(selectSource(rs.y,t,k,primary,previous),ro.y),c=rgbOperand(selectSource(rs.z,t,k,primary,previous),ro.z);
 float aa=alphaOperand(selectSource(asrc.x,t,k,primary,previous),ao.x),ab=alphaOperand(selectSource(asrc.y,t,k,primary,previous),ao.y),ac=alphaOperand(selectSource(asrc.z,t,k,primary,previous),ao.z);
 vec3 rgb=rgbOperation(ops.x,a,b,c);float alpha=rgbOperation(ops.y,vec3(aa),vec3(ab),vec3(ac)).x*ops.w;
 if(ops.x>6.5)alpha=rgb.x*ops.w;
 return clamp(vec4(rgb*ops.z,alpha),0.0,1.0);
}
vec4 stage(vec4 c,vec4 t,int e,vec4 k){if(e==0)return c*t;if(e==1)return t;if(e==2)return vec4(mix(c.rgb,t.rgb,t.a),c.a);if(e==3)return vec4(mix(c.rgb,k.rgb,t.rgb),c.a*t.a);if(e==4)return vec4(min(c.rgb+t.rgb,vec3(1.0)),c.a*t.a);return c;}
#ifdef AFFINE_TEXTURES
vec2 pointCoordinate(vec2 uv,float replace){return uv;}
#else
vec2 project(vec4 uv){return uv.w==0.0?vec2(0.0):uv.xy/uv.w;}
vec2 pointCoordinate(vec4 uv,float replace){return replace>0.5?gl_PointCoord:project(uv);}
#endif
float limitedBias(vec2 uv,float bias,vec4 limit,vec2 bounds){
#ifdef LOD_DERIVATIVES
 if(limit.w>0.5){
  vec2 dx=dFdx(uv)*limit.xy;vec2 dy=dFdy(uv)*limit.xy;
  float rho=max(length(dx),length(dy));
  float lod=log2(max(rho,0.000001));
  bias=min(bias,limit.z-lod);
 }
#endif
 return bias;
}
#ifdef TEXTURE_LOD_RANGE
vec4 sampleRange(sampler2D tex,vec2 uv,float bias,vec4 limit,vec2 bounds){
 if(bounds.x>0.0 || bounds.y<limit.z){
  vec2 dx=dFdx(uv)*limit.xy;vec2 dy=dFdy(uv)*limit.xy;
  float lod=log2(max(max(length(dx),length(dy)),0.000001))+bias;
  lod=min(max(lod,bounds.x),bounds.y);
  return RANGE_SAMPLE(tex,uv,min(lod,limit.z));
 }
 return texture2D(tex,uv,limitedBias(uv,bias,limit,bounds));
}
#else
#define sampleRange(tex,uv,bias,lim,ran) texture2D(tex,uv,limitedBias(uv,bias,lim,ran))
#endif
/* NV_COMBINER_FUNCTION */
void main(){vec4 c=twoSided!=0&&!gl_FrontFacing?vBackColor:vColor;vec4 primary=c;
#ifdef NV_COMBINER
c=nvCombiner(primary);
#elif !defined(UNTEXTURED_DRAW)
if(env0>=0){vec4 t=sampleRange(sampler0,pointCoordinate(uv0,pointReplace.x),lodBias0,lodLimit0,lodRange0);if(env0==5)c=combineStage(c,t,envColor0,primary,combineOps0,combineRgbSource0,combineAlphaSource0,combineRgbOperand0,combineAlphaOperand0);else c=stage(c,t,env0,envColor0);}if(env1>=0){vec4 t=sampleRange(sampler1,pointCoordinate(uv1,pointReplace.y),lodBias1,lodLimit1,lodRange1);if(env1==5)c=combineStage(c,t,envColor1,primary,combineOps1,combineRgbSource1,combineAlphaSource1,combineRgbOperand1,combineAlphaOperand1);else c=stage(c,t,env1,envColor1);}if(env2>=0){vec4 t=sampleRange(sampler2,pointCoordinate(uv2,pointReplace.z),lodBias2,lodLimit2,lodRange2);if(env2==5)c=combineStage(c,t,envColor2,primary,combineOps2,combineRgbSource2,combineAlphaSource2,combineRgbOperand2,combineAlphaOperand2);else c=stage(c,t,env2,envColor2);}if(env3>=0){vec4 t=sampleRange(sampler3,pointCoordinate(uv3,pointReplace.w),lodBias3,lodLimit3,lodRange3);if(env3==5)c=combineStage(c,t,envColor3,primary,combineOps3,combineRgbSource3,combineAlphaSource3,combineRgbOperand3,combineAlphaOperand3);else c=stage(c,t,env3,envColor3);}
#ifdef WIDE_TEXTURES
if(env4>=0){vec4 t=sampleRange(sampler4,pointCoordinate(uv4,pointReplaceHigh.x),lodBias4,lodLimit4,lodRange4);if(env4==5)c=combineStage(c,t,envColor4,primary,combineOps4,combineRgbSource4,combineAlphaSource4,combineRgbOperand4,combineAlphaOperand4);else c=stage(c,t,env4,envColor4);}
if(env5>=0){vec4 t=sampleRange(sampler5,pointCoordinate(uv5,pointReplaceHigh.y),lodBias5,lodLimit5,lodRange5);if(env5==5)c=combineStage(c,t,envColor5,primary,combineOps5,combineRgbSource5,combineAlphaSource5,combineRgbOperand5,combineAlphaOperand5);else c=stage(c,t,env5,envColor5);}
#endif

#endif
float alpha=floor(clamp(c.a,0.0,1.0)*255.0+0.5);
if(alphaMode==0||(alphaMode==1&&alpha>=alphaRef)||(alphaMode==2&&alpha!=alphaRef)||(alphaMode==3&&alpha>alphaRef)||(alphaMode==4&&alpha<=alphaRef)||(alphaMode==5&&alpha==alphaRef)||(alphaMode==6&&alpha<alphaRef))discard;
// Carmageddon uses decreasing negative linear-fog endpoints. Preserve the
// signed span, and match software's unfogged result for a degenerate range.
float f=1.0;if(fogMode==1){float span=fogEnd-fogStart;if(span!=0.0 && abs(span)>=0.000000001)f=(fogEnd-vFog)/span;}if(fogMode==2)f=exp(-fogDensity*vFog);if(fogMode==3){float d=fogDensity*vFog;f=exp(-d*d);}c.rgb=mix(fogColor.rgb,c.rgb,clamp(f,0.0,1.0));
#ifndef AFFINE_TEXTURES
if(logicMode>=0)c=logicalColor(c,texture2D(logicDestination,gl_FragCoord.xy/logicSize));
#endif
gl_FragColor=c;}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster;
    #[test]
    #[ignore = "requires native OpenGL/GLES; cached host renderer identity"]
    fn host_renderer_identity_matches_the_current_desktop_and_es_driver() {
        for es in [false, true] {
            let renderer = if es {
                Renderer::new_es_for_test(8, 8).unwrap()
            } else {
                Renderer::new(8, 8).unwrap()
            };
            unsafe {
                let ptr = (renderer.gl.GetString)(0x1f01);
                assert!(!ptr.is_null());
                let expected = CStr::from_ptr(ptr.cast()).to_string_lossy();
                assert!(!expected.is_empty());
                assert_eq!(renderer.renderer_name(), expected);
            }
        }
    }

    #[test]
    #[ignore = "requires native OpenGL/GLES; specialized fragment equivalence"]
    fn simple_fragment_matches_complete_rgb565_for_textures_alpha_and_blending() {
        for es in [false, true] {
            let make = || {
                let mut g = if es {
                    Renderer::new_es_for_test(8, 8).unwrap()
                } else {
                    Renderer::new(8, 8).unwrap()
                };
                g.set_surface_rgb565(true).unwrap();
                g
            };
            let (mut fast, mut reference) = (make(), make());
            reference.simple_available = false;
            assert!(fast.simple_available);
            let (mut a, mut b) = (
                RenderTarget::with_format(8, 8, true),
                RenderTarget::with_format(8, 8, true),
            );
            let mut tex = Texture::default();
            tex.set_level(
                0,
                2,
                2,
                vec![
                    240, 120, 200, 91, 32, 120, 200, 255, 80, 231, 40, 137, 200, 23, 167, 199,
                ],
            );
            let stage = TextureStage {
                unit: 0,
                env: TexEnvMode::Modulate,
                env_color: [0.; 4],
                lod_bias: 0.,
            };
            for filter in [Filter::Nearest, Filter::Linear] {
                tex.min_filter = filter;
                tex.mag_filter = filter;
                let textures = HashMap::from([(1, tex.clone())]);
                for textured in [false, true] {
                    for alpha in [0., 0.1, 0.5, 0.7, 1.] {
                        for blend_on in [false, true] {
                            let state = PipelineState {
                                viewport: (0, 0, 8, 8),
                                blend: blend_on,
                                blend_src: BlendFactor::SrcAlpha,
                                blend_dst: BlendFactor::OneMinusSrcAlpha,
                                ..Default::default()
                            };
                            let vertices =
                                [[-1., -1., 0., 1.], [1., -1., 0., 1.], [0., 1., 0., 1.]].map(
                                    |pos| Vertex {
                                        pos,
                                        color: [0.61, 0.39, 0.23, alpha],
                                        texcoord: [[0.37, 0.63, 0., 1.]; 6],
                                        ..Default::default()
                                    },
                                );
                            let stages = if textured {
                                std::slice::from_ref(&stage)
                            } else {
                                &[]
                            };
                            for gpu in [&mut fast, &mut reference] {
                                gpu.clear(
                                    8,
                                    8,
                                    &state,
                                    [0.13, 0.27, 0.41, 1.],
                                    1.,
                                    GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
                                )
                                .unwrap();
                            }
                            for _ in 0..10 {
                                fast.draw(
                                    &a,
                                    &state,
                                    GL_TRIANGLES,
                                    &vertices,
                                    stages,
                                    &textures,
                                    [1, 0, 0, 0, 0, 0],
                                )
                                .unwrap();
                                reference
                                    .draw(
                                        &b,
                                        &state,
                                        GL_TRIANGLES,
                                        &vertices,
                                        stages,
                                        &textures,
                                        [1, 0, 0, 0, 0, 0],
                                    )
                                    .unwrap();
                            }
                            fast.read(&mut a).unwrap();
                            reference.read(&mut b).unwrap();
                            assert!(fast.simple_active);
                            assert_eq!(a.color,b.color,"ES={es} textured={textured} alpha={alpha} blend={blend_on} filter={filter:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "requires native OpenGL/GLES; shader fallback and mixed transitions"]
    fn simple_fragment_restores_complex_states_and_avoids_mixed_program_churn() {
        for es in [false, true] {
            let make = || {
                let mut g = if es {
                    Renderer::new_es_for_test(8, 8).unwrap()
                } else {
                    Renderer::new(8, 8).unwrap()
                };
                g.set_surface_rgb565(true).unwrap();
                g
            };
            let (mut fast, mut reference) = (make(), make());
            reference.simple_available = false;
            let (mut a, mut b) = (
                RenderTarget::with_format(8, 8, true),
                RenderTarget::with_format(8, 8, true),
            );
            let base = PipelineState {
                viewport: (0, 0, 8, 8),
                ..Default::default()
            };
            let vertices =
                [[-1., -1., 0., 1.], [1., -1., 0., 1.], [0., 1., 0., 1.]].map(|pos| Vertex {
                    pos,
                    color: [0.61, 0.39, 0.23, 0.7],
                    ..Default::default()
                });
            let mut t = Texture::default();
            t.set_level(
                0,
                2,
                2,
                vec![
                    240, 120, 200, 255, 32, 120, 200, 255, 60, 220, 30, 255, 128, 40, 231, 255,
                ],
            );
            t.min_filter = Filter::Nearest;
            t.mag_filter = Filter::Nearest;
            let mut textures = HashMap::from([(1, t)]);
            let stage = TextureStage {
                unit: 0,
                env: TexEnvMode::Modulate,
                env_color: [0.; 4],
                lod_bias: 0.,
            };
            let mut program = 0;
            for i in 0..24 {
                let stages = if i % 2 == 0 {
                    std::slice::from_ref(&stage)
                } else {
                    &[]
                };
                fast.draw(
                    &a,
                    &base,
                    GL_TRIANGLES,
                    &vertices,
                    stages,
                    &textures,
                    [1; 6],
                )
                .unwrap();
                reference
                    .draw(
                        &b,
                        &base,
                        GL_TRIANGLES,
                        &vertices,
                        stages,
                        &textures,
                        [1; 6],
                    )
                    .unwrap();
                if i >= 7 {
                    assert!(fast.simple_active);
                    if program == 0 {
                        program = fast.program;
                    }
                    assert_eq!(program, fast.program);
                }
            }
            fast.read(&mut a).unwrap();
            reference.read(&mut b).unwrap();
            assert_eq!(a.color, b.color);
            // Every complex draw must immediately restore the complete shader.
            for feature in 0..12 {
                let mut state = base.clone();
                let mut stages = vec![stage.clone()];
                let mut mode = GL_TRIANGLES;
                let mut coordinates = vertices;
                match feature {
                    0 => {
                        state.fog = true;
                        state.fog_mode = FogMode::Linear;
                        state.fog_end = 2.;
                    }
                    1 => {
                        state.alpha_test = true;
                        state.alpha_ref = 0.8;
                    }
                    2 => {
                        state.logic_op = true;
                        state.logic_mode = 0x1506;
                    }
                    3 => mode = GL_POINTS,
                    4 => state.two_sided_lighting = true,
                    5 => stages[0].env = TexEnvMode::Replace,
                    6 => stages.push(TextureStage {
                        unit: 1,
                        ..stage.clone()
                    }),
                    7 => stages[0].unit = 1,
                    8 => stages[0].lod_bias = 1.,
                    9 => {
                        textures.get_mut(&1).unwrap().min_lod = 1.;
                    }
                    10 => {
                        for v in &mut coordinates {
                            v.texcoord[0] = [0.5, 0.5, 0., 2.];
                        }
                    }
                    _ => {
                        let t = textures.get_mut(&1).unwrap();
                        t.set_level(1, 1, 1, vec![0, 255, 0, 255]);
                        t.min_filter = Filter::LinearMipmapLinear;
                    }
                }
                fast.draw(&a, &state, mode, &coordinates, &stages, &textures, [1; 6])
                    .unwrap();
                reference
                    .draw(&b, &state, mode, &coordinates, &stages, &textures, [1; 6])
                    .unwrap();
                assert!(!fast.simple_active, "feature {feature}");
                fast.read(&mut a).unwrap();
                reference.read(&mut b).unwrap();
                assert_eq!(a.color, b.color, "feature {feature} ES={es}");
                textures.get_mut(&1).unwrap().min_lod = -1000.;
                textures.get_mut(&1).unwrap().min_filter = Filter::Nearest;
                for _ in 0..8 {
                    fast.draw(
                        &a,
                        &base,
                        GL_TRIANGLES,
                        &vertices,
                        &[stage.clone()],
                        &textures,
                        [1; 6],
                    )
                    .unwrap();
                    reference
                        .draw(
                            &b,
                            &base,
                            GL_TRIANGLES,
                            &vertices,
                            &[stage.clone()],
                            &textures,
                            [1; 6],
                        )
                        .unwrap();
                }
                assert!(fast.simple_active);
            }
            // Isolated eligible draws cannot enable the optimized program.
            for i in 0..16 {
                let state = PipelineState {
                    fog: i % 2 == 0,
                    ..base.clone()
                };
                fast.draw(&a, &state, GL_TRIANGLES, &vertices, &[], &textures, [1; 6])
                    .unwrap();
                assert!(!fast.simple_active);
            }
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native GL/GLES; shared and specialized shader color precision"]
    fn untextured_shader_preserves_rgba_and_rgb565_color_precision() {
        for es in [false, true] {
            for rgb565 in [false, true] {
                let mut gpu = if es {
                    Renderer::with_host(Host::es_for_test().unwrap(), 8, 8).unwrap()
                } else {
                    Renderer::new(8, 8).unwrap()
                };
                gpu.set_surface_rgb565(rgb565).unwrap();
                let mut target = RenderTarget::with_format(8, 8, rgb565);
                let state = PipelineState {
                    viewport: (0, 0, 8, 8),
                    ..Default::default()
                };
                let triangle =
                    [[-1., -1., 0., 1.], [1., -1., 0., 1.], [0., 1., 0., 1.]].map(|pos| Vertex {
                        pos,
                        color: [0.61, 0.39, 0.23, 0.7],
                        ..Default::default()
                    });
                let vertices = triangle.repeat(64);
                let textures = HashMap::new();
                let at = (4 * 8 + 4) * 4;
                gpu.draw(
                    &target,
                    &state,
                    GL_TRIANGLES,
                    &triangle,
                    &[],
                    &textures,
                    [0; 6],
                )
                .unwrap();
                gpu.read(&mut target).unwrap();
                assert!(!gpu.untextured_active);
                let reference = target.color[at..at + 4].to_vec();
                gpu.draw(
                    &target,
                    &state,
                    GL_TRIANGLES,
                    &vertices,
                    &[],
                    &textures,
                    [0; 6],
                )
                .unwrap();
                gpu.read(&mut target).unwrap();
                assert_eq!(gpu.untextured_active, rgb565);
                assert_eq!(
                    &target.color[at..at + 4],
                    reference.as_slice(),
                    "es={es} rgb565={rgb565}"
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native GL/GLES; untextured shader and live matrix/cache transitions"]
    fn untextured_draws_skip_texture_work_and_restore_other_shader_families() {
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::with_host(Host::es_for_test().unwrap(), 8, 8).unwrap()
            } else {
                Renderer::new(8, 8).unwrap()
            };
            gpu.set_surface_rgb565(true).unwrap();
            let mut target = RenderTarget::with_format(8, 8, true);
            let mut state = PipelineState {
                viewport: (0, 0, 8, 8),
                ..Default::default()
            };
            let vertices =
                [[-1., -1., 0., 1.], [1., -1., 0., 1.], [0., 1., 0., 1.]].map(|pos| Vertex {
                    pos,
                    color: [1., 0., 0., 1.],
                    texcoord: [[0.125, 0.125, 0., 1.]; 6],
                    ..Default::default()
                });
            let mut vertices = vertices.repeat(64);
            let mut transforms = VertexTransforms {
                projection: IDENTITY,
                modelview: IDENTITY,
                normal_matrix: IDENTITY,
                textures: [IDENTITY; 6],
                lighting: Default::default(),
            };
            let mut tex = Texture::default();
            tex.min_filter = Filter::Nearest;
            tex.mag_filter = Filter::Nearest;
            let pixels = (0..16)
                .flat_map(|i| {
                    if i % 4 < 2 {
                        [255u8, 0, 0, 255]
                    } else {
                        [0, 255, 0, 255]
                    }
                })
                .collect();
            tex.set_level(0, 4, 4, pixels);
            let textures = HashMap::from([(1, tex)]);
            let stages = [TextureStage {
                unit: 0,
                env: TexEnvMode::Replace,
                env_color: [0.; 4],
                lod_bias: 0.,
            }];
            let at = (4 * 8 + 4) * 4;
            gpu.draw_object_space(
                &target,
                &state,
                GL_TRIANGLES,
                &vertices[..3],
                &[],
                &textures,
                [1, 0, 0, 0, 0, 0],
                &transforms,
            )
            .unwrap();
            assert!(!gpu.untextured_active);
            gpu.draw_object_space(
                &target,
                &state,
                GL_TRIANGLES,
                &vertices,
                &[],
                &textures,
                [1, 0, 0, 0, 0, 0],
                &transforms,
            )
            .unwrap();
            gpu.read(&mut target).unwrap();
            assert!(gpu.untextured_active);
            let program = gpu.program;
            assert_eq!(&target.color[at..at + 4], &[255, 0, 0, 255]);
            for name in [
                "textureMatrix0",
                "textureMatrix1",
                "textureMatrix2",
                "textureMatrix3",
                "sampler0",
                "env0",
            ] {
                assert_eq!(
                    gpu.loc(name),
                    -1,
                    "unused texture work must be removed by the shader linker: {name}"
                );
            }
            gpu.draw_object_space(
                &target,
                &state,
                GL_TRIANGLES,
                &vertices,
                &stages,
                &textures,
                [1, 0, 0, 0, 0, 0],
                &transforms,
            )
            .unwrap();
            gpu.read(&mut target).unwrap();
            assert!(!gpu.untextured_active);
            assert_eq!(&target.color[at..at + 4], &[255, 0, 0, 255]);
            gpu.draw_object_space(
                &target,
                &state,
                GL_TRIANGLES,
                &vertices[..3],
                &[],
                &textures,
                [1, 0, 0, 0, 0, 0],
                &transforms,
            )
            .unwrap();
            assert!(!gpu.untextured_active);
            for v in &mut vertices {
                v.color = [0., 0., 1., 1.];
            }
            transforms.textures[0] = crate::matrix::translate(0.5, 0., 0.);
            gpu.draw_object_space(
                &target,
                &state,
                GL_TRIANGLES,
                &vertices,
                &[],
                &textures,
                [1, 0, 0, 0, 0, 0],
                &transforms,
            )
            .unwrap();
            gpu.read(&mut target).unwrap();
            assert_eq!(gpu.program, program);
            assert_eq!(&target.color[at..at + 4], &[0, 0, 255, 255]);
            gpu.draw_object_space(
                &target,
                &state,
                GL_TRIANGLES,
                &vertices,
                &stages,
                &textures,
                [1, 0, 0, 0, 0, 0],
                &transforms,
            )
            .unwrap();
            gpu.read(&mut target).unwrap();
            assert!(!gpu.untextured_active);
            assert_eq!(
                &target.color[at..at + 4],
                &[0, 255, 0, 255],
                "texture matrix updates while untextured must survive switching"
            );
            state.blend = true;
            gpu.draw_object_space(
                &target,
                &state,
                GL_TRIANGLES,
                &vertices,
                &[],
                &textures,
                [1, 0, 0, 0, 0, 0],
                &transforms,
            )
            .unwrap();
            assert!(!gpu.untextured_active);
            state.blend = false;
            state.alpha_test = true;
            gpu.draw_object_space(
                &target,
                &state,
                GL_TRIANGLES,
                &vertices,
                &[],
                &textures,
                [1, 0, 0, 0, 0, 0],
                &transforms,
            )
            .unwrap();
            assert!(!gpu.untextured_active);
            state.alpha_test = false;
            state.logic_op = true;
            state.logic_mode = crate::GL_COPY;
            gpu.draw_object_space(
                &target,
                &state,
                GL_TRIANGLES,
                &vertices,
                &[],
                &textures,
                [1, 0, 0, 0, 0, 0],
                &transforms,
            )
            .unwrap();
            gpu.read(&mut target).unwrap();
            assert!(!gpu.untextured_active);
            assert_eq!(&target.color[at..at + 4], &[0, 0, 255, 255]);
            state.logic_op = false;
            gpu.draw_object_space(
                &target,
                &state,
                GL_POINTS,
                &vertices[..1],
                &[],
                &textures,
                [1, 0, 0, 0, 0, 0],
                &transforms,
            )
            .unwrap();
            assert!(!gpu.untextured_active);
            gpu.draw_object_space(
                &target,
                &state,
                GL_TRIANGLES,
                &vertices,
                &[],
                &textures,
                [1, 0, 0, 0, 0, 0],
                &transforms,
            )
            .unwrap();
            assert!(gpu.untextured_active);
            assert_eq!(gpu.program, program);
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native GL/GLES; DXT5 native uploads and forced decoded fallback"]
    fn dxt5_native_and_decoded_routes_preserve_pixels_and_alpha() {
        for es in [false, true] {
            for fallback in [false, true] {
                let mut gpu = if es {
                    Renderer::with_host(Host::es_for_test().unwrap(), 8, 8).unwrap()
                } else {
                    Renderer::new(8, 8).unwrap()
                };
                let fmt = GL_COMPRESSED_RGBA_S3TC_DXT5_EXT;
                if fallback {
                    gpu.compressed_formats.remove(&fmt);
                }
                let mut tex = Texture::default();
                tex.min_filter = Filter::Nearest;
                tex.mag_filter = Filter::Nearest;
                let mut block = [0u8; 16];
                block[10..12].copy_from_slice(&0xffffu16.to_le_bytes());
                block[12..16].fill(0xff);
                let mut textures = HashMap::new();
                let stages = [TextureStage {
                    unit: 0,
                    env: TexEnvMode::Replace,
                    env_color: [0.; 4],
                    lod_bias: 0.,
                }];
                let vertices =
                    [[-1., -1., 0., 1.], [1., -1., 0., 1.], [0., 1., 0., 1.]].map(|pos| Vertex {
                        pos,
                        color: [1.; 4],
                        texcoord: [[0.125, 0.125, 0., 1.]; 6],
                        ..Default::default()
                    });
                let mut target = RenderTarget::new(8, 8);
                let state = PipelineState {
                    viewport: (0, 0, 8, 8),
                    ..Default::default()
                };
                for alpha in [0u8, 90, 255] {
                    block[0] = alpha;
                    tex.set_compressed(0, 4, 4, fmt, &block, Vec::new());
                    textures.insert(1, tex.clone());
                    gpu.draw(
                        &target,
                        &state,
                        GL_TRIANGLES,
                        &vertices,
                        &stages,
                        &textures,
                        [1, 0, 0, 0, 0, 0],
                    )
                    .unwrap();
                    gpu.read(&mut target).unwrap();
                    let at = (4 * 8 + 4) * 4;
                    for (got, want) in target.color[at..at + 4].iter().zip([170, 170, 170, alpha]) {
                        assert!(
                            got.abs_diff(want) <= 1,
                            "es={es} fallback={fallback} got={:?}",
                            &target.color[at..at + 4]
                        );
                    }
                }
                gpu.host.make_current().unwrap();
                unsafe {
                    assert_eq!((gpu.gl.GetError)(), 0);
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native desktop GL and GLES2; upload routes and driver fallbacks"]
    fn host_upload_routes_and_fallbacks() {
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::with_host(Host::es_for_test().unwrap(), 8, 8).unwrap()
            } else {
                Renderer::new(8, 8).unwrap()
            };
            gpu.host.make_current().unwrap();
            unsafe {
                let mut name = 0;
                (gpu.gl.GenTextures)(1, &mut name);
                (gpu.gl.BindTexture)(GL_TEXTURE_2D, name);
                let mut tex = Texture::default();
                tex.set_level(0, 4, 4, [255, 0, 0, 255].repeat(16));
                assert!(!tex.is_complete());
                assert!(upload_texture(
                    &gpu.gl,
                    &gpu.compressed_formats,
                    &tex,
                    true,
                    gpu.native_max_level
                )
                .is_err());
                tex.set_parameter(0x8191, 1);
                tex.set_level(0, 4, 4, [255, 0, 0, 255].repeat(16));
                assert!(tex.is_complete());
                assert!(matches!(
                    upload_texture(
                        &gpu.gl,
                        &gpu.compressed_formats,
                        &tex,
                        true,
                        gpu.native_max_level
                    )
                    .unwrap(),
                    UploadRoute::Rgba
                ));
                tex.set_level(1, 2, 2, [0, 255, 0, 255].repeat(4));
                assert!(matches!(
                    upload_texture(
                        &gpu.gl,
                        &gpu.compressed_formats,
                        &tex,
                        true,
                        gpu.native_max_level
                    )
                    .unwrap(),
                    UploadRoute::Rgba
                ));
                let bytes = [0x00, 0xf8, 0x00, 0xf8, 0, 0, 0, 0];
                tex = Texture::default();
                tex.min_filter = Filter::Linear;
                tex.set_compressed(
                    0,
                    4,
                    4,
                    GL_COMPRESSED_RGBA_S3TC_DXT1_EXT,
                    &bytes,
                    Vec::new(),
                );
                assert!(tex.rgba.is_empty());
                assert_eq!(&*tex.rgba_level(0).unwrap(), &[255, 0, 0, 255].repeat(16));
                if gpu.supports_compressed(GL_COMPRESSED_RGBA_S3TC_DXT1_EXT) {
                    assert!(matches!(
                        upload_texture(
                            &gpu.gl,
                            &gpu.compressed_formats,
                            &tex,
                            false,
                            gpu.native_max_level
                        )
                        .unwrap(),
                        UploadRoute::Compressed
                    ));
                }
                assert!(matches!(
                    upload_texture(&gpu.gl, &HashSet::new(), &tex, false, gpu.native_max_level)
                        .unwrap(),
                    UploadRoute::Rgba
                ));
                let upload = gpu.gl.CompressedTexImage2D.take();
                assert!(matches!(
                    upload_texture(
                        &gpu.gl,
                        &gpu.compressed_formats,
                        &tex,
                        false,
                        gpu.native_max_level
                    )
                    .unwrap(),
                    UploadRoute::Rgba
                ));
                gpu.gl.CompressedTexImage2D = upload;
                // Force an advertised format the driver does not actually accept.
                // A refused upload must fall back without leaving a GL error.
                if !gpu.supports_compressed(GL_ATC_RGB_AMD) {
                    let mut refused = Texture::default();
                    refused.min_filter = Filter::Linear;
                    refused.set_compressed(0, 4, 4, GL_ATC_RGB_AMD, &bytes, Vec::new());
                    let formats = HashSet::from([GL_ATC_RGB_AMD]);
                    assert!(matches!(
                        upload_texture(&gpu.gl, &formats, &refused, false, gpu.native_max_level)
                            .unwrap(),
                        UploadRoute::Rgba
                    ));
                    assert_eq!((gpu.gl.GetError)(), 0);
                }
                tex.ensure_rgba();
                assert_eq!(tex.rgba, [255, 0, 0, 255].repeat(16));
                tex.set_level(0, 4, 4, [0, 0, 255, 255].repeat(16));
                assert!(tex.compressed.is_empty());
                assert_eq!((gpu.gl.GetError)(), 0);
                (gpu.gl.DeleteTextures)(1, &name);
            }
        }
    }
    #[test]
    fn packed_vertex_keys_keep_additional_coordinates_distinct() {
        let mut source = vec![0.; 40];
        let mut second = source.clone();
        second[35] = 1.;
        source.extend(&second);
        source.extend([0.; 40]);
        let mut keys = HashMap::<[u32; 48], u16>::new();
        let mut packed = Vec::new();
        let mut indices = Vec::new();
        assert!(unique_records(
            &source,
            40,
            &mut keys,
            &mut packed,
            &mut indices
        ));
        assert_eq!(indices, [0, 1, 0]);
        assert_eq!(keys.len(), 2);
        assert_eq!(packed.len(), 80);
        assert_eq!(packed[75], 1.);
    }

    #[cfg(unix)]

    fn identity_transforms() -> VertexTransforms {
        VertexTransforms {
            lighting: Default::default(),
            normal_matrix: IDENTITY,
            projection: IDENTITY,
            modelview: IDENTITY,
            textures: [IDENTITY; 6],
        }
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires native desktop GL and GLES2; mip/LOD and direct native indices"]
    fn host_mips_lod_bias_and_direct_guest_indices() {
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::with_host(Host::es_for_test().unwrap(), 32, 32).unwrap()
            } else {
                Renderer::new(32, 32).unwrap()
            };
            let mut target = RenderTarget::new(32, 32);
            let state = PipelineState {
                viewport: (0, 0, 32, 32),
                ..Default::default()
            };
            let positions = [
                [-1., -1., 0., 1.],
                [1., -1., 0., 1.],
                [-1., 1., 0., 1.],
                [1., 1., 0., 1.],
            ];
            let uv = [[0., 0.], [1., 0.], [0., 1.], [1., 1.]];
            let vertices: Vec<_> = positions
                .iter()
                .zip(uv)
                .map(|(p, t)| {
                    let mut v = triangle([1.; 4], 0.)[0];
                    v.pos = *p;
                    v.texcoord[0] = [t[0], t[1], 0., 1.];
                    v
                })
                .collect();
            let mut texture = Texture::default();
            texture.set_level(0, 64, 64, [255, 0, 0, 255].repeat(64 * 64));
            texture.set_level(1, 32, 32, [0, 255, 0, 255].repeat(32 * 32));
            texture.set_level(2, 16, 16, [0, 0, 255, 255].repeat(16 * 16));
            for level in 3..=6 {
                let side = 64 >> level;
                texture.set_level(
                    level,
                    side,
                    side,
                    [0, 0, 255, 255].repeat((side * side) as usize),
                );
            }
            texture.set_parameter(GL_TEXTURE_MAX_LEVEL_SGIS, 2);
            texture.mipmaps.retain(|level, _| *level <= 2);
            texture.formats.retain(|level, _| *level <= 2);
            texture.min_filter = Filter::NearestMipmapNearest;
            texture.mag_filter = Filter::Nearest;
            let mut textures = HashMap::from([(1, texture)]);
            let mut stage = TextureStage {
                unit: 0,
                env: TexEnvMode::Replace,
                env_color: [0.; 4],
                lod_bias: 0.,
            };
            for (bias, expected) in [
                (0., [0, 255, 0, 255]),
                (1., [0, 0, 255, 255]),
                (-1., [255, 0, 0, 255]),
            ] {
                stage.lod_bias = bias;
                gpu.draw_indexed_object_space(
                    &target,
                    &state,
                    GL_TRIANGLE_STRIP,
                    &vertices,
                    &[0, 1, 2, 3],
                    &[stage],
                    &textures,
                    [1, 0, 0, 0, 0, 0],
                    &identity_transforms(),
                )
                .unwrap();
                assert_eq!(gpu.scratch.len(), 4 * 10);
                assert_eq!(gpu.indices, [0, 1, 2, 3]);
                gpu.read(&mut target).unwrap();
                assert_eq!(
                    &target.color[(16 * 32 + 16) * 4..(16 * 32 + 16) * 4 + 4],
                    &expected,
                    "es={es} bias={bias}"
                );
            }
            // Native max-level and GLES2 derivative fallback must cap the
            // selected authored level, and permit it again when the cap grows.
            // The earlier checks used only the three SGIS-required images.
            // Supply the rest before testing a restored unlimited maximum.
            assert_eq!(textures[&1].mipmaps.len(), 2);
            for level in 3..=6 {
                let side = 64 >> level;
                textures.get_mut(&1).unwrap().set_level(
                    level,
                    side,
                    side,
                    [0, 0, 255, 255].repeat((side * side) as usize),
                );
            }
            let native_max_level = gpu.native_max_level;
            for native_cap in [true, false] {
                gpu.native_max_level = native_cap;
                if !native_cap {
                    let id = gpu.textures[&1].0;
                    unsafe {
                        (gpu.gl.ActiveTexture)(GL_TEXTURE0);
                        (gpu.gl.BindTexture)(GL_TEXTURE_2D, id);
                        (gpu.gl.TexParameteri)(GL_TEXTURE_2D, GL_TEXTURE_MAX_LEVEL_SGIS, 1000);
                    }
                }
                for min_filter in [Filter::NearestMipmapNearest, Filter::LinearMipmapLinear] {
                    for (cap, expected) in [
                        (0, [255, 0, 0, 255]),
                        (1, [0, 255, 0, 255]),
                        (1000, [0, 0, 255, 255]),
                        (1, [0, 255, 0, 255]),
                    ] {
                        let texture = textures.get_mut(&1).unwrap();
                        texture.min_filter = min_filter;
                        texture.set_parameter(GL_TEXTURE_MAX_LEVEL_SGIS, cap);
                        stage.lod_bias = 1.;
                        gpu.draw_indexed_object_space(
                            &target,
                            &state,
                            GL_TRIANGLE_STRIP,
                            &vertices,
                            &[0, 1, 2, 3],
                            &[stage],
                            &textures,
                            [1, 0, 0, 0, 0, 0],
                            &identity_transforms(),
                        )
                        .unwrap();
                        gpu.read(&mut target).unwrap();
                        let i = (16 * 32 + 16) * 4;
                        for (actual, expected) in target.color[i..i + 4].iter().zip(expected) {
                            assert!(
                                actual.abs_diff(expected) <= 1,
                                "es={es} native={native_cap} cap={cap} filter={min_filter:?}"
                            );
                        }
                    }
                }
            }
            // Old ES2 without derivatives retains base filtering rather than
            // sampling beyond a cap it cannot represent.
            let derivatives = gpu.lod_derivatives;
            gpu.lod_derivatives = false;
            gpu.draw_indexed_object_space(
                &target,
                &state,
                GL_TRIANGLE_STRIP,
                &vertices,
                &[0, 1, 2, 3],
                &[stage],
                &textures,
                [1, 0, 0, 0, 0, 0],
                &identity_transforms(),
            )
            .unwrap();
            gpu.read(&mut target).unwrap();
            let i = (16 * 32 + 16) * 4;
            assert_eq!(&target.color[i..i + 4], &[255, 0, 0, 255]);
            gpu.lod_derivatives = derivatives;
            gpu.native_max_level = native_max_level;
            textures.get_mut(&1).unwrap().min_filter = Filter::NearestMipmapNearest;
            textures
                .get_mut(&1)
                .unwrap()
                .set_parameter(GL_TEXTURE_MAX_LEVEL_SGIS, 1000);
            textures
                .get_mut(&1)
                .unwrap()
                .set_level(1, 32, 32, [255, 255, 0, 255].repeat(32 * 32));
            stage.lod_bias = 0.;
            gpu.draw_indexed_object_space(
                &target,
                &state,
                GL_TRIANGLE_STRIP,
                &vertices,
                &[0, 1, 2, 3],
                &[stage],
                &textures,
                [1, 0, 0, 0, 0, 0],
                &identity_transforms(),
            )
            .unwrap();
            gpu.read(&mut target).unwrap();
            assert_eq!(
                &target.color[(16 * 32 + 16) * 4..(16 * 32 + 16) * 4 + 4],
                &[255, 255, 0, 255]
            );
            let tex = textures.get_mut(&1).unwrap();
            tex.min_filter = Filter::Nearest;
            tex.set_level(1, 32, 32, [0, 255, 255, 255].repeat(32 * 32));
            gpu.draw_indexed_object_space(
                &target,
                &state,
                GL_TRIANGLE_STRIP,
                &vertices,
                &[0, 1, 2, 3],
                &[stage],
                &textures,
                [1, 0, 0, 0, 0, 0],
                &identity_transforms(),
            )
            .unwrap();
            gpu.read(&mut target).unwrap();
            assert_eq!(
                &target.color[(16 * 32 + 16) * 4..(16 * 32 + 16) * 4 + 4],
                &[255, 0, 0, 255]
            );
            textures.get_mut(&1).unwrap().min_filter = Filter::NearestMipmapNearest;
            gpu.draw_indexed_object_space(
                &target,
                &state,
                GL_TRIANGLE_STRIP,
                &vertices,
                &[0, 1, 2, 3],
                &[stage],
                &textures,
                [1, 0, 0, 0, 0, 0],
                &identity_transforms(),
            )
            .unwrap();
            gpu.read(&mut target).unwrap();
            assert_eq!(
                &target.color[(16 * 32 + 16) * 4..(16 * 32 + 16) * 4 + 4],
                &[0, 255, 255, 255]
            );
            if es {
                gpu.full_npot = false; // Exercise core-ES2 fallback even on a capable Mesa driver.
                                       // This host-fallback test intentionally supplies only the base.
                textures.get_mut(&1).unwrap().min_filter = Filter::Nearest;
                textures
                    .get_mut(&1)
                    .unwrap()
                    .set_level(0, 3, 5, [255, 0, 0, 255].repeat(15));
                gpu.draw_indexed_object_space(
                    &target,
                    &state,
                    GL_TRIANGLE_STRIP,
                    &vertices,
                    &[0, 1, 2, 3],
                    &[stage],
                    &textures,
                    [1, 0, 0, 0, 0, 0],
                    &identity_transforms(),
                )
                .unwrap();
                let id = gpu.textures[&1].0;
                assert_eq!(
                    gpu.texture_params[&id],
                    (
                        Filter::Nearest,
                        Filter::Nearest,
                        Wrap::ClampToEdge,
                        Wrap::ClampToEdge
                    )
                );
                gpu.read(&mut target).unwrap();
                assert_eq!(
                    &target.color[(16 * 32 + 16) * 4..(16 * 32 + 16) * 4 + 4],
                    &[255, 0, 0, 255]
                );
            }
            assert!(gpu
                .draw_indexed_object_space(
                    &target,
                    &state,
                    GL_TRIANGLES,
                    &vertices,
                    &[9],
                    &[],
                    &textures,
                    [0; 6],
                    &identity_transforms()
                )
                .is_err());
        }
    }

    fn typed_flat_source() -> VertexStream {
        let mut stream = VertexStream::default();
        stream.count = 4;
        for i in 0..4 {
            for v in [i as f32, 1., 2.] {
                stream.bytes.extend(v.to_le_bytes());
            }
        }
        stream.attributes[0] = Some(StreamAttribute {
            size: 3,
            ty: GL_FLOAT,
            normalized: 0,
            stride: 12,
            offset: 0,
        });
        for i in 0..4 {
            stream.bytes.extend([i, 10 + i, 20 + i, 255]);
        }
        stream.attributes[1] = Some(StreamAttribute {
            size: 4,
            ty: GL_UNSIGNED_BYTE,
            normalized: 1,
            stride: 4,
            offset: 48,
        });
        for i in 0..4i16 {
            for v in [i, -i] {
                stream.bytes.extend(v.to_le_bytes());
            }
        }
        stream.attributes[2] = Some(StreamAttribute {
            size: 2,
            ty: GL_SHORT,
            normalized: 0,
            stride: 4,
            offset: 64,
        });
        for i in 0..4 {
            stream.bytes.extend([i, 0, 127]);
        }
        stream.attributes[6] = Some(StreamAttribute {
            size: 3,
            ty: GL_BYTE,
            normalized: 1,
            stride: 3,
            offset: 80,
        });
        stream.attributes[7] = stream.attributes[0];
        stream.indices = vec![0, 1, 2, 2, 1, 3];
        stream
    }

    #[test]
    fn flat_stream_preserves_typed_geometry_uv_and_provoking_lighting_data() {
        let source = typed_flat_source();
        let mut out = VertexStream::default();
        assert!(source.expand_flat_triangles(&mut out));
        assert!(out.flat_triangles);
        assert!(out.indices.is_empty());
        assert_eq!(out.count, 6);
        assert_eq!(out.bytes.len(), 216);
        for (corner, &index) in source.indices.iter().enumerate() {
            let provoking = source.indices[corner / 3 * 3 + 2] as usize;
            for slot in [0, 1, 2, 6, 7] {
                let before = source.attributes[slot].unwrap();
                let after = out.attributes[slot].unwrap();
                assert_eq!(
                    (before.ty, before.size, before.normalized),
                    (after.ty, after.size, after.normalized)
                );
                let index = if matches!(slot, 1 | 6 | 7) {
                    provoking
                } else {
                    index as usize
                };
                let width = match before.ty {
                    GL_BYTE | GL_UNSIGNED_BYTE => 1,
                    GL_SHORT => 2,
                    _ => 4,
                };
                let len = before.size as usize * width;
                let start = before.offset + index * before.stride as usize;
                let dst = after.offset + corner * after.stride as usize;
                assert_eq!(
                    &out.bytes[dst..dst + len],
                    &source.bytes[start..start + len]
                );
            }
        }
    }
    #[test]
    fn flat_stream_keeps_constant_attributes_and_ignores_incomplete_triangle() {
        let mut source = typed_flat_source();
        source.indices = vec![0, 1, 2, 3];
        source.attributes[1] = None;
        source.attributes[6] = None;
        source.attributes[7] = None;
        source.constants[1] = [0.25, 0.5, 0.75, 1.];
        let mut out = VertexStream::default();
        assert!(source.expand_flat_triangles(&mut out));
        assert_eq!(out.count, 3);
        assert_eq!(out.constants, source.constants);
        assert!(out.attributes[1].is_none());
        assert!(out.attributes[6].is_none());
        source.indices.clear();
        assert!(source.expand_flat_triangles(&mut out));
        assert_eq!(out.count, 3);
    }
    #[test]
    fn flat_stream_rejects_invalid_snapshots_and_overlarge_expansion() {
        let source = typed_flat_source();
        let mut out = VertexStream::default();
        assert!(source.expand_flat_triangles(&mut out));
        let mut bad = typed_flat_source();
        bad.indices[0] = 99;
        assert!(!bad.expand_flat_triangles(&mut out));
        assert!(!out.flat_triangles);
        assert!(out.bytes.is_empty());
        bad = typed_flat_source();
        bad.bytes.truncate(80);
        assert!(!bad.expand_flat_triangles(&mut out));
        bad = typed_flat_source();
        bad.attributes[2].as_mut().unwrap().stride = 0;
        assert!(!bad.expand_flat_triangles(&mut out));
        bad = typed_flat_source();
        bad.indices = vec![0; 65538];
        assert!(!bad.expand_flat_triangles(&mut out));
        bad = typed_flat_source();
        bad.flat_triangles = true;
        assert!(!bad.expand_flat_triangles(&mut out));
    }

    #[test]
    fn flat_direct_submission_excludes_smooth_shared_topologies_and_native_indices() {
        assert!(direct_flat_triangles(GL_TRIANGLES, false, false));
        assert!(!direct_flat_triangles(GL_TRIANGLES, true, false));
        assert!(!direct_flat_triangles(GL_TRIANGLES, false, true));
        for mode in [
            GL_TRIANGLE_STRIP,
            GL_TRIANGLE_FAN,
            GL_LINES,
            GL_LINE_STRIP,
            GL_LINE_LOOP,
            GL_POINTS,
        ] {
            assert!(!direct_flat_triangles(mode, false, false));
        }
    }

    #[test]
    fn affine_shader_selection_requires_a_proof_for_every_texture_unit() {
        let stages = [TextureStage {
            unit: 3,
            env: TexEnvMode::Replace,
            env_color: [0.; 4],
            lod_bias: 0.,
        }];
        let mut vertices = triangle([1.; 4], 0.);
        let mut transforms = identity_transforms();
        assert!(affine_texture_coordinates(
            &vertices,
            &stages,
            Some(&transforms),
            None
        ));
        vertices[1].texcoord[3][3] = 2.;
        assert!(!affine_texture_coordinates(&vertices, &stages, None, None));
        assert!(affine_texture_coordinates(&vertices, &[], None, None));
        vertices[1].texcoord[3][3] = 1.;
        for index in [3, 7, 11, 15] {
            transforms.textures[3][index] += 1.;
            assert!(!affine_texture_coordinates(
                &vertices,
                &stages,
                Some(&transforms),
                None
            ));
            transforms.textures[3] = IDENTITY;
        }
        let mut stream = VertexStream::default();
        stream.constants[5] = [0., 0., 0., 1.];
        assert!(affine_texture_coordinates(
            &vertices,
            &stages,
            None,
            Some(&stream)
        ));
        stream.constants[5][3] = 0.;
        assert!(!affine_texture_coordinates(
            &vertices,
            &stages,
            None,
            Some(&stream)
        ));
        for size in [2, 3, 4] {
            stream.attributes[5] = Some(StreamAttribute {
                size,
                ty: GL_FLOAT,
                normalized: 0,
                stride: 0,
                offset: 0,
            });
            assert_eq!(
                affine_texture_coordinates(&vertices, &stages, None, Some(&stream)),
                size < 4
            );
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native desktop GL and GLES2; affine/general shader switching"]
    fn host_affine_shader_matches_general_and_preserves_program_uniform_caches() {
        for es in [false, true] {
            for rgb565 in [false, true] {
                for scale in [1, 2] {
                    let mut gpu = if es {
                        Renderer::with_host(Host::es_for_test().unwrap(), 32, 32).unwrap()
                    } else {
                        Renderer::new(32, 32).unwrap()
                    };
                    gpu.set_surface_rgb565(rgb565).unwrap();
                    gpu.set_render_scale(scale).unwrap();
                    let mut target = RenderTarget::new(32, 32);
                    let mut state = PipelineState::default();
                    state.viewport = (0, 0, 32, 32);
                    let mut texture = Texture::default();
                    texture.set_level(
                        0,
                        2,
                        2,
                        vec![
                            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
                        ],
                    );
                    texture.min_filter = Filter::Linear;
                    texture.mag_filter = Filter::Linear;
                    let textures = HashMap::from([(1, texture)]);
                    let stages = [TextureStage {
                        unit: 0,
                        env: TexEnvMode::Modulate,
                        env_color: [0.; 4],
                        lod_bias: 0.,
                    }];
                    let mut transforms = identity_transforms();
                    let mut vertices = triangle([1.; 4], 0.);
                    vertices[0].texcoord[0] = [0., 0., 0., 1.];
                    vertices[1].texcoord[0] = [1., 0., 0., 1.];
                    vertices[2].texcoord[0] = [0.5, 1., 0., 1.];
                    let mut programs = [0; 2];
                    for color in [
                        [1., 0.25, 0.5, 1.],
                        [0.2, 0.8, 0.1, 0.5],
                        [1., 0.25, 0.5, 1.],
                    ] {
                        for v in &mut vertices {
                            v.color = color;
                            v.back_color = color;
                        }
                        let mut reference = Vec::new();
                        // Leave point-only uniforms enabled in the general
                        // program, then visit affine triangles before returning.
                        let mut points = state.clone();
                        points.point_size_array = true;
                        points.point_sprite = true;
                        points.point_coord_replace[0] = true;
                        gpu.draw_object_space(
                            &target,
                            &points,
                            GL_POINTS,
                            &vertices,
                            &stages,
                            &textures,
                            [1, 0, 0, 0, 0, 0],
                            &transforms,
                        )
                        .unwrap();
                        for general in [false, true, false] {
                            // r=0 for every corner, so adding r to q changes no coordinate.
                            // The conservative selector must nevertheless use the general shader.
                            transforms.textures[0][11] = if general { 1. } else { 0. };
                            gpu.clear(
                                32,
                                32,
                                &state,
                                [0.; 4],
                                1.,
                                GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
                            )
                            .unwrap();
                            gpu.draw_object_space(
                                &target,
                                &state,
                                GL_TRIANGLES,
                                &vertices,
                                &stages,
                                &textures,
                                [1, 0, 0, 0, 0, 0],
                                &transforms,
                            )
                            .unwrap();
                            assert_eq!(gpu.affine_program, !general);
                            let slot = usize::from(general);
                            if programs[slot] == 0 {
                                programs[slot] = gpu.program;
                            }
                            assert_eq!(programs[slot], gpu.program);
                            gpu.read(&mut target).unwrap();
                            if reference.is_empty() {
                                reference = target.color.clone();
                            } else {
                                for (a, b) in reference.iter().zip(&target.color) {
                                    assert!(
                                        a.abs_diff(*b) <= 1,
                                        "es={es} rgb565={rgb565} scale={scale}"
                                    );
                                }
                            }
                        }
                    }
                    assert_ne!(programs[0], programs[1]);
                }
            }
        }
    }

    #[test]
    fn expanded_projective_attributes_keep_all_components_without_growing_affine_payloads() {
        let state = PipelineState::default();
        let stages = [TextureStage {
            unit: 0,
            env: TexEnvMode::Replace,
            env_color: [0.; 4],
            lod_bias: 0.,
        }];
        let affine = VertexLayout::new(&state, &stages, None);
        let projective = VertexLayout::with_projective_mask(&state, &stages, None, 1);
        assert_eq!(affine.attributes[2], Some((2, 8)));
        assert_eq!(affine.floats, 10);
        assert_eq!(projective.attributes[2], Some((4, 8)));
        assert_eq!(projective.floats, 12);
        let vertex = Vertex {
            texcoord: [[0.25, 0.5, 2., 3.]; 6],
            ..Default::default()
        };
        let mut packed = Vec::new();
        projective.pack(&mut packed, vertex, vertex, true);
        assert_eq!(&packed[8..12], &[0.25, 0.5, 2., 3.]);
    }

    #[test]
    fn direct_flat_triangles_preserve_all_provoking_and_interpolated_attributes() {
        let mut state = PipelineState::default();
        state.smooth_shading = false;
        let mut transforms = identity_transforms();
        transforms.lighting.enabled = true;
        let stages: Vec<_> = (0..4)
            .map(|unit| TextureStage {
                unit,
                env: TexEnvMode::Modulate,
                env_color: [0.; 4],
                lod_bias: 0.,
            })
            .collect();
        let layout = VertexLayout::new(&state, &stages, Some(&transforms));
        let vertices: Vec<_> = (0..6)
            .map(|i| Vertex {
                pos: [i as f32, 1., 2., 1.],
                color: [i as f32 / 6., 0.5, 0.25, 1.],
                normal: [i as f32, 0., 1.],
                texcoord: [[i as f32, 0.5, 0., 1.]; 6],
                fog_depth: i as f32 + 0.25,
                ..Default::default()
            })
            .collect();
        let mut packed = Vec::new();
        pack_flat_triangles(&layout, &mut packed, &vertices);
        assert_eq!(packed.len(), 6 * layout.floats);
        for (i, record) in packed.chunks_exact(layout.floats).enumerate() {
            let provoking = &vertices[i / 3 * 3 + 2];
            assert_eq!(&record[..4], &vertices[i].pos);
            assert_eq!(&record[4..8], &provoking.color);
            for unit in 0..4 {
                let (size, offset) = layout.attributes[unit + 2].unwrap();
                assert_eq!(
                    &record[offset..offset + size as usize],
                    &vertices[i].texcoord[unit][..size as usize]
                );
            }
            let (_, offset) = layout.attributes[6].unwrap();
            assert_eq!(&record[offset..offset + 3], &provoking.normal);
            assert_eq!(record[offset + 3], vertices[i].fog_depth);
            let (_, offset) = layout.attributes[7].unwrap();
            assert_eq!(&record[offset..offset + 4], &provoking.pos);
        }
    }

    #[test]
    fn direct_flat_triangles_ignore_incomplete_primitives_and_reuse_output() {
        let mut state = PipelineState::default();
        state.smooth_shading = false;
        let layout = VertexLayout::new(&state, &[], None);
        let v = Vertex::default();
        let mut out = Vec::new();
        for count in 0..10 {
            out.clear();
            pack_flat_triangles(&layout, &mut out, &vec![v; count]);
            assert_eq!(out.len(), count / 3 * 3 * layout.floats);
        }
    }

    #[test]
    fn compact_layout_keeps_only_active_attributes_and_flat_lighting_data() {
        let mut state = PipelineState::default();
        let mut transforms = identity_transforms();
        assert_eq!(
            VertexLayout::new(&state, &[], Some(&transforms)).floats * 4,
            32
        );
        let stage = TextureStage {
            unit: 2,
            env: TexEnvMode::Modulate,
            env_color: [0.; 4],
            lod_bias: 0.0,
        };
        let layout = VertexLayout::new(&state, &[stage], Some(&transforms));
        assert_eq!(layout.floats * 4, 40);
        assert!(layout.attributes[2].is_none());
        assert_eq!(layout.attributes[4], Some((2, 8)));
        transforms.lighting.enabled = true;
        let layout = VertexLayout::new(&state, &[stage], Some(&transforms));
        assert_eq!(layout.floats * 4, 56);
        assert_eq!(layout.attributes[7], Some((4, 0)));
        state.smooth_shading = false;
        let layout = VertexLayout::new(&state, &[stage], Some(&transforms));
        assert_eq!(layout.floats * 4, 72);
        let vertex = Vertex {
            pos: [1., 2., 3., 4.],
            normal: [1., 0., 0.],
            color: [1.; 4],
            ..Default::default()
        };
        let provoking = Vertex {
            pos: [5., 6., 7., 8.],
            normal: [0., 1., 0.],
            color: [0.5; 4],
            ..vertex
        };
        let mut packed = Vec::new();
        layout.pack(&mut packed, vertex, provoking, false);
        assert_eq!(&packed[4..8], &provoking.color);
        assert_eq!(&packed[10..13], &provoking.normal);
        assert_eq!(&packed[14..18], &provoking.pos);
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires host desktop GL and GLES2; direct strips/fans and fused display readback"]
    fn host_direct_arrays_compact_layout_and_fused_present_match_reference() {
        let transforms = identity_transforms();
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::with_host(Host::es_for_test().unwrap(), 32, 32).unwrap()
            } else {
                Renderer::new(32, 32).unwrap()
            };
            let mut target = RenderTarget::new(32, 32);
            let positions = [
                [-0.75, -0.75, 0., 1.],
                [0.75, -0.75, 0., 1.],
                [-0.75, 0.75, 0., 1.],
                [0.75, 0.75, 0., 1.],
            ];
            let colors = [
                [1., 0., 0., 1.],
                [0., 1., 0., 1.],
                [0., 0., 1., 1.],
                [1., 1., 0., 1.],
            ];
            let vertices: [Vertex; 4] = std::array::from_fn(|i| Vertex {
                pos: positions[i],
                color: colors[i],
                ..Default::default()
            });
            let mut state = PipelineState {
                viewport: (0, 0, 32, 32),
                ..Default::default()
            };
            for smooth in [true, false] {
                state.smooth_shading = smooth;
                for mode in [GL_TRIANGLE_STRIP, GL_TRIANGLE_FAN] {
                    let vertices = if mode == GL_TRIANGLE_FAN {
                        [vertices[0], vertices[1], vertices[3], vertices[2]]
                    } else {
                        vertices
                    };
                    gpu.clear(32, 32, &state, [0.; 4], 1., GL_COLOR_BUFFER_BIT)
                        .unwrap();
                    gpu.draw_object_space(
                        &target,
                        &state,
                        mode,
                        &vertices,
                        &[],
                        &HashMap::new(),
                        [0; 6],
                        &transforms,
                    )
                    .unwrap();
                    gpu.read(&mut target).unwrap();
                    let reference = target.color.clone();
                    gpu.clear(32, 32, &state, [0.; 4], 1., GL_COLOR_BUFFER_BIT)
                        .unwrap();
                    gpu.draw_arrays_object_space(
                        &target,
                        &state,
                        mode,
                        &vertices,
                        &[],
                        &HashMap::new(),
                        [0; 6],
                        &transforms,
                    )
                    .unwrap();
                    if smooth {
                        assert_eq!(gpu.scratch.len() * 4, 128);
                        assert!(gpu.indices.is_empty());
                    }
                    let mut rgb = vec![0; 32 * 32 * 2];
                    gpu.read_present(&mut target, &mut rgb).unwrap();
                    for (a, b) in reference.iter().zip(&target.color) {
                        assert!(a.abs_diff(*b) <= 1, "es={es} mode={mode} smooth={smooth}");
                    }
                    let mut expected = vec![0; rgb.len()];
                    target.to_rgb565(&mut expected);
                    assert_eq!(rgb, expected);
                    rgb.fill(0);
                    gpu.read_present(&mut target, &mut rgb).unwrap(); // Clean-cache path.
                    assert_eq!(rgb, expected);
                    gpu.read(&mut target).unwrap(); // Canonical cache remains valid.
                }
            }
            state.smooth_shading = true;
            let stage = TextureStage {
                unit: 2,
                env: TexEnvMode::Modulate,
                env_color: [0.; 4],
                lod_bias: 0.0,
            };
            gpu.draw_arrays_object_space(
                &target,
                &state,
                GL_TRIANGLE_STRIP,
                &vertices,
                &[stage],
                &HashMap::new(),
                [0; 6],
                &transforms,
            )
            .unwrap();
            assert_eq!(gpu.scratch.len() * 4, 160); // Four 40-byte records, no indices.
            assert!(gpu.indices.is_empty());
            gpu.draw_arrays_object_space(
                &target,
                &state,
                GL_TRIANGLE_STRIP,
                &vertices,
                &[],
                &HashMap::new(),
                [0; 6],
                &transforms,
            )
            .unwrap();
            assert_eq!(gpu.scratch.len() * 4, 128); // Drop the texture attribute again.
            state.viewport = (0, 0, 7, 5);
            state.scissor = None;
            gpu.clear(7, 5, &state, [1., 0., 0., 1.], 1., GL_COLOR_BUFFER_BIT)
                .unwrap();
            state.scissor = Some((0, 0, 7, 2));
            gpu.clear(7, 5, &state, [0., 0., 1., 1.], 1., GL_COLOR_BUFFER_BIT)
                .unwrap();
            target.resize(7, 5);
            let mut short = vec![0x55; 7 * 5 * 2 - 1];
            gpu.read_present(&mut target, &mut short).unwrap();
            let mut expected = vec![0x55; short.len()];
            target.to_rgb565(&mut expected);
            assert_eq!(short, expected);
            assert_eq!(short.last(), Some(&0x55));
            assert_eq!(&target.color[..4], &[255, 0, 0, 255]);
            assert_eq!(&target.color[7 * 4 * 4..7 * 4 * 4 + 4], &[0, 0, 255, 255]);
        }
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires desktop GL and GLES2; indexed transfers and cache invalidation"]
    fn host_indexed_transfers_uniform_cache_and_polygon_offset() {
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::with_host(Host::es_for_test().unwrap(), 32, 32).unwrap()
            } else {
                Renderer::new(32, 32).unwrap()
            };
            let mut target = RenderTarget::new(32, 32);
            let mut state = PipelineState {
                viewport: (0, 0, 32, 32),
                depth_test: true,
                ..Default::default()
            };
            let tri = triangle([1., 0., 0., 1.], 0.);
            let vertices: Vec<_> = tri.into_iter().cycle().take(36).collect();
            gpu.draw(
                &target,
                &state,
                GL_TRIANGLES,
                &vertices,
                &[],
                &HashMap::new(),
                [0; 6],
            )
            .unwrap();
            assert_eq!(gpu.scratch.len(), 3 * 8);
            assert_eq!(gpu.indices.len(), 36);
            let colors_indices = [
                0, 2, 1, 2, 0, 3, 1, 5, 4, 5, 1, 2, 5, 3, 6, 3, 5, 2, 3, 7, 6, 7, 3, 0, 6, 4, 5, 4,
                6, 7, 1, 7, 0, 7, 1, 4,
            ];
            let cube: [Vertex; 8] = std::array::from_fn(|i| Vertex {
                pos: [
                    if i & 1 == 0 { -0.5 } else { 0.5 },
                    if i & 2 == 0 { -0.5 } else { 0.5 },
                    if i & 4 == 0 { -0.3 } else { 0.3 },
                    1.,
                ],
                color: [1., 0., 0., 1.],
                ..Default::default()
            });
            let mesh: Vec<_> = colors_indices.iter().map(|&i| cube[i]).collect();
            gpu.draw(
                &target,
                &state,
                GL_TRIANGLES,
                &mesh,
                &[],
                &HashMap::new(),
                [0; 6],
            )
            .unwrap();
            assert_eq!(gpu.scratch.len() * 4 + gpu.indices.len() * 2, 328);
            gpu.clear(32, 32, &state, [0.; 4], 1., GL_DEPTH_BUFFER_BIT)
                .unwrap();
            gpu.draw(
                &target,
                &state,
                GL_TRIANGLES,
                &vertices,
                &[],
                &HashMap::new(),
                [0; 6],
            )
            .unwrap();
            let uniforms = gpu.uniform_values.clone();
            gpu.draw(
                &target,
                &state,
                GL_TRIANGLES,
                &vertices,
                &[],
                &HashMap::new(),
                [0; 6],
            )
            .unwrap();
            assert_eq!(gpu.uniform_values, uniforms);
            assert!(!gpu.changed_uniform(gpu.loc("pointSize"), &[1f32.to_bits()]));
            gpu.read(&mut target).unwrap();
            assert_eq!(
                &target.color[(16 * 32 + 16) * 4..(16 * 32 + 16) * 4 + 4],
                &[255, 0, 0, 255]
            );
            state.color_mask = [false; 4];
            gpu.clear(32, 32, &state, [0.; 4], 1., GL_COLOR_BUFFER_BIT)
                .unwrap();
            assert!(gpu.pipeline.is_none());
            state.color_mask = [true; 4];
            state.polygon_offset = true;
            state.polygon_units = -2.;
            gpu.draw(
                &target,
                &state,
                GL_TRIANGLES,
                &triangle([0., 1., 0., 1.], 0.),
                &[],
                &HashMap::new(),
                [0; 6],
            )
            .unwrap();
            gpu.read(&mut target).unwrap();
            assert_eq!(
                &target.color[(16 * 32 + 16) * 4..(16 * 32 + 16) * 4 + 4],
                &[0, 255, 0, 255]
            );
            gpu.resize(16, 16).unwrap();
            assert!(gpu.pipeline.is_none());
        }
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires host OpenGL; checks points and line modes with both shader dialects"]
    fn host_points_lines_match_software_and_preserve_triangle_state() {
        use crate::raster;
        let vertices = [
            Vertex {
                pos: [-0.65, 0.4, 0., 1.],
                color: [1., 0.2, 0.1, 0.5],
                ..Default::default()
            },
            Vertex {
                pos: [0.3, 0.4, 0., 1.],
                color: [0.1, 1., 0.2, 0.5],
                ..Default::default()
            },
            Vertex {
                pos: [0.3, -0.6, 0., 1.],
                color: [0.2, 0.1, 1., 0.5],
                ..Default::default()
            },
        ];
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::with_host(Host::es_for_test().unwrap(), 32, 32).unwrap()
            } else {
                Renderer::new(32, 32).unwrap()
            };
            for mode in [GL_POINTS, GL_LINES, GL_LINE_STRIP, GL_LINE_LOOP] {
                for (size, smooth) in [1f32, 2., 3., 5.]
                    .into_iter()
                    .flat_map(|size| [(size, true), (size, false)])
                {
                    let mut state = PipelineState {
                        viewport: (0, 0, 32, 32),
                        smooth_shading: smooth,
                        point_size: size,
                        line_width: size.min(gpu.line_range[1]),
                        cull: Some(CullMode::FrontAndBack),
                        ..Default::default()
                    };
                    let mut draw_vertices = vertices;
                    // Wide-line edge interpolation differs across conforming drivers.
                    // Validate coverage/constant color for wide primitives and gradients at width 1.
                    if size > 1. {
                        for v in &mut draw_vertices {
                            v.color = [0.8, 0.4, 0.2, 0.5];
                        }
                    }
                    let mut cpu = RenderTarget::new(32, 32);
                    let mut native = RenderTarget::new(32, 32);
                    let sample = |_: usize, _: f32, _: f32| None;
                    gpu.clear(
                        32,
                        32,
                        &state,
                        [0.; 4],
                        1.,
                        GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
                    )
                    .unwrap();
                    match mode {
                        GL_POINTS => {
                            for v in draw_vertices {
                                raster::draw_point(&mut cpu, &state, &sample, &[], v);
                            }
                        }
                        GL_LINES => {
                            raster::draw_line(
                                &mut cpu,
                                &state,
                                &sample,
                                &[],
                                [draw_vertices[0], draw_vertices[1]],
                            );
                        }
                        _ => {
                            for v in draw_vertices.windows(2) {
                                raster::draw_line(&mut cpu, &state, &sample, &[], [v[0], v[1]]);
                            }
                            if mode == GL_LINE_LOOP {
                                raster::draw_line(
                                    &mut cpu,
                                    &state,
                                    &sample,
                                    &[],
                                    [draw_vertices[2], draw_vertices[0]],
                                );
                            }
                        }
                    }
                    gpu.draw(
                        &native,
                        &state,
                        mode,
                        &draw_vertices,
                        &[],
                        &HashMap::new(),
                        [0; 6],
                    )
                    .unwrap();
                    gpu.read(&mut native).unwrap();
                    let count =
                        |t: &RenderTarget| t.color.chunks_exact(4).filter(|p| p[3] > 0).count();
                    let a = count(&cpu);
                    let b = count(&native);
                    assert!(a > 0 && b > 0, "es={es}, mode={mode}, size={size}: {a}/{b}");
                    assert!(
                        a.abs_diff(b) <= vertices.len() * size as usize * 2,
                        "es={es}, mode={mode}, size={size}: coverage {a}/{b}"
                    );
                    for (a, b) in cpu
                        .color
                        .chunks_exact(4)
                        .zip(native.color.chunks_exact(4))
                        .filter(|(a, b)| a[3] > 0 && b[3] > 0)
                    {
                        for (&a, &b) in a.iter().zip(b) {
                            assert!(
                                a.abs_diff(b) <= 4,
                                "es={es},mode={mode},size={size}: color {a}/{b}"
                            );
                        }
                    }
                    // Point/line culling must not affect their drawing, while triangles still cull.
                    state.point_size = 1.;
                    state.line_width = 1.;
                    gpu.clear(32, 32, &state, [0.; 4], 1., GL_COLOR_BUFFER_BIT)
                        .unwrap();
                    gpu.draw(
                        &native,
                        &state,
                        GL_TRIANGLES,
                        &draw_vertices,
                        &[],
                        &HashMap::new(),
                        [0; 6],
                    )
                    .unwrap();
                    gpu.read(&mut native).unwrap();
                    assert_eq!(count(&native), 0);
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native desktop GL/GLES2; signed linear fog must match software"]
    fn host_linear_fog_descending_and_degenerate_ranges_match_software() {
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::with_host(Host::es_for_test().unwrap(), 32, 32).unwrap()
            } else {
                Renderer::new(32, 32).unwrap()
            };
            for (start, end, depth) in [
                (-100.5, -343.28522, 100.0), // Carmageddon's actual range.
                (10.0, 2.0, 6.0),
                (10.0, 2.0, 0.0),
                (10.0, 2.0, 20.0),
                (0.0, 10.0, 5.0),
                (2.0, 2.0, 6.0),
            ] {
                let state = PipelineState {
                    viewport: (0, 0, 32, 32),
                    fog: true,
                    fog_mode: FogMode::Linear,
                    fog_start: start,
                    fog_end: end,
                    fog_color: [0.0, 0.0, 0.8, 1.0],
                    ..Default::default()
                };
                let mut vertices = triangle([1.0, 0.4, 0.2, 1.0], 0.0);
                for v in &mut vertices {
                    v.fog_depth = depth;
                }
                let mut cpu = RenderTarget::new(32, 32);
                let mut native = RenderTarget::new(32, 32);
                raster::draw_triangle(&mut cpu, &state, &|_, _, _| None, &[], vertices);
                gpu.clear(32, 32, &state, [0.0; 4], 1.0, GL_COLOR_BUFFER_BIT)
                    .unwrap();
                gpu.draw(
                    &native,
                    &state,
                    GL_TRIANGLES,
                    &vertices,
                    &[],
                    &HashMap::new(),
                    [0; 6],
                )
                .unwrap();
                gpu.read(&mut native).unwrap();
                for (a, b) in center(&cpu).into_iter().zip(center(&native)) {
                    assert!(
                        a.abs_diff(b) <= 2,
                        "es={es}, fog={start}..{end}, depth={depth}: software={a}, GPU={b}"
                    );
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native GL/GLES2; strict alpha tests must match software precision"]
    fn host_strict_alpha_equal_matches_software_quantization() {
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::with_host(Host::es_for_test().unwrap(), 32, 32).unwrap()
            } else {
                Renderer::new(32, 32).unwrap()
            };
            let state = PipelineState {
                viewport: (0, 0, 32, 32),
                alpha_test: true,
                alpha_func: CompareFunc::Equal,
                alpha_ref: 1.,
                ..Default::default()
            };
            let vertices = triangle([0.8, 0.4, 0.2, 0.9999], 0.);
            let mut cpu = RenderTarget::new(32, 32);
            let mut native = RenderTarget::new(32, 32);
            raster::draw_triangle(&mut cpu, &state, &|_, _, _| None, &[], vertices);
            gpu.clear(32, 32, &state, [0.; 4], 1., GL_COLOR_BUFFER_BIT)
                .unwrap();
            gpu.draw(
                &native,
                &state,
                GL_TRIANGLES,
                &vertices,
                &[],
                &HashMap::new(),
                [0; 6],
            )
            .unwrap();
            gpu.read(&mut native).unwrap();
            let i = (16 * 32 + 16) * 4;
            assert!(cpu.color[i] > 0);
            for c in 0..4 {
                assert!(
                    cpu.color[i + c].abs_diff(native.color[i + c]) <= 1,
                    "es={es}, channel={c}: software {}, GPU {}",
                    cpu.color[i + c],
                    native.color[i + c]
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native GL/GLES2; ES depth clear mask with writes disabled"]
    fn host_depth_clear_matches_software_with_depth_writes_disabled() {
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::with_host(Host::es_for_test().unwrap(), 32, 32).unwrap()
            } else {
                Renderer::new(32, 32).unwrap()
            };
            let mut state = PipelineState {
                viewport: (0, 0, 32, 32),
                depth_test: true,
                ..Default::default()
            };
            let mut cpu = RenderTarget::new(32, 32);
            let mut native = RenderTarget::new(32, 32);
            gpu.clear(
                32,
                32,
                &state,
                [0.; 4],
                1.,
                GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
            )
            .unwrap();
            let near = triangle([1., 0., 0., 1.], 0.);
            gpu.draw(
                &native,
                &state,
                GL_TRIANGLES,
                &near,
                &[],
                &HashMap::new(),
                [0; 6],
            )
            .unwrap();
            raster::draw_triangle(&mut cpu, &state, &|_, _, _| None, &[], near);
            state.depth_write = false;
            cpu.clear_color([0; 4], None);
            // The previous near geometry must retain its depth.
            gpu.clear(
                32,
                32,
                &state,
                [0.; 4],
                1.,
                GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
            )
            .unwrap();
            let far = triangle([0., 1., 0., 1.], 0.5);
            gpu.draw(
                &native,
                &state,
                GL_TRIANGLES,
                &far,
                &[],
                &HashMap::new(),
                [0; 6],
            )
            .unwrap();
            gpu.read(&mut native).unwrap();
            raster::draw_triangle(&mut cpu, &state, &|_, _, _| None, &[], far);
            let i = (16 * 32 + 16) * 4;
            assert_eq!(&cpu.color[i..i + 4], &[0; 4], "far geometry stays occluded");
            assert_eq!(
                &native.color[i..i + 4],
                &[0; 4],
                "es={es}: depth mask must protect the near depth"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native GL/GLES2; Intel 2700G depth bias at both render scales"]
    fn host_intel2700g_constant_bias_preserves_occlusion_and_coplanar_lighting() {
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::with_host(Host::es_for_test().unwrap(), 32, 32).unwrap()
            } else {
                Renderer::new(32, 32).unwrap()
            };
            for format in [0x81a5, 0x81a6] {
                for scale in [1, 2] {
                    gpu.depth_format = format;
                    gpu.resize(16, 16).unwrap();
                    gpu.resize(32, 32).unwrap();
                    gpu.set_render_scale(scale).unwrap();
                    let mut target = RenderTarget::new(32, 32);
                    let mut state = PipelineState {
                        viewport: (0, 0, 32, 32),
                        depth_test: true,
                        intel2700g_depth_bias: true,
                        ..Default::default()
                    };
                    gpu.clear(
                        32,
                        32,
                        &state,
                        [0.; 4],
                        1.,
                        GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
                    )
                    .unwrap();
                    // Neither the profile alone nor an unbiased draw needs
                    // any host resolution measurement.
                    let previous_step = gpu.depth_bias_step;
                    let foreground = triangle([0., 0., 1., 1.], -0.2);
                    gpu.draw(
                        &target,
                        &state,
                        GL_TRIANGLES,
                        &foreground,
                        &[],
                        &HashMap::new(),
                        [0; 6],
                    )
                    .unwrap();
                    assert_eq!(gpu.depth_bias_step, previous_step);
                    let mut light = triangle([1., 0., 0., 1.], 0.3);
                    light[0].pos[2] = 0.2;
                    light[1].pos[2] = 0.4;
                    state.polygon_offset = true;
                    state.polygon_factor = -512.;
                    state.polygon_units = -512.;
                    state.blend = true;
                    state.blend_src = BlendFactor::One;
                    state.blend_dst = BlendFactor::One;
                    gpu.draw(
                        &target,
                        &state,
                        GL_TRIANGLES,
                        &light,
                        &[],
                        &HashMap::new(),
                        [0; 6],
                    )
                    .unwrap();
                    gpu.read(&mut target).unwrap();
                    let pixel = (16 * 32 + 16) * 4;
                    assert_eq!(
                        &target.color[pixel..pixel + 4],
                        &[0, 0, 255, 255],
                        "es={es} format={format:x} scale={scale}"
                    );
                    // Bias remains active: an exactly coplanar light must pass
                    // GL_LESS, rather than disappearing as when bias is disabled.
                    let light = triangle([0., 1., 0., 1.], -0.2);
                    state.blend = false;
                    gpu.draw(
                        &target,
                        &state,
                        GL_TRIANGLES,
                        &light,
                        &[],
                        &HashMap::new(),
                        [0; 6],
                    )
                    .unwrap();
                    gpu.read(&mut target).unwrap();
                    assert_eq!(
                        &target.color[pixel..pixel + 4],
                        &[0, 255, 0, 255],
                        "coplanar es={es} format={format:x} scale={scale}"
                    );
                    let measured = gpu.depth_bias_step;
                    assert_eq!(measured.map(|x| x.0), Some(format));
                    // A slightly separated decal needs fixed-input bias, not
                    // merely an offset large enough to separate identical Z.
                    state.polygon_units = -32.;
                    let light = triangle([1., 0., 0., 1.], -0.2 + 1. / 2048.);
                    // Clear the earlier green triangle's biased depth and
                    // repeat against the original unbiased surface.
                    gpu.clear(
                        32,
                        32,
                        &state,
                        [0.; 4],
                        1.,
                        GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
                    )
                    .unwrap();
                    state.polygon_offset = false;
                    gpu.draw(
                        &target,
                        &state,
                        GL_TRIANGLES,
                        &foreground,
                        &[],
                        &HashMap::new(),
                        [0; 6],
                    )
                    .unwrap();
                    state.polygon_offset = true;
                    gpu.draw(
                        &target,
                        &state,
                        GL_TRIANGLES,
                        &light,
                        &[],
                        &HashMap::new(),
                        [0; 6],
                    )
                    .unwrap();
                    gpu.read(&mut target).unwrap();
                    assert_eq!(
                        &target.color[pixel..pixel + 4],
                        &[255, 0, 0, 255],
                        "separated es={es} format={format:x} scale={scale}"
                    );
                    assert_eq!(gpu.depth_bias_step, measured);
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native GL/GLES2; closely spaced terrain depth layers"]
    fn host_depth24_separates_layers_that_depth16_collapses() {
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::with_host(Host::es_for_test().unwrap(), 32, 32).unwrap()
            } else {
                Renderer::new(32, 32).unwrap()
            };
            assert_eq!(gpu.depth_format, 0x81a6);
            let state = PipelineState {
                viewport: (0, 0, 32, 32),
                depth_test: true,
                ..Default::default()
            };
            let mut native = RenderTarget::new(32, 32);
            let mut cpu = RenderTarget::new(32, 32);
            let far = triangle([1., 0., 0., 1.], 0.75);
            let near = triangle([0., 1., 0., 1.], 0.75 - 0.000001);
            for format in [0x81a5, 0x81a6] {
                gpu.depth_format = format;
                // Force a real attachment reallocation, as on orientation changes.
                gpu.resize(16, 16).unwrap();
                gpu.resize(32, 32).unwrap();
                gpu.clear(
                    32,
                    32,
                    &state,
                    [0.; 4],
                    1.,
                    GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
                )
                .unwrap();
                for vertices in [far, near] {
                    gpu.draw(
                        &native,
                        &state,
                        GL_TRIANGLES,
                        &vertices,
                        &[],
                        &HashMap::new(),
                        [0; 6],
                    )
                    .unwrap();
                }
                gpu.read(&mut native).unwrap();
                let i = (16 * 32 + 16) * 4;
                if format == 0x81a5 {
                    assert_eq!(&native.color[i..i + 4], &[255, 0, 0, 255]);
                } else {
                    for vertices in [far, near] {
                        raster::draw_triangle(&mut cpu, &state, &|_, _, _| None, &[], vertices);
                    }
                    assert_eq!(&native.color[i..i + 4], &[0, 255, 0, 255]);
                    assert_eq!(&native.color[i..i + 4], &cpu.color[i..i + 4]);
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native desktop GL and GLES; inactive uniform payload fast path"]
    fn inactive_uniforms_skip_payload_reads_and_active_uniforms_keep_value_caching() {
        use std::cell::Cell;
        thread_local! {static VECTORS:Cell<usize>=const{Cell::new(0)};static MATRICES:Cell<usize>=const{Cell::new(0)};}
        unsafe extern "system" fn vector(_: i32, _: i32, _: *const f32) {
            VECTORS.with(|n| n.set(n.get() + 1));
        }
        unsafe extern "system" fn matrix(_: i32, _: i32, _: u8, _: *const f32) {
            MATRICES.with(|n| n.set(n.get() + 1));
        }
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::new_es_for_test(8, 8).unwrap()
            } else {
                Renderer::new(8, 8).unwrap()
            };
            let (v, m) = (gpu.gl.Uniform4fv, gpu.gl.UniformMatrix4fv);
            gpu.gl.Uniform4fv = vector;
            gpu.gl.UniformMatrix4fv = matrix;
            VECTORS.with(|n| n.set(0));
            MATRICES.with(|n| n.set(0));
            unsafe {
                // Inactive locations return before touching any payload.
                gpu.uniform_4fv(-1, 8, std::ptr::null());
                gpu.uniform_matrix4fv(-1, 1, 0, std::ptr::null());
                assert!(!gpu.uniform_values.contains_key(&-1));
                let mut color = [0.25; 4];
                gpu.uniform_4fv(501, 1, color.as_ptr());
                gpu.uniform_4fv(501, 1, color.as_ptr());
                color[0] = 0.75;
                gpu.uniform_4fv(501, 1, color.as_ptr());
                gpu.uniform_matrix4fv(502, 1, 0, IDENTITY.as_ptr());
                gpu.uniform_matrix4fv(502, 1, 0, IDENTITY.as_ptr());
            }
            VECTORS.with(|n| assert_eq!(n.get(), 2));
            MATRICES.with(|n| assert_eq!(n.get(), 1));
            gpu.gl.Uniform4fv = v;
            gpu.gl.UniformMatrix4fv = m;
            gpu.poll_error().unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native desktop GL and GLES; NV sampler/coordinate routing transitions"]
    fn nv_unused_sampler_units_skip_bindings_uploads_and_lod_without_losing_coordinates() {
        use std::cell::Cell;
        type Active = unsafe extern "system" fn(u32);
        thread_local! {static REAL:Cell<Option<Active>>=const{Cell::new(None)};static UNITS:Cell<u32>=const{Cell::new(0)};}
        unsafe extern "system" fn active(unit: u32) {
            UNITS.with(|m| m.set(m.get() | 1 << (unit - GL_TEXTURE0)));
            let real = REAL.with(Cell::get).unwrap();
            real(unit);
        }
        let program = std::sync::Arc::new(
            crate::combiner_program::Program::decode(&[
                b'A', b'R', b'1', b'0', 1, 0x70, 0x06, 0, 0, 0x83, 0xc3, 0xcc, 0x70, 0x80, 0, 0, 0,
                0, 0xa4, 0xe4, 0, 0x88, 0x29, 0x5c, 0x5b, 0x29,
            ])
            .unwrap()
            .compile_basic()
            .unwrap(),
        );
        assert_eq!(program.texture_units(), 1);
        assert_eq!(program.coordinate_units(), 2);
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::new_es_for_test(8, 8).unwrap()
            } else {
                Renderer::new(8, 8).unwrap()
            };
            let mut target = RenderTarget::new(8, 8);
            let state = PipelineState {
                viewport: (0, 0, 8, 8),
                ..Default::default()
            };
            let vertices = [[-1., -1., 0., 1.], [1., -1., 0., 1.], [0., 1., 0., 1.]].map(|pos| {
                let mut v = Vertex {
                    pos,
                    color: [1.; 4],
                    ..Default::default()
                };
                v.texcoord[1] = [0.75, 0., 0., 1.];
                v
            });
            let mut t = Texture::default();
            t.min_filter = Filter::Nearest;
            t.mag_filter = Filter::Nearest;
            t.set_level(0, 2, 1, vec![255, 0, 0, 255, 0, 255, 0, 255]);
            let mut unused = Texture::default();
            unused.min_filter = Filter::Nearest;
            unused.max_lod = 0.;
            unused.set_level(0, 2, 2, [0, 0, 255, 255].repeat(4));
            let textures = HashMap::from([(1, t), (2, unused)]);
            let stages = [0, 1].map(|unit| TextureStage {
                unit,
                env: TexEnvMode::Replace,
                env_color: [0.; 4],
                lod_bias: 0.,
            });
            let real = gpu.gl.ActiveTexture;
            REAL.with(|r| r.set(Some(real)));
            gpu.gl.ActiveTexture = active;
            for nv in [true, false, true] {
                gpu.set_combiner(nv.then_some(&program), [[0.; 4]; 4]);
                UNITS.with(|m| m.set(0));
                gpu.draw(
                    &target,
                    &state,
                    GL_TRIANGLES,
                    &vertices,
                    &stages,
                    &textures,
                    [1, 2, 0, 0, 0, 0],
                )
                .unwrap();
                UNITS.with(|m| assert_eq!(m.get(), if nv { 1 } else { 15 }, "es={es} nv={nv}"));
                assert_eq!(gpu.range_active, !nv);
                if nv && gpu.textures.len() == 1 {
                    assert!(!gpu.textures.contains_key(&2));
                }
                gpu.read(&mut target).unwrap();
                let i = (3 * 8 + 4) * 4;
                assert_eq!(
                    &target.color[i..i + 4],
                    if nv {
                        &[0, 255, 0, 255]
                    } else {
                        &[0, 0, 255, 255]
                    },
                    "es={es} nv={nv}"
                );
                gpu.poll_error().unwrap();
            }
            gpu.gl.ActiveTexture = real;
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native desktop GL and GLES; shared texture cache lifecycle"]
    fn shared_texture_cache_retains_live_images_and_prunes_local_or_unlinked_objects() {
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::new_es_for_test(8, 8).unwrap()
            } else {
                Renderer::new(8, 8).unwrap()
            };
            let mut target = RenderTarget::new(8, 8);
            let state = PipelineState {
                viewport: (0, 0, 8, 8),
                ..Default::default()
            };
            let vertices =
                [[-1., -1., 0., 1.], [1., -1., 0., 1.], [0., 1., 0., 1.]].map(|pos| Vertex {
                    pos,
                    ..Default::default()
                });
            let stage = TextureStage {
                unit: 0,
                env: TexEnvMode::Replace,
                env_color: [0.; 4],
                lod_bias: 0.,
            };
            let mut tex = Texture::default();
            tex.min_filter = Filter::Nearest;
            tex.set_level(0, 1, 1, vec![255, 0, 0, 255]);
            let mut textures = HashMap::from([(0, tex.clone()), (7, tex.clone()), (8, tex)]);
            for name in [0, 7, 8] {
                gpu.draw(
                    &target,
                    &state,
                    GL_TRIANGLES,
                    &vertices,
                    &[stage],
                    &textures,
                    [name, 0, 0, 0, 0, 0],
                )
                .unwrap();
            }
            let original = gpu.textures[&7];
            gpu.retain_shared_texture_cache(&std::collections::HashSet::from([7]));
            assert!(!gpu.textures.contains_key(&0));
            assert!(!gpu.textures.contains_key(&8));
            assert_eq!(gpu.textures[&7], original);
            gpu.draw(
                &target,
                &state,
                GL_TRIANGLES,
                &vertices,
                &[stage],
                &textures,
                [7, 0, 0, 0, 0, 0],
            )
            .unwrap();
            assert_eq!(gpu.textures[&7], original);
            textures
                .get_mut(&7)
                .unwrap()
                .set_level(0, 1, 1, vec![0, 255, 0, 255]);
            gpu.draw(
                &target,
                &state,
                GL_TRIANGLES,
                &vertices,
                &[stage],
                &textures,
                [7, 0, 0, 0, 0, 0],
            )
            .unwrap();
            assert_eq!(gpu.textures[&7].0, original.0);
            assert_ne!(gpu.textures[&7].1, original.1);
            gpu.read(&mut target).unwrap();
            assert_eq!(
                &target.color[(3 * 8 + 4) * 4..(3 * 8 + 4) * 4 + 4],
                &[0, 255, 0, 255]
            );
            gpu.invalidate_texture_cache();
            assert!(gpu.textures.is_empty());
            assert!(gpu.texture_params.is_empty());
            // A new unrelated namespace may have the same object id/revision.
            textures
                .get_mut(&7)
                .unwrap()
                .set_level(0, 1, 1, vec![0, 0, 255, 255]);
            textures.get_mut(&7).unwrap().revision = original.1;
            gpu.draw(
                &target,
                &state,
                GL_TRIANGLES,
                &vertices,
                &[stage],
                &textures,
                [7, 0, 0, 0, 0, 0],
            )
            .unwrap();
            gpu.read(&mut target).unwrap();
            assert_eq!(
                &target.color[(3 * 8 + 4) * 4..(3 * 8 + 4) * 4 + 4],
                &[0, 0, 255, 255]
            );
            gpu.poll_error().unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native desktop GL and GLES2; completion without readback"]
    fn finish_and_flush_submit_native_work_without_readback() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static FINISH_CALLS: AtomicUsize = AtomicUsize::new(0);
        static FLUSH_CALLS: AtomicUsize = AtomicUsize::new(0);
        unsafe extern "system" fn finish_probe() {
            FINISH_CALLS.fetch_add(1, Ordering::SeqCst);
        }
        unsafe extern "system" fn flush_probe() {
            FLUSH_CALLS.fetch_add(1, Ordering::SeqCst);
        }
        FINISH_CALLS.store(0, Ordering::SeqCst);
        FLUSH_CALLS.store(0, Ordering::SeqCst);
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::new_es_for_test(16, 16).unwrap()
            } else {
                Renderer::new(16, 16).unwrap()
            };
            let mut target = RenderTarget::new(16, 16);
            gpu.clear(
                16,
                16,
                &PipelineState::default(),
                [1., 0., 0., 1.],
                1.,
                GL_COLOR_BUFFER_BIT,
            )
            .unwrap();
            let before = target.color.clone();
            let finish = gpu.gl.Finish;
            let flush = gpu.gl.Flush;
            gpu.gl.Finish = finish_probe;
            gpu.gl.Flush = flush_probe;
            gpu.flush().unwrap();
            gpu.finish().unwrap();
            assert_eq!(FINISH_CALLS.load(Ordering::SeqCst), if es { 2 } else { 1 });
            assert_eq!(FLUSH_CALLS.load(Ordering::SeqCst), if es { 2 } else { 1 });
            gpu.gl.Finish = finish;
            gpu.gl.Flush = flush;
            gpu.flush().unwrap();
            gpu.finish().unwrap();
            assert!(
                gpu.dirty,
                "completion must not mark the readback cache as current"
            );
            assert_eq!(target.color, before);
            assert!(gpu.readback.is_empty(), "no hidden framebuffer download");
            gpu.read(&mut target).unwrap();
            assert_eq!(&target.color[..4], &[255, 0, 0, 255]);
            assert!(!gpu.dirty);
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native desktop GL and GLES2; deferred driver errors"]
    fn draw_and_flush_defer_driver_errors_to_read_or_finish() {
        use std::cell::Cell;
        thread_local! {
            static CALLS: Cell<u32> = const { Cell::new(0) };
            static ERROR: Cell<u32> = const { Cell::new(0) };
        }
        unsafe extern "system" fn error_probe() -> u32 {
            CALLS.with(|n| n.set(n.get() + 1));
            ERROR.with(|e| e.replace(0))
        }
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::new_es_for_test(16, 16).unwrap()
            } else {
                Renderer::new(16, 16).unwrap()
            };
            let mut target = RenderTarget::new(16, 16);
            let state = PipelineState::default();
            let original = gpu.gl.GetError;
            CALLS.with(|n| n.set(0));
            ERROR.with(|e| e.set(0));
            gpu.gl.GetError = error_probe;
            for _ in 0..16 {
                gpu.draw(
                    &target,
                    &state,
                    GL_TRIANGLES,
                    &triangle([1.; 4], 0.),
                    &[],
                    &HashMap::new(),
                    [0; 6],
                )
                .unwrap();
                gpu.flush().unwrap();
            }
            assert_eq!(CALLS.with(Cell::get), 0, "no per-draw or flush query");
            ERROR.with(|e| e.set(GL_OUT_OF_MEMORY));
            assert!(
                gpu.poll_error().is_err(),
                "explicit guest error query sees pending failures"
            );
            ERROR.with(|e| e.set(GL_OUT_OF_MEMORY));
            assert!(gpu.read(&mut target).is_err());
            assert!(gpu.dirty, "failed read must not publish the native cache");
            gpu.dirty = false;
            ERROR.with(|e| e.set(GL_INVALID_OPERATION));
            assert!(
                gpu.read(&mut target).is_err(),
                "clean-cache reads still check deferred errors"
            );
            gpu.dirty = true;

            ERROR.with(|e| e.set(GL_INVALID_OPERATION));
            assert!(gpu.finish().is_err());
            let mut texture = Texture::default();
            texture.set_level(0, 1, 1, vec![255; 4]);
            texture.min_filter = Filter::Nearest;
            let textures = HashMap::from([(1, texture)]);
            let stage = TextureStage {
                unit: 0,
                env: TexEnvMode::Modulate,
                env_color: [0.; 4],
                lod_bias: 0.,
            };
            ERROR.with(|e| e.set(GL_INVALID_OPERATION));
            assert!(
                gpu.draw(
                    &target,
                    &state,
                    GL_TRIANGLES,
                    &triangle([1.; 4], 0.),
                    &[stage],
                    &textures,
                    [1; 6]
                )
                .is_err(),
                "pending draw errors must not be consumed by upload fallback"
            );
            assert_eq!(gpu.textures[&1].1, u64::MAX, "upload must not proceed");
            gpu.gl.GetError = original;
            gpu.read(&mut target).unwrap();
            assert!(!gpu.dirty);
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires desktop GL and GLES2; real 2x geometry, logical resolve and scissor"]
    fn internal_resolution_renders_new_pixels_and_preserves_logical_reads() {
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::new_es_for_test(16, 16).unwrap()
            } else {
                Renderer::new(16, 16).unwrap()
            };
            gpu.set_render_scale(2).unwrap();
            assert_eq!((gpu.width, gpu.height), (32, 32));
            let mut target = RenderTarget::new(16, 16);
            let mut state = PipelineState {
                viewport: (0, 0, 16, 16),
                ..Default::default()
            };
            let textures = HashMap::new();
            gpu.clear(16, 16, &state, [0., 0., 0., 1.], 1., GL_COLOR_BUFFER_BIT)
                .unwrap();
            gpu.draw(
                &target,
                &state,
                GL_TRIANGLES,
                &triangle([1., 0., 0., 1.], 0.),
                &[],
                &textures,
                [0; 6],
            )
            .unwrap();
            let mut logical = vec![0; 16 * 16 * 2];
            gpu.read_present(&mut target, &mut logical).unwrap();
            assert_eq!(
                (target.width, target.height, target.color.len()),
                (16, 16, 16 * 16 * 4)
            );
            let mut high = Vec::new();
            assert_eq!(gpu.scaled_rgb565_into(&mut high), Some((32, 32)));
            assert_eq!(high.len(), 32 * 32 * 2);
            let mut rgba = Vec::new();
            assert_eq!(gpu.scaled_display_rgba_into(&mut rgba), Some((32, 32)));
            for (old, new) in high.chunks_exact(2).zip(rgba.chunks_exact(4)) {
                let p = u16::from_le_bytes([old[0], old[1]]);
                let r = (p >> 11) as u8;
                let g = ((p >> 5) & 63) as u8;
                let b = (p & 31) as u8;
                assert_eq!(
                    new,
                    &[
                        (r << 3) | (r >> 2),
                        (g << 2) | (g >> 4),
                        (b << 3) | (b >> 2),
                        255
                    ]
                );
            }

            let pixel = |x: usize, y: usize| {
                u16::from_le_bytes([high[(y * 32 + x) * 2], high[(y * 32 + x) * 2 + 1]])
            };
            let mut subpixel_edges = 0;
            for y in 0..16 {
                for x in 0..16 {
                    let first = pixel(x * 2, y * 2);
                    if pixel(x * 2 + 1, y * 2) != first
                        || pixel(x * 2, y * 2 + 1) != first
                        || pixel(x * 2 + 1, y * 2 + 1) != first
                    {
                        subpixel_edges += 1;
                    }
                }
            }
            assert!(
                subpixel_edges > 0,
                "2x must rasterize geometry, not repeat native pixels"
            );
            // A logical scissor affects a doubled rectangle in native GL coordinates.
            state.scissor = Some((0, 0, 8, 8));
            gpu.clear(16, 16, &state, [0., 1., 0., 1.], 1., GL_COLOR_BUFFER_BIT)
                .unwrap();
            gpu.read(&mut target).unwrap();
            assert_eq!(
                &target.color[((15 * 16) * 4)..((15 * 16) * 4 + 4)],
                &[0, 255, 0, 255]
            );
            assert_ne!(&target.color[..4], &[0, 255, 0, 255]);
            // Rotation-sized surfaces remain logical; disabling restores native FBO size.
            gpu.resize(24, 16).unwrap();
            assert_eq!((gpu.width, gpu.height), (48, 32));
            gpu.set_render_scale(1).unwrap();
            assert_eq!((gpu.width, gpu.height), (24, 16));
            assert_eq!(gpu.scaled_rgb565_into(&mut high), None);
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires native OpenGL and GLES2; ordered color logic operations"]
    fn host_logic_ops_match_software_with_overlapping_primitives() {
        for es in [false, true] {
            for scale in [1, 2] {
                let mut gpu = if es {
                    Renderer::with_host(Host::es_for_test().unwrap(), 32, 32).unwrap()
                } else {
                    Renderer::new(32, 32).unwrap()
                };
                gpu.set_render_scale(scale).unwrap();
                let textures = HashMap::new();
                let src = [0x69u8, 0xb4, 0xe1, 0x93];
                let dst = [0xa5u8, 0x3c, 0x72, 0xc6];
                for op in 0x1500..=0x150f {
                    let mut state = PipelineState {
                        viewport: (0, 0, 32, 32),
                        logic_op: true,
                        logic_mode: op,
                        blend: true,
                        blend_src: BlendFactor::Zero,
                        blend_dst: BlendFactor::Zero,
                        color_mask: [true, false, true, true],
                        ..Default::default()
                    };
                    let clear = dst.map(|v| v as f32 / 255.);
                    let mut native = RenderTarget::new(32, 32);
                    let mut cpu = RenderTarget::new(32, 32);
                    cpu.clear_color(dst, None);
                    gpu.clear(
                        32,
                        32,
                        &PipelineState::default(),
                        clear,
                        1.,
                        GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
                    )
                    .unwrap();
                    let tri = triangle(src.map(|v| v as f32 / 255.), 0.);
                    gpu.draw(&native, &state, GL_TRIANGLES, &tri, &[], &textures, [0; 6])
                        .unwrap();
                    gpu.read(&mut native).unwrap();
                    let i = (16 * 32 + 16) * 4;
                    let expected = std::array::from_fn::<_, 4, _>(|channel| {
                        if state.color_mask[channel] {
                            raster::logic_byte(op, src[channel], dst[channel])
                        } else {
                            dst[channel]
                        }
                    });
                    assert_eq!(
                        &native.color[i..i + 4],
                        &expected,
                        "single es={es} scale={scale} op={op:x}"
                    );
                    gpu.clear(
                        32,
                        32,
                        &PipelineState::default(),
                        clear,
                        1.,
                        GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
                    )
                    .unwrap();
                    let mut vertices = tri.to_vec();
                    vertices.extend(tri);
                    gpu.draw(
                        &native,
                        &state,
                        GL_TRIANGLES,
                        &vertices,
                        &[],
                        &textures,
                        [0; 6],
                    )
                    .unwrap();
                    for _ in 0..2 {
                        raster::draw_triangle(&mut cpu, &state, &|_, _, _| None, &[], tri);
                    }
                    gpu.read(&mut native).unwrap();
                    let i = (16 * 32 + 16) * 4;
                    assert_eq!(
                        &native.color[i..i + 4],
                        &cpu.color[i..i + 4],
                        "es={es} scale={scale} op={op:x}"
                    );
                    // Switching back must restore blending and normal batched draws.
                    state.logic_op = false;
                    state.color_mask = [true; 4];
                    gpu.draw(&native, &state, GL_TRIANGLES, &tri, &[], &textures, [0; 6])
                        .unwrap();
                    gpu.read(&mut native).unwrap();
                    assert_eq!(&native.color[i..i + 4], &[0; 4]);
                }
            }
        }
    }

    fn triangle(color: [f32; 4], z: f32) -> [Vertex; 3] {
        [[-0.9, -0.9, z, 1.], [0.9, -0.9, z, 1.], [0., 0.9, z, 1.]].map(|pos| Vertex {
            pos,
            color,
            ..Vertex::default()
        })
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires host OpenGL and GLES2 drivers"]
    fn host_vertex_shader_matches_cpu_transforms() {
        use crate::matrix;
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::with_host(Host::es_for_test().unwrap(), 32, 32).unwrap()
            } else {
                Renderer::new(32, 32).unwrap()
            };
            for perspective in [false, true] {
                let transforms = VertexTransforms {
                    lighting: Default::default(),
                    normal_matrix: IDENTITY,
                    projection: if perspective {
                        matrix::frustum(-1., 1., -1., 1., 1., 10.)
                    } else {
                        IDENTITY
                    },
                    modelview: matrix::multiply(
                        &matrix::translate(0.125, -0.125, if perspective { -2. } else { -0.25 }),
                        &matrix::multiply(
                            &matrix::rotate(23., 0., 0., 1.),
                            &matrix::scale(0.75, 0.625, 1.),
                        ),
                    ),
                    textures: std::array::from_fn(|i| {
                        matrix::multiply(
                            &matrix::translate(0.125 * i as f32, 0.25, 0.),
                            &matrix::scale(0.5, 0.75, 1.),
                        )
                    }),
                };
                let mut raw = triangle([0.9, 0.75, 0.5, 1.], 0.);
                raw[0].texcoord = [[0.1, 0.1, 0., 1.]; 6];
                raw[1].texcoord = [[0.8, 0.1, 0., 1.]; 6];
                raw[2].texcoord = [[0.4, 0.9, 0., 1.]; 6];
                let mut cpu = raw;
                let mvp = matrix::multiply(&transforms.projection, &transforms.modelview);
                for vertex in &mut cpu {
                    vertex.fog_depth =
                        matrix::transform(&transforms.modelview, vertex.pos)[2].abs();
                    vertex.pos = matrix::transform(&mvp, vertex.pos);
                    for i in 0..4 {
                        let uv = vertex.texcoord[i];
                        let t = matrix::transform(&transforms.textures[i], uv);
                        vertex.texcoord[i] = t;
                    }
                }
                let texture = Texture {
                    width: 4,
                    height: 4,
                    rgba: (0..16)
                        .flat_map(|i| [(i % 4 * 64) as u8, (i / 4 * 64) as u8, 128, 255])
                        .collect(),
                    min_filter: Filter::Linear,
                    mag_filter: Filter::Linear,
                    revision: 1,
                    ..Default::default()
                };
                let textures = HashMap::from([(1, texture)]);
                for unit in 0..4 {
                    let stages = [TextureStage {
                        unit,
                        env: TexEnvMode::Modulate,
                        env_color: [0.; 4],
                        lod_bias: 0.0,
                    }];
                    let mut names = [0; 6];
                    names[unit] = 1;
                    for mode in [GL_TRIANGLES, GL_TRIANGLE_STRIP, GL_TRIANGLE_FAN] {
                        let state = PipelineState {
                            viewport: (0, 0, 32, 32),
                            fog: true,
                            fog_mode: FogMode::Linear,
                            fog_start: 0.,
                            fog_end: 5.,
                            fog_color: [0.1, 0.2, 0.3, 1.],
                            depth_test: true,
                            ..Default::default()
                        };
                        let mut target = RenderTarget::new(32, 32);
                        gpu.clear(
                            32,
                            32,
                            &state,
                            [0., 0., 0., 1.],
                            1.,
                            GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
                        )
                        .unwrap();
                        gpu.draw(&target, &state, mode, &cpu, &stages, &textures, names)
                            .unwrap();
                        gpu.read(&mut target).unwrap();
                        let expected = target.color.clone();
                        gpu.clear(
                            32,
                            32,
                            &state,
                            [0., 0., 0., 1.],
                            1.,
                            GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
                        )
                        .unwrap();
                        gpu.draw_object_space(
                            &target,
                            &state,
                            mode,
                            &raw,
                            &stages,
                            &textures,
                            names,
                            &transforms,
                        )
                        .unwrap();
                        gpu.read(&mut target).unwrap();
                        assert!(expected.chunks_exact(4).any(|p| p[0] != 0));
                        let differences = expected
                            .iter()
                            .zip(&target.color)
                            .filter(|(a, b)| a.abs_diff(**b) > 2)
                            .count();
                        assert!(differences <= 12, "es={es} perspective={perspective} unit={unit} mode={mode} differences={differences}");
                    }
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires host OpenGL and GLES2 lighting shaders"]
    fn host_fixed_lighting_materials_normals_spots_and_flat_shading() {
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::with_host(Host::es_for_test().unwrap(), 32, 32).unwrap()
            } else {
                Renderer::new(32, 32).unwrap()
            };
            let mut target = RenderTarget::new(32, 32);
            let mut state = PipelineState {
                viewport: (0, 0, 32, 32),
                ..Default::default()
            };
            let textures = HashMap::new();
            let raw = triangle([1., 0., 0., 1.], 0.);
            let mut transform = VertexTransforms {
                projection: IDENTITY,
                modelview: IDENTITY,
                textures: [IDENTITY; 6],
                normal_matrix: IDENTITY,
                lighting: Default::default(),
            };
            transform.lighting.enabled = true;
            transform.lighting.model_ambient = [0.; 4];
            transform.lighting.ambient = [0.; 4];
            transform.lighting.diffuse = [0.8, 0.4, 0.2, 0.7];
            transform.lighting.lights[0].enabled = true;
            let mut render = |state: &PipelineState, vertices: &[Vertex], t: &VertexTransforms| {
                gpu.clear(
                    32,
                    32,
                    state,
                    [0., 0., 0., 1.],
                    1.,
                    GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
                )
                .unwrap();
                gpu.draw_object_space(
                    &target,
                    state,
                    GL_TRIANGLES,
                    vertices,
                    &[],
                    &textures,
                    [0; 6],
                    t,
                )
                .unwrap();
                if !state.smooth_shading {
                    assert!(
                        gpu.indices.is_empty(),
                        "flat triangle lists use direct submission"
                    );
                }
                gpu.read(&mut target).unwrap();
                center(&target)
            };
            let lit = render(&state, &raw, &transform);
            for (actual, expected) in lit.into_iter().zip([204u8, 102, 51, 178]) {
                assert!(actual.abs_diff(expected) <= 1);
            }
            transform.lighting.lights[0].enabled = false;
            transform.lighting.model_ambient = [0.2, 0.3, 0.4, 1.];
            transform.lighting.ambient = [0.5, 0.25, 0.1, 1.];
            transform.lighting.emission = [0.1, 0.2, 0.3, 1.];
            for (actual, expected) in render(&state, &raw, &transform)
                .into_iter()
                .zip([51u8, 70, 87, 178])
            {
                assert!(actual.abs_diff(expected) <= 1);
            }
            transform.lighting.model_ambient = [0.; 4];
            transform.lighting.ambient = [0.; 4];
            transform.lighting.emission = [0.; 4];
            transform.lighting.lights[0].enabled = true;
            let mut away = raw;
            for v in &mut away {
                v.normal = [0., 0., -1.];
            }
            assert_eq!(&render(&state, &away, &transform)[..3], &[0, 0, 0]);
            transform.normal_matrix =
                crate::lighting::normal_matrix(&crate::matrix::scale(2., 2., 2.));
            let dim = render(&state, &raw, &transform);
            assert!(dim[0].abs_diff(102) <= 1);
            transform.lighting.normalize = true;
            assert_eq!(render(&state, &raw, &transform), lit);
            transform.lighting.normalize = false;
            transform.lighting.rescale = true;
            assert_eq!(render(&state, &raw, &transform), lit);
            transform.normal_matrix = IDENTITY;
            transform.lighting.rescale = false;
            transform.lighting.color_material = true;
            assert_eq!(render(&state, &raw, &transform), [255, 0, 0, 255]);
            transform.lighting.color_material = false;
            transform.lighting.diffuse = [0., 0., 0., 0.7];
            transform.lighting.specular = [0.5, 0.25, 0., 1.];
            transform.lighting.shininess = 32.;
            let spec = render(&state, &raw, &transform);
            assert!(spec[0].abs_diff(128) <= 1 && spec[1].abs_diff(64) <= 1);
            transform.lighting.specular = [0.; 4];
            transform.lighting.diffuse = [0.8, 0.4, 0.2, 0.7];
            transform.lighting.lights[0].enabled = false;
            transform.lighting.lights[7].enabled = true;
            transform.lighting.lights[7].diffuse = [1.; 4];
            assert_eq!(render(&state, &raw, &transform), lit);
            transform.lighting.lights[7].position = [0., 0., 2., 1.];
            transform.lighting.lights[7].attenuation = [1., 0., 0.];
            let near = render(&state, &raw, &transform);
            transform.lighting.lights[7].attenuation = [1., 1., 0.];
            let attenuated = render(&state, &raw, &transform);
            assert!(near[0] > attenuated[0] && attenuated[0] > 0);
            transform.lighting.lights[7].spot_cutoff = 30.;
            transform.lighting.lights[7].spot_direction = [0., 0., 1.];
            assert_eq!(&render(&state, &raw, &transform)[..3], &[0, 0, 0]);
            transform.lighting.lights[7].spot_direction = [0., 0., -1.];
            assert!(render(&state, &raw, &transform)[0] > 0);
            transform.lighting.lights[7].spot_cutoff = 180.;
            transform.lighting.lights[7].position = [0., 0., -1., 0.];
            let back = [raw[1], raw[0], raw[2]];
            assert_eq!(&render(&state, &back, &transform)[..3], &[0, 0, 0]);
            transform.lighting.two_sided = true;
            assert_eq!(render(&state, &back, &transform), lit);
            transform.lighting.two_sided = false;
            transform.lighting.lights[7].position = [0., 0., 1., 0.];
            let mut mixed = raw;
            mixed[2].normal = [0., 0., -1.];
            state.smooth_shading = false;
            assert_eq!(&render(&state, &mixed, &transform)[..3], &[0, 0, 0]);
            state.smooth_shading = true;
            assert!(render(&state, &mixed, &transform)[0] > 0);
            transform.lighting.enabled = false;
            assert_eq!(render(&state, &raw, &transform), [255, 0, 0, 255]);
        }
    }

    fn center(t: &RenderTarget) -> [u8; 4] {
        t.color[(16 * 32 + 16) * 4..(16 * 32 + 16) * 4 + 4]
            .try_into()
            .unwrap()
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires a host EGL GLES2 driver; run explicitly"]
    fn host_es2_shaders_fog_scissor_masks_and_readback_orientation() {
        let mut gpu = Renderer::with_host(Host::es_for_test().unwrap(), 32, 32).unwrap();
        let mut target = RenderTarget::new(32, 32);
        let mut state = PipelineState {
            viewport: (0, 0, 32, 32),
            ..Default::default()
        };
        let textures = HashMap::new();
        gpu.clear(
            32,
            32,
            &state,
            [0., 0., 1., 1.],
            1.,
            GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
        )
        .unwrap();
        state.scissor = Some((0, 0, 32, 16));
        gpu.clear(32, 32, &state, [1., 0., 0., 1.], 1., GL_COLOR_BUFFER_BIT)
            .unwrap();
        gpu.read(&mut target).unwrap();
        assert_eq!(&target.color[..4], &[0, 0, 255, 255]);
        assert_eq!(
            &target.color[31 * 32 * 4..31 * 32 * 4 + 4],
            &[255, 0, 0, 255]
        );
        state.scissor = None;
        state.fog = true;
        state.fog_mode = FogMode::Linear;
        state.fog_start = 0.;
        state.fog_end = 2.;
        state.fog_color = [0., 0., 1., 1.];
        let mut vertices = triangle([1., 0., 0., 1.], 0.);
        for v in &mut vertices {
            v.fog_depth = 1.;
        }
        gpu.draw(
            &target,
            &state,
            GL_TRIANGLES,
            &vertices,
            &[],
            &textures,
            [0; 6],
        )
        .unwrap();
        gpu.read(&mut target).unwrap();
        let p = center(&target);
        assert!((126..=129).contains(&p[0]) && (126..=129).contains(&p[2]));
        state.fog = false;
        state.color_mask = [false, true, false, false];
        gpu.draw(
            &target,
            &state,
            GL_TRIANGLES,
            &triangle([0., 1., 0., 1.], 0.),
            &[],
            &textures,
            [0; 6],
        )
        .unwrap();
        gpu.read(&mut target).unwrap();
        let q = center(&target);
        assert_eq!(q[0], p[0]);
        assert_eq!(q[1], 255);
        assert_eq!(q[2], p[2]);
    }
    #[test]
    #[ignore = "requires a host OpenGL driver; run explicitly"]
    fn host_gpu_renders_depth_blend_alpha_textures_and_survives_context_switches() {
        let mut target = RenderTarget::new(32, 32);
        let mut state = PipelineState {
            viewport: (0, 0, 32, 32),
            ..Default::default()
        };
        let mut gpu = Renderer::new(32, 32).expect("host OpenGL initialization");
        let mut textures = HashMap::new();
        gpu.clear(
            32,
            32,
            &state,
            [0., 0., 0., 1.],
            1.,
            GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
        )
        .unwrap();
        gpu.draw(
            &target,
            &state,
            GL_TRIANGLES,
            &triangle([1., 0., 0., 1.], 0.),
            &[],
            &textures,
            [0; 6],
        )
        .unwrap();
        gpu.read(&mut target).unwrap();
        assert_eq!(center(&target), [255, 0, 0, 255]);
        state.depth_test = true;
        gpu.clear(
            32,
            32,
            &state,
            [0., 0., 0., 1.],
            1.,
            GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
        )
        .unwrap();
        gpu.draw(
            &target,
            &state,
            GL_TRIANGLES,
            &triangle([0., 1., 0., 1.], -0.5),
            &[],
            &textures,
            [0; 6],
        )
        .unwrap();
        gpu.draw(
            &target,
            &state,
            GL_TRIANGLES,
            &triangle([1., 0., 0., 1.], 0.5),
            &[],
            &textures,
            [0; 6],
        )
        .unwrap();
        gpu.read(&mut target).unwrap();
        assert_eq!(center(&target), [0, 255, 0, 255]);
        state.depth_test = false;
        state.alpha_test = true;
        state.alpha_func = CompareFunc::Greater;
        state.alpha_ref = 0.5;
        gpu.draw(
            &target,
            &state,
            GL_TRIANGLES,
            &triangle([1., 0., 0., 0.25], 0.),
            &[],
            &textures,
            [0; 6],
        )
        .unwrap();
        gpu.read(&mut target).unwrap();
        assert_eq!(center(&target), [0, 255, 0, 255]);
        state.alpha_test = false;
        state.blend = true;
        state.blend_src = BlendFactor::SrcAlpha;
        state.blend_dst = BlendFactor::OneMinusSrcAlpha;
        gpu.draw(
            &target,
            &state,
            GL_TRIANGLES,
            &triangle([1., 0., 0., 0.5], 0.),
            &[],
            &textures,
            [0; 6],
        )
        .unwrap();
        gpu.read(&mut target).unwrap();
        let p = center(&target);
        assert!((126..=129).contains(&p[0]) && (126..=129).contains(&p[1]));
        state.blend = false;
        textures.insert(
            1,
            Texture {
                width: 1,
                height: 1,
                rgba: vec![0, 0, 255, 255],
                revision: 1,
                ..Default::default()
            },
        );
        let stages = [TextureStage {
            unit: 0,
            env: TexEnvMode::Replace,
            env_color: [0.; 4],
            lod_bias: 0.0,
        }];
        gpu.draw(
            &target,
            &state,
            GL_TRIANGLES,
            &triangle([1.; 4], 0.),
            &stages,
            &textures,
            [1, 0, 0, 0, 0, 0],
        )
        .unwrap();
        gpu.read(&mut target).unwrap();
        assert_eq!(center(&target), [0, 0, 255, 255]);
        textures.get_mut(&1).unwrap().rgba = vec![255, 255, 0, 255];
        textures.get_mut(&1).unwrap().revision += 1;
        gpu.draw(
            &target,
            &state,
            GL_TRIANGLES,
            &triangle([1.; 4], 0.),
            &stages,
            &textures,
            [1, 0, 0, 0, 0, 0],
        )
        .unwrap();
        gpu.read(&mut target).unwrap();
        assert_eq!(center(&target), [255, 255, 0, 255]);
        // A child renderer uses the same EGL display; dropping it must not
        // terminate the display or delete any parent resources.
        for _ in 0..10 {
            let child = Renderer::new(16, 16).unwrap();
            drop(child);
            gpu.draw(
                &target,
                &state,
                GL_TRIANGLES,
                &triangle([1.; 4], 0.),
                &stages,
                &textures,
                [1, 0, 0, 0, 0, 0],
            )
            .unwrap();
            gpu.read(&mut target).unwrap();
            assert_eq!(center(&target), [255, 255, 0, 255]);
        }
        gpu.resize(64, 48).unwrap();
        target.resize(64, 48);
        gpu.read(&mut target).unwrap();
        assert_eq!(target.color.len(), 64 * 48 * 4);
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires native GL/GLES; Intel opaque depth tolerance and state restoration"]
    fn host_intel_depth_tolerance_preserves_depth_alpha_and_scaling() {
        for es in [false, true] {
            for format in [0x81a5, 0x81a6] {
                for scale in [1, 2] {
                    for rgb565 in [false, true] {
                        for reversed in [false, true] {
                            let mut gpu = if es {
                                Renderer::with_host(Host::es_for_test().unwrap(), 32, 32).unwrap()
                            } else {
                                Renderer::new(32, 32).unwrap()
                            };
                            gpu.depth_format = format;
                            gpu.resize(16, 16).unwrap();
                            gpu.resize(32, 32).unwrap();
                            gpu.set_render_scale(scale).unwrap();
                            gpu.set_surface_rgb565(rgb565).unwrap();
                            let error = if rgb565 { 5 } else { 1 };
                            let mut state = PipelineState {
                                viewport: (0, 0, 32, 32),
                                depth_test: true,
                                depth_func: if reversed {
                                    CompareFunc::Gequal
                                } else {
                                    CompareFunc::Lequal
                                },
                                intel2700g_depth_bias: true,
                                blend: true,
                                blend_src: BlendFactor::SrcAlpha,
                                blend_dst: BlendFactor::OneMinusSrcAlpha,
                                ..Default::default()
                            };
                            let mut cpu = RenderTarget::with_format(32, 32, rgb565);
                            let mut native = RenderTarget::with_format(32, 32, rgb565);
                            let clear = if reversed { 0. } else { 1. };
                            cpu.clear_depth(clear, None);
                            gpu.clear(
                                32,
                                32,
                                &state,
                                [0.; 4],
                                clear,
                                GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
                            )
                            .unwrap();
                            let sign = if reversed { -1. } else { 1. };
                            // The color replay can expose the slightly separated decal,
                            // but cannot move stored depth or accumulate the tolerance.
                            // A half-transparent overlay must blend only once.
                            for (z, color, expected) in [
                                (0., [0., 0., 1., 1.], [0, 0, 255]),
                                (sign * 0.0003, [1., 0., 0., 1.], [255, 0, 0]),
                                (sign * 0.0007, [0., 1., 0., 1.], [255, 0, 0]),
                                (sign * 0.0003, [0., 1., 0., 0.5], [255, 0, 0]),
                                (0., [0., 1., 0., 0.5], [128, 128, 0]),
                            ] {
                                let vertices = triangle(color, z);
                                raster::draw_triangle(
                                    &mut cpu,
                                    &state,
                                    &|_, _, _| None,
                                    &[],
                                    vertices,
                                );
                                let pass = state.intel2700g_depth_color_pass().unwrap();
                                raster::draw_triangle(
                                    &mut cpu,
                                    &pass,
                                    &|_, _, _| None,
                                    &[],
                                    vertices,
                                );
                                gpu.draw(
                                    &native,
                                    &state,
                                    GL_TRIANGLES,
                                    &vertices,
                                    &[],
                                    &HashMap::new(),
                                    [0; 6],
                                )
                                .unwrap();
                                gpu.read(&mut native).unwrap();
                                let at = (16 * 32 + 16) * 4;
                                for (actual, label) in [
                                    (&cpu.color[at..at + 3], "software"),
                                    (&native.color[at..at + 3], "native"),
                                ] {
                                    for (a, e) in actual.iter().zip(expected) {
                                        assert!(a.abs_diff(e)<=error,"{label} rgb565={rgb565} es={es} format={format:x} scale={scale} reversed={reversed} z={z} actual={actual:?}");
                                    }
                                }
                                assert_eq!(cpu.depth[16 * 32 + 16], 0.5);
                            }
                            // The extra pass must restore depth writes, alpha testing,
                            // polygon offset and uniform state before an ordinary draw.
                            state.intel2700g_depth_bias = false;
                            let nearer = triangle([0., 0., 1., 0.5], -sign * 0.01);
                            let hidden = triangle([1., 1., 1., 1.], -sign * 0.005);
                            for vertices in [nearer, hidden] {
                                raster::draw_triangle(
                                    &mut cpu,
                                    &state,
                                    &|_, _, _| None,
                                    &[],
                                    vertices,
                                );
                                gpu.draw(
                                    &native,
                                    &state,
                                    GL_TRIANGLES,
                                    &vertices,
                                    &[],
                                    &HashMap::new(),
                                    [0; 6],
                                )
                                .unwrap();
                            }
                            gpu.read(&mut native).unwrap();
                            let at = (16 * 32 + 16) * 4;
                            let result = native.color[at..at + 3].to_vec();
                            // Compare against ordinary coplanar draws on the same
                            // format. Sequential RGB565 blends round differently
                            // from RGBA8 and from software's intermediate rounding.
                            gpu.clear(
                                32,
                                32,
                                &state,
                                [0.; 4],
                                clear,
                                GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT,
                            )
                            .unwrap();
                            for vertices in [
                                triangle([0., 0., 1., 1.], 0.),
                                triangle([1., 0., 0., 1.], 0.),
                                triangle([0., 1., 0., 0.5], 0.),
                                nearer,
                                hidden,
                            ] {
                                gpu.draw(
                                    &native,
                                    &state,
                                    GL_TRIANGLES,
                                    &vertices,
                                    &[],
                                    &HashMap::new(),
                                    [0; 6],
                                )
                                .unwrap();
                            }
                            gpu.read(&mut native).unwrap();
                            assert_eq!(result, native.color[at..at + 3]);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod resolution_policy_tests {
    use super::*;
    #[test]
    fn selective_doubling_uses_logical_surface_size_in_both_orientations() {
        for size in [
            (320, 240),
            (240, 320),
            (640, 480),
            (480, 640),
            (800, 480),
            (16, 16),
        ] {
            assert_eq!(render_scale_for(1, size.0, size.1), 1);
            assert_eq!(render_scale_for(2, size.0, size.1), 2);
            assert_eq!(
                render_scale_for(3, size.0, size.1),
                if matches!(size, (320, 240) | (240, 320)) {
                    2
                } else {
                    1
                }
            );
            assert_eq!(
                render_scale_for(4, size.0, size.1),
                if matches!(size, (320, 240) | (240, 320)) {
                    3
                } else {
                    1
                }
            );
            assert_eq!(
                render_scale_for(5, size.0, size.1),
                if matches!(size, (320, 240) | (240, 320)) {
                    4
                } else {
                    1
                }
            );
            assert_eq!(render_scale_for(99, size.0, size.1), 1);
        }
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires native desktop GL and GLES2 contexts"]
    fn native_framebuffer_policy_tracks_surface_resizes_and_policy_changes() {
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::new_es_for_test(320, 240).unwrap()
            } else {
                Renderer::new(320, 240).unwrap()
            };
            gpu.set_render_scale(3).unwrap();
            assert_eq!((gpu.width, gpu.height), (640, 480));
            gpu.resize(640, 480).unwrap();
            assert_eq!((gpu.width, gpu.height), (640, 480));
            assert_eq!(gpu.render_scale, 1);
            gpu.set_render_scale(2).unwrap();
            assert_eq!((gpu.width, gpu.height), (1280, 960));
            gpu.set_render_scale(3).unwrap();
            assert_eq!((gpu.width, gpu.height), (640, 480));
            gpu.resize(240, 320).unwrap();
            assert_eq!((gpu.width, gpu.height), (480, 640));
            gpu.resize(480, 640).unwrap();
            assert_eq!((gpu.width, gpu.height), (480, 640));
            gpu.set_render_scale(2).unwrap();
            assert_eq!((gpu.width, gpu.height), (960, 1280));
            gpu.set_render_scale(1).unwrap();
            assert_eq!((gpu.width, gpu.height), (480, 640));
        }
    }
}

#[cfg(all(test, unix))]
mod nonzero_base_es2_tests {
    use super::*;
    #[test]
    #[ignore = "requires native GL/GLES; POT mip chain without original level zero under ES2 limits"]
    fn pot_base_is_not_mistaken_for_an_undefined_npot_level_zero() {
        for es in [false, true] {
            let mut gpu = if es {
                Renderer::new_es_for_test(16, 16).unwrap()
            } else {
                Renderer::new(16, 16).unwrap()
            };
            gpu.full_npot = false;
            let mut tex = Texture {
                base_level: 2,
                min_filter: Filter::NearestMipmapNearest,
                mag_filter: Filter::Nearest,
                ..Default::default()
            };
            for (l, w, color) in [
                (2, 4, [255, 0, 0, 255]),
                (3, 2, [0, 255, 0, 255]),
                (4, 1, [0, 0, 255, 255]),
            ] {
                tex.set_level(l, w, w, color.repeat((w * w) as usize));
            }
            assert!(tex.is_complete());
            assert_eq!(tex.width, 0);
            let textures = HashMap::from([(1, tex)]);
            let stages = [TextureStage {
                unit: 0,
                env: TexEnvMode::Replace,
                env_color: [0.; 4],
                lod_bias: 0.,
            }];
            let tri = [[-1., -1., 0., 1.], [1., -1., 0., 1.], [0., 1., 0., 1.]].map(|pos| Vertex {
                pos,
                texcoord: [[(pos[0] + 1.) * 64., (pos[1] + 1.) * 64., 0., 1.]; 6],
                ..Default::default()
            });
            let mut target = RenderTarget::new(16, 16);
            gpu.draw(
                &target,
                &PipelineState {
                    viewport: (0, 0, 16, 16),
                    ..Default::default()
                },
                GL_TRIANGLES,
                &tri,
                &stages,
                &textures,
                [1, 0, 0, 0, 0, 0],
            )
            .unwrap();
            gpu.read(&mut target).unwrap();
            assert_eq!(
                &target.color[(8 * 16 + 7) * 4..(8 * 16 + 7) * 4 + 4],
                &[0, 0, 255, 255]
            );
        }
    }
}
