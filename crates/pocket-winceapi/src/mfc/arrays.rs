//! Shared CE array storage. CString cells own their reference-counted strings;
//! relocating their pointers transfers ownership instead of duplicating it.
use super::*;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (47, "??0CByteArray@@QAA@XZ", construct_byte),
    (218, "??1CByteArray@@UAA@XZ", destruct_byte),
    (2413, "?SetSize@CByteArray@@QAAXHH@Z", set_size_byte),
    (2304, "?SetAtGrow@CByteArray@@QAAXHE@Z", set_at_grow_byte),
    (
        2283,
        "?Serialize@CByteArray@@UAAXAAVCArchive@@@Z",
        serialize_byte,
    ),
    (788, "?Append@CObArray@@QAAHABV1@@Z", append_objects),
    (892, "?Copy@CObArray@@QAAXABV1@@Z", copy_objects),
    (1190, "?FreeExtra@CObArray@@QAAXXZ", free_extra_objects),
    (
        1651,
        "?InsertAt@CObArray@@QAAXHPAV1@@Z",
        insert_array_objects,
    ),
    (
        1652,
        "?InsertAt@CObArray@@QAAXHPAVCObject@@H@Z",
        insert_object,
    ),
    (792, "?Append@CWordArray@@QAAHABV1@@Z", append_word),
    (896, "?Copy@CWordArray@@QAAXABV1@@Z", copy_word),
    (1195, "?FreeExtra@CWordArray@@QAAXXZ", free_extra_word),
    (
        1661,
        "?InsertAt@CWordArray@@QAAXHPAV1@@Z",
        insert_array_word,
    ),
    (1193, "?FreeExtra@CStringArray@@QAAXXZ", free_extra_strings),
    (87, "??0CDWordArray@@QAA@XZ", construct_dword),
    (237, "??1CDWordArray@@UAA@XZ", destruct_dword),
    (787, "?Append@CDWordArray@@QAAHABV1@@Z", append_dword),
    (891, "?Copy@CDWordArray@@QAAXABV1@@Z", copy_dword),
    (1189, "?FreeExtra@CDWordArray@@QAAXXZ", free_extra_dword),
    (1649, "?InsertAt@CDWordArray@@QAAXHKH@Z", insert_dword),
    (
        1650,
        "?InsertAt@CDWordArray@@QAAXHPAV1@@Z",
        insert_array_dword,
    ),
    (2186, "?RemoveAt@CDWordArray@@QAAXHH@Z", remove_dword),
    (
        2284,
        "?Serialize@CDWordArray@@UAAXAAVCArchive@@@Z",
        serialize_dword,
    ),
    (2305, "?SetAtGrow@CDWordArray@@QAAXHK@Z", set_at_grow_dword),
    (2414, "?SetSize@CDWordArray@@QAAXHH@Z", set_size_dword),
    (
        1463,
        "?GetRuntimeClass@CDWordArray@@UBAPAUCRuntimeClass@@XZ",
        runtime_dword,
    ),
    (
        2289,
        "?Serialize@CObArray@@UAAXAAVCArchive@@@Z",
        serialize_objects,
    ),
    (
        2292,
        "?Serialize@CStringArray@@UAAXAAVCArchive@@@Z",
        serialize_strings,
    ),
    (134, "??0CObArray@@QAA@XZ", construct_objects),
    (269, "??1CObArray@@UAA@XZ", destruct_objects),
    (2415, "?SetSize@CObArray@@QAAXHH@Z", set_size_objects),
    (
        2306,
        "?SetAtGrow@CObArray@@QAAXHPAVCObject@@@Z",
        set_at_grow_object,
    ),
    (2187, "?RemoveAt@CObArray@@QAAXHH@Z", remove_objects),
    (
        1500,
        "?GetRuntimeClass@CObArray@@UBAPAUCRuntimeClass@@XZ",
        runtime_objects,
    ),
    (1660, "?InsertAt@CWordArray@@QAAXHGH@Z", insert_word),
    (2194, "?RemoveAt@CWordArray@@QAAXHH@Z", remove_word),
    (207, "??0CWordArray@@QAA@XZ", construct_word),
    (314, "??1CWordArray@@UAA@XZ", destruct_word),
    (2419, "?SetSize@CWordArray@@QAAXHH@Z", set_size_word),
    (2311, "?SetAtGrow@CWordArray@@QAAXHG@Z", set_at_grow_word),
    (
        2294,
        "?Serialize@CWordArray@@UAAXAAVCArchive@@@Z",
        serialize_word,
    ),
    (
        1543,
        "?GetRuntimeClass@CWordArray@@UBAPAUCRuntimeClass@@XZ",
        runtime_word,
    ),
    (182, "??0CStringArray@@QAA@XZ", construct),
    (298, "??1CStringArray@@UAA@XZ", destruct),
    (2417, "?SetSize@CStringArray@@QAAXHH@Z", set_size),
    (2309, "?SetAtGrow@CStringArray@@QAAXHPBG@Z", set_at_grow),
    (
        2308,
        "?SetAtGrow@CStringArray@@QAAXHABVCString@@@Z",
        set_at_grow_string,
    ),
];
#[derive(Clone, Copy)]
pub(super) enum Element {
    Byte,
    Word,
    Dword,
    Pointer,
    String,
}
impl Element {
    fn stride(self) -> u32 {
        match self {
            Self::Byte => 1,
            Self::Word => 2,
            Self::String | Self::Pointer | Self::Dword => 4,
        }
    }
    fn init(self, ctx: &mut CallCtx<'_>, p: u32, count: u32) -> Result<(), KernelError> {
        match self {
            Self::Byte | Self::Word | Self::Dword | Self::Pointer => ctx
                .cpu
                .write_mem(p, &vec![0; count as usize * self.stride() as usize])
                .map_err(Into::into),
            Self::String => {
                for i in 0..count {
                    if let Err(e) = new_string(ctx, p + i * 4, &[]) {
                        for j in 0..i {
                            release_string(ctx, p + j * 4)?;
                        }
                        return Err(e);
                    }
                }
                Ok(())
            }
        }
    }
    fn release(self, ctx: &mut CallCtx<'_>, p: u32, count: u32) -> Result<(), KernelError> {
        if let Self::String = self {
            for i in 0..count {
                release_string(ctx, p + i * 4)?;
            }
        }
        Ok(())
    }
}
pub(super) fn resize(
    ctx: &mut CallCtx<'_>,
    object: u32,
    size: u32,
    grow: i32,
    kind: Element,
) -> Result<(), KernelError> {
    ctx.cpu
        .check_guest_access(object, 20, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let stride = kind.stride();
    if size > i32::MAX as u32 / stride || grow < -1 {
        return Err(KernelError::Loader("MFC array size/growth invalid".into()));
    }
    let data = word(ctx, object + 4)?;
    let old = word(ctx, object + 8)?;
    let capacity = word(ctx, object + 12)?;
    let growth = if grow < 0 {
        word(ctx, object + 16)?
    } else {
        grow as u32
    };
    if old > capacity || capacity > i32::MAX as u32 / stride || (data == 0 && capacity != 0) {
        return Err(KernelError::Loader("MFC array object invalid".into()));
    }
    if data != 0 && capacity != 0 {
        ctx.cpu.check_guest_access(
            data,
            capacity * stride,
            pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
        )?;
    }
    if size == 0 {
        kind.release(ctx, data, old)?;
        if data != 0 {
            ctx.kernel.heap.free(data);
        }
        ctx.cpu.write_mem(object + 4, &[0; 12])?;
    } else if data == 0 || size > capacity {
        let next = if data == 0 {
            size
        } else {
            size.max(
                capacity
                    .checked_add(if growth == 0 {
                        (old / 8).clamp(4, 1024)
                    } else {
                        growth
                    })
                    .ok_or_else(|| KernelError::Loader("MFC array capacity overflow".into()))?,
            )
        };
        let bytes = next
            .checked_mul(stride)
            .filter(|n| *n <= i32::MAX as u32)
            .ok_or_else(|| KernelError::Loader("MFC array storage overflow".into()))?;
        let p = alloc(ctx, bytes)?;
        let result = (|| {
            if old != 0 {
                let values = ctx.cpu.read_mem(data, old * stride)?;
                ctx.cpu.write_mem(p, &values)?;
            }
            kind.init(ctx, p + old * stride, size - old)?;
            Ok::<_, KernelError>(())
        })();
        if let Err(e) = result {
            ctx.kernel.heap.free(p);
            return Err(e);
        }
        put(ctx, object + 4, p)?;
        put(ctx, object + 8, size)?;
        put(ctx, object + 12, next)?;
        if data != 0 {
            ctx.kernel.heap.free(data);
        }
    } else {
        if size > old {
            kind.init(ctx, data + old * stride, size - old)?;
        } else {
            kind.release(ctx, data + size * stride, old - size)?;
        }
        put(ctx, object + 8, size)?;
    }
    put(ctx, object + 16, growth)
}
fn construct_kind(ctx: &mut CallCtx<'_>, class: &str) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    ctx.cpu.check_guest_access(o, 20, pocket_cpu::Prot::WRITE)?;
    let table = super::classes::vtable(ctx, class)?;
    ctx.cpu.write_mem(o, &[0; 20])?;
    put(ctx, o, table)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct_kind(ctx, "CStringArray")
}
fn construct_objects(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct_kind(ctx, "CObArray")
}
fn destruct_objects(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct_kind(ctx, Element::Pointer)
}
fn set_size_objects(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    set_size_kind(ctx, Element::Pointer)
}
fn runtime_objects(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx, "CObArray",
    )?))
}
fn set_at_grow_object(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    set_at_grow_scalar(ctx, Element::Pointer)
}
fn set_at_grow_scalar(
    ctx: &mut CallCtx<'_>,
    kind: Element,
) -> Result<DispatchOutcome, KernelError> {
    set_at_grow_plain(ctx, kind)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn set_at_grow_plain(ctx: &mut CallCtx<'_>, kind: Element) -> Result<u32, KernelError> {
    let object = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)?;
    let value = ctx.arg_u32(2)?;
    let count = index
        .checked_add(1)
        .ok_or_else(|| KernelError::Dispatch("MFC array index overflow".into()))?;
    if index >= word(ctx, object + 8)? {
        resize(ctx, object, count, -1, kind)?;
    }
    let data = word(ctx, object + 4)?;
    let stride = kind.stride();
    let destination = index
        .checked_mul(stride)
        .and_then(|offset| data.checked_add(offset))
        .ok_or_else(|| KernelError::Dispatch("MFC array index overflow".into()))?;
    ctx.cpu
        .write_mem(destination, &value.to_le_bytes()[..stride as usize])?;
    Ok(object)
}
fn construct_byte(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct_kind(ctx, "CByteArray")
}
fn destruct_byte(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct_kind(ctx, Element::Byte)
}
fn set_size_byte(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    set_size_kind(ctx, Element::Byte)
}
fn set_at_grow_byte(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(set_at_grow_plain(
        ctx,
        Element::Byte,
    )?))
}
fn serialize_byte(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    serialize_plain(ctx, Element::Byte)
}

