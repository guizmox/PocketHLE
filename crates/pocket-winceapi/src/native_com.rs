//! Registry-selected guest COM servers. No host COM activation is performed.
//! CE6 PegHelp and IESample share the WebBrowser class in shdocvw.dll; its
//! native factory owns the browser implementation and requested interfaces.
use crate::{coredll, CallCtx};
use pocket_cpu::{regs::ArmReg, Prot};
use pocket_kernel::{
    ce::{ComActivation, ComStage},
    registry::RegistryValue,
    DispatchOutcome, GuestCallFrame, KernelError,
};

const INVALID: u32 = 0x8007_0057;
const NO_MEMORY: u32 = 0x8007_000e;
const UNEXPECTED: u32 = 0x8000_ffff;
const CLASS_UNAVAILABLE: u32 = 0x8004_0111;
const NO_AGGREGATION: u32 = 0x8004_0110;
const CLASS_FACTORY: [u8; 16] = [1, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46];

fn returned(value: u32) -> DispatchOutcome {
    DispatchOutcome::ReturnedR0(value)
}

pub(crate) fn resume(ctx: &mut CallCtx<'_>) -> Result<Option<DispatchOutcome>, KernelError> {
    let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
    let index = ctx.kernel.ce.com.activations.iter().rposition(|state| {
        state.thread == ctx.kernel.current_thread
            && state.thunk == ctx.thunk.thunk_va
            && state.frame.sp == sp
    });
    let Some(index) = index else {
        return Ok(None);
    };
    let state = ctx.kernel.ce.com.activations.remove(index);
    advance(ctx, state).map(Some)
}

pub(crate) fn activate(
    ctx: &mut CallCtx<'_>,
    create: bool,
) -> Result<Option<DispatchOutcome>, KernelError> {
    let clsid = ctx.arg_u32(0)?;
    let iid = ctx.arg_u32(3)?;
    let output = ctx.arg_u32(4)?;
    if clsid == 0
        || iid == 0
        || output == 0
        || ctx.cpu.check_guest_access(clsid, 16, Prot::READ).is_err()
        || ctx.cpu.check_guest_access(iid, 16, Prot::READ).is_err()
        || ctx.cpu.check_guest_access(output, 4, Prot::WRITE).is_err()
    {
        return Ok(Some(returned(INVALID)));
    }
    let clsid_bytes = ctx.cpu.read_mem(clsid, 16)?;
    let iid_bytes = ctx.cpu.read_mem(iid, 16)?;
    let key = format!(
        r"HKCR\CLSID\{}\InprocServer32",
        super::ole32::guid_string(&clsid_bytes)
    );
    let server = match ctx
        .kernel
        .registry
        .value(&key, "")
        .or_else(|| ctx.kernel.registry.value(&key, "default"))
    {
        Some(RegistryValue::Sz(server)) if !server.trim().is_empty() => {
            server.trim().trim_matches('"').to_owned()
        }
        _ => return Ok(None),
    };
    ctx.cpu.write_mem(output, &0u32.to_le_bytes())?;
    let context = ctx.arg_u32(if create { 2 } else { 1 })?;
    if context & 1 == 0 {
        return Ok(Some(returned(CLASS_UNAVAILABLE)));
    }
    let outer = if create { ctx.arg_u32(1)? } else { 0 };
    // Aggregation needs a controlling IUnknown and different ownership rules.
    if outer != 0 {
        return Ok(Some(returned(NO_AGGREGATION)));
    }
    if !create && ctx.arg_u32(2)? != 0 {
        return Ok(Some(returned(INVALID)));
    }
    let Some(scratch) = ctx.kernel.heap.alloc(64) else {
        return Ok(Some(returned(NO_MEMORY)));
    };
    let mut bytes = [0u8; 64];
    bytes[..16].copy_from_slice(&clsid_bytes);
    bytes[16..32].copy_from_slice(if create {
        CLASS_FACTORY.as_slice()
    } else {
        iid_bytes.as_slice()
    });
    bytes[36..52].copy_from_slice(&iid_bytes);
    if let Err(error) = ctx.cpu.write_mem(scratch, &bytes) {
        ctx.kernel.heap.free(scratch);
        return Err(error.into());
    }
    let state = ComActivation {
        thread: ctx.kernel.current_thread,
        thunk: ctx.thunk.thunk_va,
        frame: GuestCallFrame {
            args: [
                ctx.arg_u32(0)?,
                ctx.arg_u32(1)?,
                ctx.arg_u32(2)?,
                ctx.arg_u32(3)?,
            ],
            sp: ctx.cpu.read_reg(ArmReg::Sp)?,
            lr: ctx.cpu.read_reg(ArmReg::Lr)?,
        },
        output,
        outer,
        scratch,
        server,
        create,
        stage: ComStage::Loading,
        factory: 0,
        result: 0,
        value: 0,
    };
    log::info!(
        "COM: activate {} from {:?}",
        super::ole32::guid_string(&clsid_bytes),
        state.server
    );
    advance(ctx, state).map(Some)
}

