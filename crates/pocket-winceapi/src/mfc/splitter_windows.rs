//! CSplitterWnd construction; unported splitter behavior remains explicit.
use super::callbacks::{virtual_function, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) const COMMON: &str = "__PocketHLE_MFC_SplitterCreateCommon";
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", COMMON, create_common);
}
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (1619, "?IdFromRowCol@CSplitterWnd@@QBAHHH@Z", pane_id),
    (2332, "?SetColumnInfo@CSplitterWnd@@QAAXHHH@Z", set_column),
    (2402, "?SetRowInfo@CSplitterWnd@@QAAXHHH@Z", set_row),
    (
        985,
        "?CreateStatic@CSplitterWnd@@QAAHPAVCWnd@@HHKI@Z",
        create_static,
    ),
    (
        935,
        "?CreateCommon@CSplitterWnd@@IAAHPAVCWnd@@UtagSIZE@@KI@Z",
        create_common,
    ),
    (292, "??1CSplitterWnd@@UAA@XZ", destruct),
    (2024, "?OnSize@CSplitterWnd@@IAAXIHH@Z", on_size),
    (169, "??0CSplitterWnd@@QAA@XZ", construct),
    (
        1523,
        "?GetRuntimeClass@CSplitterWnd@@UBAPAUCRuntimeClass@@XZ",
        runtime,
    ),
];
fn pane_id(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let row = ctx.arg_u32(1)?;
    let column = ctx.arg_u32(2)?;
    Ok(DispatchOutcome::ReturnedR0(
        0xe900u32
            .wrapping_add(row.wrapping_shl(4))
            .wrapping_add(column),
    ))
}
fn set_column(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    set_info(ctx, 0x84, 0x50)
}
fn set_row(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    set_info(ctx, 0x88, 0x4c)
}
fn set_info(
    ctx: &mut CallCtx<'_>,
    array_slot: u32,
    count_slot: u32,
) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    let index = ctx.arg_u32(1)?;
    let ideal = ctx.arg_u32(2)?;
    let min = ctx.arg_u32(3)?;
    let count = word(ctx, object + count_slot)?;
    let array = word(ctx, object + array_slot)?;
    if !(1..=16).contains(&count)
        || index >= count
        || ctx
            .kernel
            .heap
            .msize(array)
            .is_none_or(|size| size < count * 12)
    {
        return Err(KernelError::Dispatch(
            "Invalid owned MFC splitter row/column".into(),
        ));
    }
    let entry = array + index * 12;
    ctx.cpu
        .check_guest_access(entry, 12, Prot::READ | Prot::WRITE)?;
    put(ctx, entry + 4, ideal)?;
    put(ctx, entry, min)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn construct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let object = ctx.arg_u32(0)?;
    super::windows::construct_target_class(ctx, "CSplitterWnd", 208)?;
    // The CE3 AUX_DATA constructor initializes bWin4 to TRUE, independent of
    // host OS/version. The retail splitter constructor uses this fixed branch.
    // These are framework logical-pixel defaults, not fabricated host metrics.
    for (offset, value) in [
        (0x54, 7),
        (0x58, 7),
        (0x5c, 1),
        (0x60, 1),
        (0x64, 7),
        (0x68, 7),
        (0x6c, 1),
        (0x70, 1),
    ] {
        put(ctx, object + offset, value)?;
    }
    Ok(DispatchOutcome::ReturnedR0(object))
}
fn runtime(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(super::classes::runtime(
        ctx,
        "CSplitterWnd",
    )?))
}

