//! Stateful TreeView, Header and UpDown messages used by CERF SampleApp.
//! Structures follow the CE6 SDK ABI; guest procedures remain application-owned.
use super::*;
use pocket_kernel::controls::{CommonControlChange, CommonControlFrame, HeaderItem, TreeItem};

pub(super) const CHANGE: u32 = 0x7fff_ff02;

#[cfg(test)]
#[path = "common_controls_tests.rs"]
mod tests;

pub(super) fn initialize(ctx: &mut CallCtx<'_>, hwnd: u32) {
    let Some(c) = ctx.kernel.controls.get(hwnd) else {
        return;
    };
    if c.class == ControlClass::UpDown && c.style & 0x10 != 0 {
        let parent = c.parent;
        let siblings = ctx.kernel.controls.children_of(parent);
        if let Some(index) = siblings.iter().position(|h| *h == hwnd) {
            if index > 0 {
                ctx.kernel.controls.get_mut(hwnd).unwrap().updown.buddy = siblings[index - 1];
                sync_buddy(ctx, hwnd);
            }
        }
    }
}

fn accessible(ctx: &mut CallCtx<'_>, ptr: u32, size: u32, prot: Prot) -> bool {
    if ptr == 0 || ctx.cpu.check_guest_access(ptr, size, prot).is_err() {
        set_thread_error(ctx, 87);
        false
    } else {
        true
    }
}
fn text(ctx: &mut CallCtx<'_>, ptr: u32, wide: bool) -> Result<Option<String>, KernelError> {
    if ptr == 0 || ptr == u32::MAX {
        return Ok(Some(String::new()));
    }
    if !accessible(ctx, ptr, if wide { 2 } else { 1 }, Prot::READ) {
        return Ok(None);
    }
    let (value, size) = if wide {
        let chars = read_wstr(ctx, ptr, 32768)?;
        let size = (chars.len() as u32 + 1) * 2;
        (
            String::from_utf16_lossy(&chars)
                .trim_end_matches('\0')
                .to_string(),
            size,
        )
    } else {
        let bytes = read_cstr(ctx, ptr, 32768)?;
        let size = bytes.len() as u32 + 1;
        (crate::crt_ansi::decode(&bytes), size)
    };
    Ok(accessible(ctx, ptr, size, Prot::READ).then_some(value))
}
fn text_output(
    ctx: &mut CallCtx<'_>,
    ptr: u32,
    capacity: u32,
    value: &str,
    wide: bool,
) -> Result<bool, KernelError> {
    let capacity = capacity.min(32768) as usize;
    if capacity == 0 {
        return Ok(true);
    }
    let bytes = if wide {
        value
            .encode_utf16()
            .take(capacity - 1)
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    } else {
        value.bytes().take(capacity - 1).chain(Some(0)).collect()
    };
    if !accessible(ctx, ptr, bytes.len() as u32, Prot::WRITE) {
        return Ok(false);
    }
    ctx.cpu.write_mem(ptr, &bytes)?;
    Ok(true)
}
fn rect(ctx: &mut CallCtx<'_>, ptr: u32, r: [i32; 4]) -> Result<bool, KernelError> {
    if !accessible(ctx, ptr, 16, Prot::WRITE) {
        return Ok(false);
    }
    ctx.cpu.write_mem(
        ptr,
        &r.into_iter().flat_map(i32::to_le_bytes).collect::<Vec<_>>(),
    )?;
    Ok(true)
}
fn read_tree_item(
    ctx: &mut CallCtx<'_>,
    ptr: u32,
    wide: bool,
    mut item: TreeItem,
) -> Result<Option<TreeItem>, KernelError> {
    if !accessible(ctx, ptr, 40, Prot::READ) {
        return Ok(None);
    }
    let mask = ctx.cpu.read_u32_le(ptr)?;
    if mask & 1 != 0 {
        let p = ctx.cpu.read_u32_le(ptr + 16)?;
        let Some(value) = text(ctx, p, wide)? else {
            return Ok(None);
        };
        item.text = value;
        item.text_callback = p == u32::MAX;
    }
    if mask & 2 != 0 {
        item.image = ctx.cpu.read_u32_le(ptr + 24)? as i32;
        item.image_callback = item.image == -1;
    }
    if mask & 0x20 != 0 {
        item.selected_image = ctx.cpu.read_u32_le(ptr + 28)? as i32;
    }
    if mask & 0x40 != 0 {
        item.children_hint = ctx.cpu.read_u32_le(ptr + 32)? as i32;
    }
    if mask & 4 != 0 {
        item.data = ctx.cpu.read_u32_le(ptr + 36)?;
    }
    if mask & 8 != 0 {
        let state_mask = ctx.cpu.read_u32_le(ptr + 12)?;
        item.state = (item.state & !state_mask) | (ctx.cpu.read_u32_le(ptr + 8)? & state_mask);
    }
    Ok(Some(item))
}

