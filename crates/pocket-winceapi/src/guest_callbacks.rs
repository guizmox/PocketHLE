//! Shared guest-owned continuations for synchronous WinCE and guest callbacks.
//! Namespace identities keep existing MFC frames byte-compatible; checkpointing
//! uses the normal guest heap/export roots rather than a parallel host registry.
use crate::{CallCtx, WinCeDispatcher};
use pocket_cpu::regs::ArmReg;
use pocket_kernel::{DispatchOutcome, KernelError};
#[derive(Clone, Copy)]
pub(crate) struct Namespace {
    pub magic: u32,
    pub root: &'static str,
    pub continue_name: &'static str,
}
const BYTES: u32 = 256;
pub(crate) const WINCE: Namespace = Namespace {
    magic: 0x57434501,
    root: "__PocketHLE_WinCE_Frame:",
    continue_name: "__PocketHLE_WinCE_Continue",
};
pub(crate) fn owner(ctx: &CallCtx<'_>, marker: &str) -> Result<u32, KernelError> {
    ctx.kernel
        .dynamic_exports
        .iter()
        .find_map(|(&owner, exports)| exports.contains_key(marker).then_some(owner))
        .ok_or_else(|| KernelError::Loader("WinCE callback owner is missing".into()))
}
pub(crate) fn service(ctx: &CallCtx<'_>, name: &str) -> Result<u32, KernelError> {
    let owner = owner(ctx, WINCE.continue_name)?;
    ctx.kernel.dynamic_exports[&owner]
        .get(name)
        .copied()
        .ok_or_else(|| KernelError::Dispatch(format!("WinCE callback service {name} missing")))
}
fn word(ctx: &mut CallCtx<'_>, p: u32) -> Result<u32, KernelError> {
    Ok(u32::from_le_bytes(
        ctx.cpu.read_mem(p, 4)?.try_into().unwrap(),
    ))
}
fn put(ctx: &mut CallCtx<'_>, p: u32, value: u32) -> Result<(), KernelError> {
    Ok(ctx.cpu.write_mem(p, &value.to_le_bytes())?)
}
fn alloc(ctx: &mut CallCtx<'_>, bytes: u32) -> Result<u32, KernelError> {
    match crate::coredll::do_alloc(ctx, bytes)? {
        DispatchOutcome::ReturnedR0(p) if p != 0 => Ok(p),
        _ => Err(KernelError::Dispatch(
            "WinCE callback allocation failed".into(),
        )),
    }
}
pub(crate) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", WINCE.continue_name, resume_wince);
}
fn resume_wince(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let owner = owner(ctx, WINCE.continue_name)?;
    resume(ctx, WINCE, owner)
}
pub(crate) fn discard_wince_thread(ctx: &mut CallCtx<'_>) -> Result<(), KernelError> {
    let Ok(owner) = owner(ctx, WINCE.continue_name) else {
        return Ok(());
    };
    discard(ctx, WINCE, owner, 0)
}
pub(crate) struct Frame {
    namespace: Namespace,
    owner: u32,
    key: String,
    pub address: u32,
    pub fresh: bool,
}
impl Frame {
    pub fn enter(
        ctx: &mut CallCtx<'_>,
        namespace: Namespace,
        owner: u32,
    ) -> Result<Self, KernelError> {
        let root = namespace.root;
        let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
        let key = format!(
            "{root}{}:{sp:08x}:{:08x}",
            ctx.kernel.current_thread, ctx.thunk.thunk_va
        );
        if let Some(&p) = ctx.kernel.dynamic_exports[&owner].get(&key) {
            ctx.cpu.check_guest_access(
                p,
                BYTES,
                pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
            )?;
            if word(ctx, p)? != namespace.magic
                || word(ctx, p + 24)? != sp
                || word(ctx, p + 32)? != 0
                || word(ctx, p + 36)? != ctx.thunk.thunk_va
            {
                return Err(KernelError::Dispatch("guest callback frame invalid".into()));
            }
            return Ok(Self {
                namespace,
                owner,
                key,
                address: p,
                fresh: false,
            });
        }
        let p = alloc(ctx, BYTES)?;
        let result = (|| {
            // do_alloc already zeroes the complete frame. Populate its header
            // with one guest write rather than invalidating executable heap
            // pages separately for every saved argument.
            let mut header = [0u8; 40];
            header[..4].copy_from_slice(&namespace.magic.to_le_bytes());
            header[36..40].copy_from_slice(&ctx.thunk.thunk_va.to_le_bytes());
            for (i, r) in [
                ArmReg::R0,
                ArmReg::R1,
                ArmReg::R2,
                ArmReg::R3,
                ArmReg::Lr,
                ArmReg::Sp,
            ]
            .into_iter()
            .enumerate()
            {
                let value = ctx.cpu.read_reg(r)?;
                header[4 + i * 4..8 + i * 4].copy_from_slice(&value.to_le_bytes());
            }
            ctx.cpu.write_mem(p, &header)?;
            Ok::<_, KernelError>(())
        })();
        if let Err(e) = result {
            ctx.kernel.heap.free(p);
            return Err(e);
        }
        ctx.kernel
            .dynamic_exports
            .get_mut(&owner)
            .unwrap()
            .insert(key.clone(), p);
        Ok(Self {
            namespace,
            owner,
            key,
            address: p,
            fresh: true,
        })
    }
    pub fn phase(&self, ctx: &mut CallCtx<'_>) -> Result<u32, KernelError> {
        word(ctx, self.address + 28)
    }
    pub fn set_phase(&self, ctx: &mut CallCtx<'_>, p: u32) -> Result<(), KernelError> {
        put(ctx, self.address + 28, p)
    }
    pub fn arg(&self, ctx: &mut CallCtx<'_>, i: u32) -> Result<u32, KernelError> {
        word(ctx, self.address + 4 + i * 4)
    }
    pub fn get(&self, ctx: &mut CallCtx<'_>, i: u32) -> Result<u32, KernelError> {
        word(ctx, self.address + 48 + i * 4)
    }
    pub fn set(&self, ctx: &mut CallCtx<'_>, i: u32, v: u32) -> Result<(), KernelError> {
        put(ctx, self.address + 48 + i * 4, v)
    }
    pub fn call(
        &self,
        ctx: &mut CallCtx<'_>,
        phase: u32,
        callee: u32,
        args: [u32; 4],
    ) -> Result<DispatchOutcome, KernelError> {
        self.call_args(ctx, phase, callee, &args)
    }
    /// Resolve a service in this frame's export owner, preserving the same
    /// call/return ABI for MFC and ordinary synchronous WinCE continuations.
    pub fn call_service(
        &self,
        ctx: &mut CallCtx<'_>,
        phase: u32,
        name: &str,
        args: &[u32],
    ) -> Result<DispatchOutcome, KernelError> {
        let target = ctx.kernel.dynamic_exports[&self.owner]
            .get(name)
            .copied()
            .ok_or_else(|| KernelError::Dispatch(format!("WinCE service {name} missing")))?;
        self.call_args(ctx, phase, target, args)
    }
    pub fn call_args(
        &self,
        ctx: &mut CallCtx<'_>,
        phase: u32,
        callee: u32,
        args: &[u32],
    ) -> Result<DispatchOutcome, KernelError> {
        let root = self.namespace.root;
        if callee == 0 || args.len() > 12 {
            return Err(KernelError::Dispatch("Invalid guest callback".into()));
        }
        let continuation = *ctx.kernel.dynamic_exports[&self.owner]
            .get(self.namespace.continue_name)
            .ok_or_else(|| KernelError::Dispatch("guest return service missing".into()))?;
        // A host API may synchronously enter the same WinCE method without an
        // ARM prologue. Give nested calls their own SP and reserve a distinct
        // return-thunk identity, even when all arguments fit in registers.
        let home = ctx.cpu.stack_arg_offset();
        let bytes = ((home + (args.len().saturating_sub(4) as u32) * 4 + 7) & !7).max(8);
        let sp = word(ctx, self.address + 24)?
            .checked_sub(bytes)
            .ok_or_else(|| KernelError::Dispatch("guest callback stack underflow".into()))?;
        ctx.cpu
            .check_guest_access(sp, bytes, pocket_cpu::Prot::WRITE)?;
        let key = format!(
            "{root}{}:{sp:08x}:{continuation:08x}",
            ctx.kernel.current_thread
        );
        if word(ctx, self.address + 32)? != 0
            || ctx.kernel.dynamic_exports[&self.owner].contains_key(&key)
        {
            return Err(KernelError::Dispatch(
                "guest callback stack collision".into(),
            ));
        }
        for (i, v) in args.iter().skip(4).enumerate() {
            ctx.cpu
                .write_mem(sp + home + i as u32 * 4, &v.to_le_bytes())?;
        }
        self.set_phase(ctx, phase)?;
        put(ctx, self.address + 32, sp)?;
        ctx.kernel
            .dynamic_exports
            .get_mut(&self.owner)
            .unwrap()
            .insert(key, self.address);
        ctx.cpu.write_reg(ArmReg::Sp, sp)?;
        for i in 0..4 {
            ctx.cpu
                .write_arg(i, args.get(i as usize).copied().unwrap_or(0))?;
        }
        ctx.cpu.write_reg(ArmReg::Lr, continuation)?;
        Ok(DispatchOutcome::JumpTo(callee))
    }
    /// One temporary allocation owned across callbacks, released on normal or
    /// nonlocal return. The slot was reserved/zero in existing frame images.
    pub fn own(&self, ctx: &mut CallCtx<'_>, pointer: u32) -> Result<(), KernelError> {
        if word(ctx, self.address + 40)? != 0 {
            return Err(KernelError::Dispatch(
                "Callback temporary already owned".into(),
            ));
        }
        put(ctx, self.address + 40, pointer)
    }
    /// Save a bounded byte region in this guest frame. A tagged descriptor
    /// extends the original single-word guard without changing frame size.
    pub fn save_region(
        &self,
        ctx: &mut CallCtx<'_>,
        offset: u32,
        target: u32,
        bytes: u32,
    ) -> Result<(), KernelError> {
        if offset < 48
            || offset % 4 != 0
            || bytes == 0
            || bytes > BYTES
            || offset.checked_add(8 + bytes).is_none_or(|end| end > BYTES)
            || word(ctx, self.address + 44)? != 0
        {
            return Err(KernelError::Dispatch(
                "Invalid callback restore region".into(),
            ));
        }
        ctx.cpu.check_guest_access(
            target,
            bytes,
            pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
        )?;
        let saved = ctx.cpu.read_mem(target, bytes)?;
        let descriptor = self.address + offset;
        put(ctx, descriptor, target)?;
        put(ctx, descriptor + 4, bytes)?;
        ctx.cpu.write_mem(descriptor + 8, &saved)?;
        put(ctx, self.address + 44, descriptor | 1)
    }
    pub fn disown(&self, ctx: &mut CallCtx<'_>) -> Result<(), KernelError> {
        put(ctx, self.address + 40, 0)
    }
    pub fn return_address(&self, ctx: &CallCtx<'_>) -> Result<u32, KernelError> {
        ctx.kernel.dynamic_exports[&self.owner]
            .get(self.namespace.continue_name)
            .copied()
            .ok_or_else(|| KernelError::Dispatch("Callback return service missing".into()))
    }
    pub fn finish(
        self,
        ctx: &mut CallCtx<'_>,
        result: u32,
    ) -> Result<DispatchOutcome, KernelError> {
        for (i, r) in [ArmReg::R1, ArmReg::R2, ArmReg::R3, ArmReg::Lr, ArmReg::Sp]
            .into_iter()
            .enumerate()
        {
            let value = word(ctx, self.address + 8 + i as u32 * 4)?;
            ctx.cpu.write_reg(r, value)?;
        }
        ctx.kernel
            .dynamic_exports
            .get_mut(&self.owner)
            .unwrap()
            .remove(&self.key);
        restore_region(ctx, self.address)?;
        release_temporary(ctx, self.address)?;
        ctx.kernel.heap.free(self.address);
        Ok(DispatchOutcome::ReturnedR0(result))
    }
}

