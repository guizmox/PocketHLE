//! Native programmable resources and context state at the checkpoint boundary.
//! Resource readback happens only on explicit capture.
use super::*;
use serde::{Deserialize, Serialize};

const DEPTH_STENCIL: u32 = 0x84f9;
const UNSIGNED_INT_24_8: u32 = 0x84fa;
const MAX_CHECKPOINT_BYTES: usize = 256 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
pub(super) struct NativeCheckpoint {
    shaders: Vec<Shader>,
    programs: Vec<Program>,
    buffers: Vec<Buffer>,
    textures: Vec<Texture>,
    renderbuffers: Vec<Renderbuffer>,
    framebuffers: Vec<Framebuffer>,
    retained: Vec<RetainedPlane>,
    window: Image,
    state: State,
    client: Vec<(u32, ClientAttribute)>,
    configured: Vec<u32>,
    error: u32,
}

#[derive(Serialize, Deserialize)]
struct Shader {
    guest: u32,
    kind: u32,
    source: Vec<u8>,
    guest_source: Option<Vec<u8>>,
    executable: Option<CompiledShader>,
    compiled: bool,
    deleted: bool,
}

#[derive(Serialize, Deserialize)]
struct Program {
    guest: u32,
    shaders: Vec<u32>,
    executable: Vec<CompiledShader>,
    attributes: Vec<(Vec<u8>, u32)>,
    uniforms: Vec<Uniform>,
    linked: bool,
    validated: bool,
    deleted: bool,
}

#[derive(Serialize, Deserialize)]
struct Uniform {
    name: Vec<u8>,
    location: i32,
    kind: u32,
    values: Vec<u32>,
}

#[derive(Serialize, Deserialize)]
struct Buffer {
    guest: u32,
    usage: u32,
    #[serde(with = "serde_bytes")]
    bytes: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct Texture {
    guest: u32,
    target: u32,
    parameters: [i32; 4],
    levels: Vec<(u32, i32, Image)>,
}

#[derive(Serialize, Deserialize)]
struct Renderbuffer {
    guest: u32,
    image: Image,
}

#[derive(Serialize, Deserialize)]
struct RetainedPlane {
    id: u64,
    kind: u32,
    guest: u32,
    face: u32,
    level: i32,
    image: Image,
}

#[derive(Serialize, Deserialize)]
struct Framebuffer {
    guest: u32,
    attachments: Vec<Attachment>,
}

#[derive(Serialize, Deserialize)]
struct Attachment {
    retained: Option<u64>,
    point: u32,
    kind: u32,
    guest: u32,
    level: i32,
    face: u32,
}

#[derive(Serialize, Deserialize)]
struct Image {
    width: i32,
    height: i32,
    internal: u32,
    format: u32,
    kind: u32,
    compressed: bool,
    #[serde(with = "serde_bytes")]
    bytes: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Attribute {
    index: u32,
    enabled: bool,
    size: i32,
    kind: u32,
    normalized: i32,
    stride: i32,
    buffer: u32,
    pointer: u32,
    value: [f32; 4],
}

#[derive(Serialize, Deserialize)]
struct State {
    integers: Vec<(u32, Vec<i32>)>,
    floats: Vec<(u32, Vec<f32>)>,
    enables: Vec<(u32, bool)>,
    textures: Vec<(u32, u32)>,
    attributes: Vec<Attribute>,
}

fn integer(renderer: &mut Renderer, parameter: u32) -> Result<i32> {
    let mut value = 0;
    invoke!(renderer, "glGetIntegerv", (parameter => u32, &mut value => *mut i32), ());
    Ok(value)
}

fn shader_parameter(renderer: &mut Renderer, shader: u32, parameter: u32) -> Result<i32> {
    let mut value = 0;
    invoke!(renderer, "glGetShaderiv", (shader => u32, parameter => u32, &mut value => *mut i32), ());
    Ok(value)
}

fn program_parameter(renderer: &mut Renderer, program: u32, parameter: u32) -> Result<i32> {
    let mut value = 0;
    invoke!(renderer, "glGetProgramiv", (program => u32, parameter => u32, &mut value => *mut i32), ());
    Ok(value)
}

fn guest(renderer: &Renderer, kind: Kind, host: u32) -> u32 {
    renderer
        .names
        .as_ref()
        .expect("renderer names")
        .guest(kind, host)
}

fn host(renderer: &Renderer, kind: Kind, guest: u32) -> u32 {
    renderer
        .names
        .as_ref()
        .expect("renderer names")
        .host(kind, guest)
}

fn uniform_components(kind: u32) -> Result<(usize, bool)> {
    match kind {
        0x1406 => Ok((1, false)),
        0x8b50 => Ok((2, false)),
        0x8b51 => Ok((3, false)),
        0x8b52 => Ok((4, false)),
        0x1404 | 0x8b56 | 0x8b5e | 0x8b60 => Ok((1, true)),
        0x8b53 | 0x8b57 => Ok((2, true)),
        0x8b54 | 0x8b58 => Ok((3, true)),
        0x8b55 | 0x8b59 => Ok((4, true)),
        0x8b5a => Ok((4, false)),
        0x8b5b => Ok((9, false)),
        0x8b5c => Ok((16, false)),
        _ => Err(error("unsupported programmable snapshot uniform type")),
    }
}

fn check_error(renderer: &mut Renderer, stage: &str) -> Result<()> {
    let pending = invoke!(renderer, "glGetError", (), u32);
    if pending != 0 {
        return Err(error(format!(
            "programmable snapshot {stage}: GL error {pending:#x}"
        )));
    }
    Ok(())
}

pub(super) fn image_format(internal: u32) -> u32 {
    match internal {
        0x81a5 | 0x81a6 | 0x81a7 | 0x1902 => 0x1902,
        0x88f0 | DEPTH_STENCIL => DEPTH_STENCIL,
        0x8d48 | 0x1901 => 0x1901,
        _ => RGBA,
    }
}

impl Image {
    fn new(width: i32, height: i32, internal: u32) -> Result<Self> {
        let format = image_format(internal);
        let (kind, pixel_bytes) = match format {
            0x1902 => (0x1405, 4),
            DEPTH_STENCIL => (UNSIGNED_INT_24_8, 4),
            0x1901 => (UNSIGNED_BYTE, 1),
            _ => (UNSIGNED_BYTE, 4),
        };
        let size = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(pixel_bytes))
            .filter(|&bytes| bytes <= MAX_TRANSFER)
            .ok_or_else(|| error("invalid programmable snapshot image dimensions"))?;
        Ok(Self {
            width,
            height,
            internal,
            format,
            kind,
            compressed: false,
            bytes: vec![0; size],
        })
    }

