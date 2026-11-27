//! MFC window services delegate to the existing WinCE window/class owner.
use super::*;
const WINDOW_PROC: &str = "__PocketHLE_MFC_WindowProc";
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", WINDOW_PROC, window_proc);
}
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        635,
        "?AfxGetAfxWndProc@@YAP6AJPAUHWND__@@IIJ@ZXZ",
        get_window_proc,
    ),
    (
        688,
        "?AfxRegisterWndClass@@YAPBGIPAUHICON__@@PAUHBRUSH__@@0@Z",
        register_generated_class,
    ),
    (
        687,
        "?AfxRegisterClass@@YAHPAUtagWNDCLASSW@@@Z",
        register_class,
    ),
    (68, "??0CCmdTarget@@QAA@XZ", construct_target),
    (206, "??0CWnd@@QAA@XZ", construct_window),
    (
        1542,
        "?GetRuntimeClass@CWnd@@UBAPAUCRuntimeClass@@XZ",
        runtime_window,
    ),
];
pub(super) fn construct_target_class(
    ctx: &mut CallCtx<'_>,
    class: &str,
    bytes: u32,
) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    initialize_target(ctx, object, class, bytes)?;
    Ok(DispatchOutcome::ReturnedR0(object))
}
pub(super) fn initialize_target(
    ctx: &mut CallCtx<'_>,
    object: u32,
    class: &str,
    bytes: u32,
) -> Result<(), KernelError> {
    ctx.cpu.check_guest_access(
        object,
        bytes,
        pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
    )?;
    let module = super::application::state(ctx)?;
    let table = super::classes::vtable(ctx, class)?;
    ctx.cpu.write_mem(object, &vec![0; bytes as usize])?;
    for (offset, value) in [(0, table), (4, 1), (0x14, 1), (0x1c, module)] {
        put(ctx, object + offset, value)?;
    }
    Ok(())
}
fn construct_target(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct_target_class(ctx, "CCmdTarget", 32)
}
fn construct_window(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct_target_class(ctx, "CWnd", 72)
}
fn runtime_window(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx, "CWnd",
    )?))
}

