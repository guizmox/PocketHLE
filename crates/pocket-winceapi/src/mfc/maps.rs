//! CE3 serializable maps share the guest bucket/association/CPlex owner.
//! CString keys and values are owned; CObject values remain borrowed.
use super::*;
use std::collections::HashSet;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        1220,
        "?GetAssocAt@CMapStringToOb@@IBAPAUCAssoc@1@PBGAAI@Z",
        objects_get_assoc,
    ),
    (
        1773,
        "?NewAssoc@CMapStringToOb@@IAAPAUCAssoc@1@XZ",
        objects_new_assoc,
    ),
    (
        1181,
        "?FreeAssoc@CMapStringToOb@@IAAXPAUCAssoc@1@@Z",
        objects_free_assoc,
    ),
    (
        1222,
        "?GetAssocAt@CMapStringToString@@IBAPAUCAssoc@1@PBGAAI@Z",
        strings_get_assoc,
    ),
    (
        1775,
        "?NewAssoc@CMapStringToString@@IAAPAUCAssoc@1@XZ",
        strings_new_assoc,
    ),
    (
        1183,
        "?FreeAssoc@CMapStringToString@@IAAXPAUCAssoc@1@@Z",
        strings_free_assoc,
    ),
    (
        1223,
        "?GetAssocAt@CMapWordToOb@@IBAPAUCAssoc@1@GAAI@Z",
        words_get_assoc,
    ),
    (
        1776,
        "?NewAssoc@CMapWordToOb@@IAAPAUCAssoc@1@XZ",
        words_new_assoc,
    ),
    (
        1184,
        "?FreeAssoc@CMapWordToOb@@IAAXPAUCAssoc@1@@Z",
        words_free_assoc,
    ),
    (125, "??0CMapStringToString@@QAA@H@Z", strings_construct),
    (261, "??1CMapStringToString@@UAA@XZ", strings_destruct),
    (
        356,
        "??ACMapStringToString@@QAAAAVCString@@PBG@Z",
        strings_subscript,
    ),
    (
        1742,
        "?Lookup@CMapStringToString@@QBAHPBGAAVCString@@@Z",
        strings_lookup,
    ),
    (
        1748,
        "?LookupKey@CMapStringToString@@QBAHPBGAAPBG@Z",
        strings_lookup_key,
    ),
    (
        2178,
        "?RemoveAll@CMapStringToString@@QAAXXZ",
        strings_remove_all,
    ),
    (
        2211,
        "?RemoveKey@CMapStringToString@@QAAHPBG@Z",
        strings_remove_key,
    ),
    (
        1388,
        "?GetNextAssoc@CMapStringToString@@QBAXAAPAU__POSITION@@AAVCString@@1@Z",
        strings_next_assoc,
    ),
    (
        2287,
        "?Serialize@CMapStringToString@@UAAXAAVCArchive@@@Z",
        strings_serialize,
    ),
    (
        1629,
        "?InitHashTable@CMapStringToString@@QAAXIH@Z",
        strings_init_hash_table,
    ),
    (126, "??0CMapWordToOb@@QAA@H@Z", words_construct),
    (262, "??1CMapWordToOb@@UAA@XZ", words_destruct),
    (
        357,
        "??ACMapWordToOb@@QAAAAPAVCObject@@G@Z",
        words_subscript,
    ),
    (
        1743,
        "?Lookup@CMapWordToOb@@QBAHGAAPAVCObject@@@Z",
        words_lookup,
    ),
    (2179, "?RemoveAll@CMapWordToOb@@QAAXXZ", words_remove_all),
    (2212, "?RemoveKey@CMapWordToOb@@QAAHG@Z", words_remove_key),
    (
        1389,
        "?GetNextAssoc@CMapWordToOb@@QBAXAAPAU__POSITION@@AAGAAPAVCObject@@@Z",
        words_next_assoc,
    ),
    (
        2288,
        "?Serialize@CMapWordToOb@@UAAXAAVCArchive@@@Z",
        words_serialize,
    ),
    (
        1630,
        "?InitHashTable@CMapWordToOb@@QAAXIH@Z",
        words_init_hash_table,
    ),
    (
        2286,
        "?Serialize@CMapStringToOb@@UAAXAAVCArchive@@@Z",
        serialize,
    ),
    (123, "??0CMapStringToOb@@QAA@H@Z", construct),
    (259, "??1CMapStringToOb@@UAA@XZ", destruct),
    (354, "??ACMapStringToOb@@QAAAAPAVCObject@@PBG@Z", subscript),
    (
        1627,
        "?InitHashTable@CMapStringToOb@@QAAXIH@Z",
        init_hash_table,
    ),
    (
        1740,
        "?Lookup@CMapStringToOb@@QBAHPBGAAPAVCObject@@@Z",
        lookup,
    ),
    (
        1746,
        "?LookupKey@CMapStringToOb@@QBAHPBGAAPBG@Z",
        lookup_key,
    ),
    (2176, "?RemoveAll@CMapStringToOb@@QAAXXZ", remove_all),
    (2209, "?RemoveKey@CMapStringToOb@@QAAHPBG@Z", remove_key),
    (
        1386,
        "?GetNextAssoc@CMapStringToOb@@QBAXAAPAU__POSITION@@AAVCString@@AAPAVCObject@@@Z",
        next_assoc,
    ),
];