    fn upload(&self, renderer: &mut Renderer, target: u32, level: i32) -> Result<()> {
        if self.compressed {
            if self.width <= 0 || self.height <= 0 || self.bytes.len() > MAX_CHECKPOINT_BYTES {
                return Err(error("invalid compressed snapshot image"));
            }
            invoke!(renderer, "glCompressedTexImage2D", (target => u32, level => i32, self.internal => u32,
                self.width => i32, self.height => i32, 0 => i32, self.bytes.len() as i32 => i32,
                self.bytes.as_ptr().cast() => *const c_void), ());
            return Ok(());
        }
        let expected = Self::new(self.width, self.height, self.internal)?;
        if expected.bytes.len() != self.bytes.len()
            || expected.format != self.format
            || expected.kind != self.kind
        {
            return Err(error("invalid programmable snapshot image data"));
        }
        invoke!(renderer, "glTexImage2D", (target => u32, level => i32, self.internal as i32 => i32,
            self.width => i32, self.height => i32, 0 => i32, self.format => u32,
            self.kind => u32, self.bytes.as_ptr().cast() => *const c_void), ());
        Ok(())
    }
}

impl Renderer {
    pub(super) fn remember_compiled_shader(&mut self, shader: u32) -> Result<()> {
        if invoke!(self, "glIsShader", (shader => u32), u8) == 0 {
            return Ok(());
        }
        if shader_parameter(self, shader, 0x8b81)? == 0 {
            return Ok(());
        }
        let kind = shader_parameter(self, shader, 0x8b4f)? as u32;
        let capacity = shader_parameter(self, shader, 0x8b88)?.max(1);
        let mut source = vec![0; transfer_size(capacity, 1)?];
        let mut length = 0;
        invoke!(self, "glGetShaderSource", (shader => u32, capacity => i32, &mut length => *mut i32, source.as_mut_ptr().cast() => *mut c_char), ());
        source.truncate(length.max(0) as usize);
        self.compiled_shaders
            .insert(shader, CompiledShader { kind, source });
        Ok(())
    }

    pub(super) fn remember_linked_program(&mut self, program: u32) -> Result<()> {
        if invoke!(self, "glIsProgram", (program => u32), u8) == 0 {
            return Ok(());
        }
        if program_parameter(self, program, 0x8b82)? == 0 {
            return Ok(());
        }
        let count = program_parameter(self, program, 0x8b85)?;
        let mut shaders = vec![0; transfer_size(count, 4)? / 4];
        invoke!(self, "glGetAttachedShaders", (program => u32, count => i32, std::ptr::null_mut() => *mut i32, shaders.as_mut_ptr() => *mut u32), ());
        let executables = shaders
            .iter()
            .map(|shader| {
                self.compiled_shaders
                    .get(shader)
                    .cloned()
                    .ok_or_else(|| error("linked shader has no compiled source"))
            })
            .collect::<Result<Vec<_>>>()?;
        self.linked_programs.insert(program, executables);
        Ok(())
    }
}

impl NativeCheckpoint {
    pub(super) fn capture(renderer: &mut Renderer) -> Result<Self> {
        renderer.host.make_programmable_current().map_err(error)?;
        // Guest errors belong to the saved context; errors raised by our own
        // queries must instead fail capture, never leak back into the game.
        for _ in 0..16 {
            let pending = invoke!(renderer, "glGetError", (), u32);
            if pending == 0 {
                break;
            }
            renderer.reject(pending);
        }
        let state = State::capture(renderer)?;
        let pending = invoke!(renderer, "glGetError", (), u32);
        if pending != 0 {
            return Err(error(format!(
                "programmable state capture failed with GL error {pending:#x}"
            )));
        }
        let result = Self::capture_resources(renderer, state.clone_for_readback());
        let restored = state.apply(renderer);
        restored?;
        result
    }

