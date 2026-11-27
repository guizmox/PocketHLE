//! One guest-owned template registration list and the native CDocManager owner.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) const ADD: &str = "__PocketHLE_MFC_ManagerAddTemplate";
const PROMPT: &str = "__PocketHLE_MFC_DocumentPrompt";
pub(super) const APP_CLOSE_ALL: &str = "__PocketHLE_MFC_AppCloseAllDocuments";
const ROOT: &str = "__PocketHLE_MFC_TemplateRegistration";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (866, "?CloseAllDocuments@CWinApp@@QAAXH@Z", app_close_all),
    (2240, "?SaveAllModified@CWinApp@@UAAHXZ", app_save_all),
    (83, "??0CDocManager@@QAA@XZ", construct),
    (234, "??1CDocManager@@UAA@XZ", destruct),
    (
        1460,
        "?GetRuntimeClass@CDocManager@@UBAPAUCRuntimeClass@@XZ",
        runtime,
    ),
    (
        571,
        "?AddDocTemplate@CDocManager@@UAAXPAVCDocTemplate@@@Z",
        add,
    ),
    (
        572,
        "?AddDocTemplate@CWinApp@@QAAXPAVCDocTemplate@@@Z",
        app_add,
    ),
    (
        1300,
        "?GetFirstDocTemplatePosition@CDocManager@@UBAPAU__POSITION@@XZ",
        first,
    ),
    (
        1301,
        "?GetFirstDocTemplatePosition@CWinApp@@QBAPAU__POSITION@@XZ",
        app_first,
    ),
    (
        1392,
        "?GetNextDocTemplate@CDocManager@@UBAPAVCDocTemplate@@AAPAU__POSITION@@@Z",
        next,
    ),
    (
        1393,
        "?GetNextDocTemplate@CWinApp@@QBAPAVCDocTemplate@@AAPAU__POSITION@@@Z",
        app_next,
    ),
    (
        2066,
        "?OpenDocumentFile@CDocManager@@UAAPAVCDocument@@PBG@Z",
        open,
    ),
    (
        2068,
        "?OpenDocumentFile@CWinApp@@UAAPAVCDocument@@PBG@Z",
        app_open,
    ),
    (2238, "?SaveAllModified@CDocManager@@UAAHXZ", save_all),
    (864, "?CloseAllDocuments@CDocManager@@UAAXH@Z", close_all),
    (1401, "?GetOpenDocumentCount@CDocManager@@UAAHXZ", count),
    (1280, "?GetDocumentCount@CDocManager@@IAAHXZ", count),
    (
        1086,
        "?DoPromptFileName@CDocManager@@UAAHAAVCString@@IKHPAVCDocTemplate@@@Z",
        prompt,
    ),
    (
        1087,
        "?DoPromptFileName@CWinApp@@QAAHAAVCString@@IKHPAVCDocTemplate@@@Z",
        app_prompt,
    ),
    (1901, "?OnFileNew@CDocManager@@UAAXXZ", new),
    (1902, "?OnFileNew@CWinApp@@IAAXXZ", app_new),
    (1903, "?OnFileOpen@CDocManager@@UAAXXZ", open_prompt),
    (1904, "?OnFileOpen@CWinApp@@IAAXXZ", app_open_prompt),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", APP_CLOSE_ALL, app_close_all);
    d.register_handler("coredll.dll", ADD, add);
    d.register_handler("coredll.dll", PROMPT, prompt);
}
fn registration(ctx: &mut CallCtx<'_>) -> Result<u32, KernelError> {
    let (p, fresh) = super::application::allocation(ctx, ROOT, 40)?;
    if fresh {
        put(ctx, p, 1)?;
        super::lists::initialize(ctx, p + 8, 10)?;
    }
    Ok(p)
}
fn initialize(ctx: &mut CallCtx<'_>, o: u32) -> Result<(), KernelError> {
    ctx.cpu
        .check_guest_access(o, 32, Prot::READ | Prot::WRITE)?;
    let v = super::classes::vtable(ctx, "CDocManager")?;
    put(ctx, o, v)?;
    super::lists::initialize(ctx, o + 4, 10)
}
fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    initialize(ctx, o)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx,
        "CDocManager",
    )?))
}
pub(super) fn defer_template(ctx: &mut CallCtx<'_>, template: u32) -> Result<bool, KernelError> {
    let root = registration(ctx)?;
    if word(ctx, root)? == 0 {
        return Ok(false);
    }
    if word(ctx, root + 4)? == 0 {
        let manager = alloc_cpp(ctx, 32)?;
        if let Err(e) = initialize(ctx, manager) {
            ctx.kernel.heap.free(manager);
            return Err(e);
        }
        put(ctx, root + 4, manager)?;
    }
    super::lists::append(ctx, root + 8, template)?;
    put(ctx, template + 0x20, 0)?;
    Ok(true)
}
/// Remove a destroyed template from borrowed registration lists as well as an
/// active manager. A manager's deletion loop may already have removed its node.
pub(super) fn forget_template(ctx: &mut CallCtx<'_>, template: u32) -> Result<(), KernelError> {
    let owner = super::application::owner(ctx)?;
    let mut lists = Vec::new();
    if let Some(&root) = ctx.kernel.dynamic_exports[&owner].get(ROOT) {
        lists.push(root + 8);
    }
    if let Some(module) = super::application::existing_state(ctx)? {
        let app = word(ctx, module + 4)?;
        if app != 0 {
            let manager = word(ctx, app + 0x80)?;
            if manager != 0 {
                lists.push(manager + 4);
            }
        }
    }
    lists.sort_unstable();
    lists.dedup();
    let mut removals = Vec::new();
    for list in lists {
        let node = super::lists::search(ctx, list, template, 0)?;
        if node != 0 {
            removals.push((list, node));
        }
    }
    for (list, node) in removals {
        super::lists::remove(ctx, list, node)?;
    }
    Ok(())
}
pub(super) fn activate(ctx: &mut CallCtx<'_>, app: u32) -> Result<u32, KernelError> {
    let root = registration(ctx)?;
    let manager = word(ctx, app + 0x80)?;
    let deferred = word(ctx, root + 4)?;
    if manager == 0 && deferred != 0 {
        put(ctx, app + 0x80, deferred)?;
        put(ctx, root + 4, 0)?;
    }
    if manager != 0 && deferred != 0 && manager != deferred {
        if super::lists::next_position(ctx, deferred + 4, 0)? != 0 {
            return Err(KernelError::Dispatch(
                "Deferred MFC manager unexpectedly owns templates".into(),
            ));
        }
        super::lists::clear(ctx, deferred + 4)?;
        ctx.kernel.heap.free(deferred);
        put(ctx, root + 4, 0)?;
    }
    put(ctx, root, 0)?;
    word(ctx, app + 0x80)
}
pub(super) fn ensure(ctx: &mut CallCtx<'_>, app: u32) -> Result<u32, KernelError> {
    let old = word(ctx, app + 0x80)?;
    if old != 0 {
        return Ok(old);
    }
    let o = alloc_cpp(ctx, 32)?;
    if let Err(e) = initialize(ctx, o) {
        ctx.kernel.heap.free(o);
        return Err(e);
    }
    put(ctx, app + 0x80, o)?;
    Ok(o)
}
fn app_add(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let app = f.arg(ctx, 0)?;
        let manager = ensure(ctx, app)?;
        let template = f.arg(ctx, 1)?;
        let target = virtual_function(ctx, manager, 5)?;
        return f.call_args(ctx, 1, target, &[manager, template]);
    }
    f.finish(ctx, 0)
}
fn add(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let manager = f.arg(ctx, 0)?;
    let template = f.arg(ctx, 1)?;
    if template == 0 {
        let root = registration(ctx)?;
        let node = super::lists::next_position(ctx, root + 8, 0)?;
        if node == 0 {
            super::lists::clear(ctx, root + 8)?;
            put(ctx, root, 0)?;
            return f.finish(ctx, 0);
        }
        let template = word(ctx, node + 8)?;
        super::lists::remove(ctx, root + 8, node)?;
        let target = virtual_function(ctx, manager, 5)?;
        return f.call_args(ctx, 1, target, &[manager, template]);
    }
    if f.fresh {
        let target = virtual_function(ctx, template, 21)?;
        return f.call_args(ctx, 1, target, &[template]);
    }
    if super::lists::search(ctx, manager + 4, template, 0)? != 0 {
        return Err(KernelError::Dispatch(
            "MFC template is already registered".into(),
        ));
    }
    super::lists::append(ctx, manager + 4, template)?;
    f.finish(ctx, 0)
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
        let flags = f.arg(ctx, 1)?;
        if deleting && flags & !1 != 0 {
            return Err(KernelError::Dispatch("MFC manager vector deletion".into()));
        }
        f.set(ctx, 0, u32::from(deleting && flags & 1 != 0))?;
    }
    let node = super::lists::next_position(ctx, o + 4, 0)?;
    if node != 0 {
        let template = word(ctx, node + 8)?;
        super::lists::remove(ctx, o + 4, node)?;
        if word(ctx, template + 0x20)? != 0 {
            let target = virtual_function(ctx, template, 1)?;
            return f.call_args(ctx, 1, target, &[template, 1]);
        }
        // Static templates are borrowed: remove the node but preserve their object.
        return delete_next(ctx, f, o);
    }
    delete_next(ctx, f, o)
}
fn delete_next(ctx: &mut CallCtx<'_>, f: Frame, o: u32) -> Result<DispatchOutcome, KernelError> {
    let mut node = super::lists::next_position(ctx, o + 4, 0)?;
    while node != 0 {
        let template = word(ctx, node + 8)?;
        super::lists::remove(ctx, o + 4, node)?;
        if word(ctx, template + 0x20)? != 0 {
            let target = virtual_function(ctx, template, 1)?;
            return f.call_args(ctx, 1, target, &[template, 1]);
        }
        node = super::lists::next_position(ctx, o + 4, 0)?;
    }
    super::lists::clear(ctx, o + 4)?;
    if f.get(ctx, 0)? != 0 {
        ctx.kernel.heap.free(o);
    }
    f.finish(ctx, o)
}
fn first(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(super::lists::next_position(
        ctx,
        o + 4,
        0,
    )?))
}
fn next(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let ptr = ctx.arg_u32(1)?;
    let node = word(ctx, ptr)?;
    let after = super::lists::next_position(ctx, o + 4, node)?;
    if node == 0 {
        return Err(KernelError::Dispatch(
            "MFC template iterator is empty".into(),
        ));
    }
    let value = word(ctx, node + 8)?;
    put(ctx, ptr, after)?;
    Ok(DispatchOutcome::ReturnedR0(value))
}
fn app_first(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let app = ctx.arg_u32(0)?;
    let o = word(ctx, app + 0x80)?;
    Ok(DispatchOutcome::ReturnedR0(if o == 0 {
        0
    } else {
        super::lists::next_position(ctx, o + 4, 0)?
    }))
}
fn app_close_all(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    forward(ctx, 10, false)
}
fn app_save_all(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    forward(ctx, 9, false)
}
fn app_next(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    forward(ctx, 7, false)
}
fn app_open(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    forward(ctx, 8, false)
}
fn app_prompt(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    forward(ctx, 12, true)
}
fn app_new(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    forward(ctx, 14, false)
}
fn app_open_prompt(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    forward(ctx, 15, false)
}
fn forward(ctx: &mut CallCtx<'_>, slot: u32, prompt: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let app = f.arg(ctx, 0)?;
        let manager = word(ctx, app + 0x80)?;
        if manager == 0 {
            return f.finish(ctx, u32::from(slot == 9));
        }
        let count = if prompt {
            6
        } else if matches!(slot, 7 | 8 | 10) {
            2
        } else {
            1
        };
        let mut args = vec![manager];
        for i in 1..count {
            args.push(if i < 4 {
                f.arg(ctx, i)?
            } else {
                ctx.arg_u32(i as u8)?
            });
        }
        let target = virtual_function(ctx, manager, slot)?;
        return f.call_args(ctx, 1, target, &args);
    }
    let value = ctx.cpu.read_reg(ArmReg::R0)?;
    f.finish(ctx, value)
}
fn save_all(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    iterate(ctx, 31, true, false)
}
fn close_all(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    iterate(ctx, 32, false, false)
}
fn count(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    iterate(ctx, 22, false, true)
}
fn iterate(
    ctx: &mut CallCtx<'_>,
    slot: u32,
    stop_false: bool,
    count: bool,
) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        let node = super::lists::next_position(ctx, o + 4, 0)?;
        f.set(ctx, 0, node)?;
    } else if count {
        if f.phase(ctx)? == 1 {
            put(ctx, f.address + 128, result)?;
        } else if f.phase(ctx)? == 2 {
            let next = f.get(ctx, 1)? + 1;
            f.set(ctx, 1, next)?;
        }
        if word(ctx, f.address + 128)? != 0 {
            let template = f.get(ctx, 2)?;
            let target = virtual_function(ctx, template, 23)?;
            return f.call_args(ctx, 2, target, &[template, f.address + 128]);
        }
    } else if stop_false && result == 0 {
        return f.finish(ctx, 0);
    }
    let node = f.get(ctx, 0)?;
    if node == 0 {
        let value = if count {
            f.get(ctx, 1)?
        } else {
            u32::from(stop_false)
        };
        return f.finish(ctx, value);
    }
    let after = super::lists::next_position(ctx, o + 4, node)?;
    f.set(ctx, 0, after)?;
    let template = word(ctx, node + 8)?;
    f.set(ctx, 2, template)?;
    let target = virtual_function(ctx, template, slot)?;
    let args = if slot == 32 {
        vec![template, f.arg(ctx, 1)?]
    } else {
        vec![template]
    };
    f.call_args(ctx, 1, target, &args)
}
// Open and dialogs are kept in a separate adapter: registry/list ownership is
// independent of modal filename selection and template-confidence traversal.
fn open(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    super::document_selection::open(ctx)
}
fn new(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    super::document_selection::new(ctx)
}
fn open_prompt(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    super::document_selection::open_prompt(ctx)
}
fn prompt(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    super::document_selection::prompt(ctx)
}
