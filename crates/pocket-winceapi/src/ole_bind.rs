//! OLE binding contexts used by the native CE WebBrowser and URL monikers.
//! Parameters retain real IUnknown references across asynchronous callbacks.
use crate::{native_com, CallCtx, WinCeDispatcher};
use pocket_cpu::Prot;
use pocket_kernel::{ce::BindContext, DispatchOutcome, KernelError};

const INVALID: u32 = 0x8007_0057;
const UNEXPECTED: u32 = 0x8000_ffff;
const NO_MEMORY: u32 = 0x8007_000e;
const NOT_BOUND: u32 = 0x8004_01e9;
const METHODS: [&str; 13] = [
    "bind_query",
    "bind_add_ref",
    "bind_release",
    "bind_register_bound",
    "bind_revoke_bound",
    "bind_release_bound",
    "bind_set_options",
    "bind_get_options",
    "bind_get_rot",
    "bind_register_param",
    "bind_get_param",
    "bind_enum_params",
    "bind_revoke_param",
];

pub(crate) fn register(dispatcher: &mut WinCeDispatcher) {
    dispatcher.register_handler("ole32.dll", "CreateBindCtx", create);
    for name in METHODS {
        dispatcher.register_handler("ole32.dll", name, dispatch);
    }
}
fn ret(value: u32) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(value))
}
fn output(ctx: &CallCtx<'_>, ptr: u32, size: u32) -> bool {
    ptr != 0 && ctx.cpu.check_guest_access(ptr, size, Prot::WRITE).is_ok()
}
fn input(ctx: &CallCtx<'_>, ptr: u32, size: u32) -> bool {
    ptr != 0 && ctx.cpu.check_guest_access(ptr, size, Prot::READ).is_ok()
}

fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let out = ctx.arg_u32(1)?;
    if !output(ctx, out, 4) {
        return ret(INVALID);
    }
    ctx.cpu.write_mem(out, &0u32.to_le_bytes())?;
    if ctx.arg_u32(0)? != 0 {
        return ret(INVALID);
    }
    let Some(exports) = ctx
        .kernel
        .dynamic_exports
        .get(&pocket_kernel::OLE32_MODULE_HANDLE)
    else {
        return ret(UNEXPECTED);
    };
    let Some(addresses) = METHODS
        .iter()
        .map(|name| exports.get(*name).copied())
        .collect::<Option<Vec<_>>>()
    else {
        return ret(UNEXPECTED);
    };
    if addresses.contains(&0) {
        return ret(UNEXPECTED);
    }
    let Some(vtable) = ctx.kernel.heap.alloc(52) else {
        return ret(NO_MEMORY);
    };
    let Some(object) = ctx.kernel.heap.alloc(4) else {
        ctx.kernel.heap.free(vtable);
        return ret(NO_MEMORY);
    };
    let write = (|| {
        ctx.cpu.write_mem(
            vtable,
            &addresses
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )?;
        ctx.cpu.write_mem(object, &vtable.to_le_bytes())?;
        ctx.cpu.write_mem(out, &object.to_le_bytes())
    })();
    if let Err(error) = write {
        ctx.kernel.heap.free(object);
        ctx.kernel.heap.free(vtable);
        return Err(error.into());
    }
    ctx.kernel.ce.com.bind_contexts.insert(
        object,
        BindContext {
            vtable,
            refs: 1,
            options: [32, 0, 2, 0, 0, 5, 0x409, 0],
            bound: Vec::new(),
            params: Default::default(),
        },
    );
    ret(0)
}

fn valid_reference(ctx: &mut CallCtx<'_>, object: u32, slot: u32) -> Result<bool, KernelError> {
    Ok(native_com::method(ctx, object, slot)?.is_some())
}