fn construct_word(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct_kind(ctx, "CWordArray")
}
fn destruct_kind(ctx: &mut CallCtx<'_>, kind: Element) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    resize(ctx, o, 0, -1, kind)?;
    let table = super::classes::vtable(ctx, "CObject")?;
    put(ctx, o, table)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct_kind(ctx, Element::String)
}
fn destruct_word(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct_kind(ctx, Element::Word)
}
fn set_size_kind(ctx: &mut CallCtx<'_>, kind: Element) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let n = ctx.arg_u32(1)?;
    let grow = ctx.arg_u32(2)? as i32;
    resize(ctx, o, n, grow, kind)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn set_size(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    set_size_kind(ctx, Element::String)
}
fn set_size_word(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    set_size_kind(ctx, Element::Word)
}
fn set_at_grow_word(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(set_at_grow_plain(
        ctx,
        Element::Word,
    )?))
}

fn runtime_word(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx,
        "CWordArray",
    )?))
}
fn serialize_word(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    serialize_plain(ctx, Element::Word)
}
pub(super) fn serialize_plain(
    ctx: &mut CallCtx<'_>,
    kind: Element,
) -> Result<DispatchOutcome, KernelError> {
    use super::callbacks::Frame;
    use pocket_cpu::regs::ArmReg;
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let stream = f.arg(ctx, 1)?;
    let loading = archive::loading(ctx, stream)?;
    if f.fresh {
        if loading {
            return f.call_service(ctx, 1, archive::READ_COUNT, &[stream]);
        }
        let count = word(ctx, object + 8)?;
        let capacity = word(ctx, object + 12)?;
        if count > capacity {
            return Err(KernelError::Dispatch("MFC array: corrupt size".into()));
        }
        f.set(ctx, 0, count)?;
        return f.call_service(ctx, 1, archive::WRITE_COUNT, &[stream, count]);
    }
    let phase = f.phase(ctx)?;
    if phase == 1 && loading {
        let count = ctx.cpu.read_reg(ArmReg::R0)?;
        resize(ctx, object, count, -1, kind)?;
        f.set(ctx, 0, count)?;
    }
    let count = f.get(ctx, 0)?;
    let data = word(ctx, object + 4)?;
    if matches!(kind, Element::Byte | Element::Word | Element::Dword) {
        let bytes = count
            .checked_mul(kind.stride())
            .ok_or_else(|| KernelError::Dispatch("MFC array size overflow".into()))?;
        if phase == 1 {
            return f.call_service(
                ctx,
                2,
                if loading {
                    archive::READ
                } else {
                    archive::WRITE
                },
                &[stream, data, bytes],
            );
        }
        if loading && ctx.cpu.read_reg(ArmReg::R0)? != bytes {
            return Err(archive::fault(ctx, stream, 3));
        }
        return f.finish(ctx, 0);
    }
    let mut i = f.get(ctx, 1)?;
    if phase == 2 {
        if loading && matches!(kind, Element::Pointer) {
            let object = ctx.cpu.read_reg(ArmReg::R0)?;
            put(ctx, data + i * 4, object)?;
        }
        i += 1;
        f.set(ctx, 1, i)?;
    }
    if i == count {
        return f.finish(ctx, 0);
    }
    let (service, value) = match (kind, loading) {
        (Element::String, true) => (archive::READ_STRING, data + i * 4),
        (Element::String, false) => (archive::WRITE_STRING, data + i * 4),
        (Element::Pointer, true) => (super::archive_graph::READ_OBJECT, 0),
        (Element::Pointer, false) => (super::archive_graph::WRITE_OBJECT, word(ctx, data + i * 4)?),
        _ => unreachable!(),
    };
    f.call_service(ctx, 2, service, &[stream, value])
}
fn serialize_strings(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    serialize_plain(ctx, Element::String)
}
fn serialize_objects(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    serialize_plain(ctx, Element::Pointer)
}

