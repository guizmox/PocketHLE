//! Runtime-owned New Document dialog over the shared WinCE modal/control engine.
use super::callbacks::{virtual_function, Frame, ScopedString};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const CHOOSE: &str = "__PocketHLE_MFC_ChooseTemplate";
const PROC: &str = "__PocketHLE_MFC_ChooseTemplateProc";
const MAX: u32 = 256;
const BYTES: u32 = 65536;
const DIALOG: u32 = 4 + MAX * 8;
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", CHOOSE, choose);
    d.register_handler("coredll.dll", PROC, procedure);
}
fn put_text(ctx: &mut CallCtx<'_>, p: u32, text: &[u16]) -> Result<(), KernelError> {
    let bytes: Vec<_> = text
        .iter()
        .copied()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    Ok(ctx.cpu.write_mem(p, &bytes)?)
}
fn choose(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let manager = f.arg(ctx, 0)?;
    let temp = ScopedString::at(&f, 128, 192);
    if f.fresh {
        let p = alloc(ctx, BYTES)?;
        f.own(ctx, p)?;
        ctx.cpu.write_mem(p, &vec![0; BYTES as usize])?;
        f.set(ctx, 0, p)?;
        f.set(ctx, 3, 4096)?;
        temp.begin(ctx, &f)?;
        let node = super::lists::next_position(ctx, manager + 4, 0)?;
        f.set(ctx, 1, node)?;
    } else {
        if let Some(outcome) = temp.rethrow(ctx, &f)? {
            return Ok(outcome);
        }
        if f.phase(ctx)? == 2 {
            let result = ctx.cpu.read_reg(ArmReg::R0)?;
            temp.end(ctx)?;
            return f.finish(ctx, result);
        }
        let mut text = strings::value(ctx, temp.cell)?;
        if text.is_empty() {
            text = super::resources::framework_string(ctx, 0xf003)?;
        }
        let p = f.get(ctx, 0)?;
        let count = f.get(ctx, 2)?;
        let offset = f.get(ctx, 3)?;
        let bytes = (text.len() as u32 + 1) * 2;
        if count >= MAX || offset.checked_add(bytes).is_none_or(|end| end > BYTES) {
            return Err(KernelError::Dispatch(
                "MFC template selection exceeds its bound".into(),
            ));
        }
        put_text(ctx, p + offset, &text)?;
        let template = f.get(ctx, 4)?;
        put(ctx, p + 4 + count * 8, template)?;
        put(ctx, p + 8 + count * 8, p + offset)?;
        f.set(ctx, 2, count + 1)?;
        f.set(ctx, 3, offset + bytes)?;
    }
    let node = f.get(ctx, 1)?;
    if node != 0 {
        let next = super::lists::next_position(ctx, manager + 4, node)?;
        f.set(ctx, 1, next)?;
        let template = word(ctx, node + 8)?;
        f.set(ctx, 4, template)?;
        let target = virtual_function(ctx, template, 26)?;
        return f.call_args(ctx, 1, target, &[template, temp.cell, 2]);
    }
    let p = f.get(ctx, 0)?;
    let count = f.get(ctx, 2)?;
    put(ctx, p, count)?;
    if count == 0 {
        temp.end(ctx)?;
        return f.finish(ctx, 0);
    }
    let data = dialog_template();
    ctx.cpu.write_mem(p + DIALOG, &data)?;
    let proc = super::callbacks::service(ctx, PROC)?;
    let parent = ctx.kernel.active_window;
    f.call_service(
        ctx,
        2,
        "DialogBoxIndirectParamW",
        &[0, p + DIALOG, parent, proc, p],
    )
}
fn dialog_template() -> Vec<u8> {
    fn u16b(v: &mut Vec<u8>, n: u16) {
        v.extend_from_slice(&n.to_le_bytes());
    }
    fn u32b(v: &mut Vec<u8>, n: u32) {
        v.extend_from_slice(&n.to_le_bytes());
    }
    fn text(v: &mut Vec<u8>, s: &str) {
        for c in s.encode_utf16().chain(Some(0)) {
            u16b(v, c);
        }
    }
    let mut v = Vec::new();
    u32b(&mut v, 0x80c800c0);
    u32b(&mut v, 0);
    u16b(&mut v, 3);
    for n in [0, 0, 108, 112, 0, 0] {
        u16b(&mut v, n);
    }
    text(&mut v, "New Document");
    u16b(&mut v, 8);
    text(&mut v, "Tahoma");
    for (style, rect, id, class, label) in [
        (0x50a10001, [5, 5, 98, 78], 100, 0x83, ""),
        (0x50010001, [5, 90, 44, 14], 1, 0x80, "OK"),
        (0x50010000, [59, 90, 44, 14], 2, 0x80, "Cancel"),
    ] {
        while v.len() % 4 != 0 {
            v.push(0);
        }
        u32b(&mut v, style);
        u32b(&mut v, 0);
        for n in rect {
            u16b(&mut v, n);
        }
        u16b(&mut v, id);
        u16b(&mut v, 0xffff);
        u16b(&mut v, class);
        text(&mut v, label);
        u16b(&mut v, 0);
    }
    v
}
fn add_label(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let context = f.get(ctx, 1)?;
    let index = f.get(ctx, 2)?;
    let list = f.get(ctx, 0)?;
    if index >= word(ctx, context)? {
        return f.call_service(ctx, 4, "SendMessageW", &[list, 0x186, 0, 0]);
    }
    let label = word(ctx, context + 8 + index * 8)?;
    f.call_service(ctx, 3, "SendMessageW", &[list, 0x180, 0, label])
}
fn procedure(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let hwnd = f.arg(ctx, 0)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        let message = f.arg(ctx, 1)?;
        let command = f.arg(ctx, 2)?;
        if message == 0x110 {
            let context = f.arg(ctx, 3)?;
            f.set(ctx, 1, context)?;
            return f.call_service(ctx, 1, "SetWindowLongW", &[hwnd, (-21i32) as u32, context]);
        }
        if message == 0x10 || message == 0x111 && (command & 0xffff) == 2 {
            return f.call_service(ctx, 13, "EndDialog", &[hwnd, 0]);
        }
        if message == 0x111
            && ((command & 0xffff) == 1 || command & 0xffff == 100 && command >> 16 == 2)
        {
            return f.call_service(ctx, 10, "GetWindowLongW", &[hwnd, (-21i32) as u32]);
        }
        return f.finish(ctx, 0);
    }
    match f.phase(ctx)? {
        1 => f.call_service(ctx, 2, "GetDlgItem", &[hwnd, 100]),
        2 => {
            f.set(ctx, 0, result)?;
            add_label(ctx, f)
        }
        3 => {
            if result == u32::MAX || result == u32::MAX - 1 {
                return Err(KernelError::Dispatch(
                    "MFC new-document list insertion failed".into(),
                ));
            }
            let next = f.get(ctx, 2)? + 1;
            f.set(ctx, 2, next)?;
            add_label(ctx, f)
        }
        4 => {
            let list = f.get(ctx, 0)?;
            f.call_service(ctx, 5, "SetFocus", &[list])
        }
        5 => f.finish(ctx, 0),
        10 => {
            f.set(ctx, 1, result)?;
            f.call_service(ctx, 11, "GetDlgItem", &[hwnd, 100])
        }
        11 => f.call_service(ctx, 12, "SendMessageW", &[result, 0x188, 0, 0]),
        12 => {
            let context = f.get(ctx, 1)?;
            if result >= word(ctx, context)? {
                return f.finish(ctx, 1);
            }
            let template = word(ctx, context + 4 + result * 8)?;
            f.call_service(ctx, 13, "EndDialog", &[hwnd, template])
        }
        13 => f.finish(ctx, 1),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC template dialog continuation".into(),
        )),
    }
}