pub(super) fn message(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    msg: u32,
    wp: u32,
    lp: u32,
) -> Result<Option<u32>, KernelError> {
    match ctx.kernel.controls.get(hwnd).map(|c| c.class) {
        Some(ControlClass::TreeView) => tree_message(ctx, hwnd, msg, wp, lp),
        Some(ControlClass::Header) => header_message(ctx, hwnd, msg, wp, lp),
        Some(ControlClass::UpDown) => updown_message(ctx, hwnd, msg, wp, lp),
        _ => Ok(None),
    }
}
fn tree_message(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    msg: u32,
    wp: u32,
    lp: u32,
) -> Result<Option<u32>, KernelError> {
    let result = match msg {
        0x1100 | 0x1132 => {
            if !accessible(ctx, lp, 48, Prot::READ) {
                return Ok(Some(0));
            }
            let parent = ctx.cpu.read_u32_le(lp)?;
            let after = ctx.cpu.read_u32_le(lp + 4)?;
            let Some(item) = read_tree_item(
                ctx,
                lp + 8,
                msg == 0x1132,
                TreeItem {
                    image: -1,
                    selected_image: -1,
                    ..Default::default()
                },
            )?
            else {
                return Ok(Some(0));
            };
            let selected = item.state & 2 != 0;
            let handle = ctx
                .kernel
                .controls
                .get_mut(hwnd)
                .unwrap()
                .tree
                .insert(parent, after, item);
            if selected && handle != 0 {
                ctx.kernel.controls.get_mut(hwnd).unwrap().tree.selected = handle;
            }
            handle
        }
        0x1101 => u32::from(ctx.kernel.controls.get_mut(hwnd).unwrap().tree.remove(lp)),
        0x1104 => {
            if !accessible(ctx, lp, 16, Prot::READ | Prot::WRITE) {
                return Ok(Some(0));
            }
            let handle = ctx.cpu.read_u32_le(lp)?;
            let r = ctx
                .kernel
                .controls
                .get(hwnd)
                .unwrap()
                .tree_rect(handle, wp != 0);
            if let Some(r) = r {
                u32::from(rect(ctx, lp, r)?)
            } else {
                0
            }
        }
        0x1105 => ctx.kernel.controls.get(hwnd).unwrap().tree.items.len() as u32,
        0x1106 => ctx.kernel.controls.get(hwnd).unwrap().tree.indent as u32,
        0x1107 => {
            ctx.kernel.controls.get_mut(hwnd).unwrap().tree.indent = (wp as i32).clamp(8, 4096);
            0
        }
        0x1108 | 0x1109 => {
            if !matches!(wp, 0 | 2) {
                return Ok(Some(0));
            }
            let t = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().tree;
            if msg == 0x1109 {
                std::mem::replace(&mut t.image_lists[wp as usize], lp)
            } else {
                t.image_lists[wp as usize]
            }
        }
        0x110a => ctx.kernel.controls.get(hwnd).unwrap().tree.next(lp, wp),
        0x110b if wp != 9 => {
            let c = ctx.kernel.controls.get_mut(hwnd).unwrap();
            if lp != 0 && !c.tree.items.contains_key(&lp) {
                return Ok(Some(0));
            }
            if wp == 8 {
                c.tree.drop_target = lp;
            } else if wp == 5 {
                c.tree.ensure_visible(lp, c.h);
                c.tree.first_visible = c
                    .tree
                    .visible()
                    .iter()
                    .position(|(h, _)| *h == lp)
                    .unwrap_or(0);
            } else {
                return Ok(Some(0));
            }
            1
        }
        0x110c | 0x113e => {
            if !accessible(ctx, lp, 40, Prot::READ | Prot::WRITE) {
                return Ok(Some(0));
            }
            let handle = ctx.cpu.read_u32_le(lp + 4)?;
            let c = ctx.kernel.controls.get(hwnd).unwrap();
            let Some(item) = c.tree.items.get(&handle).cloned() else {
                return Ok(Some(0));
            };
            let state = c.tree.item_state(&item);
            let mask = ctx.cpu.read_u32_le(lp)?;
            if mask & 1 != 0 {
                let ptr = ctx.cpu.read_u32_le(lp + 16)?;
                let cap = ctx.cpu.read_u32_le(lp + 20)?;
                if !text_output(ctx, ptr, cap, &item.text, msg == 0x113e)? {
                    return Ok(Some(0));
                }
            }
            for (bit, offset, value) in [
                (2, 24, item.image as u32),
                (0x20, 28, item.selected_image as u32),
                (
                    0x40,
                    32,
                    i32::from(!item.children.is_empty()).max(item.children_hint) as u32,
                ),
                (4, 36, item.data),
            ] {
                if mask & bit != 0 {
                    ctx.cpu.write_mem(lp + offset, &value.to_le_bytes())?;
                }
            }
            if mask & 8 != 0 {
                let m = ctx.cpu.read_u32_le(lp + 12)?;
                ctx.cpu.write_mem(lp + 8, &(state & m).to_le_bytes())?;
            }
            1
        }
        0x110d | 0x113f => {
            if !accessible(ctx, lp, 40, Prot::READ) {
                return Ok(Some(0));
            }
            let handle = ctx.cpu.read_u32_le(lp + 4)?;
            let c = ctx.kernel.controls.get(hwnd).unwrap();
            let Some(mut item) = c.tree.items.get(&handle).cloned() else {
                return Ok(Some(0));
            };
            item.state = c.tree.item_state(&item);
            let Some(item) = read_tree_item(ctx, lp, msg == 0x113f, item)? else {
                return Ok(Some(0));
            };
            let mask = ctx.cpu.read_u32_le(lp)?;
            let sm = ctx.cpu.read_u32_le(lp + 12)?;
            let t = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().tree;
            if mask & 8 != 0 && sm & 2 != 0 {
                if item.state & 2 != 0 {
                    t.selected = handle;
                } else if t.selected == handle {
                    t.selected = 0;
                }
            }
            if mask & 8 != 0 && sm & 8 != 0 {
                if item.state & 8 != 0 {
                    t.drop_target = handle;
                } else if t.drop_target == handle {
                    t.drop_target = 0;
                }
            }
            t.items.insert(handle, item);
            1
        }
        0x110f => 0, // No label editor until TVM_EDITLABEL is requested.
        0x1110 => {
            let c = ctx.kernel.controls.get(hwnd).unwrap();
            (c.h.max(0) / c.tree.row_height.max(1)) as u32
        }
        0x1111 => {
            if !accessible(ctx, lp, 16, Prot::READ | Prot::WRITE) {
                return Ok(Some(0));
            }
            let x = ctx.cpu.read_u32_le(lp)? as i32;
            let y = ctx.cpu.read_u32_le(lp + 4)? as i32;
            let hit = ctx.kernel.controls.get(hwnd).unwrap().tree_hit(x, y);
            let (item, flags) =
                hit.map_or((0, 1u32), |(h, button)| (h, if button { 0x10 } else { 4 }));
            ctx.cpu.write_mem(lp + 8, &flags.to_le_bytes())?;
            ctx.cpu.write_mem(lp + 12, &item.to_le_bytes())?;
            item
        }
        0x1113 => u32::from(
            ctx.kernel
                .controls
                .get_mut(hwnd)
                .unwrap()
                .tree
                .sort(lp, wp != 0),
        ),
        0x1114 => {
            let c = ctx.kernel.controls.get_mut(hwnd).unwrap();
            u32::from(c.tree.ensure_visible(lp, c.h))
        }
        0x111b => {
            let t = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().tree;
            let old = t.row_height;
            t.row_height = if wp == u32::MAX {
                20
            } else {
                (wp as i32).clamp(2, 4096)
            };
            old as u32
        }
        0x111c => ctx.kernel.controls.get(hwnd).unwrap().tree.row_height as u32,
        0x111d | 0x111e => {
            let t = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().tree;
            let value = if lp == u32::MAX {
                if msg == 0x111d {
                    0x00ff_ffff
                } else {
                    0
                }
            } else {
                lp
            };
            if msg == 0x111d {
                std::mem::replace(&mut t.background, value)
            } else {
                std::mem::replace(&mut t.text_color, value)
            }
        }
        0x111f => ctx.kernel.controls.get(hwnd).unwrap().tree.background,
        0x1120 => ctx.kernel.controls.get(hwnd).unwrap().tree.text_color,
        _ => return Ok(None),
    };
    if matches!(
        msg,
        0x1100
            | 0x1132
            | 0x1101
            | 0x1107
            | 0x1109
            | 0x110b
            | 0x110d
            | 0x113f
            | 0x1113
            | 0x1114
            | 0x111b
            | 0x111d
            | 0x111e
    ) {
        repaint_controls(ctx);
    }
    Ok(Some(result))
}

