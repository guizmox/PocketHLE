//! PocketPC MFC document-list windows adapt the shared VFS-backed control.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", "__PocketHLE_MFC_CreateDocListWindow", create);
}
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (2693, "??0CCeDocList@@QAA@XZ", construct),
    (
        3551,
        "??0CCeDocList@@QAA@PAVCCeDocListDocTemplate@@@Z",
        construct_template,
    ),
    (2697, "??1CCeDocList@@UAA@XZ", destruct),
    (2715, "?Create@CCeDocList@@QAAHPAVCWnd@@PBG1K@Z", create),
    (2748, "?OnCreate@CCeDocList@@UAAXXZ", on_create),
    (2746, "?OnClose@CCeDocList@@UAAXXZ", on_close),
    (
        2753,
        "?OnItemActivated@CCeDocList@@QAAXPAUtagDLNHDR@@@Z",
        activated,
    ),
    (
        2720,
        "?DeleteSelection@CCeDocList@@QAAJXZ",
        delete_selection,
    ),
    (2722, "?DisableUpdate@CCeDocList@@QAAXXZ", disable),
    (2723, "?EnableUpdate@CCeDocList@@QAAXXZ", enable),
    (2725, "?GetFilterIndex@CCeDocList@@QAAHXZ", filter_index),
    (2726, "?GetItemCount@CCeDocList@@QAAHXZ", count),
    (2733, "?GetNextItem@CCeDocList@@QAAHHI@Z", next),
    (2740, "?GetSelectCount@CCeDocList@@QAAHXZ", selected_count),
    (
        2741,
        "?GetSelectedPathname@CCeDocList@@QAAHPAGG@Z",
        selected_path,
    ),
    (2771, "?Refresh@CCeDocList@@QAAHXZ", refresh),
    (2774, "?SelectItem@CCeDocList@@QAAJPAGH@Z", select),
    (2777, "?SetFilterIndex@CCeDocList@@QAAJH@Z", set_filter),
    (2778, "?SetFolder@CCeDocList@@QAAXPAG@Z", set_folder),
    (2779, "?SetItemState@CCeDocList@@QAAHHII@Z", set_item_state),
    (
        2780,
        "?SetSelectedPathname@CCeDocList@@QAAXPAG@Z",
        set_selected,
    ),
];
pub(super) fn construct_value(
    ctx: &mut CallCtx<'_>,
    object: u32,
    template: u32,
) -> Result<DispatchOutcome, KernelError> {
    windows::initialize_target(ctx, object, "CCeDocList", 628)?;
    new_string(ctx, object + 0x4c, &[])?;
    put(ctx, object + 0x48, template)?;
    Ok(DispatchOutcome::ReturnedR0(object))
}
fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    construct_value(ctx, object, 0)
}
fn construct_template(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let template = ctx.arg_u32(1)?;
    construct_value(ctx, object, template)
}
fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    delete(ctx, false)
}
pub(super) fn delete(
    ctx: &mut CallCtx<'_>,
    deleting: bool,
) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let flags = f.arg(ctx, 1)?;
        if deleting && flags & !1 != 0 {
            return Err(KernelError::Dispatch(
                "MFC document-list vector deletion unsupported".into(),
            ));
        }
        f.set(ctx, 0, u32::from(deleting && flags & 1 != 0))?;
        let table = classes::vtable(ctx, "CCeDocList")?;
        put(ctx, object, table)?;
        if word(ctx, object + 0x20)? != 0 || word(ctx, object + 0x50)? != 0 {
            return f.call_virtual(ctx, 1, object, 47, &[]);
        }
    }
    if f.fresh || f.phase(ctx)? == 1 {
        release_string(ctx, object + 0x4c)?;
        return f.call_service(ctx, 2, window_lifetime::DESTRUCT, &[object]);
    }
    if f.get(ctx, 0)? != 0 {
        ctx.kernel.heap.free(object);
    }
    f.finish(ctx, object)
}
fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let parent = f.arg(ctx, 1)?;
        let filter = f.arg(ctx, 2)?;
        let folder = f.arg(ctx, 3)?;
        let flags = ctx.arg_u32(4)?;
        let h = if parent == 0 {
            0
        } else {
            word(ctx, parent + 0x20)?
        };
        if h == 0 {
            return f.finish(ctx, 0);
        }
        let folder = if folder == 0 {
            "\\My Documents".encode_utf16().collect::<Vec<_>>()
        } else {
            strings::wide(ctx, folder)?
        };
        if folder.len() >= 260 {
            return Err(KernelError::Dispatch(
                "Document-list folder exceeds MAX_PATH".into(),
            ));
        }
        let raw: Vec<_> = folder
            .iter()
            .copied()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        ctx.cpu.write_mem(object + 0x6c, &raw)?;
        let filter = if filter == 0 {
            Vec::new()
        } else {
            let supplied = strings::wide(ctx, filter)?;
            "All Files|*.*|".encode_utf16().chain(supplied).collect()
        };
        // MFC's filter uses pipes; the WinCE control consumes double-null pairs.
        let filter: Vec<_> = filter
            .into_iter()
            .map(|c| if c == b'|' as u16 { 0 } else { c })
            .collect();
        strings::replace(ctx, object + 0x4c, &filter)?;
        let filter = word(ctx, object + 0x4c)?;
        let info = object + 0x54;
        for (i, v) in [24, h, object + 0x6c, filter, 1 | (0xe40a << 16), flags]
            .into_iter()
            .enumerate()
        {
            put(ctx, info + i as u32 * 4, v)?;
        }
        return f.call_service(ctx, 1, "__PocketHLE_DocListCreate", &[info]);
    }
    if f.phase(ctx)? == 1 {
        let h = ctx.cpu.read_reg(ArmReg::R0)?;
        if h == 0 {
            return f.finish(ctx, 0);
        }
        if !window_handles::associate(ctx, object, h)? {
            return f.call_service(ctx, 3, "DestroyWindow", &[h]);
        }
        return f.call_virtual(ctx, 2, object, 46, &[]);
    }
    let success = u32::from(f.phase(ctx)? == 2);
    f.finish(ctx, success)
}
fn on_create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let bar = alloc_cpp(ctx, 228)?;
        f.set(ctx, 0, bar)?;
        // The existing native-compatible shell-bar constructor owns its fields.
        let constructor = super::handler_named("??0CCeCommandBar@@QAA@XZ")
            .ok_or_else(|| KernelError::Dispatch("Command-bar constructor missing".into()))?;
        ctx.cpu.write_arg(0, bar)?;
        if let Err(error) = constructor(ctx) {
            ctx.kernel.heap.free(bar);
            return Err(error);
        }
        let table = classes::vtable(ctx, "CCeDocListCB")?;
        put(ctx, bar, table)?;
        put(ctx, bar + 0xe0, object)?;
        put(ctx, bar + 0xdc, 1)?;
        put(ctx, object + 0x50, bar)?;
        let target = callbacks::virtual_function(ctx, bar, 56)?;
        return f.call_args(
            ctx,
            1,
            target,
            &[bar, object, 0x800, 0x50002000, 0, 0, 0, 0, 0xe800],
        );
    }
    if f.phase(ctx)? == 2 {
        return f.finish(ctx, 0);
    }
    let bar = f.get(ctx, 0)?;
    if ctx.cpu.read_reg(ArmReg::R0)? == 0 {
        put(ctx, object + 0x50, 0)?;
        return f.call_virtual(ctx, 2, bar, 1, &[1]);
    }
    f.finish(ctx, 0)
}
fn on_close(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let phase = if f.fresh { 0 } else { f.phase(ctx)? };
    let bar = word(ctx, object + 0x50)?;
    if phase == 0 && bar != 0 {
        return f.call_virtual(ctx, 1, bar, 23, &[]);
    }
    if phase <= 1 && bar != 0 {
        return f.call_virtual(ctx, 2, bar, 1, &[1]);
    }
    put(ctx, object + 0x50, 0)?;
    if phase <= 2 {
        return f.call_service(ctx, 3, window_lifetime::DESTROY, &[object]);
    }
    f.finish(ctx, 0)
}
fn activated(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let info = f.arg(ctx, 1)?;
        let path = word(ctx, info + 16)?;
        let app = application::thread_object(ctx)?;
        return f.call_virtual(ctx, 1, app, 31, &[path]);
    }
    if f.phase(ctx)? == 1 && ctx.cpu.read_reg(ArmReg::R0)? != 0 {
        return f.call_virtual(ctx, 2, object, 47, &[]);
    }
    f.finish(ctx, 0)
}
fn send(
    ctx: &mut CallCtx<'_>,
    message: u32,
    w: u32,
    l: u32,
) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        let h = word(ctx, object + 0x20)?;
        return f.call_service(ctx, 1, "SendMessageW", &[h, message, w, l]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    f.finish(ctx, result)
}
fn delete_selection(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    send(ctx, 0x66b, 0, 0)
}
fn disable(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    send(ctx, 0x65b, 0, 0)
}
fn enable(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    send(ctx, 0x65c, 0, 0)
}
fn filter_index(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    send(ctx, 0x666, 0, 0)
}
fn count(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    send(ctx, 0x1004, 0, 0)
}
fn selected_count(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    send(ctx, 0x665, 0, 0)
}
fn refresh(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    if frame.fresh {
        let object = frame.arg(ctx, 0)?;
        let hwnd = word(ctx, object + 0x20)?;
        return frame.call_service(ctx, 1, "SendMessageW", &[hwnd, 0x658, 0, 0]);
    }
    // The native MFC wrapper ignores DLM_REFRESH's result and returns TRUE.
    frame.finish(ctx, 1)
}
fn next(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let w = ctx.arg_u32(1)?;
    let l = ctx.arg_u32(2)? as u16 as u32;
    send(ctx, 0x100c, w, l)
}
fn selected_path(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(1)?;
    let cap = ctx.arg_u32(2)? as u16 as u32;
    send(ctx, 0x65e, cap, p)
}
fn select(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(1)?;
    let on = ctx.arg_u32(2)?;
    send(ctx, 0x65f, on, p)
}
fn set_filter(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let index = ctx.arg_u32(1)?;
    send(ctx, 0x659, index, 0)
}
fn set_folder(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(1)?;
    send(ctx, 0x664, 0, p)
}
fn set_selected(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(1)?;
    send(ctx, 0x667, 0, p)
}
fn set_item_state(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        let index = f.arg(ctx, 1)?;
        let state = f.arg(ctx, 2)?;
        let mask = f.arg(ctx, 3)?;
        ctx.cpu.write_mem(f.address + 128, &[0; 40])?;
        put(ctx, f.address + 140, state)?;
        put(ctx, f.address + 144, mask)?;
        let h = word(ctx, object + 0x20)?;
        return f.call_service(ctx, 1, "SendMessageW", &[h, 0x102b, index, f.address + 128]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    f.finish(ctx, result)
}
