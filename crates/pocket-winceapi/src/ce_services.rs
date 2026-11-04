//! Service-manager queries for the emulated CE device.
//! BTH1: is a stream driver; it must not be confused with the absent BTS1:
//! Bluetooth shell service that GTShell tries to unload on startup.
use crate::{CallCtx, WinCeDispatcher};
use pocket_cpu::Prot;
use pocket_kernel::{DispatchOutcome, KernelError};
pub fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", "GetServiceHandle", get);
    d.register_handler("coredll.dll", "DeregisterService", deregister);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
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
