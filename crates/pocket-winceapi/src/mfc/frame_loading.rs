//! CE frame loading delegates resources and window callbacks to their shared owners.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::regs::ArmReg;
use pocket_pe::ResourceKey;
pub(super) const GET_MENU: &str = "__PocketHLE_MFC_FrameGetMenu";
pub(super) const SET_MENU: &str = "__PocketHLE_MFC_FrameSetMenu";
const CREATE_VIEW: &str = "__PocketHLE_MFC_FrameCreateView";
const CREATE: &str = "__PocketHLE_MFC_FrameCreate";
const ICON_CLASS: &str = "__PocketHLE_MFC_FrameIconClass";
const ACCELERATORS: &str = "__PocketHLE_MFC_FrameAccelerators";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        987,
        "?CreateView@CFrameWnd@@QAAPAVCWnd@@PAUCCreateContext@@I@Z",
        create_view,
    ),
    (
        1722,
        "?LoadFrame@CFrameWnd@@UAAHIKPAVCWnd@@PAUCCreateContext@@@Z",
        load,
    ),
    (
        910,
        "?Create@CFrameWnd@@QAAHPBG0KABUtagRECT@@PAVCWnd@@0KPAUCCreateContext@@@Z",
        create,
    ),
    (1316, "?GetIconWndClass@CFrameWnd@@IAAPBGKI@Z", icon_class),
    (
        2093,
        "?PreCreateWindow@CFrameWnd@@MAAHAAUtagCREATESTRUCTW@@@Z",
        pre_create,
    ),
    (1719, "?LoadAccelTable@CFrameWnd@@QAAHPBG@Z", accelerators),
    (622, "?AfxEndDeferRegisterClass@@YAHJ@Z", register_classes),
    (
        1850,
        "?OnCreate@CFrameWnd@@IAAHPAUtagCREATESTRUCTW@@@Z",
        on_create,
    ),
    (
        1855,
        "?OnCreateHelper@CFrameWnd@@IAAHPAUtagCREATESTRUCTW@@PAUCCreateContext@@@Z",
        on_create_helper,
    ),
    (
        1854,
        "?OnCreateClient@CFrameWnd@@MAAHPAUtagCREATESTRUCTW@@PAUCCreateContext@@@Z",
        create_client,
    ),
    (2526, "?wce_GetMenu@@YAPAUHMENU__@@PAUHWND__@@@Z", get_menu),
    (2568, "?wce_SetMenu@@YAHPAUHWND__@@PAUHMENU__@@@Z", set_menu),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", GET_MENU, get_menu);
    d.register_handler("coredll.dll", SET_MENU, set_menu);
    d.register_handler(
        "coredll.dll",
        "__PocketHLE_MFC_FrameOnCreateHelper",
        on_create_helper,
    );
    d.register_handler(
        "coredll.dll",
        "__PocketHLE_MFC_WindowDefault",
        super::window_dispatch::last_message_default,
    );
    d.register_handler("coredll.dll", CREATE, create);
    d.register_handler("coredll.dll", CREATE_VIEW, create_view);
    d.register_handler("coredll.dll", ICON_CLASS, icon_class);
    d.register_handler("coredll.dll", ACCELERATORS, accelerators);
}
fn get_menu(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    if hwnd == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let object = super::windows::wrapper(ctx, hwnd)?;
    Ok(DispatchOutcome::ReturnedR0(word(ctx, object + 0x24)?))
}
fn set_menu(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    let menu = ctx.arg_u32(1)?;
    if hwnd == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let object = super::windows::wrapper(ctx, hwnd)?;
    // The CE shim stores the logical menu; it does not create a command bar.
    put(ctx, object + 0x24, menu)?;
    Ok(DispatchOutcome::ReturnedR0(1))
}