pub(crate) fn resume(
    ctx: &mut CallCtx<'_>,
    namespace: Namespace,
    owner: u32,
) -> Result<DispatchOutcome, KernelError> {
    let root = namespace.root;
    let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
    let key = format!(
        "{root}{}:{sp:08x}:{:08x}",
        ctx.kernel.current_thread, ctx.thunk.thunk_va
    );
    let p = *ctx.kernel.dynamic_exports[&owner]
        .get(&key)
        .ok_or_else(|| KernelError::Dispatch("guest return without a pending callback".into()))?;
    ctx.cpu
        .check_guest_access(p, BYTES, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let parent = word(ctx, p + 36)?;
    let original_sp = word(ctx, p + 24)?;
    let canonical = format!(
        "{root}{}:{original_sp:08x}:{parent:08x}",
        ctx.kernel.current_thread
    );
    if word(ctx, p)? != namespace.magic
        || word(ctx, p + 32)? != sp
        || parent == 0
        || ctx.kernel.dynamic_exports[&owner].get(&canonical) != Some(&p)
    {
        return Err(KernelError::Dispatch("guest return frame invalid".into()));
    }
    ctx.cpu.write_reg(ArmReg::Sp, original_sp)?;
    put(ctx, p + 32, 0)?;
    ctx.kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .remove(&key);
    Ok(DispatchOutcome::JumpTo(parent))
}

pub(crate) fn discard(
    ctx: &mut CallCtx<'_>,
    namespace: Namespace,
    owner: u32,
    keep: u32,
) -> Result<(), KernelError> {
    let root = namespace.root;
    let prefix = format!("{root}{}:", ctx.kernel.current_thread);
    let entries: Vec<_> = ctx.kernel.dynamic_exports[&owner]
        .iter()
        .filter(|(k, p)| k.starts_with(&prefix) && **p != keep)
        .map(|(k, &p)| (k.clone(), p))
        .collect();
    let addresses: std::collections::HashSet<_> = entries.iter().map(|(_, p)| *p).collect();
    for &p in &addresses {
        if ctx.kernel.heap.msize(p).is_none_or(|n| n < BYTES) || word(ctx, p)? != namespace.magic {
            return Err(KernelError::Dispatch(
                "Invalid abandoned guest callback frame".into(),
            ));
        }
    }
    let mut ordered = Vec::with_capacity(addresses.len());
    for &p in &addresses {
        saved_region(ctx, p)?;
        ordered.push((word(ctx, p + 24)?, p));
    }
    ordered.sort_unstable();
    for (key, _) in entries {
        ctx.kernel
            .dynamic_exports
            .get_mut(&owner)
            .unwrap()
            .remove(&key);
    }
    for (_, p) in ordered {
        restore_region(ctx, p)?;
        release_temporary(ctx, p)?;
        ctx.kernel.heap.free(p);
    }
    Ok(())
}

/// Validate before applying any nonlocal cleanup. Old untagged descriptors
/// remain the original (address, previous word) pair.
fn saved_region(ctx: &mut CallCtx<'_>, frame: u32) -> Result<Option<(u32, Vec<u8>)>, KernelError> {
    let tagged = word(ctx, frame + 44)?;
    if tagged == 0 {
        return Ok(None);
    }
    let descriptor = tagged & !1;
    ctx.cpu
        .check_guest_access(descriptor, 8, pocket_cpu::Prot::READ)?;
    let target = word(ctx, descriptor)?;
    let data = if tagged & 1 != 0 {
        let bytes = word(ctx, descriptor + 4)?;
        if descriptor < frame + 48
            || bytes == 0
            || bytes > BYTES
            || descriptor
                .checked_add(8 + bytes)
                .is_none_or(|end| end > frame + BYTES)
        {
            return Err(KernelError::Dispatch(
                "Invalid callback restore descriptor".into(),
            ));
        }
        ctx.cpu.read_mem(descriptor + 8, bytes)?
    } else {
        word(ctx, descriptor + 4)?.to_le_bytes().to_vec()
    };
    ctx.cpu
        .check_guest_access(target, data.len() as u32, pocket_cpu::Prot::WRITE)?;
    Ok(Some((target, data)))
}
fn restore_region(ctx: &mut CallCtx<'_>, frame: u32) -> Result<(), KernelError> {
    if let Some((target, bytes)) = saved_region(ctx, frame)? {
        ctx.cpu.write_mem(target, &bytes)?;
    }
    Ok(())
}

fn release_temporary(ctx: &mut CallCtx<'_>, frame: u32) -> Result<(), KernelError> {
    let owned = word(ctx, frame + 40)?;
    if owned != 0 {
        ctx.kernel.heap.free(owned);
        put(ctx, frame + 40, 0)?;
    }
    Ok(())
}

/// A nonlocal ARM return abandons only younger frames of the current thread.
/// Outer callback aliases remain intact, including an HLE TRY's return alias.
/// Validate the complete selection before removing roots or freeing any frame.
pub(crate) struct UnwindPlan {
    roots: Vec<(u32, String, u32)>,
    frames: Vec<(u32, u32)>,
}
pub(crate) fn prepare_unwind(
    ctx: &mut CallCtx<'_>,
    target_sp: u32,
) -> Result<UnwindPlan, KernelError> {
    let mut abandoned = Vec::new();
    for (&owner, exports) in &ctx.kernel.dynamic_exports {
        for (key, &p) in exports {
            if ["__PocketHLE_MFC_Frame:", WINCE.root]
                .iter()
                .any(|root| key.starts_with(&format!("{root}{}:", ctx.kernel.current_thread)))
            {
                abandoned.push((owner, key.clone(), p));
            }
        }
    }
    let mut selected = std::collections::HashSet::new();
    for &(_, _, p) in &abandoned {
        ctx.cpu
            .check_guest_access(p, BYTES, pocket_cpu::Prot::READ)?;
        if ctx.kernel.heap.msize(p).is_none_or(|n| n < BYTES)
            || !matches!(word(ctx, p)?, 0x4d464303 | 0x57434501)
        {
            return Err(KernelError::Dispatch(
                "Invalid nonlocal callback frame".into(),
            ));
        }
        if word(ctx, p + 24)? <= target_sp {
            let owned = word(ctx, p + 40)?;
            if owned != 0 && ctx.kernel.heap.msize(owned).is_none() {
                return Err(KernelError::Dispatch(
                    "Invalid nonlocal callback temporary".into(),
                ));
            }
            saved_region(ctx, p)?;
            selected.insert(p);
        }
    }
    abandoned.retain(|(_, _, p)| selected.contains(p));
    let mut ordered = Vec::with_capacity(selected.len());
    for p in selected {
        ordered.push((word(ctx, p + 24)?, p));
    }
    ordered.sort_unstable();
    Ok(UnwindPlan {
        roots: abandoned,
        frames: ordered,
    })
}
impl UnwindPlan {
    pub(crate) fn apply(self, ctx: &mut CallCtx<'_>) -> Result<(), KernelError> {
        for (owner, key, _) in self.roots {
            ctx.kernel
                .dynamic_exports
                .get_mut(&owner)
                .unwrap()
                .remove(&key);
        }
        for (_, p) in self.frames {
            restore_region(ctx, p)?;
            release_temporary(ctx, p)?;
            ctx.kernel.heap.free(p);
        }
        Ok(())
    }
}
