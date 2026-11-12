use crate::{coredll::read_wstr, CallCtx, WinCeDispatcher};
use pocket_cpu::Prot;
use pocket_kernel::registry::{QueueSubscription, RegistryValue};
use pocket_kernel::{DispatchOutcome, KernelError};

const E_INVALIDARG: u32 = 0x8007_0057;
const E_NOTIMPL: u32 = 0x8000_4001;
const E_OUTOFMEMORY: u32 = 0x8007_000e;
const FILE_NOT_FOUND: u32 = 0x8007_0002;
const UNSUPPORTED_TYPE: u32 = 0x8007_065e;

pub(super) fn register(dispatcher: &mut WinCeDispatcher) {
    for (name, handler) in [
        ("RegistryGetDWORD", get_dword as crate::Handler),
        ("RegistryNotifyMsgQueue", notify_msg_queue),
        ("RegistryCloseNotification", close_notification),
    ] {
        dispatcher.register_handler("aygshell.dll", name, handler);
    }
}

fn optional_string(ctx: &mut CallCtx<'_>, ptr: u32) -> Result<String, KernelError> {
    if ptr == 0 {
        return Ok(String::new());
    }
    Ok(String::from_utf16_lossy(&read_wstr(ctx, ptr, 260)?))
}

fn target(ctx: &mut CallCtx<'_>) -> Result<Option<(String, String)>, KernelError> {
    let key = ctx.arg_u32(0)?;
    let subkey = ctx.arg_u32(1)?;
    let name = ctx.arg_u32(2)?;
    let subkey = optional_string(ctx, subkey)?;
    let name = optional_string(ctx, name)?;
    Ok(ctx
        .kernel
        .registry
        .resolve(key, &subkey)
        .map(|path| (path, name)))
}

fn get_dword(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let out = ctx.arg_u32(3)?;
    if out == 0 || ctx.cpu.check_guest_access(out, 4, Prot::WRITE).is_err() {
        return Ok(DispatchOutcome::ReturnedR0(E_INVALIDARG));
    }
    let Some((path, name)) = target(ctx)? else {
        return Ok(DispatchOutcome::ReturnedR0(E_INVALIDARG));
    };
    let result = match ctx.kernel.registry.value(&path, &name) {
        Some(RegistryValue::Dword(value)) => {
            ctx.cpu.write_mem(out, &value.to_le_bytes())?;
            0
        }
        Some(_) => UNSUPPORTED_TYPE,
        None => FILE_NOT_FOUND,
    };
    Ok(DispatchOutcome::ReturnedR0(result))
}

fn notify_msg_queue(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let queue = ctx.arg_u32(3)?;
    let user_data = ctx.arg_u32(4)?;
    let condition = ctx.arg_u32(5)?;
    let out = ctx.arg_u32(6)?;
    if queue == 0 || out == 0 || ctx.cpu.check_guest_access(out, 4, Prot::WRITE).is_err() {
        return Ok(DispatchOutcome::ReturnedR0(E_INVALIDARG));
    }
    let queue = String::from_utf16_lossy(&read_wstr(ctx, queue, 260)?);
    let Some((path, value)) = target(ctx)? else {
        return Ok(DispatchOutcome::ReturnedR0(E_INVALIDARG));
    };
    if queue.is_empty() {
        return Ok(DispatchOutcome::ReturnedR0(E_INVALIDARG));
    }
    if condition != 0 {
        return Ok(DispatchOutcome::ReturnedR0(E_NOTIMPL));
    }
    let previous = ctx.kernel.registry.value(&path, &value);
    let Some(handle) = ctx
        .kernel
        .registry_notifications
        .register(QueueSubscription {
            path,
            value,
            queue,
            user_data,
            previous,
        })
    else {
        return Ok(DispatchOutcome::ReturnedR0(E_OUTOFMEMORY));
    };
    if let Err(error) = ctx.cpu.write_mem(out, &handle.to_le_bytes()) {
        ctx.kernel.registry_notifications.close(handle);
        return Err(error.into());
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn close_notification(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let closed = ctx.kernel.registry_notifications.close(handle);
    Ok(DispatchOutcome::ReturnedR0(if closed {
        0
    } else {
        E_INVALIDARG
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;

    #[test]
    fn dword_reads_and_change_messages_follow_the_guest_registry() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let key = r"HKLM\Software\Test";
        for (address, text) in [
            (0x1100, r"Software\Test"),
            (0x1200, "State"),
            (0x1300, "Notify"),
        ] {
            let bytes: Vec<_> = text
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect();
            cpu.write_mem(address, &bytes).unwrap();
        }
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "aygshell.dll",
                "RegistryGetDWORD",
                &[0x80000002, 0x1100, 0x1200, 0x1500]
            ),
            DispatchOutcome::ReturnedR0(FILE_NOT_FOUND)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "aygshell.dll",
                "RegistryNotifyMsgQueue",
                &[0x80000002, 0x1100, 0x1200, 0x1300, 42, 0, 0x1500]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        let notification = cpu.read_u32_le(0x1500).unwrap();
        let options: Vec<_> = [20u32, 0, 4, 32, 1]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        cpu.write_mem(0x1600, &options).unwrap();
        let DispatchOutcome::ReturnedR0(queue) = call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "CreateMsgQueue",
            &[0x1300, 0x1600],
        ) else {
            panic!("expected queue handle");
        };
        kernel
            .registry
            .set_value(key, "State", RegistryValue::Dword(7));
        kernel.sync_power_notifications();
        let message = &kernel.msg_queues[&queue].messages[0];
        assert_eq!(
            message,
            &[notification, 42, 4, 7]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "aygshell.dll",
                "RegistryGetDWORD",
                &[0x80000002, 0x1100, 0x1200, 0x1500]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_u32_le(0x1500).unwrap(), 7);
    }
}
