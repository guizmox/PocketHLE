//! Native document/view links reuse CPtrList, CString and HWND owners. Lists
//! borrow views; destroying a document detaches them instead of freeing them.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) const ADD: &str = "__PocketHLE_MFC_DocumentAddView";
pub(super) const REMOVE: &str = "__PocketHLE_MFC_DocumentRemoveView";
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", ADD, add);
    d.register_handler("coredll.dll", REMOVE, remove);
}
fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    super::windows::initialize_target(ctx, o, "CDocument", 88)?;
    new_string(ctx, o + 0x20, &[])?;
    if let Err(e) = new_string(ctx, o + 0x24, &[]) {
        release_string(ctx, o + 0x20)?;
        return Err(e);
    }
    if let Err(e) = super::lists::initialize(ctx, o + 0x2c, 10) {
        release_string(ctx, o + 0x24)?;
        release_string(ctx, o + 0x20)?;
        return Err(e);
    }
    put(ctx, o + 0x4c, 1)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx,
        "CDocument",
    )?))
}
fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    delete(ctx, false)
}
pub(super) fn delete(
    ctx: &mut CallCtx<'_>,
    deleting: bool,
) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu
            .check_guest_access(o, 88, Prot::READ | Prot::WRITE)?;
        let flags = f.arg(ctx, 1)?;
        if deleting && flags & !1 != 0 {
            return Err(KernelError::Dispatch(
                "MFC CDocument vector deletion is unsupported".into(),
            ));
        }
        f.set(ctx, 0, u32::from(deleting && flags & 1 != 0))?;
        let table = super::classes::vtable(ctx, "CDocument")?;
        put(ctx, o, table)?;
        let mut node = super::lists::next_position(ctx, o + 0x2c, 0)?;
        while node != 0 {
            let view = word(ctx, node + 8)?;
            put(ctx, view + 0x48, 0)?;
            node = super::lists::next_position(ctx, o + 0x2c, node)?;
        }
        let template = word(ctx, o + 0x28)?;
        if template != 0 {
            let target = virtual_function(ctx, template, 25)?;
            return f.call_args(ctx, 1, target, &[template, o]);
        }
    }
    if f.fresh || f.phase(ctx)? == 1 {
        super::lists::clear(ctx, o + 0x2c)?;
        release_string(ctx, o + 0x24)?;
        release_string(ctx, o + 0x20)?;
        return f.call_service(ctx, 2, super::command_targets::DESTRUCT, &[o]);
    }
    if f.get(ctx, 0)? != 0 {
        ctx.kernel.heap.free(o);
    }
    f.finish(ctx, o)
}
fn add(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    link(ctx, true)
}
fn remove(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    link(ctx, false)
}
fn link(ctx: &mut CallCtx<'_>, adding: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        return f.finish(ctx, 0);
    }
    let doc = f.arg(ctx, 0)?;
    let view = f.arg(ctx, 1)?;
    ctx.cpu
        .check_guest_access(view, 76, Prot::READ | Prot::WRITE)?;
    let position = super::lists::search(ctx, doc + 0x2c, view, 0)?;
    if adding {
        if word(ctx, view + 0x48)? != 0 || position != 0 {
            return Err(KernelError::Dispatch(
                "MFC view already belongs to a document".into(),
            ));
        }
        super::lists::append(ctx, doc + 0x2c, view)?;
        put(ctx, view + 0x48, doc)?;
    } else {
        if word(ctx, view + 0x48)? != doc || position == 0 {
            return Err(KernelError::Dispatch(
                "MFC view/document association is inconsistent".into(),
            ));
        }
        super::lists::remove(ctx, doc + 0x2c, position)?;
        put(ctx, view + 0x48, 0)?;
    }
    let target = virtual_function(ctx, doc, 27)?;
    f.call_args(ctx, 1, target, &[doc])
}
fn first_view(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let doc = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(super::lists::next_position(
        ctx,
        doc + 0x2c,
        0,
    )?))
}
fn next_view(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let doc = ctx.arg_u32(0)?;
    let out = ctx.arg_u32(1)?;
    ctx.cpu
        .check_guest_access(out, 4, Prot::READ | Prot::WRITE)?;
    let node = word(ctx, out)?;
    if node == 0 {
        return Err(KernelError::Dispatch(
            "MFC GetNextView has no POSITION".into(),
        ));
    }
    let next = super::lists::next_position(ctx, doc + 0x2c, node)?;
    let view = word(ctx, node + 8)?;
    put(ctx, out, next)?;
    Ok(DispatchOutcome::ReturnedR0(view))
}
fn changed_views(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        return f.finish(ctx, 0);
    }
    let doc = f.arg(ctx, 0)?;
    let slot = if word(ctx, doc + 0x38)? == 0 && word(ctx, doc + 0x4c)? != 0 {
        32
    } else {
        41
    };
    let target = virtual_function(ctx, doc, slot)?;
    f.call_args(ctx, 1, target, &[doc])
}
fn advance_view(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    phase: u32,
) -> Result<DispatchOutcome, KernelError> {
    if word(ctx, f.address + 128)? == 0 {
        return f.finish(ctx, 0);
    }
    let visits = f.get(ctx, 7)?;
    if visits >= 4096 {
        return Err(KernelError::Dispatch(
            "MFC document traversal exceeded its bound".into(),
        ));
    }
    f.set(ctx, 7, visits + 1)?;
    let doc = f.arg(ctx, 0)?;
    let target = virtual_function(ctx, doc, 26)?;
    f.call_args(ctx, phase, target, &[doc, f.address + 128])
}
fn update_all(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let doc = f.arg(ctx, 0)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        let target = virtual_function(ctx, doc, 25)?;
        return f.call_args(ctx, 1, target, &[doc]);
    }
    match f.phase(ctx)? {
        1 => {
            put(ctx, f.address + 128, result)?;
            advance_view(ctx, f, 2)
        }
        2 => {
            let sender = f.arg(ctx, 1)?;
            if result == sender {
                return advance_view(ctx, f, 2);
            }
            let target = virtual_function(ctx, result, 53)?;
            let hint = f.arg(ctx, 2)?;
            let object = f.arg(ctx, 3)?;
            f.call_args(ctx, 3, target, &[result, sender, hint, object])
        }
        3 => advance_view(ctx, f, 2),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC UpdateAllViews continuation".into(),
        )),
    }
}
fn update_counts(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let doc = f.arg(ctx, 0)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        let target = virtual_function(ctx, doc, 25)?;
        return f.call_args(ctx, 1, target, &[doc]);
    }
    match f.phase(ctx)? {
        1 => {
            put(ctx, f.address + 128, result)?;
            counts_next(ctx, f)
        }
        2 => {
            f.set(ctx, 4, result)?;
            let hwnd = word(ctx, result + 0x20)?;
            f.call_service(ctx, 3, "IsWindowVisible", &[hwnd])
        }
        3 if result != 0 => {
            let view = f.get(ctx, 4)?;
            f.call_service(ctx, 4, super::hierarchy::PARENT, &[view])
        }
        3 => counts_next(ctx, f),
        4 if result == 0 => counts_next(ctx, f),
        4 => {
            match f.get(ctx, 0)? {
                0 => put(ctx, result + 0x58, u32::MAX)?,
                1 if word(ctx, result + 0x58)? == u32::MAX => {
                    let n = f.get(ctx, 1)? + 1;
                    f.set(ctx, 1, n)?;
                    put(ctx, result + 0x58, n)?;
                }
                2 if word(ctx, result + 0x58)? == f.get(ctx, 2)? + 1 => {
                    let n = f.get(ctx, 2)? + 1;
                    f.set(ctx, 2, n)?;
                    if f.get(ctx, 1)? == 1 {
                        put(ctx, result + 0x58, 0)?;
                    }
                    let target = virtual_function(ctx, result, 55)?;
                    return f.call_args(ctx, 5, target, &[result, 1]);
                }
                _ => {}
            }
            counts_next(ctx, f)
        }
        5 => counts_next(ctx, f),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC frame-count continuation".into(),
        )),
    }
}
fn counts_next(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    if word(ctx, f.address + 128)? != 0 {
        return advance_view(ctx, f, 2);
    }
    let pass = f.get(ctx, 0)? + 1;
    if pass >= 3 {
        return f.finish(ctx, 0);
    }
    f.set(ctx, 0, pass)?;
    f.set(ctx, 7, 0)?;
    let doc = f.arg(ctx, 0)?;
    let target = virtual_function(ctx, doc, 25)?;
    f.call_args(ctx, 1, target, &[doc])
}
fn modified(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let doc = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(word(ctx, doc + 0x48)?))
}
fn set_modified(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let doc = ctx.arg_u32(0)?;
    let value = ctx.arg_u32(1)?;
    put(ctx, doc + 0x48, value)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn set_title(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        return f.finish(ctx, 0);
    }
    let doc = f.arg(ctx, 0)?;
    let text = f.arg(ctx, 1)?;
    super::strings::assign_pointer(ctx, doc + 0x20, text)?;
    let target = virtual_function(ctx, doc, 41)?;
    f.call_args(ctx, 1, target, &[doc])
}
fn empty_contents(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    ctx.cpu.check_guest_access(object, 88, Prot::READ)?;
    // Retail DeleteContents and PreCloseFrame have empty base bodies.
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn close_document(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let doc = f.arg(ctx, 0)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        let old = word(ctx, doc + 0x4c)?;
        f.set(ctx, 0, old)?;
        f.save_region(ctx, 144, doc + 0x4c, 4)?;
        put(ctx, doc + 0x4c, 0)?;
        return close_next(ctx, f);
    }
    match f.phase(ctx)? {
        1 if result != 0 => {
            f.set(ctx, 2, result)?;
            let target = virtual_function(ctx, doc, 38)?;
            f.call_args(ctx, 2, target, &[doc, result])
        }
        1 => Err(KernelError::Dispatch(
            "MFC document view has no parent frame".into(),
        )),
        2 => {
            let frame = f.get(ctx, 2)?;
            let target = virtual_function(ctx, frame, 23)?;
            f.call_args(ctx, 3, target, &[frame])
        }
        3 => {
            let before = f.get(ctx, 1)?;
            if word(ctx, doc + 0x38)? >= before {
                return Err(KernelError::Dispatch(
                    "MFC frame destruction did not detach document views".into(),
                ));
            }
            close_next(ctx, f)
        }
        4 => {
            if word(ctx, doc + 0x4c)? != 0 {
                let target = virtual_function(ctx, doc, 1)?;
                return f.call_args(ctx, 5, target, &[doc, 1]);
            }
            f.finish(ctx, 0)
        }
        5 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC document close continuation".into(),
        )),
    }
}
fn close_next(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let doc = f.arg(ctx, 0)?;
    let count = word(ctx, doc + 0x38)?;
    if count == 0 {
        let old = f.get(ctx, 0)?;
        put(ctx, doc + 0x4c, old)?;
        put(ctx, f.address + 44, 0)?; // the object may be deleted after this point
        let target = virtual_function(ctx, doc, 28)?;
        return f.call_args(ctx, 4, target, &[doc]);
    }
    f.set(ctx, 1, count)?;
    let head = super::lists::next_position(ctx, doc + 0x2c, 0)?;
    let view = word(ctx, head + 8)?;
    f.call_service(ctx, 1, super::hierarchy::PARENT, &[view])
}
fn final_release(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        return f.finish(ctx, 0);
    }
    let doc = f.arg(ctx, 0)?;
    let target = virtual_function(ctx, doc, 32)?;
    f.call_args(ctx, 1, target, &[doc])
}
fn command(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let doc = f.arg(ctx, 0)?;
    if f.fresh {
        let info = ctx.arg_u32(4)?;
        f.set(ctx, 0, info)?;
        let args = [doc, f.arg(ctx, 1)?, f.arg(ctx, 2)?, f.arg(ctx, 3)?, info];
        return f.call_service(ctx, 1, super::command_routing::BASE, &args);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.phase(ctx)? == 1 && result == 0 {
        let template = word(ctx, doc + 0x28)?;
        if template != 0 {
            let target = virtual_function(ctx, template, 5)?;
            let args = [
                template,
                f.arg(ctx, 1)?,
                f.arg(ctx, 2)?,
                f.arg(ctx, 3)?,
                f.get(ctx, 0)?,
            ];
            return f.call_args(ctx, 2, target, &args);
        }
    }
    f.finish(ctx, u32::from(result != 0))
}

pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (85, "??0CDocument@@QAA@XZ", construct),
    (236, "??1CDocument@@UAA@XZ", destruct),
    (
        1462,
        "?GetRuntimeClass@CDocument@@UBAPAUCRuntimeClass@@XZ",
        runtime,
    ),
    (600, "?AddView@CDocument@@QAAXPAVCView@@@Z", add),
    (2219, "?RemoveView@CDocument@@QAAXPAVCView@@@Z", remove),
    (
        1302,
        "?GetFirstViewPosition@CDocument@@UBAPAU__POSITION@@XZ",
        first_view,
    ),
    (
        1395,
        "?GetNextView@CDocument@@UBAPAVCView@@AAPAU__POSITION@@@Z",
        next_view,
    ),
    (1812, "?OnChangedViewList@CDocument@@UAAXXZ", changed_views),
    (
        2493,
        "?UpdateAllViews@CDocument@@QAAXPAVCView@@JPAVCObject@@@Z",
        update_all,
    ),
    (2497, "?UpdateFrameCounts@CDocument@@UAAXXZ", update_counts),
    (1702, "?IsModified@CDocument@@UAAHXZ", modified),
    (2376, "?SetModifiedFlag@CDocument@@UAAXH@Z", set_modified),
    (2435, "?SetTitle@CDocument@@UAAXPBG@Z", set_title),
    (1033, "?DeleteContents@CDocument@@UAAXXZ", empty_contents),
    (
        2089,
        "?PreCloseFrame@CDocument@@UAAXPAVCFrameWnd@@@Z",
        empty_contents,
    ),
    (1828, "?OnCloseDocument@CDocument@@UAAXXZ", close_document),
    (1909, "?OnFinalRelease@CDocument@@UAAXXZ", final_release),
    (
        1832,
        "?OnCmdMsg@CDocument@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z",
        command,
    ),
];

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    #[test]
    fn documents_borrow_views_dispatch_updates_and_release_only_their_strings_and_list_blocks() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let mut freed = None;
        for _ in 0..2 {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:85",
                &[0x2000],
            );
            cpu.write_mem(0x204c, &0u32.to_le_bytes()).unwrap(); // externally owned document
            for (object, hwnd) in [(0x2200, 7u32), (0x2400, 8)] {
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "mfcce300.dll",
                    "ord:200",
                    &[object],
                );
                cpu.write_mem(object + 0x20, &hwnd.to_le_bytes()).unwrap();
                k.window_classes.insert(hwnd, "View".into());
                k.window_visibility.insert(hwnd, false);
                cpu.write_arg(0, 0x2000).unwrap();
                cpu.write_arg(1, object).unwrap();
                drive(&mut cpu, &mut k, &mut d, entry(600), |pc, _| {
                    panic!("unexpected {pc:x}")
                });
                assert_eq!(cpu.read_u32_le(object + 0x48).unwrap(), 0x2000);
            }
            assert_eq!(cpu.read_u32_le(0x2038).unwrap(), 2);
            let table = cpu.read_u32_le(0x2400).unwrap();
            let methods = cpu.read_mem(table, 55 * 4).unwrap();
            cpu.write_mem(0x2800, &methods).unwrap();
            cpu.write_mem(0x2800 + 53 * 4, &0x6100u32.to_le_bytes())
                .unwrap();
            cpu.write_mem(0x2400, &0x2800u32.to_le_bytes()).unwrap();
            for (i, value) in [0x2000u32, 0x2200, 73, 0x3333].into_iter().enumerate() {
                cpu.write_arg(i as u8, value).unwrap();
            }
            let mut seen = 0;
            drive(&mut cpu, &mut k, &mut d, entry(2493), |pc, cpu| {
                assert_eq!(pc, 0x6100);
                assert_eq!(
                    (0..4).map(|i| cpu.read_arg(i).unwrap()).collect::<Vec<_>>(),
                    [0x2400, 0x2200, 73, 0x3333]
                );
                seen += 1;
                0
            });
            assert_eq!(seen, 1);
            let first = result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1302",
                &[0x2000],
            ));
            cpu.write_mem(0x2600, &first.to_le_bytes()).unwrap();
            assert_eq!(
                result(call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "mfcce300.dll",
                    "ord:1395",
                    &[0x2000, 0x2600]
                )),
                0x2200
            );
            assert_eq!(
                result(call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "mfcce300.dll",
                    "ord:1395",
                    &[0x2000, 0x2600]
                )),
                0x2400
            );
            assert_eq!(cpu.read_u32_le(0x2600).unwrap(), 0);
            cpu.write_arg(0, 0x2000).unwrap();
            drive(&mut cpu, &mut k, &mut d, entry(236), |pc, _| {
                panic!("unexpected {pc:x}")
            });
            for object in [0x2200, 0x2400] {
                assert_eq!(cpu.read_u32_le(object + 0x48).unwrap(), 0);
                // The native document destructor detaches borrowed views; it
                // does not send WM_DESTROY or delete their caller-owned bytes.
                assert_ne!(cpu.read_u32_le(object).unwrap(), 0);
                cpu.write_mem(object + 0x20, &0u32.to_le_bytes()).unwrap();
                cpu.write_arg(0, object).unwrap();
                drive(&mut cpu, &mut k, &mut d, entry(309), |pc, _| {
                    panic!("unexpected {pc:x}")
                });
            }
            if let Some(free) = freed {
                assert_eq!(k.heap.free_bytes(), free);
            }
            freed = Some(k.heap.free_bytes());
            assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        }
    }
}
