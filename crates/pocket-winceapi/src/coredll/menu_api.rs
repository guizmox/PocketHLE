//! Owned ordered MENU resources; enumeration fails at the end instead of looping.
use super::*;
use pocket_kernel::MenuItem;
fn ret(v: u32) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(v))
}
pub(crate) fn alloc(k: &mut KernelState, items: Vec<MenuItem>) -> u32 {
    let mut handle = k.next_menu_handle;
    while handle == 0 || k.menus.contains_key(&handle) || k.menu_items.contains_key(&handle) {
        let Some(next) = handle.checked_add(1) else {
            return 0;
        };
        handle = next;
    }
    let Some(next) = handle.checked_add(1) else {
        return 0;
    };
    k.next_menu_handle = next;
    k.menus
        .insert(handle, items.iter().map(|i| (i.id, 0)).collect());
    k.menu_items.insert(handle, items);
    handle
}

#[derive(Default)]
struct Parsed {
    item: MenuItem,
    state: u32,
    children: Vec<Parsed>,
}
fn parse(b: &[u8]) -> Option<Vec<Parsed>> {
    fn word(b: &[u8], p: &mut usize) -> Option<u16> {
        let v = u16::from_le_bytes(b.get(*p..*p + 2)?.try_into().ok()?);
        *p += 2;
        Some(v)
    }
    fn list(b: &[u8], p: &mut usize, depth: usize, budget: &mut usize) -> Option<Vec<Parsed>> {
        if depth > 16 {
            return None;
        }
        let mut out = Vec::new();
        loop {
            if *budget == 0 {
                return None;
            }
            *budget -= 1;
            let flags = word(b, p)? as u32;
            let id = if flags & 16 == 0 {
                word(b, p)? as u32
            } else {
                0
            };
            let mut text = Vec::new();
            loop {
                let c = word(b, p)?;
                if c == 0 {
                    break;
                }
                if text.len() >= 4096 {
                    return None;
                }
                text.push(c);
            }
            let children = if flags & 16 != 0 {
                list(b, p, depth + 1, budget)?
            } else {
                Vec::new()
            };
            out.push(Parsed {
                item: MenuItem {
                    id,
                    // Standard MENU templates also encode separators as an
                    // empty, zero-ID non-popup entry (without MF_SEPARATOR).
                    kind: (flags & 0x6b64)
                        | if flags & (0x10 | 0x104) == 0 && id == 0 && text.is_empty() {
                            0x800
                        } else {
                            0
                        },
                    text,
                    ..Default::default()
                },
                state: flags & 0xb,
                children,
            });
            if flags & 128 != 0 {
                break;
            }
        }
        Some(out)
    }
    let mut p = 0;
    if word(b, &mut p)? != 0 {
        return None;
    }
    let extra = word(b, &mut p)? as usize;
    p = p.checked_add(extra)?;
    list(b, &mut p, 0, &mut 4096)
}
fn commit(k: &mut KernelState, parsed: Vec<Parsed>) -> u32 {
    let mut items = Vec::new();
    let mut states = Vec::new();
    let mut children = Vec::new();
    for mut p in parsed {
        if !p.children.is_empty() {
            let child = commit(k, p.children);
            if child == 0 {
                for handle in children {
                    destroy_owned(k, handle);
                }
                return 0;
            }
            p.item.submenu = child;
            children.push(child);
        }
        states.push((p.item.id, p.state));
        items.push(p.item);
    }
    let handle = alloc(k, items);
    if handle == 0 {
        for child in children {
            destroy_owned(k, child);
        }
        return 0;
    }
    k.menus.insert(handle, states.into_iter().collect());
    handle
}

/// An unpublished MENU tree. Inspect labels without allocating runtime handles;
/// publishing consumes the plan and commits through the ordinary menu owner.
pub(crate) struct MenuTemplate(Vec<Parsed>);
impl MenuTemplate {
    pub(crate) fn parse(bytes: &[u8]) -> Option<Self> {
        parse(bytes).map(Self)
    }
    pub(crate) fn root_label(&self, index: usize) -> Option<&[u16]> {
        self.0.get(index).map(|p| p.item.text.as_slice())
    }
    pub(crate) fn install(self, kernel: &mut KernelState) -> Option<u32> {
        let handle = commit(kernel, self.0);
        (handle != 0).then_some(handle)
    }
}
pub(crate) fn from_bytes(k: &mut KernelState, bytes: &[u8]) -> Option<u32> {
    MenuTemplate::parse(bytes)?.install(k)
}

