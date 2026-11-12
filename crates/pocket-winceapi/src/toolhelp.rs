//! Windows CE Toolhelp process and thread snapshots.
use crate::{coredll::set_thread_error, CallCtx, WinCeDispatcher};
use pocket_cpu::Prot;
use pocket_kernel::handles::ToolhelpSnapshot;
use pocket_kernel::{DispatchOutcome, KernelError};

const PROCESS_ENTRY_SIZE: usize = 564;
const THREAD_ENTRY_SIZE: usize = 36;

pub fn register(dispatcher: &mut WinCeDispatcher) {
    for (name, handler) in [
        (
            "CreateToolhelp32Snapshot",
            create_snapshot as crate::Handler,
        ),
        ("CloseToolhelp32Snapshot", close_snapshot),
        ("Process32First", |ctx| enumerate(ctx, true, true)),
        ("Process32Next", |ctx| enumerate(ctx, true, false)),
        ("Thread32First", |ctx| enumerate(ctx, false, true)),
        ("Thread32Next", |ctx| enumerate(ctx, false, false)),
    ] {
        dispatcher.register_handler("toolhelp.dll", name, handler);
    }
}

fn word(entry: &mut [u8], offset: usize, value: u32) {
    entry[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn create_snapshot(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    const SNAP_PROCESS: u32 = 2;
    const SNAP_THREAD: u32 = 4;
    const SNAP_NO_HEAPS: u32 = 0x4000_0000;
    let flags = ctx.arg_u32(0)?;
    if flags & (SNAP_PROCESS | SNAP_THREAD) == 0
        || flags & !(SNAP_PROCESS | SNAP_THREAD | SNAP_NO_HEAPS) != 0
    {
        set_thread_error(ctx, 50); // ERROR_NOT_SUPPORTED
        return Ok(DispatchOutcome::ReturnedR0(u32::MAX));
    }
    let handles = &ctx.kernel.object_handles;
    handles.set_process_image(&ctx.kernel.module_path, ctx.kernel.image_base);
    let mut snapshot = ToolhelpSnapshot::default();
    for process in handles.snapshot_processes() {
        if flags & SNAP_PROCESS != 0 {
            let mut entry = vec![0; PROCESS_ENTRY_SIZE];
            word(&mut entry, 0, PROCESS_ENTRY_SIZE as u32);
            word(&mut entry, 8, process.pid);
            word(&mut entry, 20, process.threads.len() as u32);
            let name = process.path.rsplit(['\\', '/']).next().unwrap_or_default();
            for (dst, unit) in entry[36..556]
                .chunks_exact_mut(2)
                .zip(name.encode_utf16().take(259))
            {
                dst.copy_from_slice(&unit.to_le_bytes());
            }
            word(&mut entry, 556, process.image_base);
            snapshot.processes.push(entry);
        }
        if flags & SNAP_THREAD != 0 {
            for tid in process.threads {
                let mut entry = vec![0; THREAD_ENTRY_SIZE];
                word(&mut entry, 0, THREAD_ENTRY_SIZE as u32);
                word(&mut entry, 8, tid);
                word(&mut entry, 12, process.pid);
                word(&mut entry, 32, process.pid);
                snapshot.threads.push(entry);
            }
        }
    }
    let Some(handle) = handles.add_toolhelp_snapshot(snapshot) else {
        set_thread_error(ctx, 8); // ERROR_NOT_ENOUGH_MEMORY
        return Ok(DispatchOutcome::ReturnedR0(u32::MAX));
    };
    Ok(DispatchOutcome::ReturnedR0(handle))
}

fn enumerate(
    ctx: &mut CallCtx<'_>,
    process: bool,
    first: bool,
) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let out = ctx.arg_u32(1)?;
    let size = if process {
        PROCESS_ENTRY_SIZE
    } else {
        THREAD_ENTRY_SIZE
    } as u32;
    let result = ctx
        .kernel
        .object_handles
        .with_toolhelp_snapshot(handle, |snapshot| {
            if out == 0
                || ctx.cpu.read_u32_le(out).ok().is_none_or(|n| n < size)
                || ctx.cpu.check_guest_access(out, size, Prot::WRITE).is_err()
            {
                return Ok(Err(87)); // ERROR_INVALID_PARAMETER
            }
            let (entries, cursor) = if process {
                (&snapshot.processes, &mut snapshot.process_cursor)
            } else {
                (&snapshot.threads, &mut snapshot.thread_cursor)
            };
            let index = if first {
                Some(0)
            } else {
                cursor.map(|i| i + 1)
            };
            let Some((index, entry)) = index.and_then(|i| entries.get(i).map(|entry| (i, entry)))
            else {
                return Ok(Err(18)); // ERROR_NO_MORE_FILES
            };
            ctx.cpu.write_mem(out, entry)?;
            *cursor = Some(index);
            Ok::<_, KernelError>(Ok(()))
        });
    match result.transpose()?.unwrap_or(Err(6)) {
        Ok(()) => Ok(DispatchOutcome::ReturnedR0(1)),
        Err(error) => {
            set_thread_error(ctx, error);
            Ok(DispatchOutcome::ReturnedR0(0))
        }
    }
}

fn close_snapshot(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let closed = ctx.kernel.object_handles.close_toolhelp_snapshot(handle);
    if !closed {
        set_thread_error(ctx, 6); // ERROR_INVALID_HANDLE
    }
    Ok(DispatchOutcome::ReturnedR0(u32::from(closed)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;

    #[test]
    fn snapshot_walk_keeps_guest_processes_that_exit_after_capture() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let (_, child) = kernel.object_handles.new_child().unwrap();
        child.set_process_image(r"\Program Files\Child\child.exe", 0x20000);
        let DispatchOutcome::ReturnedR0(handle) = call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "toolhelp.dll",
            "CreateToolhelp32Snapshot",
            &[0x4000_0002, 0],
        ) else {
            panic!("expected snapshot handle");
        };
        assert_ne!(handle, u32::MAX);
        child.mark_inactive();

        let out = 0x2000;
        cpu.write_mem(out, &(PROCESS_ENTRY_SIZE as u32).to_le_bytes())
            .unwrap();
        for (name, expected) in [
            ("Process32First", 1),
            ("Process32Next", 1),
            ("Process32Next", 0),
        ] {
            assert_eq!(
                call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "toolhelp.dll",
                    name,
                    &[handle, out]
                ),
                DispatchOutcome::ReturnedR0(expected)
            );
        }
        assert_eq!(cpu.read_u32_le(out + 8).unwrap(), child.process_id());
        assert_eq!(
            cpu.read_mem(out + 36, 20).unwrap(),
            "child.exe\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert_eq!(cpu.read_u32_le(out + 556).unwrap(), 0x20000);
        for expected in [1, 0] {
            assert_eq!(
                call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "toolhelp.dll",
                    "CloseToolhelp32Snapshot",
                    &[handle]
                ),
                DispatchOutcome::ReturnedR0(expected)
            );
        }
    }
}
