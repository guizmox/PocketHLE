//! Serializable CE3 object/string lists share the pointer-list CPlex owner.
//! Object values are borrowed; only active CString nodes own string storage.
use super::callbacks::{Frame, ScopedString};
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (135, "??0CObList@@QAA@H@Z", ob_construct),
    (270, "??1CObList@@UAA@XZ", ob_destruct),
    (2181, "?RemoveAll@CObList@@QAAXXZ", ob_clear),
    (
        576,
        "?AddHead@CObList@@QAAPAU__POSITION@@PAVCObject@@@Z",
        ob_head,
    ),
    (
        592,
        "?AddTail@CObList@@QAAPAU__POSITION@@PAVCObject@@@Z",
        ob_tail,
    ),
    (577, "?AddHead@CObList@@QAAXPAV1@@Z", ob_head_list),
    (593, "?AddTail@CObList@@QAAXPAV1@@Z", ob_tail_list),
    (
        1154,
        "?Find@CObList@@QBAPAU__POSITION@@PAVCObject@@PAU2@@Z",
        ob_find,
    ),
    (
        1161,
        "?FindIndex@CObList@@QBAPAU__POSITION@@H@Z",
        ob_find_index,
    ),
    (
        1643,
        "?InsertAfter@CObList@@QAAPAU__POSITION@@PAU2@PAVCObject@@@Z",
        ob_after,
    ),
    (
        1662,
        "?InsertBefore@CObList@@QAAPAU__POSITION@@PAU2@PAVCObject@@@Z",
        ob_before,
    ),
    (2188, "?RemoveAt@CObList@@QAAXPAU__POSITION@@@Z", ob_remove),
    (2200, "?RemoveHead@CObList@@QAAPAVCObject@@XZ", ob_pop_head),
    (2216, "?RemoveTail@CObList@@QAAPAVCObject@@XZ", ob_pop_tail),
    (
        2291,
        "?Serialize@CObList@@UAAXAAVCArchive@@@Z",
        ob_serialize,
    ),
    (
        1778,
        "?NewNode@CObList@@IAAPAUCNode@1@PAU21@0@Z",
        ob_new_node,
    ),
    (1196, "?FreeNode@CObList@@IAAXPAUCNode@1@@Z", ob_free_node),
    (183, "??0CStringList@@QAA@H@Z", string_construct),
    (299, "??1CStringList@@UAA@XZ", string_destruct),
    (2183, "?RemoveAll@CStringList@@QAAXXZ", string_clear),
    (
        581,
        "?AddHead@CStringList@@QAAPAU__POSITION@@ABVCString@@@Z",
        string_head,
    ),
    (
        582,
        "?AddHead@CStringList@@QAAPAU__POSITION@@PBG@Z",
        wide_head,
    ),
    (
        596,
        "?AddTail@CStringList@@QAAPAU__POSITION@@ABVCString@@@Z",
        string_tail,
    ),
    (
        597,
        "?AddTail@CStringList@@QAAPAU__POSITION@@PBG@Z",
        wide_tail,
    ),
    (583, "?AddHead@CStringList@@QAAXPAV1@@Z", string_head_list),
    (598, "?AddTail@CStringList@@QAAXPAV1@@Z", string_tail_list),
    (
        1160,
        "?Find@CStringList@@QBAPAU__POSITION@@PBGPAU2@@Z",
        string_find,
    ),
    (
        1163,
        "?FindIndex@CStringList@@QBAPAU__POSITION@@H@Z",
        string_find_index,
    ),
    (
        1645,
        "?InsertAfter@CStringList@@QAAPAU__POSITION@@PAU2@ABVCString@@@Z",
        string_after,
    ),
    (
        1646,
        "?InsertAfter@CStringList@@QAAPAU__POSITION@@PAU2@PBG@Z",
        wide_after,
    ),
    (
        1664,
        "?InsertBefore@CStringList@@QAAPAU__POSITION@@PAU2@ABVCString@@@Z",
        string_before,
    ),
    (
        1665,
        "?InsertBefore@CStringList@@QAAPAU__POSITION@@PAU2@PBG@Z",
        wide_before,
    ),
    (
        2192,
        "?RemoveAt@CStringList@@QAAXPAU__POSITION@@@Z",
        string_remove,
    ),
    (
        2202,
        "?RemoveHead@CStringList@@QAA?AVCString@@XZ",
        string_pop_head,
    ),
    (
        2218,
        "?RemoveTail@CStringList@@QAA?AVCString@@XZ",
        string_pop_tail,
    ),
    (
        2293,
        "?Serialize@CStringList@@UAAXAAVCArchive@@@Z",
        string_serialize,
    ),
    (
        1780,
        "?NewNode@CStringList@@IAAPAUCNode@1@PAU21@0@Z",
        string_new_node,
    ),
    (
        1198,
        "?FreeNode@CStringList@@IAAXPAUCNode@1@@Z",
        string_free_node,
    ),
];
fn ob_construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct(ctx, false)
}
fn ob_destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct(ctx, false)
}
fn ob_clear(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    clear_call(ctx, false)
}
fn ob_head(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    add(ctx, false, false, true)
}
fn ob_tail(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    add(ctx, false, false, false)
}
fn ob_head_list(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    add_list(ctx, false, true)
}
fn ob_tail_list(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    add_list(ctx, false, false)
}
fn ob_find(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    find(ctx, false)
}
fn ob_find_index(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    find_index(ctx)
}
fn ob_after(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    insert(ctx, false, false, false)
}
fn ob_before(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    insert(ctx, false, false, true)
}
fn ob_remove(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    remove_at(ctx, false)
}
fn ob_pop_head(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    pop(ctx, false, true)
}
fn ob_pop_tail(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    pop(ctx, false, false)
}
fn ob_serialize(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    serialize(ctx, false)
}
fn ob_new_node(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    new_node(ctx, false)
}
fn ob_free_node(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    free_node(ctx, false)
}
fn string_construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    construct(ctx, true)
}
fn string_destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct(ctx, true)
}
fn string_clear(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    clear_call(ctx, true)
}
fn string_head(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    add(ctx, true, false, true)
}
fn wide_head(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    add(ctx, true, true, true)
}
fn string_tail(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    add(ctx, true, false, false)
}
fn wide_tail(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    add(ctx, true, true, false)
}
fn string_head_list(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    add_list(ctx, true, true)
}
fn string_tail_list(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    add_list(ctx, true, false)
}
fn string_find(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    find(ctx, true)
}
fn string_find_index(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    find_index(ctx)
}
fn string_after(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    insert(ctx, true, false, false)
}
fn wide_after(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    insert(ctx, true, true, false)
}
fn string_before(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    insert(ctx, true, false, true)
}
fn wide_before(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    insert(ctx, true, true, true)
}
fn string_remove(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    remove_at(ctx, true)
}
fn string_pop_head(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    pop(ctx, true, true)
}
fn string_pop_tail(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    pop(ctx, true, false)
}
fn string_serialize(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    serialize(ctx, true)
}
fn string_new_node(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    new_node(ctx, true)
}
fn string_free_node(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    free_node(ctx, true)
}

