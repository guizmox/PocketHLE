//! SDI open/new transitions use native virtuals and scoped document flags.
use super::callbacks::{virtual_function, Frame, ScopedString};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) fn match_type(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let out = f.arg(ctx, 2)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    let temp = ScopedString::at(&f, 132, 192);
    if f.fresh {
        ctx.cpu.check_guest_access(out, 4, Prot::WRITE)?;
        put(ctx, out, 0)?;
        temp.begin(ctx, &f)?;
        let target = virtual_function(ctx, o, 22)?;
        return f.call_args(ctx, 1, target, &[o]);
    }
    if let Some(outcome) = temp.rethrow(ctx, &f)? {
        return Ok(outcome);
    }
    match f.phase(ctx)? {
        1 => put(ctx, f.address + 128, result)?,
        2 => {
            let path = strings::value(ctx, result + 0x24)?;
            let pointer = f.arg(ctx, 1)?;
            let input = strings::wide(ctx, pointer)?;
            if same(&path, &input) {
                put(ctx, out, result)?;
                temp.end(ctx)?;
                return f.finish(ctx, 5);
            }
        }
        3 => {
            let ext = strings::value(ctx, temp.cell)?;
            let pointer = f.arg(ctx, 1)?;
            let input = strings::wide(ctx, pointer)?;
            let suffix = input.iter().rposition(|&c| c == 46).map(|i| &input[i..]);
            let native =
                result != 0 && !ext.is_empty() && suffix.is_some_and(|suffix| same(suffix, &ext));
            temp.end(ctx)?;
            return f.finish(ctx, if native { 4 } else { 3 });
        }
        _ => {
            return Err(KernelError::Dispatch(
                "Invalid document match continuation".into(),
            ))
        }
    }
    if word(ctx, f.address + 128)? == 0 {
        let target = virtual_function(ctx, o, 26)?;
        return f.call_args(ctx, 3, target, &[o, temp.cell, 4]);
    }
    let n = f.get(ctx, 0)?;
    if n >= 4096 {
        return Err(KernelError::Dispatch(
            "MFC template matching exceeded its bound".into(),
        ));
    }
    f.set(ctx, 0, n + 1)?;
    let target = virtual_function(ctx, o, 23)?;
    f.call_args(ctx, 2, target, &[o, f.address + 128])
}