    fn capture_resources(renderer: &mut Renderer, state: State) -> Result<Self> {
        let mut checkpoint = Self {
            shaders: Vec::new(),
            programs: Vec::new(),
            buffers: Vec::new(),
            textures: Vec::new(),
            renderbuffers: Vec::new(),
            framebuffers: Vec::new(),
            retained: Vec::new(),
            window: Image::new(
                renderer.render_size().0 as i32,
                renderer.render_size().1 as i32,
                RGBA,
            )?,
            state,
            client: renderer
                .client_attributes
                .iter()
                .map(|(&id, &value)| (id, value))
                .collect(),
            configured: renderer.configured_attributes.iter().copied().collect(),
            error: renderer.guest_error,
        };
        if let Some(reason) = &renderer.retained.failure {
            return Err(error(reason));
        }
        let mut budget = MAX_CHECKPOINT_BYTES;
        let retained: Vec<_> = renderer
            .retained
            .images
            .iter()
            .map(|(&id, image)| (id, image.clone()))
            .collect();
        for (id, kept) in retained {
            let mut image = Image::new(kept.width, kept.height, kept.internal)?;
            budget = budget
                .checked_sub(image.bytes.len())
                .ok_or_else(|| error("programmable snapshot exceeds 256 MiB"))?;
            invoke!(renderer,"glBindFramebuffer",(FRAMEBUFFER=>u32,kept.keeper=>u32),());
            invoke!(renderer,"glReadPixels",(0=>i32,0=>i32,image.width=>i32,image.height=>i32,
                image.format=>u32,image.kind=>u32,image.bytes.as_mut_ptr().cast()=>*mut c_void),());
            check_error(renderer, "retained framebuffer image capture")?;
            checkpoint.retained.push(RetainedPlane {
                id,
                kind: kept.kind,
                guest: kept.guest,
                face: kept.face,
                level: kept.level,
                image,
            });
        }

        for (guest, shader) in renderer.names.as_ref().unwrap().pairs(Kind::Shader) {
            if invoke!(renderer, "glIsShader", (shader => u32), u8) == 0 {
                continue;
            }
            let kind = shader_parameter(renderer, shader, 0x8b4f)? as u32;
            let size = shader_parameter(renderer, shader, 0x8b88)?.max(1);
            let mut source = vec![0; transfer_size(size, 1)?];
            let mut length = 0;
            invoke!(renderer, "glGetShaderSource", (shader => u32, size => i32, &mut length => *mut i32, source.as_mut_ptr().cast() => *mut c_char), ());
            source.truncate(length.max(0) as usize);
            checkpoint.shaders.push(Shader {
                guest,
                kind,
                source,
                guest_source: renderer.desktop_shader_sources.get(&shader).cloned(),
                executable: renderer.compiled_shaders.get(&shader).cloned(),
                compiled: shader_parameter(renderer, shader, 0x8b81)? != 0,
                deleted: shader_parameter(renderer, shader, 0x8b80)? != 0,
            });
        }
        check_error(renderer, "shader capture")?;
        for (guest_id, program) in renderer.names.as_ref().unwrap().pairs(Kind::Program) {
            if invoke!(renderer, "glIsProgram", (program => u32), u8) == 0 {
                continue;
            }
            let mut saved = Program {
                guest: guest_id,
                shaders: Vec::new(),
                executable: renderer
                    .linked_programs
                    .get(&program)
                    .cloned()
                    .unwrap_or_default(),
                attributes: Vec::new(),
                uniforms: Vec::new(),
                linked: program_parameter(renderer, program, 0x8b82)? != 0,
                validated: program_parameter(renderer, program, 0x8b83)? != 0,
                deleted: program_parameter(renderer, program, 0x8b80)? != 0,
            };
            if !saved.linked
                && !saved.executable.is_empty()
                && checkpoint.state.int(0x8b8d)? as u32 == guest_id
            {
                return Err(error(
                    "snapshot contains an active program whose last link failed",
                ));
            }
            let count = program_parameter(renderer, program, 0x8b85)?;
            let mut shaders = vec![0; transfer_size(count, 4)? / 4];
            invoke!(renderer, "glGetAttachedShaders", (program => u32, count => i32, std::ptr::null_mut() => *mut i32, shaders.as_mut_ptr() => *mut u32), ());
            saved.shaders = shaders
                .into_iter()
                .map(|shader| guest(renderer, Kind::Shader, shader))
                .collect();
            if saved.linked {
                for (query, size_query, function, uniform) in [
                    (0x8b89, 0x8b8a, "glGetActiveAttrib", false),
                    (0x8b86, 0x8b87, "glGetActiveUniform", true),
                ] {
                    let count = program_parameter(renderer, program, query)?;
                    let capacity = program_parameter(renderer, program, size_query)?.max(1);
                    for index in 0..count {
                        let mut name = vec![0; transfer_size(capacity, 1)?];
                        let (mut length, mut count, mut kind) = (0, 0, 0);
                        invoke!(renderer, function, (program => u32, index as u32 => u32, capacity => i32,
                            &mut length => *mut i32, &mut count => *mut i32, &mut kind => *mut u32,
                            name.as_mut_ptr().cast() => *mut c_char), ());
                        name.truncate(length.max(0) as usize);
                        if !uniform {
                            let c_name = CString::new(name.clone())
                                .map_err(|_| error("invalid attribute name"))?;
                            let location = invoke!(renderer, "glGetAttribLocation", (program => u32, c_name.as_ptr() => *const c_char), i32);
                            if location >= 0 {
                                saved.attributes.push((name, location as u32));
                            }
                            continue;
                        }
                        let (components, integer) = uniform_components(kind)?;
                        for element in 0..count {
                            let element_name = if count > 1 {
                                let base = String::from_utf8_lossy(&name);
                                if base.contains("[0]") {
                                    base.replacen("[0]", &format!("[{element}]"), 1)
                                        .into_bytes()
                                } else {
                                    format!("{base}[{element}]").into_bytes()
                                }
                            } else {
                                name.clone()
                            };
                            let c_name = CString::new(element_name.clone())
                                .map_err(|_| error("invalid uniform name"))?;
                            let location = invoke!(renderer, "glGetUniformLocation", (program => u32, c_name.as_ptr() => *const c_char), i32);
                            if location < 0 {
                                continue;
                            }
                            let mut values = vec![0u32; components];
                            if integer {
                                invoke!(renderer, "glGetUniformiv", (program => u32, location => i32, values.as_mut_ptr().cast() => *mut i32), ());
                            } else {
                                invoke!(renderer, "glGetUniformfv", (program => u32, location => i32, values.as_mut_ptr().cast() => *mut f32), ());
                            }
                            saved.uniforms.push(Uniform {
                                name: element_name,
                                location,
                                kind,
                                values,
                            });
                        }
                    }
                }
            }
            checkpoint.programs.push(saved);
        }
        check_error(renderer, "program capture")?;
        invoke!(renderer, "glPixelStorei", (0x0d05 => u32, 1 => i32), ());
        for (guest, buffer) in renderer.names.as_ref().unwrap().pairs(Kind::Buffer) {
            if invoke!(renderer, "glIsBuffer", (buffer => u32), u8) == 0 {
                continue;
            }
            invoke!(renderer, "glBindBuffer", (ARRAY_BUFFER => u32, buffer => u32), ());
            let (mut size, mut usage) = (0i32, 0i32);
            invoke!(renderer, "glGetBufferParameteriv", (ARRAY_BUFFER => u32, 0x8764 => u32, &mut size => *mut i32), ());
            invoke!(renderer, "glGetBufferParameteriv", (ARRAY_BUFFER => u32, 0x8765 => u32, &mut usage => *mut i32), ());
            let mut bytes = vec![0; transfer_size(size, 1)?];
            budget = budget
                .checked_sub(bytes.len())
                .ok_or_else(|| error("programmable snapshot exceeds 256 MiB"))?;
            invoke!(renderer, "glGetBufferSubData", (ARRAY_BUFFER => u32, 0 => isize, bytes.len() as isize => isize, bytes.as_mut_ptr().cast() => *mut c_void), ());
            checkpoint.buffers.push(Buffer {
                guest,
                usage: usage as u32,
                bytes,
            });
        }
        check_error(renderer, "buffer capture")?;
        invoke!(renderer, "glActiveTexture", (GL_TEXTURE0 => u32), ());
        for (guest, texture) in renderer
            .guest_textures
            .iter()
            .map(|&host| (guest(renderer, Kind::Texture, host), host))
            .collect::<Vec<_>>()
        {
            if invoke!(renderer, "glIsTexture", (texture => u32), u8) == 0 {
                continue;
            }
            let target = checkpoint
                .state
                .textures
                .iter()
                .find_map(|&(two, cube)| {
                    if two == guest {
                        Some(TEXTURE_2D)
                    } else if cube == guest {
                        Some(0x8513)
                    } else {
                        None
                    }
                })
                .unwrap_or(TEXTURE_2D);
            // Object target is immutable. Probe the alternate target when the
            // texture is currently unbound and the first bind is rejected.
            invoke!(renderer, "glBindTexture", (target => u32, texture => u32), ());
            let target = if invoke!(renderer, "glGetError", (), u32) != 0 {
                let alternate = if target == TEXTURE_2D {
                    0x8513
                } else {
                    TEXTURE_2D
                };
                invoke!(renderer, "glBindTexture", (alternate => u32, texture => u32), ());
                alternate
            } else {
                target
            };
            check_error(
                renderer,
                &format!("texture {guest} target {target:#x} bind"),
            )?;
            let mut parameters = [0; 4];
            for (pname, value) in [0x2800, 0x2801, 0x2802, 0x2803]
                .into_iter()
                .zip(&mut parameters)
            {
                invoke!(renderer, "glGetTexParameteriv", (target => u32, pname => u32, value => *mut i32), ());
            }
            let mut levels = Vec::new();
            let maximum = integer(renderer, if target == TEXTURE_2D { 0x0d33 } else { 0x851c })?;
            let last_level = u32::try_from(maximum)
                .ok()
                .filter(|&v| v != 0)
                .ok_or_else(|| error("invalid host texture size limit"))?
                .ilog2() as i32;
            for face in if target == TEXTURE_2D {
                vec![target]
            } else {
                (0x8515..=0x851a).collect()
            } {
                for level in 0..=last_level {
                    let (mut width, mut height, mut internal) = (0, 0, 0);
                    for (pname, value) in [
                        (0x1000, &mut width),
                        (0x1001, &mut height),
                        (0x1003, &mut internal),
                    ] {
                        invoke!(renderer, "glGetTexLevelParameteriv", (face => u32, level => i32, pname => u32, value => *mut i32), ());
                    }
                    check_error(
                        renderer,
                        &format!("texture {guest} target {face:#x} level {level} query"),
                    )?;
                    if width == 0 || height == 0 {
                        continue;
                    }
                    let mut image = Image::new(width, height, internal as u32)?;
                    let mut compressed = 0;
                    invoke!(renderer, "glGetTexLevelParameteriv", (face => u32, level => i32, 0x86a1 => u32, &mut compressed => *mut i32), ());
                    if compressed != 0 {
                        let mut length = 0;
                        invoke!(renderer, "glGetTexLevelParameteriv", (face => u32, level => i32, 0x86a0 => u32, &mut length => *mut i32), ());
                        image.bytes = vec![0; transfer_size(length, 1)?];
                        image.compressed = true;
                    }
                    budget = budget
                        .checked_sub(image.bytes.len())
                        .ok_or_else(|| error("programmable snapshot exceeds 256 MiB"))?;
                    if image.compressed {
                        invoke!(renderer, "glGetCompressedTexImage", (face => u32, level => i32, image.bytes.as_mut_ptr().cast() => *mut c_void), ());
                    } else {
                        invoke!(renderer, "glGetTexImage", (face => u32, level => i32, image.format => u32,
                            image.kind => u32, image.bytes.as_mut_ptr().cast() => *mut c_void), ());
                    }
                    check_error(renderer, &format!("texture {guest} target {face:#x} level {level} internal {internal:#x} capture"))?;
                    levels.push((face, level, image));
                }
            }
            checkpoint.textures.push(Texture {
                guest,
                target,
                parameters,
                levels,
            });
        }
        check_error(renderer, "texture capture")?;
        for (guest_id, framebuffer) in renderer
            .guest_framebuffers
            .iter()
            .map(|&host| (guest(renderer, Kind::Framebuffer, host), host))
            .collect::<Vec<_>>()
        {
            if invoke!(renderer, "glIsFramebuffer", (framebuffer => u32), u8) == 0 {
                continue;
            }
            invoke!(renderer, "glBindFramebuffer", (FRAMEBUFFER => u32, framebuffer => u32), ());
            let mut attachments = Vec::new();
            for point in [0x8ce0, 0x8d00, 0x8d20] {
                if let Some(&id) = renderer.retained.attachments.get(&(framebuffer, point)) {
                    let kept = &renderer.retained.images[&id];
                    attachments.push(Attachment {
                        retained: Some(id),
                        point,
                        kind: kept.kind,
                        guest: kept.guest,
                        level: kept.level,
                        face: kept.face,
                    });
                    continue;
                }
                let query = |renderer: &mut Renderer, parameter| -> Result<i32> {
                    let mut value = 0;
                    invoke!(renderer, "glGetFramebufferAttachmentParameteriv", (FRAMEBUFFER => u32, point => u32, parameter => u32, &mut value => *mut i32), ());
                    Ok(value)
                };
                let kind = query(renderer, 0x8cd0)? as u32;
                if kind == 0 {
                    continue;
                }
                let object = query(renderer, 0x8cd1)? as u32;
                let namespace = if kind == RENDERBUFFER {
                    Kind::Renderbuffer
                } else {
                    Kind::Texture
                };
                if (kind == RENDERBUFFER && !renderer.guest_renderbuffers.contains(&object))
                    || (kind != RENDERBUFFER && !renderer.guest_textures.contains(&object))
                {
                    return Err(error("snapshot is missing a retained framebuffer image"));
                }
                attachments.push(Attachment {
                    retained: None,
                    point,
                    kind,
                    guest: guest(renderer, namespace, object),
                    level: if kind == RENDERBUFFER {
                        0
                    } else {
                        query(renderer, 0x8cd2)?
                    },
                    face: if kind == RENDERBUFFER {
                        0
                    } else {
                        query(renderer, 0x8cd3)? as u32
                    },
                });
            }
            checkpoint.framebuffers.push(Framebuffer {
                guest: guest_id,
                attachments,
            });
        }
        let mut scratch = 0;
        invoke!(renderer, "glGenFramebuffers", (1 => i32, &mut scratch => *mut u32), ());
        let captured = (|| {
            for (guest, buffer) in renderer
                .guest_renderbuffers
                .iter()
                .map(|&host| (guest(renderer, Kind::Renderbuffer, host), host))
                .collect::<Vec<_>>()
            {
                if invoke!(renderer, "glIsRenderbuffer", (buffer => u32), u8) == 0 {
                    continue;
                }
                invoke!(renderer, "glBindRenderbuffer", (RENDERBUFFER => u32, buffer => u32), ());
                let mut size = [0; 3];
                for (pname, value) in [0x8d42, 0x8d43, 0x8d44].into_iter().zip(&mut size) {
                    invoke!(renderer, "glGetRenderbufferParameteriv", (RENDERBUFFER => u32, pname => u32, value => *mut i32), ());
                }
                if size[0] == 0 || size[1] == 0 {
                    continue;
                }
                let image =
                    read_renderbuffer(renderer, scratch, buffer, size[0], size[1], size[2] as u32)?;
                budget = budget
                    .checked_sub(image.bytes.len())
                    .ok_or_else(|| error("programmable snapshot exceeds 256 MiB"))?;
                checkpoint.renderbuffers.push(Renderbuffer { guest, image });
            }
            invoke!(renderer, "glBindFramebuffer", (FRAMEBUFFER => u32, renderer.default_fbo => u32), ());
            invoke!(renderer, "glReadPixels", (0 => i32, 0 => i32, checkpoint.window.width => i32,
                checkpoint.window.height => i32, RGBA => u32, UNSIGNED_BYTE => u32,
                checkpoint.window.bytes.as_mut_ptr().cast() => *mut c_void), ());
            let window_depth = read_renderbuffer(
                renderer,
                scratch,
                renderer.window_depth_stencil,
                checkpoint.window.width,
                checkpoint.window.height,
                0x88f0,
            )?;
            check_error(renderer, "window image capture")?;
            checkpoint.renderbuffers.push(Renderbuffer {
                guest: 0,
                image: window_depth,
            });
            let gl_error = invoke!(renderer, "glGetError", (), u32);
            if gl_error != 0 {
                return Err(error(format!(
                    "programmable resource capture failed with GL error {gl_error:#x}"
                )));
            }
            Ok(())
        })();
        invoke!(renderer, "glDeleteFramebuffers", (1 => i32, &scratch => *const u32), ());
        captured?;
        Ok(checkpoint)
    }
}

pub(super) fn attachment_point(format: u32) -> u32 {
    match format {
        DEPTH_STENCIL => 0x821a,
        0x1902 => 0x8d00,
        0x1901 => 0x8d20,
        _ => 0x8ce0,
    }
}

fn read_renderbuffer(
    renderer: &mut Renderer,
    scratch: u32,
    buffer: u32,
    width: i32,
    height: i32,
    internal: u32,
) -> Result<Image> {
    let mut image = Image::new(width, height, internal)?;
    invoke!(renderer, "glBindFramebuffer", (FRAMEBUFFER => u32, scratch => u32), ());
    // Detach the previous scratch attachments so a size or format mismatch
    // cannot affect completeness of the next read.
    for point in [0x8ce0, 0x8d00, 0x8d20] {
        invoke!(renderer, "glFramebufferRenderbuffer", (FRAMEBUFFER => u32, point => u32, RENDERBUFFER => u32, 0 => u32), ());
    }
    let point = attachment_point(image.format);
    invoke!(renderer, "glFramebufferRenderbuffer", (FRAMEBUFFER => u32, point => u32, RENDERBUFFER => u32, buffer => u32), ());
    let color = if image.format == RGBA { 0x8ce0 } else { 0 };
    invoke!(renderer, "glDrawBuffer", (color => u32), ());
    invoke!(renderer, "glReadBuffer", (color => u32), ());
    if invoke!(renderer, "glCheckFramebufferStatus", (FRAMEBUFFER => u32), u32) != 0x8cd5 {
        return Err(error("programmable snapshot renderbuffer is incomplete"));
    }
    invoke!(renderer, "glReadPixels", (0 => i32, 0 => i32, width => i32, height => i32,
        image.format => u32, image.kind => u32, image.bytes.as_mut_ptr().cast() => *mut c_void), ());
    Ok(image)
}

impl State {
    fn clone_for_readback(&self) -> Self {
        Self {
            integers: self.integers.clone(),
            floats: self.floats.clone(),
            enables: self.enables.clone(),
            textures: self.textures.clone(),
            attributes: self.attributes.clone(),
        }
    }

