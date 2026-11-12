//! Service-manager queries for the emulated CE device.
//! BTH1: is a stream driver; it must not be confused with the absent BTS1:
//! Bluetooth shell service that GTShell tries to unload on startup.
use crate::{CallCtx, WinCeDispatcher};
use pocket_cpu::Prot;
use pocket_kernel::{DispatchOutcome, KernelError};
pub fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", "RegisterService", register_service);
    d.register_handler("coredll.dll", "GetServiceHandle", get);
    d.register_handler("coredll.dll", "DeregisterService", deregister);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
    #[test]
    fn register_service_ordinal_reports_a_missing_service_dll() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        for (address, text) in [(0x1000, "ABC"), (0x1100, "missing-service.dll")] {
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
                "coredll.dll",
                "#1509",
                &[0x1000, 0, 0x1100, 1]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "GetLastError",
                &[]
            ),
            DispatchOutcome::ReturnedR0(126)
        );
    }

    #[test]
    fn ce_services_absent_shell_service_is_invalid_handle_not_stream_driver() {
        let (mut cpu, mut k, mut d) = setup();
        cpu.write_mem(
            0x1000,
            &"BTS1:"
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "GetServiceHandle",
                &[0x1000, 0, 0]
            ),
            DispatchOutcome::ReturnedR0(u32::MAX)
        );
        assert_eq!(
            call(&mut cpu, &mut k, &mut d, "coredll.dll", "GetLastError", &[]),
            DispatchOutcome::ReturnedR0(1060)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "DeregisterService",
                &[u32::MAX]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert!(k.vfs.bluetooth.service.is_enabled());
    }
}
fn failed(ctx: &mut CallCtx<'_>, code: u32, value: u32) -> Result<DispatchOutcome, KernelError> {
    crate::coredll::set_thread_error(ctx, code);
    Ok(DispatchOutcome::ReturnedR0(value))
}
fn register_service(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let prefix_ptr = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)?;
    let dll_ptr = ctx.arg_u32(2)?;
    let prefix = match crate::ce_shortcut::text(ctx, prefix_ptr, 4) {
        Ok(prefix) => prefix,
        Err(error) => return failed(ctx, error, 0),
    };
    if index > 9 || prefix.len() != 3 || !prefix.bytes().all(|b| b.is_ascii_alphabetic()) {
        return failed(ctx, 87, 0);
    }
    let dll = match crate::ce_shortcut::text(ctx, dll_ptr, 260) {
        Ok(dll) if !dll.is_empty() => dll,
        Ok(_) => return failed(ctx, 87, 0),
        Err(error) => return failed(ctx, error, 0),
    };
    if ctx.kernel.find_module_file(&dll).is_none() {
        return failed(ctx, 126, 0); // ERROR_MOD_NOT_FOUND
    }
    // A DLL in the VFS is not a running service. There is no Services.exe
    // host to initialize it and dispatch its service entry points.
    failed(ctx, 50, 0) // ERROR_NOT_SUPPORTED
}

fn get(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let prefix = ctx.arg_u32(0)?;
    let dll = ctx.arg_u32(1)?;
    let size = ctx.arg_u32(2)?;
    let prefix = match crate::ce_shortcut::text(ctx, prefix, 6) {
        Ok(n) => n,
        Err(e) => return failed(ctx, e, u32::MAX),
    };
    let bytes = prefix.as_bytes();
    if bytes.len() != 5
        || !bytes[..3].iter().all(u8::is_ascii_alphabetic)
        || !bytes[3].is_ascii_digit()
        || bytes[4] != b':'
    {
        return failed(ctx, 87, u32::MAX);
    }
    if dll != 0 && size == 0 {
        return failed(ctx, 87, u32::MAX);
    }
    if size != 0
        && ctx
            .cpu
            .check_guest_access(size, 4, Prot::READ | Prot::WRITE)
            .is_err()
    {
        return failed(ctx, 87, u32::MAX);
    }
    // No services.exe DLL instance is loaded by this device model. A registry
    // entry or a similarly named stream driver is not a running service.
    // Failure leaves optional output buffers untouched and prevents GTShell
    // from attempting to deregister a spurious zero handle.
    failed(ctx, 1060, u32::MAX)
}
fn deregister(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let _handle = ctx.arg_u32(0)?;
    // No service instance can have been obtained in this device model.
    // Never close an unrelated VFS/device handle or stop a host OS service.
    failed(ctx, 6, 0)
}