pub(super) fn rect_default(ctx: &mut CallCtx<'_>) -> Result<u32, KernelError> {
    let owner = super::application::owner(ctx)?;
    let key = "__PocketHLE_MFC_FrameRectDefault";
    if let Some(&pointer) = ctx.kernel.dynamic_exports[&owner].get(key) {
        return Ok(pointer);
    }
    let pointer = alloc(ctx, 16)?;
    // Values after the native static initializer, not the zeroed PE data.
    for (i, value) in [0u32, 0, 0x80000000, 0x80000000].into_iter().enumerate() {
        put(ctx, pointer + i as u32 * 4, value)?;
    }
    ctx.kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .insert(key.into(), pointer);
    Ok(pointer)
}

pub(super) fn standard_class(ctx: &mut CallCtx<'_>, frame_class: bool) -> Result<u32, KernelError> {
    let (name, bit, brush) = if frame_class {
        ("AfxFrameOrView42u", 8, 0x40000006)
    } else {
        ("AfxWnd42u", 1, 0)
    };
    standard_class_spec(ctx, name, bit, 11, brush)
}
pub(super) fn standard_class_spec(
    ctx: &mut CallCtx<'_>,
    name: &str,
    bit: u32,
    style: u32,
    brush: u32,
) -> Result<u32, KernelError> {
    let state = super::application::state(ctx)?;
    let owner = super::application::owner(ctx)?;
    let key = format!("__PocketHLE_MFC_StandardClass:{name}");
    if let Some(&pointer) = ctx.kernel.dynamic_exports[&owner].get(&key) {
        return Ok(pointer);
    }
    let bytes: Vec<_> = name
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    let pointer = alloc(ctx, bytes.len() as u32)?;
    let result = (|| {
        ctx.cpu.write_mem(pointer, &bytes)?;
        let instance = word(ctx, state + 8)?;
        let procedure = word(ctx, state + 0x203c)?;
        // Cursor/icon fields are optional. Do not substitute the old virtual
        // system-cursor sentinel for an unavailable host cursor resource.
        if super::windows::register_fields(
            ctx,
            [style, procedure, 0, 0, instance, 0, 0, brush, 0, pointer],
            None,
        )? == 0
        {
            return Err(KernelError::Dispatch(
                "Could not register the standard MFC class".into(),
            ));
        }
        let flags = word(ctx, state + 0x18)?;
        put(ctx, state + 0x18, flags | bit)
    })();
    if let Err(error) = result {
        ctx.kernel.heap.free(pointer);
        return Err(error);
    }
    ctx.kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .insert(key, pointer);
    Ok(pointer)
}
fn register_classes(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let flags = ctx.arg_u32(0)?;
    if flags & !13 != 0 {
        return Err(KernelError::Dispatch(format!(
            "MFC deferred class flags 0x{flags:x} are not implemented"
        )));
    }
    if flags & 1 != 0 {
        standard_class(ctx, false)?;
    }
    if flags & 8 != 0 {
        standard_class(ctx, true)?;
    }
    if flags & 4 != 0 {
        standard_class_spec(ctx, "AfxMDIFrame42u", 4, 8, 0)?;
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn pre_create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let cs = ctx.arg_u32(1)?;
    ctx.cpu
        .check_guest_access(object, 232, pocket_cpu::Prot::READ)?;
    ctx.cpu
        .check_guest_access(cs, 48, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    if word(ctx, cs + 40)? == 0 {
        let name = standard_class(ctx, true)?;
        put(ctx, cs + 40, name)?;
    }
    let style = word(ctx, cs + 32)?;
    // The CE3 AUX_DATA initializer sets bWin4 to TRUE.
    if style & 0x8000 != 0 {
        put(ctx, cs + 32, style | 0x4000)?;
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn icon_class(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let object = frame.arg(ctx, 0)?;
    let cs = frame.address + 128;
    let wc = frame.address + 184;
    let returned = ctx.cpu.read_reg(ArmReg::R0)?;
    if frame.fresh {
        let id = frame.arg(ctx, 2)? & 0xffff;
        let instance =
            super::resources::module_for(ctx, &ResourceKey::Id(id), &ResourceKey::Id(14))?;
        frame.set(ctx, 0, instance)?;
        let owner = super::application::owner(ctx)?;
        let key = format!("__PocketHLE_MFC_ClassIcon:{instance:08x}:{id:04x}");
        if let Some(icon) = ctx.kernel.dynamic_exports[&owner]
            .get(&key)
            .copied()
            .filter(|icon| ctx.kernel.gdi.icons.contains_key(icon))
        {
            frame.set(ctx, 1, icon)?;
            return pre_create_icon(ctx, &frame, object, cs);
        }
        return frame.call_service(ctx, 1, "LoadIconW", &[instance, id]);
    }
    match frame.phase(ctx)? {
        1 => {
            if returned == 0 {
                return frame.finish(ctx, 0);
            }
            frame.set(ctx, 1, returned)?;
            let instance = frame.get(ctx, 0)?;
            let id = frame.arg(ctx, 2)? & 0xffff;
            let owner = super::application::owner(ctx)?;
            ctx.kernel.dynamic_exports.get_mut(&owner).unwrap().insert(
                format!("__PocketHLE_MFC_ClassIcon:{instance:08x}:{id:04x}"),
                returned,
            );
            pre_create_icon(ctx, &frame, object, cs)
        }
        2 => {
            let name = word(ctx, cs + 40)?;
            if name == 0 {
                return frame.finish(ctx, 0);
            }
            let instance = super::application::state(ctx)?;
            let instance = word(ctx, instance + 8)?;
            frame.call_service(ctx, 3, "GetClassInfoW", &[instance, name, wc])
        }
        3 => {
            if returned == 0 {
                return frame.finish(ctx, 0);
            }
            ctx.kernel
                .control_class_prototypes
                .remove(&(ctx.kernel.current_thread, wc));
            let icon = frame.get(ctx, 1)?;
            if word(ctx, wc + 20)? == icon {
                return frame.finish(ctx, 0);
            }
            let style = word(ctx, wc)?;
            let cursor = word(ctx, wc + 24)?;
            let brush = word(ctx, wc + 28)?;
            let name = super::windows::generated_class(ctx, style, cursor, brush, icon)?;
            frame.finish(ctx, name)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC icon-class continuation".into(),
        )),
    }
}
fn pre_create_icon(
    ctx: &mut CallCtx<'_>,
    frame: &Frame,
    object: u32,
    cs: u32,
) -> Result<DispatchOutcome, KernelError> {
    ctx.cpu.write_mem(cs, &[0; 48])?;
    let style = frame.arg(ctx, 1)?;
    put(ctx, cs + 32, style)?;
    let target = virtual_function(ctx, object, 24)?;
    frame.call_args(ctx, 2, target, &[object, cs])
}

fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let object = frame.arg(ctx, 0)?;
    let returned = ctx.cpu.read_reg(ArmReg::R0)?;
    if frame.fresh {
        let mut a = [0; 9];
        for (i, value) in a.iter_mut().enumerate() {
            *value = ctx.arg_u32(i as u8)?;
        }
        put(ctx, object + 0x50, a[6])?;
        super::strings::assign_pointer(ctx, object + 0xc0, a[2])?;
        frame.set(ctx, 0, a[6])?;
        let parent = if a[5] == 0 {
            0
        } else {
            word(ctx, a[5] + 0x20)?
        };
        let rect = a[4];
        let x = word(ctx, rect)?;
        let y = word(ctx, rect + 4)?;
        let width = word(ctx, rect + 8)?.wrapping_sub(x);
        let height = word(ctx, rect + 12)?.wrapping_sub(y);
        return frame.call_service(
            ctx,
            1,
            super::window_creation::CREATE_EX,
            &[
                object, a[7], a[1], a[2], a[3], x, y, width, height, parent, a[6], a[8],
            ],
        );
    }
    match frame.phase(ctx)? {
        1 => {
            if returned == 0 {
                return frame.finish(ctx, 0);
            }
            let name = frame.get(ctx, 0)?;
            if name == 0 {
                return frame.finish(ctx, 1);
            }
            let key = crate::coredll::read_wide_resource_key(ctx, name)?;
            let instance = super::resources::module_for(ctx, &key, &ResourceKey::Id(3))?;
            frame.call_service(ctx, 2, "LoadImageW", &[instance, name, 1, 16, 16, 0])
        }
        2 => {
            if returned == 0 {
                return frame.finish(ctx, 1);
            }
            let hwnd = word(ctx, object + 0x20)?;
            frame.call_service(ctx, 3, "SendMessageW", &[hwnd, 0x80, 0, returned])
        }
        3 => frame.finish(ctx, 1),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC frame-create continuation".into(),
        )),
    }
}
fn accelerators(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let object = frame.arg(ctx, 0)?;
    if !frame.fresh {
        let handle = ctx.cpu.read_reg(ArmReg::R0)?;
        put(ctx, object + 0x60, handle)?;
        return frame.finish(ctx, u32::from(handle != 0));
    }
    let name = frame.arg(ctx, 1)?;
    let key = crate::coredll::read_wide_resource_key(ctx, name)?;
    let instance = super::resources::module_for(ctx, &key, &ResourceKey::Id(9))?;
    frame.call_service(ctx, 1, "LoadAcceleratorsW", &[instance, name])
}
fn load(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let object = frame.arg(ctx, 0)?;
    let returned = ctx.cpu.read_reg(ArmReg::R0)?;
    if frame.fresh {
        let context = ctx.arg_u32(4)?;
        frame.set(ctx, 0, context)?;
        let id = frame.arg(ctx, 1)?;
        put(ctx, object + 0xa0, id)?;
        let text = super::resources::string(ctx, id)?;
        if !text.is_empty() {
            let end = text.iter().position(|&c| c == 10).unwrap_or(text.len());
            super::strings::replace(ctx, object + 0xc0, &text[..end])?;
        }
        standard_class(ctx, true)?;
        let style = frame.arg(ctx, 2)?;
        return frame.call_service(ctx, 1, ICON_CLASS, &[object, style, id]);
    }
    match frame.phase(ctx)? {
        1 => {
            let title = word(ctx, object + 0xc0)?;
            let rect = rect_default(ctx)?;
            let parent = frame.arg(ctx, 3)?;
            let id = frame.arg(ctx, 1)? & 0xffff;
            let context = frame.get(ctx, 0)?;
            frame.call_service(
                ctx,
                2,
                CREATE,
                &[
                    object, returned, title, 0x10000000, rect, parent, id, 0, context,
                ],
            )
        }
        2 => {
            if returned == 0 {
                return frame.finish(ctx, 0);
            }
            let menu = word(ctx, object + 0x24)?;
            put(ctx, object + 0x5c, menu)?;
            let id = frame.arg(ctx, 1)? & 0xffff;
            frame.call_service(ctx, 3, ACCELERATORS, &[object, id])
        }
        3 => {
            if frame.get(ctx, 0)? == 0 {
                let hwnd = word(ctx, object + 0x20)?;
                return frame.call_service(
                    ctx,
                    4,
                    super::descendants::TARGET,
                    &[hwnd, 0x364, 0, 0, 1, 1],
                );
            }
            frame.finish(ctx, 1)
        }
        4 => frame.finish(ctx, 1),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC LoadFrame continuation".into(),
        )),
    }
}

