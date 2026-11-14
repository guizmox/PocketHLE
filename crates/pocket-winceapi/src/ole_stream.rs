//! CE Explorer saves folder state through SHLWAPI's IStream helpers. Returning
//! S_OK without an object from CreateStreamOnHGlobal makes the native shell
//! dereference NULL on its next Write. Storage and references belong to a process.
use crate::{CallCtx, WinCeDispatcher};
use pocket_cpu::Prot;
use pocket_kernel::{
    com_stream::{Backing, Stream},
    DispatchOutcome, KernelError,
};

const INVALID: u32 = 0x8007_0057;
const POINTER: u32 = 0x8003_0009;
const NO_MEMORY: u32 = 0x8007_000e;
const FULL: u32 = 0x8003_0070;
const METHODS: [&str; 14] = [
    "stream_query",
    "stream_add_ref",
    "stream_release",
    "stream_read",
    "stream_write",
    "stream_seek",
    "stream_set_size",
    "stream_copy_to",
    "stream_commit",
    "stream_revert",
    "stream_lock",
    "stream_unlock",
    "stream_stat",
    "stream_clone",
];

pub(crate) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("ole32.dll", "CreateStreamOnHGlobal", create);
    d.register_handler("ole32.dll", "GetHGlobalFromStream", get_handle);
    for name in METHODS {
        d.register_handler("ole32.dll", name, dispatch);
    }
}

fn ret(value: u32) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(value))
}
fn access(ctx: &CallCtx<'_>, p: u32, size: u32, prot: Prot) -> bool {
    p != 0 && ctx.cpu.check_guest_access(p, size, prot).is_ok()
}
fn count_output(ctx: &mut CallCtx<'_>, p: u32, size: u32) -> Result<bool, KernelError> {
    if p == 0 {
        return Ok(true);
    }
    if !access(ctx, p, size, Prot::WRITE) {
        return Ok(false);
    }
    ctx.cpu.write_mem(p, &vec![0; size as usize])?;
    Ok(true)
}
fn object(ctx: &mut CallCtx<'_>, backing: u64, position: u64) -> Result<Option<u32>, KernelError> {
    let Some(exports) = ctx
        .kernel
        .dynamic_exports
        .get(&pocket_kernel::OLE32_MODULE_HANDLE)
    else {
        return Ok(None);
    };
    let Some(addresses) = METHODS
        .iter()
        .map(|name| exports.get(*name).copied())
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(None);
    };
    if addresses.contains(&0) {
        return Ok(None);
    }
    let Some(vtable) = ctx.kernel.heap.alloc(56) else {
        return Ok(None);
    };
    let Some(this) = ctx.kernel.heap.alloc(4) else {
        ctx.kernel.heap.free(vtable);
        return Ok(None);
    };
    if let Err(error) = ctx
        .cpu
        .write_mem(
            vtable,
            &addresses
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .and_then(|_| ctx.cpu.write_mem(this, &vtable.to_le_bytes()))
    {
        ctx.kernel.heap.free(this);
        ctx.kernel.heap.free(vtable);
        return Err(error.into());
    }
    ctx.kernel.ce.com.streams.objects.insert(
        this,
        Stream {
            vtable,
            refs: 1,
            backing,
            position,
        },
    );
    ctx.kernel
        .ce
        .com
        .streams
        .backings
        .get_mut(&backing)
        .unwrap()
        .streams += 1;
    Ok(Some(this))
}

fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let out = ctx.arg_u32(2)?;
    if !access(ctx, out, 4, Prot::WRITE) {
        return ret(INVALID);
    }
    ctx.cpu.write_mem(out, &0u32.to_le_bytes())?;
    let supplied = ctx.arg_u32(0)?;
    let delete_on_release = ctx.arg_u32(1)? != 0;
    let size = if supplied != 0 {
        let Some(size) = ctx.kernel.heap.msize(supplied) else {
            return ret(INVALID);
        };
        if !access(ctx, supplied, size, Prot::READ | Prot::WRITE) {
            return ret(INVALID);
        }
        size
    } else {
        0
    };
    let handle = if supplied != 0 {
        supplied
    } else {
        let Some(value) = ctx.kernel.heap.alloc(0) else {
            return ret(NO_MEMORY);
        };
        value
    };
    let Some(id) = ctx.kernel.ce.com.streams.next_id.checked_add(1) else {
        if supplied == 0 {
            ctx.kernel.heap.free(handle);
        }
        return ret(NO_MEMORY);
    };
    ctx.kernel.ce.com.streams.next_id = id;
    ctx.kernel.ce.com.streams.backings.insert(
        id,
        Backing {
            handle,
            capacity: size,
            length: size,
            streams: 0,
            delete_on_release,
        },
    );
    match object(ctx, id, 0) {
        Ok(Some(this)) => {
            ctx.cpu.write_mem(out, &this.to_le_bytes())?;
            ret(0)
        }
        result => {
            ctx.kernel.ce.com.streams.backings.remove(&id);
            if supplied == 0 {
                ctx.kernel.heap.free(handle);
            }
            match result {
                Err(error) => Err(error),
                _ => ret(NO_MEMORY),
            }
        }
    }
}

fn get_handle(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let out = ctx.arg_u32(1)?;
    if !access(ctx, out, 4, Prot::WRITE) {
        return ret(INVALID);
    }
    ctx.cpu.write_mem(out, &0u32.to_le_bytes())?;
    let this = ctx.arg_u32(0)?;
    let Some(stream) = ctx.kernel.ce.com.streams.objects.get(&this).copied() else {
        return ret(INVALID);
    };
    let handle = ctx.kernel.ce.com.streams.backings[&stream.backing].handle;
    ctx.cpu.write_mem(out, &handle.to_le_bytes())?;
    ret(0)
}

fn resize(ctx: &mut CallCtx<'_>, id: u64, length: u32) -> Result<bool, KernelError> {
    let old = ctx.kernel.ce.com.streams.backings[&id];
    if ctx.kernel.heap.msize(old.handle).is_none() {
        return Ok(false);
    }
    let handle = if length > old.capacity {
        let Some(new_handle) = ctx.kernel.heap.alloc(length) else {
            return Ok(false);
        };
        let result = (|| {
            ctx.cpu.write_mem(new_handle, &vec![0; length as usize])?;
            let bytes = ctx.cpu.read_mem(old.handle, old.length)?;
            ctx.cpu.write_mem(new_handle, &bytes)
        })();
        if let Err(error) = result {
            ctx.kernel.heap.free(new_handle);
            return Err(error.into());
        }
        ctx.kernel.heap.free(old.handle);
        new_handle
    } else {
        if length > old.length {
            ctx.cpu.write_mem(
                old.handle + old.length,
                &vec![0; (length - old.length) as usize],
            )?;
        }
        old.handle
    };
    let storage = ctx.kernel.ce.com.streams.backings.get_mut(&id).unwrap();
    storage.handle = handle;
    storage.capacity = old.capacity.max(length);
    storage.length = length;
    Ok(true)
}

fn write(ctx: &mut CallCtx<'_>, this: u32, bytes: &[u8]) -> Result<Option<u32>, KernelError> {
    let stream = ctx.kernel.ce.com.streams.objects[&this];
    let Some(end) = stream
        .position
        .checked_add(bytes.len() as u64)
        .and_then(|v| u32::try_from(v).ok())
    else {
        return Ok(None);
    };
    if !bytes.is_empty() {
        let current = ctx.kernel.ce.com.streams.backings[&stream.backing].length;
        if end > current && !resize(ctx, stream.backing, end)? {
            return Ok(None);
        }
        let storage = ctx.kernel.ce.com.streams.backings[&stream.backing];
        ctx.cpu
            .write_mem(storage.handle + stream.position as u32, bytes)?;
        ctx.kernel
            .ce
            .com
            .streams
            .objects
            .get_mut(&this)
            .unwrap()
            .position = end as u64;
    }
    Ok(Some(bytes.len() as u32))
}

