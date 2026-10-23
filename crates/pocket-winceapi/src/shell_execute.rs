//! Windows CE ShellExecuteEx: launch guest executables through the existing
//! asynchronous CreateProcess runner, never through the host operating system.
use super::*;
const SIZE: usize = 60;
const NOCLOSEPROCESS: u32 = 0x40;
const NO_UI: u32 = 0x400;

fn failed(ctx: &mut CallCtx<'_>, ptr: u32, error: u32) -> Result<DispatchOutcome, KernelError> {
    let shell_error: u32 = match error {
        2 | 3 | 5 | 8 => error,
        193 => 11,
        1155 => 31,
        _ => 5,
    };
    let _ = ctx
        .cpu
        .write_mem(ptr.wrapping_add(32), &shell_error.to_le_bytes());
    let _ = ctx.cpu.write_mem(ptr.wrapping_add(56), &0u32.to_le_bytes());
    set_thread_error(ctx, error);
    Ok(DispatchOutcome::ReturnedR0(0))
}

pub(super) fn execute(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let ptr = ctx.arg_u32(0)?;
    let Ok(info) = ctx.cpu.read_mem(ptr, SIZE) else {
        set_thread_error(ctx, ERROR_INVALID_PARAMETER);
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    let word = |off: usize| u32::from_le_bytes(info[off..off + 4].try_into().unwrap());
    if ptr == 0
        || word(0) != SIZE as u32
        || ctx
            .cpu
            .check_guest_access(ptr, SIZE as u32, Prot::WRITE)
            .is_err()
    {
        set_thread_error(ctx, ERROR_INVALID_PARAMETER);
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let mask = word(4);
    let key = (
        ctx.kernel.current_thread,
        ctx.thunk.thunk_va,
        ctx.cpu.read_reg(ArmReg::Sp)?,
    );
    if let Some(result) = ctx.kernel.process_launch_results.remove(&key) {
        let handle = match result {
            Ok(h) => h,
            Err(e) => return failed(ctx, ptr, e),
        };
        let table = ctx.kernel.object_handles.child(handle).unwrap();
        ctx.kernel
            .object_handles
            .bind(handle, HandleObject::ChildProcess(handle));
        ctx.kernel
            .object_handles
            .bind(handle + 1, HandleObject::ChildThread(handle));
        ctx.kernel.child_processes.insert(
            handle,
            pocket_kernel::ChildProcess {
                thread_handle: handle + 1,
                exit_code: None,
                process_handle_closed: false,
                thread_handle_closed: false,
            },
        );
        // CE returns a process handle only when explicitly requested. Closing
        // our temporary handles must not terminate the launched application.
        ctx.cpu.write_mem(ptr + 32, &handle.to_le_bytes())?;
        ctx.cpu.write_mem(
            ptr + 56,
            &(if mask & NOCLOSEPROCESS != 0 {
                handle
            } else {
                0
            })
            .to_le_bytes(),
        )?;
        close_owned_handle(ctx.kernel, handle + 1);
        if mask & NOCLOSEPROCESS == 0 {
            close_owned_handle(ctx.kernel, handle);
        }
        table.allow_start();
        return Ok(DispatchOutcome::ReturnedR0(1));
    }
    ctx.cpu.write_mem(ptr + 56, &0u32.to_le_bytes())?;
    if mask & !(NOCLOSEPROCESS | NO_UI) != 0 || word(24) != 0 {
        return failed(ctx, ptr, ERROR_NOT_SUPPORTED);
    }
    let mut string = |off| -> Result<String, KernelError> {
        let p = word(off);
        if p == 0 {
            return Ok(String::new());
        }
        Ok(String::from_utf16_lossy(&read_wstr(ctx, p, 32768)?))
    };
    let (verb, file, parameters) = match (string(12), string(16), string(20)) {
        (Ok(v), Ok(f), Ok(p)) => (v, f, p),
        _ => return failed(ctx, ptr, ERROR_INVALID_PARAMETER),
    };
    if file.is_empty() {
        return failed(ctx, ptr, 2);
    }
    if !verb.is_empty() && !verb.eq_ignore_ascii_case("open") {
        return failed(ctx, ptr, 1155);
    }
    // Unknown document classes must fail honestly; an executable must actually
    // be loaded by the guest runner before reporting success.
    if !file.to_ascii_lowercase().ends_with(".exe") {
        return failed(ctx, ptr, 1155);
    }
    if !ctx.kernel.process_launch_enabled
        || ctx.kernel.dll_notification_frame.is_some()
        || !ctx.kernel.module_attach_frames.is_empty()
    {
        return failed(ctx, ptr, ERROR_NOT_SUPPORTED);
    }
    log::info!("ShellExecuteEx: launch {file:?}, parameters={parameters:?}");
    let outcome = queue_child_launch(ctx, file, parameters, 0, key)?;
    if outcome == DispatchOutcome::ReturnedR0(0) {
        let error = ctx
            .kernel
            .thread_last_errors
            .get(&ctx.kernel.current_thread)
            .copied()
            .unwrap_or(5);
        failed(ctx, ptr, error)
    } else {
        Ok(outcome)
    }
}
