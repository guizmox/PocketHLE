//! Stateful WinCE tab controls. Pages remain owned and drawn by the application.
use super::*;
use pocket_kernel::controls::{TabItem, TabNotifyFrame};

// Host-only queued request; never delivered to a guest window procedure.
pub(super) const SELECT: u32 = 0x7fff_ff01;

pub(super) fn message(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    msg: u32,
    wp: u32,
    lp: u32,
) -> Result<Option<u32>, KernelError> {
    if !ctx
        .kernel
        .controls
        .get(hwnd)
        .is_some_and(|c| c.class == ControlClass::Tab)
    {
        return Ok(None);
    }
    let result = match msg {
        0x1302 => ctx.kernel.controls.get(hwnd).unwrap().tab.image_list,
        0x1303 => {
            let t = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().tab;
            std::mem::replace(&mut t.image_list, lp)
        }
        0x1304 => ctx.kernel.controls.get(hwnd).unwrap().tab.items.len() as u32,
        0x1305 | 0x133c => {
            let Some(item) = ctx
                .kernel
                .controls
                .get(hwnd)
                .unwrap()
                .tab
                .items
                .get(wp as usize)
                .cloned()
            else {
                return Ok(Some(0));
            };
            let mask = ctx.cpu.read_u32_le(lp)?;
            if mask & 1 != 0 {
                let ptr = ctx.cpu.read_u32_le(lp + 12)?;
                let capacity = ctx.cpu.read_u32_le(lp + 16)?.min(32768) as usize;
                if ptr != 0 && capacity > 0 {
                    let bytes = if msg == 0x133c {
                        item.text
                            .encode_utf16()
                            .take(capacity - 1)
                            .chain(Some(0))
                            .flat_map(u16::to_le_bytes)
                            .collect::<Vec<_>>()
                    } else {
                        item.text
                            .bytes()
                            .take(capacity - 1)
                            .chain(Some(0))
                            .collect()
                    };
                    ctx.cpu.write_mem(ptr, &bytes)?;
                }
            }
            if mask & 2 != 0 {
                ctx.cpu.write_mem(lp + 20, &item.image.to_le_bytes())?;
            }
            if mask & 8 != 0 {
                let size = ctx.kernel.controls.get(hwnd).unwrap().tab.extra;
                let mut bytes = item.extra.clone();
                bytes.resize(size, 0);
                for (i, b) in item.data.to_le_bytes().iter().enumerate().take(size.min(4)) {
                    bytes[i] = *b;
                }
                ctx.cpu.write_mem(lp + 24, &bytes)?;
            }
            if mask & 0x10 != 0 {
                let state_mask = ctx.cpu.read_u32_le(lp + 8)?;
                ctx.cpu
                    .write_mem(lp + 4, &(item.state & state_mask).to_le_bytes())?;
            }
            1
        }
        0x1306 | 0x1307 | 0x133d | 0x133e => {
            let insert = matches!(msg, 0x1307 | 0x133e);
            let count = ctx.kernel.controls.get(hwnd).unwrap().tab.items.len();
            if (wp as i32) < 0 || (!insert && wp as usize >= count) || count >= 4096 && insert {
                return Ok(Some(u32::MAX * u32::from(insert)));
            }
            let mask = ctx.cpu.read_u32_le(lp)?;
            let mut item = if insert {
                TabItem {
                    image: -1,
                    ..Default::default()
                }
            } else {
                ctx.kernel.controls.get(hwnd).unwrap().tab.items[wp as usize].clone()
            };
            if mask & 1 != 0 {
                let ptr = ctx.cpu.read_u32_le(lp + 12)?;
                item.text = if ptr == 0 || ptr == u32::MAX {
                    String::new()
                } else if msg >= 0x133d {
                    String::from_utf16_lossy(&read_wstr(ctx, ptr, 32768)?)
                        .trim_end_matches('\0')
                        .to_string()
                } else {
                    crate::crt_ansi::decode(&read_cstr(ctx, ptr, 32768)?)
                };
            }
            if mask & 2 != 0 {
                item.image = ctx.cpu.read_u32_le(lp + 20)? as i32;
            }
            if mask & 8 != 0 {
                let size = ctx.kernel.controls.get(hwnd).unwrap().tab.extra;
                item.extra = ctx.cpu.read_mem(lp + 24, size as u32)?;
                let mut value = [0; 4];
                let n = size.min(4);
                value[..n].copy_from_slice(&item.extra[..n]);
                item.data = u32::from_le_bytes(value);
            }
            if mask & 0x10 != 0 {
                let m = ctx.cpu.read_u32_le(lp + 8)?;
                item.state = (item.state & !m) | (ctx.cpu.read_u32_le(lp + 4)? & m);
            }
            let t = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().tab;
            if insert {
                let index = (wp as usize).min(t.items.len());
                t.items.insert(index, item);
                if t.items.len() == 1 {
                    t.selected = Some(0);
                    t.focus = Some(0);
                } else {
                    if let Some(i) = &mut t.selected {
                        if *i >= index {
                            *i += 1;
                        }
                    }
                    if let Some(i) = &mut t.focus {
                        if *i >= index {
                            *i += 1;
                        }
                    }
                }
                index as u32
            } else {
                t.items[wp as usize] = item;
                1
            }
        }
        0x1308 => {
            let t = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().tab;
            let index = wp as usize;
            if index >= t.items.len() {
                return Ok(Some(0));
            }
            t.items.remove(index);
            t.selected = t.selected.and_then(|i| {
                if i == index {
                    None
                } else {
                    Some(i - usize::from(i > index))
                }
            });
            t.focus = t.focus.and_then(|i| {
                if i == index {
                    None
                } else {
                    Some(i - usize::from(i > index))
                }
            });
            1
        }
        0x1309 => {
            let t = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().tab;
            t.items.clear();
            t.selected = None;
            t.focus = None;
            1
        }
        0x130a => {
            let Some((x, y, w, h)) = ctx.kernel.controls.get(hwnd).unwrap().tab_rect(wp as usize)
            else {
                return Ok(Some(0));
            };
            write_rect_at(ctx, lp, [x, y, x + w, y + h])?;
            1
        }
        0x130b => ctx
            .kernel
            .controls
            .get(hwnd)
            .unwrap()
            .tab
            .selected
            .map_or(u32::MAX, |i| i as u32),
        0x130c => {
            let t = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().tab;
            let old = t.selected.map_or(u32::MAX, |i| i as u32);
            if wp == u32::MAX {
                t.selected = None;
            } else if (wp as usize) < t.items.len() {
                t.selected = Some(wp as usize);
                t.focus = t.selected;
            }
            // Programmatic selection never sends TCN_SELCHANGE.
            old
        }
        0x130d => {
            let x = ctx.cpu.read_u32_le(lp)? as i32;
            let y = ctx.cpu.read_u32_le(lp + 4)? as i32;
            let i = ctx.kernel.controls.get(hwnd).unwrap().tab_hit(x, y);
            ctx.cpu
                .write_mem(lp + 8, &if i.is_some() { 4u32 } else { 1u32 }.to_le_bytes())?;
            i.map_or(u32::MAX, |i| i as u32)
        }
        0x130e => {
            let t = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().tab;
            if !t.items.is_empty() || wp > 4096 {
                return Ok(Some(0));
            }
            t.extra = wp as usize;
            1
        }
        0x1328 => {
            let c = ctx.kernel.controls.get(hwnd).unwrap();
            let strip = c.tab_rows() * c.tab.size.1.max(1);
            let bottom = c.style & 2 != 0;
            let inset = if bottom {
                [2, 2, -2, -strip - 2]
            } else {
                [2, strip + 2, -2, -2]
            };
            let sign = if wp == 0 { 1i32 } else { -1 };
            let mut rect = [0; 4];
            for i in 0..4 {
                rect[i] = (ctx.cpu.read_u32_le(lp + i as u32 * 4)? as i32)
                    .saturating_add(sign * inset[i]);
            }
            write_rect_at(ctx, lp, rect)?;
            0
        }
        0x1329 => {
            let t = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().tab;
            let old = (t.size.0 as u32 & 0xffff) | ((t.size.1 as u32 & 0xffff) << 16);
            t.size = ((lp & 0xffff) as i32, ((lp >> 16) as i32).max(1));
            old
        }
        0x132a => {
            for item in &mut ctx.kernel.controls.get_mut(hwnd).unwrap().tab.items {
                if item.image == wp as i32 {
                    item.image = -1;
                } else if item.image > wp as i32 {
                    item.image -= 1;
                }
            }
            0
        }
        0x132b => {
            ctx.kernel.controls.get_mut(hwnd).unwrap().tab.padding =
                ((lp & 0xffff) as i32, (lp >> 16) as i32);
            0
        }
        0x132c => ctx.kernel.controls.get(hwnd).unwrap().tab_rows() as u32,
        0x132d => ctx
            .kernel
            .window_extra
            .get(&(hwnd, -1337))
            .copied()
            .unwrap_or(0),
        0x132e => {
            ctx.kernel.window_extra.insert((hwnd, -1337), wp);
            0
        }
        0x132f => ctx
            .kernel
            .controls
            .get(hwnd)
            .unwrap()
            .tab
            .focus
            .map_or(u32::MAX, |i| i as u32),
        0x1330 => {
            let c = ctx.kernel.controls.get_mut(hwnd).unwrap();
            if (wp as usize) < c.tab.items.len() {
                c.tab.focus = Some(wp as usize);
                ctx.kernel.posted_messages.push_back((hwnd, SELECT, wp, 0));
            }
            0
        }
        0x1331 => {
            let t = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().tab;
            let old = t.minimum_width;
            t.minimum_width = if lp == u32::MAX {
                0
            } else {
                (lp as i32).max(0)
            };
            old as u32
        }
        0x1332 => {
            let t = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().tab;
            for (i, item) in t.items.iter_mut().enumerate() {
                if wp == 0 || t.selected != Some(i) {
                    item.state &= !1;
                }
            }
            0
        }
        0x1333 => {
            let t = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().tab;
            let Some(item) = t.items.get_mut(wp as usize) else {
                return Ok(Some(0));
            };
            item.state = (item.state & !2) | if lp & 0xffff != 0 { 2 } else { 0 };
            1
        }
        0x1334 => {
            let t = &mut ctx.kernel.controls.get_mut(hwnd).unwrap().tab;
            let old = t.extended_style;
            let mask = if wp == 0 { u32::MAX } else { wp };
            t.extended_style = (old & !mask) | (lp & mask);
            old
        }
        0x1335 => ctx.kernel.controls.get(hwnd).unwrap().tab.extended_style,
        _ => return Ok(None),
    };
    repaint_controls(ctx);
    Ok(Some(result))
}
fn write_rect_at(ctx: &mut CallCtx<'_>, ptr: u32, r: [i32; 4]) -> Result<(), KernelError> {
    ctx.cpu.write_mem(
        ptr,
        &r.into_iter().flat_map(i32::to_le_bytes).collect::<Vec<_>>(),
    )?;
    Ok(())
}

