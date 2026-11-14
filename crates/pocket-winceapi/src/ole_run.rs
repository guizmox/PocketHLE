//! OleRun delegates to the guest's IRunnableObject, without host COM activation.
use crate::{native_com, CallCtx};
use pocket_cpu::regs::ArmReg;
use pocket_kernel::{
    ce::{ComRun, ComRunStage},
    DispatchOutcome, GuestCallFrame, KernelError,
};

const POINTER: u32 = 0x8000_4003;
const UNEXPECTED: u32 = 0x8000_ffff;
const NO_MEMORY: u32 = 0x8007_000e;
const RUNNABLE: [u8; 16] = [0x26, 1, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46];

pub(crate) fn run(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
    if let Some(index) = ctx.kernel.ce.com.runs.iter().rposition(|state| {
        state.thread == ctx.kernel.current_thread
            && state.thunk == ctx.thunk.thunk_va
            && state.frame.sp == sp
    }) {
        let state = ctx.kernel.ce.com.runs.remove(index);
        return resume(ctx, state);
    }
    let object = ctx.arg_u32(0)?;
    let Some(address) = native_com::method(ctx, object, 0)? else {
        return Ok(DispatchOutcome::ReturnedR0(POINTER));
    };
    let Some(scratch) = ctx.kernel.heap.alloc(20) else {
        return Ok(DispatchOutcome::ReturnedR0(NO_MEMORY));
    };
    let mut data = [0u8; 20];
    data[..16].copy_from_slice(&RUNNABLE);
    if let Err(error) = ctx.cpu.write_mem(scratch, &data) {
        ctx.kernel.heap.free(scratch);
        return Err(error.into());
    }
    // CE6 Messenger calls OleRun on its own COM object during UI startup.
    // QueryInterface/Run/Release can themselves call HLE or yield; keep their
    // continuation on the owning thread until the original stack returns.
    let state = ComRun {
        thread: ctx.kernel.current_thread,
        thunk: ctx.thunk.thunk_va,
        frame: GuestCallFrame {
            args: [object, ctx.arg_u32(1)?, ctx.arg_u32(2)?, ctx.arg_u32(3)?],
            sp,
            lr: ctx.cpu.read_reg(ArmReg::Lr)?,
        },
        scratch,
        object,
        stage: ComRunStage::Query,
        result: 0,
    };
    jump(ctx, state, address, [object, scratch, scratch + 16, 0])
}

fn jump(
    ctx: &mut CallCtx<'_>,
    state: ComRun,
    address: u32,
    args: [u32; 4],
) -> Result<DispatchOutcome, KernelError> {
    for (index, value) in args.into_iter().enumerate() {
        ctx.cpu.write_arg(index as u8, value)?;
    }
    ctx.cpu.write_reg(ArmReg::Lr, state.thunk)?;
    ctx.kernel.ce.com.runs.push(state);
    Ok(DispatchOutcome::JumpTo(address))
}

fn complete(ctx: &mut CallCtx<'_>, state: ComRun) -> Result<DispatchOutcome, KernelError> {
    ctx.kernel.heap.free(state.scratch);
    for (index, value) in state.frame.args.into_iter().enumerate() {
        ctx.cpu.write_arg(index as u8, value)?;
    }
    ctx.cpu.write_reg(ArmReg::Sp, state.frame.sp)?;
    ctx.cpu.write_reg(ArmReg::Lr, state.frame.lr)?;
    Ok(DispatchOutcome::ReturnedR0(state.result))
}

fn release(ctx: &mut CallCtx<'_>, mut state: ComRun) -> Result<DispatchOutcome, KernelError> {
    let Some(address) = native_com::method(ctx, state.object, 2)? else {
        state.result = UNEXPECTED;
        return complete(ctx, state);
    };
    state.stage = ComRunStage::Release;
    let object = state.object;
    jump(ctx, state, address, [object, 0, 0, 0])
}

