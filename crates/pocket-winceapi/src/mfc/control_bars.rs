//! CE command bars share shell HWNDs, intrusive frame lists and toolbar services.
//! Continuations remain guest-owned; derived virtual overrides are never bypassed.
use super::callbacks::{service, virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) const DESTROY_BARS: &str = "__PocketHLE_MFC_DestroyDockBars";
const CREATE_BAR: &str = "__PocketHLE_MFC_CreateShellBar";
const DEFAULT: &str = "__PocketHLE_MFC_DefaultCommandBar";
const INSERT_MENU: &str = "__PocketHLE_MFC_InsertMenuBar";
const KIND: &str = "__PocketHLE_MFC_CommandBarKind";
const UNSUBCLASS: &str = "__PocketHLE_MFC_CommandBarUnsubclass";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        1445,
        "?GetRuntimeClass@CCeCommandBar@@UBAPAUCRuntimeClass@@XZ",
        runtime_command,
    ),
    (
        1454,
        "?GetRuntimeClass@CControlBar@@UBAPAUCRuntimeClass@@XZ",
        runtime_control,
    ),
    (
        1532,
        "?GetRuntimeClass@CToolBar@@UBAPAUCRuntimeClass@@XZ",
        runtime_toolbar,
    ),
    (
        2711,
        "?CalcDynamicLayout@CCeCommandBar@@UAA?AVCSize@@HK@Z",
        shell_layout,
    ),
    (
        2712,
        "?CalcFixedLayout@CCeCommandBar@@UAA?AVCSize@@HH@Z",
        shell_layout,
    ),
    (48, "??0CCeCommandBar@@QAA@XZ", new_command),
    (1055, "?DestroyWindow@CControlBar@@UAAHXZ", destroy_window),
    (2084, "?PostNcDestroy@CControlBar@@UAAXXZ", post_nc_destroy),
    (1865, "?OnDestroy@CControlBar@@QAAXXZ", on_destroy),
    (73, "??0CControlBar@@IAA@XZ", new_control),
    (195, "??0CToolBar@@QAA@XZ", new_toolbar),
    (219, "??1CCeCommandBar@@UAA@XZ", delete_command),
    (229, "??1CControlBar@@UAA@XZ", delete_control),
    (304, "??1CToolBar@@UAA@XZ", delete_toolbar),
    (1694, "?IsFloating@CControlBar@@QBAHXZ", is_floating),
    (1692, "?IsDockBar@CControlBar@@UBAHXZ", is_floating),
    (1053, "?DestroyDockBars@CFrameWnd@@QAAXXZ", destroy_bars),
    (
        1278,
        "?GetDockingFrame@CControlBar@@QBAPAVCFrameWnd@@XZ",
        docking_frame,
    ),
    (
        1262,
        "?GetDefaultCommandBar@CCeCommandBar@@CAPAV1@PAVCFrameWnd@@H@Z",
        default_bar,
    ),
    (
        1666,
        "?InsertButtons@CFrameWnd@@QAAHQAU_TBBUTTON@@IHH@Z",
        insert_buttons,
    ),
    (2714, "?Create@CCeCommandBar@@UAAHPAVCWnd@@KI@Z", create),
    (
        2717,
        "?CreateEx@CCeCommandBar@@UAAHPAVCWnd@@KKVCRect@@I@Z",
        create_ex,
    ),
    (
        1676,
        "?InsertMenuBar@CCeCommandBar@@QAAPAVCMenu@@PBGH@Z",
        insert_menu,
    ),
    (
        2195,
        "?RemoveControlBar@CFrameWnd@@QAAXPAVCControlBar@@@Z",
        remove_control,
    ),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", DESTROY_BARS, destroy_bars);
    d.register_handler("coredll.dll", CREATE_BAR, crate::aygshell::menu_bar::create);
    d.register_handler("coredll.dll", DEFAULT, default_bar);
    d.register_handler("coredll.dll", INSERT_MENU, insert_menu);
    d.register_handler("coredll.dll", KIND, kind);
    d.register_handler("coredll.dll", UNSUBCLASS, super::window_handles::unsubclass);
}
fn kind(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    super::handler_named("?IsKindOf@CObject@@QBAHPBUCRuntimeClass@@@Z").unwrap()(ctx)
}
fn construct(ctx: &mut CallCtx<'_>, class: &str) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let bytes = match class {
        "CControlBar" => 124,
        "CToolBar" => 164,
        _ => 224,
    };
    ctx.cpu
        .check_guest_access(o, bytes, Prot::READ | Prot::WRITE)?;
    let table = super::classes::vtable(ctx, class)?;
    super::windows::construct_target_class(ctx, "CWnd", 72)?;
    for (offset, value) in [
        (0x48, 0),
        (0x4c, 6),
        (0x50, 6),
        (0x54, 1),
        (0x58, 1),
        (0x5c, 2),
        (0x60, 0x7fff),
        (0x64, 0),
        (0x68, 0),
        (0x6c, 0),
        (0x70, 0),
        (0x74, 0),
        (0x78, 0),
    ] {
        put(ctx, o + offset, value)?;
    }
    if bytes >= 164 {
        for (offset, value) in [
            (0x7c, 0),
            (0x80, 0),
            (0x84, 0),
            (0x88, 1),
            (0x8c, 0),
            (0x90, 16),
            (0x94, 15),
            (0x98, 23),
            (0x9c, 22),
            (0xa0, 0),
            (0x54, 0),
            (0x58, 2),
        ] {
            put(ctx, o + offset, value)?;
        }
    }
    if bytes == 224 {
        let array = super::classes::vtable(ctx, "CObArray")?;
        for offset in [0xa4, 0xb8] {
            ctx.cpu.write_mem(o + offset, &[0; 20])?;
            put(ctx, o + offset, array)?;
        }
        for offset in [0xcc, 0xd0, 0xd4, 0xdc] {
            put(ctx, o + offset, 0)?;
        }
        put(ctx, o + 0x8c, o)?;
        ctx.cpu.write_mem(o + 0xd8, &0xf000u16.to_le_bytes())?;
    }
    put(ctx, o, table)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn new_control(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct(ctx, "CControlBar")
}
fn new_toolbar(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct(ctx, "CToolBar")
}
pub(super) fn new_command(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct(ctx, "CCeCommandBar")
}
fn remove_control(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = ctx.arg_u32(0)?;
    let bar = ctx.arg_u32(1)?;
    unlink(ctx, frame, bar)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn unlink(ctx: &mut CallCtx<'_>, frame: u32, bar: u32) -> Result<(), KernelError> {
    let at = super::lists::search(ctx, frame + 0x80, bar, 0)?;
    if at != 0 {
        super::lists::remove(ctx, frame + 0x80, at)?;
    }
    Ok(())
}
fn default_bar(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let parent = f.arg(ctx, 0)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        ctx.cpu
            .check_guest_access(parent, 232, Prot::READ | Prot::WRITE)?;
        if word(ctx, parent + 0x84)? != 0 {
            // Validate the list's full traversal once before following its positions.
            super::lists::search(ctx, parent + 0x80, 0, 0)?;
            let node = super::lists::next_position(ctx, parent + 0x80, 0)?;
            f.set(ctx, 0, node)?;
            let bar = word(ctx, node + 8)?;
            let class = super::classes::runtime(ctx, "CCeCommandBar")?;
            return f.call_service(ctx, 1, KIND, &[bar, class]);
        }
        let bar = alloc_cpp(ctx, 224)?;
        f.set(ctx, 1, bar)?;
        ctx.cpu.write_arg(0, bar)?;
        if let Err(e) = construct(ctx, "CCeCommandBar") {
            ctx.kernel.heap.free(bar);
            return Err(e);
        }
        let target = virtual_function(ctx, bar, 55)?;
        return f.call_args(ctx, 2, target, &[bar, parent, 0x50002800, 0xe800]);
    }
    match f.phase(ctx)? {
        1 => {
            let node = f.get(ctx, 0)?;
            if result != 0 {
                let bar = word(ctx, node + 8)?;
                return f.finish(ctx, bar);
            }
            let next = super::lists::next_position(ctx, parent + 0x80, node)?;
            if next == 0 {
                return f.finish(ctx, 0);
            }
            let n = f.get(ctx, 2)?;
            if n >= 4096 {
                return Err(KernelError::Dispatch(
                    "MFC command-bar list exceeded its bound".into(),
                ));
            }
            f.set(ctx, 2, n + 1)?;
            f.set(ctx, 0, next)?;
            let bar = word(ctx, next + 8)?;
            let class = super::classes::runtime(ctx, "CCeCommandBar")?;
            f.call_service(ctx, 1, KIND, &[bar, class])
        }
        2 => {
            let bar = f.get(ctx, 1)?;
            if result == 0 {
                let target = virtual_function(ctx, bar, 1)?;
                return f.call_args(ctx, 4, target, &[bar, 1]);
            }
            if f.arg(ctx, 1)? != 0 {
                let menu = word(ctx, parent + 0x50)?;
                if menu != 0 {
                    return f.call_service(ctx, 3, INSERT_MENU, &[bar, menu, 0xffff]);
                }
            }
            complete_default(ctx, f, parent, bar)
        }
        3 => {
            let bar = f.get(ctx, 1)?;
            complete_default(ctx, f, parent, bar)
        }
        4 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC default command-bar continuation".into(),
        )),
    }
}
fn complete_default(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    parent: u32,
    bar: u32,
) -> Result<DispatchOutcome, KernelError> {
    let hwnd = word(ctx, bar + 0x20)?;
    put(ctx, parent + 0x48, parent)?;
    put(ctx, parent + 0x4c, hwnd)?;
    put(ctx, bar + 0x48, 1)?;
    f.finish(ctx, bar)
}
fn insert_buttons(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        let parent = f.arg(ctx, 0)?;
        let count = f.arg(ctx, 2)?;
        if count > 4096 {
            return f.finish(ctx, 0);
        }
        let bitmap_count = ctx.arg_u32(4)?;
        f.set(ctx, 1, bitmap_count)?;
        return f.call_service(ctx, 1, DEFAULT, &[parent, 1]);
    }
    match f.phase(ctx)? {
        1 => {
            if result == 0 {
                return f.finish(ctx, 0);
            }
            f.set(ctx, 0, result)?;
            let hwnd = word(ctx, result + 0x20)?;
            let module = super::application::state(ctx)?;
            let instance = word(ctx, module + 8)?;
            let bitmap = f.arg(ctx, 3)?;
            let count = f.get(ctx, 1)?;
            let result = crate::coredll::toolbar::add_bitmap(ctx, hwnd, instance, bitmap, count)?;
            if (result as i32) < 0 {
                return f.finish(ctx, 0);
            }
            let buttons = f.arg(ctx, 1)?;
            let count = f.arg(ctx, 2)?;
            f.call_service(ctx, 2, "SendMessageW", &[hwnd, 0x414, count, buttons])
        }
        2 => f.finish(ctx, result),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC InsertButtons continuation".into(),
        )),
    }
}
fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if !f.fresh {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        return f.finish(ctx, result);
    }
    let parent = f.arg(ctx, 1)?;
    let style = f.arg(ctx, 2)?;
    let id = f.arg(ctx, 3)?;
    let target = virtual_function(ctx, o, 56)?;
    f.call_args(ctx, 1, target, &[o, parent, 0, style, 0, 0, 0, 0, id])
}
fn create_ex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let parent = f.arg(ctx, 1)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        if word(ctx, o + 0x20)? != 0 {
            return f.finish(ctx, 0);
        }
        let parent_hwnd = word(ctx, parent + 0x20)?;
        let info = f.address + 128;
        ctx.cpu.write_mem(info, &[0; 32])?;
        put(ctx, info, 32)?;
        put(ctx, info + 4, parent_hwnd)?;
        put(ctx, info + 8, 1)?;
        return f.call_service(ctx, 1, CREATE_BAR, &[info]);
    }
    match f.phase(ctx)? {
        1 => {
            if result == 0 {
                return f.finish(ctx, 0);
            }
            let hwnd = word(ctx, f.address + 156)?;
            f.set(ctx, 0, hwnd)?;
            // CE's framework template seeds command 10 before PreSubclassWindow.
            // Construct the same public button contract without copying SDK resources.
            let text = f.address + 160;
            ctx.cpu.write_mem(text, &[78, 0, 101, 0, 119, 0, 0, 0])?;
            let record = [u32::MAX - 1, 10, 4 | (0x10 << 8), 0, text];
            for (i, value) in record.into_iter().enumerate() {
                put(ctx, f.address + 184 + i as u32 * 4, value)?;
            }
            if crate::coredll::toolbar::message(ctx, hwnd, 0x414, 1, f.address + 184)? != Some(1) {
                return f.call_service(ctx, 5, "DestroyWindow", &[hwnd]);
            }
            f.call_service(ctx, 2, super::window_handles::SUBCLASS, &[o, hwnd])
        }
        2 => {
            if result == 0 {
                let hwnd = f.get(ctx, 0)?;
                return f.call_service(ctx, 5, "DestroyWindow", &[hwnd]);
            }
            let target = virtual_function(ctx, parent, 44)?;
            f.call_args(ctx, 3, target, &[parent])
        }
        3 => {
            if result != 0 {
                super::lists::append(ctx, parent + 0x80, o)?;
                put(ctx, o + 0x78, parent)?;
                let hwnd = f.get(ctx, 0)?;
                put(ctx, parent + 0x4c, hwnd)?;
            }
            if word(ctx, o + 0xdc)? == 0 {
                let target = virtual_function(ctx, o, 40)?;
                return f.call_args(ctx, 4, target, &[o, 0x416, 0, 0]);
            }
            finish_create(ctx, f, o)
        }
        4 => finish_create(ctx, f, o),
        5 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC command-bar creation continuation".into(),
        )),
    }
}
fn finish_create(ctx: &mut CallCtx<'_>, f: Frame, o: u32) -> Result<DispatchOutcome, KernelError> {
    let hwnd = f.get(ctx, 0)?;
    let count = crate::coredll::toolbar::message(ctx, hwnd, 0x418, 0, 0)?.unwrap_or(0);
    put(ctx, o + 0x64, count)?;
    f.finish(ctx, 1)
}
fn insert_menu(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        let name = f.arg(ctx, 1)?;
        let key = crate::coredll::read_wide_resource_key(ctx, name)?;
        let module = super::resources::module_for(ctx, &key, &pocket_pe::ResourceKey::Id(4))?;
        return f.call_service(ctx, 1, "LoadMenuW", &[module, name]);
    }
    match f.phase(ctx)? {
        1 => {
            if result == 0 {
                return f.finish(ctx, 0);
            }
            let menu = result;
            f.set(ctx, 0, menu)?;
            let hwnd = word(ctx, o + 0x20)?;
            let roots = ctx
                .kernel
                .menu_items
                .get(&menu)
                .cloned()
                .unwrap_or_default();
            let count = roots.iter().take_while(|i| i.submenu != 0).count() as u32;
            let existing = crate::coredll::toolbar::message(ctx, hwnd, 0x418, 0, 0)?.unwrap_or(0);
            let at = f.arg(ctx, 2)?;
            let at = if at == existing || matches!(at, 0xffff | u32::MAX) {
                existing
            } else {
                at
            };
            if count > 4096 || at > existing || count > 4096 - existing {
                crate::coredll::menu_api::destroy_owned(ctx.kernel, menu);
                return f.finish(ctx, 0);
            }
            let wrapper = match super::menus::new(ctx, menu) {
                Ok(p) => p,
                Err(e) => {
                    crate::coredll::menu_api::destroy_owned(ctx.kernel, menu);
                    return Err(e);
                }
            };
            f.set(ctx, 1, wrapper)?;
            f.set(ctx, 2, 0)?;
            f.set(ctx, 3, count)?;
            f.set(ctx, 4, at)?;
            // Reserve the pointer-array owner before publishing any borrowed submenu.
            let size = word(ctx, o + 0xc0)?;
            if let Err(e) =
                super::arrays::resize(ctx, o + 0xb8, size + 1, -1, super::arrays::Element::Pointer)
            {
                super::menus::detach_value(ctx, wrapper)?;
                ctx.kernel.heap.free(wrapper);
                crate::coredll::menu_api::destroy_owned(ctx.kernel, menu);
                return Err(e);
            }
            f.set(ctx, 5, size)?;
            menu_next(ctx, f, o)
        }
        2 => {
            let buffer = f.get(ctx, 6)?;
            ctx.kernel.heap.free(buffer);
            f.set(ctx, 6, 0)?;
            if result == 0 {
                return rollback_menu(ctx, f, o);
            }
            let index = f.get(ctx, 2)?;
            f.set(ctx, 2, index + 1)?;
            menu_next(ctx, f, o)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC menu insertion continuation".into(),
        )),
    }
}
fn menu_next(ctx: &mut CallCtx<'_>, f: Frame, o: u32) -> Result<DispatchOutcome, KernelError> {
    let index = f.get(ctx, 2)?;
    let count = f.get(ctx, 3)?;
    let menu = f.get(ctx, 0)?;
    if index == count {
        let size = f.get(ctx, 5)?;
        let wrapper = f.get(ctx, 1)?;
        let data = word(ctx, o + 0xbc)?;
        put(ctx, data + size * 4, wrapper)?;
        let parent = word(ctx, o + 0x78)?;
        if parent != 0 {
            let hwnd = word(ctx, parent + 0x20)?;
            if !ctx.kernel.window_menus.contains_key(&hwnd) {
                ctx.kernel.window_menus.insert(hwnd, menu);
            }
            let flags = word(ctx, parent + 0xe4)?;
            put(ctx, parent + 0xe4, flags | 8)?;
        }
        let hwnd = word(ctx, o + 0x20)?;
        let total = crate::coredll::toolbar::message(ctx, hwnd, 0x418, 0, 0)?.unwrap_or(0);
        put(ctx, o + 0x64, total)?;
        return f.finish(ctx, wrapper);
    }
    let item = ctx.kernel.menu_items[&menu][index as usize].clone();
    let text = item
        .text
        .iter()
        .copied()
        .filter(|&c| c != 38)
        .take(255)
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let buffer = alloc(ctx, text.len() as u32)?;
    if let Err(e) = ctx.cpu.write_mem(buffer, &text) {
        ctx.kernel.heap.free(buffer);
        return Err(e.into());
    }
    // The shared toolbar copies this label synchronously; no guest pointer escapes.
    let next = ctx.cpu.read_mem(o + 0xd8, 2)?;
    let command = u16::from_le_bytes(next.try_into().unwrap());
    let record = [
        u32::MAX - 1,
        u32::from(command),
        4 | (0x98 << 8),
        item.submenu,
        buffer,
    ];
    for (i, value) in record.into_iter().enumerate() {
        put(ctx, f.address + 128 + i as u32 * 4, value)?;
    }
    let at = f.get(ctx, 4)? + index;
    f.set(ctx, 6, buffer)?;
    ctx.cpu
        .write_mem(o + 0xd8, &command.wrapping_add(1).to_le_bytes())?;
    let target = virtual_function(ctx, o, 40)?;
    f.call_args(ctx, 2, target, &[o, 0x415, at, f.address + 128])
}
fn rollback_menu(ctx: &mut CallCtx<'_>, f: Frame, o: u32) -> Result<DispatchOutcome, KernelError> {
    let hwnd = word(ctx, o + 0x20)?;
    let at = f.get(ctx, 4)?;
    let done = f.get(ctx, 2)?;
    for _ in 0..done {
        crate::coredll::toolbar::message(ctx, hwnd, 0x416, at, 0)?;
    }
    let size = f.get(ctx, 5)?;
    super::arrays::resize(ctx, o + 0xb8, size, -1, super::arrays::Element::Pointer)?;
    let wrapper = f.get(ctx, 1)?;
    let menu = super::menus::detach_value(ctx, wrapper)?;
    ctx.kernel.heap.free(wrapper);
    crate::coredll::menu_api::destroy_owned(ctx.kernel, menu);
    f.finish(ctx, 0)
}
fn delete_control(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct(ctx, "CControlBar", false)
}
fn delete_toolbar(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct(ctx, "CToolBar", false)
}
fn delete_command(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct(ctx, "CCeCommandBar", false)
}
pub(super) fn deleting(ctx: &mut CallCtx<'_>, class: &str) -> Result<DispatchOutcome, KernelError> {
    destruct(ctx, class, true)
}
fn destruct(
    ctx: &mut CallCtx<'_>,
    class: &str,
    deleting: bool,
) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        let flags = f.arg(ctx, 1)?;
        f.set(ctx, 0, u32::from(deleting && flags & 1 != 0))?;
        f.set(
            ctx,
            1,
            match class {
                "CControlBar" => 124,
                "CToolBar" => 164,
                _ => 224,
            },
        )?;
        f.set(ctx, 3, 0)?;
        return destruct_next(ctx, f, o);
    } else if matches!(f.phase(ctx)?, 10 | 20 | 21) {
        if f.phase(ctx)? == 20 {
            put(ctx, o + 0x84, 0)?;
        }
        if f.phase(ctx)? == 21 {
            put(ctx, o + 0xa0, 0)?;
        }
        return destruct_next(ctx, f, o);
    } else {
        match f.phase(ctx)? {
            1 => {
                let hwnd = f.get(ctx, 2)?;
                return f.call_service(ctx, 2, "DestroyWindow", &[hwnd]);
            }
            2 => {}
            _ => {
                return Err(KernelError::Dispatch(
                    "Invalid MFC bar destruction continuation".into(),
                ))
            }
        }
    }
    finish_destruct(ctx, f, o)
}
fn destruct_next(ctx: &mut CallCtx<'_>, f: Frame, o: u32) -> Result<DispatchOutcome, KernelError> {
    if f.get(ctx, 1)? == 224 {
        for offset in [0xb8, 0xa4] {
            let count = word(ctx, o + offset + 8)?;
            if count > 4096 {
                return Err(KernelError::Dispatch(
                    "MFC owned bar array exceeds its bound".into(),
                ));
            }
            if count != 0 {
                let data = word(ctx, o + offset + 4)?;
                let child = word(ctx, data + (count - 1) * 4)?;
                put(ctx, data + (count - 1) * 4, 0)?;
                put(ctx, o + offset + 8, count - 1)?;
                if child != 0 {
                    if offset == 0xb8 {
                        let menu = word(ctx, child + 4)?;
                        let parent = word(ctx, o + 0x78)?;
                        if parent != 0 {
                            let hwnd = word(ctx, parent + 0x20)?;
                            if ctx.kernel.window_menus.get(&hwnd) == Some(&menu) {
                                ctx.kernel.window_menus.remove(&hwnd);
                            }
                        }
                    } else {
                        super::window_handles::detach_value(ctx, child)?;
                    }
                    let target = virtual_function(ctx, child, 1)?;
                    return f.call_args(ctx, 10, target, &[child, 1]);
                }
                return destruct_next(ctx, f, o);
            }
        }
    }
    if f.get(ctx, 1)? >= 164 {
        // Retail CE3 stores an owned HBITMAP here, not an image-list wrapper.
        if f.get(ctx, 3)? == 0 {
            f.set(ctx, 3, 1)?;
            let bitmap = word(ctx, o + 0x84)?;
            if bitmap != 0 {
                return f.call_service(ctx, 20, "DeleteObject", &[bitmap]);
            }
        }
        if f.get(ctx, 3)? == 1 {
            f.set(ctx, 3, 2)?;
            let owner = word(ctx, o + 0xa0)?;
            if owner != 0 {
                return f.call_virtual(ctx, 21, owner, 1, &[1]);
            }
        }
        put(ctx, o + 0x64, 0)?;
    }
    let hwnd = word(ctx, o + 0x20)?;
    if hwnd != 0 {
        f.set(ctx, 2, hwnd)?;
        return f.call_service(ctx, 1, UNSUBCLASS, &[o]);
    }
    finish_destruct(ctx, f, o)
}
fn finish_destruct(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    o: u32,
) -> Result<DispatchOutcome, KernelError> {
    let parent = word(ctx, o + 0x78)?;
    if parent != 0 {
        unlink(ctx, parent, o)?;
        let hwnd = f.get(ctx, 2)?;
        if word(ctx, parent + 0x4c)? == hwnd {
            put(ctx, parent + 0x4c, 0)?;
        }
        put(ctx, o + 0x78, 0)?;
    }
    let bytes = f.get(ctx, 1)?;
    if bytes == 224 {
        for offset in [0xa4, 0xb8] {
            super::arrays::resize(ctx, o + offset, 0, -1, super::arrays::Element::Pointer)?;
        }
    }
    for offset in [0x68] {
        let memory = word(ctx, o + offset)?;
        if memory != 0 {
            ctx.kernel.heap.free(memory);
            put(ctx, o + offset, 0)?;
        }
    }
    let table = super::classes::vtable(ctx, "CWnd")?;
    put(ctx, o, table)?;
    let free = f.get(ctx, 0)? != 0;
    if free {
        super::dialog_init::forget_owned_bar(ctx, o)?;
        ctx.kernel.heap.free(o);
    }
    f.finish(ctx, o)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    #[test]
    fn inserted_menu_owns_real_submenus_and_virtual_teardown_releases_wrappers_and_resources() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let mut words = vec![0u16, 0, 0x90];
        words.extend("&File".encode_utf16().chain(Some(0)));
        words.extend([0x80, 81]);
        words.extend("Execute".encode_utf16().chain(Some(0)));
        let bytes = words
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        cpu.write_mem(0x3000, &bytes).unwrap();
        k.image_base = 0x1000;
        k.resources.push(pocket_pe::ResourceEntry {
            ty: pocket_pe::ResourceKey::Id(4),
            name: pocket_pe::ResourceKey::Id(99),
            data_rva: 0x2000,
            native_leaves: Vec::new(),
            size: bytes.len() as u32,
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
        k.window_classes.insert(7, "Frame".into());
        cpu.write_mem(0x2020, &7u32.to_le_bytes()).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:48",
            &[0x2400],
        );
        for (i, v) in [0x2400, 0x2000, 0x50002800, 0xe800].into_iter().enumerate() {
            cpu.write_arg(i as u8, v).unwrap();
        }
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(2714), |pc, _| panic!(
                "unexpected guest {pc:x}"
            )),
            1
        );
        for (i, v) in [0x2400, 99, 0xffff].into_iter().enumerate() {
            cpu.write_arg(i as u8, v).unwrap();
        }
        let wrapper = drive(&mut cpu, &mut k, &mut d, entry(1676), |pc, _| {
            panic!("unexpected guest {pc:x}")
        });
        assert_ne!(wrapper, 0);
        let root = cpu.read_u32_le(wrapper + 4).unwrap();
        let submenu = k.menu_items[&root][0].submenu;
        assert_ne!(submenu, 0);
        let hwnd = cpu.read_u32_le(0x2420).unwrap();
        let display = k.gdi.shell_menus.bars[&hwnd].menu;
        assert_eq!(
            k.menu_items[&display][0].text,
            "File".encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(k.menu_items[&display][0].submenu, submenu);
        assert_eq!(k.menu_items[&submenu][0].id, 81);
        assert_eq!(cpu.read_u32_le(0x24c0).unwrap(), 1);
        cpu.write_arg(0, 0x2400).unwrap();
        drive(&mut cpu, &mut k, &mut d, entry(219), |pc, _| {
            panic!("unexpected guest {pc:x}")
        });
        assert!(!k.menu_items.contains_key(&root));
        assert!(!k.menu_items.contains_key(&submenu));
        assert!(!k.menu_items.contains_key(&display));
        assert!(k.heap.msize(wrapper).is_none());
        assert!(!k.window_menus.contains_key(&7));
        assert!(!k.dynamic_exports[&1]
            .keys()
            .any(|key| key.starts_with("__PocketHLE_MFC_Menu:")));
    }
    #[test]
    fn command_bar_creation_links_real_frame_and_teardown_releases_its_hwnd_and_menu_projection() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:103",
            &[0x2000],
        );
        k.window_classes.insert(7, "Frame".into());
        cpu.write_mem(0x2020, &7u32.to_le_bytes()).unwrap();
        // Warm lazy process-lifetime class tables before the allocation comparison.
        for class in ["CCeCommandBar", "CToolBar", "CControlBar", "CObArray"] {
            let thunk = entry(48);
            let mut ctx = crate::CallCtx {
                cpu: &mut cpu,
                kernel: &mut k,
                thunk: &thunk,
            };
            super::super::classes::vtable(&mut ctx, class).unwrap();
        }
        // DestroyDockBars enters the real message map; ordinary explicit bar
        // destruction first unsubclasses. Both paths share process-lifetime maps.
        for name in ["ord:2727", "ord:1374", "ord:1353", "ord:1380", "ord:651"] {
            call(&mut cpu, &mut k, &mut d, "mfcce300.dll", name, &[]);
        }
        let free = k.heap.free_bytes();
        let roots = k.dynamic_exports[&1].clone();
        let menus = k.menu_items.len();
        for iteration in 0..2 {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:48",
                &[0x2400],
            );
            for (i, v) in [0x2400, 0x2000, 0x50002800, 0xe800].into_iter().enumerate() {
                cpu.write_arg(i as u8, v).unwrap();
            }
            assert_eq!(
                drive(&mut cpu, &mut k, &mut d, entry(2714), |pc, _| panic!(
                    "unexpected guest {pc:x}"
                )),
                1
            );
            let hwnd = cpu.read_u32_le(0x2420).unwrap();
            assert_ne!(hwnd, 0);
            assert_eq!(cpu.read_u32_le(0x2478).unwrap(), 0x2000);
            assert_eq!(cpu.read_u32_le(0x208c).unwrap(), 1);
            assert_eq!(cpu.read_u32_le(0x204c).unwrap(), hwnd);
            assert_eq!(k.gdi.shell_toolbars[&hwnd].state.buttons.len(), 0);
            if iteration == 1 {
                cpu.write_arg(0, 0x2000).unwrap();
                drive(&mut cpu, &mut k, &mut d, entry(1053), |pc, _| {
                    panic!("unexpected guest {pc:x}")
                });
                assert!(!k.window_classes.contains_key(&hwnd));
                assert_eq!(cpu.read_u32_le(0x208c).unwrap(), 0);
            }
            cpu.write_arg(0, 0x2400).unwrap();
            drive(&mut cpu, &mut k, &mut d, entry(219), |pc, _| {
                panic!("unexpected guest {pc:x}")
            });
            assert!(!k.window_classes.contains_key(&hwnd));
            assert!(!k.gdi.shell_menus.bars.contains_key(&hwnd));
            assert!(!k.gdi.shell_toolbars.contains_key(&hwnd));
            assert_eq!(cpu.read_u32_le(0x208c).unwrap(), 0);
            // Retail CCeCommandBar clears the frame cache only while its HWND
            // is still attached. DestroyDockBars already detached that window.
            if iteration == 0 {
                assert_eq!(cpu.read_u32_le(0x204c).unwrap(), 0);
            }
            assert_eq!(k.menu_items.len(), menus);
            assert_eq!(
                k.heap.free_bytes(),
                free,
                "new roots: {:?}",
                k.dynamic_exports[&1]
                    .iter()
                    .filter(|(key, _)| !roots.contains_key(*key))
                    .map(|(key, address)| (key, address, k.heap.msize(*address)))
                    .collect::<Vec<_>>()
            );
        }
    }
    #[test]
    fn command_bar_constructor_initializes_only_native_fields_and_arrays_borrow_their_elements() {
        let (mut cpu, mut k, mut d) = framework_setup();
        cpu.write_mem(0x1ffc, &[0xa5; 232]).unwrap();
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:48",
                &[0x2000]
            )),
            0x2000
        );
        assert_eq!(cpu.read_mem(0x1ffc, 4).unwrap(), [0xa5; 4]);
        assert_eq!(cpu.read_mem(0x20e0, 4).unwrap(), [0xa5; 4]);
        assert_eq!(cpu.read_u32_le(0x208c).unwrap(), 0x2000);
        assert_eq!(cpu.read_u32_le(0x2094).unwrap(), 15);
        assert_eq!(cpu.read_mem(0x20d8, 2).unwrap(), 0xf000u16.to_le_bytes());
        assert_eq!(cpu.read_mem(0x20a8, 16).unwrap(), [0; 16]);
        assert_eq!(cpu.read_mem(0x20bc, 16).unwrap(), [0; 16]);
        let free = k.heap.free_bytes();
        cpu.write_arg(0, 0x2000).unwrap();
        drive(&mut cpu, &mut k, &mut d, entry(219), |pc, _| {
            panic!("unexpected guest {pc:x}")
        });
        assert_eq!(k.heap.free_bytes(), free);
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
    }
}