    fn capture(renderer: &mut Renderer) -> Result<Self> {
        let mut state = Self {
            integers: Vec::new(),
            floats: Vec::new(),
            enables: Vec::new(),
            textures: Vec::new(),
            attributes: Vec::new(),
        };
        for (parameter, count) in [
            (0x0ba2, 4),
            (0x0c10, 4),
            (0x0c23, 4),
            (0x8009, 1),
            (0x883d, 1),
            (0x80c8, 1),
            (0x80c9, 1),
            (0x80ca, 1),
            (0x80cb, 1),
            (0x0b45, 1),
            (0x0b46, 1),
            (0x0b72, 1),
            (0x0b74, 1),
            (0x0b91, 1),
            (0x0b92, 1),
            (0x0b93, 1),
            (0x0b94, 1),
            (0x0b95, 1),
            (0x0b96, 1),
            (0x0b97, 1),
            (0x0b98, 1),
            (0x8800, 1),
            (0x8801, 1),
            (0x8802, 1),
            (0x8803, 1),
            (0x8ca3, 1),
            (0x8ca4, 1),
            (0x8ca5, 1),
            (0x0d05, 1),
            (0x0cf5, 1),
            (0x84e0, 1),
            (0x8894, 1),
            (0x8895, 1),
            (0x8b8d, 1),
            (0x80ab, 1),
            (0x8192, 1),
            (0x8ca6, 1),
            (0x8ca7, 1),
        ] {
            let mut values = vec![0; count];
            invoke!(renderer, "glGetIntegerv", (parameter => u32, values.as_mut_ptr() => *mut i32), ());
            let object = match parameter {
                0x8894 | 0x8895 => Some(Kind::Buffer),
                0x8b8d => Some(Kind::Program),
                0x8ca7 => Some(Kind::Renderbuffer),
                0x8ca6 => Some(Kind::Framebuffer),
                _ => None,
            };
            if parameter == 0x8ca6 && renderer.framebuffer == 0 {
                values[0] = 0;
            } else if let Some(kind) = object {
                values[0] = guest(renderer, kind, values[0] as u32) as i32;
            }
            state.integers.push((parameter, values));
        }
        for (parameter, count) in [
            (0x0c22, 4),
            (0x8005, 4),
            (0x0b73, 1),
            (0x0b70, 2),
            (0x0b21, 1),
            (0x8038, 1),
            (0x2a00, 1),
            (0x80aa, 1),
        ] {
            let mut values = vec![0.; count];
            invoke!(renderer, "glGetFloatv", (parameter => u32, values.as_mut_ptr() => *mut f32), ());
            state.floats.push((parameter, values));
        }
        for capability in [
            0x0be2, 0x0b44, 0x0b71, 0x0b90, 0x0c11, 0x0bd0, 0x8037, 0x809e, 0x80a0,
        ] {
            let enabled = invoke!(renderer, "glIsEnabled", (capability => u32), u8) != 0;
            state.enables.push((capability, enabled));
        }
        let active = state.int(0x84e0)? as u32;
        for unit in 0..renderer.texture_units {
            invoke!(renderer, "glActiveTexture", (GL_TEXTURE0 + unit => u32), ());
            let two = integer(renderer, 0x8069)? as u32;
            let cube = integer(renderer, 0x8514)? as u32;
            state.textures.push((
                guest(renderer, Kind::Texture, two),
                guest(renderer, Kind::Texture, cube),
            ));
        }
        invoke!(renderer, "glActiveTexture", (active => u32), ());
        for index in 0..renderer.max_attributes {
            let query = |renderer: &mut Renderer, parameter| -> Result<i32> {
                let mut value = 0;
                invoke!(renderer, "glGetVertexAttribiv", (index => u32, parameter => u32, &mut value => *mut i32), ());
                Ok(value)
            };
            let mut pointer = std::ptr::null_mut();
            invoke!(renderer, "glGetVertexAttribPointerv", (index => u32, 0x8645 => u32, &mut pointer => *mut *mut c_void), ());
            let mut value = renderer.attribute_zero;
            // Desktop compatibility contexts reserve attribute zero for the
            // conventional vertex position; querying its current value is invalid.
            if index != 0 {
                invoke!(renderer, "glGetVertexAttribfv", (index => u32, 0x8626 => u32, value.as_mut_ptr() => *mut f32), ());
            }
            let buffer = query(renderer, 0x889f)? as u32;
            state.attributes.push(Attribute {
                index,
                enabled: query(renderer, 0x8622)? != 0,
                size: query(renderer, 0x8623)?,
                kind: query(renderer, 0x8625)? as u32,
                normalized: query(renderer, 0x886a)?,
                stride: query(renderer, 0x8624)?,
                buffer: guest(renderer, Kind::Buffer, buffer),
                pointer: pointer as usize as u32,
                value,
            });
        }
        Ok(state)
    }

