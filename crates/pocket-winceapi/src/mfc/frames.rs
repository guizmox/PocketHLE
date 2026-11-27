//! CE frame construction and the module-thread's intrusive frame list.
//! Native CFrameWnd links through offset 0x6c in AFX_MODULE_THREAD_STATE's list.
use super::*;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (2045, "?OnUpdateFrameTitle@CFrameWnd@@UAAXH@Z", update_title),
    (103, "??0CFrameWnd@@QAA@XZ", construct),
    (244, "??1CFrameWnd@@UAA@XZ", destruct),
    (
        1475,
        "?GetRuntimeClass@CFrameWnd@@UBAPAUCRuntimeClass@@XZ",
        runtime,
    ),
    (575, "?AddFrameWnd@CFrameWnd@@IAAXXZ", add),
    (2199, "?RemoveFrameWnd@CFrameWnd@@IAAXXZ", remove),
];
fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    delete(ctx, false)
}
pub(super) fn delete(
    ctx: &mut CallCtx<'_>,
    deleting: bool,
) -> Result<DispatchOutcome, KernelError> {
    let f = super::callbacks::Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu.check_guest_access(
            object,
            232,
            pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
        )?;
        let flags = f.arg(ctx, 1)?;
        if deleting && flags & 2 != 0 {
            return Err(KernelError::Dispatch(
                "MFC CFrameWnd vector deletion is not implemented".into(),
            ));
        }
        f.set(ctx, 0, u32::from(deleting && flags & 1 != 0))?;
        let table = super::classes::vtable(ctx, "CFrameWnd")?;
        put(ctx, object, table)?;
        ctx.cpu.write_arg(0, object)?;
        remove(ctx)?;
        let thread = super::application::current_thread_state(ctx)?;
        if word(ctx, thread + 0x12c)? == object {
            put(ctx, thread + 0x12c, 0)?;
        }
        let help = word(ctx, object + 0xb8)?;
        if help != 0 {
            ctx.kernel.heap.free(help);
            put(ctx, object + 0xb8, 0)?;
        }
        release_string(ctx, object + 0xc0)?;
        super::lists::clear(ctx, object + 0x80)?;
        let base = super::classes::vtable(ctx, "CObject")?;
        put(ctx, object + 0x80, base)?;
        return f.call_service(ctx, 1, super::window_lifetime::DESTRUCT, &[object]);
    }
    if f.get(ctx, 0)? != 0 {
        ctx.kernel.heap.free(object);
    }
    f.finish(ctx, object)
}

