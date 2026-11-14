//! Adapter discovery for CE applications. No CE network adapter is installed.
//! CE6 mstscax imports this query at load time. Its ROM iphlpapi DllMain
//! otherwise enters the ROM security-provider driver before the RDP UI opens.
use crate::{CallCtx, WinCeDispatcher};
use pocket_cpu::Prot;
use pocket_kernel::{DispatchOutcome, KernelError};

pub(crate) fn register(dispatcher: &mut WinCeDispatcher) {
    dispatcher.register_handler(
        "iphlpapi.dll",
        "GetAdaptersAddresses",
        get_adapters_addresses,
    );
    for name in ["GetAdaptersInfo", "GetInterfaceInfo", "GetNetworkParams"] {
        dispatcher.register_handler("iphlpapi.dll", name, empty_query);
    }
    for name in ["IpReleaseAddress", "IpRenewAddress"] {
        dispatcher.register_handler("iphlpapi.dll", name, unavailable_address);
    }
}

fn empty_query(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let size = ctx.arg_u32(1)?;
    if size == 0 || ctx.cpu.check_guest_access(size, 4, Prot::WRITE).is_err() {
        return Ok(DispatchOutcome::ReturnedR0(87));
    }
    ctx.cpu.write_mem(size, &0u32.to_le_bytes())?;
    Ok(DispatchOutcome::ReturnedR0(232))
}
fn unavailable_address(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(if ctx.arg_u32(0)? == 0 {
        87
    } else {
        50
    }))
}

fn get_adapters_addresses(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let family = ctx.arg_u32(0)?;
    let reserved = ctx.arg_u32(2)?;
    let size = ctx.arg_u32(4)?;
    if !matches!(family, 0 | 2 | 23)
        || reserved != 0
        || size == 0
        || ctx.cpu.check_guest_access(size, 4, Prot::WRITE).is_err()
    {
        return Ok(DispatchOutcome::ReturnedR0(87));
    }
    ctx.cpu.write_mem(size, &0u32.to_le_bytes())?;
    // ERROR_NO_DATA: there are no adapter records, rather than S_OK with an
    // uninitialized list. Winsock's host socket transport is independent.
    Ok(DispatchOutcome::ReturnedR0(232))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
    #[test]
    fn adapter_query_reports_an_empty_device_without_writing_an_adapter_record() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        for family in [0, 2, 23] {
            cpu.write_mem(0x1100, &4096u32.to_le_bytes()).unwrap();
            cpu.write_mem(0x1200, &[0xa5; 16]).unwrap();
            assert_eq!(
                call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "iphlpapi.dll",
                    "GetAdaptersAddresses",
                    &[family, 0, 0, 0x1200, 0x1100]
                ),
                DispatchOutcome::ReturnedR0(232)
            );
            assert_eq!(cpu.read_u32_le(0x1100).unwrap(), 0);
            assert_eq!(cpu.read_mem(0x1200, 16).unwrap(), [0xa5; 16]);
        }
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "iphlpapi.dll",
                "GetAdaptersAddresses",
                &[1, 0, 0, 0, 0x1100]
            ),
            DispatchOutcome::ReturnedR0(87)
        );
    }
}
