//! CWnd construction adapts CREATESTRUCTW and delegates window ownership to WinCE.
use super::callbacks::service;
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const CREATE_EX: &str = "__PocketHLE_MFC_CreateEx";
pub(super) const HOOK: &str = "__PocketHLE_MFC_HookWindowCreate";
pub(super) const UNHOOK: &str = "__PocketHLE_MFC_UnhookWindowCreate";
const INITIAL: &str = "__PocketHLE_MFC_InitialWindowProc";
const PENDING: &str = "__PocketHLE_MFC_Create:";
const ROUTE: &str = "__PocketHLE_MFC_WindowProc";

pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (654, "?AfxHookWindowCreate@@YAXPAVCWnd@@@Z", hook),
    (769, "?AfxUnhookWindowCreate@@YAHXZ", unhook),
    (
        901,
        "?Create@CButton@@QAAHPBGKABUtagRECT@@PAVCWnd@@I@Z",
        create_button,
    ),
    (
        904,
        "?Create@CComboBox@@QAAHKABUtagRECT@@PAVCWnd@@I@Z",
        create_combo,
    ),
    (
        907,
        "?Create@CEdit@@QAAHKABUtagRECT@@PAVCWnd@@I@Z",
        create_edit,
    ),
    (
        916,
        "?Create@CListBox@@QAAHKABUtagRECT@@PAVCWnd@@I@Z",
        create_list,
    ),
    (
        927,
        "?Create@CStatic@@QAAHPBGKABUtagRECT@@PAVCWnd@@I@Z",
        create_static,
    ),
    (
        934,
        "?Create@CWnd@@UAAHPBG0KABUtagRECT@@PAV1@IPAUCCreateContext@@@Z",
        create_rect,
    ),
    (
        930,
        "?Create@CTabCtrl@@QAAHKABUtagRECT@@PAVCWnd@@I@Z",
        create_tab,
    ),
    (
        919,
        "?Create@CProgressCtrl@@QAAHKABUtagRECT@@PAVCWnd@@I@Z",
        create_progress,
    ),
    (
        924,
        "?Create@CSliderCtrl@@QAAHKABUtagRECT@@PAVCWnd@@I@Z",
        create_slider,
    ),
    (
        944,
        "?CreateEx@CWnd@@QAAHKPBG0KHHHHPAUHWND__@@PAUHMENU__@@PAX@Z",
        create,
    ),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", INITIAL, initial_proc);
    d.register_handler("coredll.dll", HOOK, hook);
    d.register_handler("coredll.dll", UNHOOK, unhook);
    d.register_handler("coredll.dll", CREATE_EX, create);
}
fn pending_key(ctx: &CallCtx<'_>) -> String {
    format!("{PENDING}{}", ctx.kernel.current_thread)
}

