//! Noreturn failure boundary for MSVC CE C++ throws. Native C++ unwinding is
//! not implemented; returning normally would execute an impossible continuation.
use crate::{CallCtx, WinCeDispatcher};
use pocket_cpu::{Cpu, Prot};
use pocket_kernel::{DispatchOutcome, KernelError};

const CPP_EXCEPTION: u32 = 0xe06d_7363;

pub fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", "__CxxThrowException", throw);
}

fn read(cpu: &mut dyn Cpu, pointer: u32, size: u32) -> Option<Vec<u8>> {
    if pointer == 0
        || pointer.checked_add(size).is_none()
        || cpu.check_guest_access(pointer, size, Prot::READ).is_err()
    {
        return None;
    }
    cpu.read_mem(pointer, size).ok()
}

fn exception_type(cpu: &mut dyn Cpu, info: u32) -> Option<String> {
    let info = read(cpu, info, 16)?;
    let catch_array = u32::from_le_bytes(info[12..16].try_into().ok()?);
    let catches = read(cpu, catch_array, 8)?;
    let count = u32::from_le_bytes(catches[..4].try_into().ok()?);
    if !(1..=64).contains(&count) {
        return None;
    }
    let catch_type = u32::from_le_bytes(catches[4..8].try_into().ok()?);
    let catch_type = read(cpu, catch_type, 8)?;
    let descriptor = u32::from_le_bytes(catch_type[4..8].try_into().ok()?);
    let name = descriptor.checked_add(8)?;
    let mut bytes = Vec::new();
    for offset in 0..512 {
        let byte = read(cpu, name.checked_add(offset)?, 1)?[0];
        if byte == 0 {
            return String::from_utf8(bytes)
                .ok()
                .filter(|name| !name.is_empty());
        }
        if !byte.is_ascii_graphic() {
            return None;
        }
        bytes.push(byte);
    }
    None
}

fn throw(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let info = ctx.arg_u32(1)?;
    let kind = exception_type(ctx.cpu, info).unwrap_or_else(|| "unknown type".into());
    // EmulatorStub throws CErrorWin32Code when DMA7:, the Device Emulator
    // host channel, fails to open. Keep that driver failure visible rather
    // than inventing DMA traffic or pretending the C++ exception was caught.
    let reason = format!("unhandled MSVC C++ exception 0x{CPP_EXCEPTION:08x}: {kind} (object=0x{object:08x}, ThrowInfo=0x{info:08x}); native C++ unwinding is unavailable");
    log::error!("{reason}");
    ctx.kernel.record_process_exit(CPP_EXCEPTION);
    Ok(DispatchOutcome::Halt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;

    #[test]
    fn cxx_throw_is_noreturn_and_reports_bounded_type_metadata() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        cpu.write_mem(0x140c, &0x1500u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x1500, &1u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x1504, &0x1600u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x1604, &0x1700u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x1708, b".?AVCErrorWin32Code@@\0").unwrap();
        assert_eq!(
            exception_type(&mut cpu, 0x1400).as_deref(),
            Some(".?AVCErrorWin32Code@@")
        );
        let outcome = call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "__CxxThrowException",
            &[0x1800, 0x1400],
        );
        assert_eq!(outcome, DispatchOutcome::Halt);
        assert_eq!(kernel.process_exit_code, Some(CPP_EXCEPTION));
        assert_eq!(
            kernel
                .object_handles
                .exit_code(kernel.object_handles.process_id(), None),
            Some(CPP_EXCEPTION)
        );
    }

    #[test]
    fn invalid_throw_metadata_cannot_return_success_or_read_protected_memory() {
        for pointer in [0, 0xffff_fff8, 0x6000, 0x5000] {
            let (mut cpu, mut kernel, mut dispatcher) = setup();
            cpu.map_region(0x5000, 4096, Prot::NONE).unwrap();
            assert_eq!(exception_type(&mut cpu, pointer), None);
            let outcome = call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "__CxxThrowException",
                &[0, pointer],
            );
            assert_eq!(outcome, DispatchOutcome::Halt);
            assert_eq!(kernel.process_exit_code, Some(CPP_EXCEPTION));
        }
    }
}