fn set_at_grow(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (o, index, p) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    // Copy before resizing: p may point into this very array's string storage.
    let text = super::strings::wide(ctx, p)?;
    set_string_at_grow(ctx, o, index, &text)
}
fn set_at_grow_string(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (object, index, string) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    let text = super::strings::value(ctx, string)?;
    set_string_at_grow(ctx, object, index, &text)
}
fn set_string_at_grow(
    ctx: &mut CallCtx<'_>,
    o: u32,
    index: u32,
    text: &[u16],
) -> Result<DispatchOutcome, KernelError> {
    if index >= word(ctx, o + 8)? {
        resize(
            ctx,
            o,
            index.checked_add(1).unwrap_or(u32::MAX),
            -1,
            Element::String,
        )?;
    }
    let data = word(ctx, o + 4)?;
    super::strings::replace(ctx, data + index * 4, text)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}

#[cfg(test)]
mod tests {
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn resize_transfers_string_ownership_and_shrink_destruction_release_storage() {
        let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
        let run = |cpu: &mut pocket_cpu::stub::StubCpu,
                   k: &mut pocket_kernel::KernelState,
                   d: &mut crate::WinCeDispatcher,
                   n: &str,
                   a: &[u32]| {
            super::super::tests::result(call(cpu, k, d, "mfcce300.dll", n, a))
        };
        run(&mut cpu, &mut k, &mut d, "ord:182", &[0x2000]);
        let baseline = k.heap.free_bytes();
        let bytes: Vec<_> = "saved key"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x1000, &bytes).unwrap();
        run(&mut cpu, &mut k, &mut d, "ord:2309", &[0x2000, 2, 0x1000]);
        let data = cpu.read_u32_le(0x2004).unwrap();
        let string = cpu.read_u32_le(data + 8).unwrap();
        run(
            &mut cpu,
            &mut k,
            &mut d,
            "ord:2417",
            &[0x2000, 100, u32::MAX],
        );
        let data = cpu.read_u32_le(0x2004).unwrap();
        assert_eq!(cpu.read_u32_le(data + 8).unwrap(), string);
        assert_eq!(cpu.read_mem(string, bytes.len() as u32).unwrap(), bytes);
        assert_eq!(cpu.read_u32_le(string - 12).unwrap(), 1);
        run(&mut cpu, &mut k, &mut d, "ord:2417", &[0x2000, 1, u32::MAX]);
        run(&mut cpu, &mut k, &mut d, "ord:2417", &[0x2000, 3, u32::MAX]);
        let data = cpu.read_u32_le(0x2004).unwrap();
        let empty = cpu.read_u32_le(data + 8).unwrap();
        assert_eq!(cpu.read_mem(empty, 2).unwrap(), [0, 0]);
        run(&mut cpu, &mut k, &mut d, "ord:298", &[0x2000]);
        assert_eq!(k.heap.free_bytes(), baseline);
    }
}

