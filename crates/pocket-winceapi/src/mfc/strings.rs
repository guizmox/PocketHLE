//! Unicode CString operations verified against the CE3 ARM runtime.
use super::*;

pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (175, "??0CString@@QAA@ABV0@@Z", copy as crate::Handler),
    (324, "??4CString@@QAAABV0@PBG@Z", assign_wide),
    (361, "??H@YA?AVCString@@ABV0@PBG@Z", concatenate),
    (366, "??YCString@@QAAABV0@PBG@Z", append_wide),
    (1173, "?Format@CString@@QAAXPBGZZ", format),
    (1229, "?GetBuffer@CString@@QAAPAGH@Z", get_buffer),
    (
        1233,
        "?GetBufferSetLength@CString@@QAAPAGH@Z",
        get_buffer_set_length,
    ),
    (2167, "?ReleaseBuffer@CString@@QAAXH@Z", release_buffer),
];

pub(super) fn wide(ctx: &mut CallCtx<'_>, p: u32) -> Result<Vec<u16>, KernelError> {
    if p == 0 {
        return Ok(Vec::new());
    }
    let mut text = Vec::new();
    for i in 0..=0x100000u32 {
        let c = u16::from_le_bytes(
            ctx.cpu
                .read_mem(
                    p.checked_add(i * 2)
                        .ok_or_else(|| KernelError::Loader("MFC string address overflow".into()))?,
                    2,
                )?
                .try_into()
                .unwrap(),
        );
        if c == 0 {
            return Ok(text);
        }
        text.push(c);
    }
    Err(KernelError::Loader("MFC string unterminated".into()))
}
pub(super) fn value(ctx: &mut CallCtx<'_>, object: u32) -> Result<Vec<u16>, KernelError> {
    let p = word(ctx, object)?;
    let header = string_header(p)?;
    let len = word(ctx, header + 4)?;
    let cap = word(ctx, header + 8)?;
    if len > cap || cap > 0x100000 {
        return Err(KernelError::Loader("MFC string length invalid".into()));
    }
    Ok(ctx
        .cpu
        .read_mem(p, len * 2)?
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect())
}
pub(super) fn replace(ctx: &mut CallCtx<'_>, object: u32, text: &[u16]) -> Result<(), KernelError> {
    ctx.cpu
        .check_guest_access(object, 4, pocket_cpu::Prot::WRITE)?;
    let slot = alloc(ctx, 4)?;
    if let Err(e) = new_string(ctx, slot, text) {
        ctx.kernel.heap.free(slot);
        return Err(e);
    }
    let p = word(ctx, slot)?;
    let result = release_string(ctx, object).and_then(|_| put(ctx, object, p));
    if result.is_err() {
        ctx.kernel.heap.free(p - 12);
    }
    ctx.kernel.heap.free(slot);
    result
}
fn copy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (o, other) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?);
    let p = word(ctx, other)?;
    ctx.cpu.check_guest_access(o, 4, pocket_cpu::Prot::WRITE)?;
    let header = string_header(p)?;
    let refs = word(ctx, header)? as i32;
    if refs < 0 {
        let text = value(ctx, other)?;
        new_string(ctx, o, &text)?;
    } else {
        let refs = refs
            .checked_add(1)
            .ok_or_else(|| KernelError::Loader("MFC string reference overflow".into()))?;
        put(ctx, header, refs as u32)?;
        put(ctx, o, p)?;
    }
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn assign_wide(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (o, p) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?);
    let text = wide(ctx, p)?;
    replace(ctx, o, &text)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn concatenate(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (out, left, right) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    let mut text = value(ctx, left)?;
    text.extend(wide(ctx, right)?);
    new_string(ctx, out, &text)?;
    Ok(DispatchOutcome::ReturnedR0(out))
}
fn append_wide(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (o, p) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?);
    let mut text = value(ctx, o)?;
    text.extend(wide(ctx, p)?);
    replace(ctx, o, &text)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn format(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let p = ctx.arg_u32(1)?;
    let fmt = String::from_utf16_lossy(&wide(ctx, p)?);
    let text: Vec<_> = crate::coredll::render_printf(ctx, &fmt, true, 2)?
        .encode_utf16()
        .collect();
    replace(ctx, o, &text)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn reserve(ctx: &mut CallCtx<'_>, o: u32, minimum: u32) -> Result<u32, KernelError> {
    if minimum > 0x100000 {
        return Err(KernelError::Loader("MFC buffer too large".into()));
    }
    ctx.cpu.check_guest_access(o, 4, pocket_cpu::Prot::WRITE)?;
    let old = word(ctx, o)?;
    let header = string_header(old)?;
    let refs = word(ctx, header)? as i32;
    let len = word(ctx, old - 8)?;
    let capacity = word(ctx, old - 4)?;
    if len > capacity || capacity > 0x100000 {
        return Err(KernelError::Loader("MFC buffer metadata invalid".into()));
    }
    if refs <= 1 && capacity >= minimum {
        return Ok(old);
    }
    let cap = minimum.max(len);
    let p = alloc(ctx, 12 + (cap + 1) * 2)?;
    let result = (|| {
        let bytes = ctx.cpu.read_mem(old, (len + 1) * 2)?;
        ctx.cpu.write_mem(p + 12, &bytes)?;
        put(ctx, p, 1)?;
        put(ctx, p + 4, len)?;
        put(ctx, p + 8, cap)?;
        release_string(ctx, o)?;
        put(ctx, o, p + 12)
    })();
    if let Err(e) = result {
        ctx.kernel.heap.free(p);
        return Err(e);
    }
    Ok(p + 12)
}
fn get_buffer(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let n = ctx.arg_u32(1)?;
    Ok(DispatchOutcome::ReturnedR0(reserve(ctx, o, n)?))
}
fn get_buffer_set_length(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let n = ctx.arg_u32(1)?;
    let p = reserve(ctx, o, n)?;
    put(ctx, p - 8, n)?;
    ctx.cpu.write_mem(p + n * 2, &[0, 0])?;
    Ok(DispatchOutcome::ReturnedR0(p))
}
fn release_buffer(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let requested = ctx.arg_u32(1)?;
    let old = word(ctx, o)?;
    let header = string_header(old)?;
    let cap = word(ctx, header + 8)?;
    let p = reserve(ctx, o, cap)?;
    let n = if requested == u32::MAX {
        if cap > 0x100000 {
            return Err(KernelError::Loader("MFC string capacity invalid".into()));
        }
        let bytes = ctx.cpu.read_mem(p, (cap + 1) * 2)?;
        bytes
            .chunks_exact(2)
            .position(|c| c == [0, 0])
            .ok_or_else(|| KernelError::Loader("MFC ReleaseBuffer unterminated".into()))?
            as u32
    } else {
        requested
    };
    if n > cap {
        return Err(KernelError::Loader(
            "MFC ReleaseBuffer exceeds capacity".into(),
        ));
    }
    put(ctx, p - 8, n)?;
    ctx.cpu.write_mem(p + n * 2, &[0, 0])?;
    Ok(DispatchOutcome::ReturnedR0(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::{Cpu, Prot};
    #[test]
    fn shared_cstring_buffer_detaches_and_remains_terminated() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        let initial = kernel.heap.free_bytes();
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "mfcce300.dll",
            "ord:181",
            &[0x2000],
        );
        cpu.write_mem(0x2100, &[b'a', 0, b'b', 0, 0, 0]).unwrap();
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "mfcce300.dll",
            "ord:324",
            &[0x2000, 0x2100],
        );
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "mfcce300.dll",
            "ord:175",
            &[0x2010, 0x2000],
        );
        let original = cpu.read_u32_le(0x2000).unwrap();
        assert_eq!(cpu.read_u32_le(0x2010).unwrap(), original);
        let result = call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "mfcce300.dll",
            "ord:1229",
            &[0x2010, 8],
        );
        let DispatchOutcome::ReturnedR0(buffer) = result else {
            panic!("unexpected outcome")
        };
        assert_ne!(buffer, original);
        cpu.write_mem(buffer, &[b'c', 0, 0, 0]).unwrap();
        call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "mfcce300.dll",
            "ord:2167",
            &[0x2010, u32::MAX],
        );
        assert_eq!(cpu.read_mem(original, 6).unwrap(), [b'a', 0, b'b', 0, 0, 0]);
        assert_eq!(cpu.read_u32_le(buffer - 8).unwrap(), 1);
        for object in [0x2000, 0x2010] {
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "mfcce300.dll",
                "ord:297",
                &[object],
            );
        }
        assert_eq!(kernel.heap.free_bytes(), initial);
    }
}