fn on_create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    if !frame.fresh {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        return frame.finish(ctx, result);
    }
    let object = frame.arg(ctx, 0)?;
    let cs = frame.arg(ctx, 1)?;
    let context = word(ctx, cs)?;
    frame.call_service(
        ctx,
        1,
        "__PocketHLE_MFC_FrameOnCreateHelper",
        &[object, cs, context],
    )
}
fn on_create_helper(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    let object = frame.arg(ctx, 0)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if frame.fresh {
        return frame.call_service(ctx, 1, "__PocketHLE_MFC_WindowDefault", &[object]);
    }
    match frame.phase(ctx)? {
        1 if result == u32::MAX => frame.finish(ctx, u32::MAX),
        1 => {
            let cs = frame.arg(ctx, 1)?;
            let context = frame.arg(ctx, 2)?;
            let target = virtual_function(ctx, object, 54)?;
            frame.call_args(ctx, 2, target, &[object, cs, context])
        }
        2 if result == 0 => frame.finish(ctx, u32::MAX),
        2 => {
            let hwnd = word(ctx, object + 0x20)?;
            frame.call_service(ctx, 3, "SendMessageW", &[hwnd, 0x362, 0xe001, 0])
        }
        3 => {
            let target = virtual_function(ctx, object, 50)?;
            frame.call_args(ctx, 4, target, &[object, 1])
        }
        4 => frame.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC frame OnCreate continuation".into(),
        )),
    }
}
fn create_client(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        let result = u32::from(ctx.cpu.read_reg(ArmReg::R0)? != 0);
        return f.finish(ctx, result);
    }
    let context = f.arg(ctx, 2)?;
    if context == 0 || word(ctx, context)? == 0 {
        return f.finish(ctx, 1);
    }
    let document = word(ctx, context + 4)?;
    if document != 0 && word(ctx, document + 0x54)? != 0 {
        return f.finish(ctx, 1);
    }
    let object = f.arg(ctx, 0)?;
    f.call_service(ctx, 1, CREATE_VIEW, &[object, context, 0xe900])
}
fn create_view(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let context = f.arg(ctx, 1)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        ctx.cpu
            .check_guest_access(context, 20, pocket_cpu::Prot::READ)?;
        let class = word(ctx, context)?;
        return f.call_service(ctx, 1, super::class_queries::CREATE, &[class]);
    }
    match f.phase(ctx)? {
        1 if result == 0 => f.finish(ctx, 0),
        1 => {
            f.set(ctx, 0, result)?;
            let hwnd = word(ctx, object + 0x20)?;
            f.call_service(ctx, 2, "GetClientRect", &[hwnd, f.address + 128])
        }
        2 => {
            // Create uses the factory's actual virtual CWnd::Create override,
            // including CREATESTRUCT callbacks and document/context attachment.
            let view = f.get(ctx, 0)?;
            let target = virtual_function(ctx, view, 22)?;
            let id = f.arg(ctx, 2)?;
            f.call_args(
                ctx,
                3,
                target,
                &[view, 0, 0, 0x50800000, f.address + 128, object, id, context],
            )
        }
        3 => {
            let view = if result == 0 { 0 } else { f.get(ctx, 0)? };
            f.finish(ctx, view)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC view creation continuation".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};

    #[test]
    fn load_frame_keeps_its_resource_title_alive_through_real_window_creation() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let text: Vec<_> = "Frame title\nDocument".encode_utf16().collect();
        let mut bytes = vec![0u8; 4];
        bytes.extend_from_slice(&(text.len() as u16).to_le_bytes());
        bytes.extend(text.into_iter().flat_map(u16::to_le_bytes));
        cpu.write_mem(0x1000, &bytes).unwrap();
        k.resources.push(pocket_pe::ResourceEntry {
            ty: pocket_pe::ResourceKey::Id(6),
            name: pocket_pe::ResourceKey::Id(9),
            data_rva: 0x1000,
            size: bytes.len() as u32,
            native_leaves: vec![],
            codepage: 0,
        });
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:103",
            &[0x2000],
        );
        for (i, value) in [0x2000, 130, 0x00cf8000, 0, 0].into_iter().enumerate() {
            if i < 4 {
                cpu.write_arg(i as u8, value).unwrap();
            } else {
                cpu.write_mem(0x4000, &value.to_le_bytes()).unwrap();
            }
        }
        let created = drive(&mut cpu, &mut k, &mut d, entry(1722), |pc, _| {
            panic!("Unexpected guest callback {pc:x}")
        });
        assert_eq!(created, 1);
        let hwnd = cpu.read_u32_le(0x2020).unwrap();
        assert_ne!(hwnd, 0);
        assert_eq!(k.window_titles[&hwnd], "Frame title");
        let title = cpu.read_u32_le(0x20c0).unwrap();
        let expected: Vec<_> = "Frame title\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(
            cpu.read_mem(title, expected.len() as u32).unwrap(),
            expected
        );
        assert_eq!(cpu.read_u32_le(0x20a0).unwrap(), 130);
        assert_eq!(cpu.read_u32_le(0x2060).unwrap(), 0);
        assert_eq!(cpu.read_u32_le(0x205c).unwrap(), 0);
        assert!(!k.dynamic_exports[&1]
            .keys()
            .any(|key| key.starts_with("__PocketHLE_MFC_Frame:")
                || key.starts_with("__PocketHLE_MFC_Create:")
                || key.starts_with("__PocketHLE_MFC_DescendantsDepth:")));
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
    }

    #[test]
    fn on_create_preserves_context_and_stops_at_each_rejected_virtual() {
        for rejected in [0, 1, 2] {
            let (mut cpu, mut k, mut d) = framework_setup();
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:103",
                &[0x2000],
            );
            let table = cpu.read_u32_le(0x2000).unwrap();
            let bytes = cpu.read_mem(table, 55 * 4).unwrap();
            cpu.write_mem(0x2400, &bytes).unwrap();
            for (slot, function) in [(40, 0x6000u32), (54, 0x6100), (50, 0x6200)] {
                cpu.write_mem(0x2400 + slot * 4, &function.to_le_bytes())
                    .unwrap();
            }
            cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
            cpu.write_mem(0x2020, &0xdead1000u32.to_le_bytes()).unwrap();
            cpu.write_mem(0x2800, &0xabcdefu32.to_le_bytes()).unwrap();
            k.window_classes.insert(0xdead1000, "FrameTest".into());
            k.window_procs.insert(0xdead1000, 0x6300);
            cpu.write_arg(0, 0x2000).unwrap();
            cpu.write_arg(1, 0x2800).unwrap();
            let mut calls = Vec::new();
            // Initialize thread state before measuring transient callback ownership.
            call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:651", &[]);
            cpu.write_arg(0, 0x2000).unwrap();
            cpu.write_arg(1, 0x2800).unwrap();
            let available = k.heap.free_bytes();
            let value = drive(&mut cpu, &mut k, &mut d, entry(1850), |pc, cpu| {
                calls.push(pc);
                match pc {
                    0x6000 => {
                        if rejected == 1 {
                            u32::MAX
                        } else {
                            0
                        }
                    }
                    0x6100 => {
                        assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
                        assert_eq!(cpu.read_arg(1).unwrap(), 0x2800);
                        assert_eq!(cpu.read_arg(2).unwrap(), 0xabcdef);
                        u32::from(rejected != 2)
                    }
                    0x6300 => {
                        assert_eq!(cpu.read_arg(0).unwrap(), 0xdead1000);
                        assert_eq!(cpu.read_arg(1).unwrap(), 0x362);
                        assert_eq!(cpu.read_arg(2).unwrap(), 0xe001);
                        0
                    }
                    0x6200 => {
                        assert_eq!(cpu.read_arg(0).unwrap(), 0x2000);
                        assert_eq!(cpu.read_arg(1).unwrap(), 1);
                        0
                    }
                    _ => panic!("Unexpected callback {pc:x}"),
                }
            });
            assert_eq!(value, if rejected == 0 { 0 } else { u32::MAX });
            assert_eq!(
                calls,
                match rejected {
                    1 => vec![0x6000],
                    2 => vec![0x6000, 0x6100],
                    _ => vec![0x6000, 0x6100, 0x6300, 0x6200],
                }
            );
            assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
            assert_eq!(k.heap.free_bytes(), available);
            assert!(!k.dynamic_exports[&1]
                .keys()
                .any(|key| key.starts_with("__PocketHLE_MFC_Frame:")));
        }
    }

    #[test]
    fn base_client_returns_true_only_when_no_view_creation_is_needed() {
        let (mut cpu, mut k, mut d) = framework_setup();
        for context in [0, 0x2800] {
            assert_eq!(
                result(call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "mfcce300.dll",
                    "ord:1854",
                    &[0x2000, 0, context]
                )),
                1
            );
        }
    }
}