fn insert_word(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)?;
    let value = ctx.arg_u32(2)? as u16;
    let count = ctx.arg_u32(3)?;
    insert_repeated(
        ctx,
        object,
        index,
        &value.to_le_bytes(),
        count,
        Element::Word,
    )?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn remove_word(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    remove_plain(ctx, Element::Word)
}
fn remove_objects(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    remove_plain(ctx, Element::Pointer)
}
fn remove_plain(ctx: &mut CallCtx<'_>, kind: Element) -> Result<DispatchOutcome, KernelError> {
    let stride = kind.stride();
    let object = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)?;
    let count = ctx.arg_u32(2)?;
    ctx.cpu
        .check_guest_access(object, 20, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let old = word(ctx, object + 8)?;
    let capacity = word(ctx, object + 12)?;
    let end = index
        .checked_add(count)
        .filter(|n| *n <= old)
        .ok_or_else(|| KernelError::Loader("Invalid MFC array removal range".into()))?;
    if old > capacity || capacity > i32::MAX as u32 / stride {
        return Err(KernelError::Loader("Invalid MFC array size".into()));
    }
    if count != 0 {
        let data = word(ctx, object + 4)?;
        let source = data
            .checked_add(end * stride)
            .ok_or_else(|| KernelError::Loader("MFC array removal address overflow".into()))?;
        let destination = data
            .checked_add(index * stride)
            .ok_or_else(|| KernelError::Loader("MFC array removal address overflow".into()))?;
        let tail = ctx.cpu.read_mem(source, (old - end) * stride)?;
        ctx.cpu.write_mem(destination, &tail)?;
        put(ctx, object + 8, old - count)?;
    }
    // Native RemoveAt keeps capacity even when the last item is removed.
    // SetSize(0)/destruction, rather than removal, release the buffer.
    Ok(DispatchOutcome::ReturnedR0(0))
}

