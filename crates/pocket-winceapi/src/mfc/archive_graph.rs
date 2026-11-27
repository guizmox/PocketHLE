//! CArchive object identities and class/schema tags. Private map storage is
//! guest-owned; VFS buffering, factories and virtual serializers have one owner.
use super::callbacks::{service, virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
const MAGIC: u32 = 0x4d464347;
const BUCKETS: u32 = 137;
const LIMIT: u32 = 0x100000;
const STRIDE: u32 = 16;
const CLASS: u32 = 1;
const OBJECT: u32 = 2;
pub(super) const READ_OBJECT: &str = "__PocketHLE_MFC_ArchiveReadObject";
pub(super) const WRITE_OBJECT: &str = "__PocketHLE_MFC_ArchiveWriteObject";
const READ_CLASS: &str = "__PocketHLE_MFC_ArchiveReadClass";
const LOAD_RUNTIME: &str = "__PocketHLE_MFC_RuntimeLoad";
const WRITE_CLASS: &str = "__PocketHLE_MFC_ArchiveWriteClass";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        2146,
        "?ReadObject@CArchive@@QAAPAVCObject@@PBUCRuntimeClass@@@Z",
        read_object,
    ),
    (
        2590,
        "?WriteObject@CArchive@@QAAXPBVCObject@@@Z",
        write_object,
    ),
    (
        2141,
        "?ReadClass@CArchive@@QAAPAUCRuntimeClass@@PBU2@PAIPAK@Z",
        read_class,
    ),
    (
        2587,
        "?WriteClass@CArchive@@QAAXPBUCRuntimeClass@@@Z",
        write_class,
    ),
    (
        2295,
        "?SerializeClass@CArchive@@QAAXPBUCRuntimeClass@@@Z",
        serialize_class,
    ),
    (1754, "?MapObject@CArchive@@QAAXPBVCObject@@@Z", map_object),
    (1399, "?GetObjectSchema@CArchive@@QAAIXZ", object_schema),
    (
        1718,
        "?Load@CRuntimeClass@@SAPAU1@AAVCArchive@@PAI@Z",
        load_runtime,
    ),
    (
        2458,
        "?Store@CRuntimeClass@@QBAXAAVCArchive@@@Z",
        store_runtime,
    ),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", READ_OBJECT, read_object);
    d.register_handler("coredll.dll", WRITE_OBJECT, write_object);
    d.register_handler("coredll.dll", LOAD_RUNTIME, load_runtime);
    d.register_handler("coredll.dll", READ_CLASS, read_class);
    d.register_handler("coredll.dll", WRITE_CLASS, write_class);
}
fn error(message: &str) -> KernelError {
    KernelError::Dispatch(format!("MFC CArchive graph: {message}"))
}
fn direction(ctx: &mut CallCtx<'_>, archive: u32, load: bool) -> Result<(), KernelError> {
    ctx.cpu
        .check_guest_access(archive, 68, Prot::READ | Prot::WRITE)?;
    if super::archive::loading(ctx, archive)? != load {
        return Err(super::archive::fault(
            ctx,
            archive,
            if load { 2 } else { 4 },
        ));
    }
    Ok(())
}
// Private map: capacity/data/magic/reserved followed by hash buckets. Entries
// carry value/kind/schema/next-index. Public schema/count fields retain CE ABI.
// Indices survive vector relocation, and every pointer is guest snapshot memory.
fn map(ctx: &mut CallCtx<'_>, archive: u32) -> Result<u32, KernelError> {
    ctx.cpu
        .check_guest_access(archive, 68, Prot::READ | Prot::WRITE)?;
    let mut p = word(ctx, archive + 52)?;
    if p == 0 {
        p = alloc(ctx, 16 + BUCKETS * 4)?;
        let result = (|| {
            ctx.cpu
                .write_mem(p, &vec![0; (16 + BUCKETS * 4) as usize])?;
            let data = alloc(ctx, 64 * STRIDE)?;
            let result = (|| {
                ctx.cpu.write_mem(data, &[0; STRIDE as usize])?;
                put(ctx, p, 64)?;
                put(ctx, p + 4, data)?;
                put(ctx, p + 8, MAGIC)?;
                put(ctx, archive + 48, 1)?;
                put(ctx, archive + 52, p)
            })();
            if result.is_err() {
                ctx.kernel.heap.free(data);
            }
            result
        })();
        if result.is_err() {
            ctx.kernel.heap.free(p);
        }
        result?;
    }
    ctx.cpu
        .check_guest_access(p, 16 + BUCKETS * 4, Prot::READ | Prot::WRITE)?;
    let capacity = word(ctx, p)?;
    let count = word(ctx, archive + 48)?;
    if word(ctx, p + 8)? != MAGIC
        || capacity == 0
        || capacity > LIMIT
        || count == 0
        || count > capacity
    {
        return Err(error("invalid identity map"));
    }
    let data = word(ctx, p + 4)?;
    if ctx
        .kernel
        .heap
        .msize(data)
        .is_none_or(|n| n < capacity * STRIDE)
    {
        return Err(error("identity vector has no matching allocation"));
    }
    Ok(p)
}
fn entry(ctx: &mut CallCtx<'_>, archive: u32, index: u32) -> Result<u32, KernelError> {
    let p = map(ctx, archive)?;
    if index >= word(ctx, archive + 48)? {
        return Err(super::archive::fault(ctx, archive, 5));
    }
    let at = word(ctx, p + 4)? + index * STRIDE;
    ctx.cpu.check_guest_access(at, STRIDE, Prot::READ)?;
    Ok(at)
}
fn bucket(value: u32, kind: u32) -> u32 {
    (value.wrapping_mul(0x9e3779b1) ^ kind) % BUCKETS
}
fn find(ctx: &mut CallCtx<'_>, archive: u32, value: u32, kind: u32) -> Result<u32, KernelError> {
    let p = map(ctx, archive)?;
    let mut index = word(ctx, p + 16 + bucket(value, kind) * 4)?;
    let count = word(ctx, archive + 48)?;
    for _ in 0..count {
        if index == 0 {
            return Ok(0);
        }
        let e = entry(ctx, archive, index)?;
        if word(ctx, e)? == value && word(ctx, e + 4)? == kind {
            return Ok(index);
        }
        index = word(ctx, e + 12)?;
    }
    Err(error("cyclic identity bucket"))
}
fn insert(
    ctx: &mut CallCtx<'_>,
    archive: u32,
    value: u32,
    kind: u32,
    schema: u32,
) -> Result<u32, KernelError> {
    let p = map(ctx, archive)?;
    let count = word(ctx, archive + 48)?;
    let capacity = word(ctx, p)?;
    if count == LIMIT {
        return Err(error("identity map exceeds its limit"));
    }
    let mut data = word(ctx, p + 4)?;
    if count == capacity {
        let next_capacity = (capacity * 2).min(LIMIT);
        let next = alloc(ctx, next_capacity * STRIDE)?;
        let result = (|| {
            let bytes = ctx.cpu.read_mem(data, count * STRIDE)?;
            ctx.cpu.write_mem(next, &bytes)?;
            put(ctx, p, next_capacity)?;
            put(ctx, p + 4, next)
        })();
        if let Err(e) = result {
            ctx.kernel.heap.free(next);
            return Err(e);
        }
        ctx.kernel.heap.free(data);
        data = next;
    }
    let head = p + 16 + bucket(value, kind) * 4;
    let old = word(ctx, head)?;
    for (offset, value) in [(0, value), (4, kind), (8, schema), (12, old)] {
        put(ctx, data + count * STRIDE + offset, value)?;
    }
    put(ctx, head, count)?;
    put(ctx, archive + 48, count + 1)?;
    Ok(count)
}
pub(super) fn release(ctx: &mut CallCtx<'_>, archive: u32) -> Result<(), KernelError> {
    if word(ctx, archive + 52)? == 0 {
        return Ok(());
    }
    let p = map(ctx, archive)?;
    let data = word(ctx, p + 4)?;
    // Values are borrowed objects/classes. An archive never deletes the graph
    // it serialized, even when Close/Abort releases the identity table.
    ctx.kernel.heap.free(data);
    ctx.kernel.heap.free(p);
    put(ctx, archive + 52, 0)?;
    put(ctx, archive + 48, 0)
}