#[cfg(test)]
mod view_tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    #[test]
    fn view_creation_uses_guest_factory_and_virtual_create_with_the_actual_context() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2200],
        );
        let table = cpu.read_u32_le(0x2200).unwrap();
        let bytes = cpu.read_mem(table, 46 * 4).unwrap();
        cpu.write_mem(0x2800, &bytes).unwrap();
        cpu.write_mem(0x2800 + 22 * 4, &0x6200u32.to_le_bytes())
            .unwrap();
        cpu.write_mem(0x2200, &0x2800u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x240c, &0x6100u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2600, &0x2400u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2020, &0xdead1000u32.to_le_bytes()).unwrap();
        k.window_classes.insert(0xdead1000, "Frame".into());
        k.window_rects.insert(0xdead1000, (9, 10, 160, 120));
        call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:2649", &[]);
        let free = k.heap.free_bytes();
        let mut calls = 0;
        for succeeds in [true, false] {
            cpu.write_arg(0, 0x2000).unwrap();
            cpu.write_arg(1, 0x2600).unwrap();
            cpu.write_arg(2, 0xe900).unwrap();
            let value = drive(&mut cpu, &mut k, &mut d, entry(987), |pc, cpu| {
                calls += 1;
                match pc {
                    0x6100 => 0x2200,
                    0x6200 => {
                        assert_eq!(cpu.read_arg(0).unwrap(), 0x2200);
                        assert_eq!(cpu.read_arg(1).unwrap(), 0);
                        assert_eq!(cpu.read_arg(2).unwrap(), 0);
                        assert_eq!(cpu.read_arg(3).unwrap(), 0x50800000);
                        let sp = cpu.read_reg(ArmReg::Sp).unwrap();
                        let rect = cpu.read_u32_le(sp).unwrap();
                        assert_eq!(cpu.read_u32_le(rect + 8).unwrap(), 160);
                        assert_eq!(cpu.read_u32_le(rect + 12).unwrap(), 120);
                        assert_eq!(cpu.read_u32_le(sp + 4).unwrap(), 0x2000);
                        assert_eq!(cpu.read_u32_le(sp + 8).unwrap(), 0xe900);
                        assert_eq!(cpu.read_u32_le(sp + 12).unwrap(), 0x2600);
                        u32::from(succeeds)
                    }
                    _ => panic!("unexpected {pc:x}"),
                }
            });
            assert_eq!(value, if succeeds { 0x2200 } else { 0 });
            assert_eq!(k.heap.free_bytes(), free);
        }
        assert_eq!(calls, 4);
        // Abstract runtime classes return NULL without a fabricated instance.
        cpu.write_mem(0x240c, &0u32.to_le_bytes()).unwrap();
        cpu.write_arg(0, 0x2400).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(975), |pc, _| panic!(
                "unexpected {pc:x}"
            )),
            0
        );
        assert_eq!(k.heap.free_bytes(), free);
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:975",
                &[0x2400]
            )),
            0
        );
    }
}