fn runtime_control(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx,
        "CControlBar",
    )?))
}
fn runtime_toolbar(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx, "CToolBar",
    )?))
}
fn runtime_command(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx,
        "CCeCommandBar",
    )?))
}
fn shell_layout(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // The CE shell popup takes no space in the frame's docked-bar layout.
    // CSize is an indirect eight-byte return at r1 in this ARM ABI.
    let output = ctx.arg_u32(1)?;
    ctx.cpu.check_guest_access(output, 8, Prot::WRITE)?;
    ctx.cpu.write_mem(output, &[0; 8])?;
    Ok(DispatchOutcome::ReturnedR0(output))
}

fn post_nc_destroy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        return f.finish(ctx, 0);
    }
    let object = f.arg(ctx, 0)?;
    if word(ctx, object + 0x48)? == 0 {
        return f.finish(ctx, 0);
    }
    let target = virtual_function(ctx, object, 1)?;
    f.call_args(ctx, 1, target, &[object, 1])
}
fn on_destroy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let parent = word(ctx, object + 0x78)?;
        if parent != 0 {
            unlink(ctx, parent, object)?;
            put(ctx, object + 0x78, 0)?;
        }
        let target = service(ctx, super::window_lifetime::ON_DESTROY)?;
        return f.call_args(ctx, 1, target, &[object]);
    }
    f.finish(ctx, 0)
}