fn class_name(ctx: &mut CallCtx<'_>, class: u32) -> Result<Vec<u8>, KernelError> {
    ctx.cpu.check_guest_access(class, 24, Prot::READ)?;
    let p = word(ctx, class)?;
    let mut name = Vec::new();
    for i in 0..64 {
        let b = ctx.cpu.read_mem(p + i, 1)?[0];
        if b == 0 {
            return Ok(name);
        }
        name.push(b);
    }
    Err(error("runtime-class name exceeds CE3's bound"))
}

fn write_class(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let a = f.arg(ctx, 0)?;
    let class = f.arg(ctx, 1)?;
    if f.fresh {
        direction(ctx, a, false)?;
        if word(ctx, class + 8)? == 0xffff {
            return Err(super::archive::fault(ctx, a, 6));
        }
        let index = find(ctx, a, class, CLASS)?;
        let bytes = if index != 0 {
            tag_bytes(index, true)
        } else {
            f.set(ctx, 0, 1)?;
            let mut bytes = vec![0xff, 0xff];
            bytes.extend(class_bytes(ctx, class)?);
            bytes
        };
        return write_packet(ctx, &f, 1, a, &bytes);
    }
    if f.get(ctx, 0)? != 0 {
        let schema = word(ctx, class + 8)? & 0x7fffffff;
        insert(ctx, a, class, CLASS, schema)?;
    }
    f.finish(ctx, 0)
}
fn read_class(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let a = f.arg(ctx, 0)?;
    let p = f.address + 128;
    if f.fresh {
        direction(ctx, a, true)?;
        for out in [f.arg(ctx, 2)?, f.arg(ctx, 3)?] {
            if out != 0 {
                ctx.cpu.check_guest_access(out, 4, Prot::WRITE)?;
            }
        }
        return f.call_service(ctx, 10, super::archive::READ, &[a, p, 2]);
    }
    match f.phase(ctx)? {
        10 => {
            exact(ctx, a, 2)?;
            let short = ctx.cpu.read_u16_le(p)?;
            if short == 0x7fff {
                return f.call_service(ctx, 11, super::archive::READ, &[a, p, 4]);
            }
            let tag = u32::from(short & 0x7fff) | (u32::from(short & 0x8000) << 16);
            f.set(ctx, 2, tag)?;
            if short == 0xffff {
                return f.call_service(ctx, 12, LOAD_RUNTIME, &[a, p]);
            }
            resolve_tag(ctx, f, a, tag)
        }
        11 => {
            exact(ctx, a, 4)?;
            let tag = word(ctx, p)?;
            f.set(ctx, 2, tag)?;
            resolve_tag(ctx, f, a, tag)
        }
        12 => {
            let class = ctx.cpu.read_reg(ArmReg::R0)?;
            if class == 0 {
                return Err(super::archive::fault(ctx, a, 6));
            }
            let schema = word(ctx, p)?;
            let current = word(ctx, class + 8)?;
            if current == 0xffff || (current & 0x7fffffff != schema && current & 0x80000000 == 0) {
                return Err(super::archive::fault(ctx, a, 7));
            }
            insert(ctx, a, class, CLASS, schema)?;
            check_class(ctx, f, a, class, schema)
        }
        1 => {
            if ctx.cpu.read_reg(ArmReg::R0)? == 0 {
                return Err(super::archive::fault(ctx, a, 6));
            }
            finish_class(ctx, f, a)
        }
        _ => Err(error("invalid class reader continuation")),
    }
}
fn write_object(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let archive = f.arg(ctx, 0)?;
    let object = f.arg(ctx, 1)?;
    if f.fresh {
        direction(ctx, archive, false)?;
        map(ctx, archive)?;
        if object == 0 {
            return write_packet(ctx, &f, 9, archive, &tag_bytes(0, false));
        }
        let index = find(ctx, archive, object, OBJECT)?;
        if index != 0 {
            return write_packet(ctx, &f, 9, archive, &tag_bytes(index, false));
        }
        let target = virtual_function(ctx, object, 0)?;
        return f.call_args(ctx, 1, target, &[object]);
    }
    match f.phase(ctx)? {
        1 => {
            let class = ctx.cpu.read_reg(ArmReg::R0)?;
            let target = service(ctx, WRITE_CLASS)?;
            f.call_args(ctx, 2, target, &[archive, class])
        }
        2 => {
            insert(ctx, archive, object, OBJECT, 0)?;
            let target = virtual_function(ctx, object, 2)?;
            f.call_args(ctx, 3, target, &[object, archive])
        }
        3 | 9 => f.finish(ctx, 0),
        _ => Err(error("invalid object writer continuation")),
    }
}
fn read_object(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let archive = f.arg(ctx, 0)?;
    if f.fresh {
        direction(ctx, archive, true)?;
        let expected = f.arg(ctx, 1)?;
        let target = service(ctx, READ_CLASS)?;
        return f.call_args(
            ctx,
            1,
            target,
            &[archive, expected, f.address + 120, f.address + 124],
        );
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 if result == 0 => {
            let index = word(ctx, f.address + 124)?;
            let e = entry(ctx, archive, index)?;
            if index != 0 && word(ctx, e + 4)? != OBJECT {
                return Err(super::archive::fault(ctx, archive, 5));
            }
            let object = word(ctx, e)?;
            f.set(ctx, 0, object)?;
            let expected = f.arg(ctx, 1)?;
            if object != 0 && expected != 0 {
                let target = service(ctx, super::class_queries::KIND)?;
                return f.call_args(ctx, 4, target, &[object, expected]);
            }
            f.finish(ctx, object)
        }
        1 => {
            let target = service(ctx, super::class_queries::CREATE)?;
            f.call_args(ctx, 2, target, &[result])
        }
        2 => {
            if result == 0 {
                return Err(super::io_exceptions::fault("MFC CMemory", 0, 0, Vec::new()));
            }
            f.set(ctx, 0, result)?;
            insert(ctx, archive, result, OBJECT, 0)?;
            let previous = word(ctx, archive + 12)?;
            let schema = word(ctx, f.address + 120)?;
            put(ctx, f.address + 144, archive + 12)?;
            put(ctx, f.address + 148, previous)?;
            put(ctx, f.address + 44, f.address + 144)?;
            put(ctx, archive + 12, schema)?;
            let target = virtual_function(ctx, result, 2)?;
            f.call_args(ctx, 3, target, &[result, archive])
        }
        3 => {
            let object = f.get(ctx, 0)?;
            f.finish(ctx, object)
        }
        4 if result != 0 => {
            let object = f.get(ctx, 0)?;
            f.finish(ctx, object)
        }
        4 => Err(super::archive::fault(ctx, archive, 6)),
        _ => Err(error("invalid object reader continuation")),
    }
}
fn map_object(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let archive = ctx.arg_u32(0)?;
    let object = ctx.arg_u32(1)?;
    map(ctx, archive)?;
    if object != 0 {
        insert(ctx, archive, object, OBJECT, 0)?;
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn object_schema(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let archive = ctx.arg_u32(0)?;
    let value = word(ctx, archive + 12)?;
    put(ctx, archive + 12, u32::MAX)?;
    Ok(DispatchOutcome::ReturnedR0(value))
}
fn serialize_class(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if !f.fresh {
        return f.finish(ctx, 0);
    }
    let archive = f.arg(ctx, 0)?;
    let class = f.arg(ctx, 1)?;
    let name = if super::archive::loading(ctx, archive)? {
        READ_CLASS
    } else {
        WRITE_CLASS
    };
    let target = service(ctx, name)?;
    f.call_args(ctx, 1, target, &[archive, class, 0, 0])
}
fn load_runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let a = f.arg(ctx, 0)?;
    let out = f.arg(ctx, 1)?;
    let p = f.address + 128;
    if f.fresh {
        ctx.cpu.check_guest_access(out, 4, Prot::WRITE)?;
        return f.call_service(ctx, 1, super::archive::READ, &[a, p, 4]);
    }
    if f.phase(ctx)? == 1 {
        exact(ctx, a, 4)?;
        let schema = u32::from(ctx.cpu.read_u16_le(p)?);
        put(ctx, out, schema)?;
        let len = u32::from(ctx.cpu.read_u16_le(p + 2)?);
        if len >= 64 {
            return f.finish(ctx, 0);
        }
        f.set(ctx, 0, len)?;
        return f.call_service(ctx, 2, super::archive::READ, &[a, p, len]);
    }
    let len = f.get(ctx, 0)?;
    exact(ctx, a, len)?;
    let bytes = ctx.cpu.read_mem(p, len)?;
    let class = super::classes::find_runtime(ctx, &bytes)?.unwrap_or(0);
    f.finish(ctx, class)
}
fn store_runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let class = f.arg(ctx, 0)?;
        let a = f.arg(ctx, 1)?;
        let bytes = class_bytes(ctx, class)?;
        return write_packet(ctx, &f, 1, a, &bytes);
    }
    f.finish(ctx, 0)
}

