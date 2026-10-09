//! Explicit offline Winsock boundary. No sockets are created and no host
//! network traffic is issued. A failed recv must not look like a clean EOF.
use crate::{CallCtx, WinCeDispatcher};
use pocket_kernel::{DispatchOutcome, KernelError};

const SOCKET_ERROR: u32 = u32::MAX;
const WSASYSNOTREADY: u32 = 10091;
const WSANOTINITIALISED: u32 = 10093;

pub fn register(dispatcher: &mut WinCeDispatcher) {
    dispatcher.register_handler("ws2.dll", "recv", recv);
    dispatcher.register_handler("ws2.dll", "WSAStartup", startup);
    dispatcher.register_handler("ws2.dll", "WSACleanup", not_initialized);
    dispatcher.register_handler("ws2.dll", "WSAGetLastError", get_last_error);
    dispatcher.register_handler("ws2.dll", "WSASetLastError", set_last_error);
}

fn startup(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // WSAStartup reports its own failure directly. Do not publish WSADATA or
    // claim successful initialization of a network subsystem that is absent.
    Ok(DispatchOutcome::ReturnedR0(WSASYSNOTREADY))
}

fn not_initialized(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    ctx.kernel.winsock_last_errors.insert(ctx.kernel.current_thread, WSANOTINITIALISED);
    Ok(DispatchOutcome::ReturnedR0(SOCKET_ERROR))
}

fn recv(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // Initialization never succeeds in this offline boundary. In particular,
    // the demo's recv(0, NULL, 4096, 0) must neither write NULL nor return EOF.
    not_initialized(ctx)
}

fn get_last_error(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(*ctx.kernel.winsock_last_errors
        .get(&ctx.kernel.current_thread).unwrap_or(&0)))
}

fn set_last_error(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let error = ctx.arg_u32(0)?;
    ctx.kernel.winsock_last_errors.insert(ctx.kernel.current_thread, error);
    Ok(DispatchOutcome::ReturnedR0(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_cpu::{stub::StubCpu, Cpu, regs::ArmReg};
    use pocket_kernel::{Dispatcher, Thunk};
    use pocket_pe::ImportBinding;

    #[test]
    fn gizmondo_rom_ordinals_resolve_the_offline_handlers() {
        assert_eq!(crate::ordinals::lookup("WS2.dll", 35).as_deref(), Some("WSAStartup"));
        assert_eq!(crate::ordinals::lookup("ws2.dll", 71).as_deref(), Some("recv"));
        assert_eq!(crate::ordinals::lookup("btd.dll", 31).as_deref(), Some("COM_Open"));
        let mut cpu = StubCpu::new();
        let mut kernel = crate::gx::tests::fresh_kernel();
        let mut dispatcher = WinCeDispatcher::new();
        let thunk = Thunk { thunk_va: 0, iat_va: 0, dll: "WS2.dll".into(),
            binding: ImportBinding::Ordinal(35), friendly_name: None };
        assert_eq!(dispatcher.dispatch(&mut cpu, &thunk, &mut kernel).unwrap(),
            DispatchOutcome::ReturnedR0(WSASYSNOTREADY));
    }

    #[test]
    fn offline_recv_and_thread_errors_are_dispatched_without_touching_buffers() {
        let mut cpu = StubCpu::new();
        let mut kernel = crate::gx::tests::fresh_kernel();
        let mut dispatcher = WinCeDispatcher::new();
        let call = |name: &str, dispatcher: &mut WinCeDispatcher, cpu: &mut StubCpu, kernel: &mut pocket_kernel::KernelState| {
            let thunk = Thunk { thunk_va: 0, iat_va: 0, dll: "WS2.dll".into(),
                binding: ImportBinding::Name(name.into()), friendly_name: Some(name.into()) };
            dispatcher.dispatch(cpu, &thunk, kernel).unwrap()
        };
        // Reproduce the demo's NULL receive buffer: no access or fabricated EOF.
        cpu.write_reg(ArmReg::R0, 0).unwrap();
        cpu.write_reg(ArmReg::R1, 0).unwrap();
        cpu.write_reg(ArmReg::R2, 4096).unwrap();
        assert_eq!(call("recv", &mut dispatcher, &mut cpu, &mut kernel), DispatchOutcome::ReturnedR0(SOCKET_ERROR));
        assert_eq!(call("WSAGetLastError", &mut dispatcher, &mut cpu, &mut kernel), DispatchOutcome::ReturnedR0(WSANOTINITIALISED));
        assert_eq!(call("WSAGetLastError", &mut dispatcher, &mut cpu, &mut kernel), DispatchOutcome::ReturnedR0(WSANOTINITIALISED));
        kernel.current_thread = 1;
        assert_eq!(call("WSAGetLastError", &mut dispatcher, &mut cpu, &mut kernel), DispatchOutcome::ReturnedR0(0));
        cpu.write_reg(ArmReg::R0, 10038).unwrap();
        call("WSASetLastError", &mut dispatcher, &mut cpu, &mut kernel);
        assert_eq!(call("WSAGetLastError", &mut dispatcher, &mut cpu, &mut kernel), DispatchOutcome::ReturnedR0(10038));
        kernel.current_thread = 0;
        assert_eq!(call("WSAStartup", &mut dispatcher, &mut cpu, &mut kernel), DispatchOutcome::ReturnedR0(WSASYSNOTREADY));
        assert_eq!(call("WSACleanup", &mut dispatcher, &mut cpu, &mut kernel), DispatchOutcome::ReturnedR0(SOCKET_ERROR));
        assert_eq!(call("WSAGetLastError", &mut dispatcher, &mut cpu, &mut kernel), DispatchOutcome::ReturnedR0(WSANOTINITIALISED));
        assert!(kernel.thread_last_errors.is_empty());
    }
}