fn resume(ctx: &mut CallCtx<'_>, mut state: ComRun) -> Result<DispatchOutcome, KernelError> {
    match state.stage {
        ComRunStage::Query => {
            // OleRun succeeds when QI fails: an object without a runnable
            // interface has no transition to request. Run's HRESULT, when
            // called, is preserved across releasing the acquired interface.
            if ctx.cpu.read_reg(ArmReg::R0)? & 0x8000_0000 != 0 {
                return complete(ctx, state);
            }
            state.object = ctx.cpu.read_u32_le(state.scratch + 16)?;
            if state.object == 0 {
                state.result = UNEXPECTED;
                return complete(ctx, state);
            }
            let Some(address) = native_com::method(ctx, state.object, 4)? else {
                state.result = UNEXPECTED;
                return release(ctx, state);
            };
            state.stage = ComRunStage::Run;
            let object = state.object;
            jump(ctx, state, address, [object, 0, 0, 0])
        }
        ComRunStage::Run => {
            state.result = ctx.cpu.read_reg(ArmReg::R0)?;
            release(ctx, state)
        }
        ComRunStage::Release => complete(ctx, state),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bluetooth::tests::setup, WinCeDispatcher};
    use pocket_cpu::{stub::StubCpu, Cpu, Prot};
    use pocket_kernel::{Dispatcher, Heap, KernelState, Thunk};
    use pocket_pe::ImportBinding;

    fn fixture() -> (StubCpu, KernelState, WinCeDispatcher, Thunk) {
        let (mut cpu, mut k, d) = setup();
        cpu.map_region(0x6000, 0x1000, Prot::ALL).unwrap();
        cpu.map_region(0x5000_0000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        k.heap = Heap::new(0x5000_0000, 0x10000);
        cpu.write_mem(0x1100, &0x1200u32.to_le_bytes()).unwrap();
        for (slot, method) in [(0, 0x6000u32), (2, 0x6080), (4, 0x6040)] {
            cpu.write_mem(0x1200 + slot * 4, &method.to_le_bytes())
                .unwrap();
        }
        for (index, value) in [0x1100, 11, 22, 33].into_iter().enumerate() {
            cpu.write_arg(index as u8, value).unwrap();
        }
        cpu.write_reg(ArmReg::Lr, 0x4321).unwrap();
        let t = Thunk {
            thunk_va: 0x7000_0000,
            iat_va: 0,
            dll: "ole32.dll".into(),
            binding: ImportBinding::Name("OleRun".into()),
            friendly_name: Some("OleRun".into()),
        };
        (cpu, k, d, t)
    }

    #[test]
    fn unavailable_runnable_interfaces_succeed_without_run_and_restore_the_caller() {
        for error in [0x8000_4002u32, 0x8000_4005, 0x8007_0057] {
            let (mut cpu, mut k, mut d, t) = fixture();
            assert_eq!(
                d.dispatch(&mut cpu, &t, &mut k).unwrap(),
                DispatchOutcome::JumpTo(0x6000)
            );
            let scratch = k.ce.com.runs[0].scratch;
            assert_eq!(cpu.read_mem(scratch, 16).unwrap(), RUNNABLE);
            cpu.write_reg(ArmReg::R0, error).unwrap();
            assert_eq!(
                d.dispatch(&mut cpu, &t, &mut k).unwrap(),
                DispatchOutcome::ReturnedR0(0)
            );
            assert!(k.ce.com.runs.is_empty());
            assert_eq!(k.heap.msize(scratch), None);
            assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x4321);
            assert_eq!(cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
            assert_eq!(cpu.read_reg(ArmReg::R1).unwrap(), 11);
        }
    }

    #[test]
    fn run_failures_release_the_query_reference_without_losing_the_hresult() {
        for result in [0u32, 1, 0x8004_0112] {
            let (mut cpu, mut k, mut d, t) = fixture();
            assert_eq!(
                d.dispatch(&mut cpu, &t, &mut k).unwrap(),
                DispatchOutcome::JumpTo(0x6000)
            );
            let output = cpu.read_reg(ArmReg::R2).unwrap();
            cpu.write_mem(output, &0x1100u32.to_le_bytes()).unwrap();
            cpu.write_reg(ArmReg::R0, 0).unwrap();
            assert_eq!(
                d.dispatch(&mut cpu, &t, &mut k).unwrap(),
                DispatchOutcome::JumpTo(0x6040)
            );
            assert_eq!(cpu.read_reg(ArmReg::R1).unwrap(), 0);
            cpu.write_reg(ArmReg::R0, result).unwrap();
            assert_eq!(
                d.dispatch(&mut cpu, &t, &mut k).unwrap(),
                DispatchOutcome::JumpTo(0x6080)
            );
            cpu.write_reg(ArmReg::R0, 99).unwrap();
            assert_eq!(
                d.dispatch(&mut cpu, &t, &mut k).unwrap(),
                DispatchOutcome::ReturnedR0(result)
            );
            assert!(k.ce.com.runs.is_empty());
            assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x4321);
        }
    }

    #[test]
    fn invalid_objects_and_failed_runnable_calls_do_not_leak_scratch() {
        let (mut cpu, mut k, mut d, t) = fixture();
        cpu.write_reg(ArmReg::R0, 0).unwrap();
        assert_eq!(
            d.dispatch(&mut cpu, &t, &mut k).unwrap(),
            DispatchOutcome::ReturnedR0(POINTER)
        );
        cpu.write_reg(ArmReg::R0, 0x1100).unwrap();
        assert_eq!(
            d.dispatch(&mut cpu, &t, &mut k).unwrap(),
            DispatchOutcome::JumpTo(0x6000)
        );
        let scratch = k.ce.com.runs[0].scratch;
        let output = cpu.read_reg(ArmReg::R2).unwrap();
        cpu.write_mem(output, &0x1100u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x1210, &0x1000u32.to_le_bytes()).unwrap(); // not executable
        cpu.write_reg(ArmReg::R0, 0).unwrap();
        assert_eq!(
            d.dispatch(&mut cpu, &t, &mut k).unwrap(),
            DispatchOutcome::JumpTo(0x6080)
        );
        cpu.write_reg(ArmReg::R0, 1).unwrap();
        assert_eq!(
            d.dispatch(&mut cpu, &t, &mut k).unwrap(),
            DispatchOutcome::ReturnedR0(UNEXPECTED)
        );
        assert!(k.ce.com.runs.is_empty());
        assert_eq!(k.heap.msize(scratch), None);
    }

    #[test]
    fn nested_ole_runs_resume_only_at_their_own_thread_and_stack() {
        let (mut cpu, mut k, mut d, t) = fixture();
        d.dispatch(&mut cpu, &t, &mut k).unwrap();
        cpu.write_reg(ArmReg::Sp, 0x3fc0).unwrap();
        cpu.write_reg(ArmReg::R0, 0x1100).unwrap();
        d.dispatch(&mut cpu, &t, &mut k).unwrap();
        assert_eq!(k.ce.com.runs.len(), 2);
        cpu.write_reg(ArmReg::R0, 0x8000_4002).unwrap();
        assert_eq!(
            d.dispatch(&mut cpu, &t, &mut k).unwrap(),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(k.ce.com.runs.len(), 1);
        cpu.write_reg(ArmReg::Sp, 0x4000).unwrap();
        k.current_thread = 1;
        cpu.write_reg(ArmReg::R0, 0x1100).unwrap();
        assert_eq!(
            d.dispatch(&mut cpu, &t, &mut k).unwrap(),
            DispatchOutcome::JumpTo(0x6000)
        );
        cpu.write_reg(ArmReg::R0, 0x8000_4002).unwrap();
        assert_eq!(
            d.dispatch(&mut cpu, &t, &mut k).unwrap(),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(k.ce.com.runs.len(), 1);
        k.current_thread = 0;
        cpu.write_reg(ArmReg::R0, 0x8000_4002).unwrap();
        assert_eq!(
            d.dispatch(&mut cpu, &t, &mut k).unwrap(),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x4321);
        assert!(k.ce.com.runs.is_empty());
    }
}
