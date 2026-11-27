//! Reentrant MFC continuations. Frames contain only guest addresses/registers;
//! normal heap/export checkpointing preserves them, including parked callbacks.
use super::*;
#[cfg(test)]
use pocket_cpu::regs::ArmReg;
const MAGIC: u32 = 0x4d464303;
const CONTINUE: &str = "__PocketHLE_MFC_Continue";
const ROOT: &str = "__PocketHLE_MFC_Frame:";
const NAMESPACE: crate::guest_callbacks::Namespace = crate::guest_callbacks::Namespace {
    magic: MAGIC,
    root: ROOT,
    continue_name: CONTINUE,
};
pub(super) struct Frame(crate::guest_callbacks::Frame);
impl std::ops::Deref for Frame {
    type Target = crate::guest_callbacks::Frame;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Frame {
    pub fn enter(ctx: &mut CallCtx<'_>) -> Result<Self, KernelError> {
        let owner = super::application::owner(ctx)?;
        Ok(Self(crate::guest_callbacks::Frame::enter(
            ctx, NAMESPACE, owner,
        )?))
    }
    pub fn call_virtual(
        &self,
        ctx: &mut CallCtx<'_>,
        phase: u32,
        object: u32,
        slot: u32,
        tail: &[u32],
    ) -> Result<DispatchOutcome, KernelError> {
        let target = virtual_function(ctx, object, slot)?;
        let mut args = Vec::with_capacity(tail.len() + 1);
        args.push(object);
        args.extend_from_slice(tail);
        self.call_args(ctx, phase, target, &args)
    }
    pub fn finish(self, ctx: &mut CallCtx<'_>, value: u32) -> Result<DispatchOutcome, KernelError> {
        self.0.finish(ctx, value)
    }
}
/// CString temporary guarded by an ordinary CE3 TRY link in the same guest
/// frame. Offsets are reconstructed after callbacks; no host owner is parked.
pub(super) struct ScopedString {
    pub cell: u32,
    link: u32,
    count: u32,
}
impl ScopedString {
    pub fn at(frame: &Frame, cell: u32, link: u32) -> Self {
        Self::array(frame, cell, link, 1)
    }
    pub fn array(frame: &Frame, cell: u32, link: u32, count: u32) -> Self {
        Self {
            cell: frame.address + cell,
            link: frame.address + link,
            count,
        }
    }
    pub fn begin(&self, ctx: &mut CallCtx<'_>, frame: &Frame) -> Result<(), KernelError> {
        for i in 0..self.count {
            if let Err(error) = new_string(ctx, self.cell + i * 4, &[]) {
                for j in 0..i {
                    release_string(ctx, self.cell + j * 4)?;
                }
                return Err(error);
            }
        }
        if let Err(error) =
            super::exception_links::begin_catch(ctx, frame, self.link - frame.address)
        {
            for i in 0..self.count {
                release_string(ctx, self.cell + i * 4)?;
            }
            return Err(error);
        }
        Ok(())
    }
    pub fn rethrow(
        &self,
        ctx: &mut CallCtx<'_>,
        frame: &Frame,
    ) -> Result<Option<DispatchOutcome>, KernelError> {
        let exception = word(ctx, self.link + 52)?;
        if exception == 0 {
            return Ok(None);
        }
        self.end(ctx)?;
        Ok(Some(frame.call_service(
            ctx,
            0xffff,
            super::exception_throw::THROW,
            &[exception],
        )?))
    }
    pub fn end(&self, ctx: &mut CallCtx<'_>) -> Result<(), KernelError> {
        for i in 0..self.count {
            release_string(ctx, self.cell + i * 4)?;
        }
        super::exception_links::unlink(ctx, self.link)
    }
}

pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", CONTINUE, resume);
}
fn resume(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let owner = super::application::owner(ctx)?;
    crate::guest_callbacks::resume(ctx, NAMESPACE, owner)
}

#[cfg(test)]
pub(super) fn return_from_callback(
    cpu: &mut dyn pocket_cpu::Cpu,
    kernel: &mut pocket_kernel::KernelState,
    dispatcher: &mut WinCeDispatcher,
) -> u32 {
    use pocket_kernel::Dispatcher;
    let thunk_va = kernel.dynamic_exports[&1][CONTINUE];
    assert_eq!(cpu.read_reg(ArmReg::Lr).unwrap(), thunk_va);
    let thunk = pocket_kernel::Thunk {
        thunk_va,
        iat_va: 0,
        dll: "coredll.dll".into(),
        binding: pocket_pe::ImportBinding::Name(CONTINUE.into()),
        friendly_name: None,
    };
    let DispatchOutcome::JumpTo(parent) = dispatcher.dispatch(cpu, &thunk, kernel).unwrap() else {
        panic!("MFC continuation must resume its parent");
    };
    parent
}

