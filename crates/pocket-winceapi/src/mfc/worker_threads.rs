//! MFC worker startup/teardown uses the shared WinCE threads and event handshake.
use super::callbacks::{service, virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
const CONSTRUCT: &str = "__PocketHLE_MFC_ConstructWorker";
const CREATE: &str = "__PocketHLE_MFC_CreateWorker";
const START: &str = "__PocketHLE_MFC_WorkerStart";
const END: &str = "__PocketHLE_MFC_WorkerEnd";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        605,
        "?AfxBeginThread@@YAPAVCWinThread@@PAUCRuntimeClass@@HIKPAU_SECURITY_ATTRIBUTES@@@Z",
        begin_ui,
    ),
    (1636, "?InitInstance@CWinThread@@UAAHXZ", init_ui),
    (203, "??0CWinThread@@QAA@P6AIPAX@Z0@Z", construct_worker),
    (204, "??0CWinThread@@QAA@XZ", construct_ui),
    (312, "??1CWinThread@@UAA@XZ", destruct),
    (
        604,
        "?AfxBeginThread@@YAPAVCWinThread@@P6AIPAX@Z0HIKPAU_SECURITY_ATTRIBUTES@@@Z",
        begin,
    ),
    (623, "?AfxEndThread@@YAXIH@Z", end),
    (
        986,
        "?CreateThread@CWinThread@@QAAHKIPAU_SECURITY_ATTRIBUTES@@@Z",
        create,
    ),
    (1030, "?Delete@CWinThread@@UAAXXZ", delete),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    for (name, handler) in [
        (CONSTRUCT, construct_worker as crate::Handler),
        (CREATE, create),
        (START, start),
        (END, end),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}