fn dispatch(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if let Some(result) = native_com::resume_references(ctx)? {
        return Ok(result);
    }
    let this = ctx.arg_u32(0)?;
    if !ctx.kernel.ce.com.bind_contexts.contains_key(&this) {
        return ret(UNEXPECTED);
    }
    let name = ctx.thunk.friendly_name.as_deref().unwrap_or("");
    match name {
        "bind_query" => {
            let iid = ctx.arg_u32(1)?;
            let out = ctx.arg_u32(2)?;
            if !input(ctx, iid, 16) || !output(ctx, out, 4) {
                return ret(INVALID);
            }
            ctx.cpu.write_mem(out, &0u32.to_le_bytes())?;
            let bytes = ctx.cpu.read_mem(iid, 16)?;
            let mut unknown = [0u8; 16];
            unknown[8] = 0xc0;
            unknown[15] = 0x46;
            let mut binding = unknown;
            binding[0] = 0x0e;
            if bytes != unknown && bytes != binding {
                return ret(0x8000_4002);
            }
            ctx.cpu.write_mem(out, &this.to_le_bytes())?;
            let state = ctx.kernel.ce.com.bind_contexts.get_mut(&this).unwrap();
            state.refs = state.refs.saturating_add(1);
            ret(0)
        }
        "bind_add_ref" => {
            let state = ctx.kernel.ce.com.bind_contexts.get_mut(&this).unwrap();
            state.refs = state.refs.saturating_add(1);
            ret(state.refs)
        }
        "bind_release" => {
            let state = ctx.kernel.ce.com.bind_contexts.get_mut(&this).unwrap();
            state.refs -= 1;
            if state.refs != 0 {
                return ret(state.refs);
            }
            let state = ctx.kernel.ce.com.bind_contexts.remove(&this).unwrap();
            let refs = state
                .bound
                .into_iter()
                .chain(state.params.into_values())
                .map(|object| (object, 2))
                .collect();
            native_com::references(ctx, refs, 0, vec![this, state.vtable])
        }
        "bind_set_options" | "bind_get_options" => {
            let ptr = ctx.arg_u32(1)?;
            if !input(ctx, ptr, 4) {
                return ret(INVALID);
            }
            let size = ctx.cpu.read_u32_le(ptr)?;
            if !(16..=32).contains(&size) || size % 4 != 0 {
                return ret(INVALID);
            }
            if name == "bind_set_options" {
                if !input(ctx, ptr, size) {
                    return ret(INVALID);
                }
                let bytes = ctx.cpu.read_mem(ptr, size)?;
                let state = ctx.kernel.ce.com.bind_contexts.get_mut(&this).unwrap();
                for (i, word) in bytes.chunks_exact(4).enumerate().skip(1) {
                    state.options[i] = u32::from_le_bytes(word.try_into().unwrap());
                }
            } else {
                if !output(ctx, ptr, size) {
                    return ret(INVALID);
                }
                let mut options = ctx.kernel.ce.com.bind_contexts[&this].options;
                options[0] = size;
                ctx.cpu.write_mem(
                    ptr,
                    &options
                        .into_iter()
                        .take((size / 4) as usize)
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>(),
                )?;
            }
            ret(0)
        }
        "bind_register_param" | "bind_get_param" | "bind_revoke_param" => {
            let key = ctx.arg_u32(1)?;
            let key = match crate::ce_shortcut::text(ctx, key, 32768) {
                Ok(key) => key,
                Err(_) => return ret(INVALID),
            };
            let old = ctx.kernel.ce.com.bind_contexts[&this]
                .params
                .get(&key)
                .copied();
            if name == "bind_get_param" {
                let out = ctx.arg_u32(2)?;
                if !output(ctx, out, 4) {
                    return ret(INVALID);
                }
                ctx.cpu.write_mem(out, &0u32.to_le_bytes())?;
                let Some(object) = old else {
                    return ret(0x8000_4005);
                };
                if !valid_reference(ctx, object, 1)? {
                    return ret(INVALID);
                }
                ctx.cpu.write_mem(out, &object.to_le_bytes())?;
                return native_com::references(ctx, vec![(object, 1)], 0, Vec::new());
            }
            if let Some(old) = old {
                if !valid_reference(ctx, old, 2)? {
                    return ret(INVALID);
                }
            }
            if name == "bind_register_param" {
                let object = ctx.arg_u32(2)?;
                if !valid_reference(ctx, object, 1)? || !valid_reference(ctx, object, 2)? {
                    return ret(INVALID);
                }
                ctx.kernel
                    .ce
                    .com
                    .bind_contexts
                    .get_mut(&this)
                    .unwrap()
                    .params
                    .insert(key, object);
                let refs = std::iter::once((object, 1))
                    .chain(old.map(|old| (old, 2)))
                    .collect();
                native_com::references(ctx, refs, 0, Vec::new())
            } else {
                let Some(object) = old else {
                    return ret(0x8000_4005);
                };
                ctx.kernel
                    .ce
                    .com
                    .bind_contexts
                    .get_mut(&this)
                    .unwrap()
                    .params
                    .remove(&key);
                native_com::references(ctx, vec![(object, 2)], 0, Vec::new())
            }
        }
        "bind_register_bound" | "bind_revoke_bound" => {
            let object = ctx.arg_u32(1)?;
            if name == "bind_register_bound" {
                if !valid_reference(ctx, object, 1)? || !valid_reference(ctx, object, 2)? {
                    return ret(INVALID);
                }
                ctx.kernel
                    .ce
                    .com
                    .bind_contexts
                    .get_mut(&this)
                    .unwrap()
                    .bound
                    .push(object);
                native_com::references(ctx, vec![(object, 1)], 0, Vec::new())
            } else {
                let Some(index) = ctx.kernel.ce.com.bind_contexts[&this]
                    .bound
                    .iter()
                    .position(|item| *item == object)
                else {
                    return ret(NOT_BOUND);
                };
                if !valid_reference(ctx, object, 2)? {
                    return ret(INVALID);
                }
                ctx.kernel
                    .ce
                    .com
                    .bind_contexts
                    .get_mut(&this)
                    .unwrap()
                    .bound
                    .remove(index);
                native_com::references(ctx, vec![(object, 2)], 0, Vec::new())
            }
        }
        "bind_release_bound" => {
            let bound = std::mem::take(
                &mut ctx
                    .kernel
                    .ce
                    .com
                    .bind_contexts
                    .get_mut(&this)
                    .unwrap()
                    .bound,
            );
            native_com::references(
                ctx,
                bound.into_iter().map(|object| (object, 2)).collect(),
                0,
                Vec::new(),
            )
        }
        "bind_get_rot" | "bind_enum_params" => {
            let out = ctx.arg_u32(1)?;
            if !output(ctx, out, 4) {
                return ret(INVALID);
            }
            ctx.cpu.write_mem(out, &0u32.to_le_bytes())?;
            ret(0x8000_4001)
        }
        _ => ret(UNEXPECTED),
    }
}
