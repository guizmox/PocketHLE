//! Process-local CE debug-zone metadata; no host registry or debugger is used.
use crate::{CallCtx, WinCeDispatcher};
use pocket_cpu::Prot;
use pocket_kernel::{ce::DebugZones, DispatchOutcome, KernelError, PROCESS_INSTANCE_HANDLE};

const PARAM_SIZE: u32 = 1092;

pub(crate) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", "RegisterDbgZones", register_zones);
}

fn failed(ctx: &mut CallCtx<'_>, error: u32) -> Result<DispatchOutcome, KernelError> {
    ctx.kernel
        .thread_last_errors
        .insert(ctx.kernel.current_thread, error);
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn register_zones(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let module = ctx.arg_u32(0)?;
    let p = ctx.arg_u32(1)?;
    let module = if module == 0 || module == PROCESS_INSTANCE_HANDLE {
        ctx.kernel.image_base
    } else {
        module
    };
    if module != ctx.kernel.image_base
        && !ctx.kernel.modules.iter().any(|entry| entry.base == module)
        && !ctx.kernel.dynamic_exports.contains_key(&module)
    {
        return failed(ctx, 6);
    }
    if p == 0
        || ctx
            .cpu
            .check_guest_access(p, PARAM_SIZE, Prot::READ)
            .is_err()
    {
        return failed(ctx, 87);
    }
    let bytes = ctx.cpu.read_mem(p, PARAM_SIZE)?;
    let field = |index: usize| {
        let words: Vec<_> = bytes[index * 64..(index + 1) * 64]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|unit| *unit != 0)
            .collect();
        String::from_utf16_lossy(&words)
    };
    // cgacutil's native runtime registers its module's default debug mask.
    // Retain the bounded SDK record; registration does not start a debugger
    // or make .NET/hardware services ready.
    ctx.kernel.ce.debug_zones.insert(
        module,
        DebugZones {
            address: p,
            name: field(0),
            zones: std::array::from_fn(|index| field(index + 1)),
            mask: u32::from_le_bytes(bytes[1088..1092].try_into().unwrap()),
        },
    );
    Ok(DispatchOutcome::ReturnedR0(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;

    #[test]
    fn module_debug_metadata_preserves_masks_and_rejects_partial_or_unmapped_records() {
        let (mut cpu, mut k, mut d) = setup();
        let mut bytes = [0u8; PARAM_SIZE as usize];
        for (index, text) in [(0, "CE Runtime"), (1, "Errors"), (16, "Verbose")] {
            for (offset, unit) in text.encode_utf16().enumerate() {
                bytes[index * 64 + offset * 2..index * 64 + offset * 2 + 2]
                    .copy_from_slice(&unit.to_le_bytes());
            }
        }
        bytes[1088..].copy_from_slice(&0x8000_8001u32.to_le_bytes());
        cpu.write_mem(0x1800, &bytes).unwrap();
        k.thread_last_errors.insert(0, 123);
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RegisterDbgZones",
                &[0, 0x1800]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        let state = &k.ce.debug_zones[&k.image_base];
        assert_eq!(state.name, "CE Runtime");
        assert_eq!(state.zones[0], "Errors");
        assert_eq!(state.zones[15], "Verbose");
        assert_eq!(state.mask, 0x8000_8001);
        assert_eq!(k.thread_last_errors[&0], 123);
        for pointer in [0, 0x4ff0, u32::MAX] {
            assert_eq!(
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "coredll.dll",
                    "RegisterDbgZones",
                    &[0, pointer]
                ),
                DispatchOutcome::ReturnedR0(0)
            );
            assert_eq!(k.thread_last_errors[&0], 87);
            assert_eq!(k.ce.debug_zones[&k.image_base].mask, 0x8000_8001);
        }
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RegisterDbgZones",
                &[0xbaad, 0x1800]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(k.thread_last_errors[&0], 6);
    }
}
