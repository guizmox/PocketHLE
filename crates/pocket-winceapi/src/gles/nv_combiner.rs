//! GoForce AR10/AR11 subset for host GPU rendering and software comparison.

use super::{with_ctx, VOID};
use crate::{CallCtx, DispatchOutcome, KernelError, WinCeDispatcher};
use pocket_gles::{
    combiner_program::Program, GL_INVALID_ENUM, GL_INVALID_OPERATION, GL_INVALID_VALUE,
};

pub(super) const TARGET: u32 = 0x890a;

pub(super) fn enabled() -> bool {
    with_ctx(|c| {
        (c.nv_combiner_client && (!c.gpu_active() || c.gpu_supports_combiner()))
            || (!c.gpu_requested()
                && std::env::var_os("POCKETHLE_EXPERIMENTAL_NV_COMBINER").is_some())
    })
}

pub(super) fn diagnostic_extensions() {
    let advertised = enabled();
    let state = with_ctx(|c| {
        [
            advertised,
            c.nv_combiner_client,
            c.gpu_requested(),
            c.gpu_active(),
            c.gpu_supports_combiner(),
        ]
    });
    thread_local! { static QUERIES: std::cell::Cell<u8> = const { std::cell::Cell::new(0) }; }
    if !QUERIES.with(|queries| {
        let count = queries.get();
        if count >= 32 {
            return false;
        }
        queries.set(count + 1);
        true
    }) {
        return;
    }
    log::info!("NV_DIAG event=extensions advertised={} client={} gpu_requested={} gpu_active={} gpu_support={}", state[0], state[1], state[2], state[3], state[4]);
}

fn error(code: u32) -> DispatchOutcome {
    with_ctx(|c| c.set_error(code));
    VOID
}

pub(super) fn set_enabled(on: bool) {
    with_ctx(|c| c.combiner_programs.set_enabled(on));
}

fn generate(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if !enabled() {
        return Ok(error(GL_INVALID_OPERATION));
    }
    let count = ctx.arg_u32(0)?;
    let out = ctx.arg_u32(1)?;
    if count > 4096 || out == 0 {
        return Ok(error(GL_INVALID_VALUE));
    }
    for index in 0..count {
        let Some(name) = with_ctx(|c| c.combiner_programs.generate()) else {
            return Ok(error(pocket_gles::GL_OUT_OF_MEMORY));
        };
        ctx.cpu
            .write_mem(out.wrapping_add(index * 4), &name.to_le_bytes())?;
    }
    Ok(VOID)
}

fn delete(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if !enabled() {
        return Ok(error(GL_INVALID_OPERATION));
    }
    let count = ctx.arg_u32(0)?;
    let pointer = ctx.arg_u32(1)?;
    if count > 4096 || pointer == 0 {
        return Ok(error(GL_INVALID_VALUE));
    }
    for index in 0..count {
        let name = ctx.cpu.read_u32_le(pointer.wrapping_add(index * 4))?;
        with_ctx(|c| c.combiner_programs.delete(name));
    }
    Ok(VOID)
}

fn bind(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if !enabled() {
        return Ok(error(GL_INVALID_OPERATION));
    }
    if ctx.arg_u32(0)? != TARGET {
        return Ok(error(GL_INVALID_ENUM));
    }
    let name = ctx.arg_u32(1)?;
    if !with_ctx(|c| c.combiner_programs.bind(name)) {
        return Ok(error(GL_INVALID_OPERATION));
    }
    Ok(VOID)
}

fn load(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if !enabled() {
        return Ok(error(GL_INVALID_OPERATION));
    }
    if ctx.arg_u32(0)? != TARGET || ctx.arg_u32(1)? != 0x890b {
        return Ok(error(GL_INVALID_ENUM));
    }
    let length = ctx.arg_u32(2)?;
    let pointer = ctx.arg_u32(3)?;
    if !(5..=4096).contains(&length) || pointer == 0 {
        return Ok(error(GL_INVALID_VALUE));
    }
    let bytes = ctx.cpu.read_mem(pointer, length)?;
    let decoded = match Program::decode(&bytes) {
        Ok(program) => program,
        Err(_) => {
            diagnostic_load("decode_rejected", &bytes, None);
            return Ok(error(GL_INVALID_OPERATION));
        }
    };
    let program = match decoded.compile_basic() {
        Ok(program) => program,
        Err(_) => {
            diagnostic_load("compile_rejected", &bytes, None);
            return Ok(error(GL_INVALID_OPERATION));
        }
    };
    diagnostic_load("compiled", &bytes, Some(&program));
    if with_ctx(|c| {
        c.combiner_programs
            .load_bound(program, pocket_gles::raster::MAX_TEXTURE_STAGES)
    })
    .is_err()
    {
        diagnostic_load("unit_rejected", &bytes, None);
        return Ok(error(GL_INVALID_OPERATION));
    }
    diagnostic_load("accepted", &bytes, None);
    Ok(VOID)
}