/// Deliver selection notifications synchronously, including the parent's veto.
pub(super) fn dispatch(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    msg: u32,
    index: u32,
    modal: bool,
) -> Result<Option<DispatchOutcome>, KernelError> {
    if msg != SELECT {
        return Ok(None);
    }
    let Some(c) = ctx
        .kernel
        .controls
        .get(hwnd)
        .filter(|c| c.class == ControlClass::Tab)
        .cloned()
    else {
        return Ok(Some(if modal {
            DispatchOutcome::JumpTo(ctx.thunk.thunk_va)
        } else {
            DispatchOutcome::ReturnedR0(0)
        }));
    };
    if index as usize >= c.tab.items.len() || c.tab.selected == Some(index as usize) {
        return Ok(Some(if modal {
            DispatchOutcome::JumpTo(ctx.thunk.thunk_va)
        } else {
            DispatchOutcome::ReturnedR0(0)
        }));
    }
    let proc = ctx.kernel.window_procs.get(&c.parent).copied().unwrap_or(0);
    if proc == 0 {
        ctx.kernel.controls.get_mut(hwnd).unwrap().tab.selected = Some(index as usize);
        repaint_controls(ctx);
        return Ok(Some(if modal {
            DispatchOutcome::JumpTo(ctx.thunk.thunk_va)
        } else {
            DispatchOutcome::ReturnedR0(0)
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
    let DispatchOutcome::ReturnedR0(scratch) = do_alloc(ctx, 12)? else {
        return Ok(Some(if modal {
            DispatchOutcome::JumpTo(ctx.thunk.thunk_va)
        } else {
            DispatchOutcome::ReturnedR0(0)
        }));
    };
    if scratch == 0 {
        return Ok(Some(if modal {
            DispatchOutcome::JumpTo(ctx.thunk.thunk_va)
        } else {
            DispatchOutcome::ReturnedR0(0)
        }));
    }
    let state = TabNotifyFrame {
        thread: ctx.kernel.current_thread,
        thunk: ctx.thunk.thunk_va,
        frame,
        hwnd,
        index: index as usize,
        scratch,
        stage: 0,
        modal,
    };
    Ok(Some(callback(ctx, state, proc, c.parent, c.id, -552)?))
}
fn callback(
    ctx: &mut CallCtx<'_>,
    state: TabNotifyFrame,
    proc: u32,
    parent: u32,
    id: u32,
    code: i32,
) -> Result<DispatchOutcome, KernelError> {
    ctx.cpu.write_mem(
        state.scratch,
        &[state.hwnd, id, code as u32]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
    )?;
    // Dialog procedures return WM_NOTIFY results through DWL_MSGRESULT.
    if ctx.kernel.controls.panel(parent).is_some() {
        ctx.kernel.window_extra.insert((parent, 0), 0);
    }
    ctx.cpu.write_reg(ArmReg::R0, parent)?;
    ctx.cpu.write_reg(ArmReg::R1, 0x4e)?;
    ctx.cpu.write_reg(ArmReg::R2, id)?;
    ctx.cpu.write_reg(ArmReg::R3, state.scratch)?;
    ctx.cpu.write_reg(ArmReg::Lr, state.thunk)?;
    ctx.kernel.controls.tab_notify_frames.push(state);
    Ok(DispatchOutcome::JumpTo(proc))
}
pub(super) fn resume(ctx: &mut CallCtx<'_>) -> Result<Option<DispatchOutcome>, KernelError> {
    let sp = ctx.cpu.read_reg(ArmReg::Sp)?;
    if !ctx
        .kernel
        .controls
        .tab_notify_frames
        .last()
        .is_some_and(|f| {
            f.thread == ctx.kernel.current_thread
                && f.thunk == ctx.thunk.thunk_va
                && f.frame.sp == sp
        })
    {
        return Ok(None);
    }
    let mut f = ctx.kernel.controls.tab_notify_frames.pop().unwrap();
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
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
                result != 0
            };
            if !veto && f.index < c.tab.items.len() {
                let t = &mut ctx.kernel.controls.get_mut(f.hwnd).unwrap().tab;
                t.selected = Some(f.index);
                t.focus = t.selected;
                repaint_controls(ctx);
                if let Some(proc) = ctx
                    .kernel
                    .window_procs
                    .get(&c.parent)
                    .copied()
                    .filter(|p| *p != 0)
                {
                    f.stage = 1;
                    return Ok(Some(callback(ctx, f, proc, c.parent, c.id, -551)?));
                }
            } else if veto {
                ctx.kernel.controls.get_mut(f.hwnd).unwrap().tab.focus = c.tab.selected;
            }
        }
    }
    do_free(ctx, f.scratch);
    for (r, v) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
        .into_iter()
        .zip(f.frame.args)
    {
        ctx.cpu.write_reg(r, v)?;
    }
    ctx.cpu.write_reg(ArmReg::Sp, f.frame.sp)?;
    ctx.cpu.write_reg(ArmReg::Lr, f.frame.lr)?;
    Ok(Some(if f.modal {
        DispatchOutcome::JumpTo(f.thunk)
    } else {
        DispatchOutcome::ReturnedR0(0)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_cpu::Cpu;
    fn create(ctx: &mut CallCtx<'_>) -> u32 {
        ctx.kernel.controls.create(
            FAKE_HWND,
            ControlClass::Tab,
            504,
            String::new(),
            0x50000200,
            2,
            24,
            200,
            90,
        )
    }
    fn insert(ctx: &mut CallCtx<'_>, hwnd: u32, index: u32, label: &str) {
        let text: Vec<u8> = label
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        ctx.cpu.write_mem(0x1800, &text).unwrap();
        ctx.cpu
            .write_mem(
                0x1700,
                &[9u32, 0, 0, 0x1800, 40, 0, 0x12340000 + index]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        assert_eq!(
            message(ctx, hwnd, 0x133e, index, 0x1700).unwrap(),
            Some(index)
        );
    }
    #[test]
    fn unicode_items_data_rectangles_and_programmatic_selection() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut kernel = super::super::tests::fresh_kernel();
        let thunk = super::super::tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        let hwnd = create(&mut ctx);
        assert_eq!(
            ControlClass::from_class_name("systabcontrol32"),
            Some(ControlClass::Tab)
        );
        insert(&mut ctx, hwnd, 0, "Income");
        insert(&mut ctx, hwnd, 1, "Expenses");
        assert_eq!(message(&mut ctx, hwnd, 0x1304, 0, 0).unwrap(), Some(2));
        assert_eq!(message(&mut ctx, hwnd, 0x130b, 0, 0).unwrap(), Some(0));
        assert_eq!(message(&mut ctx, hwnd, 0x130c, 1, 0).unwrap(), Some(0));
        assert_eq!(message(&mut ctx, hwnd, 0x130b, 0, 0).unwrap(), Some(1));
        assert!(ctx.kernel.posted_messages.is_empty());
        assert_eq!(message(&mut ctx, hwnd, 0x133c, 1, 0x1700).unwrap(), Some(1));
        assert_eq!(ctx.cpu.read_u32_le(0x1718).unwrap(), 0x12340001);
        assert_eq!(
            String::from_utf16_lossy(&read_wstr(&mut ctx, 0x1800, 40).unwrap())
                .trim_end_matches('\0'),
            "Expenses"
        );
        assert_eq!(message(&mut ctx, hwnd, 0x130a, 1, 0x1900).unwrap(), Some(1));
        let c = ctx.kernel.controls.get(hwnd).unwrap();
        let (x, y, w, h) = c.tab_rect(1).unwrap();
        assert_eq!(ctx.cpu.read_u32_le(0x1900).unwrap(), x as u32);
        ctx.cpu
            .write_mem(
                0x1920,
                &[x + 1, y + 1, 0]
                    .into_iter()
                    .flat_map(i32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        assert_eq!(message(&mut ctx, hwnd, 0x130d, 0, 0x1920).unwrap(), Some(1));
        assert!(w > 0 && h > 0);
        ctx.cpu
            .write_mem(
                0x1900,
                &[0i32, 0, 200, 90]
                    .into_iter()
                    .flat_map(i32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        message(&mut ctx, hwnd, 0x1328, 0, 0x1900).unwrap();
        assert!(ctx.cpu.read_u32_le(0x1904).unwrap() > 0);
        message(&mut ctx, hwnd, 0x1328, 1, 0x1900).unwrap();
        assert_eq!(
            ctx.cpu.read_mem(0x1900, 16).unwrap(),
            [0i32, 0, 200, 90]
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        message(&mut ctx, hwnd, 0x1308, 0, 0).unwrap();
        assert_eq!(message(&mut ctx, hwnd, 0x130b, 0, 0).unwrap(), Some(0));
    }
    #[test]
    fn selection_veto_and_two_notifications_restore_the_calling_frame() {
        for (veto, dialog) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut cpu = super::super::tests::scanf_cpu();
            cpu.map_region(
                0x50000000,
                0x10000,
                pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
            )
            .unwrap();
            let mut kernel = super::super::tests::fresh_kernel();
            let thunk = super::super::tests::dummy_thunk();
            kernel.window_procs.insert(FAKE_HWND, 0x123400);
            if dialog {
                kernel
                    .controls
                    .add_panel(pocket_kernel::controls::DialogPanel {
                        hwnd: FAKE_HWND,
                        x: 0,
                        y: 0,
                        w: 240,
                        h: 320,
                        visible: true,
                        border: true,
                        caption: Some("Budget".into()),
                        ok_pressed: false,
                        caption_height: None,
                    });
            }

            let mut ctx = CallCtx {
                cpu: &mut cpu,
                kernel: &mut kernel,
                thunk: &thunk,
            };
            let hwnd = create(&mut ctx);
            insert(&mut ctx, hwnd, 0, "Income");
            insert(&mut ctx, hwnd, 1, "Expenses");
            ctx.cpu.write_reg(ArmReg::Lr, 0x8888).unwrap();
            assert_eq!(
                dispatch(&mut ctx, hwnd, SELECT, 1, false).unwrap(),
                Some(DispatchOutcome::JumpTo(0x123400))
            );
            let ptr = ctx.cpu.read_reg(ArmReg::R3).unwrap();
            assert_eq!(ctx.cpu.read_u32_le(ptr + 8).unwrap(), (-552i32) as u32);
            assert_eq!(ctx.kernel.controls.get(hwnd).unwrap().tab.selected, Some(0));
            if dialog {
                ctx.kernel
                    .window_extra
                    .insert((FAKE_HWND, 0), u32::from(veto));
            }
            ctx.cpu
                .write_reg(ArmReg::R0, if dialog { 1 } else { u32::from(veto) })
                .unwrap();
            let outcome = resume(&mut ctx).unwrap();
            if veto {
                assert_eq!(outcome, Some(DispatchOutcome::ReturnedR0(0)));
                assert_eq!(ctx.kernel.controls.get(hwnd).unwrap().tab.selected, Some(0));
            } else {
                assert_eq!(outcome, Some(DispatchOutcome::JumpTo(0x123400)));
                assert_eq!(ctx.kernel.controls.get(hwnd).unwrap().tab.selected, Some(1));
                assert_eq!(ctx.cpu.read_u32_le(ptr + 8).unwrap(), (-551i32) as u32);
                ctx.cpu.write_reg(ArmReg::R0, 0).unwrap();
                assert_eq!(
                    resume(&mut ctx).unwrap(),
                    Some(DispatchOutcome::ReturnedR0(0))
                );
            }
            assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), 0x8888);
            assert!(ctx.kernel.controls.tab_notify_frames.is_empty());
        }
    }
    #[test]
    fn modal_pointer_and_keyboard_keep_tab_notifications() {
        let mut controls = Controls::default();
        let hwnd = controls.create(
            FAKE_HWND,
            ControlClass::Tab,
            504,
            String::new(),
            0x50000200,
            0,
            0,
            200,
            90,
        );
        let c = controls.get_mut(hwnd).unwrap();
        c.tab.items = vec![
            TabItem {
                text: "Income".into(),
                ..Default::default()
            },
            TabItem {
                text: "Expenses".into(),
                ..Default::default()
            },
        ];
        c.tab.selected = Some(0);
        let (x, y, _, _) = c.tab_rect(1).unwrap();
        assert!(matches!(
            controls.pointer_down_in(FAKE_HWND, x + 2, y + 2),
            Some(ControlAction::Notify { index: 1, .. })
        ));
        assert_eq!(controls.get(hwnd).unwrap().tab.selected, Some(0));
        // NM_CLICK is separate from TCN_SELCHANGE: applications can use either.
        assert!(matches!(controls.pointer_up_in(FAKE_HWND, x + 2, y + 2),
            Some(ControlAction::Notify { code, index: 1, .. }) if code == (-2i32) as u32));
        assert!(!matches!(
            controls.pointer_up_in(FAKE_HWND, x + 2, y + 2),
            Some(ControlAction::Notify { .. })
        ));
        controls.focus = hwnd;
        assert!(matches!(
            controls.key_down_in(FAKE_HWND, 0x27),
            Some(ControlAction::Notify { index: 1, .. })
        ));
    }
}