#[derive(Clone, Copy)]
enum Kind {
    Objects,
    Strings,
    Words,
}
impl Kind {
    fn class(self) -> &'static str {
        match self {
            Self::Objects => "CMapStringToOb",
            Self::Strings => "CMapStringToString",
            Self::Words => "CMapWordToOb",
        }
    }
    fn word_key(self) -> bool {
        matches!(self, Self::Words)
    }
    fn string_value(self) -> bool {
        matches!(self, Self::Strings)
    }
    fn stride(self) -> u32 {
        if self.word_key() {
            12
        } else {
            16
        }
    }
    fn value_offset(self) -> u32 {
        self.stride() - 4
    }
    fn hash(self, text: &[u16]) -> u32 {
        if self.word_key() {
            u32::from(text[0]) >> 4
        } else {
            text.iter()
                .fold(0u32, |v, c| v.wrapping_mul(33).wrapping_add(u32::from(*c)))
        }
    }
    fn argument(self, ctx: &mut CallCtx<'_>, value: u32) -> Result<Vec<u16>, KernelError> {
        if self.word_key() {
            Ok(vec![value as u16])
        } else {
            strings::wide(ctx, value)
        }
    }
    fn key(self, ctx: &mut CallCtx<'_>, at: u32) -> Result<Vec<u16>, KernelError> {
        if self.word_key() {
            Ok(vec![u16::from_le_bytes(
                ctx.cpu.read_mem(at + 4, 2)?.try_into().unwrap(),
            )])
        } else {
            strings::value(ctx, at + 8)
        }
    }
    fn release(self, ctx: &mut CallCtx<'_>, at: u32) -> Result<(), KernelError> {
        if !self.word_key() {
            release_string(ctx, at + 8)?;
        }
        if self.string_value() {
            release_string(ctx, at + 12)?;
        }
        Ok(())
    }
}