fn exact(ctx: &mut CallCtx<'_>, a: u32, n: u32) -> Result<(), KernelError> {
    if ctx.cpu.read_reg(ArmReg::R0)? != n {
        Err(super::archive::fault(ctx, a, 3))
    } else {
        Ok(())
    }
}
fn tag_bytes(index: u32, class: bool) -> Vec<u8> {
    if index < 0x7fff {
        ((index as u16) | if class { 0x8000 } else { 0 })
            .to_le_bytes()
            .to_vec()
    } else {
        let mut b = 0x7fffu16.to_le_bytes().to_vec();
        b.extend_from_slice(&(index | if class { 0x80000000 } else { 0 }).to_le_bytes());
        b
    }
}
fn class_bytes(ctx: &mut CallCtx<'_>, class: u32) -> Result<Vec<u8>, KernelError> {
    let schema = word(ctx, class + 8)?;
    let name = class_name(ctx, class)?;
    let mut bytes = (schema as u16).to_le_bytes().to_vec();
    bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&name);
    Ok(bytes)
}
fn write_packet(
    ctx: &mut CallCtx<'_>,
    f: &Frame,
    phase: u32,
    a: u32,
    bytes: &[u8],
) -> Result<DispatchOutcome, KernelError> {
    let p = alloc(ctx, bytes.len() as u32)?;
    f.own(ctx, p)?;
    ctx.cpu.write_mem(p, bytes)?;
    f.call_service(
        ctx,
        phase,
        super::archive::WRITE,
        &[a, p, bytes.len() as u32],
    )
}
fn resolve_tag(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    a: u32,
    tag: u32,
) -> Result<DispatchOutcome, KernelError> {
    if tag & 0x80000000 == 0 {
        let out = f.arg(ctx, 3)?;
        if out == 0 {
            return Err(super::archive::fault(ctx, a, 5));
        }
        put(ctx, out, tag)?;
        return f.finish(ctx, 0);
    }
    let e = entry(ctx, a, tag & 0x7fffffff)?;
    if word(ctx, e + 4)? != CLASS {
        return Err(super::archive::fault(ctx, a, 5));
    }
    let class = word(ctx, e)?;
    let schema = word(ctx, e + 8)?;
    check_class(ctx, f, a, class, schema)
}
fn check_class(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    a: u32,
    class: u32,
    schema: u32,
) -> Result<DispatchOutcome, KernelError> {
    f.set(ctx, 0, class)?;
    f.set(ctx, 1, schema)?;
    let expected = f.arg(ctx, 1)?;
    if expected != 0 {
        return f.call_service(ctx, 1, super::class_queries::DERIVED, &[class, expected]);
    }
    finish_class(ctx, f, a)
}
fn finish_class(ctx: &mut CallCtx<'_>, f: Frame, a: u32) -> Result<DispatchOutcome, KernelError> {
    let out = f.arg(ctx, 2)?;
    let schema = f.get(ctx, 1)?;
    put(ctx, if out == 0 { a + 12 } else { out }, schema)?;
    let out = f.arg(ctx, 3)?;
    if out != 0 {
        let tag = f.get(ctx, 2)?;
        put(ctx, out, tag)?;
    }
    let class = f.get(ctx, 0)?;
    f.finish(ctx, class)
}