fn thread_list(ctx: &mut CallCtx<'_>, object: u32) -> Result<u32, KernelError> {
    let saved = word(ctx, object + 0x1c)?;
    if saved != super::application::state(ctx)? {
        return Err(KernelError::Dispatch(
            "MFC frame has an unknown module state".into(),
        ));
    }
    Ok(super::application::module_thread(ctx)? + 8)
}
fn link(ctx: &mut CallCtx<'_>, object: u32, list: u32) -> Result<(), KernelError> {
    let offset = word(ctx, list + 4)?;
    if offset != 0x6c {
        return Err(KernelError::Dispatch(
            "Invalid MFC frame-list link offset".into(),
        ));
    }
    let head = word(ctx, list)?;
    put(ctx, object + offset, head)?;
    put(ctx, list, object)
}
fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    ctx.cpu.check_guest_access(
        object,
        232,
        pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
    )?;
    let table = super::classes::vtable(ctx, "CFrameWnd")?;
    let list = super::application::module_thread(ctx)? + 8;
    // Only fields initialized by the CE constructor are written. Derived/unused
    // fields are not replaced by a blanket memset of the whole frame object.
    super::windows::construct_target_class(ctx, "CWnd", 72)?;
    super::lists::initialize(ctx, object + 0x80, 10)?;
    new_string(ctx, object + 0xc0, &[])?;
    put(ctx, object, table)?;
    for offset in [
        0x5c, 0x60, 0x64, 0x68, 0xa0, 0xa4, 0xa8, 0xac, 0xb0, 0xb4, 0xb8, 0xbc, 0xc4, 0xc8, 0xd0,
        0xd4, 0xe4,
    ] {
        put(ctx, object + offset, 0)?;
    }
    ctx.cpu.write_mem(object + 0x70, &[0; 16])?;
    for (offset, value) in [
        (0x48, object),
        (0x54, 1),
        (0x58, u32::MAX),
        (0x9c, u32::MAX),
        (0xcc, 12),
    ] {
        put(ctx, object + offset, value)?;
    }
    if let Err(e) = link(ctx, object, list) {
        release_string(ctx, object + 0xc0)?;
        return Err(e);
    }
    Ok(DispatchOutcome::ReturnedR0(object))
}
fn add(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let list = thread_list(ctx, object)?;
    link(ctx, object, list)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn remove(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let list = thread_list(ctx, object)?;
    let offset = word(ctx, list + 4)?;
    if offset != 0x6c {
        return Err(KernelError::Dispatch(
            "Invalid MFC frame-list link offset".into(),
        ));
    }
    let mut previous = list;
    let mut at = word(ctx, list)?;
    let mut seen = std::collections::HashSet::new();
    while at != 0 {
        if !seen.insert(at) {
            return Err(KernelError::Dispatch("Cyclic MFC frame list".into()));
        }
        let link = at
            .checked_add(offset)
            .ok_or_else(|| KernelError::Dispatch("MFC frame address overflow".into()))?;
        let next = word(ctx, link)?;
        if at == object {
            put(ctx, previous, next)?;
            return Ok(DispatchOutcome::ReturnedR0(0));
        }
        previous = link;
        at = next;
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx,
        "CFrameWnd",
    )?))
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    fn release_parts(
        cpu: &mut pocket_cpu::stub::StubCpu,
        k: &mut pocket_kernel::KernelState,
        d: &mut crate::WinCeDispatcher,
        object: u32,
    ) {
        cpu.write_arg(0, object).unwrap();
        super::super::window_dispatch::tests::drive(
            cpu,
            k,
            d,
            super::super::window_dispatch::tests::entry(244),
            |pc, _| panic!("unexpected guest {pc:x}"),
        );
    }
    #[test]
    fn frame_destructor_releases_its_embedded_owners_help_storage_and_thread_cache() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let thread = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:651",
            &[],
        ));
        let module = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:645",
            &[],
        ));
        let mut baseline = None;
        for _ in 0..128 {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:103",
                &[0x2600],
            );
            let help = result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:315",
                &[128],
            ));
            cpu.write_mem(0x26b8, &help.to_le_bytes()).unwrap();
            cpu.write_mem(thread + 0x12c, &0x2600u32.to_le_bytes())
                .unwrap();
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:594",
                &[0x2680, 0x1234],
            );
            release_parts(&mut cpu, &mut k, &mut d, 0x2600);
            assert_eq!(cpu.read_u32_le(thread + 0x12c).unwrap(), 0);
            assert_eq!(cpu.read_u32_le(module + 8).unwrap(), 0);
            assert_eq!(cpu.read_u32_le(0x2684).unwrap(), 0);
            let free = k.heap.free_bytes();
            assert_eq!(*baseline.get_or_insert(free), free);
        }
    }
    #[test]
    fn frame_layout_embedded_owners_and_intrusive_lists_are_isolated_per_thread() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:103",
            &[0x2600],
        );
        release_parts(&mut cpu, &mut k, &mut d, 0x2600);
        let thread0 = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:645",
            &[],
        ));
        k.current_thread = 1;
        let thread1 = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:645",
            &[],
        ));
        // Destruction consults AFX_THREAD_STATE as well as MODULE_THREAD_STATE.
        // Warm the second thread's persistent owner before measuring lifetimes.
        call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:651", &[]);
        k.current_thread = 0;
        let baseline = k.heap.free_bytes();
        cpu.write_mem(0x1ffc, &[0xa5; 240]).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:103",
            &[0x2000],
        );
        assert_eq!(cpu.read_mem(0x1ffc, 4).unwrap(), [0xa5; 4]);
        assert_eq!(cpu.read_mem(0x20e8, 4).unwrap(), [0xa5; 4]);
        assert_eq!(cpu.read_u32_le(0x2048).unwrap(), 0x2000);
        assert_eq!(cpu.read_u32_le(0x2054).unwrap(), 1);
        assert_eq!(cpu.read_u32_le(0x2058).unwrap(), u32::MAX);
        assert_eq!(cpu.read_u32_le(0x209c).unwrap(), u32::MAX);
        assert_eq!(cpu.read_u32_le(0x20cc).unwrap(), 12);
        assert_eq!(cpu.read_u32_le(0x2098).unwrap(), 10);
        assert_eq!(cpu.read_mem(0x2084, 20).unwrap(), [0; 20]);
        for offset in [0x4c, 0x50, 0xd8, 0xdc, 0xe0] {
            assert_eq!(cpu.read_mem(0x2000 + offset, 4).unwrap(), [0xa5; 4]);
        }
        let string = cpu.read_u32_le(0x20c0).unwrap();
        assert_eq!(cpu.read_u32_le(string - 8).unwrap(), 0);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:103",
            &[0x2200],
        );
        assert_eq!(cpu.read_u32_le(thread0 + 8).unwrap(), 0x2200);
        assert_eq!(cpu.read_u32_le(0x226c).unwrap(), 0x2000);
        assert_eq!(cpu.read_u32_le(0x206c).unwrap(), 0);
        k.current_thread = 1;
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:103",
            &[0x2400],
        );
        assert_eq!(cpu.read_u32_le(thread1 + 8).unwrap(), 0x2400);
        assert_eq!(cpu.read_u32_le(thread0 + 8).unwrap(), 0x2200);
        release_parts(&mut cpu, &mut k, &mut d, 0x2400);
        k.current_thread = 0;
        release_parts(&mut cpu, &mut k, &mut d, 0x2000);
        assert_eq!(cpu.read_u32_le(thread0 + 8).unwrap(), 0x2200);
        assert_eq!(cpu.read_u32_le(0x226c).unwrap(), 0);
        release_parts(&mut cpu, &mut k, &mut d, 0x2200);
        assert_eq!(cpu.read_u32_le(thread0 + 8).unwrap(), 0);
        assert_eq!(k.heap.free_bytes(), baseline);
    }
}

