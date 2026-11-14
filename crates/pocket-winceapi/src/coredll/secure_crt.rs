//! The native CE6 Remote Desktop ActiveX control formats its ATL window class
//! and grows its class-atom array through the checked CRT entry points.
use super::*;

pub(super) fn register(dispatcher: &mut WinCeDispatcher, dll: &str) {
    dispatcher.register_handler(dll, "swprintf_s", swprintf_s);
    dispatcher.register_handler(dll, "vswprintf_s", vswprintf_s);
    dispatcher.register_handler(dll, "_recalloc", recalloc);
}

fn swprintf_s(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    printf_s(ctx, false)
}

fn vswprintf_s(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    printf_s(ctx, true)
}

fn printf_s(ctx: &mut CallCtx<'_>, from_va: bool) -> Result<DispatchOutcome, KernelError> {
    let (destination, capacity, format) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    let valid_destination = destination != 0
        && capacity != 0
        && capacity.checked_mul(2).is_some_and(|bytes| {
            ctx.cpu
                .check_guest_access(destination, bytes, Prot::WRITE)
                .is_ok()
        });
    if !valid_destination {
        set_thread_error(ctx, ERROR_INVALID_PARAMETER);
        return Ok(DispatchOutcome::ReturnedR0(u32::MAX));
    }
    if format == 0 || ctx.cpu.check_guest_access(format, 2, Prot::READ).is_err() {
        ctx.cpu.write_mem(destination, &[0, 0])?;
        set_thread_error(ctx, ERROR_INVALID_PARAMETER);
        return Ok(DispatchOutcome::ReturnedR0(u32::MAX));
    }
    let format = String::from_utf16_lossy(&read_wstr(ctx, format, 0x4000)?);
    let rendered = if from_va {
        let arguments = ctx.arg_u32(3)?;
        render_printf_va(ctx, &format, true, arguments)?
    } else {
        render_printf(ctx, &format, true, 3)?
    };
    let mut units: Vec<u16> = rendered.encode_utf16().collect();
    if units.len() >= capacity as usize {
        // Unlike strsafe, the checked CRT clears the destination on overflow.
        ctx.cpu.write_mem(destination, &[0, 0])?;
        set_thread_error(ctx, ERROR_INVALID_PARAMETER);
        return Ok(DispatchOutcome::ReturnedR0(u32::MAX));
    }
    let length = units.len() as u32;
    units.push(0);
    ctx.cpu.write_mem(destination, &wide_to_bytes(&units))?;
    Ok(DispatchOutcome::ReturnedR0(length))
}

fn recalloc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (pointer, count, element_size) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    let Some(bytes) = count.checked_mul(element_size) else {
        set_thread_error(ctx, 8);
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    // The shared reallocator preserves contents, zeros the grown tail and
    // keeps the original allocation alive on failure.
    do_realloc(ctx, pointer, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;

    #[test]
    fn checked_wide_format_counts_capacity_separately_from_register_and_stack_arguments() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let format: Vec<u16> = "ATL:%08X:%u".encode_utf16().chain([0]).collect();
        cpu.write_mem(0x1100, &wide_to_bytes(&format)).unwrap();
        cpu.write_mem(0x1200, &[0xa5; 42]).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "swprintf_s",
                &[0x1200, 20, 0x1100, 0x1234, 7]
            ),
            DispatchOutcome::ReturnedR0(14)
        );
        let expected: Vec<u16> = "ATL:00001234:7".encode_utf16().chain([0]).collect();
        assert_eq!(cpu.read_mem(0x1200, 30).unwrap(), wide_to_bytes(&expected));
        assert_eq!(cpu.read_mem(0x1228, 2).unwrap(), [0xa5; 2]);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "swprintf_s",
                &[0x1200, 4, 0x1100, 0x1234, 7]
            ),
            DispatchOutcome::ReturnedR0(u32::MAX)
        );
        assert_eq!(cpu.read_mem(0x1200, 2).unwrap(), [0, 0]);
        cpu.write_mem(0x1300, &0x1234u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x1304, &7u32.to_le_bytes()).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "vswprintf_s",
                &[0x1200, 20, 0x1100, 0x1300]
            ),
            DispatchOutcome::ReturnedR0(14)
        );
    }

    #[test]
    fn checked_reallocation_preserves_bytes_zeros_growth_and_rejects_overflow() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        cpu.map_region(0x5000_0000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        kernel.heap = pocket_kernel::Heap::new(0x5000_0000, 0x10000);
        let DispatchOutcome::ReturnedR0(original) = call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "_recalloc",
            &[0, 2, 2],
        ) else {
            panic!("allocation");
        };
        assert_ne!(original, 0);
        cpu.write_mem(original, &[1, 2, 3, 4]).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "_recalloc",
                &[original, u32::MAX, 2]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(kernel.heap.msize(original), Some(4));
        let DispatchOutcome::ReturnedR0(grown) = call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "_recalloc",
            &[original, 4, 2],
        ) else {
            panic!("growth");
        };
        assert_ne!(grown, 0);
        assert_eq!(cpu.read_mem(grown, 8).unwrap(), [1, 2, 3, 4, 0, 0, 0, 0]);
        assert_eq!(kernel.heap.msize(original), None);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "_recalloc",
                &[grown, 0, 2]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(kernel.heap.msize(grown), None);
    }
}
