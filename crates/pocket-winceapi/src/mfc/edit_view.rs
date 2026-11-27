//! CE3 CEditView adapts the existing EDIT HWND and archive stream.
//! Cached text is guest-owned view storage, never a second editor model.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
const LOCK: &str = "__PocketHLE_MFC_EditViewLock";
const READ: &str = "__PocketHLE_MFC_EditViewRead";
const WRITE: &str = "__PocketHLE_MFC_EditViewWrite";
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler(
        "coredll.dll",
        "__PocketHLE_MFC_EditInitializeReplace",
        initialize_replace,
    );
    d.register_handler(
        "coredll.dll",
        "__PocketHLE_MFC_EditSameSelected",
        same_selected,
    );
    d.register_handler("coredll.dll", "__PocketHLE_MFC_EditFindText", find_text);
    for (name, handler) in [
        (LOCK, lock as crate::Handler),
        (READ, read_archive),
        (WRITE, write_archive),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        2048,
        "?OnUpdateNeedFind@CEditView@@IAAXPAVCCmdUI@@@Z",
        update_find,
    ),
    (
        1632,
        "?InitializeReplace@CEditView@@IAAHXZ",
        initialize_replace,
    ),
    (1997, "?OnReplaceAll@CEditView@@MAAXPBG0H@Z", replace_all),
    (
        1999,
        "?OnReplaceSel@CEditView@@MAAXPBGHH0@Z",
        replace_selection,
    ),
    (
        3563,
        "?DismissFindReplaceBar@CEditView@@QAAXXZ",
        dismiss_find,
    ),
    (
        1912,
        "?OnFindReplaceCmd@CEditView@@IAAJIJ@Z",
        find_replace_command,
    ),
    (
        831,
        "?CalcWindowRect@CEditView@@MAAXPAUtagRECT@@I@Z",
        calc_rect,
    ),
    (1165, "?FindText@CEditView@@QAAHPBGHH@Z", find_text),
    (1911, "?OnFindNext@CEditView@@MAAXPBGHH@Z", find_next),
    (2035, "?OnTextNotFound@CEditView@@MAAXPBG@Z", not_found),
    (1915, "?OnGetDlgCode@CEditView@@IAAJXZ", dialog_code),
    (2014, "?OnSetFont@CEditView@@IAAJIJ@Z", set_font),
    (1866, "?OnDestroy@CEditView@@IAAXXZ", on_destroy),
    (3556, "?OnChar@CEditView@@IAAXIII@Z", default_input),
    (3558, "?OnKeyUp@CEditView@@IAAXIII@Z", default_input),
    (
        3559,
        "?OnLButtonUp@CEditView@@IAAXIVCPoint@@@Z",
        default_input,
    ),
    (1882, "?OnEditCopy@CEditView@@IAAXXZ", copy),
    (1883, "?OnEditCut@CEditView@@IAAXXZ", cut),
    (1886, "?OnEditPaste@CEditView@@IAAXXZ", paste),
    (
        2047,
        "?OnUpdateNeedClip@CEditView@@IAAXPAVCCmdUI@@@Z",
        update_clip,
    ),
    (
        1848,
        "?OnCreate@CEditView@@IAAHPAUtagCREATESTRUCTW@@@Z",
        on_create,
    ),
    (91, "??0CEditView@@QAA@XZ", construct),
    (240, "??1CEditView@@UAA@XZ", destruct),
    (1034, "?DeleteContents@CEditView@@UAAXXZ", delete_contents),
    (1230, "?GetBufferLength@CEditView@@QBAIXZ", length),
    (
        1558,
        "?GetSelectedText@CEditView@@QBAXAAVCString@@@Z",
        selected,
    ),
    (1732, "?LockBuffer@CEditView@@QBAPBGXZ", lock),
    (2484, "?UnlockBuffer@CEditView@@QBAXXZ", unlock),
    (
        2092,
        "?PreCreateWindow@CEditView@@MAAHAAUtagCREATESTRUCTW@@@Z",
        pre_create,
    ),
    (
        2091,
        "?PreCreateWindow@CCtrlView@@MAAHAAUtagCREATESTRUCTW@@@Z",
        ctrl_pre_create,
    ),
    (1875, "?OnDraw@CCtrlView@@MAAXPAVCDC@@@Z", control_draw),
    (1977, "?OnPaint@CCtrlView@@IAAXXZ", control_paint),
    (2285, "?Serialize@CEditView@@UAAXAAVCArchive@@@Z", serialize),
    (
        2297,
        "?SerializeRaw@CEditView@@QAAXAAVCArchive@@@Z",
        serialize_raw,
    ),
    (
        2144,
        "?ReadFromArchive@CEditView@@QAAXAAVCArchive@@I@Z",
        read_archive,
    ),
    (
        2596,
        "?WriteToArchive@CEditView@@QAAXAAVCArchive@@@Z",
        write_archive,
    ),
    (2427, "?SetTabStops@CEditView@@QAAXH@Z", set_tabs),
    (1880, "?OnEditChange@CEditView@@IAAHXZ", changed),
    (
        2049,
        "?OnUpdateNeedSel@CEditView@@IAAXPAVCCmdUI@@@Z",
        update_selection,
    ),
    (
        2050,
        "?OnUpdateNeedText@CEditView@@IAAXPAVCCmdUI@@@Z",
        update_text,
    ),
    (
        2043,
        "?OnUpdateEditUndo@CEditView@@IAAXPAVCCmdUI@@@Z",
        update_undo,
    ),
    (1889, "?OnEditSelectAll@CEditView@@IAAXXZ", select_all),
    (1881, "?OnEditClear@CEditView@@IAAXXZ", clear_selection),
    (1890, "?OnEditUndo@CEditView@@IAAXXZ", undo),
    (2237, "?SameAsSelected@CEditView@@IAAHPBGH@Z", same_selected),
];
fn hwnd(ctx: &mut CallCtx<'_>, object: u32) -> Result<u32, KernelError> {
    word(ctx, object + 0x20)
}
fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    windows::initialize_target(ctx, object, "CEditView", 96)?;
    new_string(
        ctx,
        object + 0x4c,
        &"EDIT".encode_utf16().collect::<Vec<_>>(),
    )?;
    put(ctx, object + 0x54, 32)?;
    Ok(DispatchOutcome::ReturnedR0(object))
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
                "MFC CEditView vector deletion is unsupported".into(),
            ));
        }
        f.set(ctx, 0, u32::from(deleting && flags & 1 != 0))?;
        let buffer = word(ctx, object + 0x58)?;
        if buffer != 0 {
            ctx.kernel.heap.free(buffer);
        }
        put(ctx, object + 0x58, 0)?;
        put(ctx, object + 0x5c, 0)?;
        release_string(ctx, object + 0x4c)?;
        let table = classes::vtable(ctx, "CView")?;
        let destructor = word(ctx, table + 4)?;
        return f.call_args(ctx, 1, destructor, &[object, 0]);
    }
    if f.get(ctx, 0)? != 0 {
        ctx.kernel.heap.free(object);
    }
    f.finish(ctx, object)
}
fn ctrl_pre_create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        let cs = f.arg(ctx, 1)?;
        let class = word(ctx, object + 0x4c)?;
        put(ctx, cs + 40, class)?;
        let table = classes::vtable(ctx, "CView")?;
        let target = word(ctx, table + 24 * 4)?;
        return f.call_args(ctx, 1, target, &[object, cs]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    f.finish(ctx, result)
}
fn pre_create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    ctrl_pre_create(ctx)
}
// Native CCtrlView delegates drawing to its control superclass. No separate
// MFC rasterization or synthetic successful paint is introduced.
fn control_draw(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn control_paint(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        return f.call_service(ctx, 1, "__PocketHLE_MFC_WindowDefault", &[object]);
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
        let h = hwnd(ctx, object)?;
        return f.call_service(ctx, 1, "SendMessageW", &[h, message, w, l]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    f.finish(ctx, result)
}
fn delete_contents(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    send(ctx, 0xc, 0, 0)
}
fn select_all(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    send(ctx, 0xb1, 0, u32::MAX)
}
fn clear_selection(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    send(ctx, 0xc2, 1, 0)
}
fn undo(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    send(ctx, 0xc7, 0, 0)
}
fn unlock(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // Native CE3 UnlockBuffer is an empty non-owning release hook.
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn lock(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let h = hwnd(ctx, object)?;
    if f.fresh {
        return f.call_service(ctx, 1, "SendMessageW", &[h, 0xe, 0, 0]);
    }
    match f.phase(ctx)? {
        1 => {
            let count = ctx
                .cpu
                .read_reg(ArmReg::R0)?
                .checked_add(1)
                .filter(|n| *n <= 0x100001)
                .ok_or_else(|| {
                    KernelError::Dispatch("CEditView text exceeds its supported extent".into())
                })?;
            let mut buffer = word(ctx, object + 0x58)?;
            if count > word(ctx, object + 0x5c)? {
                let replacement = alloc_cpp(ctx, count * 2)?;
                if buffer != 0 {
                    ctx.kernel.heap.free(buffer);
                }
                put(ctx, object + 0x58, replacement)?;
                put(ctx, object + 0x5c, count)?;
                buffer = replacement;
            }
            f.call_service(ctx, 2, "SendMessageW", &[h, 0xd, count, buffer])
        }
        2 => f.call_service(ctx, 3, "SendMessageW", &[h, 0xb9, 0, 0]),
        3 => {
            let buffer = word(ctx, object + 0x58)?;
            f.finish(ctx, buffer)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid CEditView buffer continuation".into(),
        )),
    }
}
fn length(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    send(ctx, 0xe, 0, 0)
}
fn selected(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let h = hwnd(ctx, object)?;
        return f.call_service(
            ctx,
            1,
            "SendMessageW",
            &[h, 0xb0, f.address + 128, f.address + 132],
        );
    }
    if f.phase(ctx)? == 1 {
        return f.call_service(ctx, 2, LOCK, &[object]);
    }
    let buffer = ctx.cpu.read_reg(ArmReg::R0)?;
    let a = word(ctx, f.address + 128)?;
    let b = word(ctx, f.address + 132)?;
    let text = strings::wide(ctx, buffer)?;
    let a = (a as usize).min(text.len());
    let b = (b as usize).min(text.len());
    let output = f.arg(ctx, 1)?;
    strings::replace(ctx, output, &text[a.min(b)..a.max(b)])?;
    f.finish(ctx, 0)
}
fn same_selected(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let input = ctx.arg_u32(1)?;
    let sensitive = ctx.arg_u32(2)? != 0;
    let h = hwnd(ctx, object)?;
    let child = ctx
        .kernel
        .controls
        .get(h)
        .filter(|c| c.class == pocket_kernel::controls::ControlClass::Edit)
        .ok_or_else(|| KernelError::Dispatch("CEditView has no EDIT owner".into()))?;
    let (a, b) = child.edit.range(&child.text);
    let selected: Vec<_> = child
        .text
        .encode_utf16()
        .skip(a as usize)
        .take((b - a) as usize)
        .collect();
    let input = strings::wide(ctx, input)?;
    let equal = selected.len() == input.len()
        && selected.iter().zip(&input).all(|(a, b)| {
            if sensitive {
                *a == *b
            } else {
                folded(*a) == folded(*b)
            }
        });
    Ok(DispatchOutcome::ReturnedR0(u32::from(equal)))
}
fn set_tabs(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let h = hwnd(ctx, object)?;
    if f.fresh {
        let stops = f.arg(ctx, 1)?;
        put(ctx, object + 0x54, stops)?;
        return f.call_service(ctx, 1, "SendMessageW", &[h, 0xcb, 1, object + 0x54]);
    }
    if f.phase(ctx)? == 1 {
        return f.call_service(ctx, 2, "InvalidateRect", &[h, 0, 1]);
    }
    f.finish(ctx, 0)
}
fn changed(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        let doc = word(ctx, object + 0x48)?;
        if doc != 0 {
            return f.call_virtual(ctx, 1, doc, 24, &[1]);
        }
    }
    f.finish(ctx, 0)
}
fn update(ctx: &mut CallCtx<'_>, message: u32) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        let h = hwnd(ctx, object)?;
        return f.call_service(ctx, 1, "SendMessageW", &[h, message, 0, 0]);
    }
    if f.phase(ctx)? == 1 {
        let value = ctx.cpu.read_reg(ArmReg::R0)?;
        let ui = f.arg(ctx, 1)?;
        let enabled = if message == 0xb0 {
            u32::from(value & 0xffff != value >> 16)
        } else {
            u32::from(value != 0)
        };
        return f.call_virtual(ctx, 2, ui, 0, &[enabled]);
    }
    f.finish(ctx, 0)
}
fn update_selection(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    update(ctx, 0xb0)
}
fn update_text(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    update(ctx, 0xe)
}
fn update_undo(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    update(ctx, 0xc6)
}
fn write_archive(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let stream = f.arg(ctx, 1)?;
    if f.fresh {
        return f.call_service(ctx, 1, LOCK, &[object]);
    }
    if f.phase(ctx)? == 1 {
        let buffer = ctx.cpu.read_reg(ArmReg::R0)?;
        let text = strings::wide(ctx, buffer)?;
        return f.call_service(
            ctx,
            2,
            archive::WRITE,
            &[stream, buffer, text.len() as u32 * 2],
        );
    }
    f.finish(ctx, 0)
}
fn read_archive(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let stream = f.arg(ctx, 1)?;
    if f.fresh {
        let count = f.arg(ctx, 2)?;
        if count > 0x100000 {
            return Err(archive::fault(ctx, stream, 5));
        }
        let buffer = alloc(ctx, (count + 1) * 2)?;
        f.own(ctx, buffer)?;
        f.set(ctx, 0, buffer)?;
        ctx.cpu.write_mem(buffer + count * 2, &[0, 0])?;
        return f.call_service(ctx, 1, archive::READ, &[stream, buffer, count * 2]);
    }
    if f.phase(ctx)? == 1 {
        let count = f.arg(ctx, 2)?;
        if ctx.cpu.read_reg(ArmReg::R0)? != count * 2 {
            return Err(archive::fault(ctx, stream, 3));
        }
        let buffer = f.get(ctx, 0)?;
        let h = hwnd(ctx, object)?;
        return f.call_service(ctx, 2, "SendMessageW", &[h, 0xc, 0, buffer]);
    }
    if ctx.cpu.read_reg(ArmReg::R0)? == 0 {
        return Err(io_exceptions::fault(io_exceptions::FILE, 9, 0, Vec::new()));
    }
    let buffer = word(ctx, object + 0x58)?;
    if buffer != 0 {
        ctx.kernel.heap.free(buffer);
    }
    put(ctx, object + 0x58, 0)?;
    put(ctx, object + 0x5c, 0)?;
    f.finish(ctx, 0)
}
fn serialize(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let stream = f.arg(ctx, 1)?;
    let loading = archive::loading(ctx, stream)?;
    if f.fresh {
        if loading {
            return f.call_service(ctx, 1, archive::READ, &[stream, f.address + 128, 4]);
        }
        let h = hwnd(ctx, object)?;
        return f.call_service(ctx, 1, "SendMessageW", &[h, 0xe, 0, 0]);
    }
    if f.phase(ctx)? == 1 {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        if loading {
            if result != 4 {
                return Err(archive::fault(ctx, stream, 3));
            }
            let count = word(ctx, f.address + 128)?;
            return f.call_service(ctx, 3, READ, &[object, stream, count]);
        }
        put(ctx, f.address + 128, result)?;
        return f.call_service(ctx, 2, archive::WRITE, &[stream, f.address + 128, 4]);
    }
    if f.phase(ctx)? == 2 {
        return f.call_service(ctx, 3, WRITE, &[object, stream]);
    }
    f.finish(ctx, 0)
}
fn serialize_raw(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let stream = f.arg(ctx, 1)?;
    let loading = archive::loading(ctx, stream)?;
    if f.fresh {
        if !loading {
            return f.call_service(ctx, 2, WRITE, &[object, stream]);
        }
        let file = word(ctx, stream + 32)?;
        return f.call_virtual(ctx, 1, file, 13, &[]); // CFile::GetLength
    }
    if f.phase(ctx)? == 1 {
        let bytes = ctx.cpu.read_reg(ArmReg::R0)?;
        return f.call_service(ctx, 2, READ, &[object, stream, bytes / 2]);
    }
    f.finish(ctx, 0)
}
fn on_create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let h = hwnd(ctx, object)?;
    if f.fresh {
        let cs = f.arg(ctx, 1)?;
        return f.call_service(
            ctx,
            1,
            "__PocketHLE_MFC_MessageEntry:?OnCreate@CView@@IAAHPAUtagCREATESTRUCTW@@@Z",
            &[object, cs],
        );
    }
    match f.phase(ctx)? {
        1 => {
            if ctx.cpu.read_reg(ArmReg::R0)? == u32::MAX {
                return f.finish(ctx, u32::MAX);
            }
            f.call_service(ctx, 2, "GetStockObject", &[13])
        }
        2 => {
            let font = ctx.cpu.read_reg(ArmReg::R0)?;
            f.call_service(ctx, 3, "SendMessageW", &[h, 0x30, font, 0])
        }
        3 => f.call_service(ctx, 4, "SendMessageW", &[h, 0xc5, 0xfffff, 0]),
        4 => f.call_service(ctx, 5, "SendMessageW", &[h, 0xcb, 1, object + 0x54]),
        _ => f.finish(ctx, 0),
    }
}
fn default_input(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        return f.call_service(ctx, 1, "__PocketHLE_MFC_WindowDefault", &[object]);
    }
    f.finish(ctx, 0)
}
fn dialog_code(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // Retail CE3 returns DLGC_WANTARROWS|WANTALLKEYS|HASSETSEL|WANTCHARS.
    Ok(DispatchOutcome::ReturnedR0(0x8d))
}
fn set_font(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_service(ctx, 1, "__PocketHLE_MFC_WindowDefault", &[object]);
    }
    if f.phase(ctx)? == 1 {
        let h = hwnd(ctx, object)?;
        return f.call_service(ctx, 2, "SendMessageW", &[h, 0xcb, 1, object + 0x54]);
    }
    f.finish(ctx, 0)
}
fn on_destroy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_service(ctx, 1, super::edit_find_dialog::DISMISS, &[object]);
    }
    if f.phase(ctx)? == 1 {
        return f.call_service(
            ctx,
            2,
            "__PocketHLE_MFC_MessageEntry:?OnDestroy@CView@@IAAXXZ",
            &[object],
        );
    }
    f.finish(ctx, 0)
}
fn copy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    send(ctx, 0x301, 0, 0)
}
fn cut(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    send(ctx, 0x300, 0, 0)
}
fn paste(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    send(ctx, 0x302, 0, 0)
}
fn update_clip(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        return f.call_service(ctx, 1, "IsClipboardFormatAvailable", &[13]);
    }
    if f.phase(ctx)? == 1 {
        let value = ctx.cpu.read_reg(ArmReg::R0)?;
        let ui = f.arg(ctx, 1)?;
        return f.call_virtual(ctx, 2, ui, 0, &[value]);
    }
    f.finish(ctx, 0)
}
fn folded(unit: u16) -> u16 {
    let Some(character) = char::from_u32(u32::from(unit)) else {
        return unit;
    };
    let mut lower = character.to_lowercase();
    let first = lower.next().unwrap();
    if lower.next().is_none() && first as u32 <= 0xffff {
        first as u16
    } else {
        unit
    }
}
fn find_text(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let h = hwnd(ctx, object)?;
    if f.fresh {
        let query = f.arg(ctx, 1)?;
        let query = strings::wide(ctx, query)?;
        if query.is_empty() {
            return f.finish(ctx, 0);
        }
        let down = f.arg(ctx, 2)? != 0;
        let sensitive = f.arg(ctx, 3)? != 0;
        let child = ctx
            .kernel
            .controls
            .get(h)
            .filter(|c| c.class == pocket_kernel::controls::ControlClass::Edit)
            .ok_or_else(|| KernelError::Dispatch("CEditView has no EDIT search owner".into()))?;
        let text: Vec<_> = child.text.encode_utf16().collect();
        let (start, end) = child.edit.range(&child.text);
        let matches = |at: usize| {
            text.get(at..at + query.len()).is_some_and(|part| {
                part.iter().zip(&query).all(|(a, b)| {
                    if sensitive {
                        *a == *b
                    } else {
                        folded(*a) == folded(*b)
                    }
                })
            })
        };
        let count = text.len().saturating_sub(query.len());
        let candidates: Box<dyn Iterator<Item = usize>> = if down {
            Box::new((end as usize..=count).chain(0..(end as usize).min(count + 1)))
        } else {
            Box::new(
                (0..(start as usize).min(count + 1))
                    .rev()
                    .chain((start as usize..=count).rev()),
            )
        };
        let Some(index) = candidates.into_iter().find(|at| matches(*at)) else {
            return f.finish(ctx, 0);
        };
        f.set(ctx, 0, index as u32)?;
        return f.call_service(
            ctx,
            1,
            "SendMessageW",
            &[h, 0xb1, index as u32, (index + query.len()) as u32],
        );
    }
    f.finish(ctx, 1)
}
fn find_next(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let text = f.arg(ctx, 1)?;
        let down = f.arg(ctx, 2)?;
        let sensitive = f.arg(ctx, 3)?;
        super::edit_find_dialog::remember(ctx, text, down, sensitive)?;
        let state = super::edit_find_dialog::state(ctx)?;
        let text = word(ctx, state)?;
        f.set(ctx, 0, text)?;
        return f.call_service(
            ctx,
            1,
            "__PocketHLE_MFC_EditFindText",
            &[object, text, down, sensitive],
        );
    }
    if f.phase(ctx)? == 1 && ctx.cpu.read_reg(ArmReg::R0)? == 0 {
        let text = f.get(ctx, 0)?;
        return f.call_virtual(ctx, 2, object, 58, &[text]);
    }
    f.finish(ctx, 0)
}
fn not_found(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let query = f.arg(ctx, 1)?;
        let query = strings::wide(ctx, query)?;
        let message = format!("Cannot find \"{}\".", String::from_utf16_lossy(&query));
        let bytes: Vec<_> = message
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        let buffer = alloc(ctx, bytes.len() as u32)?;
        f.own(ctx, buffer)?;
        ctx.cpu.write_mem(buffer, &bytes)?;
        let object = f.arg(ctx, 0)?;
        let h = hwnd(ctx, object)?;
        return f.call_service(ctx, 1, "MessageBoxW", &[h, buffer, 0, 0x40]);
    }
    f.finish(ctx, 0)
}
fn dismiss_find(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        return f.call_service(ctx, 1, super::edit_find_dialog::DISMISS, &[object]);
    }
    f.finish(ctx, 0)
}
fn initialize_replace(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let state = super::edit_find_dialog::state(ctx)?;
    let query = word(ctx, state)?;
    let down = word(ctx, state + 8)?;
    let sensitive = word(ctx, state + 12)?;
    if f.fresh {
        return f.call_service(
            ctx,
            1,
            "__PocketHLE_MFC_EditSameSelected",
            &[object, query, sensitive],
        );
    }
    match f.phase(ctx)? {
        1 => {
            if ctx.cpu.read_reg(ArmReg::R0)? != 0 {
                return f.finish(ctx, 1);
            }
            f.call_service(
                ctx,
                2,
                "__PocketHLE_MFC_EditFindText",
                &[object, query, down, sensitive],
            )
        }
        2 => {
            if ctx.cpu.read_reg(ArmReg::R0)? == 0 {
                return f.call_virtual(ctx, 3, object, 58, &[query]);
            }
            f.finish(ctx, 0)
        }
        _ => f.finish(ctx, 0),
    }
}
fn replace_selection(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let state = super::edit_find_dialog::state(ctx)?;
    if f.fresh {
        let query = f.arg(ctx, 1)?;
        let down = f.arg(ctx, 2)?;
        let sensitive = f.arg(ctx, 3)?;
        let replacement = ctx.arg_u32(4)?;
        let replacement = strings::wide(ctx, replacement)?;
        super::edit_find_dialog::remember(ctx, query, down, sensitive)?;
        strings::replace(ctx, state + 4, &replacement)?;
        return f.call_service(ctx, 1, "__PocketHLE_MFC_EditInitializeReplace", &[object]);
    }
    if f.phase(ctx)? == 1 {
        if ctx.cpu.read_reg(ArmReg::R0)? == 0 {
            return f.finish(ctx, 0);
        }
        let replacement = word(ctx, state + 4)?;
        let h = hwnd(ctx, object)?;
        return f.call_service(ctx, 2, "SendMessageW", &[h, 0xc2, 1, replacement]);
    }
    if f.phase(ctx)? == 2 {
        let query = word(ctx, state)?;
        let down = word(ctx, state + 8)?;
        let sensitive = word(ctx, state + 12)?;
        return f.call_virtual(ctx, 3, object, 55, &[query, down, sensitive]);
    }
    f.finish(ctx, 0)
}
fn replace_all(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let h = hwnd(ctx, object)?;
    if f.fresh {
        let query = f.arg(ctx, 1)?;
        let replacement = f.arg(ctx, 2)?;
        let sensitive = f.arg(ctx, 3)? != 0;
        let query = strings::wide(ctx, query)?;
        let replacement = strings::wide(ctx, replacement)?;
        if query.is_empty() {
            return f.finish(ctx, 0);
        }
        let child = ctx
            .kernel
            .controls
            .get(h)
            .filter(|c| c.class == pocket_kernel::controls::ControlClass::Edit)
            .ok_or_else(|| KernelError::Dispatch("CEditView has no replacement owner".into()))?;
        let source: Vec<_> = child.text.encode_utf16().collect();
        let limit = child.edit.limit.min(0xfffff) as usize;
        let mut output = Vec::new();
        let mut index = 0;
        let mut count = 0;
        while index < source.len() {
            let matched = source.get(index..index + query.len()).is_some_and(|part| {
                part.iter().zip(&query).all(|(a, b)| {
                    if sensitive {
                        *a == *b
                    } else {
                        folded(*a) == folded(*b)
                    }
                })
            });
            let added = if matched { replacement.len() } else { 1 };
            if output
                .len()
                .checked_add(added)
                .is_none_or(|size| size > limit)
            {
                return f.call_service(ctx, 99, exception_throw::MEMORY, &[]);
            }
            if matched {
                output.extend_from_slice(&replacement);
                index += query.len();
                count += 1;
            } else {
                output.push(source[index]);
                index += 1;
            }
        }
        if count == 0 {
            let query = f.arg(ctx, 1)?;
            return f.call_virtual(ctx, 3, object, 58, &[query]);
        }
        let bytes: Vec<_> = output
            .into_iter()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        let pointer = alloc(ctx, bytes.len() as u32)?;
        f.own(ctx, pointer)?;
        ctx.cpu.write_mem(pointer, &bytes)?;
        f.set(ctx, 0, pointer)?;
        return f.call_service(ctx, 1, "SendMessageW", &[h, 0xb1, 0, u32::MAX]);
    }
    if f.phase(ctx)? == 1 {
        let pointer = f.get(ctx, 0)?;
        return f.call_service(ctx, 2, "SendMessageW", &[h, 0xc2, 1, pointer]);
    }
    f.finish(ctx, 0)
}
fn find_replace_command(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if !f.fresh {
        return f.finish(ctx, 0);
    }
    let pointer = f.arg(ctx, 2)?;
    ctx.cpu
        .check_guest_access(pointer, 40, pocket_cpu::Prot::READ)?;
    if word(ctx, pointer)? != 40 {
        return Err(KernelError::Dispatch(
            "Invalid WinCE FINDREPLACE structure".into(),
        ));
    }
    let flags = word(ctx, pointer + 12)?;
    if flags & 0x40 != 0 {
        return f.call_service(ctx, 1, super::edit_find_dialog::DISMISS, &[object]);
    }
    let query = word(ctx, pointer + 16)?;
    let replace = word(ctx, pointer + 20)?;
    let down = flags & 1;
    let sensitive = (flags >> 2) & 1;
    if flags & 0x20 != 0 {
        f.call_virtual(ctx, 1, object, 57, &[query, replace, sensitive])
    } else if flags & 0x10 != 0 {
        f.call_virtual(ctx, 1, object, 56, &[query, down, sensitive, replace])
    } else if flags & 8 != 0 {
        f.call_virtual(ctx, 1, object, 55, &[query, down, sensitive])
    } else {
        f.finish(ctx, 0)
    }
}
fn calc_rect(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let h = hwnd(ctx, object)?;
    if f.fresh {
        return f.call_service(ctx, 1, "GetWindowLongW", &[h, (-16i32) as u32]);
    }
    match f.phase(ctx)? {
        1 => {
            let style = ctx.cpu.read_reg(ArmReg::R0)? & !0x300000;
            f.set(ctx, 0, style)?;
            f.call_service(ctx, 2, "GetWindowLongW", &[h, (-20i32) as u32])
        }
        2 => {
            let extended = ctx.cpu.read_reg(ArmReg::R0)? & !0x200;
            let style = f.get(ctx, 0)?;
            let rect = f.arg(ctx, 1)?;
            f.call_service(ctx, 3, "AdjustWindowRectEx", &[rect, style, 0, extended])
        }
        _ => f.finish(ctx, 0),
    }
}

fn update_find(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let state = super::edit_find_dialog::state(ctx)?;
        let query = strings::value(ctx, state)?;
        if query.is_empty() {
            let ui = f.arg(ctx, 1)?;
            return f.call_virtual(ctx, 2, ui, 0, &[0]);
        }
        let object = f.arg(ctx, 0)?;
        let window = hwnd(ctx, object)?;
        return f.call_service(ctx, 1, "SendMessageW", &[window, 0xe, 0, 0]);
    }
    if f.phase(ctx)? == 1 {
        let enabled = u32::from(ctx.cpu.read_reg(ArmReg::R0)? != 0);
        let ui = f.arg(ctx, 1)?;
        return f.call_virtual(ctx, 2, ui, 0, &[enabled]);
    }
    f.finish(ctx, 0)
}
