//! Document policy delegates saving, titles, views and prompts to real overrides.
use super::callbacks::{Frame, ScopedString};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1934, "?OnIdle@CDocument@@UAAXXZ", empty_idle),
    (1961, "?OnNewDocument@CDocument@@UAAHXZ", new),
    (2392, "?SetPathName@CDocument@@UAAXPBGH@Z", set_path),
    (2243, "?SaveModified@CDocument@@UAAHXZ", save_modified),
    (
        838,
        "?CanCloseFrame@CDocument@@UAAHPAVCFrameWnd@@@Z",
        can_close,
    ),
    (1898, "?OnFileClose@CDocument@@IAAXXZ", close),
    (1906, "?OnFileSave@CDocument@@IAAXXZ", save),
    (1907, "?OnFileSaveAs@CDocument@@IAAXXZ", save_as),
    (1075, "?DoFileSave@CDocument@@UAAHXZ", file_save),
    (1089, "?DoSave@CDocument@@UAAHPBGH@Z", do_save),
    (
        1263,
        "?GetDefaultMenu@CDocument@@UAAPAUHMENU__@@XZ",
        default_menu,
    ),
];
fn new(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let doc = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_virtual(ctx, 1, doc, 23, &[]);
    }
    match f.phase(ctx)? {
        1 => f.call_virtual(ctx, 2, doc, 28, &[]),
        2 => {
            strings::replace(ctx, doc + 0x24, &[])?;
            f.call_virtual(ctx, 3, doc, 24, &[0])
        }
        3 => f.call_virtual(ctx, 4, doc, 25, &[]),
        4 => {
            let position = ctx.cpu.read_reg(ArmReg::R0)?;
            put(ctx, f.address + 128, position)?;
            next_new(ctx, f)
        }
        5 => next_new(ctx, f),
        _ => Err(KernelError::Dispatch(
            "Invalid new-document continuation".into(),
        )),
    }
}
fn next_new(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    if word(ctx, f.address + 128)? == 0 {
        return f.finish(ctx, 1);
    }
    let steps = f.get(ctx, 0)?;
    if steps >= 4096 {
        return Err(KernelError::Dispatch(
            "MFC new-document view traversal exceeded its bound".into(),
        ));
    }
    f.set(ctx, 0, steps + 1)?;
    let doc = f.arg(ctx, 0)?;
    f.call_virtual(ctx, 5, doc, 26, &[f.address + 128])
}
fn set_path(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let doc = f.arg(ctx, 0)?;
    if f.fresh {
        let pointer = f.arg(ctx, 1)?;
        let name = String::from_utf16_lossy(&strings::wide(ctx, pointer)?);
        let path = ctx.kernel.vfs.full_guest_path(&name);
        if path.encode_utf16().count() >= 260 {
            return Err(super::io_exceptions::fault(
                super::io_exceptions::FILE,
                3,
                206,
                path.encode_utf16().collect(),
            ));
        }
        strings::replace(ctx, doc + 0x24, &path.encode_utf16().collect::<Vec<_>>())?;
        put(ctx, doc + 0x50, 0)?;
        let title = path.rsplit('\\').next().unwrap_or("");
        let p = super::application::duplicate_wide(
            ctx,
            &title.encode_utf16().take(63).collect::<Vec<_>>(),
        )?;
        f.own(ctx, p)?;
        return f.call_virtual(ctx, 1, doc, 21, &[p]);
    }
    if f.phase(ctx)? == 1 && f.arg(ctx, 2)? != 0 {
        let module = super::application::state(ctx)?;
        let app = word(ctx, module + 4)?;
        if app != 0 {
            let path = word(ctx, doc + 0x24)?;
            return f.call_virtual(ctx, 2, app, 32, &[path]);
        }
    }
    f.finish(ctx, 0)
}
fn save_modified(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let doc = f.arg(ctx, 0)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        return f.call_virtual(ctx, 1, doc, 23, &[]);
    }
    match f.phase(ctx)? {
        1 => {
            if result == 0 {
                return f.finish(ctx, 1);
            }
            let path = strings::value(ctx, doc + 0x24)?;
            let mut label = if path.is_empty() {
                strings::value(ctx, doc + 0x20)?
            } else {
                path
            };
            if label.is_empty() {
                label = super::resources::framework_string(ctx, 0xf003)?;
            }
            let format = super::resources::framework_string(ctx, 0xf103)?;
            let message =
                String::from_utf16_lossy(&format).replace("%1", &String::from_utf16_lossy(&label));
            let p = super::application::duplicate_wide(
                ctx,
                &message.encode_utf16().collect::<Vec<_>>(),
            )?;
            f.own(ctx, p)?;
            f.call_service(ctx, 2, super::message_box::TEXT, &[p, 3, 0xf103])
        }
        2 => match result {
            6 => f.call_virtual(ctx, 3, doc, 40, &[]),
            7 => f.finish(ctx, 1),
            _ => f.finish(ctx, 0),
        },
        3 => f.finish(ctx, u32::from(result != 0)),
        _ => Err(KernelError::Dispatch(
            "Invalid SaveModified continuation".into(),
        )),
    }
}
fn can_close(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let doc = f.arg(ctx, 0)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        return f.call_virtual(ctx, 1, doc, 25, &[]);
    }
    match f.phase(ctx)? {
        1 => put(ctx, f.address + 128, result)?,
        2 => return f.call_service(ctx, 3, super::hierarchy::PARENT, &[result]),
        3 => {
            if result != 0 && word(ctx, result + 0x58)? as i32 > 0 {
                return f.finish(ctx, 1);
            }
        }
        4 => return f.finish(ctx, u32::from(result != 0)),
        _ => {
            return Err(KernelError::Dispatch(
                "Invalid document frame-close continuation".into(),
            ))
        }
    }
    if word(ctx, f.address + 128)? == 0 {
        return f.call_virtual(ctx, 4, doc, 37, &[]);
    }
    let steps = f.get(ctx, 0)?;
    if steps >= 4096 {
        return Err(KernelError::Dispatch(
            "MFC frame-close view traversal exceeded its bound".into(),
        ));
    }
    f.set(ctx, 0, steps + 1)?;
    f.call_virtual(ctx, 2, doc, 26, &[f.address + 128])
}
fn close(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let doc = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_virtual(ctx, 1, doc, 37, &[]);
    }
    if f.phase(ctx)? == 1 && ctx.cpu.read_reg(ArmReg::R0)? != 0 {
        return f.call_virtual(ctx, 2, doc, 32, &[]);
    }
    f.finish(ctx, 0)
}
fn save(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let doc = f.arg(ctx, 0)?;
        return f.call_virtual(ctx, 1, doc, 40, &[]);
    }
    f.finish(ctx, 0)
}
fn save_as(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let doc = f.arg(ctx, 0)?;
        return f.call_virtual(ctx, 1, doc, 39, &[0, 1]);
    }
    f.finish(ctx, 0)
}
fn file_save(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let doc = f.arg(ctx, 0)?;
    if f.fresh {
        let text = strings::value(ctx, doc + 0x24)?;
        let path = String::from_utf16_lossy(&text);
        let readonly = ctx.kernel.vfs.attributes(&path).is_ok_and(|a| a & 1 != 0);
        let pointer = if text.is_empty() || readonly {
            0
        } else {
            word(ctx, doc + 0x24)?
        };
        return f.call_virtual(ctx, 1, doc, 39, &[pointer, 1]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    f.finish(ctx, u32::from(result != 0))
}
fn do_save(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let doc = f.arg(ctx, 0)?;
    let text = ScopedString::at(&f, 128, 192);
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        text.begin(ctx, &f)?;
        let path = f.arg(ctx, 1)?;
        if path != 0 {
            let name = strings::wide(ctx, path)?;
            strings::replace(ctx, text.cell, &name)?;
            return save_selected(ctx, f, &text);
        }
        let path = strings::value(ctx, doc + 0x24)?;
        let label = if path.is_empty() {
            strings::value(ctx, doc + 0x20)?
        } else {
            path
        };
        let label = if label.is_empty() {
            super::resources::framework_string(ctx, 0xf003)?
        } else {
            label
        };
        let p = super::application::duplicate_wide(ctx, &label)?;
        f.own(ctx, p)?;
        let template = word(ctx, doc + 0x28)?;
        if template != 0 {
            return f.call_virtual(ctx, 1, template, 26, &[text.cell, 4]);
        }
        strings::replace(ctx, text.cell, &label)?;
        return prompt_save(ctx, f, &text);
    }
    if let Some(outcome) = text.rethrow(ctx, &f)? {
        return Ok(outcome);
    }
    match f.phase(ctx)? {
        1 => {
            let label = word(ctx, f.address + 40)?;
            let mut name = String::from_utf16_lossy(&strings::wide(ctx, label)?);
            let extension = String::from_utf16_lossy(&strings::value(ctx, text.cell)?);
            if result != 0
                && !extension.is_empty()
                && !name.rsplit('\\').next().unwrap_or("").contains('.')
            {
                name.push_str(extension.split(';').next().unwrap_or(""));
            }
            strings::replace(ctx, text.cell, &name.encode_utf16().collect::<Vec<_>>())?;
            prompt_save(ctx, f, &text)
        }
        2 => {
            if result == 0 {
                text.end(ctx)?;
                return f.finish(ctx, 0);
            }
            save_selected(ctx, f, &text)
        }
        3 => {
            if result == 0 {
                text.end(ctx)?;
                return f.finish(ctx, 0);
            }
            if f.arg(ctx, 2)? != 0 {
                let pointer = word(ctx, text.cell)?;
                return f.call_virtual(ctx, 4, doc, 22, &[pointer, 1]);
            }
            text.end(ctx)?;
            f.finish(ctx, 1)
        }
        4 => {
            text.end(ctx)?;
            f.finish(ctx, 1)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid document save continuation".into(),
        )),
    }
}
fn prompt_save(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    text: &ScopedString,
) -> Result<DispatchOutcome, KernelError> {
    let doc = f.arg(ctx, 0)?;
    let module = super::application::state(ctx)?;
    let app = word(ctx, module + 4)?;
    if app == 0 {
        return Err(KernelError::Dispatch(
            "MFC save dialog has no application owner".into(),
        ));
    }
    let manager = super::document_manager::ensure(ctx, app)?;
    let template = word(ctx, doc + 0x28)?;
    f.call_virtual(
        ctx,
        2,
        manager,
        12,
        &[text.cell, 0xf004, 0x804, 0, template],
    )
}
fn save_selected(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    text: &ScopedString,
) -> Result<DispatchOutcome, KernelError> {
    let doc = f.arg(ctx, 0)?;
    let pointer = word(ctx, text.cell)?;
    f.call_virtual(ctx, 3, doc, 31, &[pointer])
}
fn default_menu(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(0)?;
    ctx.cpu.check_guest_access(p, 88, pocket_cpu::Prot::READ)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn empty_idle(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let doc = ctx.arg_u32(0)?;
    ctx.cpu
        .check_guest_access(doc, 88, pocket_cpu::Prot::READ)?; /* native base body is empty */
    Ok(DispatchOutcome::ReturnedR0(0))
}