fn root(ctx: &CallCtx<'_>) -> String {
    format!("__PocketHLE_MFC_Worker:{}", ctx.kernel.current_thread)
}
fn construct_worker(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct(ctx, true)
}
fn construct_ui(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct(ctx, false)
}
fn construct(ctx: &mut CallCtx<'_>, worker: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu.write_arg(0, o)?;
        super::windows::construct_target_class(ctx, "CWinThread", 104)?;
        if worker {
            let entry = f.arg(ctx, 1)?;
            let param = f.arg(ctx, 2)?;
            put(ctx, o + 0x54, entry)?;
            put(ctx, o + 0x50, param)?;
        }
        put(ctx, o + 0x28, 1)?;
        return f.call_service(ctx, 1, "InitializeCriticalSection", &[o + 0x5c]);
    }
    f.finish(ctx, o)
}
fn init_ui(_: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // This FALSE is the retail base virtual. A UI thread needs the actual
    // derived InitInstance to create its windows and enter Run.
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn begin_ui(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    begin_kind(ctx, true)
}
fn begin(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    begin_kind(ctx, false)
}
fn begin_kind(ctx: &mut CallCtx<'_>, ui: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let entry = f.arg(ctx, 0)?;
        if entry == 0 {
            return f.finish(ctx, 0);
        }
        let flags = ctx.arg_u32(if ui { 3 } else { 4 })?;
        let security = ctx.arg_u32(if ui { 4 } else { 5 })?;
        f.set(ctx, 1, flags)?;
        f.set(ctx, 2, security)?;
        if ui {
            return f.call_service(ctx, 10, super::class_queries::CREATE, &[entry]);
        }
        let o = alloc_cpp(ctx, 104)?;
        f.set(ctx, 0, o)?;
        let param = f.arg(ctx, 1)?;
        return f.call_service(ctx, 1, CONSTRUCT, &[o, entry, param]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    let phase = f.phase(ctx)?;
    if phase == 10 {
        if result == 0 {
            return f.call_service(ctx, 11, super::exception_throw::MEMORY, &[]);
        }
        ctx.cpu
            .check_guest_access(result, 104, Prot::READ | Prot::WRITE)?;
        f.set(ctx, 0, result)?;
        put(ctx, result + 0x50, 0)?;
    }
    let o = f.get(ctx, 0)?;
    match phase {
        1 | 10 => {
            let flags = f.get(ctx, 1)? | 4;
            let stack = f.arg(ctx, if ui { 2 } else { 3 })?;
            let sec = f.get(ctx, 2)?;
            f.call_service(ctx, 2, CREATE, &[o, flags, stack, sec])
        }
        2 if result == 0 => {
            let target = virtual_function(ctx, o, 30)?;
            f.call_args(ctx, 5, target, &[o])
        }
        2 => {
            let handle = word(ctx, o + 0x2c)?;
            let priority = f.arg(ctx, if ui { 1 } else { 2 })?;
            f.call_service(ctx, 3, "SetThreadPriority", &[handle, priority])
        }
        3 if f.get(ctx, 1)? & 4 == 0 => {
            let h = word(ctx, o + 0x2c)?;
            f.call_service(ctx, 4, "ResumeThread", &[h])
        }
        3 | 4 => f.finish(ctx, o),
        5 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC AfxBeginThread continuation".into(),
        )),
    }
}
fn create_failed(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    for slot in [1, 2] {
        let handle = f.get(ctx, slot)?;
        if handle != 0 {
            crate::coredll::close_handle_value(ctx, handle)?;
        }
    }
    let data = f.get(ctx, 3)?;
    if data != 0 {
        ctx.kernel.heap.free(data);
    }
    f.finish(ctx, 0)
}
fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu
            .check_guest_access(o, 104, Prot::READ | Prot::WRITE)?;
        if word(ctx, o + 0x2c)? != 0 {
            return f.finish(ctx, 0);
        }
        return f.call_service(ctx, 1, "CreateEventW", &[0, 1, 0, 0]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            if result == 0 {
                return create_failed(ctx, f);
            }
            f.set(ctx, 1, result)?;
            f.call_service(ctx, 2, "CreateEventW", &[0, 1, 0, 0])
        }
        2 => {
            if result == 0 {
                return create_failed(ctx, f);
            }
            f.set(ctx, 2, result)?;
            let data = match alloc_cpp(ctx, 20) {
                Ok(data) => data,
                Err(error) => {
                    // These events were created before the guest allocation;
                    // an allocation exception must not strand their handles.
                    for slot in [1, 2] {
                        let handle = f.get(ctx, slot)?;
                        crate::coredll::close_handle_value(ctx, handle)?;
                        f.set(ctx, slot, 0)?;
                    }
                    return Err(error);
                }
            };
            f.set(ctx, 3, data)?;
            let module = word(ctx, o + 0x1c)?;
            let app = word(ctx, module + 4)?;
            let main = if app == 0 { 0 } else { word(ctx, app + 0x20)? };
            let hwnd = if main == 0 {
                0
            } else {
                word(ctx, main + 0x20)?
            };
            let init = f.get(ctx, 1)?;
            for (index, value) in [o, init, result, module, hwnd].into_iter().enumerate() {
                put(ctx, data + index as u32 * 4, value)?;
            }
            let stack = f.arg(ctx, 2)?;
            let flags = f.arg(ctx, 1)? | 4;
            let sec = f.arg(ctx, 3)?;
            let entry = service(ctx, START)?;
            f.call_service(
                ctx,
                3,
                "CreateThread",
                &[sec, stack, entry, data, flags, o + 0x30],
            )
        }
        3 => {
            if result == 0 {
                return create_failed(ctx, f);
            }
            put(ctx, o + 0x2c, result)?;
            f.call_service(ctx, 4, "ResumeThread", &[result])
        }
        4 => {
            if result == u32::MAX {
                return Err(KernelError::Dispatch(
                    "MFC worker initialization could not resume".into(),
                ));
            }
            let init = f.get(ctx, 1)?;
            f.call_service(ctx, 5, "WaitForSingleObject", &[init, u32::MAX])
        }
        5 => {
            if result != 0 {
                return Err(KernelError::Dispatch(
                    "MFC worker initialization wait failed".into(),
                ));
            }
            let init = f.get(ctx, 1)?;
            crate::coredll::close_handle_value(ctx, init)?;
            f.set(ctx, 1, 0)?;
            let data = f.get(ctx, 3)?;
            ctx.kernel.heap.free(data);
            f.set(ctx, 3, 0)?;
            if f.arg(ctx, 1)? & 4 != 0 {
                let h = word(ctx, o + 0x2c)?;
                return f.call_service(ctx, 6, "SuspendThread", &[h]);
            }
            let release = f.get(ctx, 2)?;
            f.call_service(ctx, 7, "SetEvent", &[release])
        }
        6 => {
            if result == u32::MAX {
                return Err(KernelError::Dispatch(
                    "MFC initialized worker could not suspend".into(),
                ));
            }
            let release = f.get(ctx, 2)?;
            f.call_service(ctx, 7, "SetEvent", &[release])
        }
        7 => {
            if result == 0 {
                return Err(KernelError::Dispatch(
                    "MFC worker release event failed".into(),
                ));
            }
            f.finish(ctx, 1)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC CreateThread continuation".into(),
        )),
    }
}
fn start(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let data = f.arg(ctx, 0)?;
        ctx.cpu.check_guest_access(data, 20, Prot::READ)?;
        let o = word(ctx, data)?;
        for slot in 0..3 {
            let value = word(ctx, data + slot * 4)?;
            f.set(ctx, slot, value)?;
        }
        let module = word(ctx, data + 12)?;
        let hwnd = word(ctx, data + 16)?;
        let mt = super::application::module_thread(ctx)?;
        let ts = super::application::current_thread_state(ctx)?;
        f.set(ctx, 3, mt)?;
        f.set(ctx, 4, ts)?;
        put(ctx, mt + 4, o)?;
        put(ctx, o + 0x1c, module)?;
        let alias = f.address + 128;
        ctx.cpu.write_arg(0, alias)?;
        super::windows::construct_target_class(ctx, "CWnd", 72)?;
        if hwnd != 0 && crate::coredll::window_exists(ctx.kernel, hwnd) && word(ctx, o + 0x20)? == 0
        {
            put(ctx, alias + 0x20, hwnd)?;
            put(ctx, o + 0x20, alias)?;
            f.set(ctx, 5, alias)?;
            let owner = super::application::owner(ctx)?;
            let key = super::window_handles::thread_key(ctx, hwnd);
            ctx.kernel
                .dynamic_exports
                .get_mut(&owner)
                .unwrap()
                .insert(key, alias);
        }
        let owner = super::application::owner(ctx)?;
        let key = root(ctx);
        ctx.kernel
            .dynamic_exports
            .get_mut(&owner)
            .unwrap()
            .insert(key, f.address);
        let init = f.get(ctx, 1)?;
        return f.call_service(ctx, 1, "SetEvent", &[init]);
    }
    match f.phase(ctx)? {
        1 => {
            let release = f.get(ctx, 2)?;
            f.call_service(ctx, 2, "WaitForSingleObject", &[release, u32::MAX])
        }
        2 => {
            if ctx.cpu.read_reg(ArmReg::R0)? != 0 {
                return Err(KernelError::Dispatch(
                    "MFC worker release wait failed".into(),
                ));
            }
            let release = f.get(ctx, 2)?;
            crate::coredll::close_handle_value(ctx, release)?;
            f.set(ctx, 2, 0)?;
            let o = f.get(ctx, 0)?;
            let entry = word(ctx, o + 0x54)?;
            if entry == 0 {
                let init = virtual_function(ctx, o, 21)?;
                return f.call_args(ctx, 5, init, &[o]);
            }
            let param = word(ctx, o + 0x50)?;
            f.call_args(ctx, 3, entry, &[param])
        }
        5 => {
            let o = f.get(ctx, 0)?;
            let slot = if ctx.cpu.read_reg(ArmReg::R0)? == 0 {
                27
            } else {
                22
            };
            let target = virtual_function(ctx, o, slot)?;
            f.call_args(ctx, 3, target, &[o])
        }
        3 => {
            let code = ctx.cpu.read_reg(ArmReg::R0)?;
            f.call_service(ctx, 4, END, &[code, 1])
        }
        _ => Err(KernelError::Dispatch(
            "MFC thread termination unexpectedly returned".into(),
        )),
    }
}
fn end(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let mt = super::application::module_thread(ctx)?;
        let o = word(ctx, mt + 4)?;
        f.set(ctx, 0, o)?;
        f.set(ctx, 1, mt)?;
        let owner = super::application::owner(ctx)?;
        if let Some(&worker) = ctx.kernel.dynamic_exports[&owner].get(&root(ctx)) {
            let alias = word(ctx, worker + 48 + 5 * 4)?;
            if alias != 0 {
                super::window_handles::detach_value(ctx, alias)?;
                if o != 0 && word(ctx, o + 0x20)? == alias {
                    put(ctx, o + 0x20, 0)?;
                }
            }
        }
        if o != 0 {
            let terminate = word(ctx, o + 0x58)?;
            if terminate != 0 {
                return f.call_args(ctx, 1, terminate, &[1, 0]);
            }
        }
    }
    let phase = if f.fresh { 0 } else { f.phase(ctx)? };
    if phase <= 1 {
        let o = f.get(ctx, 0)?;
        if o != 0 && f.arg(ctx, 1)? != 0 {
            let target = virtual_function(ctx, o, 30)?;
            return f.call_args(ctx, 2, target, &[o]);
        }
    }
    if phase <= 2 {
        let mt = f.get(ctx, 1)?;
        put(ctx, mt + 4, 0)?;
        return f.call_service(ctx, 3, super::gdi::CLEAR_TEMP, &[]);
    }
    if phase == 3 {
        return f.call_service(ctx, 4, super::device_contexts::CLEAR_TEMP, &[]);
    }
    if phase == 4 {
        return f.call_service(ctx, 6, super::menus::CLEAR_TEMP, &[]);
    }
    if phase == 6 {
        return f.call_service(ctx, 5, super::window_handles::CLEAR_TEMP, &[]);
    }
    if phase == 5 {
        return f.call_service(ctx, 7, super::edit_find_dialog::CLEAR, &[]);
    }
    if phase != 7 {
        return Err(KernelError::Dispatch(
            "Invalid MFC thread-exit continuation".into(),
        ));
    }
    super::application::release_thread_context(ctx)?;
    let owner = super::application::owner(ctx)?;
    let key = root(ctx);
    ctx.kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .remove(&key);
    super::callbacks::discard_thread_frames(ctx, f.address)?;
    let code = f.arg(ctx, 0)?;
    f.finish(ctx, code)?;
    ctx.cpu.write_arg(0, code)?;
    Ok(DispatchOutcome::JumpTo(service(ctx, "ExitThread")?))
}
fn delete(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh && word(ctx, o + 0x28)? != 0 {
        let target = virtual_function(ctx, o, 1)?;
        return f.call_args(ctx, 1, target, &[o, 1]);
    }
    f.finish(ctx, 0)
}
fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct_impl(ctx, false)
}
pub(super) fn deleting(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct_impl(ctx, true)
}
fn destruct_impl(ctx: &mut CallCtx<'_>, deleting: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu
            .check_guest_access(o, 104, Prot::READ | Prot::WRITE)?;
        let flags = f.arg(ctx, 1)?;
        if deleting && flags & 2 != 0 {
            return Err(KernelError::Dispatch(
                "MFC vector destruction is not implemented".into(),
            ));
        }
        f.set(ctx, 0, u32::from(deleting && flags & 1 != 0))?;
        let table = super::classes::vtable(ctx, "CWinThread")?;
        put(ctx, o, table)?;
        let handle = word(ctx, o + 0x2c)?;
        if handle != 0 {
            crate::coredll::close_handle_value(ctx, handle)?;
            put(ctx, o + 0x2c, 0)?;
        }
        let mt = super::application::module_thread(ctx)?;
        if word(ctx, mt + 4)? == o {
            put(ctx, mt + 4, 0)?;
        }
        return f.call_service(ctx, 1, "DeleteCriticalSection", &[o + 0x5c]);
    }
    if f.phase(ctx)? == 1 {
        return f.call_service(ctx, 2, super::command_targets::DESTRUCT, &[o]);
    }
    if f.get(ctx, 0)? != 0 {
        ctx.kernel.heap.free(o);
    }
    f.finish(ctx, o)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use super::*;
    use crate::bluetooth::tests::call;
    use pocket_cpu::{stub::StubCpu, Cpu};
    use pocket_kernel::{Dispatcher, KernelState, Thunk};
    use std::collections::HashMap;
    const RETURN: u32 = 0x12345678;
    const BODY: u32 = 0x9000;
    pub(super) enum Stop {
        Creator(u32),
        Worker(u32),
    }
    pub(super) fn run(
        cpu: &mut StubCpu,
        k: &mut KernelState,
        d: &mut WinCeDispatcher,
        first: Thunk,
        explicit_end: bool,
    ) -> Stop {
        let mut thunks: HashMap<_, _> = k.dynamic_exports[&1]
            .iter()
            .map(|(name, &va)| {
                (
                    va,
                    Thunk {
                        thunk_va: va,
                        iat_va: 0,
                        dll: "coredll.dll".into(),
                        binding: pocket_pe::ImportBinding::Name(name.clone()),
                        friendly_name: None,
                    },
                )
            })
            .collect();
        let mut pc = first.thunk_va;
        thunks.insert(pc, first);
        let mut end = entry(623);
        end.thunk_va = 0x70000040;
        thunks.insert(end.thunk_va, end);
        cpu.write_reg(ArmReg::Lr, RETURN).unwrap();
        for _ in 0..1024 {
            if pc == RETURN && k.current_thread == 0 {
                return Stop::Creator(cpu.read_arg(0).unwrap());
            }
            if k.current_thread != 0 && pc == k.threads[k.current_thread - 1].exit_va {
                return Stop::Worker(cpu.read_arg(0).unwrap());
            }
            if pc == 0x9400 {
                assert_eq!(k.current_thread, 0);
                let object = cpu.read_u32_le(0x2500).unwrap();
                cpu.write_arg(0, object).unwrap();
                pc = cpu.read_reg(ArmReg::Lr).unwrap();
                continue;
            }
            if matches!(pc, 0x9100 | 0x9200 | 0x9300) {
                assert_ne!(k.current_thread, 0);
                let object = cpu.read_arg(0).unwrap();
                assert_eq!(object, cpu.read_u32_le(0x2500).unwrap());
                assert_eq!(cpu.read_u32_le(object + 0x54).unwrap(), 0);
                let thread = result(call(cpu, k, d, "mfcce300.dll", "ord:650", &[]));
                assert_eq!(thread, object);
                let flag = match pc {
                    0x9100 => 1,
                    0x9200 => 2,
                    _ => 4,
                };
                let seen = cpu.read_u32_le(0x2504).unwrap();
                cpu.write_mem(0x2504, &(seen | flag).to_le_bytes()).unwrap();
                let value = match pc {
                    0x9100 => u32::from(!explicit_end),
                    0x9200 => 0x66,
                    _ => 0x77,
                };
                cpu.write_arg(0, value).unwrap();
                pc = cpu.read_reg(ArmReg::Lr).unwrap();
                continue;
            }
            if pc == BODY {
                assert_ne!(
                    k.current_thread, 0,
                    "worker entry must run on its own guest thread"
                );
                assert_eq!(cpu.read_arg(0).unwrap(), 0x98765432);
                let o = result(call(cpu, k, d, "mfcce300.dll", "ord:650", &[]));
                assert_eq!(cpu.read_u32_le(o + 0x54).unwrap(), BODY);
                let alias = cpu.read_u32_le(o + 0x20).unwrap();
                assert_ne!(alias, 0x2300);
                assert_eq!(cpu.read_u32_le(alias + 0x20).unwrap(), 7);
                assert_eq!(
                    k.dynamic_exports[&1]["__PocketHLE_MFC_Window:00000007"],
                    0x2300
                );
                cpu.write_arg(0, 0x55).unwrap();
                if explicit_end {
                    cpu.write_arg(1, 1).unwrap();
                    cpu.write_reg(ArmReg::Lr, 0xbad00bad).unwrap();
                    pc = 0x70000040;
                } else {
                    pc = cpu.read_reg(ArmReg::Lr).unwrap();
                }
                continue;
            }
            let t = thunks
                .get(&pc)
                .unwrap_or_else(|| panic!("unexpected guest {pc:x}"));
            let outcome = d
                .dispatch(cpu, t, k)
                .unwrap_or_else(|e| panic!("{:?}: {e}", t.binding));
            pc = match outcome {
                DispatchOutcome::JumpTo(next) => next,
                DispatchOutcome::ReturnedR0(value) => {
                    cpu.write_arg(0, value).unwrap();
                    cpu.read_reg(ArmReg::Lr).unwrap()
                }
                other => panic!("unexpected {other:?}"),
            };
        }
        panic!("MFC thread handshake did not finish");
    }
    pub(super) fn setup() -> (StubCpu, KernelState, WinCeDispatcher) {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:201",
            &[0x2000, 0],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2300],
        );
        k.window_classes.insert(7, "BorrowedMain".into());
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:804",
            &[0x2300, 7],
        );
        cpu.write_mem(0x2020, &0x2300u32.to_le_bytes()).unwrap();
        (cpu, k, d)
    }
    pub(super) fn args(cpu: &mut StubCpu, values: &[u32]) {
        for (i, &v) in values.iter().enumerate() {
            if i < 4 {
                cpu.write_arg(i as u8, v).unwrap();
            } else {
                cpu.write_mem(0x4000 + (i as u32 - 4) * 4, &v.to_le_bytes())
                    .unwrap();
            }
        }
    }
    #[test]
    fn suspended_mfc_workers_bind_local_context_and_clean_up_on_return_or_explicit_exit() {
        for (explicit_end, retained) in [(false, false), (true, false), (false, true), (true, true)]
        {
            let (mut cpu, mut k, mut d) = setup();
            let events = k.events.len();
            args(&mut cpu, &[BODY, 0x98765432, 3, 0, 4, 0]);
            let Stop::Creator(o) = run(&mut cpu, &mut k, &mut d, entry(604), explicit_end) else {
                panic!("suspended worker ran its body");
            };
            assert_ne!(o, 0);
            let handle = cpu.read_u32_le(o + 0x2c).unwrap();
            if retained {
                cpu.write_mem(o + 0x28, &0u32.to_le_bytes()).unwrap();
            }
            assert_eq!(k.threads.len(), 1);
            assert_eq!(k.threads[0].suspend_count, 1);
            assert_eq!(
                result(call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "coredll.dll",
                    "GetThreadPriority",
                    &[handle]
                )),
                3
            );
            assert_eq!(k.events.len(), events + 1); // worker still owns the release event
            assert_eq!(
                result(call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "coredll.dll",
                    "ResumeThread",
                    &[handle]
                )),
                1
            );
            args(&mut cpu, &[handle, u32::MAX]);
            let wait = Thunk {
                thunk_va: 0x70000020,
                iat_va: 0,
                dll: "coredll.dll".into(),
                binding: pocket_pe::ImportBinding::Name("WaitForSingleObject".into()),
                friendly_name: None,
            };
            let Stop::Worker(code) = run(&mut cpu, &mut k, &mut d, wait, explicit_end) else {
                panic!("worker did not reach WinCE exit");
            };
            assert_eq!(code, 0x55);
            if retained {
                assert!(k.heap.msize(o).is_some());
                assert!(!k.threads[0].handle_closed);
                assert!(k.critical_sections.contains_key(&(o + 0x5c)));
                // The creator deletes a retained worker after it has exited.
                k.current_thread = 0;
                cpu.write_reg(ArmReg::Sp, 0x4000).unwrap();
                cpu.write_mem(o + 0x28, &1u32.to_le_bytes()).unwrap();
                cpu.write_arg(0, o).unwrap();
                drive(&mut cpu, &mut k, &mut d, entry(1030), |pc, _| {
                    panic!("unexpected {pc:x}")
                });
            }
            assert!(k.heap.msize(o).is_none());
            assert!(k.threads[0].handle_closed);
            assert_eq!(k.events.len(), events);
            assert!(!k.critical_sections.contains_key(&(o + 0x5c)));
            for prefix in [
                "__PocketHLE_MFC_Worker:1",
                "__PocketHLE_MFC_ModuleThread:1",
                "__PocketHLE_MFC_Thread:1",
                "__PocketHLE_MFC_Frame:1:",
                "__PocketHLE_MFC_ThreadWindow:1:",
            ] {
                assert!(
                    !k.dynamic_exports[&1].keys().any(|s| s.starts_with(prefix)),
                    "{prefix} still owns a resource"
                );
            }
            assert_eq!(cpu.read_u32_le(0x2320).unwrap(), 7);
            assert!(k.window_classes.contains_key(&7));
        }
    }
    #[test]
    fn native_thread_creation_failure_releases_events_context_and_auto_deleted_object() {
        fn reject(_: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
            Ok(DispatchOutcome::ReturnedR0(0))
        }
        let (mut cpu, mut k, mut d) = setup();
        // Warm immutable class tables and the shared base-destructor path.
        args(&mut cpu, &[0x2600, BODY, 1]);
        drive(&mut cpu, &mut k, &mut d, entry(203), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        cpu.write_arg(0, 0x2600).unwrap();
        drive(&mut cpu, &mut k, &mut d, entry(312), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        let free = k.heap.free_bytes();
        let events = k.events.len();
        let critical = k.critical_sections.len();
        d.register_handler("coredll.dll", "CreateThread", reject);
        args(&mut cpu, &[BODY, 1, 3, 0, 0, 0]);
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(604), |pc, _| panic!(
                "unexpected {pc:x}"
            )),
            0
        );
        assert_eq!(k.heap.free_bytes(), free);
        assert_eq!(k.events.len(), events);
        assert_eq!(k.critical_sections.len(), critical);
        assert!(k.threads.is_empty());
    }
    #[test]
    fn legacy_priorities_round_trip_real_ce_thread_control_and_reject_invalid_arguments() {
        let (mut cpu, mut k, mut d) = setup();
        let handle = pocket_kernel::FAKE_CURRENT_THREAD_HANDLE;
        for value in 0..8 {
            assert_eq!(
                result(call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "coredll.dll",
                    "SetThreadPriority",
                    &[handle, value]
                )),
                1
            );
            assert_eq!(
                result(call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "coredll.dll",
                    "GetThreadPriority",
                    &[handle]
                )),
                value
            );
            assert_eq!(
                result(call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "coredll.dll",
                    "CeGetThreadPriority",
                    &[handle]
                )),
                248 + value
            );
        }
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SetThreadPriority",
                &[handle, 8]
            )),
            0
        );
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "GetThreadPriority",
                &[0x1234]
            )),
            i32::MAX as u32
        );
    }
}