fn construct(ctx: &mut CallCtx<'_>, string: bool) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let block = ctx.arg_u32(1)?;
    let table = classes::vtable(ctx, if string { "CStringList" } else { "CObList" })?;
    lists::initialize(ctx, object, block)?;
    put(ctx, object, table)?;
    Ok(DispatchOutcome::ReturnedR0(object))
}
fn clear(ctx: &mut CallCtx<'_>, object: u32, string: bool) -> Result<(), KernelError> {
    let nodes = lists::positions(ctx, object)?;
    if string {
        for node in nodes {
            release_string(ctx, node + 8)?;
        }
    }
    lists::clear(ctx, object)
}
fn clear_call(ctx: &mut CallCtx<'_>, string: bool) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    clear(ctx, object, string)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn destruct(ctx: &mut CallCtx<'_>, string: bool) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    clear(ctx, object, string)?;
    let table = classes::vtable(ctx, "CObject")?;
    put(ctx, object, table)?;
    Ok(DispatchOutcome::ReturnedR0(object))
}
fn value(
    ctx: &mut CallCtx<'_>,
    arg: u32,
    string: bool,
    wide: bool,
) -> Result<Vec<u16>, KernelError> {
    if !string {
        return Ok(Vec::new());
    }
    if wide {
        strings::wide(ctx, arg)
    } else {
        strings::value(ctx, arg)
    }
}
fn insert_value(
    ctx: &mut CallCtx<'_>,
    object: u32,
    before: u32,
    value: u32,
    text: Option<&[u16]>,
) -> Result<u32, KernelError> {
    let positions = lists::positions(ctx, object)?;
    if before != 0 && !positions.contains(&before) {
        return Err(KernelError::Dispatch(
            "MFC insertion POSITION is not active".into(),
        ));
    }
    // Allocate owned CString first. Node allocation failure releases it and
    // leaves the old active chain untouched.
    let temporary = if let Some(text) = text {
        let cell = alloc(ctx, 4)?;
        if let Err(error) = new_string(ctx, cell, text) {
            ctx.kernel.heap.free(cell);
            return Err(error);
        }
        Some(cell)
    } else {
        None
    };
    let appended = lists::append(ctx, object, value);
    let node = match appended {
        Ok(node) => node,
        Err(error) => {
            if let Some(cell) = temporary {
                release_string(ctx, cell)?;
                ctx.kernel.heap.free(cell);
            }
            return Err(error);
        }
    };
    if let Some(cell) = temporary {
        let pointer = word(ctx, cell)?;
        put(ctx, node + 8, pointer)?; // ownership transfer, not a second reference
        ctx.kernel.heap.free(cell);
    }
    if before != 0 {
        let previous = word(ctx, before + 4)?;
        let old_tail = word(ctx, node + 4)?;
        put(ctx, old_tail, 0)?;
        put(ctx, object + 8, old_tail)?;
        put(ctx, node, before)?;
        put(ctx, node + 4, previous)?;
        put(ctx, before + 4, node)?;
        put(ctx, if previous == 0 { object + 4 } else { previous }, node)?;
    }
    Ok(node)
}
fn add(
    ctx: &mut CallCtx<'_>,
    string: bool,
    wide: bool,
    head: bool,
) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let arg = ctx.arg_u32(1)?;
    let text = value(ctx, arg, string, wide)?;
    let before = if head { word(ctx, object + 4)? } else { 0 };
    Ok(DispatchOutcome::ReturnedR0(insert_value(
        ctx,
        object,
        before,
        arg,
        string.then_some(text.as_slice()),
    )?))
}
fn add_list(
    ctx: &mut CallCtx<'_>,
    string: bool,
    head: bool,
) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let source = ctx.arg_u32(1)?;
    let mut values = Vec::new();
    for node in lists::positions(ctx, source)? {
        let v = word(ctx, node + 8)?;
        values.push((
            v,
            if string {
                strings::value(ctx, node + 8)?
            } else {
                Vec::new()
            },
        ));
    }
    let before = if head { word(ctx, object + 4)? } else { 0 };
    for (v, text) in values {
        insert_value(ctx, object, before, v, string.then_some(text.as_slice()))?;
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn insert(
    ctx: &mut CallCtx<'_>,
    string: bool,
    wide: bool,
    before: bool,
) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let at = ctx.arg_u32(1)?;
    let arg = ctx.arg_u32(2)?;
    let nodes = lists::positions(ctx, object)?;
    if at != 0 && !nodes.contains(&at) {
        return Err(KernelError::Dispatch(
            "Inactive MFC insertion POSITION".into(),
        ));
    }
    let next = if at == 0 {
        if before {
            word(ctx, object + 4)?
        } else {
            0
        }
    } else if before {
        at
    } else {
        word(ctx, at)?
    };
    let text = value(ctx, arg, string, wide)?;
    Ok(DispatchOutcome::ReturnedR0(insert_value(
        ctx,
        object,
        next,
        arg,
        string.then_some(text.as_slice()),
    )?))
}
fn remove(ctx: &mut CallCtx<'_>, object: u32, node: u32, string: bool) -> Result<(), KernelError> {
    if !lists::positions(ctx, object)?.contains(&node) {
        return Err(KernelError::Dispatch(
            "Inactive MFC removal POSITION".into(),
        ));
    }
    if string {
        release_string(ctx, node + 8)?;
    }
    lists::remove(ctx, object, node)
}
fn remove_at(ctx: &mut CallCtx<'_>, string: bool) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let node = ctx.arg_u32(1)?;
    remove(ctx, object, node, string)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn pop(ctx: &mut CallCtx<'_>, string: bool, head: bool) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let nodes = lists::positions(ctx, object)?;
    let node = if head { nodes.first() } else { nodes.last() }
        .copied()
        .ok_or_else(|| KernelError::Dispatch("Removing from an empty MFC list".into()))?;
    let output = if string {
        let output = ctx.arg_u32(1)?; // CE member hidden CString return argument
        let text = strings::value(ctx, node + 8)?;
        new_string(ctx, output, &text)?;
        output
    } else {
        word(ctx, node + 8)?
    };
    remove(ctx, object, node, string)?;
    Ok(DispatchOutcome::ReturnedR0(output))
}
fn find(ctx: &mut CallCtx<'_>, string: bool) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let arg = ctx.arg_u32(1)?;
    let after = ctx.arg_u32(2)?;
    let nodes = lists::positions(ctx, object)?;
    let start = if after == 0 {
        0
    } else {
        nodes
            .iter()
            .position(|n| *n == after)
            .ok_or_else(|| KernelError::Dispatch("Inactive MFC search POSITION".into()))?
            + 1
    };
    // CStringList::Find receives LPCTSTR, unlike its CString insertion overload.
    let text = if string {
        strings::wide(ctx, arg)?
    } else {
        Vec::new()
    };
    for &node in &nodes[start..] {
        if if string {
            strings::value(ctx, node + 8)? == text
        } else {
            word(ctx, node + 8)? == arg
        } {
            return Ok(DispatchOutcome::ReturnedR0(node));
        }
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn find_index(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)?;
    let nodes = lists::positions(ctx, object)?;
    Ok(DispatchOutcome::ReturnedR0(
        nodes.get(index as usize).copied().unwrap_or(0),
    ))
}
fn new_node(ctx: &mut CallCtx<'_>, string: bool) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let previous = ctx.arg_u32(1)?;
    let next = ctx.arg_u32(2)?;
    let node = lists::new_node(ctx, object, previous, next)?;
    if string {
        if let Err(error) = new_string(ctx, node + 8, &[]) {
            lists::recycle_node(ctx, object, node, false)?;
            return Err(error);
        }
    }
    Ok(DispatchOutcome::ReturnedR0(node))
}
fn free_node(ctx: &mut CallCtx<'_>, string: bool) -> Result<DispatchOutcome, KernelError> {
    // Native protected FreeNode only recycles storage; its caller has already
    // unlinked the node. Validate its CPlex membership without traversing it.
    let object = ctx.arg_u32(0)?;
    let node = ctx.arg_u32(1)?;
    lists::recycle_node(ctx, object, node, string)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn serialize(ctx: &mut CallCtx<'_>, string: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let archive = f.arg(ctx, 1)?;
    let loading = archive::loading(ctx, archive)?;
    let temporary = ScopedString::at(&f, 128, 192);
    if loading && string && !f.fresh {
        if let Some(outcome) = temporary.rethrow(ctx, &f)? {
            return Ok(outcome);
        }
    }
    if f.fresh {
        if loading {
            if string {
                temporary.begin(ctx, &f)?;
            }
            return f.call_service(ctx, 1, archive::READ_COUNT, &[archive]);
        }
        let nodes = lists::positions(ctx, object)?;
        f.set(ctx, 0, nodes.len() as u32)?;
        f.set(ctx, 2, nodes.first().copied().unwrap_or(0))?;
        return f.call_service(ctx, 1, archive::WRITE_COUNT, &[archive, nodes.len() as u32]);
    }
    let phase = f.phase(ctx)?;
    if phase == 1 && loading {
        let count = ctx.cpu.read_reg(ArmReg::R0)?;
        if count > 0x100000 {
            return Err(archive::fault(ctx, archive, 5));
        }
        f.set(ctx, 0, count)?;
    }
    let mut index = f.get(ctx, 1)?;
    if phase == 2 {
        if loading {
            if string {
                let text = strings::value(ctx, temporary.cell)?;
                insert_value(ctx, object, 0, 0, Some(&text))?;
            } else {
                let value = ctx.cpu.read_reg(ArmReg::R0)?;
                lists::append(ctx, object, value)?;
            }
        }
        index += 1;
        f.set(ctx, 1, index)?;
    }
    if index == f.get(ctx, 0)? {
        if loading && string {
            temporary.end(ctx)?;
        }
        return f.finish(ctx, 0);
    }
    let (service, value) = if loading {
        if string {
            (archive::READ_STRING, temporary.cell)
        } else {
            (archive_graph::READ_OBJECT, 0)
        }
    } else {
        let node = f.get(ctx, 2)?;
        // Guest serialization overrides can mutate the list across callbacks.
        if !lists::positions(ctx, object)?.contains(&node) {
            return Err(KernelError::Dispatch(
                "MFC list changed during serialization".into(),
            ));
        }
        let next = word(ctx, node)?;
        f.set(ctx, 2, next)?;
        if string {
            (archive::WRITE_STRING, node + 8)
        } else {
            (archive_graph::WRITE_OBJECT, word(ctx, node + 8)?)
        }
    };
    f.call_service(ctx, 2, service, &[archive, value])
}
