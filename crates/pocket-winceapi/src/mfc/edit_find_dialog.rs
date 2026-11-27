//! Modeless editor search UI uses ordinary WinCE dialogs and controls. The
//! per-thread search owner lives in guest memory and the existing export roots.
use super::callbacks::Frame;
use super::*;
use crate::dlgtemplate::{class_ordinal, DialogItem, DialogTemplate, ItemClass};
use pocket_cpu::regs::ArmReg;
const PROCEDURE: &str = "__PocketHLE_MFC_EditFindDialogProc";
const SHOW: &str = "__PocketHLE_MFC_EditFindDialog";
pub(super) const CLEAR: &str = "__PocketHLE_MFC_EditFindClear";
const ROOT: &str = "__PocketHLE_MFC_EditFindState:";
pub(super) const DISMISS: &str = "__PocketHLE_MFC_EditFindDismiss";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1884, "?OnEditFind@CEditView@@IAAXXZ", find),
    (1888, "?OnEditReplace@CEditView@@IAAXXZ", replace),
    (1885, "?OnEditFindReplace@CEditView@@IAAXH@Z", show),
    (1887, "?OnEditRepeat@CEditView@@IAAXXZ", repeat),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    for (name, handler) in [
        (PROCEDURE, procedure as crate::Handler),
        (SHOW, show),
        (DISMISS, dismiss),
        (CLEAR, release),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}
