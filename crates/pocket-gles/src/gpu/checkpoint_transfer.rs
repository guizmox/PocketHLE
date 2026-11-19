//! ES checkpoint transfers shared by fixed-function and programmable rendering.
//! Staging stays on the GPU; only explicit checkpoint bytes cross to the CPU.
use super::platform::Host;
use std::{
    collections::HashMap,
    ffi::{c_char, c_void, CString},
};
type Result<T> = std::result::Result<T, String>;
const FRAMEBUFFER: u32 = 0x8d40;
const RENDERBUFFER: u32 = 0x8d41;
const TEXTURE_2D: u32 = 0x0de1;
const RGBA: u32 = 0x1908;
const UNSIGNED_BYTE: u32 = 0x1401;
const GL_TEXTURE0: u32 = 0x84c0;
const DEPTH_STENCIL: u32 = 0x84f9;
fn error(value: impl ToString) -> String {
    value.to_string()
}

pub struct Api<'a> {
    pub host: &'a Host,
    symbols: HashMap<String, *const c_void>,
}
impl<'a> Api<'a> {
    pub fn new(host: &'a Host) -> Self {
        Self {
            host,
            symbols: HashMap::new(),
        }
    }
    fn symbol(&mut self, name: &str) -> Result<*const c_void> {
        if let Some(&address) = self.symbols.get(name) {
            return Ok(address);
        }
        let name_c = CString::new(name).map_err(error)?;
        let address = self.host.load(&name_c).cast_const();
        if address.is_null() {
            return Err(format!("Host GL has no {name}"));
        }
        self.symbols.insert(name.into(), address);
        Ok(address)
    }
}
macro_rules! invoke {
    ($api:expr,$name:expr,($($arg:expr=>$ty:ty),* $(,)?),$ret:ty)=>{{
        let function:unsafe extern "system" fn($($ty),*)->$ret=unsafe{std::mem::transmute($api.symbol($name)?)};
        unsafe{function($($arg),*)}
    }};
}
pub struct Plane {
    pub width: i32,
    pub height: i32,
    pub internal: u32,
    pub format: u32,
    pub kind: u32,
    pub bytes: Vec<u8>,
    pub depth_samples: Vec<u8>,
}
impl Plane {
    pub fn layout(width: i32, height: i32, internal: u32) -> Result<(u32, u32, usize)> {
        let format = match internal {
            0x81a5 | 0x81a6 | 0x81a7 | 0x1902 => 0x1902,
            0x88f0 | DEPTH_STENCIL => DEPTH_STENCIL,
            0x8d48 | 0x1901 => 0x1901,
            _ => RGBA,
        };
        let (kind, size) = match format {
            0x1902 => (0x1405, 4),
            DEPTH_STENCIL => (0x84fa, 4),
            0x1901 => (UNSIGNED_BYTE, 1),
            _ => (UNSIGNED_BYTE, 4),
        };
        let length = usize::try_from(width)
            .ok()
            .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
            .and_then(|n| n.checked_mul(size))
            .filter(|&n| n <= 64 * 1024 * 1024)
            .ok_or("invalid GPU checkpoint plane dimensions")?;
        Ok((format, kind, length))
    }
    pub fn new(width: i32, height: i32, internal: u32) -> Result<Self> {
        let (format, kind, size) = Self::layout(width, height, internal)?;
        Ok(Self {
            width,
            height,
            internal,
            format,
            kind,
            bytes: vec![0; size],
            depth_samples: Vec::new(),
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub fn validate_data(
        width: i32,
        height: i32,
        internal: u32,
        format: u32,
        kind: u32,
        bytes: &[u8],
        samples: &[u8],
    ) -> Result<()> {
        let (f, k, size) = Self::layout(width, height, internal)?;
        let pixels = width as usize * height as usize;
        if f != format
            || k != kind
            || bytes.len() != size
            || (!samples.is_empty() && (format == RGBA || samples.len() != pixels * 4))
        {
            return Err("invalid GPU checkpoint plane data".into());
        }
        if samples.chunks_exact(4).any(|p| {
            let v = f32::from_le_bytes(p.try_into().unwrap());
            !v.is_finite() || !(0.0..=1.0).contains(&v)
        }) {
            return Err("invalid GPU checkpoint depth value".into());
        }
        Ok(())
    }
    fn validate(&self) -> Result<()> {
        Self::validate_data(
            self.width,
            self.height,
            self.internal,
            self.format,
            self.kind,
            &self.bytes,
            &self.depth_samples,
        )
    }
}
fn integer(r: &mut Api<'_>, p: u32) -> Result<i32> {
    let mut v = 0;
    invoke!(r,"glGetIntegerv",(p=>u32,&mut v=>*mut i32),());
    Ok(v)
}
fn shader_parameter(r: &mut Api<'_>, s: u32, p: u32) -> Result<i32> {
    let mut v = 0;
    invoke!(r,"glGetShaderiv",(s=>u32,p=>u32,&mut v=>*mut i32),());
    Ok(v)
}
fn program_parameter(r: &mut Api<'_>, s: u32, p: u32) -> Result<i32> {
    let mut v = 0;
    invoke!(r,"glGetProgramiv",(s=>u32,p=>u32,&mut v=>*mut i32),());
    Ok(v)
}
fn check_error(r: &mut Api<'_>, stage: &str) -> Result<()> {
    let e = invoke!(r, "glGetError", (), u32);
    if e != 0 {
        Err(format!("GPU checkpoint {stage}: GL error {e:#x}"))
    } else {
        Ok(())
    }
}
fn attachment_point(format: u32) -> u32 {
    match format {
        DEPTH_STENCIL => 0x821a,
        0x1902 => 0x8d00,
        0x1901 => 0x8d20,
        _ => 0x8ce0,
    }
}
fn draw_buffer(r: &mut Api<'_>, color: u32) -> Result<()> {
    invoke!(r,"glDrawBuffers",(1=>i32,&color=>*const u32),());
    invoke!(r,"glReadBuffer",(color=>u32),());
    Ok(())
}
pub fn select_buffer(r: &mut Api<'_>, color: u32) -> Result<()> {
    draw_buffer(r, color)
}

struct Objects {
    textures: Vec<u32>,
    framebuffers: Vec<u32>,
    programs: Vec<u32>,
}
impl Objects {
    fn new() -> Self {
        Self {
            textures: vec![],
            framebuffers: vec![],
            programs: vec![],
        }
    }
    fn texture(&mut self, r: &mut Api<'_>) -> Result<u32> {
        let mut name = 0;
        invoke!(r,"glGenTextures",(1=>i32,&mut name=>*mut u32),());
        self.textures.push(name);
        Ok(name)
    }
    fn framebuffer(&mut self, r: &mut Api<'_>) -> Result<u32> {
        let mut name = 0;
        invoke!(r,"glGenFramebuffers",(1=>i32,&mut name=>*mut u32),());
        self.framebuffers.push(name);
        Ok(name)
    }
    fn cleanup(self, r: &mut Api<'_>) -> Result<()> {
        invoke!(r,"glBindFramebuffer",(FRAMEBUFFER=>u32,0=>u32),());
        invoke!(r,"glDeleteTextures",(self.textures.len() as i32=>i32,self.textures.as_ptr()=>*const u32),());
        invoke!(r,"glDeleteFramebuffers",(self.framebuffers.len() as i32=>i32,self.framebuffers.as_ptr()=>*const u32),());
        for p in self.programs {
            invoke!(r,"glDeleteProgram",(p=>u32),());
        }
        Ok(())
    }
    fn program(&mut self, r: &mut Api<'_>, fragment: &str) -> Result<u32> {
        let program = invoke!(r, "glCreateProgram", (), u32);
        self.programs.push(program);
        for (kind,source) in [(0x8b31,"#version 300 es\nvoid main(){vec2 p=vec2(float((gl_VertexID<<1)&2),float(gl_VertexID&2)); gl_Position=vec4(p*2.0-1.0,0,1);}"),(0x8b30,fragment)] {
            let shader=invoke!(r,"glCreateShader",(kind=>u32),u32);
            let ptr=source.as_ptr().cast::<c_char>(); let len=source.len() as i32;
            invoke!(r,"glShaderSource",(shader=>u32,1=>i32,&ptr=>*const *const c_char,&len=>*const i32),());
            invoke!(r,"glCompileShader",(shader=>u32),());
            let compiled=shader_parameter(r,shader,0x8b81)?;
            if compiled!=0 { invoke!(r,"glAttachShader",(program=>u32,shader=>u32),()); }
            invoke!(r,"glDeleteShader",(shader=>u32),());
            if compiled==0 { return Err(error("ES checkpoint staging shader did not compile")); }
        }
        invoke!(r,"glLinkProgram",(program=>u32),());
        if program_parameter(r, program, 0x8b82)? == 0 {
            return Err(error("ES checkpoint staging program did not link"));
        }
        invoke!(r,"glUseProgram",(program=>u32),());
        Ok(program)
    }
}
fn uniform(r: &mut Api<'_>, program: u32, name: &str, value: i32) -> Result<()> {
    let name = CString::new(name).unwrap();
    let location =
        invoke!(r,"glGetUniformLocation",(program=>u32,name.as_ptr()=>*const c_char),i32);
    invoke!(r,"glUniform1i",(location=>i32,value=>i32),());
    Ok(())
}
fn texture_storage(
    r: &mut Api<'_>,
    texture: u32,
    width: i32,
    height: i32,
    internal: u32,
) -> Result<()> {
    invoke!(r,"glBindTexture",(TEXTURE_2D=>u32,texture=>u32),());
    invoke!(r,"glTexStorage2D",(TEXTURE_2D=>u32,1=>i32,internal=>u32,width=>i32,height=>i32),());
    for (p, v) in [
        (0x2801, 0x2600),
        (0x2800, 0x2600),
        (0x2802, 0x812f),
        (0x2803, 0x812f),
    ] {
        invoke!(r,"glTexParameteri",(TEXTURE_2D=>u32,p=>u32,v=>i32),());
    }
    Ok(())
}
fn attach(r: &mut Api<'_>, fbo: u32, texture: u32, format: u32) -> Result<()> {
    invoke!(r,"glBindFramebuffer",(FRAMEBUFFER=>u32,fbo=>u32),());
    invoke!(r,"glFramebufferTexture2D",(FRAMEBUFFER=>u32,attachment_point(format)=>u32,TEXTURE_2D=>u32,texture=>u32,0=>i32),());
    draw_buffer(r, if format == RGBA { 0x8ce0 } else { 0 })?;
    if invoke!(r,"glCheckFramebufferStatus",(FRAMEBUFFER=>u32),u32) != 0x8cd5 {
        return Err(error("incomplete ES checkpoint staging framebuffer"));
    }
    Ok(())
}
fn prepare(r: &mut Api<'_>, width: i32, height: i32) -> Result<()> {
    for cap in [
        0x0be2, 0x0b44, 0x0b71, 0x0b90, 0x0c11, 0x0bd0, 0x809e, 0x80a0, 0x8037,
    ] {
        invoke!(r,"glDisable",(cap=>u32),());
    }
    invoke!(r,"glColorMask",(1=>u8,1=>u8,1=>u8,1=>u8),());
    invoke!(r,"glActiveTexture",(GL_TEXTURE0=>u32),());
    invoke!(r,"glBindSampler",(0=>u32,0=>u32),());
    invoke!(r,"glViewport",(0=>i32,0=>i32,width=>i32,height=>i32),());
    invoke!(r,"glPixelStorei",(0x0d05=>u32,1=>i32),());
    invoke!(r,"glPixelStorei",(0x0cf5=>u32,1=>i32),());
    Ok(())
}
fn triangle(r: &mut Api<'_>) -> Result<()> {
    invoke!(r,"glDrawArrays",(4=>u32,0=>i32,3=>i32),());
    Ok(())
}
fn mask(format: u32) -> u32 {
    match format {
        DEPTH_STENCIL => 0x500,
        0x1902 => 0x100,
        0x1901 => 0x400,
        _ => 0x4000,
    }
}

/// Owner-side blits preserve the installed guest program. The shared worker
/// packs depth/stencil with GPU draws; only the final byte image crosses to CPU.
pub fn read_framebuffer(r: &mut Api<'_>, aux: &Host, source: u32, image: &mut Plane) -> Result<()> {
    if image.format == RGBA {
        invoke!(r,"glPixelStorei",(0x0d05=>u32,1=>i32),());
        invoke!(r,"glBindFramebuffer",(FRAMEBUFFER=>u32,source=>u32),());
        invoke!(r,"glReadPixels",(0=>i32,0=>i32,image.width=>i32,image.height=>i32,RGBA=>u32,UNSIGNED_BYTE=>u32,image.bytes.as_mut_ptr().cast()=>*mut c_void),());
        return check_error(r, "ES color capture");
    }
    let mut owner = Objects::new();
    let internal = if image.format == 0x1901 {
        0x88f0
    } else {
        image.internal
    };
    let result: Result<()> = (|| {
        let staged = owner.texture(r)?;
        let fbo = owner.framebuffer(r)?;
        texture_storage(r, staged, image.width, image.height, internal)?;
        attach(
            r,
            fbo,
            staged,
            if image.format == 0x1901 {
                DEPTH_STENCIL
            } else {
                image.format
            },
        )?;
        invoke!(r,"glBindFramebuffer",(0x8ca8=>u32,source=>u32),());
        invoke!(r,"glBindFramebuffer",(0x8ca9=>u32,fbo=>u32),());
        invoke!(r,"glDisable",(0x0c11=>u32),());
        invoke!(r,"glBlitFramebuffer",(0=>i32,0=>i32,image.width=>i32,image.height=>i32,0=>i32,0=>i32,image.width=>i32,image.height=>i32,mask(image.format)=>u32,0x2600=>u32),());
        check_error(r, "ES depth/stencil GPU staging")?;
        invoke!(r, "glFinish", (), ());
        aux.make_programmable_current().map_err(error)?;
        let mut temp = Objects::new();
        let captured: Result<()> = (|| {
            prepare(r, image.width, image.height)?;
            let color = temp.texture(r)?;
            let output = temp.framebuffer(r)?;
            texture_storage(r, color, image.width, image.height, 0x8058)?;
            attach(r, output, color, RGBA)?;
            invoke!(r,"glClearColor",(0.0=>f32,0.0=>f32,0.0=>f32,0.0=>f32),());
            invoke!(r,"glClear",(0x4000=>u32),());
            let mut depth_values = vec![0u32; image.width as usize * image.height as usize];
            if image.format != 0x1901 {
                let fs="#version 300 es\nprecision highp float; precision highp int; uniform highp sampler2D plane; uniform int bits; out vec4 color; void main(){uint v=floatBitsToUint(texelFetch(plane,ivec2(gl_FragCoord.xy),0).r); color=vec4(float(v&255u),float((v>>8)&255u),float((v>>16)&255u),float(v>>24))/255.0;}";
                let p = temp.program(r, fs)?;
                uniform(r, p, "plane", 0)?;
                uniform(r, p, "bits", if image.internal == 0x81a5 { 16 } else { 24 })?;
                invoke!(r,"glBindTexture",(TEXTURE_2D=>u32,staged=>u32),());
                triangle(r)?;
                // Sampling a normalized fixed-point depth can round differently
                // from writing gl_FragDepth on a host. Find a representative that
                // passes the real EQUAL test against the saved depth, on the GPU.
                // Never guess a +/- depth epsilon or silently lose a depth bit.
                let exact = temp.texture(r)?;
                let compare = temp.framebuffer(r)?;
                texture_storage(r, exact, image.width, image.height, 0x8058)?;
                attach(r, compare, exact, RGBA)?;
                invoke!(r,"glFramebufferTexture2D",(FRAMEBUFFER=>u32,attachment_point(if image.format==0x1901 {DEPTH_STENCIL}else{image.format})=>u32,TEXTURE_2D=>u32,staged=>u32,0=>i32),());
                invoke!(r,"glClearColor",(0.0=>f32,0.0=>f32,0.0=>f32,0.0=>f32),());
                invoke!(r,"glClear",(0x4000=>u32),());
                let fs="#version 300 es\nprecision highp float; precision highp int; uniform highp sampler2D plane; uniform int delta; out vec4 color; void main(){uvec4 p=uvec4(round(texelFetch(plane,ivec2(gl_FragCoord.xy),0)*255.0)); uint raw=p.r|(p.g<<8)|(p.b<<16)|(p.a<<24); raw=uint(max(0,int(raw)+delta)); float value=uintBitsToFloat(raw); if(value>1.0) discard; gl_FragDepth=value; color=vec4(float(raw&255u),float((raw>>8)&255u),float((raw>>16)&255u),float((raw>>24)|128u))/255.0;}";
                let p = temp.program(r, fs)?;
                uniform(r, p, "plane", 0)?;
                invoke!(r,"glBindTexture",(TEXTURE_2D=>u32,color=>u32),());
                invoke!(r,"glEnable",(0x0b71=>u32),());
                invoke!(r,"glDepthFunc",(0x0202=>u32),());
                invoke!(r,"glDepthMask",(0=>u8),());
                for delta in [-1, 1, 0] {
                    uniform(r, p, "delta", delta)?;
                    triangle(r)?;
                }
                invoke!(r,"glDisable",(0x0b71=>u32),());
                let mut depth = vec![0u8; depth_values.len() * 4];
                invoke!(r,"glReadPixels",(0=>i32,0=>i32,image.width=>i32,image.height=>i32,RGBA=>u32,UNSIGNED_BYTE=>u32,depth.as_mut_ptr().cast()=>*mut c_void),());
                if depth.chunks_exact(4).any(|p| p[3] & 128 == 0) {
                    return Err(error(
                        "host cannot round-trip a saved ES depth value exactly",
                    ));
                }
                image.depth_samples = depth
                    .chunks_exact(4)
                    .flat_map(|p| [p[0], p[1], p[2], p[3] & 127])
                    .collect();
                let maximum = if image.internal == 0x81a5 {
                    65535.0
                } else {
                    16777215.0
                };
                for (out, bytes) in depth_values.iter_mut().zip(depth.chunks_exact(4)) {
                    let value = f32::from_bits(u32::from_le_bytes([
                        bytes[0],
                        bytes[1],
                        bytes[2],
                        bytes[3] & 127,
                    ]));
                    *out = (f64::from(value) * maximum).round() as u32;
                }
                invoke!(r,"glBindFramebuffer",(FRAMEBUFFER=>u32,output=>u32),());
                invoke!(r,"glColorMask",(0=>u8,0=>u8,0=>u8,1=>u8),());
                invoke!(r,"glClearColor",(0.0=>f32,0.0=>f32,0.0=>f32,0.0=>f32),());
                invoke!(r,"glClear",(0x4000=>u32),());
            }
            if image.format != 0x1902 {
                // Stencil sampling is not ES 3.0 core. Eight tests accumulate its
                // actual bits in alpha without modifying the source stencil.
                invoke!(r,"glBindTexture",(TEXTURE_2D=>u32,0=>u32),());
                invoke!(r,"glFramebufferTexture2D",(FRAMEBUFFER=>u32,0x821a=>u32,TEXTURE_2D=>u32,staged=>u32,0=>i32),());
                let fs="#version 300 es\nprecision highp float; uniform int bitValue; out vec4 color; void main(){color=vec4(0,0,0,float(bitValue)/255.0);}";
                let p = temp.program(r, fs)?;
                invoke!(r,"glEnable",(0x0b90=>u32),());
                invoke!(r,"glStencilMask",(0=>u32),());
                invoke!(r,"glStencilOp",(0x1e00=>u32,0x1e00=>u32,0x1e00=>u32),());
                invoke!(r,"glColorMask",(0=>u8,0=>u8,0=>u8,1=>u8),());
                invoke!(r,"glEnable",(0x0be2=>u32),());
                invoke!(r,"glBlendEquation",(0x8006=>u32),());
                invoke!(r,"glBlendFunc",(1=>u32,1=>u32),());
                for bit in 0..8 {
                    let value = 1 << bit;
                    uniform(r, p, "bitValue", value)?;
                    invoke!(r,"glStencilFunc",(0x0202=>u32,value=>i32,value as u32=>u32),());
                    triangle(r)?;
                }
            }
            let mut packed = vec![0; image.width as usize * image.height as usize * 4];
            invoke!(r,"glReadPixels",(0=>i32,0=>i32,image.width=>i32,image.height=>i32,RGBA=>u32,UNSIGNED_BYTE=>u32,packed.as_mut_ptr().cast()=>*mut c_void),());
            check_error(r, "ES packed depth/stencil capture")?;
            for (n, p) in packed.chunks_exact(4).enumerate() {
                let d = depth_values[n];
                match image.format {
                    0x1901 => image.bytes[n] = p[3],
                    DEPTH_STENCIL => image.bytes[n * 4..n * 4 + 4]
                        .copy_from_slice(&((d << 8) | u32::from(p[3])).to_le_bytes()),
                    _ => {
                        let d = if image.internal == 0x81a5 {
                            (d << 16) | d
                        } else {
                            (d << 8) | (d >> 16)
                        };
                        image.bytes[n * 4..n * 4 + 4].copy_from_slice(&d.to_le_bytes());
                    }
                }
            }
            Ok(())
        })();
        let cleanup = temp.cleanup(r);
        let restore = r.host.make_programmable_current().map_err(error);
        captured?;
        cleanup?;
        restore?;
        Ok(())
    })();
    // Always restore ownership before deleting the owner FBO, also on error.
    r.host.make_programmable_current().map_err(error)?;
    let cleanup = owner.cleanup(r);
    result?;
    cleanup
}

/// A non-renderable legacy luminance/alpha texture is sampled, not rasterized
/// on the CPU. BASE/MAX_LEVEL changes are restored before resuming the owner.
pub fn read_texture(
    r: &mut Api<'_>,
    aux: &Host,
    texture: u32,
    face: u32,
    level: i32,
    image: &mut Plane,
) -> Result<()> {
    if image.format != RGBA {
        let mut fbo = 0;
        invoke!(r,"glGenFramebuffers",(1=>i32,&mut fbo=>*mut u32),());
        let result = (|| {
            invoke!(r,"glBindFramebuffer",(FRAMEBUFFER=>u32,fbo=>u32),());
            invoke!(r,"glFramebufferTexture2D",(FRAMEBUFFER=>u32,attachment_point(image.format)=>u32,face=>u32,texture=>u32,level=>i32),());
            draw_buffer(r, 0)?;
            read_framebuffer(r, aux, fbo, image)
        })();
        invoke!(r,"glDeleteFramebuffers",(1=>i32,&fbo=>*const u32),());
        return result;
    }
    let target = if face == TEXTURE_2D {
        TEXTURE_2D
    } else {
        0x8513
    };
    invoke!(r, "glFinish", (), ());
    aux.make_programmable_current().map_err(error)?;
    let mut temp = Objects::new();
    let mut original = [0; 4];
    invoke!(r,"glBindTexture",(target=>u32,texture=>u32),());
    for (p, v) in [0x813c, 0x813d, 0x2801, 0x2800]
        .into_iter()
        .zip(&mut original)
    {
        invoke!(r,"glGetTexParameteriv",(target=>u32,p=>u32,v=>*mut i32),());
    }
    let result: Result<()> = (|| {
        prepare(r, image.width, image.height)?;
        for (p, v) in [
            (0x813c, level),
            (0x813d, level),
            (0x2801, 0x2600),
            (0x2800, 0x2600),
        ] {
            invoke!(r,"glTexParameteri",(target=>u32,p=>u32,v=>i32),());
        }
        let output = temp.framebuffer(r)?;
        let color = temp.texture(r)?;
        texture_storage(r, color, image.width, image.height, 0x8058)?;
        attach(r, output, color, RGBA)?;
        let fs = if target == TEXTURE_2D {
            "#version 300 es\nprecision highp float; uniform highp sampler2D plane; out vec4 color; void main(){color=texelFetch(plane,ivec2(gl_FragCoord.xy),0);}"
        } else {
            "#version 300 es\nprecision highp float; uniform highp samplerCube plane; uniform int face; uniform int width; out vec4 color; void main(){vec2 q=2.0*gl_FragCoord.xy/float(width)-1.0; vec3 d=face==0?vec3(1,-q.y,-q.x):face==1?vec3(-1,-q.y,q.x):face==2?vec3(q.x,1,q.y):face==3?vec3(q.x,-1,-q.y):face==4?vec3(q.x,-q.y,1):vec3(-q.x,-q.y,-1); color=textureLod(plane,d,0.0);}"
        };
        let p = temp.program(r, fs)?;
        uniform(r, p, "plane", 0)?;
        uniform(r, p, "face", face as i32 - 0x8515)?;
        uniform(r, p, "width", image.width)?;
        invoke!(r,"glBindTexture",(target=>u32,texture=>u32),());
        triangle(r)?;
        invoke!(r,"glReadPixels",(0=>i32,0=>i32,image.width=>i32,image.height=>i32,RGBA=>u32,UNSIGNED_BYTE=>u32,image.bytes.as_mut_ptr().cast()=>*mut c_void),());
        check_error(r, "ES texture GPU capture")
    })();
    invoke!(r,"glBindTexture",(target=>u32,texture=>u32),());
    for (p, v) in [0x813c, 0x813d, 0x2801, 0x2800].into_iter().zip(original) {
        invoke!(r,"glTexParameteri",(target=>u32,p=>u32,v=>i32),());
    }
    invoke!(r, "glFinish", (), ());
    let cleanup = temp.cleanup(r);
    let restore = r.host.make_programmable_current().map_err(error);
    result?;
    cleanup?;
    restore
}

pub fn upload_depth_stencil(r: &mut Api<'_>, target: u32, image: &Plane) -> Result<()> {
    upload_plane(r, (RENDERBUFFER, target, 0, 0), image)
}
pub fn upload_texture_depth_stencil(
    r: &mut Api<'_>,
    face: u32,
    level: i32,
    image: &Plane,
) -> Result<()> {
    let target = if face == TEXTURE_2D {
        TEXTURE_2D
    } else {
        0x8513
    };
    let texture = integer(r, if target == TEXTURE_2D { 0x8069 } else { 0x8514 })? as u32;
    let result = upload_plane(r, (0x1702, texture, face, level), image);
    invoke!(r,"glBindTexture",(target=>u32,texture=>u32),());
    result
}
fn upload_plane(r: &mut Api<'_>, attachment: (u32, u32, u32, i32), image: &Plane) -> Result<()> {
    let mut temp = Objects::new();
    let result: Result<()> = (|| {
        image.validate()?;
        prepare(r, image.width, image.height)?;
        let texture = temp.texture(r)?;
        let output = temp.framebuffer(r)?;
        let packed: Vec<u32> = (0..image.width as usize * image.height as usize)
            .flat_map(|n| {
                let value = if image.format == 0x1901 {
                    0
                } else {
                    u32::from_le_bytes(image.bytes[n * 4..n * 4 + 4].try_into().unwrap())
                };
                let (d, stencil, maximum) = match image.format {
                    DEPTH_STENCIL => (value >> 8, value & 255, 16777215.0),
                    0x1901 => (0, u32::from(image.bytes[n]), 16777215.0),
                    _ if image.internal == 0x81a5 => (value >> 16, 0, 65535.0),
                    _ => (value >> 8, 0, 16777215.0),
                };
                [
                    image
                        .depth_samples
                        .get(n * 4..n * 4 + 4)
                        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
                        .unwrap_or_else(|| ((d as f64 / maximum) as f32).to_bits()),
                    stencil,
                ]
            })
            .collect();
        invoke!(r,"glBindTexture",(TEXTURE_2D=>u32,texture=>u32),());
        invoke!(r,"glTexImage2D",(TEXTURE_2D=>u32,0=>i32,0x823c=>i32,image.width=>i32,image.height=>i32,0=>i32,0x8228=>u32,0x1405=>u32,packed.as_ptr().cast()=>*const c_void),());
        invoke!(r,"glTexParameteri",(TEXTURE_2D=>u32,0x2801=>u32,0x2600=>i32),());
        invoke!(r,"glTexParameteri",(TEXTURE_2D=>u32,0x2800=>u32,0x2600=>i32),());
        invoke!(r,"glBindFramebuffer",(FRAMEBUFFER=>u32,output=>u32),());
        if attachment.0 == RENDERBUFFER {
            invoke!(r,"glFramebufferRenderbuffer",(FRAMEBUFFER=>u32,attachment_point(image.format)=>u32,RENDERBUFFER=>u32,attachment.1=>u32),());
        } else {
            invoke!(r,"glFramebufferTexture2D",(FRAMEBUFFER=>u32,attachment_point(image.format)=>u32,attachment.2=>u32,attachment.1=>u32,attachment.3=>i32),());
        }
        draw_buffer(r, 0)?;
        if invoke!(r,"glCheckFramebufferStatus",(FRAMEBUFFER=>u32),u32) != 0x8cd5 {
            return Err(error("incomplete ES depth restore target"));
        }
        let fs="#version 300 es\nprecision highp float; precision highp int; uniform highp usampler2D plane; uniform int bits; uniform int stencilBit; out vec4 color; void main(){uvec2 p=texelFetch(plane,ivec2(gl_FragCoord.xy),0).rg; if(stencilBit!=0 && (p.g&uint(stencilBit))==0u) discard; gl_FragDepth=uintBitsToFloat(p.r);color=vec4(0);}";
        let p = temp.program(r, fs)?;
        uniform(r, p, "plane", 0)?;
        uniform(r, p, "bits", if image.internal == 0x81a5 { 16 } else { 24 })?;
        if image.format != 0x1901 {
            invoke!(r,"glEnable",(0x0b71=>u32),());
            invoke!(r,"glDepthFunc",(0x0207=>u32),());
            invoke!(r,"glDepthMask",(1=>u8),());
            triangle(r)?;
        }
        if image.format != 0x1902 {
            invoke!(r,"glDisable",(0x0b71=>u32),());
            invoke!(r,"glDepthMask",(0=>u8),());
            invoke!(r,"glStencilMask",(255=>u32),());
            invoke!(r,"glClearStencil",(0=>i32),());
            invoke!(r,"glClear",(0x400=>u32),());
            invoke!(r,"glEnable",(0x0b90=>u32),());
            invoke!(r,"glStencilOp",(0x1e01=>u32,0x1e01=>u32,0x1e01=>u32),());
            for bit in 0..8 {
                let value = 1 << bit;
                uniform(r, p, "stencilBit", value)?;
                invoke!(r,"glStencilMask",(value as u32=>u32),());
                invoke!(r,"glStencilFunc",(0x0207=>u32,value=>i32,255=>u32),());
                triangle(r)?;
            }
        }
        check_error(r, "ES GPU depth/stencil restore")
    })();
    let cleanup = temp.cleanup(r);
    result?;
    cleanup
}

/// A private shared context prevents transfers from disturbing the fixed
/// renderer's cached program, textures, vertex bindings or pipeline state.
pub fn capture_renderbuffer(
    owner: &Host,
    buffer: u32,
    width: i32,
    height: i32,
    internal: u32,
) -> Result<Plane> {
    let mut previous = super::platform::SavedCurrent::capture()?;
    let result = (|| {
        owner.make_programmable_current()?;
        let mut source = Api::new(owner);
        if !owner.es || integer(&mut source, 0x821b)? < 3 {
            return Err("GPU checkpoint transfers require host OpenGL ES 3".into());
        }
        invoke!(&mut source, "glFinish", (), ());
        let context = Host::shared(&owner.shared_source())?;
        context.make_programmable_current()?;
        let mut api = Api::new(&context);
        let mut objects = Objects::new();
        let result = (|| {
            let mut plane = Plane::new(width, height, internal)?;
            let fbo = objects.framebuffer(&mut api)?;
            invoke!(&mut api,"glBindFramebuffer",(FRAMEBUFFER=>u32,fbo=>u32),());
            invoke!(&mut api,"glFramebufferRenderbuffer",(FRAMEBUFFER=>u32,attachment_point(plane.format)=>u32,RENDERBUFFER=>u32,buffer=>u32),());
            draw_buffer(&mut api, if plane.format == RGBA { 0x8ce0 } else { 0 })?;
            if invoke!(&mut api,"glCheckFramebufferStatus",(FRAMEBUFFER=>u32),u32) != 0x8cd5 {
                return Err("incomplete shared checkpoint attachment".into());
            }
            read_framebuffer(&mut api, &context, fbo, &mut plane)?;
            Ok(plane)
        })();
        let cleaned = objects.cleanup(&mut api);
        result.and_then(|plane| cleaned.map(|()| plane))
    })();
    let restored = previous.restore();
    restored?;
    result
}
pub fn restore_renderbuffer(owner: &Host, buffer: u32, plane: &Plane) -> Result<()> {
    plane.validate()?;
    let mut previous = super::platform::SavedCurrent::capture()?;
    let result = (|| {
        owner.make_programmable_current()?;
        let mut source = Api::new(owner);
        if !owner.es || integer(&mut source, 0x821b)? < 3 {
            return Err("GPU checkpoint transfers require host OpenGL ES 3".into());
        }
        invoke!(&mut source, "glFinish", (), ());
        let context = Host::shared(&owner.shared_source())?;
        context.make_programmable_current()?;
        let mut api = Api::new(&context);
        upload_depth_stencil(&mut api, buffer, plane)?;
        invoke!(&mut api, "glFinish", (), ());
        Ok(())
    })();
    let restored = previous.restore();
    restored?;
    result
}
