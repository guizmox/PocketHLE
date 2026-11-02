//! ES 1.1 state which is shared by named API entry points and both renderers.
use super::*;
use std::collections::HashSet;

#[derive(Clone)]
pub(super) struct Es11State {
    pub auxiliary: std::collections::HashMap<u32, StateValue>,
    pub combiners: [crate::raster::Combine; 4],
    pub planes: [[f32; 4]; 6],
    pub clip_enabled: [bool; 6],
    pub point_min: f32,
    pub point_max: f32,
    pub point_max_changed: bool,
    pub current_texcoords: [[f32; 4]; 4],
    pub point_fade: f32,
    pub attenuation: [f32; 3],
    pub pretransform: bool,
    pub point_draw: bool,
    pub texture_objects: HashSet<u32>,
    pub buffer_objects: HashSet<u32>,
    pub extra_enables: HashSet<u32>,
    pub dither: bool,
    pub stencil: bool,
}
impl Default for Es11State {
    fn default() -> Self {
        Self {
            auxiliary: [
                (0x0b91, StateValue::Number(vec![0.])),
                (0x0b92, enumeration(0x207)),
                (0x0b93, StateValue::Number(vec![4294967295.])),
                (0x0b94, enumeration(0x1e00)),
                (0x0b95, enumeration(0x1e00)),
                (0x0b96, enumeration(0x1e00)),
                (0x0b97, StateValue::Number(vec![0.])),
                (0x0b98, StateValue::Number(vec![4294967295.])),
                (0x80aa, StateValue::Number(vec![1.])),
                (0x80ab, StateValue::Number(vec![0.])),
                (0x0c50, enumeration(0x1100)),
                (0x0c51, enumeration(0x1100)),
                (0x0c52, enumeration(0x1100)),
                (0x0c54, enumeration(0x1100)),
                (0x8192, enumeration(0x1100)),
                (0x80a8, StateValue::Number(vec![0.])),
                (0x80a9, StateValue::Number(vec![0.])),
                (0x0b13, StateValue::Number(vec![1.])),
                (0x0b23, StateValue::Number(vec![1.])),
            ]
            .into_iter()
            .collect(),
            combiners: [Default::default(); 4],
            planes: [[0.; 4]; 6],
            clip_enabled: [false; 6],
            point_min: 0.,
            point_max: 64.,
            point_max_changed: false,
            current_texcoords: [[0., 0., 0., 1.]; 4],
            point_fade: 1.,
            attenuation: [1., 0., 0.],
            pretransform: false,
            point_draw: false,
            texture_objects: HashSet::new(),
            buffer_objects: HashSet::new(),
            extra_enables: [0x809d].into_iter().collect(),
            dither: true,
            stencil: false,
        }
    }
}
/// Queries preserve the distinction between enums and numbers: GetFixedv must
/// not scale an enum, but must scale integer, boolean and floating-point state.
#[derive(Debug, Clone)]
pub enum StateValue {
    Number(Vec<f64>),
    Enum(Vec<u32>),
}
impl StateValue {
    pub fn numbers(&self) -> Vec<f64> {
        match self {
            Self::Number(v) => v.clone(),
            Self::Enum(v) => v.iter().map(|v| f64::from(*v)).collect(),
        }
    }
    pub fn fixed(&self) -> Vec<i32> {
        match self {
            Self::Enum(v) => v.iter().map(|v| *v as i32).collect(),
            Self::Number(v) => v
                .iter()
                .map(|v| (v * 65536.).round().clamp(i32::MIN as f64, i32::MAX as f64) as i32)
                .collect(),
        }
    }
}
fn number(v: impl IntoIterator<Item = f32>) -> StateValue {
    StateValue::Number(v.into_iter().map(f64::from).collect())
}
fn enumeration(v: u32) -> StateValue {
    StateValue::Enum(vec![v])
}
fn inverse(m: &Matrix4) -> Option<Matrix4> {
    let mut a = [[0f64; 8]; 4];
    for r in 0..4 {
        for c in 0..4 {
            a[r][c] = f64::from(m[c * 4 + r]);
        }
        a[r][4 + r] = 1.;
    }
    for c in 0..4 {
        let p = (c..4).max_by(|&r, &s| a[r][c].abs().total_cmp(&a[s][c].abs()))?;
        if a[p][c] == 0. {
            return None;
        }
        a.swap(c, p);
        let d = a[c][c];
        for k in 0..8 {
            a[c][k] /= d;
        }
        for r in 0..4 {
            if r != c {
                let f = a[r][c];
                for k in 0..8 {
                    a[r][k] -= f * a[c][k];
                }
            }
        }
    }
    Some(std::array::from_fn(|i| a[i % 4][4 + i / 4] as f32))
}
fn inverse_transpose_plane(m: &Matrix4, p: [f32; 4]) -> [f32; 4] {
    // A singular transformation has undefined plane coefficients in GL.
    let Some(inv) = inverse(m) else {
        return [0.; 4];
    };
    std::array::from_fn(|r| (0..4).map(|c| inv[r * 4 + c] * p[c]).sum())
}
impl Context {
    pub fn clip_plane(&mut self, plane: u32, equation: [f32; 4]) {
        let Some(i) = plane.checked_sub(0x3000).filter(|i| *i < 6) else {
            self.set_error(GL_INVALID_ENUM);
            return;
        };
        self.es11.planes[i as usize] = inverse_transpose_plane(self.modelview.current(), equation);
    }
    pub fn get_clip_plane(&mut self, plane: u32) -> Option<[f32; 4]> {
        let Some(i) = plane.checked_sub(0x3000).filter(|i| *i < 6) else {
            self.set_error(GL_INVALID_ENUM);
            return None;
        };
        Some(self.es11.planes[i as usize])
    }
    pub fn point_parameter(&mut self, pname: u32, values: &[f32]) {
        let count = if pname == 0x8129 { 3 } else { 1 };
        if !matches!(pname, 0x8126..=0x8129) {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        if values.len() != count {
            self.set_error(GL_INVALID_ENUM);
            return;
        }
        if values.iter().any(|v| !v.is_finite() || *v < 0.) {
            self.set_error(GL_INVALID_VALUE);
            return;
        }
        match pname {
            0x8126 => self.es11.point_min = values[0],
            0x8127 => {
                self.es11.point_max = values[0];
                self.es11.point_max_changed = true;
            }
            0x8128 => self.es11.point_fade = values[0],
            _ => self.es11.attenuation.copy_from_slice(values),
        }
    }
    pub fn is_texture(&self, name: u32) -> bool {
        name != 0 && self.es11.texture_objects.contains(&name)
    }
    pub fn is_buffer(&self, name: u32) -> bool {
        name != 0 && self.es11.buffer_objects.contains(&name)
    }
    pub fn is_enabled(&mut self, cap: u32) -> Option<bool> {
        let result = match cap {
            GL_DEPTH_TEST => self.state.depth_test,
            GL_ALPHA_TEST => self.state.alpha_test,
            GL_BLEND => self.state.blend,
            GL_COLOR_LOGIC_OP => self.state.logic_op,
            GL_CULL_FACE => self.cull_enabled,
            GL_SCISSOR_TEST => self.scissor_enabled,
            GL_TEXTURE_2D => self.texture_units[self.active_texture as usize].texture_enabled,
            GL_FOG => self.state.fog,
            GL_LIGHTING => self.lighting.enabled,
            GL_NORMALIZE => self.lighting.normalize,
            GL_COLOR_MATERIAL => self.lighting.color_material,
            crate::lighting::RESCALE_NORMAL => self.lighting.rescale,
            GL_POLYGON_OFFSET_FILL => self.state.polygon_offset,
            GL_DITHER => self.es11.dither,
            GL_STENCIL_TEST => self.es11.stencil,
            0x3000..=0x3005 => self.es11.clip_enabled[(cap - 0x3000) as usize],
            0x4000..=0x4007 => self.lighting.lights[(cap - 0x4000) as usize].enabled,
            0x0b10 | 0x0b20 | 0x809d | 0x809e | 0x809f | 0x80a0 => {
                self.es11.extra_enables.contains(&cap)
            }
            GL_VERTEX_ARRAY => self.vertex_array.enabled,
            GL_NORMAL_ARRAY => self.normal_array.enabled,
            GL_COLOR_ARRAY => self.color_array.enabled,
            GL_TEXTURE_COORD_ARRAY => {
                self.texture_units[self.client_active_texture as usize]
                    .texcoord_array
                    .enabled
            }
            _ => {
                self.set_error(GL_INVALID_ENUM);
                return None;
            }
        };
        Some(result)
    }
    pub fn get_pointer(&mut self, pname: u32) -> Option<u32> {
        Some(match pname {
            0x808e => self.vertex_array.pointer,
            0x808f => self.normal_array.pointer,
            0x8090 => self.color_array.pointer,
            0x8092 => {
                self.texture_units[self.client_active_texture as usize]
                    .texcoord_array
                    .pointer
            }
            _ => {
                self.set_error(GL_INVALID_ENUM);
                return None;
            }
        })
    }
    pub fn get_buffer_parameter(&mut self, target: u32, pname: u32) -> Option<i32> {
        if !matches!(target, GL_ARRAY_BUFFER | GL_ELEMENT_ARRAY_BUFFER) {
            self.set_error(GL_INVALID_ENUM);
            return None;
        }
        if !matches!(pname, 0x8764 | 0x8765) {
            self.set_error(GL_INVALID_ENUM);
            return None;
        }
        let Some(buffer) = self
            .bound_buffer_name(target)
            .and_then(|n| self.buffers.get(&n))
        else {
            self.set_error(GL_INVALID_OPERATION);
            return None;
        };
        Some(if pname == 0x8764 {
            buffer.data.len() as i32
        } else {
            buffer.usage as i32
        })
    }
    pub fn get_tex_parameter(&mut self, target: u32, pname: u32) -> Option<StateValue> {
        if target != GL_TEXTURE_2D {
            self.set_error(GL_INVALID_ENUM);
            return None;
        }
        let t = self
            .textures
            .get(&self.texture_units[self.active_texture as usize].bound_texture)
            .cloned()
            .unwrap_or_default();
        let f = |f: crate::texture::Filter| match f {
            crate::texture::Filter::Nearest => GL_NEAREST,
            crate::texture::Filter::Linear => GL_LINEAR,
            crate::texture::Filter::NearestMipmapNearest => 0x2700,
            crate::texture::Filter::LinearMipmapNearest => 0x2701,
            crate::texture::Filter::NearestMipmapLinear => 0x2702,
            crate::texture::Filter::LinearMipmapLinear => 0x2703,
        };
        if pname == GL_TEXTURE_MAX_LEVEL_SGIS {
            return Some(StateValue::Number(vec![f64::from(t.max_level)]));
        }
        if pname == 0x8191 {
            return Some(StateValue::Number(vec![u8::from(t.generate_mipmap) as f64]));
        }
        Some(enumeration(match pname {
            GL_TEXTURE_MIN_FILTER => f(t.min_filter),
            GL_TEXTURE_MAG_FILTER => f(t.mag_filter),
            GL_TEXTURE_WRAP_S => {
                if t.wrap_s == crate::texture::Wrap::Repeat {
                    GL_REPEAT
                } else {
                    GL_CLAMP_TO_EDGE
                }
            }
            GL_TEXTURE_WRAP_T => {
                if t.wrap_t == crate::texture::Wrap::Repeat {
                    GL_REPEAT
                } else {
                    GL_CLAMP_TO_EDGE
                }
            }
            _ => {
                self.set_error(GL_INVALID_ENUM);
                return None;
            }
        }))
    }
    pub fn get_tex_env(&mut self, target: u32, pname: u32) -> Option<StateValue> {
        if target == 0x8500 && pname == 0x8501 {
            return Some(number([
                self.texture_units[self.active_texture as usize].lod_bias
            ]));
        }
        if target != 0x2300 {
            self.set_error(GL_INVALID_ENUM);
            return None;
        }
        let u = self.texture_units[self.active_texture as usize];
        Some(match pname {
            GL_TEXTURE_ENV_COLOR => number(u.tex_env_color),
            GL_TEXTURE_ENV_MODE => enumeration(match u.tex_env {
                TexEnvMode::Modulate => GL_MODULATE,
                TexEnvMode::Replace => GL_REPLACE,
                TexEnvMode::Decal => GL_DECAL,
                TexEnvMode::Blend => GL_BLEND,
                TexEnvMode::Add => GL_ADD,
                TexEnvMode::Combine(_) => GL_COMBINE,
            }),
            _ => match self.es11.combiners[self.active_texture as usize].get(pname) {
                Some(v) => {
                    if matches!(pname, 0x8573 | 0x0d1c) {
                        number([v as f32])
                    } else {
                        enumeration(v)
                    }
                }
                None => {
                    self.set_error(GL_INVALID_ENUM);
                    return None;
                }
            },
        })
    }
    pub fn set_auxiliary(&mut self, pname: u32, values: &[u32]) {
        let update: Vec<(u32, StateValue)> = match pname {
            0x0b92 => {
                if CompareFunc::from_enum(values[0]).is_none() {
                    self.set_error(GL_INVALID_ENUM);
                    return;
                }
                vec![
                    (pname, enumeration(values[0])),
                    (0x0b97, StateValue::Number(vec![0.])),
                    (0x0b93, StateValue::Number(vec![f64::from(values[2])])),
                ]
            }
            0x0b94 => {
                if values
                    .iter()
                    .any(|v| !matches!(*v, 0 | 0x1e00 | 0x1e01 | 0x1e02 | 0x1e03 | 0x150a))
                {
                    self.set_error(GL_INVALID_ENUM);
                    return;
                }
                vec![
                    (0x0b94, enumeration(values[0])),
                    (0x0b95, enumeration(values[1])),
                    (0x0b96, enumeration(values[2])),
                ]
            }
            0x0b91 => vec![(pname, StateValue::Number(vec![0.]))],
            0x0b98 => vec![(pname, StateValue::Number(vec![f64::from(values[0])]))],
            0x0bf0 => {
                if !(0x1500..=0x150f).contains(&values[0]) {
                    self.set_error(GL_INVALID_ENUM);
                    return;
                }
                self.logic_op(values[0]);
                return;
            }
            0x80aa => vec![
                (pname, number([f32::from_bits(values[0]).clamp(0., 1.)])),
                (0x80ab, number([u8::from(values[1] != 0) as f32])),
            ],
            0x0c50 | 0x0c51 | 0x0c52 | 0x0c54 | 0x8192 => {
                if !matches!(values[0], 0x1100..=0x1102) {
                    self.set_error(GL_INVALID_ENUM);
                    return;
                }
                vec![(pname, enumeration(values[0]))]
            }
            _ => {
                self.set_error(GL_INVALID_ENUM);
                return;
            }
        };
        for (key, value) in update {
            self.es11.auxiliary.insert(key, value);
        }
    }
    pub fn query_state(&mut self, pname: u32) -> Option<StateValue> {
        if let Some(v) = self.es11.auxiliary.get(&pname) {
            return Some(v.clone());
        }
        let blend = |b: BlendFactor| match b {
            BlendFactor::Zero => 0,
            BlendFactor::One => 1,
            BlendFactor::SrcColor => 0x300,
            BlendFactor::OneMinusSrcColor => 0x301,
            BlendFactor::SrcAlpha => 0x302,
            BlendFactor::OneMinusSrcAlpha => 0x303,
            BlendFactor::DstAlpha => 0x304,
            BlendFactor::OneMinusDstAlpha => 0x305,
            BlendFactor::DstColor => 0x306,
            BlendFactor::OneMinusDstColor => 0x307,
            BlendFactor::SrcAlphaSaturate => 0x308,
        };
        let v = match pname {
            0x0b74 => enumeration(self.state.depth_func as u32 + 0x200),
            0x0bc1 => enumeration(self.state.alpha_func as u32 + 0x200),
            0x0bc2 => number([self.state.alpha_ref]),
            GL_LOGIC_OP_MODE => enumeration(self.state.logic_mode),
            GL_COLOR_LOGIC_OP => number([if self.state.logic_op { 1. } else { 0. }]),
            0x0be0 => enumeration(blend(self.state.blend_dst)),
            0x0be1 => enumeration(blend(self.state.blend_src)),
            0x0b44 => enumeration(match self.cull_mode {
                CullMode::Front => 0x404,
                CullMode::Back => 0x405,
                CullMode::FrontAndBack => 0x408,
            }),
            0x0b46 => enumeration(match self.state.front_face {
                FrontFace::Cw => 0x900,
                FrontFace::Ccw => 0x901,
            }),
            0x0b54 => enumeration(if self.state.smooth_shading {
                0x1d01
            } else {
                0x1d00
            }),
            0x0b52 => number([u8::from(self.lighting.two_sided) as f32]),
            0x0b53 => number(self.lighting.model_ambient),
            0x0b62 => number([self.state.fog_density]),
            0x0b63 => number([self.state.fog_start]),
            0x0b64 => number([self.state.fog_end]),
            0x0b65 => enumeration(match self.state.fog_mode {
                FogMode::Linear => 0x2601,
                FogMode::Exp => 0x800,
                FogMode::Exp2 => 0x801,
            }),
            0x0b66 => number(self.state.fog_color),
            0x8038 => number([self.state.polygon_factor]),
            0x2a00 => number([self.state.polygon_units]),
            0x0ba3 => number([self.modelview.depth() as f32]),
            0x0ba4 => number([self.projection.depth() as f32]),
            0x0ba5 => number([self.texture_matrix[self.active_texture as usize].depth() as f32]),
            0x807a | 0x807b | 0x807c | 0x807e | 0x807f | 0x8081 | 0x8082 | 0x8083 | 0x8088
            | 0x8089 | 0x808a | 0x8896 | 0x8897 | 0x8898 | 0x889a => {
                let a = match pname {
                    0x807a | 0x807b | 0x807c | 0x8896 => self.vertex_array,
                    0x807e | 0x807f | 0x8897 => self.normal_array,
                    0x8081 | 0x8082 | 0x8083 | 0x8898 => self.color_array,
                    _ => self.texture_units[self.client_active_texture as usize].texcoord_array,
                };
                match pname {
                    0x807b | 0x807e | 0x8082 | 0x8089 => enumeration(a.ty),
                    0x8896 | 0x8897 | 0x8898 | 0x889a => {
                        StateValue::Number(vec![f64::from(a.buffer)])
                    }
                    0x807c | 0x807f | 0x8083 | 0x808a => number([a.stride as f32]),
                    _ => number([a.size as f32]),
                }
            }
            GL_MODELVIEW_MATRIX => number(*self.modelview.current()),
            GL_PROJECTION_MATRIX => number(*self.projection.current()),
            GL_TEXTURE_MATRIX => {
                number(*self.texture_matrix[self.active_texture as usize].current())
            }
            GL_CURRENT_COLOR => number(self.current_color),
            0x0b02 => number(self.current_normal),
            0x0b03 => number(self.es11.current_texcoords[self.active_texture as usize]),
            GL_DEPTH_RANGE => number([self.state.depth_range.0, self.state.depth_range.1]),
            0x0b11 => number([self.state.point_size]),
            0x0b21 => number([self.state.line_width]),
            0x8126 => number([self.es11.point_min]),
            0x8127 => number([self.es11.point_max]),
            0x8128 => number([self.es11.point_fade]),
            0x8129 => number(self.es11.attenuation),
            0x0d32 => number([6.]),
            0x0b73 => number([self.clear_depth]),
            0x0c22 => number(self.clear_color),
            GL_VIEWPORT => {
                let (x, y, w, h) = self.state.viewport;
                number([x as f32, y as f32, w as f32, h as f32])
            }
            0x0c10 => {
                let (x, y, w, h) = self.scissor_box.unwrap_or((
                    0,
                    0,
                    self.target.width as i32,
                    self.target.height as i32,
                ));
                number([x as f32, y as f32, w as f32, h as f32])
            }
            0x0c23 => number(self.state.color_mask.map(|v| u8::from(v) as f32)),
            0x0b72 => number([u8::from(self.state.depth_write) as f32]),
            0x0ba0 => enumeration(match self.matrix_mode {
                MatrixMode::Modelview => GL_MODELVIEW,
                MatrixMode::Projection => GL_PROJECTION,
                MatrixMode::Texture => GL_TEXTURE,
            }),
            0x8069 => StateValue::Number(vec![f64::from(
                self.texture_units[self.active_texture as usize].bound_texture,
            )]),
            0x84e0 => enumeration(GL_TEXTURE0 + self.active_texture),
            0x84e1 => enumeration(GL_TEXTURE0 + self.client_active_texture),
            GL_ARRAY_BUFFER_BINDING => StateValue::Number(vec![f64::from(self.array_buffer)]),
            GL_ELEMENT_ARRAY_BUFFER_BINDING => {
                StateValue::Number(vec![f64::from(self.element_array_buffer)])
            }
            _ => return self.is_enabled(pname).map(|v| number([u8::from(v) as f32])),
        };
        Some(v)
    }
    pub(super) fn clip_draw_planes(&self) -> Vec<[f32; 4]> {
        self.es11
            .planes
            .iter()
            .zip(self.es11.clip_enabled)
            .filter(|(_, on)| *on)
            .map(|(p, _)| *p)
            .collect()
    }
    pub(super) fn derived_point_size(&self, eye: [f32; 4]) -> f32 {
        let w = if eye[3] != 0. { eye[3] } else { 1. };
        let d = eye[..3]
            .iter()
            .map(|x| (x / w) * (x / w))
            .sum::<f32>()
            .sqrt();
        let [a, b, c] = self.es11.attenuation;
        let raw = self.state.point_size / (a + b * d + c * d * d).sqrt();
        raw.max(self.es11.point_min).min(self.es11.point_max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Memory(Vec<u8>);
    impl GuestMemory for Memory {
        fn read(&mut self, p: u32, n: usize, out: &mut Vec<u8>) -> bool {
            let Some(b) = self.0.get(p as usize..p as usize + n) else {
                return false;
            };
            out.clear();
            out.extend_from_slice(b);
            true
        }
    }
    fn mesh(c: &mut Context, positions: &[[f32; 3]], mode: u32) {
        let mut bytes = vec![0; 4];
        for p in positions {
            for f in p {
                bytes.extend(f.to_le_bytes());
            }
        }
        c.set_vertex_pointer(3, GL_FLOAT, 0, 4);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        c.draw_arrays(&mut Memory(bytes), mode, 0, positions.len() as u32);
    }
    #[test]
    fn object_names_are_reserved_until_bound_and_deleted_names_cease_to_exist() {
        let mut c = Context::new(8, 8);
        let t = c.gen_textures(1)[0];
        let b = c.gen_buffers(1)[0];
        assert!(!c.is_texture(t));
        assert!(!c.is_buffer(b));
        c.bind_texture(GL_TEXTURE_2D, t);
        c.bind_buffer(GL_ARRAY_BUFFER, b);
        assert!(c.is_texture(t));
        assert!(c.is_buffer(b));
        c.delete_textures(&[t]);
        c.delete_buffers(&[b]);
        assert!(!c.is_texture(t));
        assert!(!c.is_buffer(b));
        c.bind_buffer(0xdead, 77);
        assert!(!c.is_buffer(77));
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
    }
    #[test]
    fn buffer_queries_and_captured_array_pointers_preserve_real_state() {
        let mut c = Context::new(8, 8);
        c.bind_buffer(GL_ARRAY_BUFFER, 9);
        assert_eq!(
            c.get_buffer_parameter(GL_ARRAY_BUFFER, 0x8765),
            Some(GL_STATIC_DRAW as i32)
        );
        c.buffer_data(GL_ARRAY_BUFFER, 7, None, GL_DYNAMIC_DRAW);
        c.set_vertex_pointer(3, GL_FLOAT, 0, 12);
        c.bind_buffer(GL_ARRAY_BUFFER, 10);
        assert_eq!(c.get_pointer(0x808e), Some(12));
        assert_eq!(c.query_state(0x8896).unwrap().numbers(), vec![9.]);
        c.bind_buffer(GL_ARRAY_BUFFER, 9);
        assert_eq!(c.get_buffer_parameter(GL_ARRAY_BUFFER, 0x8764), Some(7));
        assert_eq!(
            c.get_buffer_parameter(GL_ARRAY_BUFFER, 0x8765),
            Some(GL_DYNAMIC_DRAW as i32)
        );
        c.bind_buffer(GL_ARRAY_BUFFER, 0);
        assert_eq!(c.get_buffer_parameter(GL_ARRAY_BUFFER, 0x8764), None);
        assert_eq!(c.take_error(), GL_INVALID_OPERATION);
    }
    #[test]
    fn fixed_queries_distinguish_enums_numbers_and_boolean_state() {
        let mut c = Context::new(8, 8);
        c.set_capability(GL_BLEND, true);
        assert_eq!(c.query_state(GL_BLEND).unwrap().fixed(), vec![65536]);
        assert_eq!(
            c.query_state(0x0ba0).unwrap().fixed(),
            vec![GL_MODELVIEW as i32]
        );
        c.current_color = [0.25, 0.5, 0.75, 1.];
        assert_eq!(
            c.query_state(GL_CURRENT_COLOR).unwrap().fixed(),
            vec![16384, 32768, 49152, 65536]
        );
        assert_eq!(c.is_enabled(0xdeaddead), None);
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
        assert!(c.query_state(0xdeadbeef).is_none());
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
    }
    #[test]
    fn clip_equation_is_captured_in_eye_space_and_queried_without_retransform() {
        let mut c = Context::new(8, 8);
        c.modelview.multiply_by(&matrix::translate(2., 0., 0.));
        c.clip_plane(0x3000, [1., 0., 0., 0.]);
        assert_eq!(c.get_clip_plane(0x3000), Some([1., 0., 0., -2.]));
        c.modelview.load_identity();
        assert_eq!(c.get_clip_plane(0x3000), Some([1., 0., 0., -2.]));
        c.clip_plane(0x3006, [0.; 4]);
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
    }
    fn check_clipped_render(mut c: Context) {
        c.current_color = [1., 0., 0., 1.];
        c.clip_plane(0x3000, [1., 0., 0., 0.]);
        c.set_capability(0x3000, true);
        mesh(
            &mut c,
            &[[-1., -1., 0.], [1., -1., 0.], [0., 1., 0.]],
            GL_TRIANGLES,
        );
        if let Some(g) = c.gpu.as_mut() {
            g.read(&mut c.target).unwrap();
        }
        let pixel = |x: usize, y: usize| c.target.color[(y * 32 + x) * 4];
        assert_eq!(pixel(8, 24), 0);
        assert!(pixel(20, 24) > 200);
        assert!(c.is_enabled(0x3000).unwrap());
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }
    #[test]
    fn software_user_clip_plane_really_removes_half_the_triangle() {
        check_clipped_render(Context::new(32, 32));
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires native GL and GLES2"]
    fn native_user_clip_plane_matches_software_on_both_host_apis() {
        let mut c = Context::new(32, 32);
        c.enable_gpu().unwrap();
        check_clipped_render(c);
        let mut c = Context::new(32, 32);
        c.gpu = Some(crate::gpu::Renderer::es2_for_test(32, 32).unwrap());
        check_clipped_render(c);
    }
    fn check_points(mut c: Context) {
        c.state.point_size = 8.;
        c.point_parameter(0x8129, &[0., 0., 1.]);
        c.current_color = [0., 1., 0., 1.];
        // Same projected centre, distance 2: width is 8/2 = 4, 16 pixels.
        mesh(&mut c, &[[0., 0., 0.]], GL_POINTS);
        // Zero distance with zero constant attenuation saturates to user max.
        c.clear(GL_COLOR_BUFFER_BIT);
        c.modelview.multiply_by(&matrix::translate(0., 0., -2.));
        c.projection.multiply_by(&matrix::scale(1., 1., 0.1));
        mesh(&mut c, &[[0., 0., 0.]], GL_POINTS);
        if let Some(g) = c.gpu.as_mut() {
            g.read(&mut c.target).unwrap();
        }
        assert_eq!(
            c.target
                .color
                .chunks_exact(4)
                .filter(|p| p[1] > 200)
                .count(),
            16
        );
    }
    #[test]
    fn software_point_attenuation_changes_the_rasterized_area() {
        check_points(Context::new(32, 32));
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires native GL and GLES2"]
    fn native_point_attenuation_matches_software() {
        let mut c = Context::new(32, 32);
        c.enable_gpu().unwrap();
        check_points(c);
        let mut c = Context::new(32, 32);
        c.gpu = Some(crate::gpu::Renderer::es2_for_test(32, 32).unwrap());
        check_points(c);
    }
    #[test]
    fn point_parameter_errors_leave_previous_state_unchanged() {
        let mut c = Context::new(8, 8);
        c.point_parameter(0x8126, &[2.]);
        c.point_parameter(0x8126, &[-1.]);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        assert_eq!(c.query_state(0x8126).unwrap().numbers(), vec![2.]);
        c.point_parameter(0x8129, &[1.]);
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
    }
    fn check_combiner(mut c: Context) {
        c.bind_texture(GL_TEXTURE_2D, 1);
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            1,
            1,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[64, 128, 192, 255],
        );
        c.set_capability(GL_TEXTURE_2D, true);
        c.current_color = [0.25, 0.5, 0.75, 1.];
        c.tex_env(GL_TEXTURE_ENV_MODE, GL_COMBINE);
        c.tex_env(0x8571, 0x8575); // RGB interpolation
        c.tex_env(0x8580, 0x1702);
        c.tex_env(0x8581, 0x8577);
        c.tex_env(0x8582, 0x8576);
        c.set_tex_env_color([0.5; 4]);
        mesh(
            &mut c,
            &[[-1., -1., 0.], [1., -1., 0.], [0., 1., 0.]],
            GL_TRIANGLES,
        );
        if let Some(g) = c.gpu.as_mut() {
            g.read(&mut c.target).unwrap();
        }
        let at = (24 * 32 + 16) * 4;
        for (actual, want) in c.target.color[at..at + 4].iter().zip([64u8, 128, 192, 255]) {
            assert!(actual.abs_diff(want) <= 1, "{actual} != {want}");
        }
        assert_eq!(
            c.get_tex_env(0x2300, GL_TEXTURE_ENV_MODE)
                .unwrap()
                .numbers(),
            vec![f64::from(GL_COMBINE)]
        );
        c.clear(GL_COLOR_BUFFER_BIT);
        c.current_color = [0.75; 4];
        c.tex_image_2d(
            GL_TEXTURE_2D,
            0,
            1,
            1,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            &[191, 191, 191, 255],
        );
        c.tex_env(0x8571, 0x86af);
        c.tex_env(0x8573, 1);
        c.tex_env(0x0d1c, 2);
        mesh(
            &mut c,
            &[[-1., -1., 0.], [1., -1., 0.], [0., 1., 0.]],
            GL_TRIANGLES,
        );
        if let Some(g) = c.gpu.as_mut() {
            g.read(&mut c.target).unwrap();
        }
        for pixel in &c.target.color[at..at + 3] {
            assert!(pixel.abs_diff(191) <= 1);
        }
        assert_eq!(c.target.color[at + 3], 255);
        assert_eq!(c.take_error(), GL_NO_ERROR);
    }
    #[test]
    fn software_texture_combine_is_not_silently_replaced_by_modulate() {
        check_combiner(Context::new(32, 32));
    }
    #[cfg(unix)]
    #[test]
    #[ignore = "requires native GL and GLES2"]
    fn native_texture_combine_matches_software() {
        let mut c = Context::new(32, 32);
        c.enable_gpu().unwrap();
        check_combiner(c);
        let mut c = Context::new(32, 32);
        c.gpu = Some(crate::gpu::Renderer::es2_for_test(32, 32).unwrap());
        check_combiner(c);
    }
    #[test]
    fn texture_queries_retain_per_unit_settings_and_numeric_scale_type() {
        let mut c = Context::new(8, 8);
        c.tex_env(0x8573, 2);
        c.tex_env(GL_TEXTURE_ENV_MODE, GL_COMBINE);
        c.set_active_texture(1);
        c.tex_env(GL_TEXTURE_ENV_MODE, GL_REPLACE);
        assert_eq!(c.get_tex_env(0x2300, 0x8573).unwrap().fixed(), vec![65536]);
        c.set_active_texture(0);
        assert_eq!(c.get_tex_env(0x2300, 0x8573).unwrap().fixed(), vec![131072]);
        c.tex_env(0x8573, 3);
        assert_eq!(c.take_error(), GL_INVALID_VALUE);
        assert_eq!(c.get_tex_env(0x2300, 0x8573).unwrap().numbers(), vec![2.]);
        c.bind_texture(GL_TEXTURE_2D, 1);
        c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
        assert_eq!(
            c.get_tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S)
                .unwrap()
                .fixed(),
            vec![GL_CLAMP_TO_EDGE as i32]
        );
    }
    #[test]
    fn auxiliary_queries_keep_state_even_without_stencil_or_multisample_attachments() {
        let mut c = Context::new(8, 8);
        c.set_auxiliary(0x0c54, &[0x1102]);
        assert_eq!(c.query_state(0x0c54).unwrap().fixed(), vec![0x1102]);
        c.set_auxiliary(0x0c54, &[0xdead]);
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
        assert_eq!(c.query_state(0x0c54).unwrap().fixed(), vec![0x1102]);
        c.set_auxiliary(0x80aa, &[0.5f32.to_bits(), 1]);
        assert_eq!(c.query_state(0x80aa).unwrap().fixed(), vec![32768]);
        assert_eq!(c.query_state(0x80ab).unwrap().fixed(), vec![65536]);
        assert_eq!(c.query_state(0x80a8).unwrap().numbers(), vec![0.]);
        c.set_auxiliary(0x0b92, &[0x201, 12, 0x1234]);
        assert_eq!(c.query_state(0x0b92).unwrap().fixed(), vec![0x201]);
        assert_eq!(c.query_state(0x0b97).unwrap().numbers(), vec![0.]);
        assert_eq!(c.query_state(0x0b93).unwrap().numbers(), vec![4660.]);
    }
    #[test]
    fn user_clip_planes_do_not_require_inverting_the_projection_matrix() {
        let mut c = Context::new(32, 32);
        c.projection.multiply_by(&matrix::scale(1., 1., 0.));
        check_clipped_render(c);
    }
    #[test]
    fn current_texture_coordinate_queries_preserve_r_and_q_per_unit() {
        let mut c = Context::new(8, 8);
        c.set_multi_texcoord4(GL_TEXTURE0 + 1, [1., 2., 3., 4.]);
        c.set_active_texture(1);
        assert_eq!(
            c.query_state(0x0b03).unwrap().numbers(),
            vec![1., 2., 3., 4.]
        );
        c.set_active_texture(0);
        assert_eq!(
            c.query_state(0x0b03).unwrap().numbers(),
            vec![0., 0., 0., 1.]
        );
    }
    #[test]
    fn object_name_wraparound_never_returns_zero_or_overwrites_an_existing_object() {
        let mut c = Context::new(8, 8);
        c.bind_texture(GL_TEXTURE_2D, u32::MAX);
        c.bind_buffer(GL_ARRAY_BUFFER, u32::MAX);
        let t = c.gen_textures(1)[0];
        let b = c.gen_buffers(1)[0];
        assert_ne!(t, 0);
        assert_ne!(t, u32::MAX);
        assert_ne!(b, 0);
        assert_ne!(b, u32::MAX);
        assert!(c.is_texture(u32::MAX));
        assert!(c.is_buffer(u32::MAX));
        assert!(!c.is_texture(t));
        assert!(!c.is_buffer(b));
    }
    #[test]
    fn point_distance_is_cartesian_for_homogeneous_vertex_coordinates() {
        let mut c = Context::new(8, 8);
        c.state.point_size = 8.;
        c.point_parameter(0x8129, &[0., 0., 1.]);
        assert_eq!(c.derived_point_size([0., 0., -4., 2.]), 4.);
    }
}
