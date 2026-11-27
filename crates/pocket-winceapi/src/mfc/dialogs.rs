//! CE3 dialog object lifetime. HWNDs and templates remain owned by WinCE.
use super::*;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (77, "??0CDialog@@QAA@IPAVCWnd@@@Z", construct_id),
    (78, "??0CDialog@@QAA@PBGPAVCWnd@@@Z", construct_named),
    (79, "??0CDialog@@QAA@XZ", construct_empty),
    (231, "??1CDialog@@UAA@XZ", destruct),
    (
        1458,
        "?GetRuntimeClass@CDialog@@UBAPAUCRuntimeClass@@XZ",
        runtime,
    ),
];
fn construct_id(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct(ctx, 1)
}
fn construct_named(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct(ctx, 2)
}
fn construct_empty(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct(ctx, 0)
}
fn construct(ctx: &mut CallCtx<'_>, form: u8) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let template = if form == 0 { 0 } else { ctx.arg_u32(1)? };
    let parent = if form == 0 { 0 } else { ctx.arg_u32(2)? };
    // All constructors first construct CWnd, then zero the 52-byte dialog tail.
    // A named template is borrowed (and can also be MAKEINTRESOURCE); copying
    // or interpreting it here would change native ownership and error timing.
    super::windows::initialize_target(ctx, object, "CDialog", 124)?;
    for (offset, value) in [
        (0x48, 1),
        (0x50, 12),
        (
            0x60,
            if form == 1 {
                template & 0xffff
            } else {
                template
            },
        ),
        (
            0x5c,
            if form == 1 || template >> 16 == 0 {
                template
            } else {
                0
            },
        ),
        (0x70, parent),
    ] {
        put(ctx, object + offset, value)?;
    }
    Ok(DispatchOutcome::ReturnedR0(object))
}
fn runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx, "CDialog",
    )?))
}
fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    super::common_control_lifetime::destruct(ctx, "CDialog", false)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn constructors_preserve_ce_dialog_layout_and_borrow_template_and_parent() {
        let (mut cpu, mut k, mut d) = framework_setup();
        for (ordinal, template, help) in [
            (77, 0x12345678u32, 0x12345678),
            (78, 0x1234, 0x1234),
            (78, 0x2800, 0x2800),
            (78, 0x12342800, 0),
            (79, 0, 0),
        ] {
            cpu.write_mem(0x1ffc, &[0xa5; 132]).unwrap();
            let got = call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                &format!("ord:{ordinal}"),
                &[0x2000, template, 0x3000],
            );
            assert_eq!(result(got), 0x2000);
            assert_eq!(cpu.read_mem(0x1ffc, 4).unwrap(), [0xa5; 4]);
            assert_eq!(cpu.read_mem(0x207c, 4).unwrap(), [0xa5; 4]);
            assert_eq!(cpu.read_u32_le(0x2048).unwrap(), 1);
            assert_eq!(cpu.read_u32_le(0x2050).unwrap(), 12);
            assert_eq!(cpu.read_u32_le(0x205c).unwrap(), help);
            assert_eq!(
                cpu.read_u32_le(0x2060).unwrap(),
                if ordinal == 77 {
                    template & 0xffff
                } else {
                    template
                }
            );
            assert_eq!(
                cpu.read_u32_le(0x2070).unwrap(),
                if ordinal == 79 { 0 } else { 0x3000 }
            );
            assert_eq!(cpu.read_mem(0x2074, 8).unwrap(), [0; 8]);
            // Ordinary destruction retains caller storage and borrowed pointers.
            assert_eq!(
                drive(&mut cpu, &mut k, &mut d, entry(231), |pc, _| panic!(
                    "unexpected callback {pc:x}"
                )),
                0x2000
            );
            assert_ne!(cpu.read_u32_le(0x2000).unwrap(), 0);
        }
    }
    #[test]
    fn scalar_deleting_dialog_releases_only_its_cpp_storage() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let template = k.heap.alloc_cpp(32).unwrap();
        let parent = k.heap.alloc_cpp(72).unwrap();
        let object = k.heap.alloc_cpp(124).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:78",
            &[object, template, parent],
        );
        let alias = "__PocketHLE_MFC_Class:CDialog:1";
        let thunk = pocket_kernel::Thunk {
            thunk_va: k.dynamic_exports[&1][alias],
            iat_va: 0,
            dll: "coredll.dll".into(),
            binding: pocket_pe::ImportBinding::Name(alias.into()),
            friendly_name: None,
        };
        cpu.write_arg(0, object).unwrap();
        cpu.write_arg(1, 1).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, thunk, |pc, _| panic!(
                "unexpected callback {pc:x}"
            )),
            object
        );
        assert_eq!(k.heap.msize(object), None);
        assert_eq!(k.heap.msize(template), Some(32));
        assert_eq!(k.heap.msize(parent), Some(72));
        k.heap.free(template);
        k.heap.free(parent);
    }
}
