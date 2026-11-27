//! Filename selection and template confidence use the shared WinCE modal services.
use super::callbacks::{virtual_function, Frame, ScopedString};
use super::*;
use pocket_cpu::regs::ArmReg;
const BUFFER: u32 = 65536;

pub(super) fn open(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let manager = f.arg(ctx, 0)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        let path = f.arg(ctx, 1)?;
        let path = String::from_utf16_lossy(&strings::wide(ctx, path)?);
        let path = ctx.kernel.vfs.full_guest_path(path.trim_matches('"'));
        if path.encode_utf16().count() >= 260 {
            return Err(super::io_exceptions::fault(
                super::io_exceptions::FILE,
                3,
                206,
                path.encode_utf16().collect(),
            ));
        }
        let p = super::application::duplicate_wide(ctx, &path.encode_utf16().collect::<Vec<_>>())?;
        f.own(ctx, p)?;
        f.set(ctx, 4, p)?;
        return f.call_virtual(ctx, 1, manager, 6, &[]);
    }
    match f.phase(ctx)? {
        1 => put(ctx, f.address + 128, result)?,
        2 => {
            f.set(ctx, 5, result)?;
            put(ctx, f.address + 132, 0)?;
            let path = f.get(ctx, 4)?;
            return f.call_virtual(ctx, 3, result, 27, &[path, f.address + 132]);
        }
        3 => {
            if result > f.get(ctx, 1)? {
                f.set(ctx, 1, result)?;
                let template = f.get(ctx, 5)?;
                f.set(ctx, 0, template)?;
                let doc = word(ctx, f.address + 132)?;
                f.set(ctx, 2, doc)?;
            }
            if result == 5 {
                let doc = word(ctx, f.address + 132)?;
                let target = virtual_function(ctx, doc, 25)?;
                return f.call_args(ctx, 6, target, &[doc]);
            }
        }
        4 => return f.finish(ctx, result),
        6 => {
            let doc = f.get(ctx, 2)?;
            if result == 0 {
                return f.finish(ctx, doc);
            }
            put(ctx, f.address + 136, result)?;
            let target = virtual_function(ctx, doc, 26)?;
            return f.call_args(ctx, 7, target, &[doc, f.address + 136]);
        }
        7 => {
            let class = super::classes::runtime(ctx, "CView")?;
            f.set(ctx, 6, result)?;
            return f.call_service(ctx, 8, super::class_queries::KIND, &[result, class]);
        }
        8 => {
            let doc = f.get(ctx, 2)?;
            let view = f.get(ctx, 6)?;
            if result == 0 {
                return f.finish(ctx, doc);
            }
            return f.call_service(ctx, 9, super::hierarchy::PARENT, &[view]);
        }
        9 => {
            if result == 0 {
                let doc = f.get(ctx, 2)?;
                return f.finish(ctx, doc);
            }
            return f.call_virtual(ctx, 10, result, 51, &[u32::MAX]);
        }
        10 => {
            let doc = f.get(ctx, 2)?;
            return f.finish(ctx, doc);
        }
        _ => {
            return Err(KernelError::Dispatch(
                "Invalid document-manager open continuation".into(),
            ))
        }
    }
    if word(ctx, f.address + 128)? != 0 {
        let n = f.get(ctx, 3)?;
        if n >= 4096 {
            return Err(KernelError::Dispatch(
                "MFC template traversal exceeded its bound".into(),
            ));
        }
        f.set(ctx, 3, n + 1)?;
        return f.call_virtual(ctx, 2, manager, 7, &[f.address + 128]);
    }
    let template = f.get(ctx, 0)?;
    if template == 0 {
        return f.finish(ctx, 0);
    }
    let path = f.get(ctx, 4)?;
    f.call_virtual(ctx, 4, template, 33, &[path, 1])
}
pub(super) fn new(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let manager = f.arg(ctx, 0)?;
    if f.fresh {
        let node = super::lists::next_position(ctx, manager + 4, 0)?;
        if node == 0 {
            return f.finish(ctx, 0);
        }
        let after = super::lists::next_position(ctx, manager + 4, node)?;
        if after == 0 {
            let template = word(ctx, node + 8)?;
            return f.call_virtual(ctx, 2, template, 33, &[0, 1]);
        }
        return f.call_service(ctx, 1, super::document_choice::CHOOSE, &[manager]);
    }
    if f.phase(ctx)? == 1 {
        let template = ctx.cpu.read_reg(ArmReg::R0)?;
        if template == 0 {
            return f.finish(ctx, 0);
        }
        return f.call_virtual(ctx, 2, template, 33, &[0, 1]);
    }
    f.finish(ctx, 0)
}
pub(super) fn open_prompt(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let manager = f.arg(ctx, 0)?;
    let text = ScopedString::at(&f, 128, 192);
    if f.fresh {
        text.begin(ctx, &f)?;
        return f.call_virtual(ctx, 1, manager, 12, &[text.cell, 0xf100, 0x1000, 1, 0]);
    }
    if let Some(outcome) = text.rethrow(ctx, &f)? {
        return Ok(outcome);
    }
    if f.phase(ctx)? == 1 && ctx.cpu.read_reg(ArmReg::R0)? != 0 {
        let p = word(ctx, text.cell)?;
        return f.call_virtual(ctx, 2, manager, 8, &[p]);
    }
    text.end(ctx)?;
    f.finish(ctx, 0)
}
fn append(ctx: &mut CallCtx<'_>, f: &Frame, text: &[u16]) -> Result<u32, KernelError> {
    let p = f.get(ctx, 0)?;
    let off = f.get(ctx, 2)?;
    let size = (text.len() as u32 + 1)
        .checked_mul(2)
        .ok_or_else(|| KernelError::Dispatch("MFC filter length overflow".into()))?;
    if off.checked_add(size).is_none_or(|end| end > BUFFER) {
        return Err(KernelError::Dispatch(
            "MFC filename filter exceeds its bound".into(),
        ));
    }
    let bytes: Vec<_> = text
        .iter()
        .copied()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    ctx.cpu.write_mem(p + off, &bytes)?;
    f.set(ctx, 2, off + size)?;
    Ok(p + off)
}
fn next_filter(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    text: &ScopedString,
) -> Result<DispatchOutcome, KernelError> {
    let specific = f.get(ctx, 5)?;
    let template = if f.fresh && specific != 0 {
        specific
    } else if specific != 0 {
        0
    } else {
        let node = f.get(ctx, 1)?;
        if node == 0 {
            0
        } else {
            let manager = f.arg(ctx, 0)?;
            let next = super::lists::next_position(ctx, manager + 4, node)?;
            f.set(ctx, 1, next)?;
            word(ctx, node + 8)?
        }
    };
    if template == 0 {
        return show_prompt(ctx, f);
    }
    f.set(ctx, 3, template)?;
    f.call_virtual(ctx, 1, template, 26, &[text.cell, 3])
}
pub(super) fn prompt(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let text = ScopedString::at(&f, 128, 192);
    if f.fresh {
        let manager = f.arg(ctx, 0)?;
        let buffer = alloc(ctx, BUFFER)?;
        f.own(ctx, buffer)?;
        ctx.cpu.write_mem(buffer, &vec![0; BUFFER as usize])?;
        f.set(ctx, 0, buffer)?;
        f.set(ctx, 2, 2048)?;
        let node = super::lists::next_position(ctx, manager + 4, 0)?;
        f.set(ctx, 1, node)?;
        // Stack arguments are not re-read after a callback: preserve their values.
        let open = ctx.arg_u32(4)?;
        f.set(ctx, 4, open)?;
        let specific = ctx.arg_u32(5)?;
        f.set(ctx, 5, specific)?;
        text.begin(ctx, &f)?;
        let specific = f.get(ctx, 5)?;
        if specific != 0 {
            f.set(ctx, 3, specific)?;
            return f.call_virtual(ctx, 1, specific, 26, &[text.cell, 3]);
        }
        return next_filter(ctx, f, &text);
    }
    if let Some(outcome) = text.rethrow(ctx, &f)? {
        return Ok(outcome);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            let label = strings::value(ctx, text.cell)?;
            let label = if label.is_empty() {
                "Files".encode_utf16().collect()
            } else {
                label
            };
            append(ctx, &f, &label)?;
            let template = f.get(ctx, 3)?;
            f.call_virtual(ctx, 2, template, 26, &[text.cell, 4])
        }
        2 => {
            let extension = strings::value(ctx, text.cell)?;
            let ext = String::from_utf16_lossy(&extension);
            let pattern = if result == 0 || ext.is_empty() {
                "*.*".into()
            } else {
                ext.split(';')
                    .map(|e| format!("*{}", e.trim()))
                    .collect::<Vec<_>>()
                    .join(";")
            };
            append(ctx, &f, &pattern.encode_utf16().collect::<Vec<_>>())?;
            if f.get(ctx, 5)? != 0 {
                return show_prompt(ctx, f);
            }
            next_filter(ctx, f, &text)
        }
        3 => {
            if result != 0 {
                let p = f.get(ctx, 0)?;
                let name = strings::wide(ctx, p + 80)?;
                let out = f.arg(ctx, 1)?;
                strings::replace(ctx, out, &name)?;
            }
            text.end(ctx)?;
            f.finish(ctx, u32::from(result != 0))
        }
        _ => Err(KernelError::Dispatch(
            "Invalid document filename dialog continuation".into(),
        )),
    }
}
fn show_prompt(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    append(
        ctx,
        &f,
        &"All Files (*.*)".encode_utf16().collect::<Vec<_>>(),
    )?;
    append(ctx, &f, &"*.*".encode_utf16().collect::<Vec<_>>())?;
    append(ctx, &f, &[])?;
    let buffer = f.get(ctx, 0)?;
    let out = f.arg(ctx, 1)?;
    let file = strings::value(ctx, out)?;
    if file.len() >= 260 {
        return Err(KernelError::Dispatch(
            "MFC dialog initial filename exceeds MAX_PATH".into(),
        ));
    }
    let bytes: Vec<_> = file
        .iter()
        .copied()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    ctx.cpu.write_mem(buffer + 80, &bytes)?;
    let title_id = f.arg(ctx, 2)?;
    let mut title = super::resources::string(ctx, title_id)?;
    if title.is_empty() {
        title = if f.get(ctx, 4)? != 0 {
            "Open"
        } else {
            "Save As"
        }
        .encode_utf16()
        .collect();
    }
    let title: Vec<_> = title
        .into_iter()
        .take(255)
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    ctx.cpu.write_mem(buffer + 608, &title)?;
    let owner = ctx.kernel.active_window;
    let flags = f.arg(ctx, 3)?;
    for (off, v) in [
        (0, 76),
        (4, owner),
        (12, buffer + 2048),
        (24, 1),
        (28, buffer + 80),
        (32, 260),
        (48, buffer + 608),
        (52, flags),
    ] {
        put(ctx, buffer + off, v)?;
    }
    let name = if f.get(ctx, 4)? != 0 {
        "GetOpenFileNameW"
    } else {
        "GetSaveFileNameW"
    };
    f.call_service(ctx, 3, name, &[buffer])
}