fn header_message(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    msg: u32,
    wp: u32,
    lp: u32,
) -> Result<Option<u32>, KernelError> {
    let result = match msg {
        0x1200 => ctx.kernel.controls.get(hwnd).unwrap().header.items.len() as u32,
        0x1201 | 0x120a | 0x1204 | 0x120c => {
            let insert = matches!(msg, 0x1201 | 0x120a);
            let count = ctx.kernel.controls.get(hwnd).unwrap().header.items.len();
            if (wp as i32) < 0
                || (!insert && wp as usize >= count)
                || insert && count >= 4096
                || !accessible(ctx, lp, 28, Prot::READ)
            {
                return Ok(Some(if insert { u32::MAX } else { 0 }));
            }
            let mask = ctx.cpu.read_u32_le(lp)?;
            let mut item = if insert {
                HeaderItem {
                    image: -1,
                    ..Default::default()
                }
            } else {
                ctx.kernel.controls.get(hwnd).unwrap().header.items[wp as usize].clone()
            };
            if mask & 1 != 0 {
                item.width = (ctx.cpu.read_u32_le(lp + 4)? as i32).clamp(0, 32768);
            }
            if mask & 2 != 0 {
                let ptr = ctx.cpu.read_u32_le(lp + 8)?;
                let Some(value) = text(ctx, ptr, matches!(msg, 0x120a | 0x120c))? else {
                    return Ok(Some(if insert { u32::MAX } else { 0 }));
                };
                item.text = value;
            }
            if mask & 0x10 != 0 {
                item.bitmap = ctx.cpu.read_u32_le(lp + 12)?;
            }
            if mask & 4 != 0 {
                item.format = ctx.cpu.read_u32_le(lp + 20)?;
            }
            if mask & 8 != 0 {
                item.data = ctx.cpu.read_u32_le(lp + 24)?;
            }
            if mask & 0x20 != 0 {
                if !accessible(ctx, lp + 28, 4, Prot::READ) {
                    return Ok(Some(if insert { u32::MAX } else { 0 }));
                }
                item.image = ctx.cpu.read_u32_le(lp + 28)? as i32;
            }
            let requested_order = if mask & 0x80 != 0 {
                if !accessible(ctx, lp + 32, 4, Prot::READ) {
                    return Ok(Some(if insert { u32::MAX } else { 0 }));
                }
                Some(ctx.cpu.read_u32_le(lp + 32)? as usize)
            } else {
                None
            };
            let h = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().header;
            let index = (wp as usize).min(count);
            if insert {
                h.items.insert(index, item);
                for i in &mut h.order {
                    if *i >= index {
                        *i += 1;
                    }
                }
                h.order
                    .insert(requested_order.unwrap_or(index).min(h.order.len()), index);
                index as u32
            } else {
                h.items[index] = item;
                if let Some(order) = requested_order {
                    h.order.retain(|i| *i != index);
                    h.order.insert(order.min(h.order.len()), index);
                }
                1
            }
        }
        0x1202 => {
            let h = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().header;
            let index = wp as usize;
            if index >= h.items.len() {
                return Ok(Some(0));
            }
            h.items.remove(index);
            h.order.retain(|i| *i != index);
            for i in &mut h.order {
                if *i > index {
                    *i -= 1;
                }
            }
            1
        }
        0x1203 | 0x120b => {
            if !accessible(ctx, lp, 28, Prot::READ | Prot::WRITE) {
                return Ok(Some(0));
            }
            let h = &ctx.kernel.controls.get(hwnd).unwrap().header;
            let Some(item) = h.items.get(wp as usize).cloned() else {
                return Ok(Some(0));
            };
            let order = h.order.iter().position(|i| *i == wp as usize).unwrap_or(0) as u32;
            let mask = ctx.cpu.read_u32_le(lp)?;
            for (bit, offset) in [(0x20, 28), (0x80, 32)] {
                if mask & bit != 0 && !accessible(ctx, lp + offset, 4, Prot::WRITE) {
                    return Ok(Some(0));
                }
            }
            if mask & 2 != 0 {
                let p = ctx.cpu.read_u32_le(lp + 8)?;
                let cap = ctx.cpu.read_u32_le(lp + 16)?;
                if !text_output(ctx, p, cap, &item.text, msg == 0x120b)? {
                    return Ok(Some(0));
                }
            }
            for (bit, offset, value) in [
                (1, 4, item.width as u32),
                (0x10, 12, item.bitmap),
                (4, 20, item.format),
                (8, 24, item.data),
                (0x20, 28, item.image as u32),
                (0x80, 32, order),
            ] {
                if mask & bit != 0 {
                    ctx.cpu.write_mem(lp + offset, &value.to_le_bytes())?;
                }
            }
            1
        }
        0x1205 => {
            if !accessible(ctx, lp, 8, Prot::READ) {
                return Ok(Some(0));
            }
            let rp = ctx.cpu.read_u32_le(lp)?;
            let pos = ctx.cpu.read_u32_le(lp + 4)?;
            if !accessible(ctx, rp, 16, Prot::READ | Prot::WRITE)
                || !accessible(ctx, pos, 28, Prot::WRITE)
            {
                return Ok(Some(0));
            }
            let left = ctx.cpu.read_u32_le(rp)? as i32;
            let top = ctx.cpu.read_u32_le(rp + 4)? as i32;
            let right = ctx.cpu.read_u32_le(rp + 8)? as i32;
            let bottom = ctx.cpu.read_u32_le(rp + 12)? as i32;
            let c = ctx.kernel.controls.get(hwnd).unwrap();
            let height = if c.style & 8 != 0 {
                0
            } else {
                c.h.max(20).min(bottom.saturating_sub(top).max(0))
            };
            let words = [
                hwnd,
                0,
                left as u32,
                top as u32,
                right.saturating_sub(left) as u32,
                height as u32,
                0x14,
            ];
            ctx.cpu.write_mem(
                pos,
                &words
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )?;
            ctx.cpu
                .write_mem(rp + 4, &top.saturating_add(height).to_le_bytes())?;
            1
        }
        0x1206 => {
            if !accessible(ctx, lp, 16, Prot::READ | Prot::WRITE) {
                return Ok(Some(u32::MAX));
            }
            let x = ctx.cpu.read_u32_le(lp)? as i32;
            let y = ctx.cpu.read_u32_le(lp + 4)? as i32;
            let c = ctx.kernel.controls.get(hwnd).unwrap();
            let index = c.header.hit(x, y, c.h).map_or(u32::MAX, |i| i as u32);
            ctx.cpu.write_mem(
                lp + 8,
                &if index == u32::MAX { 1u32 } else { 2 }.to_le_bytes(),
            )?;
            ctx.cpu.write_mem(lp + 12, &index.to_le_bytes())?;
            index
        }
        0x1207 => {
            let c = ctx.kernel.controls.get(hwnd).unwrap();
            if let Some((x, y, w, h)) = c.header.rect(wp as usize, c.h) {
                u32::from(rect(ctx, lp, [x, y, x.saturating_add(w), y + h])?)
            } else {
                0
            }
        }
        0x1208 => std::mem::replace(
            &mut ctx.kernel.controls.get_mut(hwnd).unwrap().header.image_list,
            lp,
        ),
        0x1209 => ctx.kernel.controls.get(hwnd).unwrap().header.image_list,
        0x120f => ctx
            .kernel
            .controls
            .get(hwnd)
            .unwrap()
            .header
            .order
            .get(wp as usize)
            .map_or(u32::MAX, |i| *i as u32),
        0x1211 | 0x1212 => {
            let h = &ctx.kernel.controls.get(hwnd).unwrap().header;
            if wp as usize != h.items.len()
                || wp > 4096
                || (wp != 0
                    && !accessible(
                        ctx,
                        lp,
                        wp * 4,
                        if msg == 0x1211 {
                            Prot::WRITE
                        } else {
                            Prot::READ
                        },
                    ))
            {
                return Ok(Some(0));
            }
            if msg == 0x1211 {
                let bytes = ctx
                    .kernel
                    .controls
                    .get(hwnd)
                    .unwrap()
                    .header
                    .order
                    .iter()
                    .flat_map(|i| (*i as u32).to_le_bytes())
                    .collect::<Vec<_>>();
                ctx.cpu.write_mem(lp, &bytes)?;
            } else {
                let mut order = Vec::new();
                for i in 0..wp {
                    order.push(ctx.cpu.read_u32_le(lp + i * 4)? as usize);
                }
                let mut sorted = order.clone();
                sorted.sort_unstable();
                if sorted != (0..wp as usize).collect::<Vec<_>>() {
                    return Ok(Some(0));
                }
                ctx.kernel.controls.get_mut(hwnd).unwrap().header.order = order;
            }
            1
        }
        _ => return Ok(None),
    };
    if matches!(
        msg,
        0x1201 | 0x120a | 0x1204 | 0x120c | 0x1202 | 0x1208 | 0x1212
    ) {
        repaint_controls(ctx);
    }
    Ok(Some(result))
}