#[cfg(test)]
mod tests {
    use super::super::archive::tests::Fixture;
    use super::super::window_dispatch::tests::{drive_outcomes, drive_with, entry};
    use pocket_cpu::{regs::ArmReg, Cpu};
    use pocket_kernel::{DispatchOutcome, Dispatcher};
    const ARCHIVE: u32 = 0x2300;
    const CLASS: u32 = 0x3100;
    const OBJECT: u32 = 0x2700;
    fn node(f: &mut Fixture) {
        f.cpu.write_mem(0x3000, &0x6100u32.to_le_bytes()).unwrap();
        f.cpu.write_mem(0x3008, &0x6200u32.to_le_bytes()).unwrap();
        f.cpu.write_mem(OBJECT, &0x3000u32.to_le_bytes()).unwrap();
        for (i, value) in [0x3200u32, 12, 7, 0x6300, 0, 0].into_iter().enumerate() {
            f.cpu
                .write_mem(CLASS + 4 * i as u32, &value.to_le_bytes())
                .unwrap();
        }
        f.cpu.write_mem(0x3200, b"Node\0").unwrap();
        f.call("ord:609", &[CLASS]);
        f.call("ord:2649", &[]);
    }
    fn invoke(
        cpu: &mut dyn Cpu,
        k: &mut pocket_kernel::KernelState,
        d: &mut crate::WinCeDispatcher,
        ordinal: u16,
        values: &[u32],
    ) -> DispatchOutcome {
        for (i, &value) in values.iter().enumerate() {
            cpu.write_arg(i as u8, value).unwrap();
        }
        d.dispatch(cpu, &entry(ordinal), k).unwrap()
    }
    fn args(f: &mut Fixture, values: &[u32]) {
        for (i, &value) in values.iter().enumerate() {
            f.cpu.write_arg(i as u8, value).unwrap();
        }
    }
    #[test]
    fn graph_roundtrip_has_native_tags_and_preserves_a_self_reference_before_serialize() {
        let mut f = Fixture::new(&[]);
        node(&mut f);
        let baseline = f.kernel.heap.free_bytes();
        f.construct(0, 0);
        args(&mut f, &[ARCHIVE, OBJECT]);
        let mut callbacks = Vec::new();
        drive_outcomes(
            &mut f.cpu,
            &mut f.kernel,
            &mut f.dispatcher,
            entry(2590),
            256,
            |pc, cpu, k, d| {
                callbacks.push(pc);
                match pc {
                    0x6100 => DispatchOutcome::ReturnedR0(CLASS),
                    0x6200 => {
                        assert_eq!(cpu.read_arg(0).unwrap(), OBJECT);
                        assert_eq!(cpu.read_arg(1).unwrap(), ARCHIVE);
                        // The object's index must already exist before Serialize:
                        // this nested self pointer writes a reference, not a cycle.
                        invoke(cpu, k, d, 2590, &[ARCHIVE, OBJECT])
                    }
                    _ => panic!("unexpected {pc:x}"),
                }
            },
        );
        assert_eq!(callbacks, [0x6100, 0x6200]);
        assert_eq!(f.word(ARCHIVE + 48), 3); // NULL, class, object
        f.call("ord:2590", &[ARCHIVE, OBJECT]);
        f.call("ord:2590", &[ARCHIVE, 0]);
        f.call("ord:851", &[ARCHIVE]);
        let expected = [
            0xff, 0xff, 7, 0, 4, 0, b'N', b'o', b'd', b'e', 2, 0, 2, 0, 0, 0,
        ];
        let actual = std::fs::read(f.directory.path().join("archive.bin")).unwrap();
        assert_eq!(actual, expected);
        let handle = f.handle();
        assert!(f.kernel.vfs.is_open(handle)); // archive borrows CFile
        f.call("ord:212", &[ARCHIVE]);
        assert_eq!(f.kernel.heap.free_bytes(), baseline);
        f.kernel
            .vfs
            .seek(handle, 0, pocket_kernel::vfs::SeekKind::Begin)
            .unwrap();
        f.construct(1, 0);
        args(&mut f, &[ARCHIVE, CLASS]);
        let mut callbacks = Vec::new();
        let restored = drive_outcomes(
            &mut f.cpu,
            &mut f.kernel,
            &mut f.dispatcher,
            entry(2146),
            512,
            |pc, cpu, k, d| {
                callbacks.push(pc);
                match pc {
                    0x6300 => DispatchOutcome::ReturnedR0(OBJECT),
                    0x6200 => {
                        assert_eq!(cpu.read_arg(0).unwrap(), OBJECT);
                        assert_eq!(cpu.read_arg(1).unwrap(), ARCHIVE);
                        let schema = invoke(cpu, k, d, 1399, &[ARCHIVE]);
                        assert!(matches!(schema, DispatchOutcome::ReturnedR0(7)));
                        assert_eq!(cpu.read_u32_le(ARCHIVE + 12).unwrap(), u32::MAX);
                        // Tail-call the nested self-pointer reader. Its result goes
                        // back to the real outer Serialize return continuation.
                        invoke(cpu, k, d, 2146, &[ARCHIVE, 0])
                    }
                    _ => panic!("unexpected {pc:x}"),
                }
            },
        );
        assert_eq!(restored, OBJECT);
        assert_eq!(callbacks, [0x6300, 0x6200]);
        assert_eq!(f.word(ARCHIVE + 12), u32::MAX);
        for expected in [OBJECT, 0] {
            args(&mut f, &[ARCHIVE, 0]);
            assert_eq!(
                drive_with(
                    &mut f.cpu,
                    &mut f.kernel,
                    &mut f.dispatcher,
                    entry(2146),
                    256,
                    |pc, _, _, _| panic!("unexpected {pc:x}")
                ),
                expected
            );
        }
        f.call("ord:212", &[ARCHIVE]);
        assert_eq!(f.kernel.heap.free_bytes(), baseline);
        assert_eq!(f.cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
    }
    #[test]
    fn map_growth_and_abort_release_tables_but_preserve_borrowed_objects_and_files() {
        let mut f = Fixture::new(&[]);
        node(&mut f);
        let baseline = f.kernel.heap.free_bytes();
        f.construct(0, 0);
        for i in 1..=150 {
            f.call("ord:1754", &[ARCHIVE, 0x100000 + i * 16]);
        }
        assert_eq!(f.word(ARCHIVE + 48), 151);
        // No dereference or deletion: MapObject borrows arbitrary graph values.
        f.call("ord:2590", &[ARCHIVE, 0x100000 + 150 * 16]);
        f.call("ord:1166", &[ARCHIVE]);
        assert_eq!(
            std::fs::read(f.directory.path().join("archive.bin")).unwrap(),
            [150, 0]
        );
        f.call("ord:550", &[ARCHIVE]);
        assert_eq!(f.word(ARCHIVE + 52), 0);
        let handle = f.handle();
        assert!(f.kernel.vfs.is_open(handle));
        f.call("ord:212", &[ARCHIVE]);
        assert_eq!(f.kernel.heap.free_bytes(), baseline);
    }
}