fn destroy_window(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        return f.finish(ctx, result);
    }
    let object = f.arg(ctx, 0)?;
    // CE3 IsFloating is identically FALSE; no floating mini-frame is owned
    // by this platform's CControlBar, regardless of opaque member contents.
    f.call_service(ctx, 1, super::window_lifetime::DESTROY, &[object])
}

fn is_floating(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // Verified retail CE3 routine 0x13ba3c; CE has no desktop docking frame.
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn destroy_bars(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let frame = f.arg(ctx, 0)?;
    if f.fresh {
        // Validate the intrusive list before invoking any guest virtual method.
        super::lists::search(ctx, frame + 0x80, 0, 0)?;
        let first = super::lists::next_position(ctx, frame + 0x80, 0)?;
        f.set(ctx, 0, first)?;
    }
    let node = f.get(ctx, 0)?;
    if node == 0 {
        return f.finish(ctx, 0);
    }
    let count = f.get(ctx, 1)?;
    if count >= 4096 {
        return Err(KernelError::Dispatch(
            "MFC control-bar destruction exceeded its bound".into(),
        ));
    }
    // DestroyWindow can unlink and free this node. Native CE3 advances POSITION
    // before the call, and never dereferences the removed node on return.
    let next = super::lists::next_position(ctx, frame + 0x80, node)?;
    let bar = word(ctx, node + 8)?;
    f.set(ctx, 0, next)?;
    f.set(ctx, 1, count + 1)?;
    f.call_virtual(ctx, 1, bar, 23, &[])
}
fn docking_frame(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_service(ctx, 1, super::hierarchy::PARENT, &[o]);
    }
    let parent = ctx.cpu.read_reg(ArmReg::R0)?;
    let frame = if parent == 0 {
        word(ctx, o + 0x78)?
    } else {
        parent
    };
    f.finish(ctx, frame)
}