fn wait(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    phase: u32,
    delta: i32,
) -> Result<DispatchOutcome, KernelError> {
    let module = super::application::state(ctx)?;
    let app = word(ctx, module + 4)?;
    if app == 0 {
        return Err(KernelError::Dispatch(
            "MFC document I/O has no application wait-cursor owner".into(),
        ));
    }
    f.call_virtual(ctx, phase, app, 36, &[delta as u32])
}
fn prepare_content(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let o = f.arg(ctx, 0)?;
    let doc = f.get(ctx, 0)?;
    let path = f.arg(ctx, 1)?;
    if path == 0 {
        return f.call_virtual(ctx, 6, o, 34, &[doc]);
    }
    wait(ctx, &f, 8, 1)
}
fn fail(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    if f.get(ctx, 7)? != 0 {
        return wait(ctx, &f, 19, -1);
    }
    f.finish(ctx, 0)
}
fn success(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    if f.get(ctx, 7)? != 0 {
        return wait(ctx, &f, 20, -1);
    }
    initial(ctx, f)
}
fn initial(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let o = f.arg(ctx, 0)?;
    let doc = f.get(ctx, 0)?;
    let frame = f.get(ctx, 1)?;
    let fresh = f.get(ctx, 2)? != 0;
    if fresh {
        let thread = super::application::thread_object(ctx)?;
        if thread != 0 && word(ctx, thread + 0x20)? == 0 {
            put(ctx, thread + 0x20, frame)?;
        }
    }
    let visible = f.arg(ctx, 2)?;
    f.call_virtual(ctx, 21, o, 30, &[frame, doc, visible])
}
pub(super) fn single(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    single_kind(ctx, false)
}
pub(super) fn single_with_background(
    ctx: &mut CallCtx<'_>,
) -> Result<DispatchOutcome, KernelError> {
    single_kind(ctx, true)
}
fn single_kind(ctx: &mut CallCtx<'_>, background: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        f.set(ctx, 8, u32::from(background))?;
        let doc = word(ctx, o + 0x68)?;
        f.set(ctx, 0, doc)?;
        if doc != 0 {
            return f.call_virtual(ctx, 1, doc, 37, &[]);
        }
        f.set(ctx, 2, 1)?;
        return f.call_virtual(ctx, 2, o, 28, &[]);
    }
    match f.phase(ctx)? {
        1 => {
            if result == 0 {
                return f.finish(ctx, 0);
            }
            let module = super::application::state(ctx)?;
            let app = word(ctx, module + 4)?;
            let frame = if app == 0 { 0 } else { word(ctx, app + 0x20)? };
            f.set(ctx, 1, frame)?;
            if frame != 0 {
                return prepare_content(ctx, f);
            }
            create_frame(ctx, f)
        }
        2 => {
            if result == 0 {
                return f.finish(ctx, 0);
            }
            f.set(ctx, 0, result)?;
            if f.get(ctx, 8)? != 0 {
                let thread = super::application::thread_object(ctx)?;
                let frame = if thread == 0 {
                    0
                } else {
                    word(ctx, thread + 0x20)?
                };
                if frame != 0 {
                    f.set(ctx, 1, frame)?;
                    f.set(ctx, 9, 1)?;
                    let view = word(ctx, frame + 0xac)?;
                    if view != 0 {
                        return f.call_service(ctx, 22, super::documents::ADD, &[result, view]);
                    }
                    return prepare_content(ctx, f);
                }
            }
            create_frame(ctx, f)
        }
        3 => {
            let doc = f.get(ctx, 0)?;
            let auto = f.get(ctx, 3)?;
            put(ctx, doc + 0x4c, auto)?;
            put(ctx, f.address + 44, 0)?;
            if result == 0 {
                return f.call_virtual(ctx, 18, doc, 1, &[1]);
            }
            f.set(ctx, 1, result)?;
            prepare_content(ctx, f)
        }
        6 => {
            let doc = f.get(ctx, 0)?;
            if f.arg(ctx, 2)? == 0 {
                put(ctx, doc + 0x50, 1)?;
            }
            f.call_virtual(ctx, 7, doc, 29, &[])
        }
        7 => {
            if result != 0 {
                return success(ctx, f);
            }
            if f.get(ctx, 2)? == 0 {
                return fail(ctx, f);
            }
            if f.get(ctx, 9)? != 0 {
                return background_failure(ctx, f);
            }
            let frame = f.get(ctx, 1)?;
            f.call_virtual(ctx, 18, frame, 23, &[])
        }
        8 => {
            f.set(ctx, 7, 1)?;
            let doc = f.get(ctx, 0)?;
            f.call_virtual(ctx, 9, doc, 23, &[])
        }
        9 => {
            f.set(ctx, 4, result)?;
            let doc = f.get(ctx, 0)?;
            f.call_virtual(ctx, 10, doc, 24, &[0])
        }
        10 => {
            let doc = f.get(ctx, 0)?;
            let path = f.arg(ctx, 1)?;
            f.call_virtual(ctx, 11, doc, 30, &[path])
        }
        11 => {
            let doc = f.get(ctx, 0)?;
            if result != 0 {
                let path = f.arg(ctx, 1)?;
                return f.call_virtual(ctx, 12, doc, 22, &[path, 1]);
            }
            if f.get(ctx, 2)? != 0 {
                if f.get(ctx, 9)? != 0 {
                    return background_failure(ctx, f);
                }
                let frame = f.get(ctx, 1)?;
                return f.call_virtual(ctx, 18, frame, 23, &[]);
            }
            f.call_virtual(ctx, 13, doc, 23, &[])
        }
        12 => success(ctx, f),
        13 => {
            let doc = f.get(ctx, 0)?;
            if result == 0 {
                let old = f.get(ctx, 4)?;
                return f.call_virtual(ctx, 17, doc, 24, &[old]);
            }
            f.call_virtual(ctx, 14, o, 34, &[doc])
        }
        14 => {
            let doc = f.get(ctx, 0)?;
            f.call_virtual(ctx, 17, doc, 29, &[])
        }
        17 | 18 | 23 => fail(ctx, f),
        22 => prepare_content(ctx, f),
        19 => f.finish(ctx, 0),
        20 => initial(ctx, f),
        21 => {
            let doc = f.get(ctx, 0)?;
            f.finish(ctx, doc)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid SDI open continuation".into(),
        )),
    }
}
fn create_frame(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let o = f.arg(ctx, 0)?;
    let doc = f.get(ctx, 0)?;
    let auto = word(ctx, doc + 0x4c)?;
    f.set(ctx, 3, auto)?;
    f.save_region(ctx, 144, doc + 0x4c, 4)?;
    put(ctx, doc + 0x4c, 0)?;
    if f.get(ctx, 8)? != 0 {
        let table = super::classes::vtable(ctx, "CSingleDocTemplate")?;
        let target = word(ctx, table + 29 * 4)?;
        f.call_args(ctx, 3, target, &[o, doc, 0])
    } else {
        f.call_virtual(ctx, 3, o, 29, &[doc, 0])
    }
}

fn same(a: &[u16], b: &[u16]) -> bool {
    a.iter()
        .map(|&c| crate::coredll::case_wide_unit(c, false))
        .eq(b.iter().map(|&c| crate::coredll::case_wide_unit(c, false)))
}

fn background_failure(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let document = f.get(ctx, 0)?;
    // The background frame remains alive. The ordinary document destructor
    // detaches borrowed views and removes this failed new document.
    put(ctx, document + 0x54, 1)?;
    f.call_virtual(ctx, 23, document, 1, &[1])
}
