//! CGdiObject ownership operations verified against the CE3 ARM runtime.
use super::*;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        800,
        "?Attach@CGdiObject@@QAAHPAX@Z",
        attach as crate::Handler,
    ),
    (1040, "?DeleteObject@CGdiObject@@QAAHXZ", delete),
];

fn attach(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let handle = ctx.arg_u32(1)?;
    let slot = object
        .checked_add(4)
        .ok_or_else(|| KernelError::Loader("MFC GDI object address overflow".into()))?;
    ctx.cpu
        .check_guest_access(slot, 4, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    if handle == 0 || word(ctx, slot)? != 0 || ctx.kernel.gdi.get(handle).is_none() {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    put(ctx, slot, handle)?;
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn delete(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let slot = object
        .checked_add(4)
        .ok_or_else(|| KernelError::Loader("MFC GDI object address overflow".into()))?;
    let handle = word(ctx, slot)?;
    put(ctx, slot, 0)?;
    ctx.cpu.write_arg(0, handle)?;
    crate::coredll::delete_object(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
    #[test]
    fn attach_and_delete_track_the_real_gdi_handle_and_detach_the_object() {
        let (mut cpu, mut k, mut d) = setup();
        let bitmap = k.gdi.create_compatible_bitmap(2, 2);
        let DispatchOutcome::ReturnedR0(result) = call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:800",
            &[0x2000, bitmap],
        ) else {
            panic!()
        };
        assert_eq!(result, 1);
        assert_eq!(cpu.read_u32_le(0x2004).unwrap(), bitmap);
        let DispatchOutcome::ReturnedR0(result) = call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1040",
            &[0x2000],
        ) else {
            panic!()
        };
        assert_eq!(result, 1);
        assert_eq!(cpu.read_u32_le(0x2004).unwrap(), 0);
        assert!(k.gdi.bitmap(bitmap).is_none());
    }
}