fn diagnostic_load(
    status: &str,
    bytes: &[u8],
    program: Option<&pocket_gles::combiner_program::BasicProgram>,
) {
    if !pocket_gles::graphics_diagnostic::enabled() {
        return;
    }
    thread_local! { static EVENTS: std::cell::Cell<u16> = const { std::cell::Cell::new(0) }; }
    if !EVENTS.with(|events| {
        let count = events.get();
        if count >= 512 {
            return false;
        }
        events.set(count + 1);
        true
    }) {
        return;
    }
    let key = pocket_gles::combiner_program::diagnostic_fingerprint(bytes);
    log::info!(
        "NV_DIAG event=load status={status} code_hash={key:016x} bytes={} header={:02x?}",
        bytes.len(),
        &bytes[..bytes.len().min(5)]
    );
    if let Some(program) = program {
        log::info!("NV_DIAG event=compiled code_hash={key:016x} program={:016x} framebuffer_fetch={} textures=0x{:x} coordinates=0x{:x}", program.diagnostic_fingerprint(), program.reads_framebuffer(), program.texture_units(), program.coordinate_units());
    }
}

fn query(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if !enabled() {
        return Ok(error(GL_INVALID_OPERATION));
    }
    if ctx.arg_u32(0)? != TARGET {
        return Ok(error(GL_INVALID_ENUM));
    }
    let pointer = ctx.arg_u32(2)?;
    let values: &[u32] = match ctx.arg_u32(1)? {
        0x890c => &[2],
        0x890d => &[u32::from_le_bytes(*b"AR10"), u32::from_le_bytes(*b"AR11")],
        0x88b5 => &[4],
        _ => return Ok(error(GL_INVALID_ENUM)),
    };
    for (index, value) in values.iter().enumerate() {
        ctx.cpu
            .write_mem(pointer.wrapping_add(index as u32 * 4), &value.to_le_bytes())?;
    }
    Ok(VOID)
}

fn parameter(ctx: &mut CallCtx<'_>, float: bool) -> Result<DispatchOutcome, KernelError> {
    if !enabled() {
        return Ok(error(GL_INVALID_OPERATION));
    }
    if ctx.arg_u32(0)? != TARGET {
        return Ok(error(GL_INVALID_ENUM));
    }
    let index = ctx.arg_u32(1)? as usize;
    let mut values = [0; 4];
    for (index, value) in values.iter_mut().enumerate() {
        let raw = ctx.arg_u32(2 + index as u8)?;
        *value = if float {
            (f32::from_bits(raw) * 65536.0).round() as i32
        } else {
            raw as i32
        };
    }
    if !with_ctx(|c| c.combiner_programs.set_constant(index, values)) {
        return Ok(error(GL_INVALID_VALUE));
    }
    Ok(VOID)
}

fn parameter_x(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    parameter(ctx, false)
}

fn parameter_f(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    parameter(ctx, true)
}

const ENTRY_POINTS: &[(&str, crate::Handler)] = &[
    ("glGenProgramsNV", generate),
    ("glDeleteProgramsNV", delete),
    ("glBindProgramNV", bind),
    ("glProgramStringNV", load),
    ("glGetProgramivNV", query),
    ("glProgramEnvParameter4xNV", parameter_x),
    ("glProgramEnvParameter4fNV", parameter_f),
];

pub(super) fn is_entry_point(name: &str) -> bool {
    ENTRY_POINTS.iter().any(|(entry, _)| *entry == name)
}

pub(super) fn register(d: &mut WinCeDispatcher) {
    for &(name, handler) in ENTRY_POINTS {
        d.register_handler("libgles_cm.dll", name, handler);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detected_software_client_gets_combiners_without_gpu() {
        crate::gles::reset_for_session();
        with_ctx(|context| {
            context.nv_combiner_client = true;
            assert!(!context.gpu_requested());
            assert!(!context.gpu_active());
        });
        assert!(enabled());
        with_ctx(|context| {
            context.set_egl_gpu_policy(true);
            assert!(context.gpu_requested());
            assert!(!context.gpu_active());
        });
        assert!(
            enabled(),
            "software fallback also needs the detected extension"
        );
        crate::gles::reset_for_session();
    }
}
