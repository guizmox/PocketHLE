//! CE pointer-list storage owns CPlex blocks and borrows the stored pointers.
use super::*;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (918, "?Create@CPlex@@SAPAU1@AAPAU1@II@Z", create_plex),
    (1187, "?FreeDataChain@CPlex@@QAAXXZ", free_data_chain),
    (594, "?AddTail@CPtrList@@QAAPAU__POSITION@@PAX@Z", add_tail),
    (1155, "?Find@CPtrList@@QBAPAU__POSITION@@PAXPAU2@@Z", find),
    (2190, "?RemoveAt@CPtrList@@QAAXPAU__POSITION@@@Z", remove_at),
    (153, "??0CPtrList@@QAA@H@Z", construct),
    (280, "??1CPtrList@@UAA@XZ", destruct),
    (2182, "?RemoveAll@CPtrList@@QAAXXZ", remove_all),
    (
        1511,
        "?GetRuntimeClass@CPtrList@@UBAPAUCRuntimeClass@@XZ",
        runtime,
    ),
];
fn create_plex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let head = ctx.arg_u32(0)?;
    let count = ctx.arg_u32(1)?;
    let stride = ctx.arg_u32(2)?;
    ctx.cpu
        .check_guest_access(head, 4, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let old = word(ctx, head)?;
    let bytes = count
        .checked_mul(stride)
        .and_then(|n| n.checked_add(4))
        .filter(|&n| n <= i32::MAX as u32)
        .ok_or_else(|| KernelError::Dispatch("MFC CPlex allocation size overflow".into()))?;
    let block = alloc_cpp(ctx, bytes)?;
    if let Err(e) = put(ctx, block, old).and_then(|()| put(ctx, head, block)) {
        ctx.kernel.heap.free(block);
        return Err(e);
    }
    Ok(DispatchOutcome::ReturnedR0(block))
}
fn free_data_chain(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let first = ctx.arg_u32(0)?;
    free_plex(ctx, first)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn free_plex(ctx: &mut CallCtx<'_>, first: u32) -> Result<(), KernelError> {
    // Validate the entire live chain before releasing any block. This also
    // handles native CPlex::FreeDataChain's valid null-this call.
    for block in plex_blocks(ctx, first)? {
        ctx.kernel.heap.free(block);
    }
    Ok(())
}
pub(super) fn initialize(
    ctx: &mut CallCtx<'_>,
    object: u32,
    block: u32,
) -> Result<(), KernelError> {
    ctx.cpu
        .check_guest_access(object, 28, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    if block == 0 || block > 0x100000 {
        return Err(KernelError::Dispatch(
            "Invalid MFC pointer-list block size".into(),
        ));
    }
    let table = super::classes::vtable(ctx, "CPtrList")?;
    ctx.cpu.write_mem(object, &[0; 28])?;
    put(ctx, object, table)?;
    put(ctx, object + 24, block)
}
/// Transient validation index over guest-owned CPlex blocks. It is never
/// cached or serialized; the list remains the sole persistent storage owner.
pub(super) struct NodePool {
    starts: Vec<u32>,
    block_bytes: u32,
    capacity: u64,
    stride: u32,
}
impl NodePool {
    fn read(ctx: &mut CallCtx<'_>, list: u32) -> Result<Self, KernelError> {
        Self::with_stride(ctx, list, 12)
    }
    pub(super) fn with_stride(
        ctx: &mut CallCtx<'_>,
        list: u32,
        stride: u32,
    ) -> Result<Self, KernelError> {
        let block_size = word(ctx, list + 24)?;
        if block_size == 0 || block_size > 0x100000 {
            return Err(KernelError::Dispatch(
                "Invalid MFC pointer-list block size".into(),
            ));
        }
        let first = word(ctx, list + 20)?;
        let blocks = plex_blocks(ctx, first)?;
        let capacity = blocks.len() as u64 * u64::from(block_size);
        let mut starts = Vec::with_capacity(blocks.len());
        for block in blocks {
            if ctx
                .kernel
                .heap
                .msize(block)
                .is_none_or(|size| size < 4 + block_size * stride)
            {
                return Err(KernelError::Dispatch(
                    "MFC pointer-list block exceeds its live allocation".into(),
                ));
            }
            let start = block
                .checked_add(4)
                .filter(|&p| p.checked_add(block_size * stride).is_some())
                .ok_or_else(|| KernelError::Dispatch("MFC list address overflow".into()))?;
            starts.push(start);
        }
        starts.sort_unstable();
        Ok(Self {
            starts,
            block_bytes: block_size * stride,
            capacity,
            stride,
        })
    }
    pub(super) fn validate(&self, node: u32) -> Result<(), KernelError> {
        if let Some(&start) =
            self.starts[..self.starts.partition_point(|&start| start <= node)].last()
        {
            let offset = node - start;
            if offset < self.block_bytes && offset % self.stride == 0 {
                return Ok(());
            }
        }
        Err(KernelError::Dispatch(
            "MFC POSITION does not belong to its pointer list".into(),
        ))
    }
}
fn validate_node(ctx: &mut CallCtx<'_>, list: u32, node: u32) -> Result<(), KernelError> {
    NodePool::read(ctx, list)?.validate(node)
}

pub(super) fn new_node(
    ctx: &mut CallCtx<'_>,
    list: u32,
    previous: u32,
    next: u32,
) -> Result<u32, KernelError> {
    ctx.cpu
        .check_guest_access(list, 28, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let count = word(ctx, list + 12)?
        .checked_add(1)
        .filter(|&n| n <= i32::MAX as u32)
        .ok_or_else(|| KernelError::Dispatch("MFC pointer-list count overflow".into()))?;
    let pool = NodePool::read(ctx, list)?;
    for link in [previous, next] {
        if link != 0 {
            pool.validate(link)?;
        }
    }
    let mut node = word(ctx, list + 16)?;
    if node == 0 {
        let size = word(ctx, list + 24)?;
        if size == 0 || size > 0x100000 {
            return Err(KernelError::Dispatch(
                "Invalid MFC pointer-list block size".into(),
            ));
        }
        let block = alloc_cpp(ctx, 4 + size * 12)?;
        let result = (|| {
            let old = word(ctx, list + 20)?;
            put(ctx, block, old)?;
            for i in 0..size {
                put(
                    ctx,
                    block + 4 + i * 12,
                    if i + 1 < size {
                        block + 4 + (i + 1) * 12
                    } else {
                        0
                    },
                )?;
            }
            Ok::<_, KernelError>(())
        })();
        if let Err(e) = result {
            ctx.kernel.heap.free(block);
            return Err(e);
        }
        put(ctx, list + 20, block)?;
        node = block + 4;
    } else {
        validate_node(ctx, list, node)?;
    }
    let free = word(ctx, node)?;
    ctx.cpu
        .check_guest_access(node, 12, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    put(ctx, node, next)?;
    put(ctx, node + 4, previous)?;
    put(ctx, node + 8, 0)?;
    put(ctx, list + 16, free)?;
    put(ctx, list + 12, count)?;
    Ok(node)
}
pub(super) fn append(ctx: &mut CallCtx<'_>, list: u32, value: u32) -> Result<u32, KernelError> {
    let tail = word(ctx, list + 8)?;
    let node = new_node(ctx, list, tail, 0)?;
    put(ctx, node + 8, value)?;
    put(ctx, if tail == 0 { list + 4 } else { tail }, node)?;
    put(ctx, list + 8, node)?;
    Ok(node)
}
/// Validate the complete intrusive chain before callers release owned values.
pub(super) fn positions(ctx: &mut CallCtx<'_>, list: u32) -> Result<Vec<u32>, KernelError> {
    let pool = NodePool::read(ctx, list)?;
    let count = word(ctx, list + 12)?;
    if u64::from(count) > pool.capacity {
        return Err(KernelError::Dispatch("Invalid MFC list count".into()));
    }
    let mut nodes = Vec::with_capacity(count as usize);
    let mut at = word(ctx, list + 4)?;
    let mut previous = 0;
    while at != 0 {
        pool.validate(at)?;
        if nodes.len() >= count as usize || word(ctx, at + 4)? != previous {
            return Err(KernelError::Dispatch("Inconsistent MFC list links".into()));
        }
        nodes.push(at);
        previous = at;
        at = word(ctx, at)?;
    }
    if nodes.len() != count as usize || word(ctx, list + 8)? != previous {
        return Err(KernelError::Dispatch(
            "Inconsistent MFC list endpoints".into(),
        ));
    }
    Ok(nodes)
}
fn add_tail(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let list = ctx.arg_u32(0)?;
    let value = ctx.arg_u32(1)?;
    Ok(DispatchOutcome::ReturnedR0(append(ctx, list, value)?))
}
/// Validate a POSITION at each callback boundary: guest overrides may mutate
/// the list while the caller is suspended. No unvalidated link becomes an address.
pub(super) fn next_position(
    ctx: &mut CallCtx<'_>,
    list: u32,
    after: u32,
) -> Result<u32, KernelError> {
    let pool = NodePool::read(ctx, list)?;
    let next = if after == 0 {
        word(ctx, list + 4)?
    } else {
        pool.validate(after)?;
        word(ctx, after)?
    };
    if next != 0 {
        pool.validate(next)?;
    }
    Ok(next)
}
pub(super) fn search(
    ctx: &mut CallCtx<'_>,
    list: u32,
    value: u32,
    after: u32,
) -> Result<u32, KernelError> {
    let pool = NodePool::read(ctx, list)?;
    let mut at = if after == 0 {
        word(ctx, list + 4)?
    } else {
        pool.validate(after)?;
        word(ctx, after)?
    };
    let mut remaining = word(ctx, list + 12)?;
    if u64::from(remaining) > pool.capacity {
        return Err(KernelError::Dispatch(
            "Invalid MFC pointer-list count".into(),
        ));
    }
    while at != 0 {
        if remaining == 0 {
            return Err(KernelError::Dispatch(
                "Cyclic or inconsistent MFC pointer list".into(),
            ));
        }
        remaining -= 1;
        pool.validate(at)?;
        if word(ctx, at + 8)? == value {
            return Ok(at);
        }
        at = word(ctx, at)?;
    }
    Ok(0)
}
fn find(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let list = ctx.arg_u32(0)?;
    let value = ctx.arg_u32(1)?;
    let after = ctx.arg_u32(2)?;
    Ok(DispatchOutcome::ReturnedR0(search(
        ctx, list, value, after,
    )?))
}
pub(super) fn remove(ctx: &mut CallCtx<'_>, list: u32, node: u32) -> Result<(), KernelError> {
    ctx.cpu
        .check_guest_access(list, 28, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let pool = NodePool::read(ctx, list)?;
    pool.validate(node)?;
    let count = word(ctx, list + 12)?;
    let previous = word(ctx, node + 4)?;
    let next = word(ctx, node)?;
    if count == 0 {
        return Err(KernelError::Dispatch(
            "Removing from an empty MFC pointer list".into(),
        ));
    }
    if previous == 0 {
        if word(ctx, list + 4)? != node {
            return Err(KernelError::Dispatch("Invalid MFC list head".into()));
        }
    } else {
        pool.validate(previous)?;
        if word(ctx, previous)? != node {
            return Err(KernelError::Dispatch("Invalid MFC list predecessor".into()));
        }
    }
    if next == 0 {
        if word(ctx, list + 8)? != node {
            return Err(KernelError::Dispatch("Invalid MFC list tail".into()));
        }
    } else {
        pool.validate(next)?;
        if word(ctx, next + 4)? != node {
            return Err(KernelError::Dispatch("Invalid MFC list successor".into()));
        }
    }
    for at in [
        node,
        if previous == 0 { list + 4 } else { previous },
        if next == 0 { list + 8 } else { next + 4 },
    ] {
        ctx.cpu.check_guest_access(at, 4, pocket_cpu::Prot::WRITE)?;
    }
    if count == 1 {
        return clear(ctx, list);
    }
    put(ctx, if previous == 0 { list + 4 } else { previous }, next)?;
    put(ctx, if next == 0 { list + 8 } else { next + 4 }, previous)?;
    let free = word(ctx, list + 16)?;
    put(ctx, node, free)?;
    put(ctx, list + 16, node)?;
    put(ctx, list + 12, count - 1)
}
fn remove_at(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let list = ctx.arg_u32(0)?;
    let node = ctx.arg_u32(1)?;
    remove(ctx, list, node)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let block = ctx.arg_u32(1)?;
    initialize(ctx, object, block)?;
    Ok(DispatchOutcome::ReturnedR0(object))
}
pub(super) fn plex_blocks(ctx: &mut CallCtx<'_>, first: u32) -> Result<Vec<u32>, KernelError> {
    let mut at = first;
    let mut seen = std::collections::HashSet::new();
    let mut blocks = Vec::new();
    while at != 0 {
        if !seen.insert(at) || seen.len() > 0x100000 {
            return Err(KernelError::Dispatch(
                "Invalid MFC pointer-list block chain".into(),
            ));
        }
        if ctx.kernel.heap.msize(at).is_none_or(|size| size < 4) {
            return Err(KernelError::Dispatch(
                "MFC CPlex chain references a non-owned allocation".into(),
            ));
        }
        ctx.cpu.check_guest_access(at, 4, pocket_cpu::Prot::READ)?;
        blocks.push(at);
        at = word(ctx, at)?;
    }
    Ok(blocks)
}
pub(super) fn clear(ctx: &mut CallCtx<'_>, object: u32) -> Result<(), KernelError> {
    ctx.cpu
        .check_guest_access(object, 28, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
    let first = word(ctx, object + 20)?;
    free_plex(ctx, first)?;
    ctx.cpu.write_mem(object + 4, &[0; 20])?;
    Ok(())
}
fn remove_all(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    clear(ctx, object)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    clear(ctx, object)?;
    let table = super::classes::vtable(ctx, "CObject")?;
    put(ctx, object, table)?;
    Ok(DispatchOutcome::ReturnedR0(object))
}
fn runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx, "CPtrList",
    )?))
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn invalid_plex_chains_and_overflow_do_not_partially_free_or_publish_storage() {
        use super::super::window_dispatch::tests::entry;
        use pocket_kernel::Dispatcher;
        let (mut cpu, mut k, mut d) = framework_setup();
        cpu.write_mem(0x2000, &[0; 4]).unwrap();
        let first = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:918",
            &[0x2000, 2, 8],
        ));
        let second = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:918",
            &[0x2000, 2, 8],
        ));
        let before = k.heap.free_bytes();
        cpu.write_mem(first, &second.to_le_bytes()).unwrap();
        cpu.write_arg(0, second).unwrap();
        assert!(d.dispatch(&mut cpu, &entry(1187), &mut k).is_err());
        assert_eq!(k.heap.free_bytes(), before);
        assert!(k.heap.msize(first).is_some() && k.heap.msize(second).is_some());
        for (i, value) in [0x2000, u32::MAX, 2].into_iter().enumerate() {
            cpu.write_arg(i as u8, value).unwrap();
        }
        assert!(d.dispatch(&mut cpu, &entry(918), &mut k).is_err());
        assert_eq!(cpu.read_u32_le(0x2000).unwrap(), second);
        assert_eq!(k.heap.free_bytes(), before);
        cpu.write_mem(first, &[0; 4]).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1187",
            &[second],
        );
        assert!(k.heap.msize(first).is_none() && k.heap.msize(second).is_none());
    }
    #[test]
    fn exported_plex_allocation_links_native_headers_and_frees_every_owned_block() {
        let (mut cpu, mut k, mut d) = framework_setup();
        cpu.write_mem(0x2000, &[0; 4]).unwrap();
        let before = k.heap.free_bytes();
        let mut previous = 0;
        let mut blocks = Vec::new();
        for (count, stride) in [(3, 12), (5, 8), (0, 16)] {
            let block = result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:918",
                &[0x2000, count, stride],
            ));
            assert_eq!(k.heap.msize(block), Some(4 + count * stride));
            assert_eq!(cpu.read_u32_le(block).unwrap(), previous);
            assert_eq!(cpu.read_u32_le(0x2000).unwrap(), block);
            blocks.push(block);
            previous = block;
        }
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1187",
            &[previous],
        );
        assert!(blocks.iter().all(|&p| k.heap.msize(p).is_none()));
        assert_eq!(k.heap.free_bytes(), before);
        call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:1187", &[0]);
        assert_eq!(k.heap.free_bytes(), before);
    }
    #[test]
    fn pointer_positions_preserve_order_reuse_nodes_and_free_only_owned_blocks() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:153",
            &[0x2000, 2],
        );
        let borrowed = k.heap.alloc_cpp(4).unwrap();
        cpu.write_mem(borrowed, &0x12345678u32.to_le_bytes())
            .unwrap();
        let free = k.heap.free_bytes();
        let mut nodes = Vec::new();
        for value in [borrowed, 22, borrowed] {
            nodes.push(result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:594",
                &[0x2000, value],
            )));
        }
        assert_eq!(cpu.read_u32_le(0x200c).unwrap(), 3);
        assert_eq!(cpu.read_u32_le(0x2004).unwrap(), nodes[0]);
        assert_eq!(cpu.read_u32_le(0x2008).unwrap(), nodes[2]);
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1155",
                &[0x2000, borrowed, 0]
            )),
            nodes[0]
        );
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1155",
                &[0x2000, borrowed, nodes[0]]
            )),
            nodes[2]
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2190",
            &[0x2000, nodes[1]],
        );
        let reused = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:594",
            &[0x2000, 44],
        ));
        assert_eq!(reused, nodes[1]);
        assert_eq!(cpu.read_u32_le(nodes[2]).unwrap(), reused);
        for node in [nodes[0], reused, nodes[2]] {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:2190",
                &[0x2000, node],
            );
        }
        assert_eq!(k.heap.free_bytes(), free);
        assert_eq!(cpu.read_u32_le(borrowed).unwrap(), 0x12345678);
        assert_eq!(cpu.read_mem(0x2004, 20).unwrap(), [0; 20]);
        // The list remains usable after the last removal frees every CPlex.
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:594",
            &[0x2000, borrowed],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2182",
            &[0x2000],
        );
        assert_eq!(k.heap.free_bytes(), free);
    }
    #[test]
    fn foreign_list_position_is_rejected_before_mutating_links_or_freeing_memory() {
        use pocket_kernel::Dispatcher;
        let (mut cpu, mut k, mut d) = framework_setup();
        for list in [0x2000, 0x2200] {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:153",
                &[list, 2],
            );
        }
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:594",
            &[0x2000, 1],
        );
        let foreign = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:594",
            &[0x2200, 2],
        ));
        let before = cpu.read_mem(0x2000, 28).unwrap();
        let free = k.heap.free_bytes();
        cpu.write_arg(0, 0x2000).unwrap();
        cpu.write_arg(1, foreign).unwrap();
        let error = d
            .dispatch(
                &mut cpu,
                &super::super::window_dispatch::tests::entry(2190),
                &mut k,
            )
            .unwrap_err();
        assert!(error.to_string().contains("does not belong"));
        assert_eq!(cpu.read_mem(0x2000, 28).unwrap(), before);
        assert_eq!(k.heap.free_bytes(), free);
    }
    #[test]
    fn clearing_pointer_list_frees_all_plex_blocks_but_never_the_borrowed_values() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:153",
            &[0x2000, 4],
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:280",
            &[0x2000],
        );
        let baseline = k.heap.free_bytes();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:153",
            &[0x2000, 4],
        );
        let mut allocate = |size| {
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "??2@YAPAXI@Z",
                &[size],
            ))
        };
        let first = allocate(16);
        let second = allocate(16);
        let value = allocate(4);
        cpu.write_mem(first, &second.to_le_bytes()).unwrap();
        cpu.write_mem(second, &0u32.to_le_bytes()).unwrap();
        cpu.write_mem(first + 12, &value.to_le_bytes()).unwrap();
        cpu.write_mem(second + 12, &value.to_le_bytes()).unwrap();
        cpu.write_mem(value, &0xabcdef01u32.to_le_bytes()).unwrap();
        for (offset, v) in [(4, first + 4), (8, second + 4), (12, 2), (20, first)] {
            cpu.write_mem(0x2000 + offset, &v.to_le_bytes()).unwrap();
        }
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:2182",
            &[0x2000],
        );
        assert_eq!(cpu.read_mem(0x2004, 20).unwrap(), [0; 20]);
        assert_eq!(cpu.read_u32_le(0x2018).unwrap(), 4);
        assert_eq!(cpu.read_u32_le(value).unwrap(), 0xabcdef01);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:280",
            &[0x2000],
        );
        k.heap.free(value);
        assert_eq!(k.heap.free_bytes(), baseline);
    }
}

pub(super) fn recycle_node(
    ctx: &mut CallCtx<'_>,
    list: u32,
    node: u32,
    string: bool,
) -> Result<(), KernelError> {
    let pool = NodePool::read(ctx, list)?;
    pool.validate(node)?;
    let count = word(ctx, list + 12)?;
    if count == 0 {
        return Err(KernelError::Dispatch(
            "MFC list node count underflow".into(),
        ));
    }
    let mut free = word(ctx, list + 16)?;
    let mut remaining = pool.capacity;
    while free != 0 {
        pool.validate(free)?;
        if remaining == 0 || free == node {
            return Err(KernelError::Dispatch("MFC list node already free".into()));
        }
        remaining -= 1;
        free = word(ctx, free)?;
    }
    if string {
        release_string(ctx, node + 8)?;
    }
    let free = word(ctx, list + 16)?;
    put(ctx, node, free)?;
    put(ctx, list + 16, node)?;
    put(ctx, list + 12, count - 1)?;
    if count == 1 {
        clear(ctx, list)?;
    }
    Ok(())
}