    fn int(&self, parameter: u32) -> Result<i32> {
        self.integers
            .iter()
            .find(|(key, _)| *key == parameter)
            .and_then(|(_, values)| values.first())
            .copied()
            .ok_or_else(|| error("missing programmable context integer"))
    }

    fn ints(&self, parameter: u32, count: usize) -> Result<&[i32]> {
        self.integers
            .iter()
            .find(|(key, _)| *key == parameter)
            .map(|(_, values)| values.as_slice())
            .filter(|values| values.len() == count)
            .ok_or_else(|| error("invalid programmable context integer vector"))
    }

    fn floats(&self, parameter: u32, count: usize) -> Result<&[f32]> {
        self.floats
            .iter()
            .find(|(key, _)| *key == parameter)
            .map(|(_, values)| values.as_slice())
            .filter(|values| values.len() == count)
            .ok_or_else(|| error("invalid programmable context float vector"))
    }

    fn apply(&self, renderer: &mut Renderer) -> Result<()> {
        invoke!(renderer, "glHint", (0x8192 => u32, self.int(0x8192)? as u32 => u32), ());
        let clear = self.floats(0x0c22, 4)?;
        invoke!(renderer, "glClearColor", (clear[0] => f32, clear[1] => f32, clear[2] => f32, clear[3] => f32), ());
        let blend = self.floats(0x8005, 4)?;
        invoke!(renderer, "glBlendColor", (blend[0] => f32, blend[1] => f32, blend[2] => f32, blend[3] => f32), ());
        invoke!(renderer, "glClearDepth", (self.floats(0x0b73, 1)?[0] as f64 => f64), ());
        invoke!(renderer, "glClearStencil", (self.int(0x0b91)? => i32), ());
        invoke!(renderer, "glBlendEquationSeparate", (self.int(0x8009)? as u32 => u32, self.int(0x883d)? as u32 => u32), ());
        invoke!(renderer, "glBlendFuncSeparate", (self.int(0x80c9)? as u32 => u32, self.int(0x80c8)? as u32 => u32,
            self.int(0x80cb)? as u32 => u32, self.int(0x80ca)? as u32 => u32), ());
        invoke!(renderer, "glCullFace", (self.int(0x0b45)? as u32 => u32), ());
        invoke!(renderer, "glFrontFace", (self.int(0x0b46)? as u32 => u32), ());
        invoke!(renderer, "glDepthMask", (self.int(0x0b72)? as u8 => u8), ());
        invoke!(renderer, "glDepthFunc", (self.int(0x0b74)? as u32 => u32), ());
        let range = self.floats(0x0b70, 2)?;
        invoke!(renderer, "glDepthRange", (range[0] as f64 => f64, range[1] as f64 => f64), ());
        invoke!(renderer, "glLineWidth", (self.floats(0x0b21, 1)?[0] => f32), ());
        invoke!(renderer, "glPolygonOffset", (self.floats(0x8038, 1)?[0] => f32, self.floats(0x2a00, 1)?[0] => f32), ());
        invoke!(renderer, "glSampleCoverage", (self.floats(0x80aa, 1)?[0] => f32, self.int(0x80ab)? as u8 => u8), ());
        let color = self.ints(0x0c23, 4)?;
        invoke!(renderer, "glColorMask", (color[0] as u8 => u8, color[1] as u8 => u8, color[2] as u8 => u8, color[3] as u8 => u8), ());
        for (face, function, reference, mask, fail, zfail, zpass, write) in [
            (
                0x0404, 0x0b92, 0x0b97, 0x0b93, 0x0b94, 0x0b95, 0x0b96, 0x0b98,
            ),
            (
                0x0405, 0x8800, 0x8ca3, 0x8ca4, 0x8801, 0x8802, 0x8803, 0x8ca5,
            ),
        ] {
            invoke!(renderer, "glStencilFuncSeparate", (face => u32, self.int(function)? as u32 => u32, self.int(reference)? => i32, self.int(mask)? as u32 => u32), ());
            invoke!(renderer, "glStencilOpSeparate", (face => u32, self.int(fail)? as u32 => u32, self.int(zfail)? as u32 => u32, self.int(zpass)? as u32 => u32), ());
            invoke!(renderer, "glStencilMaskSeparate", (face => u32, self.int(write)? as u32 => u32), ());
        }
        for &(capability, enabled) in &self.enables {
            let function = if enabled { "glEnable" } else { "glDisable" };
            invoke!(renderer, function, (capability => u32), ());
        }
        for (unit, &(two, cube)) in self.textures.iter().enumerate() {
            invoke!(renderer, "glActiveTexture", (GL_TEXTURE0 + unit as u32 => u32), ());
            invoke!(renderer, "glBindTexture", (TEXTURE_2D => u32, host(renderer, Kind::Texture, two) => u32), ());
            invoke!(renderer, "glBindTexture", (0x8513 => u32, host(renderer, Kind::Texture, cube) => u32), ());
        }
        for attribute in &self.attributes {
            let index = attribute.index;
            if index == 0 {
                renderer.attribute_zero = attribute.value;
            }
            invoke!(renderer, "glVertexAttrib4fv", (index => u32, attribute.value.as_ptr() => *const f32), ());
            if !renderer.client_attributes.contains_key(&index) {
                invoke!(renderer, "glBindBuffer", (ARRAY_BUFFER => u32, host(renderer, Kind::Buffer, attribute.buffer) => u32), ());
                invoke!(renderer, "glVertexAttribPointer", (index => u32, attribute.size => i32, attribute.kind => u32,
                    attribute.normalized as u8 => u8, attribute.stride => i32,
                    attribute.pointer as usize as *const c_void => *const c_void), ());
            }
            let function = if attribute.enabled {
                "glEnableVertexAttribArray"
            } else {
                "glDisableVertexAttribArray"
            };
            invoke!(renderer, function, (index => u32), ());
        }
        invoke!(renderer, "glActiveTexture", (self.int(0x84e0)? as u32 => u32), ());
        invoke!(renderer, "glBindBuffer", (ARRAY_BUFFER => u32, host(renderer, Kind::Buffer, self.int(0x8894)? as u32) => u32), ());
        invoke!(renderer, "glBindBuffer", (ELEMENT_BUFFER => u32, host(renderer, Kind::Buffer, self.int(0x8895)? as u32) => u32), ());
        invoke!(renderer, "glBindRenderbuffer", (RENDERBUFFER => u32, host(renderer, Kind::Renderbuffer, self.int(0x8ca7)? as u32) => u32), ());
        let framebuffer = self.int(0x8ca6)? as u32;
        let framebuffer = if framebuffer == 0 {
            renderer.default_fbo
        } else {
            host(renderer, Kind::Framebuffer, framebuffer)
        };
        invoke!(renderer, "glBindFramebuffer", (FRAMEBUFFER => u32, framebuffer => u32), ());
        invoke!(renderer, "glUseProgram", (host(renderer, Kind::Program, self.int(0x8b8d)? as u32) => u32), ());
        invoke!(renderer, "glPixelStorei", (0x0d05 => u32, self.int(0x0d05)? => i32), ());
        invoke!(renderer, "glPixelStorei", (0x0cf5 => u32, self.int(0x0cf5)? => i32), ());
        for (parameter, function) in [(0x0ba2, "glViewport"), (0x0c10, "glScissor")] {
            let rect = self.ints(parameter, 4)?;
            invoke!(renderer, function, (rect[0] => i32, rect[1] => i32, rect[2] => i32, rect[3] => i32), ());
        }
        Ok(())
    }
}

impl NativeCheckpoint {
    pub(super) fn restore(self, renderer: &mut Renderer) -> std::result::Result<(), String> {
        self.restore_resources(renderer)
            .map_err(|error| error.to_string())
    }