pub(super) fn service(ctx: &CallCtx<'_>, name: &str) -> Result<u32, KernelError> {
    let owner = super::application::owner(ctx)?;
    ctx.kernel.dynamic_exports[&owner]
        .get(name)
        .copied()
        .ok_or_else(|| KernelError::Dispatch(format!("WinCE service {name} missing")))
}

pub(super) fn virtual_function(
    ctx: &mut CallCtx<'_>,
    object: u32,
    slot: u32,
) -> Result<u32, KernelError> {
    let table = word(ctx, object)?;
    word(ctx, table + slot * 4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_cpu::Cpu;
    #[test]
    fn stacked_callback_resumes_original_frame_and_releases_its_roots() {
        let (mut cpu, mut kernel, mut dispatcher) = super::super::tests::framework_setup();
        for (r, v) in [
            (ArmReg::R0, 11),
            (ArmReg::R1, 22),
            (ArmReg::R2, 33),
            (ArmReg::R3, 44),
            (ArmReg::Lr, 0x12345678),
        ] {
            cpu.write_reg(r, v).unwrap();
        }
        let baseline = kernel.heap.free_bytes();
        let thunk = pocket_kernel::Thunk {
            thunk_va: 0x70000020,
            iat_va: 0,
            dll: "mfcce300.dll".into(),
            binding: pocket_pe::ImportBinding::Ordinal(775),
            friendly_name: None,
        };
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        let frame = Frame::enter(&mut ctx).unwrap();
        let p = frame.address;
        frame.set(&mut ctx, 0, 0xabcdef).unwrap();
        assert!(matches!(
            frame
                .call_args(&mut ctx, 7, 0x6000, &[1, 2, 3, 4, 5, 6])
                .unwrap(),
            DispatchOutcome::JumpTo(0x6000)
        ));
        assert_eq!(ctx.cpu.read_reg(ArmReg::Sp).unwrap(), 0x3ff8);
        assert_eq!(
            ctx.cpu.read_reg(ArmReg::Lr).unwrap(),
            ctx.kernel.dynamic_exports[&1][CONTINUE]
        );
        assert_eq!(ctx.cpu.read_u32_le(0x3ff8).unwrap(), 5);
        assert_eq!(ctx.cpu.read_u32_le(0x3ffc).unwrap(), 6);
        assert_eq!(
            return_from_callback(ctx.cpu, ctx.kernel, &mut dispatcher),
            thunk.thunk_va
        );
        let frame = Frame::enter(&mut ctx).unwrap();
        assert!(!frame.fresh);
        assert_eq!(frame.address, p);
        assert_eq!(frame.phase(&mut ctx).unwrap(), 7);
        assert_eq!(frame.get(&mut ctx, 0).unwrap(), 0xabcdef);
        assert_eq!(ctx.cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        assert_eq!(frame.arg(&mut ctx, 1).unwrap(), 22);
        assert!(matches!(
            frame.finish(&mut ctx, 99).unwrap(),
            DispatchOutcome::ReturnedR0(99)
        ));
        assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), 0x12345678);
        assert_eq!(ctx.cpu.read_reg(ArmReg::R1).unwrap(), 22);
        assert_eq!(ctx.kernel.heap.free_bytes(), baseline);
        assert!(!ctx
            .kernel
            .dynamic_exports
            .values()
            .any(|m| m.keys().any(|n| n.starts_with(ROOT))));
    }
    #[test]
    fn synchronous_same_method_recursion_keeps_each_return_and_owned_frame_separate() {
        let (mut cpu, mut kernel, mut dispatcher) = super::super::tests::framework_setup();
        cpu.write_reg(ArmReg::Lr, 0x12345678).unwrap();
        cpu.write_reg(ArmReg::R0, 11).unwrap();
        let baseline = kernel.heap.free_bytes();
        let thunk = pocket_kernel::Thunk {
            thunk_va: 0x70000020,
            iat_va: 0,
            dll: "mfcce300.dll".into(),
            binding: pocket_pe::ImportBinding::Ordinal(775),
            friendly_name: None,
        };
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        let outer = Frame::enter(&mut ctx).unwrap();
        outer.call_args(&mut ctx, 3, 0x6000, &[22]).unwrap();
        assert_eq!(ctx.cpu.read_reg(ArmReg::Sp).unwrap(), 0x3ff8);
        // WinCE can call this same method before the first callback returns,
        // without guest code making an additional stack frame in between.
        let inner = Frame::enter(&mut ctx).unwrap();
        assert!(inner.fresh);
        assert_ne!(inner.address, outer.address);
        assert_eq!(inner.arg(&mut ctx, 0).unwrap(), 22);
        inner.call_args(&mut ctx, 9, 0x6000, &[33]).unwrap();
        assert_eq!(ctx.cpu.read_reg(ArmReg::Sp).unwrap(), 0x3ff0);
        ctx.cpu.write_reg(ArmReg::R0, 77).unwrap();
        assert_eq!(
            return_from_callback(ctx.cpu, ctx.kernel, &mut dispatcher),
            thunk.thunk_va
        );
        assert_eq!(ctx.cpu.read_reg(ArmReg::R0).unwrap(), 77);
        let resumed = Frame::enter(&mut ctx).unwrap();
        assert!(!resumed.fresh);
        assert_eq!(resumed.address, inner.address);
        assert_eq!(resumed.phase(&mut ctx).unwrap(), 9);
        resumed.finish(&mut ctx, 88).unwrap();
        assert_eq!(ctx.cpu.read_reg(ArmReg::Sp).unwrap(), 0x3ff8);
        assert_eq!(
            return_from_callback(ctx.cpu, ctx.kernel, &mut dispatcher),
            thunk.thunk_va
        );
        let resumed = Frame::enter(&mut ctx).unwrap();
        assert_eq!(resumed.address, outer.address);
        assert_eq!(resumed.arg(&mut ctx, 0).unwrap(), 11);
        assert_eq!(resumed.phase(&mut ctx).unwrap(), 3);
        resumed.finish(&mut ctx, 99).unwrap();
        assert_eq!(ctx.cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
        assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), 0x12345678);
        assert_eq!(ctx.kernel.heap.free_bytes(), baseline);
        assert!(!ctx
            .kernel
            .dynamic_exports
            .values()
            .any(|m| m.keys().any(|n| n.starts_with(ROOT))));
    }

    #[test]
    fn unrelated_thread_cannot_consume_a_callback_return() {
        let (mut cpu, mut kernel, _) = super::super::tests::framework_setup();
        let thunk = pocket_kernel::Thunk {
            thunk_va: 0x70000020,
            iat_va: 0,
            dll: "mfcce300.dll".into(),
            binding: pocket_pe::ImportBinding::Ordinal(775),
            friendly_name: None,
        };
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        let frame = Frame::enter(&mut ctx).unwrap();
        frame.call_args(&mut ctx, 1, 0x6000, &[]).unwrap();
        let continuation = pocket_kernel::Thunk {
            thunk_va: ctx.kernel.dynamic_exports[&1][CONTINUE],
            iat_va: 0,
            dll: "coredll.dll".into(),
            binding: pocket_pe::ImportBinding::Name(CONTINUE.into()),
            friendly_name: None,
        };
        let thread = ctx.kernel.current_thread;
        ctx.kernel.current_thread = thread + 1;
        let mut returned = CallCtx {
            cpu: ctx.cpu,
            kernel: ctx.kernel,
            thunk: &continuation,
        };
        assert!(resume(&mut returned).is_err());
        returned.kernel.current_thread = thread;
        assert!(matches!(
            resume(&mut returned).unwrap(),
            DispatchOutcome::JumpTo(0x70000020)
        ));
        let frame = Frame::enter(&mut ctx).unwrap();
        frame.finish(&mut ctx, 0).unwrap();
    }
}

/// AfxEndThread never returns through abandoned guest callbacks. Validate and
/// release their guest-only frames before the common WinCE thread-exit service.
pub(super) fn discard_thread_frames(ctx: &mut CallCtx<'_>, keep: u32) -> Result<(), KernelError> {
    let owner = super::application::owner(ctx)?;
    crate::guest_callbacks::discard(ctx, NAMESPACE, owner, keep)
}