#[cfg(test)]
mod ui_tests {
    use super::super::window_dispatch::tests::{drive, entry};
    use super::tests::{args, run, setup, Stop};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    use pocket_kernel::{DispatchOutcome, Thunk};
    #[test]
    fn suspended_ui_thread_uses_the_same_handshake_and_runs_actual_init_run_or_exit_virtuals() {
        for fails_init in [false, true] {
            let (mut cpu, mut k, mut d) = setup();
            let object = k.heap.alloc_cpp(104).unwrap();
            cpu.write_arg(0, object).unwrap();
            drive(&mut cpu, &mut k, &mut d, entry(204), |pc, _| {
                panic!("unexpected {pc:x}")
            });
            let table = cpu.read_u32_le(object).unwrap();
            let methods = cpu.read_mem(table, 31 * 4).unwrap();
            cpu.write_mem(0x2800, &methods).unwrap();
            for (slot, value) in [(21, 0x9100u32), (22, 0x9200), (27, 0x9300)] {
                cpu.write_mem(0x2800 + slot * 4, &value.to_le_bytes())
                    .unwrap();
            }
            cpu.write_mem(object, &0x2800u32.to_le_bytes()).unwrap();
            cpu.write_mem(0x2500, &object.to_le_bytes()).unwrap();
            cpu.write_mem(0x2504, &0u32.to_le_bytes()).unwrap();
            cpu.write_mem(0x260c, &0x9400u32.to_le_bytes()).unwrap();
            args(&mut cpu, &[0x2600, 3, 0, 4, 0]);
            let Stop::Creator(created) = run(&mut cpu, &mut k, &mut d, entry(605), fails_init)
            else {
                panic!("UI body ran before resume")
            };
            assert_eq!(created, object);
            assert_eq!(cpu.read_u32_le(0x2504).unwrap(), 0);
            let handle = cpu.read_u32_le(object + 0x2c).unwrap();
            assert_eq!(k.threads[0].suspend_count, 1);
            assert!(matches!(
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "coredll.dll",
                    "ResumeThread",
                    &[handle]
                ),
                DispatchOutcome::ReturnedR0(1)
            ));
            args(&mut cpu, &[handle, u32::MAX]);
            let wait = Thunk {
                thunk_va: 0x70000020,
                iat_va: 0,
                dll: "coredll.dll".into(),
                binding: pocket_pe::ImportBinding::Name("WaitForSingleObject".into()),
                friendly_name: None,
            };
            let Stop::Worker(code) = run(&mut cpu, &mut k, &mut d, wait, fails_init) else {
                panic!("UI thread did not finish")
            };
            assert_eq!(code, if fails_init { 0x77 } else { 0x66 });
            assert_eq!(
                cpu.read_u32_le(0x2504).unwrap(),
                if fails_init { 5 } else { 3 }
            );
            assert!(k.heap.msize(object).is_none());
            assert!(!k.dynamic_exports[&1]
                .keys()
                .any(|n| n.starts_with("__PocketHLE_MFC_Worker:")));
        }
    }
}
