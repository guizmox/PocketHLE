//! The CE3 MFC byte-array and Unicode CString ABI. No application-specific state.
use crate::{CallCtx, WinCeDispatcher};
use pocket_kernel::{DispatchOutcome, KernelError};
const RUNTIME: &str = "__PocketHLE_CByteArray_GetRuntimeClass";
const DELETE: &str = "__PocketHLE_CByteArray_DeletingDestructor";
const SERIALIZE: &str = "__PocketHLE_CByteArray_Serialize";
const TABLE_BYTES: u32 = 96;

pub(crate) fn register(d: &mut WinCeDispatcher) {
    for (ordinal, name, handler) in [
        (
            47,
            "??0CByteArray@@QAA@XZ",
            byte_array_new as crate::Handler,
        ),
        (218, "??1CByteArray@@QAA@XZ", byte_array_delete),
        (2413, "?SetSize@CByteArray@@QAAXHH@Z", byte_array_resize),
        (
            2304,
            "?SetAtGrow@CByteArray@@QAAXHE@Z",
            byte_array_set_at_grow,
        ),
        (181, "??0CString@@QAA@XZ", string_new),
        (177, "??0CString@@QAA@PBD@Z", string_from_ansi),
        (321, "??4CString@@QAAABV0@ABV0@@Z", string_assign),
        (297, "??1CString@@QAA@XZ", string_delete),
    ] {
        d.register_handler("mfcce300.dll", &format!("ord:{ordinal}"), handler);
        d.register_handler("mfcce300.dll", name, handler);
    }
    // Private guest-callable virtual methods, emitted by the standard thunk loader.
    d.register_handler("coredll.dll", RUNTIME, runtime_class);
    d.register_handler("coredll.dll", DELETE, deleting_destructor);
    d.register_handler("coredll.dll", SERIALIZE, serialize);
}
fn word(ctx: &mut CallCtx<'_>, p: u32) -> Result<u32, KernelError> {
    Ok(u32::from_le_bytes(
        ctx.cpu.read_mem(p, 4)?.try_into().unwrap(),
    ))
}
fn put(ctx: &mut CallCtx<'_>, p: u32, v: u32) -> Result<(), KernelError> {
    ctx.cpu.write_mem(p, &v.to_le_bytes())?;
    Ok(())
}
fn alloc(ctx: &mut CallCtx<'_>, n: u32) -> Result<u32, KernelError> {
    match crate::coredll::do_alloc(ctx, n)? {
        DispatchOutcome::ReturnedR0(p) if p != 0 => Ok(p),
        _ => Err(KernelError::Loader("MFC allocation failed".into())),
    }
}
fn byte_array_new(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let methods: Vec<_> = [RUNTIME, DELETE, SERIALIZE]
        .iter()
        .map(|name| {
            ctx.kernel
                .dynamic_exports
                .values()
                .find_map(|m| m.get(*name).copied())
                .ok_or_else(|| KernelError::Loader(format!("MFC virtual method {name} missing")))
        })
        .collect::<Result<_, _>>()?;
    let table = alloc(ctx, TABLE_BYTES)?;
    let result = (|| {
        for (i, address) in methods.into_iter().enumerate() {
            put(ctx, table + i as u32 * 4, address)?;
        }
        // Retail CObject vtable: runtime class, deleting destructor, Serialize.
        // CRuntimeClass: name, size, schema, creation callback, base, next.
        ctx.cpu.write_mem(table + 64, b"CByteArray\0CObject\0")?;
        for (i, value) in [table + 64, 20, u32::MAX, 0, table + 40, 0]
            .into_iter()
            .enumerate()
        {
            put(ctx, table + 16 + i as u32 * 4, value)?;
        }
        for (i, value) in [table + 75, 4, u32::MAX, 0, 0, 0].into_iter().enumerate() {
            put(ctx, table + 40 + i as u32 * 4, value)?;
        }
        ctx.cpu.write_mem(object, &[0; 20])?;
        put(ctx, object, table)
    })();
    if result.is_err() {
        ctx.kernel.heap.free(table);
    }
    result?;
    Ok(DispatchOutcome::ReturnedR0(object))
}
fn resize(ctx: &mut CallCtx<'_>, object: u32, size: u32, grow: i32) -> Result<(), KernelError> {
    if size > i32::MAX as u32 || grow < -1 {
        return Err(KernelError::Loader(
            "MFC CByteArray: invalid size/growth".into(),
        ));
    }
    let data = word(ctx, object + 4)?;
    let old_size = word(ctx, object + 8)?;
    let capacity = word(ctx, object + 12)?;
    let old_grow = word(ctx, object + 16)?;
    let growth = if grow >= 0 { grow as u32 } else { old_grow };
    if old_size > capacity || (data == 0 && capacity != 0) {
        return Err(KernelError::Loader("MFC CByteArray: corrupt object".into()));
    }
    if size == 0 {
        if data != 0 {
            ctx.kernel.heap.free(data);
        }
        ctx.cpu.write_mem(object + 4, &[0; 12])?;
    } else if data == 0 || size > capacity {
        let new_capacity = if data == 0 {
            size
        } else {
            size.max(
                capacity
                    .checked_add(if growth == 0 {
                        (old_size / 8).clamp(4, 1024)
                    } else {
                        growth
                    })
                    .filter(|n| *n <= i32::MAX as u32)
                    .ok_or_else(|| KernelError::Loader("MFC array capacity overflow".into()))?,
            )
        };
        let pointer = alloc(ctx, new_capacity)?;
        if old_size != 0 {
            let bytes = match ctx.cpu.read_mem(data, old_size) {
                Ok(b) => b,
                Err(e) => {
                    ctx.kernel.heap.free(pointer);
                    return Err(e.into());
                }
            };
            if let Err(e) = ctx.cpu.write_mem(pointer, &bytes) {
                ctx.kernel.heap.free(pointer);
                return Err(e.into());
            }
        }
        put(ctx, object + 4, pointer)?;
        put(ctx, object + 12, new_capacity)?;
        if data != 0 {
            ctx.kernel.heap.free(data);
        }
        put(ctx, object + 8, size)?;
    } else {
        if size > old_size {
            ctx.cpu
                .write_mem(data + old_size, &vec![0; (size - old_size) as usize])?;
        }
        put(ctx, object + 8, size)?;
    }
    put(ctx, object + 16, growth)
}
fn byte_array_resize(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (o, n, g) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)? as i32);
    resize(ctx, o, n, g)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn byte_array_set_at_grow(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (o, i, v) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)? as u8);
    if i >= word(ctx, o + 8)? {
        resize(ctx, o, i.checked_add(1).unwrap_or(u32::MAX), -1)?;
    }
    let data = word(ctx, o + 4)?;
    ctx.cpu.write_mem(data + i, &[v])?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn destroy(ctx: &mut CallCtx<'_>, o: u32) -> Result<(), KernelError> {
    let table = word(ctx, o)?;
    resize(ctx, o, 0, -1)?;
    if table != 0 {
        ctx.kernel.heap.free(table);
    }
    put(ctx, o, 0)
}
fn byte_array_delete(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    destroy(ctx, o)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn deleting_destructor(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (o, flags) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?);
    destroy(ctx, o)?;
    if flags & 1 != 0 {
        ctx.kernel.heap.free(o);
    }
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn runtime_class(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let table = word(ctx, o)?;
    Ok(DispatchOutcome::ReturnedR0(table + 16))
}
fn serialize(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // Explicitly unsupported until CArchive is emulated; never pretend to save data.
    Err(KernelError::Loader(
        "MFC CByteArray::Serialize requires CArchive support".into(),
    ))
}
fn new_string(ctx: &mut CallCtx<'_>, object: u32, text: &[u16]) -> Result<(), KernelError> {
    let len = u32::try_from(text.len())
        .ok()
        .filter(|n| *n <= 0x3ffffff0)
        .ok_or_else(|| KernelError::Loader("MFC string too large".into()))?;
    let p = alloc(ctx, 12 + (len + 1) * 2)?;
    put(ctx, p, 1)?;
    put(ctx, p + 4, len)?;
    put(ctx, p + 8, len)?;
    let bytes: Vec<u8> = text.iter().flat_map(|c| c.to_le_bytes()).collect();
    ctx.cpu.write_mem(p + 12, &bytes)?;
    put(ctx, object, p + 12)
}
fn string_new(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    new_string(ctx, o, &[])?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn release_string(ctx: &mut CallCtx<'_>, o: u32) -> Result<(), KernelError> {
    let p = word(ctx, o)?;
    if p != 0 {
        let refs = word(ctx, p - 12)? as i32;
        if refs <= 1 {
            ctx.kernel.heap.free(p - 12);
        } else {
            put(ctx, p - 12, (refs - 1) as u32)?;
        }
    }
    put(ctx, o, 0)
}
fn string_delete(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    release_string(ctx, o)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}

fn string_from_ansi(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (o, p) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?);
    let mut bytes = Vec::new();
    if p != 0 {
        for i in 0..=1048576u32 {
            let c = ctx.cpu.read_mem(p + i, 1)?[0];
            if c == 0 {
                break;
            }
            if i == 1048576 {
                return Err(KernelError::Loader("MFC string unterminated".into()));
            }
            bytes.push(c);
        }
    }
    let text: Vec<u16> = crate::crt_ansi::decode(&bytes).encode_utf16().collect();
    new_string(ctx, o, &text)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn string_assign(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (o, other) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?);
    if o != other {
        let p = word(ctx, other)?;
        let len = word(ctx, p - 8)?;
        let b = ctx.cpu.read_mem(
            p,
            len.checked_mul(2)
                .filter(|n| *n <= 0x7fffffff)
                .ok_or_else(|| KernelError::Loader("MFC string overflow".into()))?,
        )?;
        let text: Vec<u16> = b
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        // Allocate before releasing: an allocation failure preserves the old value.
        let slot = alloc(ctx, 4)?;
        if let Err(error) = new_string(ctx, slot, &text) {
            ctx.kernel.heap.free(slot);
            return Err(error);
        }
        let p = word(ctx, slot)?;
        release_string(ctx, o)?;
        put(ctx, o, p)?;
        ctx.kernel.heap.free(slot);
    }
    Ok(DispatchOutcome::ReturnedR0(o))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::{Cpu, Prot};
    fn result(v: DispatchOutcome) -> u32 {
        let DispatchOutcome::ReturnedR0(n) = v else {
            panic!("unexpected outcome")
        };
        n
    }
    fn u32_at(cpu: &mut dyn Cpu, p: u32) -> u32 {
        u32::from_le_bytes(cpu.read_mem(p, 4).unwrap().try_into().unwrap())
    }
    #[test]
    fn byte_array_grows_preserves_and_zeroes_after_shrink_then_frees_every_allocation() {
        let (mut cpu, mut k, mut d) = setup();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        k.dynamic_exports.insert(
            1,
            [
                (RUNTIME.into(), 0x70000100),
                (DELETE.into(), 0x70000120),
                (SERIALIZE.into(), 0x70000140),
            ]
            .into(),
        );
        let initial = k.heap.free_bytes();
        let object = 0x2000;
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:47",
                &[object]
            )),
            object
        );
        let table = u32_at(&mut cpu, object);
        assert_eq!(u32_at(&mut cpu, table), 0x70000100);
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                RUNTIME,
                &[object]
            )),
            table + 16
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2413",
            &[object, 8, u32::MAX],
        );
        let first = u32_at(&mut cpu, object + 4);
        cpu.write_mem(first, b"abcdefgh").unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2413",
            &[object, 20, 16],
        );
        let next = u32_at(&mut cpu, object + 4);
        assert_eq!(cpu.read_mem(next, 8).unwrap(), b"abcdefgh");
        assert_eq!(cpu.read_mem(next + 8, 12).unwrap(), [0; 12]);
        assert!(k.heap.msize(first).is_none());
        let capacity = u32_at(&mut cpu, object + 12);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2413",
            &[object, 3, u32::MAX],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2413",
            &[object, 8, u32::MAX],
        );
        assert_eq!(u32_at(&mut cpu, object + 4), next);
        assert_eq!(u32_at(&mut cpu, object + 12), capacity);
        assert_eq!(
            cpu.read_mem(next, 8).unwrap(),
            [b'a', b'b', b'c', 0, 0, 0, 0, 0]
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2304",
            &[object, 30, 0xaa],
        );
        let data = u32_at(&mut cpu, object + 4);
        assert_eq!(u32_at(&mut cpu, object + 8), 31);
        assert_eq!(cpu.read_mem(data + 30, 1).unwrap(), [0xaa]);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:218",
            &[object],
        );
        assert_eq!(k.heap.free_bytes(), initial);
    }
    #[test]
    fn ansi_cstring_assignment_owns_unicode_copy_after_source_destruction() {
        let (mut cpu, mut k, mut d) = setup();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        let before = k.heap.free_bytes();
        cpu.write_mem(0x1800, b"Prix: \x80\xe9\0").unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:177",
            &[0x2000, 0x1800],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:181",
            &[0x2010],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:321",
            &[0x2010, 0x2000],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:297",
            &[0x2000],
        );
        let p = u32_at(&mut cpu, 0x2010);
        let expected: Vec<u8> = "Prix: €é\0"
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        assert_eq!(cpu.read_mem(p, expected.len() as u32).unwrap(), expected);
        assert_eq!(u32_at(&mut cpu, p - 8), 8);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:321",
            &[0x2010, 0x2010],
        );
        assert_eq!(u32_at(&mut cpu, 0x2010), p);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:297",
            &[0x2010],
        );
        assert_eq!(k.heap.free_bytes(), before);
    }

    #[test]
    fn empty_cstring_has_unicode_terminator_length_and_reference_count_and_destructor() {
        let (mut cpu, mut k, mut d) = setup();
        cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        let before = k.heap.free_bytes();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:181",
            &[0x2000],
        );
        let p = u32_at(&mut cpu, 0x2000);
        assert_eq!(cpu.read_mem(p, 2).unwrap(), [0; 2]);
        assert_eq!(u32_at(&mut cpu, p - 8), 0);
        assert_eq!(u32_at(&mut cpu, p - 12), 1);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:297",
            &[0x2000],
        );
        assert_eq!(k.heap.free_bytes(), before);
    }
}
