//! ES 1.1 named entry points. Ordinal tables intentionally remain unchanged.
use super::*;
use pocket_gles::context::StateValue;

#[derive(Clone, Copy)]
pub(super) enum Output {
    Bool,
    Int,
    Float,
    Fixed,
}
fn emit(
    ctx: &mut CallCtx<'_>,
    out: u32,
    value: StateValue,
    kind: Output,
) -> Result<DispatchOutcome, KernelError> {
    if out == 0 {
        return Ok(VOID);
    }
    let values = value.numbers();
    let bytes: Vec<u8> = match kind {
        Output::Bool => values.iter().map(|v| u8::from(*v != 0.)).collect(),
        Output::Int => values
            .iter()
            .flat_map(|v| (v.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32).to_le_bytes())
            .collect::<Vec<_>>(),
        Output::Float => values
            .iter()
            .flat_map(|v| (*v as f32).to_le_bytes())
            .collect(),
        Output::Fixed => value.fixed().iter().flat_map(|v| v.to_le_bytes()).collect(),
    };
    ctx.cpu.write_mem(out, &bytes)?;
    Ok(VOID)
}
pub(super) fn query(ctx: &mut CallCtx<'_>, kind: Output) -> Result<DispatchOutcome, KernelError> {
    let pname = ctx.arg_u32(0)?;
    let out = ctx.arg_u32(1)?;
    let value = if matches!(
        pname,
        pocket_gles::GL_PACK_ALIGNMENT | pocket_gles::GL_UNPACK_ALIGNMENT
    ) {
        Some(StateValue::Number(vec![
            if pname == pocket_gles::GL_PACK_ALIGNMENT {
                PACK_ALIGNMENT.with(Cell::get)
            } else {
                UNPACK_ALIGNMENT.with(Cell::get)
            } as f64,
        ]))
    } else if let Some(v) = gl_capability(pname) {
        if matches!(
            pname,
            0x8b9a | 0x8b9b | pocket_gles::GL_COMPRESSED_TEXTURE_FORMATS
        ) {
            Some(StateValue::Enum(v.iter().map(|v| *v as u32).collect()))
        } else {
            Some(StateValue::Number(
                v.iter().map(|v| f64::from(*v)).collect(),
            ))
        }
    } else {
        with_ctx(|c| c.query_state(pname))
    };
    if let Some(mut v) = value {
        if matches!(kind, Output::Int)
            && matches!(
                pname,
                0x0b00 | 0x0b02 | 0x0b70 | 0x0b73 | 0x0c22 | 0x0b53 | 0x0b66
            )
        {
            v = StateValue::Number(v.numbers().iter().map(|v| v * 2147483647.).collect());
        }
        emit(ctx, out, v, kind)
    } else {
        Ok(VOID)
    }
}
pub(super) fn get_boolean(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    query(ctx, Output::Bool)
}
pub(super) fn get_fixed(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    query(ctx, Output::Fixed)
}
pub(super) fn get_pointer(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pname = ctx.arg_u32(0)?;
    let out = ctx.arg_u32(1)?;
    if let Some(v) = with_ctx(|c| c.get_pointer(pname)) {
        if out != 0 {
            ctx.cpu.write_mem(out, &v.to_le_bytes())?;
        }
    }
    Ok(VOID)
}
pub(super) fn get_buffer(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let target = ctx.arg_u32(0)?;
    let pname = ctx.arg_u32(1)?;
    let out = ctx.arg_u32(2)?;
    if let Some(v) = with_ctx(|c| c.get_buffer_parameter(target, pname)) {
        if out != 0 {
            ctx.cpu.write_mem(out, &v.to_le_bytes())?;
        }
    }
    Ok(VOID)
}
pub(super) fn is_enabled(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let cap = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(
        with_ctx(|c| c.is_enabled(cap)).unwrap_or(false) as u32,
    ))
}
pub(super) fn is_texture(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let n = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(
        with_ctx(|c| c.is_texture(n)) as u32
    ))
}
pub(super) fn is_buffer(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let n = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(
        with_ctx(|c| c.is_buffer(n)) as u32
    ))
}
pub(super) fn color4ub(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let rgba = [
        ctx.arg_u32(0)?,
        ctx.arg_u32(1)?,
        ctx.arg_u32(2)?,
        ctx.arg_u32(3)?,
    ]
    .map(|v| f32::from(v as u8) / 255.);
    with_ctx(|c| c.current_color = rgba);
    Ok(VOID)
}
fn clip(ctx: &mut CallCtx<'_>, fixed: bool, get: bool) -> Result<DispatchOutcome, KernelError> {
    let plane = ctx.arg_u32(0)?;
    let ptr = ctx.arg_u32(1)?;
    if get {
        if let Some(v) = with_ctx(|c| c.get_clip_plane(plane)) {
            return emit(
                ctx,
                ptr,
                StateValue::Number(v.map(f64::from).to_vec()),
                if fixed { Output::Fixed } else { Output::Float },
            );
        }
    } else {
        if !(0x3000..=0x3005).contains(&plane) {
            with_ctx(|c| c.set_error(pocket_gles::GL_INVALID_ENUM));
            return Ok(VOID);
        }
        let b = ctx.cpu.read_mem(ptr, 16)?;
        let v = std::array::from_fn(|i| {
            let w = u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
            if fixed {
                word_to_f32(w)
            } else {
                f32::from_bits(w)
            }
        });
        with_ctx(|c| c.clip_plane(plane, v));
    }
    Ok(VOID)
}
pub(super) fn clip_f(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    clip(c, false, false)
}
pub(super) fn clip_x(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    clip(c, true, false)
}
pub(super) fn get_clip_f(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    clip(c, false, true)
}
pub(super) fn get_clip_x(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    clip(c, true, true)
}
fn point(ctx: &mut CallCtx<'_>, fixed: bool, vector: bool) -> Result<DispatchOutcome, KernelError> {
    let pname = ctx.arg_u32(0)?;
    if !matches!(pname, 0x8126..=0x8129) || (!vector && pname == 0x8129) {
        with_ctx(|c| c.set_error(pocket_gles::GL_INVALID_ENUM));
        return Ok(VOID);
    }
    let n = if pname == 0x8129 { 3 } else { 1 };
    let raw = ctx.arg_u32(1)?;
    let words = if vector {
        ctx.cpu
            .read_mem(raw, n * 4)?
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect()
    } else {
        vec![raw]
    };
    let values: Vec<f32> = words
        .into_iter()
        .map(|w| {
            if fixed {
                word_to_f32(w)
            } else {
                f32::from_bits(w)
            }
        })
        .collect();
    with_ctx(|c| c.point_parameter(pname, &values));
    Ok(VOID)
}
pub(super) fn point_f(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    point(c, false, false)
}
pub(super) fn point_x(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    point(c, true, false)
}
pub(super) fn point_fv(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    point(c, false, true)
}
pub(super) fn point_xv(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    point(c, true, true)
}
fn tex_get(ctx: &mut CallCtx<'_>, env: bool, kind: Output) -> Result<DispatchOutcome, KernelError> {
    let target = ctx.arg_u32(0)?;
    let pname = ctx.arg_u32(1)?;
    let out = ctx.arg_u32(2)?;
    let v = with_ctx(|c| {
        if env {
            c.get_tex_env(target, pname)
        } else {
            c.get_tex_parameter(target, pname)
        }
    });
    if let Some(mut v) = v {
        if matches!(kind, Output::Int) && env && pname == pocket_gles::GL_TEXTURE_ENV_COLOR {
            v = StateValue::Number(v.numbers().iter().map(|v| v * 2147483647.).collect());
        }
        emit(ctx, out, v, kind)
    } else {
        Ok(VOID)
    }
}
pub(super) fn get_env_f(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    tex_get(c, true, Output::Float)
}
pub(super) fn get_env_i(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    tex_get(c, true, Output::Int)
}
pub(super) fn get_env_x(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    tex_get(c, true, Output::Fixed)
}
pub(super) fn get_tex_f(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    tex_get(c, false, Output::Float)
}
pub(super) fn get_tex_i(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    tex_get(c, false, Output::Int)
}
pub(super) fn get_tex_x(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    tex_get(c, false, Output::Fixed)
}
fn tex_vector(
    ctx: &mut CallCtx<'_>,
    float: bool,
    fixed: bool,
) -> Result<DispatchOutcome, KernelError> {
    let target = ctx.arg_u32(0)?;
    let pname = ctx.arg_u32(1)?;
    let ptr = ctx.arg_u32(2)?;
    if target != pocket_gles::GL_TEXTURE_2D
        || !matches!(
            pname,
            pocket_gles::GL_TEXTURE_MIN_FILTER
                | pocket_gles::GL_TEXTURE_MAG_FILTER
                | pocket_gles::GL_TEXTURE_WRAP_S
                | pocket_gles::GL_TEXTURE_WRAP_T
                | pocket_gles::GL_TEXTURE_MAX_LEVEL_SGIS
                | pocket_gles::GL_TEXTURE_BASE_LEVEL_SGIS
                | pocket_gles::GL_TEXTURE_MIN_LOD_SGIS
                | pocket_gles::GL_TEXTURE_MAX_LOD_SGIS
                | 0x8191
        )
    {
        with_ctx(|c| c.set_error(pocket_gles::GL_INVALID_ENUM));
        return Ok(VOID);
    }
    let w = ctx.cpu.read_u32_le(ptr)?;
    with_ctx(|c| {
        if float {
            c.tex_parameter_float(target, pname, f32::from_bits(w));
        } else if fixed
            && matches!(
                pname,
                pocket_gles::GL_TEXTURE_MIN_LOD_SGIS | pocket_gles::GL_TEXTURE_MAX_LOD_SGIS
            )
        {
            c.tex_parameter_float(target, pname, word_to_f32(w));
        } else {
            c.tex_parameter(target, pname, w);
        }
    });
    Ok(VOID)
}
pub(super) fn tex_fv(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    tex_vector(c, true, false)
}
pub(super) fn tex_iv(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    tex_vector(c, false, false)
}
pub(super) fn tex_xv(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    tex_vector(c, false, true)
}
pub(super) fn env_i(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let target = ctx.arg_u32(0)?;
    let pname = ctx.arg_u32(1)?;
    let v = ctx.arg_u32(2)?;
    if !valid_env_target(target, pname) || pname == pocket_gles::GL_TEXTURE_ENV_COLOR {
        with_ctx(|c| c.set_error(pocket_gles::GL_INVALID_ENUM));
        return Ok(VOID);
    }
    with_ctx(|c| {
        if pname == 0x8501 {
            c.set_lod_bias(v as i32 as f32)
        } else {
            c.tex_env(pname, v)
        }
    });
    Ok(VOID)
}
pub(super) fn env_iv(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let target = ctx.arg_u32(0)?;
    let pname = ctx.arg_u32(1)?;
    let ptr = ctx.arg_u32(2)?;
    if !valid_env_target(target, pname) {
        with_ctx(|c| c.set_error(pocket_gles::GL_INVALID_ENUM));
        return Ok(VOID);
    }
    let count = if pname == pocket_gles::GL_TEXTURE_ENV_COLOR {
        4
    } else {
        1
    };
    let b = ctx.cpu.read_mem(ptr, count * 4)?;
    if count == 4 {
        let v = std::array::from_fn(|i| {
            i32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap()) as f32 / 2147483647.
        });
        with_ctx(|c| c.set_tex_env_color(v));
    } else {
        let v = u32::from_le_bytes(b[..4].try_into().unwrap());
        with_ctx(|c| {
            if pname == 0x8501 {
                c.set_lod_bias(v as i32 as f32)
            } else {
                c.tex_env(pname, v)
            }
        });
    }
    Ok(VOID)
}