fn buddy_value(ctx: &CallCtx<'_>, hwnd: u32) -> Option<i32> {
    let c = ctx.kernel.controls.get(hwnd)?;
    if c.style & 2 == 0 {
        return Some(c.updown.position);
    }
    let text = ctx.kernel.controls.get(c.updown.buddy)?.text.trim();
    let text = if c.updown.base == 16 {
        text.strip_prefix("0x")
            .or_else(|| text.strip_prefix("0X"))
            .unwrap_or(text)
    } else {
        text
    };
    i32::from_str_radix(text, c.updown.base)
        .ok()
        .filter(|value| c.updown.clamp(*value) == *value)
}
fn sync_buddy(ctx: &mut CallCtx<'_>, hwnd: u32) {
    let c = ctx.kernel.controls.get(hwnd).unwrap();
    if c.style & 2 == 0 {
        return;
    }
    let buddy = c.updown.buddy;
    let pos = c.updown.position;
    let text = if c.updown.base == 16 {
        format!("{pos:X}")
    } else {
        pos.to_string()
    };
    if let Some(c) = ctx.kernel.controls.get_mut(buddy) {
        c.text = text;
    }
}
fn updown_message(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    msg: u32,
    wp: u32,
    lp: u32,
) -> Result<Option<u32>, KernelError> {
    let result = match msg {
        0x465 | 0x46f => {
            let u = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().updown;
            (u.lower, u.upper) = if msg == 0x465 {
                ((lp >> 16) as i16 as i32, lp as i16 as i32)
            } else {
                (wp as i32, lp as i32)
            };
            u.position = u.clamp(u.position);
            sync_buddy(ctx, hwnd);
            0
        }
        0x466 => {
            let u = &ctx.kernel.controls.get(hwnd).unwrap().updown;
            (u.upper as u32 & 0xffff) | ((u.lower as u32 & 0xffff) << 16)
        }
        0x470 => {
            if (wp != 0 && !accessible(ctx, wp, 4, Prot::WRITE))
                || (lp != 0 && !accessible(ctx, lp, 4, Prot::WRITE))
            {
                return Ok(Some(0));
            }
            let u = &ctx.kernel.controls.get(hwnd).unwrap().updown;
            let (min, max) = (u.lower, u.upper);
            if wp != 0 {
                ctx.cpu.write_mem(wp, &min.to_le_bytes())?;
            }
            if lp != 0 {
                ctx.cpu.write_mem(lp, &max.to_le_bytes())?;
            }
            0
        }
        0x467 | 0x471 => {
            let value = if msg == 0x467 {
                lp as i16 as i32
            } else {
                lp as i32
            };
            let u = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().updown;
            let old = u.position;
            u.position = u.clamp(value);
            sync_buddy(ctx, hwnd);
            if msg == 0x467 {
                old as u32 & 0xffff
            } else {
                old as u32
            }
        }
        0x468 | 0x472 => {
            if msg == 0x472 && lp != 0 && !accessible(ctx, lp, 4, Prot::WRITE) {
                return Ok(Some(0));
            }
            let value = buddy_value(ctx, hwnd);
            let error = u32::from(value.is_none());
            if let Some(value) = value {
                ctx.kernel.controls.get_mut(hwnd).unwrap().updown.position = value;
            }
            let pos = ctx.kernel.controls.get(hwnd).unwrap().updown.position;
            if msg == 0x472 {
                if lp != 0 {
                    ctx.cpu.write_mem(lp, &error.to_le_bytes())?;
                }
                pos as u32
            } else {
                (pos as u32 & 0xffff) | (error << 16)
            }
        }
        0x469 => {
            if wp != 0 && ctx.kernel.controls.get(wp).is_none() {
                return Ok(Some(0));
            }
            let old = std::mem::replace(
                &mut ctx.kernel.controls.get_mut(hwnd).unwrap().updown.buddy,
                wp,
            );
            sync_buddy(ctx, hwnd);
            old
        }
        0x46a => ctx.kernel.controls.get(hwnd).unwrap().updown.buddy,
        0x46b => {
            if wp == 0 || wp > 32 || !accessible(ctx, lp, wp * 8, Prot::READ) {
                return Ok(Some(0));
            }
            let mut accelerators = Vec::new();
            for i in 0..wp {
                accelerators.push((
                    ctx.cpu.read_u32_le(lp + i * 8)?,
                    ctx.cpu.read_u32_le(lp + i * 8 + 4)?,
                ));
            }
            if accelerators.windows(2).any(|a| a[0].0 > a[1].0) {
                return Ok(Some(0));
            }
            ctx.kernel
                .controls
                .get_mut(hwnd)
                .unwrap()
                .updown
                .accelerators = accelerators;
            1
        }
        0x46c => {
            let a = ctx
                .kernel
                .controls
                .get(hwnd)
                .unwrap()
                .updown
                .accelerators
                .clone();
            if wp == 0 {
                return Ok(Some(a.len() as u32));
            }
            let n = wp.min(a.len() as u32);
            if !accessible(ctx, lp, n * 8, Prot::WRITE) {
                return Ok(Some(0));
            }
            let bytes = a
                .into_iter()
                .take(n as usize)
                .flat_map(|(seconds, increment)| [seconds, increment])
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>();
            ctx.cpu.write_mem(lp, &bytes)?;
            n
        }
        0x46d => {
            if !matches!(wp, 10 | 16) {
                return Ok(Some(0));
            }
            let old = std::mem::replace(
                &mut ctx.kernel.controls.get_mut(hwnd).unwrap().updown.base,
                wp,
            );
            sync_buddy(ctx, hwnd);
            old
        }
        0x46e => ctx.kernel.controls.get(hwnd).unwrap().updown.base,
        _ => return Ok(None),
    };
    if matches!(msg, 0x465 | 0x46f | 0x467 | 0x471 | 0x469 | 0x46d) {
        repaint_controls(ctx);
    }
    Ok(Some(result))
}