fn create_static(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if !f.fresh {
        let result = ctx.cpu.read_reg(ArmReg::R0)?;
        return f.finish(ctx, u32::from(result != 0));
    }
    ctx.cpu
        .check_guest_access(o, 208, Prot::READ | Prot::WRITE)?;
    let rows = f.arg(ctx, 2)?;
    let columns = f.arg(ctx, 3)?;
    if !(1..=16).contains(&rows) || !(1..=16).contains(&columns) {
        return f.finish(ctx, 0);
    }
    if word(ctx, o + 0x20)? != 0 || word(ctx, o + 0x84)? != 0 || word(ctx, o + 0x88)? != 0 {
        return f.finish(ctx, 0);
    }
    let parent = f.arg(ctx, 1)?;
    let style = ctx.arg_u32(4)?;
    let id = ctx.arg_u32(5)?;
    for (offset, value) in [(0x4c, rows), (0x50, columns), (0x74, rows), (0x78, columns)] {
        put(ctx, o + offset, value)?;
    }
    f.call_service(ctx, 1, COMMON, &[o, parent, 0, 0, style, id])
}
fn create_common(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu
            .check_guest_access(o, 208, Prot::READ | Prot::WRITE)?;
        let rows = word(ctx, o + 0x4c)?;
        let columns = word(ctx, o + 0x50)?;
        if !(1..=16).contains(&rows)
            || !(1..=16).contains(&columns)
            || word(ctx, o + 0x20)? != 0
            || word(ctx, o + 0x84)? != 0
            || word(ctx, o + 0x88)? != 0
        {
            return f.finish(ctx, 0);
        }
        let style = ctx.arg_u32(4)?;
        let id = ctx.arg_u32(5)?;
        f.set(ctx, 2, style)?;
        let class = super::frame_loading::standard_class_spec(ctx, "AfxMDIFrame42u", 4, 8, 0)?;
        // Reserve both arrays transactionally. They stay unpublished until the
        // real window callbacks finish, matching their native visibility point.
        let columns_array = alloc_cpp(ctx, columns * 12)?;
        let rows_array = match alloc_cpp(ctx, rows * 12) {
            Ok(p) => p,
            Err(e) => {
                ctx.kernel.heap.free(columns_array);
                return Err(e);
            }
        };
        f.set(ctx, 0, columns_array)?;
        f.set(ctx, 1, rows_array)?;
        let cx = f.arg(ctx, 2)?;
        let cy = f.arg(ctx, 3)?;
        for (array, count, min) in [(columns_array, columns, cx), (rows_array, rows, cy)] {
            for index in 0..count {
                for (offset, value) in [(0, min), (4, min), (8, u32::MAX)] {
                    put(ctx, array + index * 12 + offset, value)?;
                }
            }
        }
        let parent = f.arg(ctx, 1)?;
        let parent_hwnd = word(ctx, parent + 0x20)?;
        let style = style & !0x00b00000;
        return f.call_service(
            ctx,
            1,
            super::window_creation::CREATE_EX,
            &[o, 0, class, 0, style, 0, 0, 0, 0, parent_hwnd, id, 0],
        );
    }
    if f.phase(ctx)? == 2 {
        return f.finish(ctx, 1);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    let columns = f.get(ctx, 0)?;
    let rows = f.get(ctx, 1)?;
    if result == 0 {
        ctx.kernel.heap.free(columns);
        ctx.kernel.heap.free(rows);
        return f.finish(ctx, 0);
    }
    put(ctx, o + 0x84, columns)?;
    put(ctx, o + 0x88, rows)?;
    let style = f.get(ctx, 2)?;
    f.call_service(ctx, 2, super::splitter_scroll::STYLE, &[o, style])
}

