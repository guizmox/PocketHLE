//! The CE6 shell enumerates input methods before creating its taskbar.
//! Expose the existing HLE keyboard through the SDK callback and CLSID ABI.
use crate::{CallCtx, WinCeDispatcher};
use pocket_cpu::{regs::ArmReg, Prot};
use pocket_kernel::{ce::SipEnumeration, DispatchOutcome, GuestCallFrame, KernelError};

const KEYBOARD: [u8; 16] = [
    0x67, 0x96, 0x42, 0x42, 0x04, 0xae, 0xd0, 0x11, 0xa4, 0xf8, 0x00, 0xaa, 0x00, 0xa7, 0x49, 0xb9,
];

pub fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", "SipEnumIM", enumerate);
    d.register_handler("coredll.dll", "SipGetCurrentIM", current);
    d.register_handler("coredll.dll", "SipSetCurrentIM", select);
    d.register_handler(
        "coredll.dll",
        "SipSetDefaultRect",
        crate::shell_keyboard::sip_default_rect,
    );
    d.register_constant("coredll.dll", "SipStatus", 1, status);
}
fn status(_: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn failed(ctx: &mut CallCtx<'_>, code: u32, value: u32) -> Result<DispatchOutcome, KernelError> {
    crate::coredll::set_thread_error(ctx, code);
    Ok(DispatchOutcome::ReturnedR0(value))
}
fn current(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let out = ctx.arg_u32(0)?;
    if out == 0 || ctx.cpu.check_guest_access(out, 16, Prot::WRITE).is_err() {
        return failed(ctx, 87, 0);
    }
    ctx.cpu.write_mem(out, &KEYBOARD)?;
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn select(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pointer = ctx.arg_u32(0)?;
    if pointer == 0 || ctx.cpu.check_guest_access(pointer, 16, Prot::READ).is_err() {
        return failed(ctx, 87, 0);
    }
    if ctx.cpu.read_mem(pointer, 16)? != KEYBOARD {
        return failed(ctx, 1168, 0);
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn enumerate(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
    if let Some(index) = ctx.kernel.ce.sip_enumerations.iter().rposition(|state| {
        state.thread == ctx.kernel.current_thread
            && state.thunk == ctx.thunk.thunk_va
            && state.frame.sp == sp
    }) {
        let state = ctx.kernel.ce.sip_enumerations.remove(index);
        ctx.kernel.heap.free(state.scratch);
        for (index, value) in state.frame.args.into_iter().enumerate() {
            ctx.cpu.write_arg(index as u8, value)?;
        }
        ctx.cpu.write_reg(ArmReg::Lr, state.frame.lr)?;
        // There is one supported method. A callback returning zero stops
        // enumeration after that method; it does not make enumeration fail.
        return Ok(DispatchOutcome::ReturnedR0(1));
    }
    let callback = ctx.arg_u32(0)?;
    if callback == 0
        || ctx
            .cpu
            .check_guest_access(callback & !1, 2, Prot::EXEC)
            .is_err()
    {
        return failed(ctx, 87, u32::MAX);
    }
    let frame = GuestCallFrame {
        args: [
            ctx.arg_u32(0)?,
            ctx.arg_u32(1)?,
            ctx.arg_u32(2)?,
            ctx.arg_u32(3)?,
        ],
        sp,
        lr: ctx.cpu.read_reg(ArmReg::Lr)?,
    };
    let DispatchOutcome::ReturnedR0(scratch) = crate::coredll::do_alloc(ctx, 536)? else {
        unreachable!()
    };
    if scratch == 0 {
        return failed(ctx, 8, u32::MAX);
    }
    let mut info = [0u8; 536];
    for (index, unit) in "Keyboard".encode_utf16().enumerate() {
        info[index * 2..index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
    }
    info[520..].copy_from_slice(&KEYBOARD);
    if let Err(error) = ctx.cpu.write_mem(scratch, &info) {
        ctx.kernel.heap.free(scratch);
        return Err(error.into());
    }
    ctx.cpu.write_arg(0, scratch)?;
    ctx.cpu.write_reg(ArmReg::Lr, ctx.thunk.thunk_va)?;
    ctx.kernel.ce.sip_enumerations.push(SipEnumeration {
        thread: ctx.kernel.current_thread,
        thunk: ctx.thunk.thunk_va,
        frame,
        scratch,
    });
    Ok(DispatchOutcome::JumpTo(callback))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;

    #[test]
    fn shell_input_method_callback_returns_through_its_original_frame() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        cpu.map_region(0x8000, 4096, Prot::READ | Prot::EXEC)
            .unwrap();
        cpu.write_reg(ArmReg::Sp, 0x3000).unwrap();
        cpu.write_reg(ArmReg::Lr, 0x8010).unwrap();
        let initial = kernel.heap.free_bytes();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "SipEnumIM",
                &[0x8001, 11, 12, 13]
            ),
            DispatchOutcome::JumpTo(0x8001)
        );
        let scratch = cpu.read_reg(ArmReg::R0).unwrap();
        assert_eq!(cpu.read_mem(scratch + 520, 16).unwrap(), KEYBOARD);
        let return_thunk = cpu.read_reg(ArmReg::Lr).unwrap();
        assert_eq!(kernel.ce.sip_enumerations[0].thunk, return_thunk);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "SipEnumIM",
                &[0, 21, 22, 23]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), 0x8010);
        assert_eq!(cpu.read_reg(ArmReg::R1).unwrap(), 11);
        assert!(kernel.ce.sip_enumerations.is_empty());
        assert_eq!(kernel.heap.free_bytes(), initial);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "SipGetCurrentIM",
                &[0x1000]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "SipSetCurrentIM",
                &[0x1000]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        cpu.write_mem(0x1000, &[0; 16]).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "SipSetCurrentIM",
                &[0x1000]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
    }
}