fn jump(
    ctx: &mut CallCtx<'_>,
    state: ComActivation,
    address: u32,
    args: [u32; 4],
) -> Result<DispatchOutcome, KernelError> {
    for (index, value) in args.into_iter().enumerate() {
        ctx.cpu.write_arg(index as u8, value)?;
    }
    ctx.cpu.write_reg(ArmReg::Lr, state.thunk)?;
    ctx.kernel.ce.com.activations.push(state);
    Ok(DispatchOutcome::JumpTo(address))
}

fn complete(
    ctx: &mut CallCtx<'_>,
    state: ComActivation,
    result: u32,
) -> Result<DispatchOutcome, KernelError> {
    ctx.kernel.heap.free(state.scratch);
    ctx.cpu.write_mem(
        state.output,
        &(if result & 0x8000_0000 == 0 {
            state.value
        } else {
            0
        })
        .to_le_bytes(),
    )?;
    for (index, value) in state.frame.args.into_iter().enumerate() {
        ctx.cpu.write_arg(index as u8, value)?;
    }
    ctx.cpu.write_reg(ArmReg::Sp, state.frame.sp)?;
    ctx.cpu.write_reg(ArmReg::Lr, state.frame.lr)?;
    Ok(returned(result))
}

pub(crate) fn method(
    ctx: &mut CallCtx<'_>,
    object: u32,
    slot: u32,
) -> Result<Option<u32>, KernelError> {
    if object == 0 || ctx.cpu.check_guest_access(object, 4, Prot::READ).is_err() {
        return Ok(None);
    }
    let table = ctx.cpu.read_u32_le(object)?;
    let Some(entry) = table.checked_add(slot * 4) else {
        return Ok(None);
    };
    if table == 0 || ctx.cpu.check_guest_access(entry, 4, Prot::READ).is_err() {
        return Ok(None);
    }
    let address = ctx.cpu.read_u32_le(entry)?;
    if address == 0
        || ctx
            .cpu
            .check_guest_access(address & !1, 2, Prot::EXEC)
            .is_err()
    {
        return Ok(None);
    }
    Ok(Some(address))
}

pub(crate) fn resume_references(
    ctx: &mut CallCtx<'_>,
) -> Result<Option<DispatchOutcome>, KernelError> {
    let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
    let index = ctx.kernel.ce.com.callbacks.iter().rposition(|state| {
        state.thread == ctx.kernel.current_thread
            && state.thunk == ctx.thunk.thunk_va
            && state.frame.sp == sp
    });
    let Some(index) = index else {
        return Ok(None);
    };
    let state = ctx.kernel.ce.com.callbacks.remove(index);
    advance_references(ctx, state).map(Some)
}

pub(crate) fn references(
    ctx: &mut CallCtx<'_>,
    mut calls: Vec<(u32, u32)>,
    result: u32,
    free: Vec<u32>,
) -> Result<DispatchOutcome, KernelError> {
    calls.reverse();
    let state = pocket_kernel::ce::ComCallback {
        thread: ctx.kernel.current_thread,
        thunk: ctx.thunk.thunk_va,
        frame: GuestCallFrame {
            args: [
                ctx.arg_u32(0)?,
                ctx.arg_u32(1)?,
                ctx.arg_u32(2)?,
                ctx.arg_u32(3)?,
            ],
            sp: ctx.cpu.read_reg(ArmReg::Sp)?,
            lr: ctx.cpu.read_reg(ArmReg::Lr)?,
        },
        calls,
        result,
        free,
    };
    advance_references(ctx, state)
}