pub(super) fn valid_env_target(target: u32, pname: u32) -> bool {
    if target == pocket_gles::GL_POINT_SPRITE_OES || pname == pocket_gles::GL_COORD_REPLACE_OES {
        return target == pocket_gles::GL_POINT_SPRITE_OES
            && pname == pocket_gles::GL_COORD_REPLACE_OES;
    }
    if pname == 0x8501 {
        target == 0x8500
    } else {
        target == 0x2300
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gx::tests::fresh_kernel;
    use pocket_cpu::{regs::ArmReg, stub::StubCpu, Cpu, Prot};
    use pocket_kernel::{KernelState, Thunk};
    use pocket_pe::ImportBinding;
    struct Guest {
        cpu: StubCpu,
        kernel: KernelState,
        thunk: Thunk,
    }
    impl Guest {
        fn new() -> Self {
            with_ctx(|c| *c = Context::new(8, 8));
            let mut cpu = StubCpu::new();
            cpu.map_region(0x1000, 0x1000, Prot::READ | Prot::WRITE)
                .unwrap();
            Self {
                cpu,
                kernel: fresh_kernel(),
                thunk: Thunk {
                    thunk_va: 0x70000000,
                    iat_va: 0,
                    dll: "libGLES_CM.dll".into(),
                    binding: ImportBinding::Name("test".into()),
                    friendly_name: None,
                },
            }
        }
        fn call(
            &mut self,
            f: fn(&mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError>,
            args: &[u32],
        ) -> u32 {
            for (r, v) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
                .iter()
                .zip(args)
            {
                self.cpu.write_reg(*r, *v).unwrap();
            }
            let mut c = CallCtx {
                cpu: &mut self.cpu,
                kernel: &mut self.kernel,
                thunk: &self.thunk,
            };
            match f(&mut c).unwrap() {
                DispatchOutcome::ReturnedR0(v) => v,
                _ => panic!("unexpected continuation"),
            }
        }
        fn word(&mut self, p: u32) -> u32 {
            self.cpu.read_u32_le(p).unwrap()
        }
        fn words(&mut self, p: u32, v: &[u32]) {
            self.cpu
                .write_mem(
                    p,
                    &v.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>(),
                )
                .unwrap();
        }
    }
    #[test]
    fn boolean_getter_writes_bytes_fixed_getter_scales_numbers_but_not_enums() {
        let mut g = Guest::new();
        g.cpu.write_mem(0x1100, &[0xa5; 24]).unwrap();
        with_ctx(|c| c.current_color = [0., 0.25, 1., 0.]);
        g.call(get_boolean, &[pocket_gles::GL_CURRENT_COLOR, 0x1100]);
        assert_eq!(
            g.cpu.read_mem(0x1100, 8).unwrap(),
            [0, 1, 1, 0, 0xa5, 0xa5, 0xa5, 0xa5]
        );
        g.call(get_fixed, &[pocket_gles::GL_CURRENT_COLOR, 0x1100]);
        assert_eq!(g.word(0x1104), 16384);
        assert_eq!(g.word(0x1108), 65536);
        g.call(get_fixed, &[0x0ba0, 0x1100]);
        assert_eq!(g.word(0x1100), pocket_gles::GL_MODELVIEW);
        g.words(0x1100, &[0x12345678]);
        g.call(get_fixed, &[0xdead, 0x1100]);
        assert_eq!(g.word(0x1100), 0x12345678);
        assert_eq!(with_ctx(|c| c.take_error()), pocket_gles::GL_INVALID_ENUM);
    }
    #[test]
    fn byte_colors_pointers_object_queries_and_buffer_queries_use_real_guest_abi() {
        let mut g = Guest::new();
        g.call(color4ub, &[255, 128, 0, 64]);
        assert_eq!(
            with_ctx(|c| c.current_color),
            [1., 128. / 255., 0., 64. / 255.]
        );
        with_ctx(|c| {
            c.bind_buffer(pocket_gles::GL_ARRAY_BUFFER, 7);
            c.buffer_data(
                pocket_gles::GL_ARRAY_BUFFER,
                20,
                None,
                pocket_gles::GL_DYNAMIC_DRAW,
            );
            c.set_vertex_pointer(3, pocket_gles::GL_FLOAT, 0, 16);
            c.bind_texture(pocket_gles::GL_TEXTURE_2D, 9);
            c.set_capability(pocket_gles::GL_BLEND, true);
        });
        assert_eq!(g.call(is_buffer, &[7]), 1);
        assert_eq!(g.call(is_buffer, &[0]), 0);
        assert_eq!(g.call(is_texture, &[9]), 1);
        assert_eq!(g.call(is_enabled, &[pocket_gles::GL_BLEND]), 1);
        g.call(get_pointer, &[0x808e, 0x1100]);
        assert_eq!(g.word(0x1100), 16);
        g.call(get_buffer, &[pocket_gles::GL_ARRAY_BUFFER, 0x8764, 0x1100]);
        assert_eq!(g.word(0x1100), 20);
    }
    #[test]
    fn clip_plane_float_fixed_setters_and_getters_preserve_eye_space() {
        let mut g = Guest::new();
        g.words(0x1100, &[65536, 0, 0, 0]);
        g.call(clip_x, &[0x3000, 0x1100]);
        g.call(get_clip_f, &[0x3000, 0x1120]);
        assert_eq!(g.word(0x1120), 1f32.to_bits());
        g.words(0x1100, &[0, 1f32.to_bits(), 0, (-0.5f32).to_bits()]);
        g.call(clip_f, &[0x3001, 0x1100]);
        g.call(get_clip_x, &[0x3001, 0x1120]);
        assert_eq!(g.word(0x1124), 65536);
        assert_eq!(g.word(0x112c), (-32768i32) as u32);
    }
    #[test]
    fn all_point_parameter_forms_share_state_and_scalar_attenuation_is_invalid() {
        let mut g = Guest::new();
        g.call(point_f, &[0x8126, 2f32.to_bits()]);
        g.call(point_x, &[0x8127, 16 * 65536]);
        g.words(0x1100, &[1f32.to_bits(), 2f32.to_bits(), 3f32.to_bits()]);
        g.call(point_fv, &[0x8129, 0x1100]);
        g.words(0x1100, &[3 * 65536]);
        g.call(point_xv, &[0x8128, 0x1100]);
        assert_eq!(
            with_ctx(|c| c.query_state(0x8129).unwrap().numbers()),
            vec![1., 2., 3.]
        );
        assert_eq!(
            with_ctx(|c| c.query_state(0x8128).unwrap().numbers()),
            vec![3.]
        );
        g.call(point_f, &[0x8129, 1f32.to_bits()]);
        assert_eq!(with_ctx(|c| c.take_error()), pocket_gles::GL_INVALID_ENUM);
    }
    #[test]
    fn texture_vector_setters_and_all_six_getters_convert_parameter_types() {
        let mut g = Guest::new();
        let target = pocket_gles::GL_TEXTURE_2D;
        let wrap = pocket_gles::GL_TEXTURE_WRAP_S;
        g.words(0x1100, &[pocket_gles::GL_CLAMP_TO_EDGE]);
        g.call(tex_iv, &[target, wrap, 0x1100]);
        g.call(get_tex_f, &[target, wrap, 0x1120]);
        assert_eq!(
            g.word(0x1120),
            (pocket_gles::GL_CLAMP_TO_EDGE as f32).to_bits()
        );
        g.words(0x1100, &[(pocket_gles::GL_REPEAT as f32).to_bits()]);
        g.call(tex_fv, &[target, wrap, 0x1100]);
        g.call(get_tex_i, &[target, wrap, 0x1120]);
        assert_eq!(g.word(0x1120), pocket_gles::GL_REPEAT);
        g.words(0x1100, &[pocket_gles::GL_CLAMP_TO_EDGE]);
        g.call(tex_xv, &[target, wrap, 0x1100]);
        g.call(get_tex_x, &[target, wrap, 0x1120]);
        assert_eq!(g.word(0x1120), pocket_gles::GL_CLAMP_TO_EDGE);
        g.call(
            env_i,
            &[
                0x2300,
                pocket_gles::GL_TEXTURE_ENV_MODE,
                pocket_gles::GL_COMBINE,
            ],
        );
        g.words(0x1100, &[2]);
        g.call(env_iv, &[0x2300, 0x8573, 0x1100]);
        g.call(get_env_x, &[0x2300, 0x8573, 0x1120]);
        assert_eq!(g.word(0x1120), 131072);
        g.call(
            get_env_i,
            &[0x2300, pocket_gles::GL_TEXTURE_ENV_MODE, 0x1120],
        );
        assert_eq!(g.word(0x1120), pocket_gles::GL_COMBINE);
        g.words(0x1100, &[i32::MAX as u32, 0, 0, i32::MAX as u32]);
        g.call(env_iv, &[0x2300, pocket_gles::GL_TEXTURE_ENV_COLOR, 0x1100]);
        g.call(
            get_env_f,
            &[0x2300, pocket_gles::GL_TEXTURE_ENV_COLOR, 0x1120],
        );
        assert_eq!(g.word(0x1120), 1f32.to_bits());
        assert_eq!(g.word(0x112c), 1f32.to_bits());
    }
}

pub(super) fn stencil_func(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let v = [c.arg_u32(0)?, c.arg_u32(1)?, c.arg_u32(2)?];
    with_ctx(|x| x.set_auxiliary(0x0b92, &v));
    Ok(VOID)
}
pub(super) fn stencil_op(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let v = [c.arg_u32(0)?, c.arg_u32(1)?, c.arg_u32(2)?];
    with_ctx(|x| x.set_auxiliary(0x0b94, &v));
    Ok(VOID)
}
pub(super) fn stencil_mask(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let v = c.arg_u32(0)?;
    with_ctx(|x| x.set_auxiliary(0x0b98, &[v]));
    Ok(VOID)
}
pub(super) fn clear_stencil(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let v = c.arg_u32(0)?;
    with_ctx(|x| x.set_auxiliary(0x0b91, &[v]));
    Ok(VOID)
}
pub(super) fn hint(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = c.arg_u32(0)?;
    let v = c.arg_u32(1)?;
    with_ctx(|x| x.set_auxiliary(p, &[v]));
    Ok(VOID)
}
fn coverage(c: &mut CallCtx<'_>, fixed: bool) -> Result<DispatchOutcome, KernelError> {
    let v = c.arg_u32(0)?;
    let invert = c.arg_u32(1)?;
    let v = if fixed { word_to_f32(v).to_bits() } else { v };
    with_ctx(|x| x.set_auxiliary(0x80aa, &[v, invert]));
    Ok(VOID)
}
pub(super) fn sample_f(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    coverage(c, false)
}
pub(super) fn sample_x(c: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    coverage(c, true)
}