fn tree_words(tree: &pocket_kernel::controls::TreeState, item: Option<&TreeItem>) -> [u32; 10] {
    item.map_or([0; 10], |item| {
        [
            0x7f,
            item.handle,
            tree.item_state(item),
            u32::MAX,
            0,
            0,
            item.image as u32,
            item.selected_image as u32,
            item.children_hint as u32,
            item.data,
        ]
    })
}
fn callback(
    ctx: &mut CallCtx<'_>,
    f: CommonControlFrame,
    proc: u32,
    parent: u32,
    id: u32,
) -> Result<DispatchOutcome, KernelError> {
    let c = ctx.kernel.controls.get(f.hwnd).unwrap();
    let (code, action, item) = match f.change {
        CommonControlChange::TreeSelect { item, reason } => {
            (if f.stage == 0 { -450i32 } else { -451 }, reason, item)
        }
        CommonControlChange::TreeExpand { item, action } => {
            (if f.stage == 0 { -454 } else { -455 }, action & 3, item)
        }
        CommonControlChange::UpDown { .. } => (-722, 0, 0),
    };
    let mut words = vec![f.hwnd, id, code as u32];
    if let CommonControlChange::UpDown { delta } = f.change {
        words.extend([c.updown.position as u32, delta as u32]);
    } else {
        words.push(action);
        let mut old = tree_words(&c.tree, f.old_item.as_ref());
        if let Some(item) = &f.old_item {
            old[2] = item.state;
        }
        words.extend(old);
        words.extend(tree_words(&c.tree, c.tree.items.get(&item)));
        words.extend([0, 0]);
    }
    ctx.cpu.write_mem(
        f.scratch,
        &words
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
    )?;
    if ctx.kernel.controls.panel(parent).is_some() {
        ctx.kernel.window_extra.insert((parent, 0), 0);
    }
    ctx.cpu.write_reg(ArmReg::R0, parent)?;
    ctx.cpu.write_reg(ArmReg::R1, 0x4e)?;
    ctx.cpu.write_reg(ArmReg::R2, id)?;
    ctx.cpu.write_reg(ArmReg::R3, f.scratch)?;
    ctx.cpu.write_reg(ArmReg::Lr, f.thunk)?;
    ctx.kernel.controls.common_control_frames.push(f);
    Ok(DispatchOutcome::JumpTo(proc))
}
fn apply(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    change: CommonControlChange,
    delta_override: Option<i32>,
) -> bool {
    let Some(c) = ctx.kernel.controls.get_mut(hwnd) else {
        return false;
    };
    match change {
        CommonControlChange::TreeSelect { item, .. } => {
            if item != 0 && !c.tree.items.contains_key(&item) {
                return false;
            }
            c.tree.selected = item;
            c.tree.ensure_visible(item, c.h);
        }
        CommonControlChange::TreeExpand { item, action } => {
            let Some(node) = c.tree.items.get_mut(&item) else {
                return false;
            };
            if action & 3 == 2 {
                node.state |= 0x60;
            } else {
                node.state &= !0x20;
            }
            if action & 0x8000 != 0 {
                let children = node.children.clone();
                for h in children {
                    c.tree.remove(h);
                }
                if let Some(node) = c.tree.items.get_mut(&item) {
                    node.state &= !0x40;
                }
            }
        }
        CommonControlChange::UpDown { delta } => {
            c.updown.position = c
                .updown
                .step(delta_override.unwrap_or(delta), c.style & 1 != 0);
            sync_buddy(ctx, hwnd);
        }
    }
    repaint_controls(ctx);
    true
}
pub(super) fn dispatch(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    msg: u32,
    wp: u32,
    lp: u32,
    modal: bool,
) -> Result<Option<DispatchOutcome>, KernelError> {
    let Some(c) = ctx.kernel.controls.get(hwnd).cloned() else {
        return Ok(None);
    };
    let change = match (c.class, msg) {
        (ControlClass::TreeView, 0x110b) if wp == 9 => CommonControlChange::TreeSelect {
            item: lp,
            reason: 0,
        },
        (ControlClass::TreeView, 0x1102) => CommonControlChange::TreeExpand {
            item: lp,
            action: wp,
        },
        (ControlClass::TreeView, CHANGE) if wp as i32 == -450 => CommonControlChange::TreeSelect {
            item: lp,
            reason: 1,
        },
        (ControlClass::TreeView, CHANGE) if wp as i32 == -454 => CommonControlChange::TreeExpand {
            item: lp,
            action: 3,
        },
        (ControlClass::UpDown, CHANGE) if wp as i32 == -722 => {
            CommonControlChange::UpDown { delta: lp as i32 }
        }
        _ => return Ok(None),
    };
    let result = u32::from(msg != CHANGE);
    let thunk_va = ctx.thunk.thunk_va;
    let done = |value| {
        if modal {
            DispatchOutcome::JumpTo(thunk_va)
        } else {
            DispatchOutcome::ReturnedR0(value)
        }
    };
    let change = match change {
        CommonControlChange::TreeSelect { item, reason } => {
            if item != 0 && !c.tree.items.contains_key(&item) {
                return Ok(Some(done(0)));
            }
            if c.tree.selected == item {
                return Ok(Some(done(result)));
            }
            CommonControlChange::TreeSelect { item, reason }
        }
        CommonControlChange::TreeExpand { item, action } => {
            let Some(node) = c.tree.items.get(&item) else {
                return Ok(Some(done(0)));
            };
            let action = if action & 3 == 3 {
                if node.state & 0x20 != 0 {
                    1
                } else {
                    2
                }
            } else {
                action
            };
            if !matches!(action & 3, 1 | 2)
                || action & 3 == 2 && node.children.is_empty() && node.children_hint <= 0
            {
                return Ok(Some(done(0)));
            }
            if action & 0x8000 == 0 && (action & 3 == 2) == (node.state & 0x20 != 0) {
                return Ok(Some(done(result)));
            }
            CommonControlChange::TreeExpand { item, action }
        }
        CommonControlChange::UpDown { delta } => {
            if let Some(value) = buddy_value(ctx, hwnd) {
                ctx.kernel.controls.get_mut(hwnd).unwrap().updown.position = value;
            }
            CommonControlChange::UpDown { delta }
        }
    };
    let proc = ctx.kernel.window_procs.get(&c.parent).copied().unwrap_or(0);
    if proc == 0 {
        let value = u32::from(apply(ctx, hwnd, change, None)) * result;
        return Ok(Some(if modal {
            DispatchOutcome::JumpTo(ctx.thunk.thunk_va)
        } else {
            DispatchOutcome::ReturnedR0(value)
        }));
    }
    let frame = GuestCallFrame {
        args: [
            ctx.cpu.read_reg(ArmReg::R0)?,
            ctx.cpu.read_reg(ArmReg::R1)?,
            ctx.cpu.read_reg(ArmReg::R2)?,
            ctx.cpu.read_reg(ArmReg::R3)?,
        ],
        sp: ctx.cpu.read_reg(ArmReg::Sp)?,
        lr: ctx.cpu.read_reg(ArmReg::Lr)?,
    };
    let DispatchOutcome::ReturnedR0(scratch) = do_alloc(ctx, 104)? else {
        return Ok(Some(DispatchOutcome::ReturnedR0(0)));
    };
    if scratch == 0 {
        return Ok(Some(DispatchOutcome::ReturnedR0(0)));
    }
    let old_item = if matches!(change, CommonControlChange::TreeSelect { .. }) {
        c.tree.items.get(&c.tree.selected).cloned().map(|mut item| {
            item.state = c.tree.item_state(&item);
            item
        })
    } else {
        None
    };
    let f = CommonControlFrame {
        thread: ctx.kernel.current_thread,
        thunk: ctx.thunk.thunk_va,
        frame,
        hwnd,
        change,
        scratch,
        old_item,
        stage: 0,
        modal,
        result,
    };
    Ok(Some(callback(ctx, f, proc, c.parent, c.id)?))
}
pub(super) fn resume(ctx: &mut CallCtx<'_>) -> Result<Option<DispatchOutcome>, KernelError> {
    let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
    if !ctx
        .kernel
        .controls
        .common_control_frames
        .last()
        .is_some_and(|f| {
            f.thread == ctx.kernel.current_thread
                && f.thunk == ctx.thunk.thunk_va
                && f.frame.sp == sp
        })
    {
        return Ok(None);
    }
    let mut f = ctx.kernel.controls.common_control_frames.pop().unwrap();
    if f.stage == 0 {
        if let Some(c) = ctx.kernel.controls.get(f.hwnd).cloned() {
            let veto = if ctx.kernel.controls.panel(c.parent).is_some() {
                ctx.kernel
                    .window_extra
                    .get(&(c.parent, 0))
                    .copied()
                    .unwrap_or(0)
                    != 0
            } else {
                ctx.cpu.read_reg(ArmReg::R0)? != 0
            };
            let delta = if matches!(f.change, CommonControlChange::UpDown { .. }) {
                Some(ctx.cpu.read_u32_le(f.scratch + 16)? as i32)
            } else {
                None
            };
            if !veto && apply(ctx, f.hwnd, f.change, delta) {
                if matches!(f.change, CommonControlChange::UpDown { .. }) {
                    let c = ctx.kernel.controls.get(f.hwnd).unwrap();
                    let msg = if c.style & 0x40 != 0 { 0x114 } else { 0x115 };
                    let pos = (c.updown.position as u32 & 0xffff) << 16;
                    if ctx.kernel.posted_messages.len() < 254 {
                        ctx.kernel
                            .posted_messages
                            .push_back((c.parent, msg, pos | 4, f.hwnd));
                        ctx.kernel
                            .posted_messages
                            .push_back((c.parent, msg, 8, f.hwnd));
                    }
                } else if let Some(proc) = ctx
                    .kernel
                    .window_procs
                    .get(&c.parent)
                    .copied()
                    .filter(|p| *p != 0)
                {
                    f.stage = 1;
                    return Ok(Some(callback(ctx, f, proc, c.parent, c.id)?));
                }
            } else {
                f.result = 0;
            }
        } else {
            f.result = 0;
        }
    }
    do_free(ctx, f.scratch);
    for (r, value) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
        .into_iter()
        .zip(f.frame.args)
    {
        ctx.cpu.write_reg(r, value)?;
    }
    ctx.cpu.write_reg(ArmReg::Sp, f.frame.sp)?;
    ctx.cpu.write_reg(ArmReg::Lr, f.frame.lr)?;
    Ok(Some(if f.modal {
        DispatchOutcome::JumpTo(f.thunk)
    } else {
        DispatchOutcome::ReturnedR0(f.result)
    }))
}