#[cfg(test)]
mod word_edit_tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    fn values(cpu: &mut pocket_cpu::stub::StubCpu) -> Vec<u16> {
        let data = cpu.read_u32_le(0x2004).unwrap();
        let count = cpu.read_u32_le(0x2008).unwrap();
        cpu.read_mem(data, count * 2)
            .unwrap()
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect()
    }
    #[test]
    fn insert_remove_move_overlapping_words_and_reuse_capacity_when_empty() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:207",
            &[0x2000],
        );
        let baseline = k.heap.free_bytes();
        for i in 0..3 {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:2311",
                &[0x2000, i, i + 1],
            );
        }
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1660",
                &[0x2000, 1, 0xabcd, 2]
            )),
            0
        );
        assert_eq!(values(&mut cpu), [1, 0xabcd, 0xabcd, 2, 3]);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2194",
            &[0x2000, 1, 2],
        );
        assert_eq!(values(&mut cpu), [1, 2, 3]);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1660",
            &[0x2000, 5, 9, 2],
        );
        assert_eq!(values(&mut cpu), [1, 2, 3, 0, 0, 9, 9]);
        let data = cpu.read_u32_le(0x2004).unwrap();
        let capacity = cpu.read_u32_le(0x200c).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2194",
            &[0x2000, 0, 7],
        );
        assert_eq!(cpu.read_u32_le(0x2008).unwrap(), 0);
        assert_eq!(cpu.read_u32_le(0x2004).unwrap(), data);
        assert_eq!(cpu.read_u32_le(0x200c).unwrap(), capacity);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1660",
            &[0x2000, 0, 7, 1],
        );
        assert_eq!(values(&mut cpu), [7]);
        assert_eq!(cpu.read_u32_le(0x2004).unwrap(), data);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:314",
            &[0x2000],
        );
        assert_eq!(k.heap.free_bytes(), baseline);
    }
}