pub(super) fn state(ctx: &mut CallCtx<'_>) -> Result<u32, KernelError> {
    let key = key(ctx);
    let (pointer, fresh) = application::allocation(ctx, &key, 32)?;
    if fresh {
        let mut initialized = 0;
        let result = (|| {
            for offset in [0, 4] {
                new_string(ctx, pointer + offset, &[])?;
                initialized += 1;
            }
            put(ctx, pointer + 8, 1)?;
            Ok::<_, KernelError>(())
        })();
        if let Err(error) = result {
            for i in 0..initialized {
                release_string(ctx, pointer + i * 4)?;
            }
            let owner = application::owner(ctx)?;
            ctx.kernel
                .dynamic_exports
                .get_mut(&owner)
                .unwrap()
                .remove(&key);
            ctx.kernel.heap.free(pointer);
            return Err(error);
        }
    }
    Ok(pointer)
}
pub(super) fn remember(
    ctx: &mut CallCtx<'_>,
    text: u32,
    down: u32,
    sensitive: u32,
) -> Result<(), KernelError> {
    let text = strings::wide(ctx, text)?;
    let owner = state(ctx)?;
    strings::replace(ctx, owner, &text)?;
    put(ctx, owner + 8, u32::from(down != 0))?;
    put(ctx, owner + 12, u32::from(sensitive != 0))
}
fn template(find_only: bool) -> DialogTemplate {
    let mut items = Vec::new();
    let mut add = |id, class, x, y, cx, cy, title: &str, style: u32| {
        items.push(DialogItem {
            creation_data: Vec::new(),
            style: 0x50000000 | style,
            ex_style: 0,
            x,
            y,
            cx,
            cy,
            id,
            class: ItemClass::Ordinal(class),
            title_resource: None,
            title: title.into(),
        })
    };
    add(1010, class_ordinal::STATIC, 4, 4, 30, 12, "Find:", 0);
    add(1001, class_ordinal::EDIT, 36, 4, 90, 12, "", 0x810080);
    if !find_only {
        add(1011, class_ordinal::STATIC, 4, 20, 30, 12, "Replace:", 0);
        add(1002, class_ordinal::EDIT, 36, 20, 90, 12, "", 0x810080);
    }
    let y = if find_only { 20 } else { 36 };
    add(
        1003,
        class_ordinal::BUTTON,
        4,
        y,
        60,
        12,
        "Match case",
        0x10003,
    );
    add(
        1004,
        class_ordinal::BUTTON,
        66,
        y,
        60,
        12,
        "Search down",
        0x10003,
    );
    add(
        1,
        class_ordinal::BUTTON,
        4,
        y + 16,
        38,
        14,
        "Find next",
        0x10001,
    );
    if !find_only {
        add(
            1005,
            class_ordinal::BUTTON,
            44,
            y + 16,
            38,
            14,
            "Replace",
            0x10000,
        );
        add(
            1006,
            class_ordinal::BUTTON,
            86,
            y + 16,
            40,
            14,
            "Replace all",
            0x10000,
        );
    }
    add(
        2,
        class_ordinal::BUTTON,
        86,
        y + 34,
        40,
        14,
        "Done",
        0x10000,
    );
    DialogTemplate {
        style: 0x90c800c0,
        ex_style: 0,
        x: 0,
        y: 0,
        cx: 132,
        cy: y + 52,
        title: if find_only {
            "Find".into()
        } else {
            "Replace".into()
        },
        items,
        font: Some((8, "Tahoma".into())),
        font_weight: 400,
        font_italic: false,
    }
}
fn find(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    open(ctx, true)
}
fn replace(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    open(ctx, false)
}
fn open(ctx: &mut CallCtx<'_>, find_only: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        return f.call_service(ctx, 1, SHOW, &[object, u32::from(find_only)]);
    }
    f.finish(ctx, 0)
}
fn show(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let owner = state(ctx)?;
    let phase = if f.fresh { 0 } else { f.phase(ctx)? };
    if phase == 0 {
        let old = word(ctx, owner + 20)?;
        if old != 0 {
            put(ctx, owner + 20, 0)?;
            return f.call_service(ctx, 1, "DestroyWindow", &[old]);
        }
    }
    if phase <= 1 {
        let find_only = f.arg(ctx, 1)? != 0;
        let data = crate::dlgtemplate::encode(&template(find_only))
            .ok_or_else(|| KernelError::Dispatch("Cannot encode editor search dialog".into()))?;
        let pointer = alloc(ctx, data.len() as u32)?;
        f.own(ctx, pointer)?;
        ctx.cpu.write_mem(pointer, &data)?;
        put(ctx, owner + 16, object)?;
        put(ctx, owner + 24, u32::from(find_only))?;
        let parent = word(ctx, object + 0x20)?;
        let procedure = callbacks::service(ctx, PROCEDURE)?;
        return f.call_service(
            ctx,
            2,
            "CreateDialogIndirectParamW",
            &[
                pocket_kernel::PROCESS_INSTANCE_HANDLE,
                pointer,
                parent,
                procedure,
                owner,
            ],
        );
    }
    let hwnd = ctx.cpu.read_reg(ArmReg::R0)?;
    put(ctx, owner + 20, hwnd)?;
    f.finish(ctx, 0)
}
fn repeat(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let owner = state(ctx)?;
        if strings::value(ctx, owner)?.is_empty() {
            return f.finish(ctx, 0);
        }
        let object = f.arg(ctx, 0)?;
        let text = word(ctx, owner)?;
        let down = word(ctx, owner + 8)?;
        let sensitive = word(ctx, owner + 12)?;
        return f.call_virtual(ctx, 1, object, 55, &[text, down, sensitive]);
    }
    f.finish(ctx, 0)
}
fn dismiss(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let Some(owner) = existing(ctx)? else {
            return f.finish(ctx, 0);
        };
        let hwnd = word(ctx, owner + 20)?;
        if hwnd != 0 {
            return f.call_service(ctx, 1, "DestroyWindow", &[hwnd]);
        }
    }
    f.finish(ctx, 0)
}
fn procedure(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        return f.finish(ctx, 1);
    }
    let hwnd = f.arg(ctx, 0)?;
    let message = ctx.arg_u32(1)?;
    if message == 0x110 {
        let owner = ctx.arg_u32(3)?;
        ctx.kernel.window_userdata.insert(hwnd, owner);
        put(ctx, owner + 20, hwnd)?;
        let find = String::from_utf16_lossy(&strings::value(ctx, owner)?);
        let replacement = String::from_utf16_lossy(&strings::value(ctx, owner + 4)?);
        let flags = [
            (1003, word(ctx, owner + 12)?),
            (1004, word(ctx, owner + 8)?),
        ];
        let children = ctx.kernel.controls.children_of(hwnd);
        for child in children {
            let control = ctx.kernel.controls.get_mut(child).unwrap();
            match control.id {
                1001 => control.text = find.clone(),
                1002 => control.text = replacement.clone(),
                id => {
                    if let Some((_, checked)) = flags.iter().find(|(expected, _)| *expected == id) {
                        control.set_check_state(*checked);
                    }
                }
            }
        }
        return f.finish(ctx, 1);
    }
    let Some(owner) = ctx
        .kernel
        .window_userdata
        .get(&hwnd)
        .copied()
        .filter(|owner| *owner != 0)
    else {
        return f.finish(ctx, 0);
    };
    if matches!(message, 2 | 0x82) {
        if word(ctx, owner + 20)? == hwnd {
            put(ctx, owner + 20, 0)?;
        }
        return f.finish(ctx, 0);
    }
    let command = if message == 0x10 {
        2
    } else if message == 0x111 {
        ctx.arg_u32(2)? & 0xffff
    } else {
        return f.finish(ctx, 0);
    };
    if !matches!(command, 1 | 2 | 1005 | 1006) {
        return f.finish(ctx, 0);
    }
    if command == 2 {
        return f.call_service(ctx, 1, "DestroyWindow", &[hwnd]);
    }
    let text = |ctx: &CallCtx<'_>, id| {
        ctx.kernel
            .controls
            .by_id(hwnd, id)
            .map(|c| c.text.encode_utf16().collect::<Vec<_>>())
            .unwrap_or_default()
    };
    let find = text(ctx, 1001);
    let replace = text(ctx, 1002);
    strings::replace(ctx, owner, &find)?;
    strings::replace(ctx, owner + 4, &replace)?;
    for (id, offset) in [(1003, 12), (1004, 8)] {
        let value = ctx
            .kernel
            .controls
            .by_id(hwnd, id)
            .map(|c| c.check_state())
            .unwrap_or(0);
        put(ctx, owner + offset, value)?;
    }
    let object = word(ctx, owner + 16)?;
    let find = word(ctx, owner)?;
    let replace = word(ctx, owner + 4)?;
    let down = word(ctx, owner + 8)?;
    let sensitive = word(ctx, owner + 12)?;
    match command {
        1 => f.call_virtual(ctx, 1, object, 55, &[find, down, sensitive]),
        1005 => f.call_virtual(ctx, 1, object, 56, &[find, down, sensitive, replace]),
        _ => f.call_virtual(ctx, 1, object, 57, &[find, replace, sensitive]),
    }
}