fn update_title(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    use super::callbacks::{service, virtual_function, Frame};
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    if !f.fresh && f.phase(ctx)? == 2 {
        return f.finish(ctx, 0);
    }
    let hwnd = word(ctx, object + 0x20)?;
    let style = ctx.kernel.window_styles.get(&hwnd).map_or(0, |v| v.0);
    if f.fresh {
        if style & 0x8000 == 0 {
            return f.finish(ctx, 0);
        }
        let target = virtual_function(ctx, object, 47)?;
        return f.call_args(ctx, 1, target, &[object]);
    }
    let doc = ctx.cpu.read_reg(pocket_cpu::regs::ArmReg::R0)?;
    let base = super::strings::value(ctx, object + 0xc0)?;
    let mut text = base.clone();
    if f.arg(ctx, 1)? != 0 && doc != 0 {
        let mut title = super::strings::value(ctx, doc + 0x20)?;
        let number = word(ctx, object + 0x58)? as i32;
        if number > 0 {
            title.extend(format!(":{number}").encode_utf16());
        }
        let separator: Vec<_> = " - ".encode_utf16().collect();
        text = if style & 0x4000 != 0 {
            title.into_iter().chain(separator).chain(base).collect()
        } else {
            base.into_iter().chain(separator).chain(title).collect()
        };
    }
    let buffer = alloc(ctx, (text.len() as u32 + 1) * 2)?;
    f.own(ctx, buffer)?;
    ctx.cpu.write_mem(
        buffer,
        &text
            .into_iter()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )?;
    let target = service(ctx, "SetWindowTextW")?;
    f.call_args(ctx, 2, target, &[hwnd, buffer])
}
