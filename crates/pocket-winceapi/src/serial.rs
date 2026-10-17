//! WinCE serial APIs for the explicit GPS COM8 bridge; Bluetooth keeps its APIs.
use crate::{CallCtx, WinCeDispatcher};
use pocket_cpu::Prot;
use pocket_kernel::{DispatchOutcome, KernelError};
pub(crate) fn register(d: &mut WinCeDispatcher) {
    for name in [
        "GetCommState",
        "SetCommState",
        "GetCommTimeouts",
        "SetCommTimeouts",
        "ClearCommError",
        "PurgeComm",
        "SetupComm",
        "EscapeCommFunction",
        "GetCommModemStatus",
    ] {
        d.register_handler("coredll.dll", name, control);
    }
}
fn fail(ctx: &mut CallCtx<'_>, error: u32) -> Result<DispatchOutcome, KernelError> {
    crate::bluetooth::finish_wait(ctx)?;
    crate::coredll::set_thread_error(ctx, error);
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn output(ctx: &mut CallCtx<'_>, ptr: u32, size: u32) -> bool {
    ptr != 0 && ctx.cpu.check_guest_access(ptr, size, Prot::WRITE).is_ok()
}
pub(crate) fn open(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let flags = ctx.arg_u32(1)?;
    let share = ctx.arg_u32(2)?;
    let access = u32::from(flags & 0x80000000 != 0) | u32::from(flags & 0x40000000 != 0) * 2;
    let error = if access == 0 || share & !3 != 0 || ctx.arg_u32(4)? != 3 {
        Some(87)
    } else if ctx
        .kernel
        .serial_ports
        .values()
        .any(|p| (access & !p.share) != 0 || (p.access & !share) != 0)
    {
        Some(32)
    } else {
        None
    };
    if let Some(error) = error {
        crate::coredll::set_thread_error(ctx, error);
        return Ok(DispatchOutcome::ReturnedR0(u32::MAX));
    }
    let device = match ctx.kernel.vfs.gps_service().open() {
        Ok(d) => d,
        Err(e) => {
            crate::coredll::set_thread_error(ctx, e);
            return Ok(DispatchOutcome::ReturnedR0(u32::MAX));
        }
    };
    let Some(handle) = (0xdead8800..0xdead8900).find(|h| !ctx.kernel.serial_ports.contains_key(h))
    else {
        crate::coredll::set_thread_error(ctx, 4);
        return Ok(DispatchOutcome::ReturnedR0(u32::MAX));
    };
    ctx.kernel.serial_ports.insert(
        handle,
        pocket_kernel::serial::Port::new(device, access, share),
    );
    log::debug!("CreateFileW(COM8:) -> 0x{handle:08x}");
    Ok(DispatchOutcome::ReturnedR0(handle))
}
pub(crate) fn read(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    crate::bluetooth::finish_wait(ctx)?;
    let h = ctx.arg_u32(0)?;
    let ptr = ctx.arg_u32(1)?;
    let count = ctx.arg_u32(2)?;
    let done = ctx.arg_u32(3)?;
    if (count != 0 && !output(ctx, ptr, count)) || (done != 0 && !output(ctx, done, 4)) {
        return fail(ctx, 87);
    }
    if ctx.arg_u32(4)? != 0 {
        return fail(ctx, 50);
    }
    if done != 0 {
        ctx.cpu.write_mem(done, &0u32.to_le_bytes())?;
    }
    let port = ctx.kernel.serial_ports.get_mut(&h).unwrap();
    if port.access & 1 == 0 {
        return fail(ctx, 5);
    }
    if let Err(e) = port.poll() {
        return fail(ctx, e);
    }
    let n = (count as usize).min(port.queue.len());
    let data: Vec<_> = port.queue.iter().take(n).copied().collect();
    if n != 0 {
        ctx.cpu.write_mem(ptr, &data)?;
        ctx.kernel
            .serial_ports
            .get_mut(&h)
            .unwrap()
            .queue
            .drain(..n);
    }
    if done != 0 {
        ctx.cpu.write_mem(done, &(n as u32).to_le_bytes())?;
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}
pub(crate) fn write(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let h = ctx.arg_u32(0)?;
    let ptr = ctx.arg_u32(1)?;
    let count = ctx.arg_u32(2)?;
    let done = ctx.arg_u32(3)?;
    if ctx.kernel.serial_ports[&h].access & 2 == 0 {
        return fail(ctx, 5);
    }
    if !ctx.kernel.serial_ports[&h]
        .device
        .lock()
        .unwrap()
        .access_allowed()
    {
        return fail(ctx, 5);
    }
    if ctx.arg_u32(4)? != 0 {
        return fail(ctx, 50);
    }
    if count != 0 && ctx.cpu.check_guest_access(ptr, count, Prot::READ).is_err() {
        return fail(ctx, 87);
    }
    if done != 0 {
        if !output(ctx, done, 4) {
            return fail(ctx, 87);
        }
        ctx.cpu.write_mem(done, &count.to_le_bytes())?;
    }
    // GPS receiver configuration bytes do not change the existing host provider.
    Ok(DispatchOutcome::ReturnedR0(1))
}
pub(crate) fn control(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    crate::bluetooth::finish_wait(ctx)?;
    let h = ctx.arg_u32(0)?;
    let arg = ctx.arg_u32(1)?;
    let name = crate::import_name(ctx.thunk).into_owned();
    if !ctx.kernel.serial_ports.contains_key(&h) {
        return fail(ctx, 6);
    }
    if !ctx.kernel.serial_ports[&h]
        .device
        .lock()
        .unwrap()
        .access_allowed()
    {
        return fail(ctx, 5);
    }
    if name != "ClearCommError" {
        log::debug!("{name}(serial=0x{h:08x})");
    }
    match name.as_str() {
        "GetCommState" | "GetCommTimeouts" => {
            let data = if name == "GetCommState" {
                ctx.kernel.serial_ports[&h].dcb.to_vec()
            } else {
                ctx.kernel.serial_ports[&h].timeouts.to_vec()
            };
            if !output(ctx, arg, data.len() as u32) {
                return fail(ctx, 87);
            }
            ctx.cpu.write_mem(arg, &data)?;
        }
        "SetCommState" => {
            let data = ctx.cpu.read_mem(arg, 28)?;
            if u32::from_le_bytes(data[..4].try_into().unwrap()) != 28
                || !(5..=8).contains(&data[18])
                || data[19] > 4
                || data[20] > 2
            {
                return fail(ctx, 87);
            }
            ctx.kernel
                .serial_ports
                .get_mut(&h)
                .unwrap()
                .dcb
                .copy_from_slice(&data);
        }
        "SetCommTimeouts" => {
            let data = ctx.cpu.read_mem(arg, 20)?;
            ctx.kernel
                .serial_ports
                .get_mut(&h)
                .unwrap()
                .timeouts
                .copy_from_slice(&data);
        }
        "SetCommMask" => {
            if arg & !1 != 0 {
                return fail(ctx, 50);
            }
            ctx.kernel.serial_ports.get_mut(&h).unwrap().mask = arg;
        }
        "GetCommMask" => {
            if !output(ctx, arg, 4) {
                return fail(ctx, 87);
            }
            ctx.cpu
                .write_mem(arg, &ctx.kernel.serial_ports[&h].mask.to_le_bytes())?;
        }
        "WaitCommEvent" => {
            if !output(ctx, arg, 4) {
                return fail(ctx, 87);
            }
            if ctx.arg_u32(2)? != 0 {
                return fail(ctx, 50);
            }
            let port = ctx.kernel.serial_ports.get_mut(&h).unwrap();
            if let Err(e) = port.poll() {
                return fail(ctx, e);
            }
            if port.mask != 0 && port.queue.is_empty() {
                return crate::bluetooth::retry(ctx);
            }
            let mask = port.mask;
            ctx.cpu.write_mem(arg, &mask.to_le_bytes())?;
        }
        "ClearCommError" => {
            let stat = ctx.arg_u32(2)?;
            if (arg != 0 && !output(ctx, arg, 4)) || (stat != 0 && !output(ctx, stat, 12)) {
                return fail(ctx, 87);
            }
            let port = ctx.kernel.serial_ports.get_mut(&h).unwrap();
            if let Err(e) = port.poll() {
                return fail(ctx, e);
            }
            let count = port.queue.len() as u32;
            if arg != 0 {
                ctx.cpu.write_mem(arg, &0u32.to_le_bytes())?;
            }
            if stat != 0 {
                ctx.cpu.write_mem(
                    stat,
                    &[0u32, count, 0]
                        .into_iter()
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>(),
                )?;
            }
        }
        "PurgeComm" => {
            if arg & !15 != 0 {
                return fail(ctx, 87);
            }
            if arg & 8 != 0 {
                ctx.kernel.serial_ports.get_mut(&h).unwrap().queue.clear();
            }
        }
        "SetupComm" => {}
        "EscapeCommFunction" => {
            if !(1..=9).contains(&arg) {
                return fail(ctx, 87);
            }
        }
        "GetCommModemStatus" => {
            if !output(ctx, arg, 4) {
                return fail(ctx, 87);
            }
            ctx.cpu.write_mem(arg, &0xb0u32.to_le_bytes())?;
        }
        _ => return fail(ctx, 50),
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}