fn on_size(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        if f.arg(ctx, 1)? != 1 && (f.arg(ctx, 2)? as i32) > 0 && (f.arg(ctx, 3)? as i32) > 0 {
            let target = virtual_function(ctx, o, 47)?;
            return f.call_args(ctx, 1, target, &[o]);
        }
    }
    if f.fresh || f.phase(ctx)? == 1 {
        return f.call_service(ctx, 2, "__PocketHLE_MFC_WindowDefault", &[o]);
    }
    f.finish(ctx, 0)
}
fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct_impl(ctx, false)
}
pub(super) fn deleting(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    destruct_impl(ctx, true)
}
fn destruct_impl(ctx: &mut CallCtx<'_>, deleting: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if !f.fresh {
        let free = f.get(ctx, 0)? != 0;
        if free {
            ctx.kernel.heap.free(o);
        }
        return f.finish(ctx, o);
    }
    ctx.cpu
        .check_guest_access(o, 208, Prot::READ | Prot::WRITE)?;
    let flags = f.arg(ctx, 1)?;
    f.set(ctx, 0, u32::from(deleting && flags & 1 != 0))?;
    let mut arrays = Vec::new();
    for (slot, count_slot) in [(0x84, 0x50), (0x88, 0x4c)] {
        let array = word(ctx, o + slot)?;
        if array != 0 {
            let count = word(ctx, o + count_slot)?;
            if !(1..=16).contains(&count)
                || ctx
                    .kernel
                    .heap
                    .msize(array)
                    .is_none_or(|size| size < count * 12)
                || arrays.contains(&array)
            {
                return Err(KernelError::Dispatch(
                    "Invalid owned MFC splitter array".into(),
                ));
            }
            arrays.push(array);
        }
    }
    for array in arrays {
        ctx.kernel.heap.free(array);
    }
    put(ctx, o + 0x84, 0)?;
    put(ctx, o + 0x88, 0)?;
    f.call_service(ctx, 1, super::window_lifetime::DESTRUCT, &[o])
}
#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn static_splitter_owns_independent_arrays_and_repeated_teardown_returns_every_allocation() {
        use super::super::window_dispatch::tests::{drive, entry};
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2000],
        );
        k.window_classes.insert(7, "Parent".into());
        cpu.write_mem(0x2020, &7u32.to_le_bytes()).unwrap();
        for _ in 0..2 {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:169",
                &[0x2400],
            );
            for (i, v) in [0x2400, 0x2000, 2, 3, 0x50800000, 0xe900]
                .into_iter()
                .enumerate()
            {
                if i < 4 {
                    cpu.write_arg(i as u8, v).unwrap();
                } else {
                    cpu.write_mem(0x4000 + (i as u32 - 4) * 4, &v.to_le_bytes())
                        .unwrap();
                }
            }
            assert_eq!(
                drive(&mut cpu, &mut k, &mut d, entry(985), |pc, _| panic!(
                    "unexpected guest {pc:x}"
                )),
                1
            );
            let columns = cpu.read_u32_le(0x2484).unwrap();
            let rows = cpu.read_u32_le(0x2488).unwrap();
            assert_ne!(columns, rows);
            assert!(k.heap.msize(columns).unwrap() >= 36);
            assert!(k.heap.msize(rows).unwrap() >= 24);
            assert_eq!(cpu.read_u32_le(columns + 8).unwrap(), u32::MAX);
            assert_eq!(cpu.read_u32_le(rows + 8).unwrap(), u32::MAX);
            for (ordinal, array, index) in [(2332, columns, 2), (2402, rows, 1)] {
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "mfcce300.dll",
                    &format!("ord:{ordinal}"),
                    &[0x2400, index, 120, 5],
                );
                assert_eq!(cpu.read_u32_le(array + index * 12).unwrap(), 5);
                assert_eq!(cpu.read_u32_le(array + index * 12 + 4).unwrap(), 120);
                assert_eq!(cpu.read_u32_le(array + index * 12 + 8).unwrap(), u32::MAX);
            }
            let hwnd = cpu.read_u32_le(0x2420).unwrap();
            assert_ne!(hwnd, 0);
            assert_eq!(k.window_parents[&hwnd], 7);
            assert_eq!(k.window_styles[&hwnd].0 & 0x00b00000, 0);
            cpu.write_arg(0, 0x2400).unwrap();
            drive(&mut cpu, &mut k, &mut d, entry(292), |pc, _| {
                panic!("unexpected guest {pc:x}")
            });
            assert!(k.heap.msize(columns).is_none());
            assert!(k.heap.msize(rows).is_none());
            assert!(!k.window_classes.contains_key(&hwnd));
            assert_eq!(cpu.read_mem(0x2484, 8).unwrap(), [0; 8]);
        }
    }
    #[test]
    fn invalid_static_dimensions_do_not_publish_a_window_or_allocate_owned_arrays() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:169",
            &[0x2000],
        );
        // Warm continuation ownership without creating any OS window.
        for rows in [0, 17, u32::MAX] {
            let outcome = call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:985",
                &[0x2000, 0x2400, rows, 1, 0x50000000, 0xe900],
            );
            assert_eq!(result(outcome), 0);
            assert_eq!(cpu.read_u32_le(0x2020).unwrap(), 0);
            assert_eq!(cpu.read_mem(0x2084, 8).unwrap(), [0; 8]);
        }
    }
    #[test]
    fn splitter_constructor_retains_base_state_ce3_layout_and_the_full_virtual_table() {
        let (mut cpu, mut k, mut d) = framework_setup();
        cpu.write_mem(0x2000, &[0x5a; 212]).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:169",
            &[0x2000],
        );
        assert_eq!(cpu.read_u32_le(0x2004).unwrap(), 1);
        assert_eq!(cpu.read_u32_le(0x2020).unwrap(), 0);
        assert_eq!(cpu.read_u32_le(0x2054).unwrap(), 7);
        assert_eq!(cpu.read_u32_le(0x2060).unwrap(), 1);
        assert_eq!(cpu.read_u32_le(0x20cc).unwrap(), 0);
        assert_eq!(cpu.read_mem(0x20d0, 4).unwrap(), [0x5a; 4]);
        let table = cpu.read_u32_le(0x2000).unwrap();
        assert_eq!(
            cpu.read_u32_le(table + 71 * 4).unwrap(),
            k.dynamic_exports[&1]["__PocketHLE_MFC_Class:CSplitterWnd:71"]
        );
        let descriptor = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1523",
            &[0x2000],
        ));
        assert_eq!(cpu.read_u32_le(descriptor + 4).unwrap(), 208);
    }
}