    fn restore_resources(self, renderer: &mut Renderer) -> Result<()> {
        if renderer.host.es {
            return Err(error(
                "native programmable snapshot requires desktop OpenGL",
            ));
        }
        let assign = |renderer: &mut Renderer, kind, guest, host| -> Result<()> {
            renderer
                .names
                .as_mut()
                .unwrap()
                .assign(kind, host, Some(guest))
                .ok_or_else(|| error("invalid programmable snapshot object identity"))?;
            Ok(())
        };
        for shader in &self.shaders {
            let name = invoke!(renderer, "glCreateShader", (shader.kind => u32), u32);
            assign(renderer, Kind::Shader, shader.guest, name)?;
            let compiled = shader
                .executable
                .as_ref()
                .map_or(shader.source.as_slice(), |image| image.source.as_slice());
            let pointer = compiled.as_ptr().cast::<c_char>();
            let length = compiled.len() as i32;
            invoke!(renderer, "glShaderSource", (name => u32, 1 => i32,
                &pointer => *const *const c_char, &length => *const i32), ());
            if shader.compiled {
                invoke!(renderer, "glCompileShader", (name => u32), ());
                if shader_parameter(renderer, name, 0x8b81)? == 0 {
                    return Err(error("saved programmable shader could not compile"));
                }
            }
            if let Some(executable) = &shader.executable {
                renderer.compiled_shaders.insert(name, executable.clone());
            }
            let pointer = shader.source.as_ptr().cast::<c_char>();
            let length = shader.source.len() as i32;
            invoke!(renderer, "glShaderSource", (name => u32, 1 => i32,
                &pointer => *const *const c_char, &length => *const i32), ());
            if let Some(source) = &shader.guest_source {
                renderer.desktop_shader_sources.insert(name, source.clone());
            }
        }
        for program in &self.programs {
            let name = invoke!(renderer, "glCreateProgram", (), u32);
            assign(renderer, Kind::Program, program.guest, name)?;
            let mut executables = Vec::new();
            for image in &program.executable {
                let shader = invoke!(renderer, "glCreateShader", (image.kind => u32), u32);
                let pointer = image.source.as_ptr().cast::<c_char>();
                let length = image.source.len() as i32;
                invoke!(renderer, "glShaderSource", (shader => u32, 1 => i32, &pointer => *const *const c_char, &length => *const i32), ());
                invoke!(renderer, "glCompileShader", (shader => u32), ());
                invoke!(renderer, "glAttachShader", (name => u32, shader => u32), ());
                // The program owns the reference even on early failure.
                invoke!(renderer, "glDeleteShader", (shader => u32), ());
                executables.push(shader);
            }
            for (symbol, index) in &program.attributes {
                let symbol = CString::new(symbol.as_slice())
                    .map_err(|_| error("invalid saved attribute name"))?;
                invoke!(renderer, "glBindAttribLocation", (name => u32, *index => u32, symbol.as_ptr() => *const c_char), ());
            }
            if program.linked {
                invoke!(renderer, "glLinkProgram", (name => u32), ());
                if program_parameter(renderer, name, 0x8b82)? == 0 {
                    return Err(error("saved programmable program could not link"));
                }
                invoke!(renderer, "glUseProgram", (name => u32), ());
                for uniform in &program.uniforms {
                    let symbol = CString::new(uniform.name.as_slice())
                        .map_err(|_| error("invalid saved uniform name"))?;
                    let location = invoke!(renderer, "glGetUniformLocation", (name => u32, symbol.as_ptr() => *const c_char), i32);
                    if location != uniform.location {
                        return Err(error("host changed a saved programmable uniform location"));
                    }
                    let (count, integer) = uniform_components(uniform.kind)?;
                    if uniform.values.len() != count {
                        return Err(error("invalid saved uniform data"));
                    }
                    let pointer = uniform.values.as_ptr();
                    match uniform.kind {
                        0x8b5a => {
                            invoke!(renderer, "glUniformMatrix2fv", (location => i32, 1 => i32, 0 => u8, pointer.cast() => *const f32), ())
                        }
                        0x8b5b => {
                            invoke!(renderer, "glUniformMatrix3fv", (location => i32, 1 => i32, 0 => u8, pointer.cast() => *const f32), ())
                        }
                        0x8b5c => {
                            invoke!(renderer, "glUniformMatrix4fv", (location => i32, 1 => i32, 0 => u8, pointer.cast() => *const f32), ())
                        }
                        _ => {
                            let function = match (count, integer) {
                                (1, false) => "glUniform1fv",
                                (2, false) => "glUniform2fv",
                                (3, false) => "glUniform3fv",
                                (4, false) => "glUniform4fv",
                                (1, true) => "glUniform1iv",
                                (2, true) => "glUniform2iv",
                                (3, true) => "glUniform3iv",
                                (4, true) => "glUniform4iv",
                                _ => return Err(error("invalid saved uniform size")),
                            };
                            invoke!(renderer, function, (location => i32, 1 => i32, pointer.cast() => *const c_void), ());
                        }
                    }
                }
            }
            for shader in executables {
                invoke!(renderer, "glDetachShader", (name => u32, shader => u32), ());
            }
            for shader in &program.shaders {
                invoke!(renderer, "glAttachShader", (name => u32, host(renderer, Kind::Shader, *shader) => u32), ());
            }
            renderer
                .linked_programs
                .insert(name, program.executable.clone());
        }
        for buffer in self.buffers {
            let mut name = 0;
            invoke!(renderer, "glGenBuffers", (1 => i32, &mut name => *mut u32), ());
            assign(renderer, Kind::Buffer, buffer.guest, name)?;
            invoke!(renderer, "glBindBuffer", (ARRAY_BUFFER => u32, name => u32), ());
            invoke!(renderer, "glBufferData", (ARRAY_BUFFER => u32, buffer.bytes.len() as isize => isize,
                buffer.bytes.as_ptr().cast() => *const c_void, buffer.usage => u32), ());
            renderer.guest_buffers.insert(name);
            renderer.index_data.insert(name, buffer.bytes);
        }
        invoke!(renderer, "glPixelStorei", (0x0cf5 => u32, 1 => i32), ());
        invoke!(renderer, "glActiveTexture", (GL_TEXTURE0 => u32), ());
        for texture in self.textures {
            let mut name = 0;
            invoke!(renderer, "glGenTextures", (1 => i32, &mut name => *mut u32), ());
            assign(renderer, Kind::Texture, texture.guest, name)?;
            invoke!(renderer, "glBindTexture", (texture.target => u32, name => u32), ());
            for (target, level, image) in texture.levels {
                image.upload(renderer, target, level)?;
            }
            for (parameter, value) in [0x2800, 0x2801, 0x2802, 0x2803]
                .into_iter()
                .zip(texture.parameters)
            {
                invoke!(renderer, "glTexParameteri", (texture.target => u32, parameter => u32, value => i32), ());
            }
            renderer.guest_textures.insert(name);
        }
        let mut scratch = 0;
        invoke!(renderer, "glGenFramebuffers", (1 => i32, &mut scratch => *mut u32), ());
        let loaded: Result<()> = (|| {
            for buffer in self.renderbuffers {
                let name = if buffer.guest == 0 {
                    renderer.window_depth_stencil
                } else {
                    let mut name = 0;
                    invoke!(renderer, "glGenRenderbuffers", (1 => i32, &mut name => *mut u32), ());
                    assign(renderer, Kind::Renderbuffer, buffer.guest, name)?;
                    invoke!(renderer, "glBindRenderbuffer", (RENDERBUFFER => u32, name => u32), ());
                    invoke!(renderer, "glRenderbufferStorage", (RENDERBUFFER => u32, buffer.image.internal => u32,
                        buffer.image.width => i32, buffer.image.height => i32), ());
                    renderer.guest_renderbuffers.insert(name);
                    name
                };
                upload_renderbuffer(renderer, scratch, name, &buffer.image)?;
            }
            for plane in self.retained {
                if plane.id == 0
                    || renderer.retained.images.contains_key(&plane.id)
                    || !matches!(plane.kind, RENDERBUFFER | 0x1702)
                    || plane.level < 0
                    || plane.level > 30
                    || plane.face != 0 && !(0x8515..=0x851a).contains(&plane.face)
                {
                    return Err(error("invalid retained framebuffer image"));
                }
                let mut native = 0;
                if plane.kind == RENDERBUFFER {
                    invoke!(renderer,"glGenRenderbuffers",(1=>i32,&mut native=>*mut u32),());
                    invoke!(renderer,"glBindRenderbuffer",(RENDERBUFFER=>u32,native=>u32),());
                    invoke!(renderer,"glRenderbufferStorage",(RENDERBUFFER=>u32,plane.image.internal=>u32,plane.image.width=>i32,plane.image.height=>i32),());
                    upload_renderbuffer(renderer, scratch, native, &plane.image)?;
                } else {
                    invoke!(renderer,"glGenTextures",(1=>i32,&mut native=>*mut u32),());
                    let target = if plane.face == 0 { TEXTURE_2D } else { 0x8513 };
                    invoke!(renderer,"glBindTexture",(target=>u32,native=>u32),());
                    if plane.face == 0 {
                        plane.image.upload(renderer, TEXTURE_2D, plane.level)?;
                    } else {
                        for face in 0x8515..=0x851a {
                            plane.image.upload(renderer, face, plane.level)?;
                        }
                    }
                    for (pname, value) in [
                        (0x2801, 0x2600),
                        (0x2800, 0x2600),
                        (0x813c, plane.level),
                        (0x813d, plane.level),
                    ] {
                        invoke!(renderer,"glTexParameteri",(target=>u32,pname=>u32,value=>i32),());
                    }
                }
                let mut keeper = 0;
                invoke!(renderer,"glGenFramebuffers",(1=>i32,&mut keeper=>*mut u32),());
                invoke!(renderer,"glBindFramebuffer",(FRAMEBUFFER=>u32,keeper=>u32),());
                let point = attachment_point(plane.image.format);
                if plane.kind == RENDERBUFFER {
                    invoke!(renderer,"glFramebufferRenderbuffer",(FRAMEBUFFER=>u32,point=>u32,RENDERBUFFER=>u32,native=>u32),());
                } else {
                    let face = if plane.face == 0 {
                        TEXTURE_2D
                    } else {
                        plane.face
                    };
                    invoke!(renderer,"glFramebufferTexture2D",(FRAMEBUFFER=>u32,point=>u32,face=>u32,native=>u32,plane.level=>i32),());
                }
                let color = if plane.image.format == RGBA {
                    0x8ce0
                } else {
                    0
                };
                invoke!(renderer,"glDrawBuffer",(color=>u32),());
                invoke!(renderer,"glReadBuffer",(color=>u32),());
                renderer.retained.next = renderer.retained.next.max(plane.id);
                renderer.retained.images.insert(
                    plane.id,
                    retained::KeptImage {
                        native,
                        keeper,
                        kind: plane.kind,
                        guest: plane.guest,
                        face: plane.face,
                        level: plane.level,
                        width: plane.image.width,
                        height: plane.image.height,
                        internal: plane.image.internal,
                    },
                );
            }
            invoke!(renderer, "glBindTexture", (TEXTURE_2D => u32, renderer.window_color => u32), ());
            self.window.upload(renderer, TEXTURE_2D, 0)?;
            Ok(())
        })();
        invoke!(renderer, "glDeleteFramebuffers", (1 => i32, &scratch => *const u32), ());
        loaded?;
        for framebuffer in self.framebuffers {
            let mut name = 0;
            invoke!(renderer, "glGenFramebuffers", (1 => i32, &mut name => *mut u32), ());
            assign(renderer, Kind::Framebuffer, framebuffer.guest, name)?;
            invoke!(renderer, "glBindFramebuffer", (FRAMEBUFFER => u32, name => u32), ());
            for attachment in framebuffer.attachments {
                let object = if let Some(id) = attachment.retained {
                    let kept = renderer
                        .retained
                        .images
                        .get(&id)
                        .ok_or_else(|| error("missing retained framebuffer image"))?;
                    if kept.kind != attachment.kind
                        || kept.guest != attachment.guest
                        || kept.level != attachment.level
                        || kept.face != attachment.face
                    {
                        return Err(error("inconsistent retained framebuffer attachment"));
                    }
                    let object = kept.native;
                    renderer
                        .retained
                        .attachments
                        .insert((name, attachment.point), id);
                    object
                } else {
                    host(
                        renderer,
                        if attachment.kind == RENDERBUFFER {
                            Kind::Renderbuffer
                        } else {
                            Kind::Texture
                        },
                        attachment.guest,
                    )
                };
                if attachment.kind == RENDERBUFFER {
                    invoke!(renderer, "glFramebufferRenderbuffer", (FRAMEBUFFER => u32, attachment.point => u32,
                        RENDERBUFFER => u32, object => u32), ());
                } else {
                    let face = if attachment.face == 0 {
                        TEXTURE_2D
                    } else {
                        attachment.face
                    };
                    invoke!(renderer, "glFramebufferTexture2D", (FRAMEBUFFER => u32, attachment.point => u32,
                        face => u32, object => u32, attachment.level => i32), ());
                }
            }
            renderer.guest_framebuffers.insert(name);
        }
        invoke!(renderer,"glBindFramebuffer",(FRAMEBUFFER=>u32,renderer.default_fbo=>u32),());
        for kept in renderer
            .retained
            .images
            .values()
            .map(|image| (image.kind, image.native))
            .collect::<Vec<_>>()
        {
            let function = if kept.0 == RENDERBUFFER {
                "glDeleteRenderbuffers"
            } else {
                "glDeleteTextures"
            };
            invoke!(renderer,function,(1=>i32,&kept.1=>*const u32),());
        }
        renderer.collect_retained_images()?;
        renderer.client_attributes = self.client.into_iter().collect();
        renderer.configured_attributes = self.configured.into_iter().collect();
        renderer.enabled_attributes = self
            .state
            .attributes
            .iter()
            .filter(|attribute| attribute.enabled)
            .map(|attribute| attribute.index)
            .collect();
        renderer.framebuffer = host(renderer, Kind::Framebuffer, self.state.int(0x8ca6)? as u32);
        renderer.array = host(renderer, Kind::Buffer, self.state.int(0x8894)? as u32);
        renderer.elements = host(renderer, Kind::Buffer, self.state.int(0x8895)? as u32);
        renderer.active_texture = self.state.int(0x84e0)? as u32;
        renderer.pack = self.state.int(0x0d05)? as usize;
        renderer.unpack = self.state.int(0x0cf5)? as usize;
        let logical = |parameter| -> Result<[i32; 4]> {
            let rect = self.state.ints(parameter, 4)?;
            let scale = if renderer.framebuffer == 0 {
                renderer.scale as i32
            } else {
                1
            };
            Ok([
                rect[0] / scale,
                rect[1] / scale,
                rect[2] / scale,
                rect[3] / scale,
            ])
        };
        renderer.viewport = logical(0x0ba2)?;
        renderer.scissor = logical(0x0c10)?;
        self.state.apply(renderer)?;
        for program in self.programs.iter().filter(|program| program.deleted) {
            invoke!(renderer, "glDeleteProgram", (host(renderer, Kind::Program, program.guest) => u32), ());
        }
        for shader in self.shaders.iter().filter(|shader| shader.deleted) {
            let name = host(renderer, Kind::Shader, shader.guest);
            invoke!(renderer, "glDeleteShader", (name => u32), ());
            renderer.deleted_shaders.insert(name);
        }
        let pending = invoke!(renderer, "glGetError", (), u32);
        if pending != 0 {
            return Err(error(format!(
                "programmable resource restore failed with GL error {pending:#x}"
            )));
        }
        renderer.guest_error = self.error;
        Ok(())
    }
}

fn upload_renderbuffer(
    renderer: &mut Renderer,
    scratch: u32,
    target: u32,
    image: &Image,
) -> Result<()> {
    let mut texture = 0;
    let mut source = 0;
    invoke!(renderer, "glGenTextures", (1 => i32, &mut texture => *mut u32), ());
    invoke!(renderer, "glGenFramebuffers", (1 => i32, &mut source => *mut u32), ());
    let result = (|| {
        invoke!(renderer, "glBindTexture", (TEXTURE_2D => u32, texture => u32), ());
        image.upload(renderer, TEXTURE_2D, 0)?;
        invoke!(renderer, "glTexParameteri", (TEXTURE_2D => u32, 0x2801 => u32, 0x2600 => i32), ());
        let point = attachment_point(image.format);
        let color = if image.format == RGBA { 0x8ce0 } else { 0 };
        invoke!(renderer, "glBindFramebuffer", (FRAMEBUFFER => u32, source => u32), ());
        invoke!(renderer, "glFramebufferTexture2D", (FRAMEBUFFER => u32, point => u32, TEXTURE_2D => u32, texture => u32, 0 => i32), ());
        invoke!(renderer, "glDrawBuffer", (color => u32), ());
        invoke!(renderer, "glReadBuffer", (color => u32), ());
        invoke!(renderer, "glBindFramebuffer", (FRAMEBUFFER => u32, scratch => u32), ());
        for attachment in [0x8ce0, 0x8d00, 0x8d20] {
            invoke!(renderer, "glFramebufferRenderbuffer", (FRAMEBUFFER => u32, attachment => u32, RENDERBUFFER => u32, 0 => u32), ());
        }
        invoke!(renderer, "glFramebufferRenderbuffer", (FRAMEBUFFER => u32, point => u32, RENDERBUFFER => u32, target => u32), ());
        invoke!(renderer, "glDrawBuffer", (color => u32), ());
        invoke!(renderer, "glReadBuffer", (color => u32), ());
        if invoke!(renderer, "glCheckFramebufferStatus", (FRAMEBUFFER => u32), u32) != 0x8cd5 {
            return Err(error("cannot restore saved programmable renderbuffer"));
        }
        invoke!(renderer, "glBindFramebuffer", (0x8ca8 => u32, source => u32), ());
        invoke!(renderer, "glBindFramebuffer", (0x8ca9 => u32, scratch => u32), ());
        invoke!(renderer, "glDisable", (0x0c11 => u32), ());
        let mask = match image.format {
            DEPTH_STENCIL => 0x0500,
            0x1902 => 0x0100,
            0x1901 => 0x0400,
            _ => 0x4000,
        };
        let function = renderer.blit;
        invoke!(renderer, function, (0 => i32, 0 => i32, image.width => i32, image.height => i32,
            0 => i32, 0 => i32, image.width => i32, image.height => i32, mask => u32, 0x2600 => u32), ());
        Ok(())
    })();
    invoke!(renderer, "glDeleteFramebuffers", (1 => i32, &source => *const u32), ());
    invoke!(renderer, "glDeleteTextures", (1 => i32, &texture => *const u32), ());
    result
}
