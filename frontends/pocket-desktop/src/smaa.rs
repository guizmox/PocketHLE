//! Reference SMAA 1x (three passes and lookup textures), before display scaling.
//! Runs at guest resolution and caches the result until a new snapshot arrives.
use eframe::glow::{self, HasContext};
const COMMON: &str = include_str!("shaders/smaa/SMAA.glsl");
const VERTEX: &str = include_str!("shaders/reconstruction.vert");
const EDGES: &str = include_str!("shaders/smaa/edges.frag");
const WEIGHTS: &str = include_str!("shaders/smaa/weights.frag");
const BLEND: &str = include_str!("shaders/smaa/blend.frag");

pub struct Smaa {
    programs: Vec<glow::Program>,
    vao: glow::VertexArray,
    area: glow::Texture,
    search: glow::Texture,
    targets: Vec<(glow::Texture, glow::Framebuffer)>,
    size: [u32; 2],
    valid: bool,
}
fn prefix(gl: &glow::Context) -> &'static str {
    if gl.version().is_embedded {
        "#version 300 es\nprecision highp float;\nprecision highp int;\n"
    } else {
        "#version 140\n"
    }
}
fn program(gl: &glow::Context, fragment: &str) -> Result<glow::Program, String> {
    unsafe {
        let p = gl.create_program()?;
        let mut shaders = Vec::new();
        let header=format!("{}\nuniform vec4 u_metrics;\nuniform float u_threshold;\n#define SMAA_RT_METRICS u_metrics\n#define SMAA_GLSL_3 1\n#define SMAA_MAX_SEARCH_STEPS 16\n#define SMAA_MAX_SEARCH_STEPS_DIAG 8\n#define SMAA_CORNER_ROUNDING 25\n#define SMAA_THRESHOLD u_threshold\n",prefix(gl));
        for (kind, source) in [
            (glow::VERTEX_SHADER, format!("{}{VERTEX}", prefix(gl))),
            (
                glow::FRAGMENT_SHADER,
                format!("{header}{COMMON}\n{fragment}"),
            ),
        ] {
            let s = match gl.create_shader(kind) {
                Ok(s) => s,
                Err(e) => {
                    for s in shaders {
                        gl.delete_shader(s);
                    }
                    gl.delete_program(p);
                    return Err(e);
                }
            };
            gl.shader_source(s, &source);
            gl.compile_shader(s);
            if !gl.get_shader_compile_status(s) {
                let e = gl.get_shader_info_log(s);
                gl.delete_shader(s);
                for s in shaders {
                    gl.delete_shader(s);
                }
                gl.delete_program(p);
                return Err(e);
            }
            gl.attach_shader(p, s);
            shaders.push(s);
        }
        gl.link_program(p);
        for s in shaders {
            gl.detach_shader(p, s);
            gl.delete_shader(s);
        }
        if !gl.get_program_link_status(p) {
            let e = gl.get_program_info_log(p);
            gl.delete_program(p);
            return Err(e);
        }
        Ok(p)
    }
}
fn texture(
    gl: &glow::Context,
    size: [u32; 2],
    internal: u32,
    format: u32,
    bytes: Option<&[u8]>,
) -> Result<glow::Texture, String> {
    unsafe {
        let t = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D, Some(t));
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            internal as i32,
            size[0] as i32,
            size[1] as i32,
            0,
            format,
            glow::UNSIGNED_BYTE,
            bytes,
        );
        for n in [glow::TEXTURE_MIN_FILTER, glow::TEXTURE_MAG_FILTER] {
            gl.tex_parameter_i32(glow::TEXTURE_2D, n, glow::LINEAR as i32);
        }
        for n in [glow::TEXTURE_WRAP_S, glow::TEXTURE_WRAP_T] {
            gl.tex_parameter_i32(glow::TEXTURE_2D, n, glow::CLAMP_TO_EDGE as i32);
        }
        Ok(t)
    }
}
impl Smaa {
    pub fn new(gl: &glow::Context) -> Result<Self, String> {
        unsafe {
            let vao = gl.create_vertex_array()?;
            let area = match texture(
                gl,
                [160, 560],
                glow::RG8,
                glow::RG,
                Some(include_bytes!("shaders/smaa/AreaTex.bin")),
            ) {
                Ok(t) => t,
                Err(e) => {
                    gl.delete_vertex_array(vao);
                    return Err(e);
                }
            };
            let search = match texture(
                gl,
                [64, 16],
                glow::R8,
                glow::RED,
                Some(include_bytes!("shaders/smaa/SearchTex.bin")),
            ) {
                Ok(t) => t,
                Err(e) => {
                    gl.delete_texture(area);
                    gl.delete_vertex_array(vao);
                    return Err(e);
                }
            };
            let mut result = Self {
                programs: Vec::new(),
                vao,
                area,
                search,
                targets: Vec::new(),
                size: [0, 0],
                valid: false,
            };
            for source in [EDGES, WEIGHTS, BLEND] {
                match program(gl, source) {
                    Ok(p) => result.programs.push(p),
                    Err(e) => {
                        result.destroy(gl);
                        return Err(e);
                    }
                }
            }
            gl.bind_texture(glow::TEXTURE_2D, None);
            Ok(result)
        }
    }
    fn targets(&mut self, gl: &glow::Context, size: [u32; 2]) -> Result<(), String> {
        if self.size == size && self.targets.len() == 3 {
            return Ok(());
        }
        self.valid = false;
        unsafe {
            for (t, f) in self.targets.drain(..) {
                gl.delete_texture(t);
                gl.delete_framebuffer(f);
            }
            for _ in 0..3 {
                let t = texture(gl, size, glow::RGBA8, glow::RGBA, None)?;
                let f = match gl.create_framebuffer() {
                    Ok(f) => f,
                    Err(e) => {
                        gl.delete_texture(t);
                        return Err(e);
                    }
                };
                self.targets.push((t, f));
                gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(f));
                gl.framebuffer_texture_2d(
                    glow::DRAW_FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(t),
                    0,
                );
                if gl.check_framebuffer_status(glow::DRAW_FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE
                {
                    return Err("incomplete framebuffer".into());
                }
            }
            self.size = size;
        }
        Ok(())
    }
    pub fn process(
        &mut self,
        gl: &glow::Context,
        input: glow::Texture,
        size: [u32; 2],
        soft: bool,
    ) -> Result<(), String> {
        unsafe {
            let framebuffer = std::num::NonZeroU32::new(
                gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING) as u32,
            )
            .map(glow::NativeFramebuffer);
            let mut viewport = [0; 4];
            gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
            let mut clear = [0.0; 4];
            gl.get_parameter_f32_slice(glow::COLOR_CLEAR_VALUE, &mut clear);
            let scissor = gl.is_enabled(glow::SCISSOR_TEST);
            let blend = gl.is_enabled(glow::BLEND);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::BLEND);
            let result = (|| {
                self.valid = false;
                self.targets(gl, size)?;
                gl.viewport(0, 0, size[0] as i32, size[1] as i32);
                gl.bind_vertex_array(Some(self.vao));
                for pass in 0..3 {
                    gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(self.targets[pass].1));
                    gl.clear_color(0.0, 0.0, 0.0, 0.0);
                    gl.clear(glow::COLOR_BUFFER_BIT);
                    let p = self.programs[pass];
                    gl.use_program(Some(p));
                    gl.uniform_4_f32(
                        gl.get_uniform_location(p, "u_metrics").as_ref(),
                        1.0 / size[0] as f32,
                        1.0 / size[1] as f32,
                        size[0] as f32,
                        size[1] as f32,
                    );
                    gl.uniform_1_f32(
                        gl.get_uniform_location(p, "u_threshold").as_ref(),
                        if soft { 0.05 } else { 0.1 },
                    );
                    let bindings = match pass {
                        0 => vec![("u_color", input)],
                        1 => vec![
                            ("u_edges", self.targets[0].0),
                            ("u_area", self.area),
                            ("u_search", self.search),
                        ],
                        _ => vec![("u_color", input), ("u_blend", self.targets[1].0)],
                    };
                    for (unit, (name, t)) in bindings.into_iter().enumerate() {
                        gl.active_texture(glow::TEXTURE0 + unit as u32);
                        gl.bind_texture(glow::TEXTURE_2D, Some(t));
                        gl.uniform_1_i32(gl.get_uniform_location(p, name).as_ref(), unit as i32);
                    }
                    gl.draw_arrays(glow::TRIANGLES, 0, 3);
                }
                self.valid = true;
                Ok(())
            })();
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, framebuffer);
            gl.viewport(viewport[0], viewport[1], viewport[2], viewport[3]);
            gl.clear_color(clear[0], clear[1], clear[2], clear[3]);
            if scissor {
                gl.enable(glow::SCISSOR_TEST);
            }
            if blend {
                gl.enable(glow::BLEND);
            }
            gl.active_texture(glow::TEXTURE0);
            result
        }
    }
    pub fn output(&self, size: [u32; 2]) -> Option<glow::Texture> {
        if self.valid && self.size == size {
            Some(self.targets[2].0)
        } else {
            None
        }
    }
    pub fn destroy(mut self, gl: &glow::Context) {
        unsafe {
            for p in self.programs.drain(..) {
                gl.delete_program(p);
            }
            for (t, f) in self.targets.drain(..) {
                gl.delete_texture(t);
                gl.delete_framebuffer(f);
            }
            gl.delete_texture(self.area);
            gl.delete_texture(self.search);
            gl.delete_vertex_array(self.vao);
        }
    }
}