fn key(ctx: &CallCtx<'_>) -> String {
    format!("{ROOT}{}", ctx.kernel.current_thread)
}
fn existing(ctx: &CallCtx<'_>) -> Result<Option<u32>, KernelError> {
    let owner = application::owner(ctx)?;
    Ok(ctx.kernel.dynamic_exports[&owner].get(&key(ctx)).copied())
}
/// Thread exit closes the ordinary USER window before releasing its borrowed
/// state pointer. CString storage remains owned until destruction has returned.
fn release(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let frame = Frame::enter(ctx)?;
    if frame.fresh {
        let Some(pointer) = existing(ctx)? else {
            return frame.finish(ctx, 0);
        };
        frame.set(ctx, 0, pointer)?;
        let window = word(ctx, pointer + 20)?;
        if window != 0 {
            return frame.call_service(ctx, 1, "DestroyWindow", &[window]);
        }
    }
    let pointer = frame.get(ctx, 0)?;
    if word(ctx, pointer + 20)? != 0 {
        return Err(KernelError::Dispatch(
            "Editor search window survived thread cleanup".into(),
        ));
    }
    release_string(ctx, pointer + 4)?;
    release_string(ctx, pointer)?;
    let owner = application::owner(ctx)?;
    let key = key(ctx);
    ctx.kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .remove(&key);
    ctx.kernel.heap.free(pointer);
    frame.finish(ctx, 0)
}
