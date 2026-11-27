//! CE3 SDI templates own their resources and borrow documents, frames and classes.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
const CONSTRUCT: &str = "__PocketHLE_MFC_TemplateConstruct";
const LOAD: &str = "__PocketHLE_MFC_TemplateLoad";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        84,
        "??0CDocTemplate@@IAA@IPAUCRuntimeClass@@00@Z",
        construct,
    ),
    (
        162,
        "??0CSingleDocTemplate@@QAA@IPAUCRuntimeClass@@00@Z",
        construct_single,
    ),
    (235, "??1CDocTemplate@@UAA@XZ", destruct),
    (287, "??1CSingleDocTemplate@@UAA@XZ", destruct),
    (
        1461,
        "?GetRuntimeClass@CDocTemplate@@UBAPAUCRuntimeClass@@XZ",
        runtime,
    ),
    (
        1518,
        "?GetRuntimeClass@CSingleDocTemplate@@UBAPAUCRuntimeClass@@XZ",
        runtime_single,
    ),
    (1725, "?LoadTemplate@CDocTemplate@@UAAXXZ", load),
    (
        1279,
        "?GetDocString@CDocTemplate@@UBAHAAVCString@@W4DocStringIndex@1@@Z",
        doc_string,
    ),
    (573, "?AddDocument@CDocTemplate@@UAAXPAVCDocument@@@Z", add),
    (
        574,
        "?AddDocument@CSingleDocTemplate@@UAAXPAVCDocument@@@Z",
        add_single,
    ),
    (
        2197,
        "?RemoveDocument@CDocTemplate@@UAAXPAVCDocument@@@Z",
        remove,
    ),
    (
        2198,
        "?RemoveDocument@CSingleDocTemplate@@UAAXPAVCDocument@@@Z",
        remove_single,
    ),
    (
        1299,
        "?GetFirstDocPosition@CSingleDocTemplate@@UBAPAU__POSITION@@XZ",
        first,
    ),
    (
        1391,
        "?GetNextDoc@CSingleDocTemplate@@UBAPAVCDocument@@AAPAU__POSITION@@@Z",
        next,
    ),
    (
        950,
        "?CreateNewDocument@CDocTemplate@@UAAPAVCDocument@@XZ",
        create_document,
    ),
    (
        951,
        "?CreateNewFrame@CDocTemplate@@UAAPAVCFrameWnd@@PAVCDocument@@PAV2@@Z",
        create_frame,
    ),
    (
        1633,
        "?InitialUpdateFrame@CDocTemplate@@UAAXPAVCFrameWnd@@PAVCDocument@@H@Z",
        initial,
    ),
    (2239, "?SaveAllModified@CDocTemplate@@UAAHXZ", save_all),
    (865, "?CloseAllDocuments@CDocTemplate@@UAAXH@Z", close_all),
    (1933, "?OnIdle@CDocTemplate@@UAAXXZ", idle),
    (
        1831,
        "?OnCmdMsg@CDocTemplate@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z",
        command,
    ),
    (
        2339,
        "?SetDefaultTitle@CSingleDocTemplate@@UAAXPAVCDocument@@@Z",
        default_title,
    ),
    (
        1755,
        "?MatchDocType@CDocTemplate@@UAA?AW4Confidence@1@PBGAAPAVCDocument@@@Z",
        match_type,
    ),
    (
        2067,
        "?OpenDocumentFile@CSingleDocTemplate@@UAAPAVCDocument@@PBGH@Z",
        open_single,
    ),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler(
        "coredll.dll",
        "__PocketHLE_MFC_SingleTemplateConstruct",
        construct_single,
    );
    d.register_handler("coredll.dll", CONSTRUCT, construct);
    d.register_handler("coredll.dll", LOAD, load);
}
fn construct_single(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct_kind(ctx, true)
}
fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct_kind(ctx, false)
}
fn construct_kind(ctx: &mut CallCtx<'_>, single: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        super::windows::initialize_target(ctx, o, "CDocTemplate", if single { 108 } else { 104 })?;
        new_string(ctx, o + 0x64, &[])?;
        for (off, value) in [
            (0x20, 1),
            (0x40, f.arg(ctx, 1)?),
            (0x50, f.arg(ctx, 2)?),
            (0x54, f.arg(ctx, 3)?),
            (0x58, ctx.arg_u32(4)?),
        ] {
            put(ctx, o + off, value)?;
        }
        if !super::document_manager::defer_template(ctx, o)? {
            return f.call_service(ctx, 1, LOAD, &[o]);
        }
        if single {
            let v = super::classes::vtable(ctx, "CSingleDocTemplate")?;
            put(ctx, o, v)?;
        }
        return f.finish(ctx, o);
    }
    if single {
        let v = super::classes::vtable(ctx, "CSingleDocTemplate")?;
        put(ctx, o, v)?;
        put(ctx, o + 0x68, 0)?;
    }
    f.finish(ctx, o)
}
fn runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx,
        "CDocTemplate",
    )?))
}
fn runtime_single(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx,
        "CSingleDocTemplate",
    )?))
}
fn load(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        if strings::value(ctx, o + 0x64)?.is_empty() {
            let text = {
                let id = word(ctx, o + 0x40)?;
                super::resources::string(ctx, id)?
            };
            strings::replace(ctx, o + 0x64, &text)?;
        }
    } else {
        let i = f.get(ctx, 0)?;
        let slot = [0x30, 0x38, 0x28][i as usize];
        let value = ctx.cpu.read_reg(ArmReg::R0)?;
        if f.phase(ctx)? == 1 {
            put(ctx, o + slot, value)?;
            let module = f.get(ctx, 1)?;
            let id = word(ctx, o + [0x48, 0x44, 0x4c][i as usize])?;
            return f.call_service(ctx, 2, "LoadAcceleratorsW", &[module, id]);
        }
        put(ctx, o + slot + 4, value)?;
        f.set(ctx, 0, i + 1)?;
    }
    let mut i = f.get(ctx, 0)?;
    while i < 3 {
        let id = word(ctx, o + [0x48, 0x44, 0x4c][i as usize])?;
        if id != 0 && word(ctx, o + [0x30, 0x38, 0x28][i as usize])? == 0 {
            let module = super::resources::module_for(
                ctx,
                &pocket_pe::ResourceKey::Id(id & 0xffff),
                &pocket_pe::ResourceKey::Id(4),
            )?;
            f.set(ctx, 0, i)?;
            f.set(ctx, 1, module)?;
            return f.call_service(ctx, 1, "LoadMenuW", &[module, id & 0xffff]);
        }
        i += 1;
        f.set(ctx, 0, i)?;
    }
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
        super::document_manager::forget_template(ctx, o)?;
        let flags = f.arg(ctx, 1)?;
        if deleting && flags & !1 != 0 {
            return Err(KernelError::Dispatch("MFC template vector deletion".into()));
        }
        f.set(ctx, 1, u32::from(deleting && flags & 1 != 0))?;
    } else if f.phase(ctx)? == 2 {
        if f.get(ctx, 1)? != 0 {
            ctx.kernel.heap.free(o);
        }
        return f.finish(ctx, o);
    }
    let mut i = f.get(ctx, 0)?;
    while i < 3 {
        let slot = [0x28, 0x30, 0x38][i as usize];
        let handle = word(ctx, o + slot)?;
        i += 1;
        f.set(ctx, 0, i)?;
        if handle != 0 {
            put(ctx, o + slot, 0)?;
            return f.call_service(ctx, 1, "DestroyMenu", &[handle]);
        }
    }
    release_string(ctx, o + 0x64)?;
    f.call_service(ctx, 2, super::command_targets::DESTRUCT, &[o])
}
fn add(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    link(ctx, true, false)
}
fn add_single(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    link(ctx, true, true)
}
fn remove(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    link(ctx, false, false)
}
fn remove_single(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    link(ctx, false, true)
}
fn link(ctx: &mut CallCtx<'_>, add: bool, single: bool) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let doc = ctx.arg_u32(1)?;
    ctx.cpu
        .check_guest_access(doc, 88, Prot::READ | Prot::WRITE)?;
    if add {
        if word(ctx, doc + 0x28)? != 0 || single && word(ctx, o + 0x68)? != 0 {
            return Err(KernelError::Dispatch(
                "MFC document already has a template".into(),
            ));
        }
        put(ctx, doc + 0x28, o)?;
    } else {
        if word(ctx, doc + 0x28)? != o {
            return Err(KernelError::Dispatch(
                "MFC template/document ownership mismatch".into(),
            ));
        }
        put(ctx, doc + 0x28, 0)?;
    }
    if single {
        put(ctx, o + 0x68, if add { doc } else { 0 })?;
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn first(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(if word(ctx, o + 0x68)? == 0 {
        0
    } else {
        u32::MAX
    }))
}
fn next(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let position = ctx.arg_u32(1)?;
    let doc = word(ctx, o + 0x68)?;
    if word(ctx, position)? != u32::MAX || doc == 0 {
        return Err(KernelError::Dispatch(
            "Invalid SDI document POSITION".into(),
        ));
    }
    put(ctx, position, 0)?;
    Ok(DispatchOutcome::ReturnedR0(doc))
}
fn doc_string(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let out = ctx.arg_u32(1)?;
    let index = ctx.arg_u32(2)?;
    let text = strings::value(ctx, o + 0x64)?;
    let field = text.split(|&c| c == 10).nth(index as usize);
    let found = field.is_some();
    strings::replace(ctx, out, field.unwrap_or(&[]))?;
    Ok(DispatchOutcome::ReturnedR0(u32::from(found)))
}
fn create_document(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        let class = word(ctx, o + 0x50)?;
        if class == 0 {
            return f.finish(ctx, 0);
        }
        return f.call_service(ctx, 1, super::class_queries::CREATE, &[class]);
    }
    if f.phase(ctx)? == 1 {
        let doc = ctx.cpu.read_reg(ArmReg::R0)?;
        if doc == 0 {
            return f.finish(ctx, 0);
        }
        f.set(ctx, 0, doc)?;
        let target = virtual_function(ctx, o, 24)?;
        return f.call_args(ctx, 2, target, &[o, doc]);
    }
    let doc = f.get(ctx, 0)?;
    f.finish(ctx, doc)
}
fn create_frame(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        let class = word(ctx, o + 0x54)?;
        if class == 0 {
            return f.finish(ctx, 0);
        }
        for (i, value) in [word(ctx, o + 0x58)?, f.arg(ctx, 1)?, o, 0, f.arg(ctx, 2)?]
            .into_iter()
            .enumerate()
        {
            put(ctx, f.address + 128 + i as u32 * 4, value)?;
        }
        return f.call_service(ctx, 1, super::class_queries::CREATE, &[class]);
    }
    if f.phase(ctx)? == 1 {
        let frame = ctx.cpu.read_reg(ArmReg::R0)?;
        if frame == 0 {
            return f.finish(ctx, 0);
        }
        f.set(ctx, 0, frame)?;
        let target = virtual_function(ctx, frame, 46)?;
        let id = word(ctx, o + 0x40)?;
        return f.call_args(ctx, 2, target, &[frame, id, 0x8000, 0, f.address + 128]);
    }
    let frame = if ctx.cpu.read_reg(ArmReg::R0)? == 0 {
        0
    } else {
        f.get(ctx, 0)?
    };
    f.finish(ctx, frame)
}
fn initial(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let frame = f.arg(ctx, 1)?;
        let doc = f.arg(ctx, 2)?;
        let show = f.arg(ctx, 3)?;
        return f.call_service(ctx, 1, super::document_frames::INITIAL, &[frame, doc, show]);
    }
    f.finish(ctx, 0)
}
fn save_all(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    iterate(ctx, 37, true)
}
fn close_all(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    iterate(ctx, 32, false)
}
fn idle(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    iterate(ctx, 44, false)
}
fn iterate(
    ctx: &mut CallCtx<'_>,
    slot: u32,
    stop_false: bool,
) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        let target = virtual_function(ctx, o, 22)?;
        return f.call_args(ctx, 1, target, &[o]);
    }
    match f.phase(ctx)? {
        1 => put(ctx, f.address + 128, result)?,
        2 => {
            let target = virtual_function(ctx, result, slot)?;
            return f.call_args(ctx, 3, target, &[result]);
        }
        3 if stop_false && result == 0 => return f.finish(ctx, 0),
        3 => (),
        _ => {
            return Err(KernelError::Dispatch(
                "Invalid template iteration continuation".into(),
            ))
        }
    }
    if word(ctx, f.address + 128)? == 0 {
        return f.finish(ctx, u32::from(stop_false));
    }
    let steps = f.get(ctx, 0)?;
    if steps >= 4096 {
        return Err(KernelError::Dispatch(
            "MFC template document iteration exceeded its bound".into(),
        ));
    }
    f.set(ctx, 0, steps + 1)?;
    let target = virtual_function(ctx, o, 23)?;
    f.call_args(ctx, 2, target, &[o, f.address + 128])
}
fn command(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // The extra native delegation is the excluded OLE-container command route.
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let args = [
            f.arg(ctx, 0)?,
            f.arg(ctx, 1)?,
            f.arg(ctx, 2)?,
            f.arg(ctx, 3)?,
            ctx.arg_u32(4)?,
        ];
        return f.call_service(ctx, 1, super::command_routing::BASE, &args);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    f.finish(ctx, result)
}
fn default_title(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    let text = super::callbacks::ScopedString::at(&f, 128, 192);
    if f.fresh {
        text.begin(ctx, &f)?;
        let target = virtual_function(ctx, o, 26)?;
        return f.call_args(ctx, 1, target, &[o, text.cell, 1]);
    }
    if let Some(outcome) = text.rethrow(ctx, &f)? {
        return Ok(outcome);
    }
    if f.phase(ctx)? == 1 {
        let mut units = strings::value(ctx, text.cell)?;
        if ctx.cpu.read_reg(ArmReg::R0)? == 0 || units.is_empty() {
            units = super::resources::framework_string(ctx, 0xf003)?;
        }
        let p = super::application::duplicate_wide(ctx, &units)?;
        f.own(ctx, p)?;
        let doc = f.arg(ctx, 1)?;
        let target = virtual_function(ctx, doc, 21)?;
        return f.call_args(ctx, 2, target, &[doc, p]);
    }
    text.end(ctx)?;
    f.finish(ctx, 0)
}
// Match/Open use dedicated continuation adapters to keep ownership transitions
// separate from resource loading and list iteration.
fn match_type(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    super::document_open::match_type(ctx)
}
fn open_single(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    super::document_open::single(ctx)
}