fn advance_references(
    ctx: &mut CallCtx<'_>,
    mut state: pocket_kernel::ce::ComCallback,
) -> Result<DispatchOutcome, KernelError> {
    if let Some((object, slot)) = state.calls.pop() {
        let Some(address) = method(ctx, object, slot)? else {
            state.calls.clear();
            state.result = UNEXPECTED;
            return advance_references(ctx, state);
        };
        ctx.cpu.write_reg(ArmReg::R0, object)?;
        ctx.cpu.write_reg(ArmReg::Lr, state.thunk)?;
        ctx.kernel.ce.com.callbacks.push(state);
        return Ok(DispatchOutcome::JumpTo(address));
    }
    for block in state.free {
        ctx.kernel.heap.free(block);
    }
    for (index, value) in state.frame.args.into_iter().enumerate() {
        ctx.cpu.write_arg(index as u8, value)?;
    }
    ctx.cpu.write_reg(ArmReg::Sp, state.frame.sp)?;
    ctx.cpu.write_reg(ArmReg::Lr, state.frame.lr)?;
    Ok(returned(state.result))
}

fn advance(
    ctx: &mut CallCtx<'_>,
    mut state: ComActivation,
) -> Result<DispatchOutcome, KernelError> {
    match state.stage {
        ComStage::Loading => {
            let name = state.server.to_ascii_lowercase();
            let base = if let Some(&base) = ctx.kernel.ce.com.servers.get(&name) {
                base
            } else {
                match coredll::load_library_named(ctx, &state.server)? {
                    DispatchOutcome::ReturnedR0(base) => base,
                    outcome => {
                        ctx.kernel.ce.com.activations.push(state);
                        return Ok(outcome);
                    }
                }
            };
            if base == 0 {
                let error = ctx
                    .kernel
                    .thread_last_errors
                    .get(&state.thread)
                    .copied()
                    .unwrap_or(126);
                return complete(
                    ctx,
                    state,
                    if error == 8 {
                        NO_MEMORY
                    } else {
                        0x8007_0000 | error
                    },
                );
            }
            ctx.kernel.ce.com.servers.insert(name, base);
            let address = ctx
                .kernel
                .dynamic_exports
                .get(&base)
                .and_then(|exports| exports.get("DllGetClassObject"))
                .copied();
            let Some(address) = address else {
                return complete(ctx, state, CLASS_UNAVAILABLE);
            };
            state.stage = ComStage::Factory;
            let args = [state.scratch, state.scratch + 16, state.scratch + 32, 0];
            jump(ctx, state, address, args)
        }
        ComStage::Factory => {
            let result = ctx.cpu.read_reg(ArmReg::R0)?;
            state.value = ctx.cpu.read_u32_le(state.scratch + 32)?;
            if result & 0x8000_0000 != 0 {
                return complete(ctx, state, result);
            }
            if state.value == 0 {
                return complete(ctx, state, UNEXPECTED);
            }
            if !state.create {
                return complete(ctx, state, result);
            }
            state.factory = state.value;
            let Some(address) = method(ctx, state.factory, 3)? else {
                return complete(ctx, state, UNEXPECTED);
            };
            ctx.cpu.write_mem(state.scratch + 32, &0u32.to_le_bytes())?;
            state.stage = ComStage::Creating;
            let args = [
                state.factory,
                state.outer,
                state.scratch + 36,
                state.scratch + 32,
            ];
            jump(ctx, state, address, args)
        }
        ComStage::Creating => {
            state.result = ctx.cpu.read_reg(ArmReg::R0)?;
            state.value = ctx.cpu.read_u32_le(state.scratch + 32)?;
            if state.result & 0x8000_0000 == 0 && state.value == 0 {
                state.result = UNEXPECTED;
            }
            let Some(address) = method(ctx, state.factory, 2)? else {
                return complete(ctx, state, UNEXPECTED);
            };
            state.stage = ComStage::Releasing;
            let args = [state.factory, 0, 0, 0];
            jump(ctx, state, address, args)
        }
        ComStage::Releasing => {
            let result = state.result;
            complete(ctx, state, result)
        }
    }
}