fn hook(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    ctx.cpu
        .check_guest_access(object, 72, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let state = super::application::current_thread_state(ctx)?;
    put(ctx, state + 0x14, object)?;
    let owner = super::application::owner(ctx)?;
    let key = pending_key(ctx);
    ctx.kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .insert(key, object);
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn unhook(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let state = super::application::current_thread_state(ctx)?;
    let object = word(ctx, state + 0x14)?;
    if object != 0 {
        consume_hook(ctx, object)?;
    }
    Ok(DispatchOutcome::ReturnedR0(u32::from(object == 0)))
}
pub(super) fn consume_hook(ctx: &mut CallCtx<'_>, object: u32) -> Result<(), KernelError> {
    let owner = super::application::owner(ctx)?;
    let thread = format!("__PocketHLE_MFC_Thread:{}", ctx.kernel.current_thread);
    if let Some(&state) = ctx.kernel.dynamic_exports[&owner].get(&thread) {
        if word(ctx, state + 0x14)? == object {
            put(ctx, state + 0x14, 0)?;
        }
    }
    let key = pending_key(ctx);
    if ctx.kernel.dynamic_exports[&owner].get(&key) == Some(&object) {
        ctx.kernel
            .dynamic_exports
            .get_mut(&owner)
            .unwrap()
            .remove(&key);
    }
    Ok(())
}

// Called only at the shared WinCE creation boundary, before the first callback.
// Ordinary WinCE calls have no pending MFC object and keep their existing path.
pub(super) fn initial_callback(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
) -> Result<Option<u32>, KernelError> {
    let key = pending_key(ctx);
    let pending = ctx
        .kernel
        .dynamic_exports
        .iter()
        .find_map(|(&owner, exports)| exports.get(&key).copied().map(|object| (owner, object)));
    let Some((owner, object)) = pending else {
        return Ok(None);
    };
    let route = ctx.kernel.dynamic_exports[&owner][ROUTE];
    let initial = ctx.kernel.dynamic_exports[&owner][INITIAL];
    if !super::window_handles::associate(ctx, object, hwnd)? {
        return Err(KernelError::Dispatch(
            "MFC CWnd already has a window association".into(),
        ));
    }
    ctx.kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .remove(&key);
    consume_hook(ctx, object)?;
    ctx.kernel.window_procs.insert(hwnd, route);
    Ok(Some(initial))
}

fn initial_proc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let returned = ctx.cpu.read_reg(ArmReg::R0)?;
    if frame.fresh {
        let hwnd = frame.arg(ctx, 0)?;
        let owner = super::application::owner(ctx)?;
        let object = *ctx.kernel.dynamic_exports[&owner]
            .get(&format!("__PocketHLE_MFC_Window:{hwnd:08x}"))
            .ok_or_else(|| KernelError::Dispatch("MFC creation association missing".into()))?;
        let target = virtual_function(ctx, object, 21)?;
        return frame.call_args(ctx, 1, target, &[object]);
    }
    match frame.phase(ctx)? {
        1 => {
            let args = [
                frame.arg(ctx, 0)?,
                frame.arg(ctx, 1)?,
                frame.arg(ctx, 2)?,
                frame.arg(ctx, 3)?,
            ];
            let target = service(ctx, ROUTE)?;
            frame.call_args(ctx, 2, target, &args)
        }
        2 => frame.finish(ctx, returned),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC initial window callback".into(),
        )),
    }
}

fn failure(ctx: &mut CallCtx<'_>, frame: Frame) -> Result<DispatchOutcome, KernelError> {
    let object = frame.arg(ctx, 0)?;
    let owner = super::application::owner(ctx)?;
    let key = pending_key(ctx);
    if ctx.kernel.dynamic_exports[&owner].get(&key) == Some(&object) {
        ctx.kernel
            .dynamic_exports
            .get_mut(&owner)
            .unwrap()
            .remove(&key);
    }
    let temporary = frame.get(ctx, 0)?;
    if temporary != 0 {
        frame.disown(ctx)?;
        ctx.kernel.heap.free(temporary);
        frame.set(ctx, 0, 0)?;
    }
    let target = virtual_function(ctx, object, 41)?;
    frame.call_args(ctx, 4, target, &[object])
}