pub(super) fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct_kind(ctx, Kind::Objects)
}
pub(super) fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct_kind(ctx, Kind::Objects)
}
pub(super) fn subscript(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    subscript_kind(ctx, Kind::Objects)
}
pub(super) fn lookup(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    lookup_kind(ctx, Kind::Objects)
}
pub(super) fn lookup_key(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    lookup_key_kind(ctx, Kind::Objects)
}
pub(super) fn remove_all(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    remove_all_kind(ctx, Kind::Objects)
}
pub(super) fn remove_key(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    remove_key_kind(ctx, Kind::Objects)
}
pub(super) fn next_assoc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    next_assoc_kind(ctx, Kind::Objects)
}
pub(super) fn serialize(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    serialize_kind(ctx, Kind::Objects)
}
fn strings_construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct_kind(ctx, Kind::Strings)
}
fn strings_destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct_kind(ctx, Kind::Strings)
}
fn strings_subscript(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    subscript_kind(ctx, Kind::Strings)
}
fn strings_lookup(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    lookup_kind(ctx, Kind::Strings)
}
fn strings_lookup_key(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    lookup_key_kind(ctx, Kind::Strings)
}
fn strings_remove_all(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    remove_all_kind(ctx, Kind::Strings)
}
fn strings_remove_key(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    remove_key_kind(ctx, Kind::Strings)
}
fn strings_next_assoc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    next_assoc_kind(ctx, Kind::Strings)
}
fn strings_serialize(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    serialize_kind(ctx, Kind::Strings)
}
fn strings_init_hash_table(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    init_hash_table(ctx)
}
fn words_construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct_kind(ctx, Kind::Words)
}
fn words_destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct_kind(ctx, Kind::Words)
}
fn words_subscript(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    subscript_kind(ctx, Kind::Words)
}
fn words_lookup(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    lookup_kind(ctx, Kind::Words)
}
fn words_remove_all(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    remove_all_kind(ctx, Kind::Words)
}
fn words_remove_key(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    remove_key_kind(ctx, Kind::Words)
}
fn words_next_assoc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    next_assoc_kind(ctx, Kind::Words)
}
fn words_serialize(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    serialize_kind(ctx, Kind::Words)
}
fn words_init_hash_table(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    init_hash_table(ctx)
}

