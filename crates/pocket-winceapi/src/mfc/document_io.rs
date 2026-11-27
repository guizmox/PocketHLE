//! Document serialization shares CArchive, CMirrorFile and CE3 exception owners.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        1290,
        "?GetFile@CDocument@@UAAPAVCFile@@PBGIPAVCFileException@@@Z",
        get_file,
    ),
    (
        2168,
        "?ReleaseFile@CDocument@@UAAXPAVCFile@@H@Z",
        release_file,
    ),
    (1973, "?OnOpenDocument@CDocument@@UAAHPBG@Z", open),
    (2001, "?OnSaveDocument@CDocument@@UAAHPBG@Z", save),
    (
        2224,
        "?ReportSaveLoadException@CDocument@@UAAXPBGPAVCException@@HI@Z",
        report,
    ),
];
fn get_file(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let file = alloc_cpp(ctx, 20)?;
        f.own(ctx, file)?;
        f.set(ctx, 0, file)?;
        return f.call_service(ctx, 1, super::mirror_file::NEW, &[file]);
    }
    let file = f.get(ctx, 0)?;
    match f.phase(ctx)? {
        1 => {
            let name = f.arg(ctx, 1)?;
            let mode = f.arg(ctx, 2)?;
            let error = f.arg(ctx, 3)?;
            f.call_virtual(ctx, 2, file, 10, &[name, mode, error])
        }
        2 => {
            let result = ctx.cpu.read_reg(ArmReg::R0)?;
            f.disown(ctx)?;
            if result != 0 {
                return f.finish(ctx, file);
            }
            f.call_virtual(ctx, 3, file, 1, &[1])
        }
        3 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid document file construction continuation".into(),
        )),
    }
}
fn release_file(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let file = f.arg(ctx, 1)?;
    if f.fresh {
        let abort = f.arg(ctx, 2)? != 0;
        return f.call_virtual(ctx, 1, file, if abort { 16 } else { 18 }, &[]);
    }
    if f.phase(ctx)? == 1 {
        return f.call_virtual(ctx, 2, file, 1, &[1]);
    }
    f.finish(ctx, 0)
}
fn open(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    serialize(ctx, false)
}
fn save(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    serialize(ctx, true)
}
fn report_failure(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    saving: bool,
    exception: u32,
    phase: u32,
) -> Result<DispatchOutcome, KernelError> {
    let doc = f.arg(ctx, 0)?;
    let path = f.arg(ctx, 1)?;
    let id = if saving { 0xf102 } else { 0xf101 };
    f.call_virtual(
        ctx,
        phase,
        doc,
        33,
        &[path, exception, u32::from(saving), id],
    )
}
fn serialize(ctx: &mut CallCtx<'_>, saving: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let doc = f.arg(ctx, 0)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        let local = alloc(ctx, 128)?;
        f.own(ctx, local)?;
        f.set(ctx, 0, local)?;
        ctx.cpu.write_mem(local, &[0; 128])?;
        super::io_exceptions::initialize_file(ctx, local)?;
        super::exception_links::begin_catch(ctx, &f, 192)?;
        f.set(ctx, 2, 1)?;
        let path = f.arg(ctx, 1)?;
        let mode = if saving { 0x1012 } else { 0x20 };
        return f.call_virtual(ctx, 1, doc, 34, &[path, mode, local]);
    }
    let local = f.get(ctx, 0)?;
    let archive = local + 32;
    // The native TRY encloses Serialize/Close/Release. Its caught exception stays
    // owned by this link until file abort, reporting and archive destruction finish.
    let caught = if f.get(ctx, 2)? != 0 {
        word(ctx, f.address + 192 + 52)?
    } else {
        0
    };
    if caught != 0 && f.phase(ctx)? < 90 {
        f.set(ctx, 4, caught)?;
        f.set(ctx, 3, 0)?;
        let phase = f.phase(ctx)?;
        f.set(ctx, 7, u32::from(phase <= 4 || phase == 80))?;
        f.set(ctx, 5, 0)?;
        let file = f.get(ctx, 1)?;
        if file != 0 {
            return f.call_virtual(ctx, 90, doc, 35, &[file, 1]);
        }
        return exceptional_contents(ctx, f, saving);
    }
    match f.phase(ctx)? {
        1 => {
            if result == 0 {
                return report_failure(ctx, f, saving, local, 80);
            }
            f.set(ctx, 1, result)?;
            if saving {
                return construct_archive(ctx, f, saving);
            }
            f.call_virtual(ctx, 2, doc, 28, &[])
        }
        2 => f.call_virtual(ctx, 3, doc, 24, &[1]),
        3 => construct_archive(ctx, f, saving),
        4 => {
            put(ctx, archive, doc)?;
            put(ctx, archive + 4, 0)?;
            f.set(ctx, 6, 1)?;
            if saving {
                return begin_serialize(ctx, f, saving);
            }
            let file = f.get(ctx, 1)?;
            f.call_virtual(ctx, 5, file, 13, &[])
        }
        5 => {
            if result != 0 {
                begin_serialize(ctx, f, saving)
            } else {
                f.call_service(ctx, 7, super::archive::CLOSE, &[archive])
            }
        }
        6 => f.call_service(ctx, 7, super::archive::CLOSE, &[archive]),
        7 => {
            let file = f.get(ctx, 1)?;
            f.call_virtual(ctx, 8, doc, 35, &[file, 0])
        }
        8 => {
            f.set(ctx, 1, 0)?;
            if f.get(ctx, 5)? != 0 {
                let module = super::application::state(ctx)?;
                let app = word(ctx, module + 4)?;
                return f.call_virtual(ctx, 12, app, 36, &[u32::MAX]);
            }
            clear_modified(ctx, f)
        }
        11 => {
            f.set(ctx, 5, 1)?;
            let local = f.get(ctx, 0)?;
            f.call_virtual(ctx, 6, doc, 2, &[local + 32])
        }
        12 => {
            f.set(ctx, 5, 0)?;
            clear_modified(ctx, f)
        }
        9 => finish_archive(ctx, f),
        10 => finish(ctx, f),
        80 => finish(ctx, f),
        90 => {
            f.set(ctx, 1, 0)?;
            exceptional_contents(ctx, f, saving)
        }
        91 => exceptional_report(ctx, f, saving),
        92 => {
            let replacement = word(ctx, f.address + 128 + 52)?;
            super::exception_links::unlink(ctx, f.address + 128)?;
            if replacement != 0 {
                return f.call_service(ctx, 93, super::exception_links::DELETE, &[replacement]);
            }
            cleanup_exception(ctx, f)
        }
        93 => cleanup_exception(ctx, f),
        94 => finish_archive(ctx, f),
        _ => Err(KernelError::Dispatch(
            "Invalid document serialization/cleanup continuation".into(),
        )),
    }
}
fn construct_archive(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    saving: bool,
) -> Result<DispatchOutcome, KernelError> {
    let p = f.get(ctx, 0)?;
    let file = f.get(ctx, 1)?;
    f.call_service(
        ctx,
        4,
        super::archive::CONSTRUCT,
        &[p + 32, file, if saving { 2 } else { 3 }, 4096, 0],
    )
}
fn begin_serialize(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    _saving: bool,
) -> Result<DispatchOutcome, KernelError> {
    let module = super::application::state(ctx)?;
    let app = word(ctx, module + 4)?;
    if app != 0 {
        return f.call_virtual(ctx, 11, app, 36, &[1]);
    }
    let doc = f.arg(ctx, 0)?;
    let p = f.get(ctx, 0)?;
    f.call_virtual(ctx, 6, doc, 2, &[p + 32])
}
fn clear_modified(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let doc = f.arg(ctx, 0)?;
    f.set(ctx, 3, 1)?;
    f.call_virtual(ctx, 9, doc, 24, &[0])
}
fn finish_archive(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    if f.get(ctx, 6)? == 0 {
        return finish(ctx, f);
    }
    let p = f.get(ctx, 0)?;
    f.call_service(ctx, 10, super::archive::DESTRUCT, &[p + 32])
}
fn exceptional_contents(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    saving: bool,
) -> Result<DispatchOutcome, KernelError> {
    if f.get(ctx, 7)? != 0 {
        return finish_archive(ctx, f);
    }
    if !saving {
        let doc = f.arg(ctx, 0)?;
        return f.call_virtual(ctx, 91, doc, 28, &[]);
    }
    exceptional_report(ctx, f, saving)
}
fn exceptional_report(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    saving: bool,
) -> Result<DispatchOutcome, KernelError> {
    super::exception_links::begin_catch(ctx, &f, 128)?;
    let exception = f.get(ctx, 4)?;
    report_failure(ctx, f, saving, exception, 92)
}
fn cleanup_exception(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let exception = f.get(ctx, 4)?;
    put(ctx, f.address + 192 + 52, 0)?;
    f.set(ctx, 4, 0)?;
    f.call_service(ctx, 94, super::exception_links::DELETE, &[exception])
}
fn finish(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    if f.get(ctx, 2)? != 0 {
        super::exception_links::unlink(ctx, f.address + 192)?;
    }
    let p = f.get(ctx, 0)?;
    release_string(ctx, p + 16)?;
    if f.get(ctx, 7)? != 0 {
        let exception = f.get(ctx, 4)?;
        put(ctx, f.address + 192 + 52, 0)?;
        return f.call_service(ctx, 0xffff, super::exception_throw::THROW, &[exception]);
    }
    let result = f.get(ctx, 3)?;
    f.finish(ctx, result)
}
fn report(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let exception = f.arg(ctx, 2)?;
    if f.fresh {
        if exception != 0 {
            let class = super::classes::runtime(ctx, "CUserException")?;
            return f.call_service(ctx, 1, super::class_queries::KIND, &[exception, class]);
        }
        return generic_report(ctx, f);
    }
    match f.phase(ctx)? {
        1 => {
            if ctx.cpu.read_reg(ArmReg::R0)? != 0 {
                return f.finish(ctx, 0);
            }
            let p = alloc(ctx, 516)?;
            f.own(ctx, p)?;
            f.set(ctx, 0, p)?;
            put(ctx, p + 512, 0)?;
            f.call_virtual(ctx, 2, exception, 5, &[p, 256, p + 512])
        }
        2 => {
            let p = f.get(ctx, 0)?;
            if ctx.cpu.read_reg(ArmReg::R0)? == 0 {
                return generic_report(ctx, f);
            }
            let id = word(ctx, p + 512)?;
            f.call_service(ctx, 3, super::message_box::TEXT, &[p, 0x30, id])
        }
        3 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid save/load error report continuation".into(),
        )),
    }
}
fn generic_report(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let path = f.arg(ctx, 1)?;
    let path = String::from_utf16_lossy(&strings::wide(ctx, path)?);
    let saving = f.arg(ctx, 3)? != 0;
    let message = format!(
        "Could not {} {}.",
        if saving { "save" } else { "open" },
        path
    );
    let existing = f.get(ctx, 0)?;
    let pointer = if existing == 0 {
        let p =
            super::application::duplicate_wide(ctx, &message.encode_utf16().collect::<Vec<_>>())?;
        f.own(ctx, p)?;
        p
    } else {
        let data: Vec<_> = message
            .encode_utf16()
            .take(255)
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        ctx.cpu.write_mem(existing, &data)?;
        existing
    };
    f.call_service(ctx, 3, super::message_box::TEXT, &[pointer, 0x30, 0])
}