fn dispatch(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let this = ctx.arg_u32(0)?;
    let Some(stream) = ctx.kernel.ce.com.streams.objects.get(&this).copied() else {
        return ret(INVALID);
    };
    let storage = ctx.kernel.ce.com.streams.backings[&stream.backing];
    match ctx.thunk.friendly_name.as_deref().unwrap_or("") {
        "stream_query" => {
            let iid = ctx.arg_u32(1)?;
            let out = ctx.arg_u32(2)?;
            if !access(ctx, out, 4, Prot::WRITE) {
                return ret(INVALID);
            }
            ctx.cpu.write_mem(out, &0u32.to_le_bytes())?;
            if !access(ctx, iid, 16, Prot::READ) {
                return ret(INVALID);
            }
            let bytes = ctx.cpu.read_mem(iid, 16)?;
            let unknown = [0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46];
            let mut istream = unknown;
            istream[0] = 0x0c;
            let sequential = [
                0x30, 0x3a, 0x73, 0x0c, 0x1c, 0x2a, 0xce, 0x11, 0xad, 0xe5, 0, 0xaa, 0, 0x44, 0x77,
                0x3d,
            ];
            if bytes != unknown && bytes != istream && bytes != sequential {
                return ret(0x8000_4002);
            }
            ctx.kernel
                .ce
                .com
                .streams
                .objects
                .get_mut(&this)
                .unwrap()
                .refs = stream.refs.saturating_add(1);
            ctx.cpu.write_mem(out, &this.to_le_bytes())?;
            ret(0)
        }
        "stream_add_ref" => {
            let state = ctx.kernel.ce.com.streams.objects.get_mut(&this).unwrap();
            state.refs = state.refs.saturating_add(1);
            ret(state.refs)
        }
        "stream_release" => {
            if stream.refs > 1 {
                ctx.kernel
                    .ce
                    .com
                    .streams
                    .objects
                    .get_mut(&this)
                    .unwrap()
                    .refs -= 1;
                return ret(stream.refs - 1);
            }
            ctx.kernel.ce.com.streams.objects.remove(&this);
            ctx.kernel.heap.free(stream.vtable);
            ctx.kernel.heap.free(this);
            let state = ctx
                .kernel
                .ce
                .com
                .streams
                .backings
                .get_mut(&stream.backing)
                .unwrap();
            state.streams -= 1;
            if state.streams == 0 {
                let state = ctx
                    .kernel
                    .ce
                    .com
                    .streams
                    .backings
                    .remove(&stream.backing)
                    .unwrap();
                if state.delete_on_release {
                    ctx.kernel.heap.free(state.handle);
                }
            }
            ret(0)
        }
        "stream_read" | "stream_write" => {
            let buf = ctx.arg_u32(1)?;
            let count = ctx.arg_u32(2)?;
            let out = ctx.arg_u32(3)?;
            if !count_output(ctx, out, 4)? {
                return ret(POINTER);
            }
            let reading = ctx.thunk.friendly_name.as_deref() == Some("stream_read");
            if count != 0
                && !access(
                    ctx,
                    buf,
                    count,
                    if reading { Prot::WRITE } else { Prot::READ },
                )
            {
                return ret(POINTER);
            }
            let amount = if reading {
                let amount = (storage.length as u64)
                    .saturating_sub(stream.position)
                    .min(count as u64) as u32;
                if amount != 0 {
                    let bytes = ctx
                        .cpu
                        .read_mem(storage.handle + stream.position as u32, amount)?;
                    ctx.cpu.write_mem(buf, &bytes)?;
                    ctx.kernel
                        .ce
                        .com
                        .streams
                        .objects
                        .get_mut(&this)
                        .unwrap()
                        .position += amount as u64;
                }
                amount
            } else {
                let bytes = if count == 0 {
                    Vec::new()
                } else {
                    ctx.cpu.read_mem(buf, count)?
                };
                let Some(amount) = write(ctx, this, &bytes)? else {
                    return ret(FULL);
                };
                amount
            };
            if out != 0 {
                ctx.cpu.write_mem(out, &amount.to_le_bytes())?;
            }
            ret(u32::from(reading && amount < count))
        }
        "stream_seek" => {
            // CE's MSVC ABI passes LARGE_INTEGER as two consecutive DWORDs.
            let delta = (ctx.arg_u32(1)? as u64 | ((ctx.arg_u32(2)? as u64) << 32)) as i64;
            let origin = ctx.arg_u32(3)?;
            let out = ctx.arg_u32(4)?;
            if out != 0 && !access(ctx, out, 8, Prot::WRITE) {
                return ret(POINTER);
            }
            let base = match origin {
                0 => 0,
                1 => stream.position,
                2 => storage.length as u64,
                _ => return ret(INVALID),
            };
            let next = base as i128 + delta as i128;
            let Ok(next) = u64::try_from(next) else {
                return ret(0x8003_0019);
            };
            ctx.kernel
                .ce
                .com
                .streams
                .objects
                .get_mut(&this)
                .unwrap()
                .position = next;
            if out != 0 {
                ctx.cpu.write_mem(out, &next.to_le_bytes())?;
            }
            ret(0)
        }
        "stream_set_size" => {
            if ctx.arg_u32(2)? != 0 {
                return ret(FULL);
            }
            let length = ctx.arg_u32(1)?;
            ret(if resize(ctx, stream.backing, length)? {
                0
            } else {
                FULL
            })
        }
        "stream_copy_to" => {
            let destination = ctx.arg_u32(1)?;
            let count = ctx.arg_u32(2)? as u64 | ((ctx.arg_u32(3)? as u64) << 32);
            let read_out = ctx.arg_u32(4)?;
            let written_out = ctx.arg_u32(5)?;
            if !count_output(ctx, read_out, 8)? || !count_output(ctx, written_out, 8)? {
                return ret(POINTER);
            }
            if !ctx.kernel.ce.com.streams.objects.contains_key(&destination) {
                return ret(0x8000_4001);
            }
            if destination == this {
                return ret(INVALID);
            }
            let amount = (storage.length as u64)
                .saturating_sub(stream.position)
                .min(count) as u32;
            let bytes = if amount == 0 {
                Vec::new()
            } else {
                ctx.cpu
                    .read_mem(storage.handle + stream.position as u32, amount)?
            };
            let Some(written) = write(ctx, destination, &bytes)? else {
                return ret(FULL);
            };
            ctx.kernel
                .ce
                .com
                .streams
                .objects
                .get_mut(&this)
                .unwrap()
                .position += amount as u64;
            for (out, value) in [(read_out, amount), (written_out, written)] {
                if out != 0 {
                    ctx.cpu.write_mem(out, &(value as u64).to_le_bytes())?;
                }
            }
            ret(u32::from((amount as u64) < count))
        }
        "stream_commit" | "stream_revert" => ret(0),
        "stream_lock" | "stream_unlock" => ret(0x8003_0001),
        "stream_stat" => {
            let out = ctx.arg_u32(1)?;
            if !access(ctx, out, 72, Prot::WRITE) {
                return ret(POINTER);
            }
            if ctx.arg_u32(2)? > 1 {
                return ret(INVALID);
            }
            let mut stat = [0; 72];
            stat[4..8].copy_from_slice(&2u32.to_le_bytes());
            stat[8..16].copy_from_slice(&(storage.length as u64).to_le_bytes());
            stat[40..44].copy_from_slice(&2u32.to_le_bytes());
            ctx.cpu.write_mem(out, &stat)?;
            ret(0)
        }
        "stream_clone" => {
            let out = ctx.arg_u32(1)?;
            if !access(ctx, out, 4, Prot::WRITE) {
                return ret(INVALID);
            }
            ctx.cpu.write_mem(out, &0u32.to_le_bytes())?;
            let Some(clone) = object(ctx, stream.backing, stream.position)? else {
                return ret(NO_MEMORY);
            };
            ctx.cpu.write_mem(out, &clone.to_le_bytes())?;
            ret(0)
        }
        _ => ret(0x8000_4001),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;

    #[test]
    fn explorer_state_streams_share_storage_but_not_seek_positions_or_reference_counts() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        kernel.heap = pocket_kernel::Heap::new(0x50000000, 0x10000);
        kernel.dynamic_exports.insert(
            pocket_kernel::OLE32_MODULE_HANDLE,
            METHODS
                .iter()
                .enumerate()
                .map(|(i, name)| (name.to_string(), 0x6000 + i as u32 * 32))
                .collect(),
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "CreateStreamOnHGlobal",
                &[0, 1, 0x1000]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        let stream = cpu.read_u32_le(0x1000).unwrap();
        let table = cpu.read_u32_le(stream).unwrap();
        assert_eq!(cpu.read_u32_le(table + 12).unwrap(), 0x6060);
        cpu.write_mem(0x1100, b"folder-state").unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "stream_write",
                &[stream, 0x1100, 12, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_u32_le(0x1200).unwrap(), 12);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "stream_seek",
                &[stream, 0, 0, 0, 0x1208]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "stream_clone",
                &[stream, 0x1004]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        let clone = cpu.read_u32_le(0x1004).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "stream_read",
                &[clone, 0x1300, 20, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(cpu.read_mem(0x1300, 12).unwrap(), b"folder-state");
        assert_eq!(kernel.ce.com.streams.objects[&stream].position, 0);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "stream_release",
                &[stream]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        let backing = kernel.ce.com.streams.objects[&clone].backing;
        let handle = kernel.ce.com.streams.backings[&backing].handle;
        assert_eq!(kernel.heap.msize(handle), Some(12));
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "stream_release",
                &[clone]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert!(kernel.ce.com.streams.objects.is_empty());
        assert!(kernel.ce.com.streams.backings.is_empty());
        assert_eq!(kernel.heap.msize(handle), None);
    }

    #[test]
    fn stream_invalid_inputs_clear_outputs_and_preserve_callers_storage() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        kernel.heap = pocket_kernel::Heap::new(0x50000000, 0x10000);
        kernel.dynamic_exports.insert(
            pocket_kernel::OLE32_MODULE_HANDLE,
            METHODS
                .iter()
                .enumerate()
                .map(|(i, name)| (name.to_string(), 0x6000 + i as u32 * 32))
                .collect(),
        );
        cpu.write_mem(0x1000, &0xdeadbeefu32.to_le_bytes()).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "CreateStreamOnHGlobal",
                &[0x2000, 1, 0x1000]
            ),
            DispatchOutcome::ReturnedR0(INVALID)
        );
        assert_eq!(cpu.read_u32_le(0x1000).unwrap(), 0);
        let handle = kernel.heap.alloc(4).unwrap();
        cpu.write_mem(handle, b"data").unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "CreateStreamOnHGlobal",
                &[handle, 0, 0x1000]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        let stream = cpu.read_u32_le(0x1000).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "stream_seek",
                &[stream, u32::MAX, u32::MAX, 0, 0]
            ),
            DispatchOutcome::ReturnedR0(0x80030019)
        );
        assert_eq!(kernel.ce.com.streams.objects[&stream].position, 0);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "stream_set_size",
                &[stream, u32::MAX, 0]
            ),
            DispatchOutcome::ReturnedR0(FULL)
        );
        assert_eq!(cpu.read_mem(handle, 4).unwrap(), b"data");
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "stream_release",
                &[stream]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(kernel.heap.msize(handle), Some(4));
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "ole32.dll",
                "GetHGlobalFromStream",
                &[stream, 0x1000]
            ),
            DispatchOutcome::ReturnedR0(INVALID)
        );
        assert_eq!(cpu.read_u32_le(0x1000).unwrap(), 0);
    }
}