fn create_rect(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    if !frame.fresh {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        return frame.finish(ctx, result);
    }
    let object = frame.arg(ctx, 0)?;
    let class = frame.arg(ctx, 1)?;
    let title = frame.arg(ctx, 2)?;
    let style = frame.arg(ctx, 3)? | 0x40000000;
    let rect = ctx.arg_u32(4)?;
    let parent = ctx.arg_u32(5)?;
    let parent = if parent == 0 {
        0
    } else {
        word(ctx, parent + 0x20)?
    };
    let id = ctx.arg_u32(6)?;
    let context = ctx.arg_u32(7)?;
    let x = word(ctx, rect)?;
    let y = word(ctx, rect + 4)?;
    let width = word(ctx, rect + 8)?.wrapping_sub(x);
    let height = word(ctx, rect + 12)?.wrapping_sub(y);
    let target = service(ctx, CREATE_EX)?;
    frame.call_args(
        ctx,
        1,
        target,
        &[
            object, 0, class, title, style, x, y, width, height, parent, id, context,
        ],
    )
}
fn create_button(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    create_control(ctx, "BUTTON", true)
}
fn create_static(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    create_control(ctx, "STATIC", true)
}
fn create_combo(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    create_common_control(ctx, "COMBOBOX")
}
fn create_edit(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    create_common_control(ctx, "EDIT")
}
fn create_list(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    create_common_control(ctx, "LISTBOX")
}
fn create_tab(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    create_common_control(ctx, "SysTabControl32")
}
fn create_progress(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    create_common_control(ctx, "msctls_progress32")
}
fn create_slider(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    create_common_control(ctx, "msctls_trackbar32")
}
fn create_common_control(
    ctx: &mut CallCtx<'_>,
    class_name: &str,
) -> Result<DispatchOutcome, KernelError> {
    create_control(ctx, class_name, false)
}
fn create_control(
    ctx: &mut CallCtx<'_>,
    class_name: &str,
    titled: bool,
) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    if !frame.fresh {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        return frame.finish(ctx, result);
    }
    let object = frame.arg(ctx, 0)?;
    let (title, style, rect, parent, id) = if titled {
        (
            frame.arg(ctx, 1)?,
            frame.arg(ctx, 2)?,
            frame.arg(ctx, 3)?,
            ctx.arg_u32(4)?,
            ctx.arg_u32(5)?,
        )
    } else {
        (
            0,
            frame.arg(ctx, 1)?,
            frame.arg(ctx, 2)?,
            frame.arg(ctx, 3)?,
            ctx.arg_u32(4)?,
        )
    };
    // The common WinCE control owner already registers this built-in class.
    // No second common-control registry or renderer is needed.
    let class = frame.address + 128;
    let name: Vec<_> = class_name
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    ctx.cpu.write_mem(class, &name)?;
    let target = virtual_function(ctx, object, 22)?;
    frame.call_args(
        ctx,
        1,
        target,
        &[object, class, title, style, rect, parent, id, 0],
    )
}

fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let object = frame.arg(ctx, 0)?;
    let cs = frame.address + 128;
    let auxiliary = frame.address + 184;
    let returned = ctx.cpu.read_reg(ArmReg::R0)?;
    if frame.fresh {
        ctx.cpu
            .check_guest_access(object, 72, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
        if word(ctx, object + 0x20)? != 0 {
            return Err(KernelError::Dispatch(
                "MFC CreateEx on an attached CWnd".into(),
            ));
        }
        let module = super::application::state(ctx)?;
        let instance = word(ctx, module + 8)?;
        let mut args = [0; 12];
        for (index, value) in args.iter_mut().enumerate() {
            *value = ctx.arg_u32(index as u8)?;
        }
        // CREATESTRUCTW uses a different order from the C++ integer overload.
        let fields = [
            args[11], instance, args[10], args[9], args[8], args[7], args[6], args[5], args[4],
            args[3], args[2], args[1],
        ];
        for (index, value) in fields.into_iter().enumerate() {
            put(ctx, cs + index as u32 * 4, value)?;
        }
        let target = virtual_function(ctx, object, 24)?;
        return frame.call_args(ctx, 1, target, &[object, cs]);
    }
    match frame.phase(ctx)? {
        1 => {
            if returned == 0 {
                return failure(ctx, frame);
            }
            let menu = word(ctx, cs + 8)?;
            if menu > 0xffff {
                frame.set(ctx, 1, menu)?;
                put(ctx, cs + 8, 0)?;
            }
            let class = word(ctx, cs + 40)?;
            let instance = word(ctx, cs + 4)?;
            let target = service(ctx, "GetClassInfoW")?;
            frame.call_args(ctx, 2, target, &[instance, class, auxiliary])
        }
        2 => {
            if returned == 0 {
                return failure(ctx, frame);
            }
            let class = word(ctx, cs + 40)?;
            // GetClassInfoW resolved names/atoms and retains the source name in
            // WNDCLASS. Numeric atoms need its registry name before prefixing.
            let name = if class <= 0xffff {
                ctx.kernel
                    .gdi
                    .class_values
                    .iter()
                    .find_map(|((name, index), &value)| {
                        (*index == -32 && value == class).then_some(name.clone())
                    })
                    .ok_or_else(|| KernelError::Dispatch("MFC class atom disappeared".into()))?
            } else {
                String::from_utf16_lossy(&crate::coredll::read_wstr(ctx, class, 128)?)
            };
            let mut fields = [0; 10];
            for (index, value) in fields.iter_mut().enumerate() {
                *value = word(ctx, auxiliary + index as u32 * 4)?;
            }
            let route = service(ctx, ROUTE)?;
            let super_proc = if fields[1] == route { 0 } else { fields[1] };
            put(ctx, object + 0x30, super_proc)?;
            if !name.get(..3).is_some_and(|s| s.eq_ignore_ascii_case("Afx")) {
                let text: Vec<u8> = format!("Afx{name}\0")
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect();
                let temporary = alloc(ctx, text.len() as u32)?;
                frame.own(ctx, temporary)?;
                frame.set(ctx, 0, temporary)?;
                ctx.cpu.write_mem(temporary, &text)?;
                fields[1] = route;
                fields[9] = temporary;
                let registered = super::windows::register_fields(ctx, fields, Some(auxiliary))?;
                ctx.kernel
                    .control_class_prototypes
                    .remove(&(ctx.kernel.current_thread, auxiliary));
                if registered == 0 {
                    return failure(ctx, frame);
                }
                put(ctx, cs + 40, temporary)?;
            } else {
                ctx.kernel
                    .control_class_prototypes
                    .remove(&(ctx.kernel.current_thread, auxiliary));
            }
            let style = word(ctx, cs + 32)?;
            if style & 0xc0000000 == 0 {
                put(ctx, cs + 24, 0x80000000)?;
                put(ctx, cs + 28, 0x80000000)?;
                ctx.cpu.write_mem(auxiliary, &[0; 48])?;
                put(ctx, auxiliary, 48)?;
                if let DispatchOutcome::ReturnedR0(1) =
                    crate::shell_keyboard::sip_info(ctx, auxiliary)?
                {
                    let width = word(ctx, auxiliary + 16)?.wrapping_sub(word(ctx, auxiliary + 8)?);
                    let height =
                        word(ctx, auxiliary + 20)?.wrapping_sub(word(ctx, auxiliary + 12)?);
                    let sip_off = word(ctx, auxiliary + 4)? & 1 == 0;
                    put(ctx, cs + 20, width)?;
                    put(
                        ctx,
                        cs + 16,
                        height.saturating_sub(if sip_off { 26 } else { 0 }),
                    )?;
                }
            }
            let args = [
                word(ctx, cs + 44)?,
                word(ctx, cs + 40)?,
                word(ctx, cs + 36)?,
                word(ctx, cs + 32)?,
                word(ctx, cs + 28)?,
                word(ctx, cs + 24)?,
                word(ctx, cs + 20)?,
                word(ctx, cs + 16)?,
                word(ctx, cs + 12)?,
                word(ctx, cs + 8)?,
                word(ctx, cs + 4)?,
                word(ctx, cs)?,
            ];
            let owner = super::application::owner(ctx)?;
            let key = pending_key(ctx);
            if ctx.kernel.dynamic_exports[&owner]
                .get(&key)
                .is_some_and(|pending| *pending != object)
            {
                return Err(KernelError::Dispatch(
                    "MFC pending creation collision".into(),
                ));
            }
            let target = service(ctx, "CreateWindowExW")?;
            let outcome = frame.call_args(ctx, 3, target, &args)?;
            ctx.kernel
                .dynamic_exports
                .get_mut(&owner)
                .unwrap()
                .insert(key, object);
            Ok(outcome)
        }
        3 => {
            if returned == 0 {
                return failure(ctx, frame);
            }
            if word(ctx, object + 0x20)? != returned {
                return Err(KernelError::Dispatch(
                    "MFC created window was not associated".into(),
                ));
            }
            let menu = frame.get(ctx, 1)?;
            if menu != 0 {
                put(ctx, object + 0x24, menu)?;
            }
            let temporary = frame.get(ctx, 0)?;
            if temporary != 0 {
                frame.disown(ctx)?;
                ctx.kernel.heap.free(temporary);
            }
            frame.finish(ctx, 1)
        }
        4 => frame.finish(ctx, 0), // PostNcDestroy may have deleted the object.
        _ => Err(KernelError::Dispatch(
            "Invalid MFC CreateEx continuation".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::window_dispatch::tests::{drive, entry};
    use super::*;
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    fn configure(cpu: &mut dyn Cpu, kernel: &mut pocket_kernel::KernelState) {
        let table = cpu.read_u32_le(0x2000).unwrap();
        let bytes = cpu.read_mem(table, 46 * 4).unwrap();
        cpu.write_mem(0x2400, &bytes).unwrap();
        cpu.write_mem(0x2400 + 21 * 4, &0x6000u32.to_le_bytes())
            .unwrap();
        cpu.write_mem(0x2400 + 38 * 4, &0x6100u32.to_le_bytes())
            .unwrap();
        cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
        let text: Vec<_> = "AfxTest\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x11000, &text).unwrap();
        let route = kernel.dynamic_exports[&1][ROUTE];
        for (index, value) in [0, route, 0, 0, 0x10000000, 0, 0, 0, 0, 0x11000]
            .into_iter()
            .enumerate()
        {
            cpu.write_mem(0x1800 + index as u32 * 4, &value.to_le_bytes())
                .unwrap();
        }
    }
    #[test]
    fn create_ex_binds_before_subclass_and_delivers_real_create_struct_through_shared_owner() {
        for menu in [0u32, 0xdead6001] {
            let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
            cpu.map_region(
                0x10000,
                0x2000,
                pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
            )
            .unwrap();
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:206",
                &[0x2000],
            );
            configure(&mut cpu, &mut k);
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:687",
                &[0x1800],
            );
            for (index, value) in [
                0x2000, 0, 0x11000, 0, 0x90000000, 7, 8, 240, 320, 0, menu, 0xabcdef,
            ]
            .into_iter()
            .enumerate()
            {
                if index < 4 {
                    cpu.write_arg(index as u8, value).unwrap();
                } else {
                    cpu.write_mem(0x4000 + (index as u32 - 4) * 4, &value.to_le_bytes())
                        .unwrap();
                }
            }
            let mut subclassed = false;
            let mut created = 0;
            let result = drive(&mut cpu, &mut k, &mut d, entry(944), |pc, cpu| {
                assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
                assert_ne!(cpu.read_u32_le(0x2020).unwrap(), 0);
                if pc == 0x6000 {
                    assert!(!subclassed);
                    subclassed = true;
                } else {
                    assert_eq!(pc, 0x6100);
                    assert!(subclassed);
                    let message = cpu.read_arg(1).unwrap();
                    if message == 1 {
                        created += 1;
                        let cs = cpu.read_arg(3).unwrap();
                        for (index, value) in [
                            0xabcdef, 0x10000000, 0, 0, 320, 240, 8, 7, 0x90000000, 0, 0x11000, 0,
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            assert_eq!(cpu.read_u32_le(cs + index as u32 * 4).unwrap(), value);
                        }
                    }
                }
                0
            });
            assert_eq!(result, 1);
            assert_eq!(created, 1);
            let hwnd = cpu.read_u32_le(0x2020).unwrap();
            assert_eq!(k.window_rects[&hwnd], (7, 8, 240, 320));
            assert_eq!(
                k.dynamic_exports[&1][&format!("__PocketHLE_MFC_Window:{hwnd:08x}")],
                0x2000
            );
            assert!(!k.dynamic_exports[&1]
                .keys()
                .any(|key| key.starts_with(PENDING) || key.starts_with("__PocketHLE_MFC_Frame:")));
            assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
            assert_eq!(cpu.read_u32_le(0x2024).unwrap(), menu);
        }
    }

    #[test]
    fn rejected_precreation_calls_real_post_destroy_override_and_releases_temporary_frame() {
        let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2000],
        );
        let table = cpu.read_u32_le(0x2000).unwrap();
        let bytes = cpu.read_mem(table, 46 * 4).unwrap();
        cpu.write_mem(0x2400, &bytes).unwrap();
        cpu.write_mem(0x2400 + 24 * 4, &0x6000u32.to_le_bytes())
            .unwrap();
        cpu.write_mem(0x2400 + 41 * 4, &0x6100u32.to_le_bytes())
            .unwrap();
        cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
        for (index, value) in [0x2000, 0, 0, 0, 0x90000000, 0, 0, 240, 320, 0, 0, 0]
            .into_iter()
            .enumerate()
        {
            if index < 4 {
                cpu.write_arg(index as u8, value).unwrap();
            } else {
                cpu.write_mem(0x4000 + (index as u32 - 4) * 4, &value.to_le_bytes())
                    .unwrap();
            }
        }
        let available = k.heap.free_bytes();
        let mut calls = Vec::new();
        let result = drive(&mut cpu, &mut k, &mut d, entry(944), |pc, cpu| {
            calls.push(pc);
            assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
            if pc == 0x6100 {
                // A derived hook may delete the object: no subsequent read of it.
                cpu.write_mem(0x2000, &[0xff; 72]).unwrap();
            }
            0
        });
        assert_eq!(result, 0);
        assert_eq!(calls, [0x6000, 0x6100]);
        assert_eq!(k.heap.free_bytes(), available);
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        assert!(!k.dynamic_exports[&1]
            .keys()
            .any(|key| key.starts_with(PENDING) || key.starts_with("__PocketHLE_MFC_Frame:")));
    }
    #[test]
    fn builtin_class_clone_retains_control_behavior_superclass_and_releases_its_name() {
        let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
        cpu.map_region(
            0x10000,
            0x2000,
            pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
        )
        .unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2000],
        );
        configure(&mut cpu, &mut k);
        let text: Vec<_> = "BUTTON\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x11000, &text).unwrap();
        for (index, value) in [
            0x2000, 0, 0x11000, 0, 0x50000000, 7, 8, 24, 32, 0xdead1000, 7, 0,
        ]
        .into_iter()
        .enumerate()
        {
            if index < 4 {
                cpu.write_arg(index as u8, value).unwrap();
            } else {
                cpu.write_mem(0x4000 + (index as u32 - 4) * 4, &value.to_le_bytes())
                    .unwrap();
            }
        }
        let mut temporary = 0;
        let result = drive(&mut cpu, &mut k, &mut d, entry(944), |pc, cpu| {
            assert!(matches!(pc, 0x6000 | 0x6100));
            if pc == 0x6100 && cpu.read_arg(1).unwrap() == 1 {
                let cs = cpu.read_arg(3).unwrap();
                temporary = cpu.read_u32_le(cs + 40).unwrap();
                assert_ne!(temporary, 0x11000);
            }
            0
        });
        assert_eq!(result, 1);
        let hwnd = cpu.read_u32_le(0x2020).unwrap();
        assert_eq!(
            k.controls.get(hwnd).unwrap().class,
            pocket_kernel::controls::ControlClass::Button
        );
        assert_eq!(k.window_classes[&hwnd], "AfxBUTTON");
        assert_eq!(
            cpu.read_u32_le(0x2030).unwrap(),
            k.dynamic_exports[&1]["DefWindowProcW"]
        );
        assert_ne!(temporary, 0);
        assert_eq!(k.heap.msize(temporary), None); // Released by CreateEx.
        assert!(k.control_class_prototypes.is_empty());
        assert!(!k.dynamic_exports[&1]
            .keys()
            .any(|key| key.starts_with(PENDING) || key.starts_with("__PocketHLE_MFC_Frame:")));
    }
}