#[cfg(test)]
mod string_overload_tests {
    use super::super::tests::framework_setup;
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn cstring_reference_overload_reads_the_cell_and_preserves_nuls_and_self_aliasing_on_growth() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:182",
            &[0x2000],
        );
        let available = k.heap.free_bytes();
        cpu.write_mem(0x1000, &[65, 0, 0, 0, 66, 0]).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:180",
            &[0x2100, 0x1000, 3],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2308",
            &[0x2000, 0, 0x2100],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:297",
            &[0x2100],
        );
        let data = cpu.read_u32_le(0x2004).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2308",
            &[0x2000, 40, data],
        );
        let data = cpu.read_u32_le(0x2004).unwrap();
        for index in [0, 40] {
            let text = cpu.read_u32_le(data + index * 4).unwrap();
            assert_eq!(cpu.read_u32_le(text - 8).unwrap(), 3);
            assert_eq!(cpu.read_mem(text, 8).unwrap(), [65, 0, 0, 0, 66, 0, 0, 0]);
        }
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:298",
            &[0x2000],
        );
        assert_eq!(k.heap.free_bytes(), available);
    }
}

#[cfg(test)]
mod object_array_tests {
    use super::super::tests::framework_setup;
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn object_array_growth_and_removal_borrow_elements_and_destruction_frees_only_storage() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:134",
            &[0x2000],
        );
        let first = k.heap.alloc(16).unwrap();
        let second = k.heap.alloc(16).unwrap();
        cpu.write_mem(first, &[0x12; 16]).unwrap();
        cpu.write_mem(second, &[0x34; 16]).unwrap();
        let available = k.heap.free_bytes();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2306",
            &[0x2000, 1, first],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2306",
            &[0x2000, 4, second],
        );
        let data = cpu.read_u32_le(0x2004).unwrap();
        assert_eq!(cpu.read_u32_le(data).unwrap(), 0);
        assert_eq!(cpu.read_u32_le(data + 4).unwrap(), first);
        assert_eq!(cpu.read_u32_le(data + 12).unwrap(), 0);
        assert_eq!(cpu.read_u32_le(data + 16).unwrap(), second);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2187",
            &[0x2000, 1, 2],
        );
        assert_eq!(cpu.read_u32_le(0x2008).unwrap(), 3);
        assert_eq!(cpu.read_u32_le(data + 8).unwrap(), second);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:269",
            &[0x2000],
        );
        assert_eq!(k.heap.free_bytes(), available);
        assert_eq!(k.heap.msize(first), Some(16));
        assert_eq!(k.heap.msize(second), Some(16));
        assert_eq!(cpu.read_mem(first, 16).unwrap(), [0x12; 16]);
        assert_eq!(cpu.read_mem(second, 16).unwrap(), [0x34; 16]);
        k.heap.free(first);
        k.heap.free(second);
    }
}

