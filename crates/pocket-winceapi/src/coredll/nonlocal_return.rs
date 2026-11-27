//! Nonlocal jumps abort DLL initialization through the loader's own detach
//! continuation before retiring abandoned WinCE callback frames.
use super::*;
use crate::guest_callbacks::{self, Frame, WINCE};
const JUMP: &str = "__PocketHLE_WinCE_NonlocalReturn";
const ABORT_MODULE: &str = "__PocketHLE_WinCE_AbortModuleAttach";
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", JUMP, jump);
    d.register_handler("coredll.dll", ABORT_MODULE, abort_module);
}
pub(super) fn pending(ctx: &CallCtx<'_>, sp: u32) -> bool {
    ctx.kernel
        .module_attach_frames
        .iter()
        .any(|f| f.thread == ctx.kernel.current_thread && f.frame.sp <= sp)
}
pub(super) fn start(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::JumpTo(guest_callbacks::service(
        ctx, JUMP,
    )?))
}
fn jump(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let owner = guest_callbacks::owner(ctx, WINCE.continue_name)?;
    let f = Frame::enter(ctx, WINCE, owner)?;
    let buffer = f.arg(ctx, 0)?;
    let value = f.arg(ctx, 1)?;
    let blob = ctx.cpu.read_mem(buffer, 40)?;
    let sp = u32::from_le_bytes(blob[32..36].try_into().unwrap());
    // Revalidate the target after every real DLL callback. The jump buffer
    // remains guest-owned and can itself be changed by an exception handler.
    ctx.cpu.check_guest_access(sp, 4, Prot::READ)?;
    if pending(ctx, sp) {
        return f.call_service(ctx, 1, ABORT_MODULE, &[sp]);
    }
    f.finish(ctx, 0)?;
    ctx.cpu.write_arg(0, buffer)?;
    ctx.cpu.write_arg(1, value)?;
    super::longjmp(ctx)
}
fn abort_module(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if let Some(outcome) = super::finish_module_attach(ctx)? {
        return Ok(outcome);
    }
    let target_sp = ctx.arg_u32(0)?;
    let thread = ctx.kernel.current_thread;
    let index = ctx
        .kernel
        .module_attach_frames
        .iter()
        .rposition(|f| f.thread == thread && f.frame.sp <= target_sp)
        .ok_or_else(|| KernelError::Dispatch("Missing DLL exceptional transaction".into()))?;
    // Only modules introduced by this transaction are collected. Successful
    // pre-existing dependencies and other threads' callbacks retain ownership.
    let mut pending = ctx.kernel.module_attach_frames[index].clone();
    let mut seen = std::collections::HashSet::new();
    for base in &pending.cleanup {
        if !seen.insert(*base) || !ctx.kernel.modules.iter().any(|m| m.base == *base) {
            return Err(KernelError::Dispatch(
                "Invalid DLL exceptional cleanup owner".into(),
            ));
        }
    }
    if !pending.detaching {
        pending.sequence = pending.completed.clone();
    } else {
        // The interrupted detach has already been entered. Do not call the
        // same DllMain twice when its own handler performs another longjmp.
        if let Some(module) = ctx
            .kernel
            .modules
            .iter_mut()
            .find(|m| m.base == pending.base)
        {
            module.attached = false;
        }
    }
    pending.detaching = true;
    pending.result = 0;
    pending.error = Some(1114); // ERROR_DLL_INIT_FAILED, not successful loading
    pending.thunk = ctx.thunk.thunk_va;
    pending.frame = GuestCallFrame {
        args: [target_sp, ctx.arg_u32(1)?, ctx.arg_u32(2)?, ctx.arg_u32(3)?],
        lr: ctx.cpu.read_reg(ArmReg::Lr)?,
        sp: ctx.cpu.read_reg(ArmReg::Sp)?,
    };
    ctx.kernel.module_attach_frames.remove(index);
    super::advance_module_callback(ctx, pending)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mfc::tests::drive_with;
    use pocket_cpu::Cpu;
    #[test]
    fn jump_runs_successful_dependency_detach_then_collects_only_failed_attach_images() {
        let (mut cpu, mut k, mut d) = crate::mfc::tests::framework_setup();
        cpu.map_region(0x5000, 0x1000, Prot::READ | Prot::WRITE)
            .unwrap();
        let retained = super::super::tests::lifecycle_test_module(0x30000000, 0x6000);
        let dependency = super::super::tests::lifecycle_test_module(0x31000000, 0x6100);
        let mut failed = super::super::tests::lifecycle_test_module(0x32000000, 0x6200);
        failed.attached = false;
        k.modules = vec![retained, dependency, failed];
        k.module_attach_frames
            .push(pocket_kernel::ModuleAttachFrame {
                sequence: vec![],
                cleanup: vec![0x31000000, 0x32000000],
                completed: vec![0x31000000],
                result: 0x32000000,
                error: None,
                detaching: false,
                base: 0x32000000,
                thunk: 0x70000040,
                thread: 0,
                frame: GuestCallFrame {
                    args: [0; 4],
                    lr: 0x6666,
                    sp: 0x4000,
                },
            });
        for i in 0..10 {
            cpu.write_mem(
                0x2800 + i * 4,
                &(if i == 8 {
                    0x5000u32
                } else if i == 9 {
                    0x12345678
                } else {
                    0xabc00000 + i
                })
                .to_le_bytes(),
            )
            .unwrap();
        }
        cpu.write_arg(0, 0x2800).unwrap();
        cpu.write_arg(1, 9).unwrap();
        let free = k.heap.free_bytes();
        let mut calls = Vec::new();
        let result = drive_with(
            &mut cpu,
            &mut k,
            &mut d,
            pocket_kernel::Thunk {
                thunk_va: 0x70000000,
                iat_va: 0,
                dll: "coredll.dll".into(),
                binding: pocket_pe::ImportBinding::Name("longjmp".into()),
                friendly_name: None,
            },
            128,
            |pc, cpu, k, _| {
                calls.push(pc);
                assert_eq!(pc, 0x6100);
                assert_eq!(cpu.read_arg(0).unwrap(), 0x31000000);
                assert_eq!(cpu.read_arg(1).unwrap(), 0); // DLL_PROCESS_DETACH
                assert_eq!(k.modules.len(), 3); // no image unmapped during DllMain
                1
            },
        );
        assert_eq!(result, 9);
        assert_eq!(calls, [0x6100]);
        assert_eq!(k.modules.len(), 1);
        assert_eq!(k.modules[0].base, 0x30000000);
        assert!(k.modules[0].attached);
        assert!(k.module_attach_frames.is_empty());
        assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x5000);
        assert_eq!(cpu.read_reg(ArmReg::R4).unwrap(), 0xabc00000);
        assert_eq!(k.heap.free_bytes(), free);
        assert_eq!(k.thread_last_errors[&0], 1114);
    }
}