#[cfg(test)]
mod progress_tests {
    use super::super::window_dispatch::tests::{drive, entry};
    use super::*;
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn progress_wrapper_uses_virtual_rect_creation_and_the_shared_native_control_owner() {
        for (ordinal, expected_class) in [
            (919, pocket_kernel::controls::ControlClass::Progress),
            (924, pocket_kernel::controls::ControlClass::TrackBar),
            (930, pocket_kernel::controls::ControlClass::Tab),
        ] {
            let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:206",
                &[0x2000],
            );
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:206",
                &[0x2200],
            );
            cpu.write_mem(0x2220, &0xdead1000u32.to_le_bytes()).unwrap();
            for (index, value) in [7u32, 8, 107, 28].into_iter().enumerate() {
                cpu.write_mem(0x2800 + index as u32 * 4, &value.to_le_bytes())
                    .unwrap();
            }
            for (i, value) in [0x2000, 0x50000000, 0x2800, 0x2200].into_iter().enumerate() {
                cpu.write_arg(i as u8, value).unwrap();
            }
            cpu.write_mem(0x4000, &123u32.to_le_bytes()).unwrap();
            assert_eq!(
                drive(&mut cpu, &mut k, &mut d, entry(ordinal), |pc, _| panic!(
                    "Unexpected guest function {pc:x}"
                )),
                1
            );
            let hwnd = cpu.read_u32_le(0x2020).unwrap();
            let control = k.controls.get(hwnd).unwrap();
            assert_eq!(control.class, expected_class);
            assert_eq!(control.parent, 0xdead1000);
            assert_eq!(k.window_rects[&hwnd], (7, 8, 100, 20));
            assert_eq!(k.window_ids[&hwnd], 123);
            if ordinal == 924 {
                for (i, value) in [0x2000, (-250000i32) as u32, 450000, 1]
                    .into_iter()
                    .enumerate()
                {
                    cpu.write_arg(i as u8, value).unwrap();
                }
                drive(&mut cpu, &mut k, &mut d, entry(2397), |pc, _| {
                    panic!("unexpected guest {pc:x}")
                });
                assert_eq!(k.controls.get(hwnd).unwrap().track_range, (-250000, 450000));
            }
            let image_list = if ordinal == 930 {
                let label: Vec<_> = "General"
                    .encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect();
                cpu.write_mem(0x2a00, &label).unwrap();
                for (i, value) in [0x2000u32, 0x1b, 0, 0x2a00, u32::MAX, 0xdead1234, 2, 0xf]
                    .into_iter()
                    .enumerate()
                {
                    if i < 4 {
                        cpu.write_arg(i as u8, value).unwrap();
                    } else {
                        cpu.write_mem(0x4000 + (i as u32 - 4) * 4, &value.to_le_bytes())
                            .unwrap();
                    }
                }
                assert_eq!(
                    drive(&mut cpu, &mut k, &mut d, entry(1674), |pc, _| panic!(
                        "unexpected insertion {pc:x}"
                    )),
                    0
                );
                let item = &k.controls.get(hwnd).unwrap().tab.items[0];
                assert_eq!(item.text, "General");
                assert_eq!(item.data, 0xdead1234);
                assert_eq!(item.image, -1);
                assert_eq!(item.state, 2);
                let image_list = k.gdi.create_image_list(8, 8);
                assert_ne!(image_list, 0);
                k.controls.get_mut(hwnd).unwrap().tab.image_list = image_list;
                Some(image_list)
            } else {
                None
            };
            cpu.write_arg(0, 0x2000).unwrap();
            drive(
                &mut cpu,
                &mut k,
                &mut d,
                entry(match ordinal {
                    919 => 276,
                    924 => 288,
                    930 => 301,
                    _ => unreachable!(),
                }),
                |pc, _| panic!("unexpected destructor callback {pc:x}"),
            );
            assert!(k.controls.get(hwnd).is_none());
            if let Some(image_list) = image_list {
                assert!(
                    k.gdi.image_lists.contains_key(&image_list),
                    "tab destroyed its borrowed image list"
                );
            }
            assert!(!k.window_procs.contains_key(&hwnd));
            assert_eq!(cpu.read_u32_le(0x2020).unwrap(), 0);
            assert!(
                !k.dynamic_exports[&1].contains_key(&format!("__PocketHLE_MFC_Window:{hwnd:08x}"))
            );
            assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
            assert!(!k.dynamic_exports[&1]
                .keys()
                .any(|key| key.starts_with(PENDING) || key.starts_with("__PocketHLE_MFC_Frame:")));
        }
    }
}