pub(super) fn load(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let module = ctx.arg_u32(0)?;
    let raw = ctx.arg_u32(1)?;
    let key = read_wide_resource_key(ctx, raw)?;
    let Some((e, base)) = lookup_resource(ctx.kernel, module, &ResourceKey::Id(4), &key) else {
        return ret(0);
    };
    if e.size > 1048576 {
        return ret(0);
    }
    let b = ctx.cpu.read_mem(base + e.data_rva, e.size)?;
    let Some(p) = parse(&b) else {
        return ret(0);
    };
    ret(commit(ctx.kernel, p))
}
pub(super) fn indirect(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(0)?;
    let mut b = Vec::new();
    for i in 0..65536u32 {
        let Ok(v) = ctx.cpu.read_mem(p + i * 2, 2) else {
            break;
        };
        b.extend(v);
        if let Some(parsed) = parse(&b) {
            return ret(commit(ctx.kernel, parsed));
        }
    }
    ret(0)
}
pub(super) fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    ret(alloc(ctx.kernel, Vec::new()))
}
fn find(k: &KernelState, h: u32, item: u32, pos: bool, depth: usize) -> Option<(u32, usize)> {
    if depth > 16 {
        return None;
    }
    let a = k.menu_items.get(&h)?;
    if pos {
        return ((item as usize) < a.len()).then_some((h, item as usize));
    }
    for (i, v) in a.iter().enumerate() {
        if v.id == item {
            return Some((h, i));
        }
        if v.submenu != 0 {
            if let Some(found) = find(k, v.submenu, item, false, depth + 1) {
                return Some(found);
            }
        }
    }
    None
}
pub(super) fn get_info(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (h, id, pos, p) = (
        ctx.arg_u32(0)?,
        ctx.arg_u32(1)?,
        ctx.arg_u32(2)? != 0,
        ctx.arg_u32(3)?,
    );
    let Some((h, i)) = find(ctx.kernel, h, id, pos, 0) else {
        return ret(0);
    };
    let Ok(mut b) = ctx.cpu.read_mem(p, 44) else {
        return ret(0);
    };
    let f = |o| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let size = f(0);
    // CE MFC also uses this 44-byte layout with a zero-initialized cbSize.
    // Unknown nonzero structure sizes remain invalid.
    if !matches!(size, 0 | 44 | 48) {
        return ret(0);
    }
    let (mask, text, cap) = (f(4), f(36), f(40));
    let item = ctx.kernel.menu_items[&h][i].clone();
    let state = ctx
        .kernel
        .menus
        .get(&h)
        .and_then(|m| m.get(&item.id))
        .copied()
        .unwrap_or(0);
    for (off, value, bit) in [
        (8, item.kind, 0x100 | 0x10),
        (12, state, 1),
        (16, item.id, 2),
        (20, item.submenu, 4),
        (24, item.checked, 8),
        (28, item.unchecked, 8),
        (32, item.data, 32),
    ] {
        if mask & bit != 0 {
            b[off..off + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    if mask & (0x40 | 0x10) != 0 {
        let count = if text != 0 && cap > 0 {
            (cap as usize - 1).min(item.text.len())
        } else {
            item.text.len()
        };
        if text != 0 && cap > 0 {
            let bytes: Vec<_> = item.text[..count]
                .iter()
                .copied()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect();
            if ctx.cpu.write_mem(text, &bytes).is_err() {
                return ret(0);
            }
        }
        b[40..44].copy_from_slice(&(count as u32).to_le_bytes());
    }
    if ctx.cpu.write_mem(p, &b).is_err() {
        return ret(0);
    }
    if size == 48
        && mask & 128 != 0
        && ctx
            .cpu
            .write_mem(p + 44, &item.bitmap.to_le_bytes())
            .is_err()
    {
        return ret(0);
    }
    ret(1)
}
pub(super) fn set_info(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (h, id, pos, p) = (
        ctx.arg_u32(0)?,
        ctx.arg_u32(1)?,
        ctx.arg_u32(2)? != 0,
        ctx.arg_u32(3)?,
    );
    let Some((h, i)) = find(ctx.kernel, h, id, pos, 0) else {
        return ret(0);
    };
    let Ok(b) = ctx.cpu.read_mem(p, 44) else {
        return ret(0);
    };
    let f = |o| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    if !matches!(f(0), 44 | 48) {
        return ret(0);
    }
    let mask = f(4);
    let text = if mask & (0x40 | 0x10) != 0 && f(36) != 0 {
        Some(read_wstr(ctx, f(36), 4096)?)
    } else {
        None
    };
    let item = &mut ctx.kernel.menu_items.get_mut(&h).unwrap()[i];
    let old = item.id;
    if mask & 2 != 0 {
        item.id = f(16);
    }
    if mask & (0x100 | 0x10) != 0 {
        item.kind = f(8);
    }
    if mask & 4 != 0 {
        item.submenu = f(20);
    }
    if mask & 8 != 0 {
        item.checked = f(24);
        item.unchecked = f(28);
    }
    if mask & 32 != 0 {
        item.data = f(32);
    }
    if let Some(text) = text {
        item.text = text;
    }
    let new = item.id;
    let m = ctx.kernel.menus.entry(h).or_default();
    let prev = m.remove(&old).unwrap_or(0);
    m.insert(new, if mask & 1 != 0 { f(12) } else { prev });
    ret(1)
}
pub(super) fn submenu(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (h, i) = (ctx.arg_u32(0)?, ctx.arg_u32(1)? as usize);
    ret(ctx
        .kernel
        .menu_items
        .get(&h)
        .and_then(|a| a.get(i))
        .map_or(0, |i| i.submenu))
}
pub(super) fn count(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let h = ctx.arg_u32(0)?;
    ret(ctx
        .kernel
        .menu_items
        .get(&h)
        .map_or(u32::MAX, |a| a.len() as u32))
}
pub(super) fn id(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (h, i) = (ctx.arg_u32(0)?, ctx.arg_u32(1)? as usize);
    ret(ctx
        .kernel
        .menu_items
        .get(&h)
        .and_then(|a| a.get(i))
        .filter(|i| i.submenu == 0)
        .map_or(u32::MAX, |i| i.id))
}
pub(super) fn state(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (h, id, flags) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    let Some((h, i)) = find(ctx.kernel, h, id, flags & 0x400 != 0, 0) else {
        return ret(u32::MAX);
    };
    let item = &ctx.kernel.menu_items[&h][i];
    ret(ctx
        .kernel
        .menus
        .get(&h)
        .and_then(|m| m.get(&item.id))
        .copied()
        .unwrap_or(0)
        | item.kind
        | if item.submenu != 0 { 16 } else { 0 })
}
fn flag(ctx: &mut CallCtx<'_>, mask: u32) -> Result<DispatchOutcome, KernelError> {
    let (h, id, f) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    let Some((h, i)) = find(ctx.kernel, h, id, f & 0x400 != 0, 0) else {
        return ret(u32::MAX);
    };
    let id = ctx.kernel.menu_items[&h][i].id;
    let v = ctx
        .kernel
        .menus
        .entry(h)
        .or_default()
        .entry(id)
        .or_default();
    let old = *v;
    *v = (*v & !mask) | (f & mask);
    ret(old)
}
pub(super) fn check(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    flag(ctx, 8)
}
pub(super) fn enable(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    flag(ctx, 3)
}
fn make_item(ctx: &mut CallCtx<'_>, flags: u32, id: u32, p: u32) -> Result<MenuItem, KernelError> {
    Ok(MenuItem {
        id: if flags & 16 != 0 { 0 } else { id },
        submenu: if flags & 16 != 0 { id } else { 0 },
        kind: flags & 0x6b64,
        text: if p != 0 && flags & 0x104 == 0 {
            read_wstr(ctx, p, 4096)?
        } else {
            Vec::new()
        },
        ..Default::default()
    })
}
pub(super) fn append(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (h, f, id, p) = (
        ctx.arg_u32(0)?,
        ctx.arg_u32(1)?,
        ctx.arg_u32(2)?,
        ctx.arg_u32(3)?,
    );
    if !ctx.kernel.menu_items.contains_key(&h) {
        return ret(0);
    }
    let v = make_item(ctx, f, id, p)?;
    ctx.kernel.menus.entry(h).or_default().insert(v.id, f & 0xb);
    ctx.kernel.menu_items.get_mut(&h).unwrap().push(v);
    ret(1)
}
pub(super) fn insert(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (h, n, f, id, p) = (
        ctx.arg_u32(0)?,
        ctx.arg_u32(1)?,
        ctx.arg_u32(2)?,
        ctx.arg_u32(3)?,
        ctx.arg_u32(4)?,
    );
    let at = if n == u32::MAX {
        ctx.kernel.menu_items.get(&h).map(|v| (h, v.len()))
    } else {
        find(ctx.kernel, h, n, f & 0x400 != 0, 0)
    };
    let Some((h, i)) = at else {
        return ret(0);
    };
    let v = make_item(ctx, f, id, p)?;
    ctx.kernel.menus.entry(h).or_default().insert(v.id, f & 0xb);
    ctx.kernel.menu_items.get_mut(&h).unwrap().insert(i, v);
    ret(1)
}
pub(super) fn modify(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (h, n, f, id, p) = (
        ctx.arg_u32(0)?,
        ctx.arg_u32(1)?,
        ctx.arg_u32(2)?,
        ctx.arg_u32(3)?,
        ctx.arg_u32(4)?,
    );
    let Some((h, i)) = find(ctx.kernel, h, n, f & 0x400 != 0, 0) else {
        return ret(0);
    };
    let v = make_item(ctx, f, id, p)?;
    ctx.kernel.menus.entry(h).or_default().insert(v.id, f & 0xb);
    ctx.kernel.menu_items.get_mut(&h).unwrap()[i] = v;
    ret(1)
}
pub(super) fn remove(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (h, n, f) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    let Some((h, i)) = find(ctx.kernel, h, n, f & 0x400 != 0, 0) else {
        return ret(0);
    };
    let v = ctx.kernel.menu_items.get_mut(&h).unwrap().remove(i);
    if let Some(m) = ctx.kernel.menus.get_mut(&h) {
        m.remove(&v.id);
    }
    ret(1)
}
pub(crate) fn destroy_owned(k: &mut KernelState, handle: u32) -> bool {
    let existed = k.menus.contains_key(&handle);
    let mut todo = vec![handle];
    let mut seen = std::collections::HashSet::new();
    while let Some(h) = todo.pop() {
        if !seen.insert(h) {
            continue;
        }
        if let Some(items) = k.menu_items.remove(&h) {
            todo.extend(items.into_iter().map(|i| i.submenu).filter(|h| *h != 0));
        }
        k.menus.remove(&h);
    }
    k.window_menus.retain(|_, v| !seen.contains(v));
    existed
}
pub(super) fn destroy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    ret(u32::from(destroy_owned(ctx.kernel, handle)))
}

pub(super) fn get_menu(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let h = ctx.arg_u32(0)?;
    ret(ctx.kernel.window_menus.get(&h).copied().unwrap_or(0))
}
pub(super) fn set_menu(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (h, m) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?);
    if m != 0 && !ctx.kernel.menu_items.contains_key(&m) {
        return ret(0);
    }
    ctx.kernel.window_menus.insert(h, m);
    ret(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exhausted_menu_handles_roll_back_a_partially_committed_resource_tree() {
        let mut k = crate::gx::tests::fresh_kernel();
        let root = Parsed {
            item: MenuItem::default(),
            state: 0,
            children: vec![Parsed {
                item: MenuItem {
                    id: 12,
                    ..Default::default()
                },
                ..Default::default()
            }],
        };
        k.next_menu_handle = u32::MAX - 1;
        assert_eq!(commit(&mut k, vec![root]), 0);
        assert!(k.menu_items.is_empty());
        assert!(k.menus.is_empty());
    }
    #[test]
    fn menu_allocation_does_not_overwrite_existing_or_zero_handles() {
        let mut k = crate::gx::tests::fresh_kernel();
        k.next_menu_handle = 0;
        k.menu_items.insert(
            1,
            vec![MenuItem {
                id: 12,
                ..Default::default()
            }],
        );
        let handle = alloc(&mut k, Vec::new());
        assert_eq!(handle, 2);
        assert_eq!(k.menu_items[&1][0].id, 12);
        assert!(!k.menu_items.contains_key(&0));
    }
    #[test]
    fn standard_menu_implicit_and_explicit_separators_preserve_empty_commands() {
        let mut b = vec![0u8; 4];
        for (flags, id) in [(0u16, 0u16), (0x800, 0), (0x80, 123)] {
            b.extend_from_slice(&flags.to_le_bytes());
            b.extend_from_slice(&id.to_le_bytes());
            b.extend_from_slice(&0u16.to_le_bytes());
        }
        let items = parse(&b).unwrap();
        assert_eq!(items.len(), 3);
        assert_ne!(items[0].item.kind & 0x800, 0);
        assert_ne!(items[1].item.kind & 0x800, 0);
        assert_eq!(items[2].item.kind & 0x800, 0);
        assert_eq!(items[2].item.id, 123);
    }
}