const MAX: u32 = 0x100000;
fn invalid() -> KernelError {
    KernelError::Dispatch("Invalid MFC string map".into())
}
fn construct_kind(ctx: &mut CallCtx<'_>, kind: Kind) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let block = ctx.arg_u32(1)?;
    if block == 0 || block > MAX {
        return Err(invalid());
    }
    ctx.cpu.check_guest_access(o, 28, pocket_cpu::Prot::WRITE)?;
    let v = super::classes::vtable(ctx, kind.class())?;
    ctx.cpu.write_mem(o, &[0; 28])?;
    put(ctx, o, v)?;
    put(ctx, o + 8, 17)?;
    put(ctx, o + 24, block)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn header(ctx: &mut CallCtx<'_>, o: u32) -> Result<(u32, u32, u32), KernelError> {
    ctx.cpu
        .check_guest_access(o, 28, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let data = word(ctx, o + 4)?;
    let n = word(ctx, o + 8)?;
    let count = word(ctx, o + 12)?;
    if n == 0 || n > MAX || count > MAX || (data == 0 && count != 0) {
        return Err(invalid());
    }
    if data != 0 {
        ctx.cpu.check_guest_access(
            data,
            n * 4,
            pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
        )?;
    }
    Ok((data, n, count))
}
fn find(
    ctx: &mut CallCtx<'_>,
    o: u32,
    text: &[u16],
    kind: Kind,
) -> Result<(u32, u32, u32), KernelError> {
    let (data, n, _) = header(ctx, o)?;
    let hash = kind.hash(text) % n;
    if data == 0 {
        return Ok((0, hash, 0));
    }
    let pool = lists::NodePool::with_stride(ctx, o, kind.stride())?;
    let mut link = data + hash * 4;
    let mut at = word(ctx, link)?;
    let mut seen = HashSet::new();
    while at != 0 {
        pool.validate(at)?;
        if !seen.insert(at) || seen.len() > MAX as usize {
            return Err(invalid());
        }
        ctx.cpu.check_guest_access(
            at,
            kind.stride(),
            pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
        )?;
        if kind.key(ctx, at)? == text {
            return Ok((at, hash, link));
        }
        link = at;
        at = word(ctx, at)?;
    }
    Ok((0, hash, link))
}
fn associations(ctx: &mut CallCtx<'_>, o: u32, kind: Kind) -> Result<Vec<u32>, KernelError> {
    let (data, n, count) = header(ctx, o)?;
    let pool = lists::NodePool::with_stride(ctx, o, kind.stride())?;
    let mut associations = Vec::new();
    let mut seen = HashSet::new();
    if data != 0 {
        for bucket in 0..n {
            let mut at = word(ctx, data + bucket * 4)?;
            while at != 0 {
                pool.validate(at)?;
                if !seen.insert(at) || seen.len() > count as usize {
                    return Err(invalid());
                }
                ctx.cpu.check_guest_access(
                    at,
                    kind.stride(),
                    pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
                )?;
                associations.push(at);
                at = word(ctx, at)?;
            }
        }
    }
    if associations.len() != count as usize {
        return Err(invalid());
    }
    Ok(associations)
}
fn clear(ctx: &mut CallCtx<'_>, o: u32, kind: Kind) -> Result<(), KernelError> {
    let associations = associations(ctx, o, kind)?;
    let data = word(ctx, o + 4)?;
    let first = word(ctx, o + 20)?;
    let blocks = super::lists::plex_blocks(ctx, first)?;
    for at in associations {
        kind.release(ctx, at)?;
    }
    if data != 0 {
        ctx.kernel.heap.free(data);
    }
    for at in blocks {
        ctx.kernel.heap.free(at);
    }
    for offset in [4, 12, 16, 20] {
        put(ctx, o + offset, 0)?;
    }
    Ok(())
}
fn destruct_kind(ctx: &mut CallCtx<'_>, kind: Kind) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    clear(ctx, o, kind)?;
    let table = super::classes::vtable(ctx, "CObject")?;
    put(ctx, o, table)?;
    Ok(DispatchOutcome::ReturnedR0(o))
}
fn remove_all_kind(ctx: &mut CallCtx<'_>, kind: Kind) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    clear(ctx, o, kind)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn init_hash_table(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let n = ctx.arg_u32(1)?;
    let allocate = ctx.arg_u32(2)? != 0;
    let (old, _, count) = header(ctx, o)?;
    if count != 0 || n == 0 || n > MAX {
        return Err(invalid());
    }
    let data = if allocate {
        let p = alloc_cpp(ctx, n * 4)?;
        if let Err(e) = ctx.cpu.write_mem(p, &vec![0; n as usize * 4]) {
            ctx.kernel.heap.free(p);
            return Err(e.into());
        }
        p
    } else {
        0
    };
    put(ctx, o + 4, data)?;
    put(ctx, o + 8, n)?;
    if old != 0 {
        ctx.kernel.heap.free(old);
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn lookup_impl(
    ctx: &mut CallCtx<'_>,
    key: bool,
    kind: Kind,
) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let text = ctx.arg_u32(1)?;
    let out = ctx.arg_u32(2)?;
    let text = kind.argument(ctx, text)?;
    let (at, _, _) = find(ctx, o, &text, kind)?;
    if at == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    if !key && kind.string_value() {
        let text = strings::value(ctx, at + kind.value_offset())?;
        strings::replace(ctx, out, &text)?;
    } else {
        let value = word(ctx, at + if key { 8 } else { kind.value_offset() })?;
        put(ctx, out, value)?;
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn lookup_kind(ctx: &mut CallCtx<'_>, kind: Kind) -> Result<DispatchOutcome, KernelError> {
    lookup_impl(ctx, false, kind)
}
fn lookup_key_kind(ctx: &mut CallCtx<'_>, kind: Kind) -> Result<DispatchOutcome, KernelError> {
    lookup_impl(ctx, true, kind)
}
fn subscript_kind(ctx: &mut CallCtx<'_>, kind: Kind) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let p = ctx.arg_u32(1)?;
    let text = kind.argument(ctx, p)?;
    Ok(DispatchOutcome::ReturnedR0(insert_key(
        ctx, o, &text, kind,
    )?))
}
fn insert_key(ctx: &mut CallCtx<'_>, o: u32, text: &[u16], kind: Kind) -> Result<u32, KernelError> {
    let (at, hash, _) = find(ctx, o, text, kind)?;
    if at != 0 {
        return Ok(at + kind.value_offset());
    }
    let (mut data, n, count) = header(ctx, o)?;
    if count == MAX {
        return Err(invalid());
    }
    if data == 0 {
        data = alloc_cpp(ctx, n * 4)?;
        if let Err(e) = ctx.cpu.write_mem(data, &vec![0; n as usize * 4]) {
            ctx.kernel.heap.free(data);
            return Err(e.into());
        }
        put(ctx, o + 4, data)?;
    }
    let at = allocate_association(ctx, o, kind)?;
    let initialized = if kind.word_key() {
        ctx.cpu
            .write_mem(at + 4, &text[0].to_le_bytes())
            .map_err(Into::into)
    } else {
        strings::replace(ctx, at + 8, text)
    };
    if let Err(error) = initialized {
        recycle_association(ctx, o, at, kind)?;
        return Err(error);
    }
    let first = word(ctx, data + hash * 4)?;
    put(ctx, at, first)?;
    if !kind.word_key() {
        put(ctx, at + 4, hash)?;
    }
    put(ctx, data + hash * 4, at)?;
    Ok(at + kind.value_offset())
}
fn remove_key_kind(ctx: &mut CallCtx<'_>, kind: Kind) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let p = ctx.arg_u32(1)?;
    let text = kind.argument(ctx, p)?;
    let (at, _, link) = find(ctx, o, &text, kind)?;
    if at == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let next = word(ctx, at)?;
    put(ctx, link, next)?;
    recycle_association(ctx, o, at, kind)?;
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn next_assoc_kind(ctx: &mut CallCtx<'_>, kind: Kind) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let pos = ctx.arg_u32(1)?;
    let key = ctx.arg_u32(2)?;
    let value = ctx.arg_u32(3)?;
    ctx.cpu
        .check_guest_access(pos, 4, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    ctx.cpu
        .check_guest_access(value, 4, pocket_cpu::Prot::WRITE)?;
    let (data, n, _) = header(ctx, o)?;
    let mut at = word(ctx, pos)?;
    if data == 0 || at == 0 {
        return Err(invalid());
    }
    if at == u32::MAX {
        at = 0;
        for i in 0..n {
            let p = word(ctx, data + i * 4)?;
            if p != 0 {
                at = p;
                break;
            }
        }
        if at == 0 {
            return Err(invalid());
        }
    }
    if !associations(ctx, o, kind)?.contains(&at) {
        return Err(invalid());
    }
    ctx.cpu.check_guest_access(
        at,
        kind.stride(),
        pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
    )?;
    let mut next = word(ctx, at)?;
    let bucket = kind.hash(&kind.key(ctx, at)?) % n;
    if bucket >= n {
        return Err(invalid());
    }
    if next == 0 {
        for i in bucket + 1..n {
            next = word(ctx, data + i * 4)?;
            if next != 0 {
                break;
            }
        }
    }
    let text = kind.key(ctx, at)?;
    if kind.word_key() {
        ctx.cpu.write_mem(key, &text[0].to_le_bytes())?;
    } else {
        strings::replace(ctx, key, &text)?;
    }
    if kind.string_value() {
        let text = strings::value(ctx, at + kind.value_offset())?;
        strings::replace(ctx, value, &text)?;
    } else {
        let v = word(ctx, at + kind.value_offset())?;
        put(ctx, value, v)?;
    }
    put(ctx, pos, next)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn serialize_kind(ctx: &mut CallCtx<'_>, kind: Kind) -> Result<DispatchOutcome, KernelError> {
    use super::callbacks::{Frame, ScopedString};
    use pocket_cpu::regs::ArmReg;
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let archive = f.arg(ctx, 1)?;
    let loading = super::archive::loading(ctx, archive)?;
    let key = ScopedString::array(&f, 128, 192, if kind.string_value() { 2 } else { 1 });
    if !f.fresh && loading {
        if let Some(outcome) = key.rethrow(ctx, &f)? {
            return Ok(outcome);
        }
    }
    if f.fresh {
        if loading {
            key.begin(ctx, &f)?;
            return f.call_service(ctx, 1, super::archive::READ_COUNT, &[archive]);
        }
        let entries = associations(ctx, object, kind)?;
        let storage = alloc(ctx, (entries.len() as u32 * 4).max(1))?;
        f.own(ctx, storage)?;
        for (i, entry) in entries.iter().enumerate() {
            put(ctx, storage + i as u32 * 4, *entry)?;
        }
        f.set(ctx, 0, entries.len() as u32)?;
        f.set(ctx, 2, storage)?;
        return f.call_service(
            ctx,
            1,
            super::archive::WRITE_COUNT,
            &[archive, entries.len() as u32],
        );
    }
    let phase = f.phase(ctx)?;
    if loading {
        if phase == 1 {
            let count = ctx.cpu.read_reg(ArmReg::R0)?;
            if count > MAX {
                return Err(super::archive::fault(ctx, archive, 5));
            }
            f.set(ctx, 0, count)?;
        }
        if phase == 2 {
            if kind.word_key() {
                let read = ctx.cpu.read_reg(ArmReg::R0)?;
                f.set(ctx, 3, read)?;
            }
            return f.call_service(
                ctx,
                3,
                if kind.string_value() {
                    super::archive::READ_STRING
                } else {
                    super::archive_graph::READ_OBJECT
                },
                &[archive, if kind.string_value() { key.cell + 4 } else { 0 }],
            );
        }
        if phase == 3 {
            let value = ctx.cpu.read_reg(ArmReg::R0)?;
            if kind.word_key() && f.get(ctx, 3)? != 2 {
                return Err(super::archive::fault(ctx, archive, 3));
            }
            let text = if kind.word_key() {
                vec![u16::from_le_bytes(
                    ctx.cpu.read_mem(f.address + 136, 2)?.try_into().unwrap(),
                )]
            } else {
                strings::value(ctx, key.cell)?
            };
            let slot = insert_key(ctx, object, &text, kind)?;
            if kind.string_value() {
                let text = strings::value(ctx, key.cell + 4)?;
                strings::replace(ctx, slot, &text)?;
            } else {
                put(ctx, slot, value)?;
            }
            let n = f.get(ctx, 1)?;
            f.set(ctx, 1, n + 1)?;
        }
        if f.get(ctx, 1)? == f.get(ctx, 0)? {
            key.end(ctx)?;
            return f.finish(ctx, 0);
        }
        return if kind.word_key() {
            f.call_service(ctx, 2, super::archive::READ, &[archive, f.address + 136, 2])
        } else {
            f.call_service(ctx, 2, super::archive::READ_STRING, &[archive, key.cell])
        };
    }
    let mut i = f.get(ctx, 1)?;
    let entries = f.get(ctx, 2)?;
    if phase == 2 {
        let at = word(ctx, entries + i * 4)?;
        let value = if kind.string_value() {
            at + kind.value_offset()
        } else {
            word(ctx, at + kind.value_offset())?
        };
        return f.call_service(
            ctx,
            3,
            if kind.string_value() {
                super::archive::WRITE_STRING
            } else {
                super::archive_graph::WRITE_OBJECT
            },
            &[archive, value],
        );
    }
    if phase == 3 {
        i += 1;
        f.set(ctx, 1, i)?;
    }
    if i == f.get(ctx, 0)? {
        return f.finish(ctx, 0);
    }
    let at = word(ctx, entries + i * 4)?;
    if kind.word_key() {
        f.call_service(ctx, 2, super::archive::WRITE, &[archive, at + 4, 2])
    } else {
        f.call_service(ctx, 2, super::archive::WRITE_STRING, &[archive, at + 8])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn string_keys_are_owned_values_are_borrowed_and_all_blocks_are_released() {
        let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
        let invoke = |cpu: &mut pocket_cpu::stub::StubCpu,
                      k: &mut pocket_kernel::KernelState,
                      d: &mut WinCeDispatcher,
                      n: &str,
                      a: &[u32]| {
            super::super::tests::result(call(cpu, k, d, "mfcce300.dll", n, a))
        };
        invoke(&mut cpu, &mut k, &mut d, "ord:123", &[0x2000, 2]);
        // Metadata is process-owned; establish it before measuring container allocations.
        let baseline = k.heap.free_bytes();
        let text: Vec<_> = "alpha"
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        cpu.write_mem(0x1000, &text).unwrap();
        let slot = invoke(&mut cpu, &mut k, &mut d, "ord:354", &[0x2000, 0x1000]);
        cpu.write_mem(slot, &0x1234abcd_u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x1000, &[b'X', 0]).unwrap();
        assert_eq!(
            invoke(
                &mut cpu,
                &mut k,
                &mut d,
                "ord:1740",
                &[0x2000, 0x1000, 0x2100]
            ),
            0
        );
        cpu.write_mem(0x1000, &text).unwrap();
        assert_eq!(
            invoke(
                &mut cpu,
                &mut k,
                &mut d,
                "ord:1740",
                &[0x2000, 0x1000, 0x2100]
            ),
            1
        );
        assert_eq!(cpu.read_u32_le(0x2100).unwrap(), 0x1234abcd);
        assert_eq!(
            invoke(&mut cpu, &mut k, &mut d, "ord:354", &[0x2000, 0x1000]),
            slot
        );
        invoke(&mut cpu, &mut k, &mut d, "ord:181", &[0x2104]);
        cpu.write_mem(0x2108, &u32::MAX.to_le_bytes()).unwrap();
        invoke(
            &mut cpu,
            &mut k,
            &mut d,
            "ord:1386",
            &[0x2000, 0x2108, 0x2104, 0x2100],
        );
        assert_eq!(cpu.read_u32_le(0x2108).unwrap(), 0);
        let p = cpu.read_u32_le(0x2104).unwrap();
        assert_eq!(cpu.read_mem(p, text.len() as u32).unwrap(), text);
        invoke(&mut cpu, &mut k, &mut d, "ord:297", &[0x2104]);
        assert_eq!(
            invoke(&mut cpu, &mut k, &mut d, "ord:2209", &[0x2000, 0x1000]),
            1
        );
        assert_eq!(cpu.read_u32_le(0x200c).unwrap(), 0);
        assert_eq!(baseline, k.heap.free_bytes());
        // Reusing an emptied map allocates fresh blocks; destruction frees them.
        invoke(&mut cpu, &mut k, &mut d, "ord:354", &[0x2000, 0x1000]);
        invoke(&mut cpu, &mut k, &mut d, "ord:259", &[0x2000]);
        assert_eq!(baseline, k.heap.free_bytes());
    }
}

fn allocate_association(ctx: &mut CallCtx<'_>, o: u32, kind: Kind) -> Result<u32, KernelError> {
    ctx.cpu
        .check_guest_access(o, 28, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let count = word(ctx, o + 12)?;
    if count >= MAX {
        return Err(invalid());
    }
    let mut at = word(ctx, o + 16)?;
    if at == 0 {
        let block = word(ctx, o + 24)?;
        if block == 0 || block > MAX {
            return Err(invalid());
        }
        let p = alloc_cpp(ctx, 4 + block * kind.stride())?;
        let result = (|| {
            ctx.cpu
                .write_mem(p, &vec![0; (4 + block * kind.stride()) as usize])?;
            let old = word(ctx, o + 20)?;
            put(ctx, p, old)?;
            for i in 0..block - 1 {
                put(
                    ctx,
                    p + 4 + i * kind.stride(),
                    p + 4 + (i + 1) * kind.stride(),
                )?;
            }
            Ok::<_, KernelError>(())
        })();
        if let Err(e) = result {
            ctx.kernel.heap.free(p);
            return Err(e);
        }
        put(ctx, o + 20, p)?;
        at = p + 4;
        put(ctx, o + 16, at)?;
    }
    lists::NodePool::with_stride(ctx, o, kind.stride())?.validate(at)?;
    // Initialize the key before detaching the free association, so allocation
    // failure leaves the map's free-list ownership intact.
    if kind.word_key() {
        ctx.cpu.write_mem(at + 4, &0u16.to_le_bytes())?;
    } else {
        new_string(ctx, at + 8, &[])?;
    }
    if kind.string_value() {
        if let Err(error) = new_string(ctx, at + kind.value_offset(), &[]) {
            release_string(ctx, at + 8)?;
            return Err(error);
        }
    }
    let next = word(ctx, at)?;
    put(ctx, o + 16, next)?;
    put(ctx, o + 12, count + 1)?;
    if !kind.string_value() {
        put(ctx, at + kind.value_offset(), 0)?;
    }
    Ok(at)
}
fn objects_get_assoc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    get_assoc(ctx, Kind::Objects)
}
fn objects_new_assoc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    new_assoc(ctx, Kind::Objects)
}
fn objects_free_assoc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    free_assoc(ctx, Kind::Objects)
}
fn strings_get_assoc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    get_assoc(ctx, Kind::Strings)
}
fn strings_new_assoc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    new_assoc(ctx, Kind::Strings)
}
fn strings_free_assoc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    free_assoc(ctx, Kind::Strings)
}
fn words_get_assoc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    get_assoc(ctx, Kind::Words)
}
fn words_new_assoc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    new_assoc(ctx, Kind::Words)
}
fn words_free_assoc(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    free_assoc(ctx, Kind::Words)
}

fn get_assoc(ctx: &mut CallCtx<'_>, kind: Kind) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let key = ctx.arg_u32(1)?;
    let hash_out = ctx.arg_u32(2)?;
    ctx.cpu
        .check_guest_access(hash_out, 4, pocket_cpu::Prot::WRITE)?;
    let text = kind.argument(ctx, key)?;
    let (node, hash, _) = find(ctx, o, &text, kind)?;
    put(ctx, hash_out, hash)?;
    Ok(DispatchOutcome::ReturnedR0(node))
}
fn new_assoc(ctx: &mut CallCtx<'_>, kind: Kind) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    Ok(DispatchOutcome::ReturnedR0(allocate_association(
        ctx, o, kind,
    )?))
}
fn recycle_association(
    ctx: &mut CallCtx<'_>,
    o: u32,
    node: u32,
    kind: Kind,
) -> Result<(), KernelError> {
    let pool = lists::NodePool::with_stride(ctx, o, kind.stride())?;
    pool.validate(node)?;
    let count = word(ctx, o + 12)?;
    if count == 0 || count > MAX {
        return Err(invalid());
    }
    let mut free = word(ctx, o + 16)?;
    let mut seen = HashSet::new();
    while free != 0 {
        pool.validate(free)?;
        if free == node || !seen.insert(free) {
            return Err(invalid());
        }
        free = word(ctx, free)?;
    }
    kind.release(ctx, node)?;
    let free = word(ctx, o + 16)?;
    put(ctx, node, free)?;
    put(ctx, o + 16, node)?;
    put(ctx, o + 12, count - 1)?;
    if count == 1 {
        clear(ctx, o, kind)?;
    }
    Ok(())
}
fn free_assoc(ctx: &mut CallCtx<'_>, kind: Kind) -> Result<DispatchOutcome, KernelError> {
    let o = ctx.arg_u32(0)?;
    let node = ctx.arg_u32(1)?;
    recycle_association(ctx, o, node, kind)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
