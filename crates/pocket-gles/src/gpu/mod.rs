//! OpenGL 2.1 / GLES 2 offscreen rasterizer. Guest pointers never reach a driver.
mod platform;
use crate::{
    consts::*,
    raster::{
        BlendFactor, CompareFunc, CullMode, FogMode, FrontFace, PipelineState, RenderTarget,
        TexEnvMode, TextureStage, Vertex,
    },
    texture::{Filter, Texture, Wrap},
};
use platform::Host;
use std::{
    collections::HashMap,
    ffi::{c_char, c_void, CStr, CString},
};
const SAMPLERS: [&str; 4] = ["sampler0", "sampler1", "sampler2", "sampler3"];
const ENVIRONMENTS: [&str; 4] = ["env0", "env1", "env2", "env3"];
const ENV_COLORS: [&str; 4] = ["envColor0", "envColor1", "envColor2", "envColor3"];
#[allow(non_snake_case)]
struct Gl {
    GetString: unsafe extern "system" fn(u32) -> *const u8,
    GetError: unsafe extern "system" fn() -> u32,
    GetIntegerv: unsafe extern "system" fn(u32, *mut i32),
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
    Uniform4fv: unsafe extern "system" fn(i32, i32, *const f32),
    GenBuffers: unsafe extern "system" fn(i32, *mut u32),
    DeleteBuffers: unsafe extern "system" fn(i32, *const u32),
    BindBuffer: unsafe extern "system" fn(u32, u32),
    BufferData: unsafe extern "system" fn(u32, isize, *const c_void, u32),
    EnableVertexAttribArray: unsafe extern "system" fn(u32),
    VertexAttribPointer: unsafe extern "system" fn(u32, i32, u32, u8, i32, *const c_void),
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
                std::mem::transmute::<*mut c_void, unsafe extern "system" fn(u32, *const c_char) -> i32>(
                    p,
                )
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
pub struct Renderer {
    host: Host,
    gl: Gl,
    program: u32,
    buffer: u32,
    fbo: u32,
    color: u32,
    depth: u32,
    white: u32,
    width: u32,
    height: u32,
    textures: HashMap<u32, (u32, u64)>,
    uniforms: HashMap<String, i32>,
    scratch: Vec<f32>,
    readback: Vec<u8>,
    dirty: bool,
}
impl Renderer {
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        Self::with_host(Host::new()?, width, height)
    }
    fn with_host(host: Host, width: u32, height: u32) -> Result<Self, String> {
        unsafe {
            let gl = Gl::load(&host)?;
            let mut units = 0;
            let mut attributes = 0;
            (gl.GetIntegerv)(0x8872, &mut units);
            (gl.GetIntegerv)(0x8869, &mut attributes);
            if units < 4 || attributes < 7 {
                return Err("OpenGL driver lacks required texture/attribute units".into());
            }
            let mut r = Self {
                host,
                gl,
                program: 0,
                buffer: 0,
                fbo: 0,
                color: 0,
                depth: 0,
                white: 0,
                width: 0,
                height: 0,
                textures: HashMap::new(),
                uniforms: HashMap::new(),
                scratch: Vec::new(),
                readback: Vec::new(),
                dirty: true,
            };
            r.program = (r.gl.CreateProgram)();
            if r.program == 0 {
                return Err("OpenGL program allocation failed".into());
            }
            let prefix = if r.host.es {
                "#version 100\nprecision mediump float;\n"
            } else {
                "#version 120\n"
            };
            let vs = r.shader(0x8b31, &format!("{prefix}{VERTEX}"))?;
            let fs = match r.shader(0x8b30, &format!("{prefix}{FRAGMENT}")) {
                Ok(s) => s,
                Err(e) => {
                    (r.gl.DeleteShader)(vs);
                    return Err(e);
                }
            };
            (r.gl.AttachShader)(r.program, vs);
            (r.gl.AttachShader)(r.program, fs);
            for (i, name) in [
                "position", "color", "tex0", "tex1", "tex2", "tex3", "fogDepth",
            ]
            .iter()
            .enumerate()
            {
                (r.gl.BindAttribLocation)(
                    r.program,
                    i as u32,
                    CString::new(*name).unwrap().as_ptr(),
                );
            }
            (r.gl.LinkProgram)(r.program);
            (r.gl.DeleteShader)(vs);
            (r.gl.DeleteShader)(fs);
            let mut ok = 0;
            (r.gl.GetProgramiv)(r.program, 0x8b82, &mut ok);
            if ok == 0 {
                let mut msg = vec![0u8; 4096];
                (r.gl.GetProgramInfoLog)(
                    r.program,
                    4096,
                    std::ptr::null_mut(),
                    msg.as_mut_ptr().cast(),
                );
                return Err(String::from_utf8_lossy(&msg).trim_end_matches('\0').into());
            }
            (r.gl.GenBuffers)(1, &mut r.buffer);
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
                "depthRange",
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
            for i in 0..4 {
                for name in ["sampler", "env", "envColor"] {
                    r.cache_uniform(&format!("{name}{i}"));
                }
            }
            let p = (r.gl.GetString)(0x1f01);
            let label = if p.is_null() {
                "unknown".into()
            } else {
                CStr::from_ptr(p.cast()).to_string_lossy().into_owned()
            };
            log::info!("GLES host OpenGL renderer: {label}");
            r.check()?;
            Ok(r)
        }
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
    fn loc(&self, n: &str) -> i32 {
        self.uniforms[n]
    }
    unsafe fn check(&self) -> Result<(), String> {
        let e = (self.gl.GetError)();
        if e == 0 {
            Ok(())
        } else {
            Err(format!("host OpenGL error 0x{e:04x}"))
        }
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
            (self.gl.Scissor)(x, y, w, h);
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
    pub fn resize(&mut self, w: u32, h: u32) -> Result<(), String> {
        if (w, h) == (self.width, self.height) {
            return Ok(());
        }
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
                GL_RGBA as i32,
                w as i32,
                h as i32,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                std::ptr::null(),
            );
            self.params(
                Filter::Nearest,
                Filter::Nearest,
                Wrap::ClampToEdge,
                Wrap::ClampToEdge,
            );
            (self.gl.BindRenderbuffer)(0x8d41, self.depth);
            (self.gl.RenderbufferStorage)(0x8d41, 0x81a5, w as i32, h as i32);
            (self.gl.BindFramebuffer)(0x8d40, self.fbo);
            (self.gl.FramebufferTexture2D)(0x8d40, 0x8ce0, GL_TEXTURE_2D, self.color, 0);
            (self.gl.FramebufferRenderbuffer)(0x8d40, 0x8d00, 0x8d41, self.depth);
            if (self.gl.CheckFramebufferStatus)(0x8d40) != 0x8cd5 {
                return Err("GPU framebuffer incomplete".into());
            }
            self.scissor(None);
            (self.gl.ColorMask)(1, 1, 1, 1);
            (self.gl.DepthMask)(1);
            (self.gl.ClearColor)(0., 0., 0., 0.);
            (self.gl.Clear)(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT);
            self.width = w;
            self.height = h;
            self.dirty = true;
            self.check()
        }
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
        unsafe {
            (self.gl.BindFramebuffer)(0x8d40, self.fbo);
            self.scissor(state.scissor);
            (self.gl.ColorMask)(
                state.color_mask[0] as u8,
                state.color_mask[1] as u8,
                state.color_mask[2] as u8,
                state.color_mask[3] as u8,
            );
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
            self.dirty = true;
            self.check()
        }
    }
    pub fn delete_textures(&mut self, names: &[u32]) {
        if self.host.make_current().is_err() {
            return;
        }
        for n in names {
            if let Some((id, _)) = self.textures.remove(n) {
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
        names: [u32; 4],
    ) -> Result<(), String> {
        self.resize(target.width, target.height)?;
        self.host.make_current()?;
        self.scratch.clear();
        let mut triangle = |v: [Vertex; 3]| {
            for mut p in v {
                if !state.smooth_shading {
                    p.color = v[2].color;
                }
                self.scratch.extend(p.pos);
                self.scratch.extend(p.color);
                for t in p.texcoord {
                    self.scratch.extend(t);
                }
                self.scratch.push(p.fog_depth);
            }
        };
        match mode {
            GL_TRIANGLES => {
                for v in vertices.chunks_exact(3) {
                    triangle([v[0], v[1], v[2]]);
                }
            }
            GL_TRIANGLE_FAN => {
                for i in 2..vertices.len() {
                    triangle([vertices[0], vertices[i - 1], vertices[i]]);
                }
            }
            GL_TRIANGLE_STRIP => {
                for i in 2..vertices.len() {
                    triangle(if i % 2 == 0 {
                        [vertices[i - 2], vertices[i - 1], vertices[i]]
                    } else {
                        [vertices[i - 1], vertices[i - 2], vertices[i]]
                    });
                }
            }
            _ => return Err("unsupported GPU primitive".into()),
        }
        unsafe {
            (self.gl.BindFramebuffer)(0x8d40, self.fbo);
            (self.gl.UseProgram)(self.program);
            let (x, y, w, h) = state.viewport;
            (self.gl.Viewport)(x, y, w, h);
            self.scissor(state.scissor);
            self.toggle(GL_DEPTH_TEST, state.depth_test);
            (self.gl.DepthFunc)(compare(state.depth_func));
            (self.gl.DepthMask)(state.depth_write as u8);
            self.toggle(GL_BLEND, state.blend);
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
            (self.gl.Uniform2f)(
                self.loc("depthRange"),
                state.depth_range.0,
                state.depth_range.1,
            );
            (self.gl.Uniform1i)(
                self.loc("alphaMode"),
                if state.alpha_test {
                    compare(state.alpha_func) as i32 - GL_NEVER as i32
                } else {
                    7
                },
            );
            (self.gl.Uniform1f)(self.loc("alphaRef"), state.alpha_ref);
            (self.gl.Uniform1i)(
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
            (self.gl.Uniform4fv)(self.loc("fogColor"), 1, state.fog_color.as_ptr());
            for (n, v) in [
                ("fogStart", state.fog_start),
                ("fogEnd", state.fog_end),
                ("fogDensity", state.fog_density),
            ] {
                (self.gl.Uniform1f)(self.loc(n), v);
            }
            (self.gl.PixelStorei)(0x0cf5, 1);
            for (i, name) in names.into_iter().enumerate() {
                (self.gl.ActiveTexture)(GL_TEXTURE0 + i as u32);
                let stage = stages.iter().find(|s| s.unit == i);
                let mut id = self.white;
                if stage.is_some() {
                    if let Some(tex) = textures.get(&name).filter(|t| t.is_complete()) {
                        let item = self.textures.entry(name).or_insert_with(|| {
                            let mut id = 0;
                            (self.gl.GenTextures)(1, &mut id);
                            (id, u64::MAX)
                        });
                        id = item.0;
                        (self.gl.BindTexture)(GL_TEXTURE_2D, id);
                        if item.1 != tex.revision {
                            (self.gl.TexImage2D)(
                                GL_TEXTURE_2D,
                                0,
                                GL_RGBA as i32,
                                tex.width as i32,
                                tex.height as i32,
                                0,
                                GL_RGBA,
                                GL_UNSIGNED_BYTE,
                                tex.rgba.as_ptr().cast(),
                            );
                            item.1 = tex.revision;
                        }
                        self.params(tex.min_filter, tex.mag_filter, tex.wrap_s, tex.wrap_t);
                    }
                }
                (self.gl.BindTexture)(GL_TEXTURE_2D, id);
                (self.gl.Uniform1i)(self.loc(SAMPLERS[i]), i as i32);
                (self.gl.Uniform1i)(
                    self.loc(ENVIRONMENTS[i]),
                    stage.map_or(-1, |s| match s.env {
                        TexEnvMode::Modulate => 0,
                        TexEnvMode::Replace => 1,
                        TexEnvMode::Decal => 2,
                        TexEnvMode::Blend => 3,
                        TexEnvMode::Add => 4,
                    }),
                );
                if let Some(s) = stage {
                    (self.gl.Uniform4fv)(self.loc(ENV_COLORS[i]), 1, s.env_color.as_ptr());
                }
            }
            (self.gl.BindBuffer)(GL_ARRAY_BUFFER, self.buffer);
            (self.gl.BufferData)(
                GL_ARRAY_BUFFER,
                (self.scratch.len() * 4) as isize,
                self.scratch.as_ptr().cast(),
                0x88e0,
            );
            for (i, (size, offset)) in
                [(4, 0), (4, 16), (2, 32), (2, 40), (2, 48), (2, 56), (1, 64)]
                    .into_iter()
                    .enumerate()
            {
                (self.gl.EnableVertexAttribArray)(i as u32);
                (self.gl.VertexAttribPointer)(
                    i as u32,
                    size,
                    GL_FLOAT,
                    0,
                    68,
                    offset as *const c_void,
                );
            }
            (self.gl.DrawArrays)(GL_TRIANGLES, 0, (self.scratch.len() / 17) as i32);
            self.dirty = true;
            self.check()
        }
    }
    pub fn read(&mut self, target: &mut RenderTarget) -> Result<(), String> {
        self.resize(target.width, target.height)?;
        if !self.dirty {
            return Ok(());
        }
        self.host.make_current()?;
        unsafe {
            self.readback
                .resize((self.width * self.height * 4) as usize, 0);
            (self.gl.BindFramebuffer)(0x8d40, self.fbo);
            (self.gl.PixelStorei)(0x0d05, 1);
            (self.gl.ReadPixels)(
                0,
                0,
                self.width as i32,
                self.height as i32,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                self.readback.as_mut_ptr().cast(),
            );
            self.check()?;
            let stride = self.width as usize * 4;
            for (row, out) in target.color.chunks_exact_mut(stride).enumerate() {
                let start = (self.height as usize - 1 - row) * stride;
                out.copy_from_slice(&self.readback[start..start + stride]);
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
        unsafe {
            for (id, _) in self.textures.values() {
                (self.gl.DeleteTextures)(1, id);
            }
            for id in [self.color, self.white] {
                (self.gl.DeleteTextures)(1, &id);
            }
            (self.gl.DeleteRenderbuffers)(1, &self.depth);
            (self.gl.DeleteFramebuffers)(1, &self.fbo);
            (self.gl.DeleteBuffers)(1, &self.buffer);
            (self.gl.DeleteProgram)(self.program);
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
    }
}
fn wrap(v: Wrap) -> u32 {
    match v {
        Wrap::Repeat => GL_REPEAT,
        Wrap::ClampToEdge => GL_CLAMP_TO_EDGE,
    }
}
const VERTEX: &str = r#"
attribute vec4 position,color;attribute vec2 tex0,tex1,tex2,tex3;attribute float fogDepth;
varying vec4 vColor;varying vec2 uv0,uv1,uv2,uv3;varying float vFog;uniform vec2 depthRange;
void main(){gl_Position=position;gl_Position.z=(depthRange.y-depthRange.x)*position.z+(depthRange.x+depthRange.y-1.0)*position.w;vColor=color;uv0=tex0;uv1=tex1;uv2=tex2;uv3=tex3;vFog=fogDepth;}
"#;
const FRAGMENT: &str = r#"
varying vec4 vColor;varying vec2 uv0,uv1,uv2,uv3;varying float vFog;
uniform sampler2D sampler0,sampler1,sampler2,sampler3;uniform int env0,env1,env2,env3;uniform vec4 envColor0,envColor1,envColor2,envColor3;
uniform int alphaMode,fogMode;uniform float alphaRef,fogStart,fogEnd,fogDensity;uniform vec4 fogColor;
vec4 stage(vec4 c,vec4 t,int e,vec4 k){if(e==0)return c*t;if(e==1)return t;if(e==2)return vec4(mix(c.rgb,t.rgb,t.a),c.a);if(e==3)return vec4(mix(c.rgb,k.rgb,t.rgb),c.a*t.a);if(e==4)return vec4(min(c.rgb+t.rgb,vec3(1.0)),c.a*t.a);return c;}
void main(){vec4 c=vColor;c=stage(c,texture2D(sampler0,uv0),env0,envColor0);c=stage(c,texture2D(sampler1,uv1),env1,envColor1);c=stage(c,texture2D(sampler2,uv2),env2,envColor2);c=stage(c,texture2D(sampler3,uv3),env3,envColor3);
if(alphaMode==0||(alphaMode==1&&c.a>=alphaRef)||(alphaMode==2&&c.a!=alphaRef)||(alphaMode==3&&c.a>alphaRef)||(alphaMode==4&&c.a<=alphaRef)||(alphaMode==5&&c.a==alphaRef)||(alphaMode==6&&c.a<alphaRef))discard;
float f=1.0;if(fogMode==1)f=(fogEnd-vFog)/max(fogEnd-fogStart,0.000001);if(fogMode==2)f=exp(-fogDensity*vFog);if(fogMode==3){float d=fogDensity*vFog;f=exp(-d*d);}c.rgb=mix(fogColor.rgb,c.rgb,clamp(f,0.0,1.0));gl_FragColor=c;}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    fn triangle(color: [f32; 4], z: f32) -> [Vertex; 3] {
        [[-0.9, -0.9, z, 1.], [0.9, -0.9, z, 1.], [0., 0.9, z, 1.]].map(|pos| Vertex {
            pos,
            color,
            ..Vertex::default()
        })
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
            [0; 4],
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
            [0; 4],
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
            [0; 4],
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
            [0; 4],
        )
        .unwrap();
        gpu.draw(
            &target,
            &state,
            GL_TRIANGLES,
            &triangle([1., 0., 0., 1.], 0.5),
            &[],
            &textures,
            [0; 4],
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
            [0; 4],
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
            [0; 4],
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
        }];
        gpu.draw(
            &target,
            &state,
            GL_TRIANGLES,
            &triangle([1.; 4], 0.),
            &stages,
            &textures,
            [1, 0, 0, 0],
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
            [1, 0, 0, 0],
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
                [1, 0, 0, 0],
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
}