fn register_class(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pointer = ctx.arg_u32(0)?;
    let raw = ctx.cpu.read_mem(pointer, 40)?;
    let mut fields = [0; 10];
    for (field, bytes) in fields.iter_mut().zip(raw.chunks_exact(4)) {
        *field = u32::from_le_bytes(bytes.try_into().unwrap());
    }
    Ok(DispatchOutcome::ReturnedR0(register_fields(
        ctx,
        fields,
        Some(pointer),
    )?))
}
pub(super) fn register_fields(
    ctx: &mut CallCtx<'_>,
    fields: [u32; 10],
    prototype: Option<u32>,
) -> Result<u32, KernelError> {
    if fields[9] == 0 {
        crate::coredll::set_thread_error(ctx, 87);
        return Ok(0);
    }
    let name = crate::coredll::read_wstr(ctx, fields[9], 128)?;
    if crate::coredll::window_class_info(ctx.kernel, &String::from_utf16_lossy(&name)).is_some() {
        return Ok(1);
    }
    let state = super::application::state(ctx)?;
    let tracked = if ctx.cpu.read_mem(state + 0x14, 1)?[0] != 0 {
        let mut names = crate::coredll::read_wstr(ctx, state + 0x3c, 4096)?;
        if names.len() + name.len() + 2 > 4096 {
            return Err(KernelError::Dispatch(
                "MFC DLL class-registration list is full".into(),
            ));
        }
        names.extend_from_slice(&name);
        names.push(10);
        Some(names)
    } else {
        None
    };
    let atom = crate::coredll::register_window_class(ctx, fields, prototype)?;
    if atom == 0 {
        return Ok(0);
    }
    if let Some(names) = tracked {
        ctx.cpu.write_mem(
            state + 0x3c,
            &names
                .into_iter()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )?;
    }
    Ok(1)
}
pub(super) fn procedure(ctx: &CallCtx<'_>) -> Result<u32, KernelError> {
    let owner = super::application::owner(ctx)?;
    ctx.kernel.dynamic_exports[&owner]
        .get(WINDOW_PROC)
        .copied()
        .ok_or_else(|| KernelError::Dispatch("MFC window procedure export missing".into()))
}
fn get_window_proc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let state = super::application::state(ctx)?;
    Ok(DispatchOutcome::ReturnedR0(word(ctx, state + 0x203c)?))
}
fn register_generated_class(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let style = ctx.arg_u32(0)?;
    let cursor = ctx.arg_u32(1)?;
    let brush = ctx.arg_u32(2)?;
    let icon = ctx.arg_u32(3)?;
    Ok(DispatchOutcome::ReturnedR0(generated_class(
        ctx, style, cursor, brush, icon,
    )?))
}
pub(super) fn generated_class(
    ctx: &mut CallCtx<'_>,
    style: u32,
    cursor: u32,
    brush: u32,
    icon: u32,
) -> Result<u32, KernelError> {
    let state = super::application::state(ctx)?;
    let instance = word(ctx, state + 8)?;
    let thread = super::application::current_thread_state(ctx)?;
    let buffer = thread + 0x60;
    let format = if cursor == 0 && brush == 0 && icon == 0 {
        "Afx:%x:%x"
    } else {
        "Afx:%x:%x:%x:%x:%x"
    };
    let text = crate::coredll::render_printf_values(
        ctx,
        format,
        true,
        &[instance, style, cursor, brush, icon],
    )?;
    let bytes = text
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    if bytes.len() > 0x13c - 0x60 {
        return Err(KernelError::Dispatch(
            "MFC generated class name is too long".into(),
        ));
    }
    ctx.cpu.write_mem(buffer, &bytes)?;
    let procedure = word(ctx, state + 0x203c)?;
    if register_fields(
        ctx,
        [
            style, procedure, 0, 0, instance, icon, cursor, brush, 0, buffer,
        ],
        None,
    )? == 0
    {
        return Err(KernelError::Dispatch(
            "Could not register the MFC window class".into(),
        ));
    }
    Ok(buffer)
}
fn window_proc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    use super::callbacks::{service, Frame};
    use pocket_cpu::regs::ArmReg;
    let frame = Frame::enter(ctx)?;
    let message = frame.arg(ctx, 1)?;
    if frame.fresh {
        let hwnd = frame.arg(ctx, 0)?;
        let object = permanent(ctx, hwnd)?.ok_or_else(|| {
            KernelError::Dispatch(format!("MFC window 0x{hwnd:08x} has no CWnd binding"))
        })?;
        let target = service(ctx, super::window_exceptions::CALL)?;
        let wparam = frame.arg(ctx, 2)?;
        let lparam = frame.arg(ctx, 3)?;
        return frame.call_args(ctx, 1, target, &[object, hwnd, message, wparam, lparam]);
    }
    let returned = ctx.cpu.read_reg(ArmReg::R0)?;
    if message == 2 && frame.phase(ctx)? == 1 {
        // CE lacks WM_NCDESTROY: retain the existing private cleanup message.
        frame.set(ctx, 1, returned)?;
        let hwnd = frame.arg(ctx, 0)?;
        let target = service(ctx, "SendMessageW")?;
        return frame.call_args(ctx, 2, target, &[hwnd, 0x7fff, 0, 0]);
    }
    if message == 2 && frame.phase(ctx)? == 2 {
        let hwnd = frame.arg(ctx, 0)?;
        let procedure = service(ctx, "DefWindowProcW")?;
        let target = service(ctx, "SetWindowLongW")?;
        return frame.call_args(ctx, 3, target, &[hwnd, (-4i32) as u32, procedure]);
    }
    let result = if message == 2 {
        frame.get(ctx, 1)?
    } else {
        returned
    };
    frame.finish(ctx, result)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn generated_classes_reuse_metadata_and_route_bound_windows_to_the_real_virtual_override() {
        use pocket_cpu::regs::ArmReg;
        let (mut cpu, mut k, mut d) = framework_setup();
        let name = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:688",
            &[0, 0, 0, 0],
        ));
        let class = "Afx:10000000:0";
        let procedure = k.dynamic_exports[&1][super::WINDOW_PROC];
        assert_eq!(k.window_class_procs[class], procedure);
        let baseline = k.heap.free_bytes();
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:688",
                &[0, 0, 0, 0]
            )),
            name
        );
        assert_eq!(k.heap.free_bytes(), baseline);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:688",
            &[3, 0, 6, 0],
        );
        let styled = "Afx:10000000:3:0:6:0";
        assert_eq!(k.window_class_procs[styled], procedure);
        assert_eq!(k.gdi.class_values[&(styled.to_lowercase(), -26)], 3);
        assert_eq!(k.gdi.class_values[&(styled.to_lowercase(), -10)], 6);
        let class_count = k.window_class_procs.len();
        k.current_thread = 1;
        let other = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:688",
            &[0, 0, 0, 0],
        ));
        assert_ne!(name, other);
        assert_eq!(k.window_class_procs.len(), class_count);
        k.current_thread = 0;
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2000],
        );
        let table = cpu.read_u32_le(0x2000).unwrap();
        let copied = cpu.read_mem(table, 46 * 4).unwrap();
        cpu.write_mem(0x2400, &copied).unwrap();
        cpu.write_mem(0x2400 + 38 * 4, &0x6000u32.to_le_bytes())
            .unwrap();
        cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
        // The window-creation owner establishes this association. This unit
        // tests routing independently; it does not claim CreateEx support yet.
        k.dynamic_exports
            .get_mut(&1)
            .unwrap()
            .insert("__PocketHLE_MFC_Window:dead0001".into(), 0x2000);
        let thread = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:651",
            &[],
        ));
        cpu.write_mem(thread + 0x3c, &[0x5a; 28]).unwrap();
        let available = k.heap.free_bytes();
        use super::super::window_dispatch::tests::drive_outcomes;
        use pocket_kernel::{DispatchOutcome, Thunk};
        for (i, value) in [0xdead0001u32, 15, 7, 0x1234].into_iter().enumerate() {
            cpu.write_arg(i as u8, value).unwrap();
        }
        let start = Thunk {
            thunk_va: procedure,
            iat_va: 0,
            dll: "coredll.dll".into(),
            binding: pocket_pe::ImportBinding::Name(super::WINDOW_PROC.into()),
            friendly_name: None,
        };
        let answer = drive_outcomes(&mut cpu, &mut k, &mut d, start, 128, |pc, cpu, _, _| {
            assert_eq!(pc, 0x6000);
            for (index, value) in [0xdead0001u32, 15, 7, 0x1234].into_iter().enumerate() {
                assert_eq!(
                    cpu.read_u32_le(thread + 0x3c + index as u32 * 4).unwrap(),
                    value
                );
            }
            assert_eq!(cpu.read_mem(thread + 0x4c, 12).unwrap(), [0x5a; 12]);
            for (register, value) in [
                (ArmReg::R0, 0x2000),
                (ArmReg::R1, 15),
                (ArmReg::R2, 7),
                (ArmReg::R3, 0x1234),
            ] {
                assert_eq!(cpu.read_reg(register).unwrap(), value);
            }
            DispatchOutcome::ReturnedR0(55)
        });
        assert_eq!(answer, 55);
        assert_eq!(cpu.read_mem(thread + 0x3c, 28).unwrap(), [0x5a; 28]);
        assert_eq!(k.heap.free_bytes(), available);
    }
    #[test]
    fn window_constructor_preserves_ce_layout_guards_and_canonical_runtime_identity() {
        let (mut cpu, mut k, mut d) = framework_setup();
        cpu.write_mem(0x1ffc, &[0xa5; 80]).unwrap();
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:206",
                &[0x2000]
            )),
            0x2000
        );
        assert_eq!(cpu.read_mem(0x1ffc, 4).unwrap(), [0xa5; 4]);
        assert_eq!(cpu.read_mem(0x2048, 4).unwrap(), [0xa5; 4]);
        assert_eq!(cpu.read_mem(0x2020, 40).unwrap(), [0; 40]);
        assert_eq!(cpu.read_u32_le(0x2004).unwrap(), 1);
        assert_eq!(cpu.read_u32_le(0x2014).unwrap(), 1);
        let module = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:644",
            &[],
        ));
        assert_eq!(cpu.read_u32_le(0x201c).unwrap(), module);
        let runtime = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1542",
            &[0x2000],
        ));
        assert_eq!(cpu.read_u32_le(runtime + 4).unwrap(), 72);
        assert_eq!(cpu.read_u32_le(runtime + 8).unwrap(), 0xffff);
        assert_ne!(cpu.read_u32_le(runtime + 12).unwrap(), 0);
        let virtual_runtime = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "__PocketHLE_MFC_Class:CWnd:0",
            &[0x2000],
        ));
        assert_eq!(runtime, virtual_runtime);
        let available = k.heap.free_bytes();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2100],
        );
        assert_eq!(k.heap.free_bytes(), available);
        // The CCmdTarget constructor initializes only its own 32-byte base.
        cpu.write_mem(0x2200, &[0x7c; 36]).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:68",
            &[0x2200],
        );
        assert_eq!(cpu.read_mem(0x2220, 4).unwrap(), [0x7c; 4]);
        assert_eq!(cpu.read_u32_le(0x221c).unwrap(), module);
    }
    #[test]
    fn register_class_uses_real_window_metadata_preserves_existing_classes_and_tracks_dll_names() {
        let (mut cpu, mut k, mut d) = framework_setup();
        cpu.map_region(
            0x10000,
            0x1000,
            pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
        )
        .unwrap();
        for (p, s) in [(0x10000, "MfcWindow"), (0x10100, "MfcDllWindow")] {
            cpu.write_mem(
                p,
                &s.encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        let mut fields = [0u32; 10];
        fields[0] = 3;
        fields[1] = 0x12345678;
        fields[3] = 4;
        fields[4] = pocket_kernel::PROCESS_INSTANCE_HANDLE;
        fields[9] = 0x10000;
        let bytes: Vec<_> = fields.into_iter().flat_map(u32::to_le_bytes).collect();
        cpu.write_mem(0x2000, &bytes).unwrap();
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:687",
                &[0x2000]
            )),
            1
        );
        assert_eq!(k.window_class_procs["MfcWindow"], 0x12345678);
        cpu.write_mem(0x2004, &0x87654321u32.to_le_bytes()).unwrap();
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:687",
                &[0x2000]
            )),
            1
        );
        assert_eq!(k.window_class_procs["MfcWindow"], 0x12345678);
        let state = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:644",
            &[],
        ));
        cpu.write_mem(state + 0x14, &[1]).unwrap();
        cpu.write_mem(0x2024, &0x10100u32.to_le_bytes()).unwrap();
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:687",
                &[0x2000]
            )),
            1
        );
        let expected: Vec<_> = "MfcDllWindow\n"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(
            cpu.read_mem(state + 0x3c, expected.len() as u32).unwrap(),
            expected
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:687",
            &[0x2000],
        );
        assert_eq!(
            cpu.read_mem(state + 0x3c, expected.len() as u32).unwrap(),
            expected
        );
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "UnregisterClassW",
                &[0x10100, pocket_kernel::PROCESS_INSTANCE_HANDLE]
            )),
            1
        );
        assert!(!k.window_class_procs.contains_key("MfcDllWindow"));
    }
}

pub(super) fn permanent(ctx: &CallCtx<'_>, hwnd: u32) -> Result<Option<u32>, KernelError> {
    let owner = super::application::owner(ctx)?;
    let exports = &ctx.kernel.dynamic_exports[&owner];
    Ok(exports
        .get(&super::window_handles::thread_key(ctx, hwnd))
        .or_else(|| exports.get(&format!("__PocketHLE_MFC_Window:{hwnd:08x}")))
        .copied())
}
pub(super) fn wrapper(ctx: &mut CallCtx<'_>, hwnd: u32) -> Result<u32, KernelError> {
    super::handle_maps::from_handle(ctx, super::handle_maps::Kind::Wnd, hwnd)
}
