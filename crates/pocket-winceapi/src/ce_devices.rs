//! CE device-manager boundaries. Reserving a name is distinct from mounting a
//! driver; servicesStart must not receive a fabricated running device handle.
use crate::{CallCtx, Handler, WinCeDispatcher};
use pocket_cpu::{regs::ArmReg, Prot};
use pocket_kernel::{handles::HandleObject, registry::RegistryValue, DispatchOutcome, KernelError};

type Outcome = Result<DispatchOutcome, KernelError>;

pub fn register(d: &mut WinCeDispatcher) {
    for (name, handler) in [
        ("GetPasswordActive", password_active as Handler),
        ("RegisterAFSName", register_name),
        ("DeregisterAFSName", deregister_name),
        ("RegisterAFSEx", register_filesystem),
        ("DeregisterAFS", deregister_filesystem),
        ("ActivateDeviceEx", activate),
        ("PowerOffSystem", power_off),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}

fn failed(ctx: &mut CallCtx<'_>, error: u32, result: u32) -> Outcome {
    crate::coredll::set_thread_error(ctx, error);
    Ok(DispatchOutcome::ReturnedR0(result))
}

fn password_active(ctx: &mut CallCtx<'_>) -> Outcome {
    Ok(DispatchOutcome::ReturnedR0(u32::from(
        ctx.kernel.ce.device.password_active(),
    )))
}

fn register_name(ctx: &mut CallCtx<'_>) -> Outcome {
    let pointer = ctx.arg_u32(0)?;
    let name = match crate::ce_shortcut::text(ctx, pointer, 260) {
        Ok(name) => name,
        Err(error) => return failed(ctx, error, u32::MAX),
    };
    match ctx
        .kernel
        .ce
        .device
        .register_afs_name(ctx.kernel.object_handles.process_id(), name)
    {
        Ok(index) => Ok(DispatchOutcome::ReturnedR0(index as u32)),
        Err(error) => failed(ctx, error, u32::MAX),
    }
}

fn deregister_name(ctx: &mut CallCtx<'_>) -> Outcome {
    let index = ctx.arg_u32(0)? as i32;
    match ctx
        .kernel
        .ce
        .device
        .deregister_afs_name(ctx.kernel.object_handles.process_id(), index)
    {
        Ok(()) => Ok(DispatchOutcome::ReturnedR0(1)),
        Err(error) => failed(ctx, error, 0),
    }
}

fn register_filesystem(ctx: &mut CallCtx<'_>) -> Outcome {
    let index = ctx.arg_u32(0)? as i32;
    let handle = ctx.arg_u32(1)?;
    let _context = ctx.arg_u32(2)?;
    let version = ctx.arg_u32(3)?;
    let flags = ctx.arg_u32(4)?;
    if let Err(error) = ctx
        .kernel
        .ce
        .device
        .validate_afs_owner(ctx.kernel.object_handles.process_id(), index)
    {
        return failed(ctx, error, 0);
    }
    if !matches!(
        ctx.kernel.object_handles.get(handle),
        Some(HandleObject::ApiSet(_))
    ) {
        return failed(ctx, 6, 0);
    }
    if !(1..=4).contains(&version) || flags & !0x1ff != 0 {
        return failed(ctx, 87, 0);
    }
    // udevice/servicesd use a handle-based API server (version 4). Until its
    // filesystem marshalling exists, a registered name is not a mounted FS.
    failed(ctx, 50, 0)
}

fn deregister_filesystem(ctx: &mut CallCtx<'_>) -> Outcome {
    let index = ctx.arg_u32(0)? as i32;
    if let Err(error) = ctx
        .kernel
        .ce
        .device
        .validate_afs_owner(ctx.kernel.object_handles.process_id(), index)
    {
        return failed(ctx, error, 0);
    }
    failed(ctx, 1168, 0) // No filesystem provider was mounted at this index.
}

fn readable(ctx: &mut CallCtx<'_>, pointer: u32, size: u32) -> bool {
    size == 0
        || (pointer != 0
            && pointer.checked_add(size).is_some()
            && ctx
                .cpu
                .check_guest_access(pointer, size, Prot::READ)
                .is_ok())
}

fn activate(ctx: &mut CallCtx<'_>) -> Outcome {
    let key_pointer = ctx.arg_u32(0)?;
    let entries_pointer = ctx.arg_u32(1)?;
    let count = ctx.arg_u32(2)?;
    let _context = ctx.arg_u32(3)?;
    let key = match crate::ce_shortcut::text(ctx, key_pointer, 260) {
        Ok(key) if !key.is_empty() => format!("HKLM\\{key}"),
        Ok(_) => return failed(ctx, 87, 0),
        Err(error) => return failed(ctx, error, 0),
    };
    if count > 1024 || !readable(ctx, entries_pointer, count * 16) {
        return failed(ctx, 87, 0);
    }
    if count != 0 {
        let entries = ctx.cpu.read_mem(entries_pointer, count * 16)?;
        for entry in entries.chunks_exact(16) {
            let words: Vec<_> = entry
                .chunks_exact(4)
                .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
                .collect();
            if let Err(error) = crate::ce_shortcut::text(ctx, words[0], 260) {
                return failed(ctx, error, 0);
            }
            let (data, size, kind) = (words[1], words[2], words[3]);
            if size > 1024 * 1024 || !readable(ctx, data, size) {
                return failed(ctx, 87, 0);
            }
            if kind > 11
                || (matches!(kind, 4 | 5) && size != 4)
                || (kind == 11 && size != 8)
                || (matches!(kind, 1 | 2 | 6 | 7) && (size < 2 || size % 2 != 0))
            {
                return failed(ctx, 13, 0);
            }
            if matches!(kind, 1 | 2 | 6 | 7) {
                let bytes = ctx.cpu.read_mem(data, size)?;
                if !bytes.ends_with(&[0, 0])
                    || (kind == 7 && (size < 4 || !bytes.ends_with(&[0, 0, 0, 0])))
                {
                    return failed(ctx, 13, 0);
                }
            }
        }
    }
    if !ctx.kernel.registry.contains_key(&key) {
        return failed(ctx, 2, 0);
    }
    let dll = match ctx.kernel.registry.value(&key, "Dll") {
        Some(RegistryValue::Sz(dll)) if !dll.is_empty() => dll,
        None => return failed(ctx, 2, 0),
        _ => return failed(ctx, 13, 0),
    };
    log::warn!("ActivateDeviceEx({key:?}): CE driver provider {dll:?} is not supported");
    // REGINI initializes the eventual active key. Failed activation must not
    // publish those entries or change the original image's driver settings.
    failed(ctx, 50, 0)
}

fn power_off(ctx: &mut CallCtx<'_>) -> Outcome {
    let key = (
        ctx.kernel.current_thread,
        ctx.thunk.thunk_va,
        ctx.cpu.read_reg(ArmReg::Sp)?,
    );
    if let Some(generation) = ctx.kernel.ce.power_calls.get(&key).copied() {
        if ctx.kernel.ce.device.suspended_generation() != Some(generation) {
            ctx.kernel.ce.power_calls.remove(&key);
            return Ok(DispatchOutcome::ReturnedR0(0));
        }
    } else {
        let generation = ctx.kernel.begin_ce_suspend();
        ctx.kernel.ce.power_calls.insert(key, generation);
        log::info!("PowerOffSystem: emulated CE device suspended until new guest input");
    }
    Ok(DispatchOutcome::JumpTo(ctx.thunk.thunk_va))
}

#[cfg(test)]
mod tests;