fn construct_dword(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct_kind(ctx, "CDWordArray")
}
fn destruct_dword(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct_kind(ctx, Element::Dword)
}
fn set_size_dword(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    set_size_kind(ctx, Element::Dword)
}
fn runtime_dword(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(classes::runtime(
        ctx,
        "CDWordArray",
    )?))
}
fn serialize_dword(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    serialize_plain(ctx, Element::Dword)
}
fn remove_dword(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    remove_plain(ctx, Element::Dword)
}
fn set_at_grow_dword(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    set_at_grow_scalar(ctx, Element::Dword)
}
/// Copy before relocation: source and destination may be the same array.
fn copy_dword(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    copy_scalar(ctx, Element::Dword)
}
fn copy_scalar(ctx: &mut CallCtx<'_>, kind: Element) -> Result<DispatchOutcome, KernelError> {
    let target = ctx.arg_u32(0)?;
    let source = ctx.arg_u32(1)?;
    let values = scalar_values(ctx, source, kind)?;
    resize(ctx, target, values.len() as u32 / kind.stride(), -1, kind)?;
    let data = word(ctx, target + 4)?;
    ctx.cpu.write_mem(data, &values)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn append_dword(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    append_scalar(ctx, Element::Dword)
}
fn append_scalar(ctx: &mut CallCtx<'_>, kind: Element) -> Result<DispatchOutcome, KernelError> {
    let target = ctx.arg_u32(0)?;
    let source = ctx.arg_u32(1)?;
    let values = scalar_values(ctx, source, kind)?;
    let old = word(ctx, target + 8)?;
    let count = old
        .checked_add(values.len() as u32 / kind.stride())
        .ok_or_else(|| KernelError::Dispatch("MFC array append overflow".into()))?;
    resize(ctx, target, count, -1, kind)?;
    let data = word(ctx, target + 4)?;
    ctx.cpu.write_mem(data + old * kind.stride(), &values)?;
    Ok(DispatchOutcome::ReturnedR0(old))
}
fn scalar_values(
    ctx: &mut CallCtx<'_>,
    object: u32,
    kind: Element,
) -> Result<Vec<u8>, KernelError> {
    let count = word(ctx, object + 8)?;
    if count > word(ctx, object + 12)? || count > i32::MAX as u32 / kind.stride() {
        return Err(KernelError::Dispatch("Invalid MFC array extent".into()));
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    let data = word(ctx, object + 4)?;
    ctx.cpu
        .check_guest_access(data, count * kind.stride(), pocket_cpu::Prot::READ)?;
    Ok(ctx.cpu.read_mem(data, count * kind.stride())?)
}
fn free_extra_dword(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    free_extra_scalar(ctx, Element::Dword)
}
fn free_extra_scalar(ctx: &mut CallCtx<'_>, kind: Element) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    ctx.cpu
        .check_guest_access(object, 20, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let values = scalar_values(ctx, object, kind)?;
    let count = values.len() as u32 / kind.stride();
    if count != word(ctx, object + 12)? {
        let replacement = if count == 0 {
            0
        } else {
            alloc(ctx, count * kind.stride())?
        };
        if let Err(e) = if values.is_empty() {
            Ok(())
        } else {
            ctx.cpu.write_mem(replacement, &values)
        } {
            if replacement != 0 {
                ctx.kernel.heap.free(replacement);
            }
            return Err(e.into());
        }
        let old = word(ctx, object + 4)?;
        put(ctx, object + 4, replacement)?;
        put(ctx, object + 12, count)?;
        if old != 0 {
            ctx.kernel.heap.free(old);
        }
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn insert_dword(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)?;
    let value = ctx.arg_u32(2)?;
    let count = ctx.arg_u32(3)?;
    insert_repeated(
        ctx,
        object,
        index,
        &value.to_le_bytes(),
        count,
        Element::Dword,
    )?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn insert_array_dword(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)?;
    let source = ctx.arg_u32(2)?;
    let values = scalar_values(ctx, source, Element::Dword)?;
    insert_scalar_values(ctx, object, index, &values, Element::Dword)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn insert_scalar_values(
    ctx: &mut CallCtx<'_>,
    object: u32,
    index: u32,
    values: &[u8],
    kind: Element,
) -> Result<(), KernelError> {
    if values.is_empty() {
        return Ok(());
    }
    let stride = kind.stride();
    let count = u32::try_from(values.len() / stride as usize)
        .map_err(|_| KernelError::Dispatch("MFC array insertion overflow".into()))?;
    let data = reserve_insert(ctx, object, index, count, kind)?;
    ctx.cpu.write_mem(data + index * stride, values)?;
    Ok(())
}

/// Reserve guest storage before materializing repeated scalar values. Large guest
/// requests must fail in the guest allocator rather than allocate a host Vec.
fn insert_repeated(
    ctx: &mut CallCtx<'_>,
    object: u32,
    index: u32,
    value: &[u8],
    count: u32,
    kind: Element,
) -> Result<(), KernelError> {
    if count == 0 {
        return Ok(());
    }
    let data = reserve_insert(ctx, object, index, count, kind)?;
    let mut chunk = [0u8; 4096];
    for scalar in chunk.chunks_exact_mut(value.len()) {
        scalar.copy_from_slice(value);
    }
    let total = count * kind.stride();
    let mut offset = 0;
    while offset < total {
        let bytes = (total - offset).min(chunk.len() as u32);
        ctx.cpu.write_mem(
            data + index * kind.stride() + offset,
            &chunk[..bytes as usize],
        )?;
        offset += bytes;
    }
    Ok(())
}
fn reserve_insert(
    ctx: &mut CallCtx<'_>,
    object: u32,
    index: u32,
    count: u32,
    kind: Element,
) -> Result<u32, KernelError> {
    let stride = kind.stride();
    let old = word(ctx, object + 8)?;
    let size = old
        .max(index)
        .checked_add(count)
        .filter(|size| *size <= i32::MAX as u32 / stride)
        .ok_or_else(|| KernelError::Dispatch("MFC array insertion overflow".into()))?;
    resize(ctx, object, size, -1, kind)?;
    let data = word(ctx, object + 4)?;
    // Move backwards in bounded chunks, including overlapping insertion ranges.
    let mut remaining = old.saturating_sub(index) * stride;
    while remaining != 0 {
        let bytes = remaining.min(4096);
        remaining -= bytes;
        let tail = ctx.cpu.read_mem(data + index * stride + remaining, bytes)?;
        ctx.cpu
            .write_mem(data + (index + count) * stride + remaining, &tail)?;
    }
    Ok(data)
}

fn copy_objects(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    copy_scalar(ctx, Element::Pointer)
}

fn append_objects(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    append_scalar(ctx, Element::Pointer)
}

fn free_extra_objects(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    free_extra_scalar(ctx, Element::Pointer)
}

fn insert_array_objects(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let target = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)?;
    let source = ctx.arg_u32(2)?;
    let values = scalar_values(ctx, source, Element::Pointer)?;
    insert_scalar_values(ctx, target, index, &values, Element::Pointer)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn copy_word(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    copy_scalar(ctx, Element::Word)
}

fn append_word(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    append_scalar(ctx, Element::Word)
}

fn free_extra_word(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    free_extra_scalar(ctx, Element::Word)
}

fn insert_array_word(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let target = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)?;
    let source = ctx.arg_u32(2)?;
    let values = scalar_values(ctx, source, Element::Word)?;
    insert_scalar_values(ctx, target, index, &values, Element::Word)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn free_extra_strings(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    // Raw CString cell relocation transfers references; it neither duplicates
    // nor destroys strings, exactly like array capacity growth.
    free_extra_scalar(ctx, Element::String)
}
fn insert_object(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let target = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)?;
    let value = ctx.arg_u32(2)?;
    let count = ctx.arg_u32(3)?;
    insert_repeated(
        ctx,
        target,
        index,
        &value.to_le_bytes(),
        count,
        Element::Pointer,
    )?;
    Ok(DispatchOutcome::ReturnedR0(0))
}

#[cfg(test)]
mod failure_tests {
    use super::super::{archive::tests::Fixture, window_dispatch::tests::entry};
    use super::*;
    use pocket_cpu::{Cpu, Prot};
    #[test]
    fn string_array_growth_oom_and_protected_object_leave_old_owner_intact() {
        let mut f = Fixture::new(&[]);
        f.call("ord:182", &[0x2400]);
        f.call("ord:2417", &[0x2400, 2, u32::MAX]);
        let object = f.cpu.read_mem(0x2400, 20).unwrap();
        let baseline = f.kernel.heap.free_bytes();
        let mut pressure = Vec::new();
        while let Some(p) = f.kernel.heap.alloc(1) {
            pressure.push(p);
        }
        let thunk = entry(2417);
        let mut ctx = CallCtx {
            cpu: &mut f.cpu,
            kernel: &mut f.kernel,
            thunk: &thunk,
        };
        assert!(resize(&mut ctx, 0x2400, 200, -1, Element::String).is_err());
        assert_eq!(ctx.cpu.read_mem(0x2400, 20).unwrap(), object);
        for p in pressure {
            ctx.kernel.heap.free(p);
        }
        assert_eq!(ctx.kernel.heap.free_bytes(), baseline);
        ctx.cpu.protect_region(0x2000, 0x1000, Prot::READ).unwrap();
        assert!(resize(&mut ctx, 0x2400, 200, -1, Element::String).is_err());
        assert_eq!(ctx.cpu.read_mem(0x2400, 20).unwrap(), object);
        assert_eq!(ctx.kernel.heap.free_bytes(), baseline);
    }
}
