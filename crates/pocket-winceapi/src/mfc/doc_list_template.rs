//! CE document-list templates share the SDI document/open engine. Only the
//! background-frame and list transitions differ from CSingleDocTemplate.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
const OPEN_CORE: &str = "__PocketHLE_MFC_DocListOpenCore";
const CONSTRUCT_BASE: &str = "__PocketHLE_MFC_SingleTemplateConstruct";
const CREATE_LIST: &str = "__PocketHLE_MFC_CreateDocListWindow";
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", OPEN_CORE, open_core);
}
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        3553,
        "??0CCeDocListDocTemplate@@QAA@IPAUCRuntimeClass@@00AAVCString@@1@Z",
        construct,
    ),
    (2698, "??1CCeDocListDocTemplate@@UAA@XZ", destruct),
    (
        2713,
        "?CloseAllDocuments@CCeDocListDocTemplate@@UAAXH@Z",
        close_all,
    ),
    (
        2719,
        "?CreateNewFrame@CCeDocListDocTemplate@@UAAPAVCFrameWnd@@PAVCDocument@@PAV2@@Z",
        create_frame,
    ),
    (
        2769,
        "?OpenDocumentFile@CCeDocListDocTemplate@@UAAPAVCDocument@@PBGH@Z",
        open,
    ),
    (
        2772,
        "?RemoveDocument@CCeDocListDocTemplate@@UAAXPAVCDocument@@@Z",
        remove,
    ),
    (2783, "?ShowDocList@CCeDocListDocTemplate@@UAAXXZ", show),
];
fn base(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    phase: u32,
    slot: u32,
    args: &[u32],
) -> Result<DispatchOutcome, KernelError> {
    let table = classes::vtable(ctx, "CSingleDocTemplate")?;
    let target = word(ctx, table + slot * 4)?;
    f.call_args(ctx, phase, target, args)
}
fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let view = ctx.arg_u32(4)?;
        let filter = ctx.arg_u32(5)?;
        let folder = ctx.arg_u32(6)?;
        f.set(ctx, 0, filter)?;
        f.set(ctx, 1, folder)?;
        let resource = f.arg(ctx, 1)?;
        let document = f.arg(ctx, 2)?;
        let frame = f.arg(ctx, 3)?;
        return f.call_service(
            ctx,
            1,
            CONSTRUCT_BASE,
            &[object, resource, document, frame, view],
        );
    }
    let filter = f.get(ctx, 0)?;
    let folder = f.get(ctx, 1)?;
    let filter = strings::value(ctx, filter)?;
    let folder = strings::value(ctx, folder)?;
    // CE3 resources FF02/FF03 are "All Folders" and "\\My Documents".
    // +74 is the selected-folder display path, not another file filter.
    let mut display_path = "\\My Documents".encode_utf16().collect::<Vec<_>>();
    if String::from_utf16_lossy(&folder) != "All Folders" {
        display_path.push(b'\\' as u16);
        display_path.extend_from_slice(&folder);
    }
    let mut initialized = 0;
    for (offset, text) in [
        (0x6c, filter.as_slice()),
        (0x74, display_path.as_slice()),
        (0x78, folder.as_slice()),
    ] {
        if let Err(error) = new_string(ctx, object + offset, text) {
            for offset in [0x6c, 0x74, 0x78].into_iter().take(initialized) {
                release_string(ctx, object + offset)?;
            }
            return Err(error);
        }
        initialized += 1;
    }
    let list = alloc_cpp(ctx, 628)?;
    if let Err(error) = super::doc_list::construct_value(ctx, list, object) {
        ctx.kernel.heap.free(list);
        return Err(error);
    }
    put(ctx, object + 0x70, list)?;
    let table = classes::vtable(ctx, "CCeDocListDocTemplate")?;
    put(ctx, object, table)?;
    f.finish(ctx, object)
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
                "MFC document-list template vector deletion unsupported".into(),
            ));
        }
        f.set(ctx, 0, u32::from(deleting && flags & 1 != 0))?;
        let list = word(ctx, object + 0x70)?;
        put(ctx, object + 0x70, 0)?;
        if list != 0 {
            return f.call_virtual(ctx, 1, list, 1, &[1]);
        }
    }
    if f.fresh || f.phase(ctx)? == 1 {
        for offset in [0x78, 0x74, 0x6c] {
            release_string(ctx, object + offset)?;
        }
        return base(ctx, &f, 2, 1, &[object, 0]);
    }
    if f.get(ctx, 0)? != 0 {
        ctx.kernel.heap.free(object);
    }
    f.finish(ctx, object)
}
fn create_frame(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_virtual(ctx, 1, object, 28, &[]);
    }
    match f.phase(ctx)? {
        1 => {
            let document = ctx.cpu.read_reg(ArmReg::R0)?;
            if document == 0 {
                return f.finish(ctx, 0);
            }
            f.set(ctx, 0, document)?;
            put(ctx, document + 0x54, 1)?;
            base(ctx, &f, 2, 29, &[object, document, 0])
        }
        2 => {
            let frame = ctx.cpu.read_reg(ArmReg::R0)?;
            f.set(ctx, 1, frame)?;
            let document = f.get(ctx, 0)?;
            if frame == 0 {
                return f.call_virtual(ctx, 5, document, 1, &[1]);
            }
            let thread = application::thread_object(ctx)?;
            if thread != 0 && word(ctx, thread + 0x20)? == 0 {
                put(ctx, thread + 0x20, frame)?;
            }
            f.call_virtual(ctx, 3, object, 30, &[frame, document, 1])
        }
        3 => {
            let document = f.get(ctx, 0)?;
            f.call_virtual(ctx, 4, object, 25, &[document])
        }
        4 => {
            let document = f.get(ctx, 0)?;
            f.call_virtual(ctx, 5, document, 1, &[1])
        }
        _ => {
            let frame = f.get(ctx, 1)?;
            f.finish(ctx, frame)
        }
    }
}
fn open_core(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    document_open::single_with_background(ctx)
}
fn open(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let object = f.arg(ctx, 0)?;
        let path = f.arg(ctx, 1)?;
        let visible = f.arg(ctx, 2)?;
        return f.call_service(ctx, 1, OPEN_CORE, &[object, path, visible]);
    }
    let document = ctx.cpu.read_reg(ArmReg::R0)?;
    f.finish(ctx, document)
}
fn remove(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let document = f.arg(ctx, 1)?;
    if f.fresh {
        return base(ctx, &f, 1, 25, &[object, document]);
    }
    if f.phase(ctx)? == 1
        && word(ctx, document + 0x54)? == 0
        && ctx.kernel.process_exit_code.is_none()
    {
        let thread = application::thread_object(ctx)?;
        if thread != 0 && word(ctx, thread + 0x20)? != 0 {
            return f.call_virtual(ctx, 2, object, 36, &[]);
        }
    }
    f.finish(ctx, 0)
}
fn close_all(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        let end_session = f.arg(ctx, 1)?;
        return base(ctx, &f, 1, 32, &[object, end_session]);
    }
    if f.phase(ctx)? == 1 {
        let list = word(ctx, object + 0x70)?;
        if list != 0 && word(ctx, list + 0x20)? != 0 {
            return f.call_virtual(ctx, 2, list, 47, &[]);
        }
    }
    f.finish(ctx, 0)
}
fn show(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let phase = if f.fresh { 0 } else { f.phase(ctx)? };
    if phase <= 1 {
        let thread = application::thread_object(ctx)?;
        let frame = if thread == 0 {
            0
        } else {
            word(ctx, thread + 0x20)?
        };
        if frame == 0 && phase == 0 {
            return f.call_virtual(ctx, 1, object, 29, &[0, 0]);
        }
        if frame == 0 {
            return f.finish(ctx, 0);
        }
        let list = word(ctx, object + 0x70)?;
        if list == 0 {
            return Err(KernelError::Dispatch(
                "Document-list template lost its list owner".into(),
            ));
        }
        if word(ctx, list + 0x20)? != 0 {
            return f.finish(ctx, 0);
        }
        let filter = word(ctx, object + 0x6c)?;
        let folder = word(ctx, object + 0x78)?;
        return f.call_service(ctx, 2, CREATE_LIST, &[list, frame, filter, folder, 1]);
    }
    f.finish(ctx, 0)
}
