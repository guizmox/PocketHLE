//! CE6 Messenger lays out its native controls with deferred window positions
//! and stores subclass state as window properties. No guest pointer is owned
//! by the property table; destruction removes only the table entries.
use super::*;
use pocket_kernel::ce::{PropertyName, WindowBatch};

pub(super) fn register(dispatcher: &mut WinCeDispatcher, dll: &str) {
    for name in ["SetProp", "SetPropW"] {
        dispatcher.register_handler(dll, name, set_prop);
    }
    for name in ["GetProp", "GetPropW"] {
        dispatcher.register_handler(dll, name, get_prop);
    }
    for name in ["RemoveProp", "RemovePropW"] {
        dispatcher.register_handler(dll, name, remove_prop);
    }
    dispatcher.register_handler(dll, "GetAssociatedMenu", associated_menu);
    dispatcher.register_handler(dll, "BeginDeferWindowPos", begin_positions);
    dispatcher.register_handler(dll, "DeferWindowPos", defer_position);
    dispatcher.register_handler(dll, "EndDeferWindowPos", end_positions);
    dispatcher.register_handler(dll, "RedrawWindow", redraw);
    dispatcher.register_handler(dll, "SetWindowRgn", set_region);
}

fn live(kernel: &KernelState, window: u32) -> bool {
    kernel.window_rects.contains_key(&window)
        || kernel.controls.get(window).is_some()
        || kernel.controls.panel(window).is_some()
}

fn fail(ctx: &mut CallCtx<'_>, error: u32) -> Result<DispatchOutcome, KernelError> {
    set_thread_error(ctx, error);
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn property(ctx: &mut CallCtx<'_>) -> Result<Option<(u32, PropertyName)>, KernelError> {
    let (window, pointer) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?);
    if !live(ctx.kernel, window) || pointer == 0 {
        set_thread_error(ctx, if pointer == 0 { 87 } else { 1400 });
        return Ok(None);
    }
    let name = if pointer <= 0xffff {
        PropertyName::Atom(pointer as u16)
    } else {
        let mut text = Vec::new();
        for index in 0u32..256 {
            let Some(address) = pointer.checked_add(index * 2) else {
                return Ok(None);
            };
            if ctx.cpu.check_guest_access(address, 2, Prot::READ).is_err() {
                set_thread_error(ctx, 87);
                return Ok(None);
            }
            let bytes = ctx.cpu.read_mem(address, 2)?;
            let unit = u16::from_le_bytes([bytes[0], bytes[1]]);
            if unit == 0 {
                return Ok(Some((window, PropertyName::Text(text))));
            }
            text.push(unit);
        }
        set_thread_error(ctx, 87);
        return Ok(None);
    };
    Ok(Some((window, name)))
}

fn set_prop(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let Some(key) = property(ctx)? else {
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    let value = ctx.arg_u32(2)?;
    ctx.kernel.ce.window_aux.properties.insert(key, value);
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn get_prop(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let Some(key) = property(ctx)? else {
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    Ok(DispatchOutcome::ReturnedR0(
        ctx.kernel
            .ce
            .window_aux
            .properties
            .get(&key)
            .copied()
            .unwrap_or(0),
    ))
}

fn remove_prop(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let Some(key) = property(ctx)? else {
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    Ok(DispatchOutcome::ReturnedR0(
        ctx.kernel
            .ce
            .window_aux
            .properties
            .remove(&key)
            .unwrap_or(0),
    ))
}

fn associated_menu(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let window = ctx.arg_u32(0)?;
    if !live(ctx.kernel, window) {
        return fail(ctx, 1400);
    }
    let menu = ctx
        .kernel
        .window_menus
        .get(&window)
        .copied()
        .filter(|menu| *menu != 0)
        .or_else(|| {
            ctx.kernel
                .gdi
                .shell_menus
                .bars
                .get(&window)
                .map(|bar| bar.menu)
        })
        .or_else(|| {
            let mut bars: Vec<_> = ctx
                .kernel
                .gdi
                .shell_menus
                .bars
                .iter()
                .filter(|(_, bar)| bar.owner == window && bar.menu != 0)
                .collect();
            bars.sort_by_key(|(handle, _)| **handle);
            bars.first().map(|(_, bar)| bar.menu)
        })
        .unwrap_or(0);
    Ok(DispatchOutcome::ReturnedR0(menu))
}

fn parent(kernel: &KernelState, window: u32) -> u32 {
    kernel
        .window_parents
        .get(&window)
        .copied()
        .or_else(|| kernel.controls.get(window).map(|control| control.parent))
        .unwrap_or(0)
}

fn begin_positions(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let count = ctx.arg_u32(0)? as i32;
    if count < 0 {
        return fail(ctx, 87);
    }
    if count > 4096 || ctx.kernel.ce.window_aux.batches.len() >= 128 {
        return fail(ctx, 8);
    }
    let Some(handle) = ctx.kernel.heap.alloc(4) else {
        return fail(ctx, 8);
    };
    if let Err(error) = ctx.cpu.write_mem(handle, &[0; 4]) {
        ctx.kernel.heap.free(handle);
        return Err(error.into());
    }
    ctx.kernel.ce.window_aux.batches.insert(
        handle,
        WindowBatch {
            parent: None,
            positions: Vec::with_capacity(count as usize),
        },
    );
    Ok(DispatchOutcome::ReturnedR0(handle))
}

fn abandon(ctx: &mut CallCtx<'_>, handle: u32, error: u32) -> Result<DispatchOutcome, KernelError> {
    if ctx.kernel.ce.window_aux.batches.remove(&handle).is_some() {
        ctx.kernel.heap.free(handle);
    }
    fail(ctx, error)
}

fn defer_position(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let values = [
        ctx.arg_u32(1)?,
        ctx.arg_u32(2)?,
        ctx.arg_u32(3)?,
        ctx.arg_u32(4)?,
        ctx.arg_u32(5)?,
        ctx.arg_u32(6)?,
        ctx.arg_u32(7)?,
    ];
    if !live(ctx.kernel, values[0]) {
        return abandon(ctx, handle, 1400);
    }
    let owner = parent(ctx.kernel, values[0]);
    let Some(batch) = ctx.kernel.ce.window_aux.batches.get_mut(&handle) else {
        return fail(ctx, 6);
    };
    if batch.parent.is_some_and(|parent| parent != owner) {
        return abandon(ctx, handle, 87);
    }
    if batch.positions.len() >= 4096 {
        return abandon(ctx, handle, 8);
    }
    batch.parent = Some(owner);
    batch.positions.push(values);
    Ok(DispatchOutcome::ReturnedR0(handle))
}

fn end_positions(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let Some(batch) = ctx.kernel.ce.window_aux.batches.remove(&handle) else {
        return fail(ctx, 6);
    };
    ctx.kernel.heap.free(handle);
    if batch.positions.iter().any(|values| {
        !live(ctx.kernel, values[0]) || batch.parent != Some(parent(ctx.kernel, values[0]))
    }) {
        return fail(ctx, 1400);
    }
    for values in batch.positions {
        set_window_pos_values(ctx, values)?;
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn redraw(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (window, rectangle, region, flags) = (
        ctx.arg_u32(0)?,
        ctx.arg_u32(1)?,
        ctx.arg_u32(2)?,
        ctx.arg_u32(3)?,
    );
    if window != 0 && !live(ctx.kernel, window) {
        return fail(ctx, 1400);
    }
    if rectangle != 0
        && ctx
            .cpu
            .check_guest_access(rectangle, 16, Prot::READ)
            .is_err()
    {
        return fail(ctx, 87);
    }
    if region != 0 && !ctx.kernel.gdi.regions.contains_key(&region) {
        return fail(ctx, 6);
    }
    if flags & !0xfff != 0 {
        return fail(ctx, 87);
    }
    let mut windows: Vec<_> = if window == 0 {
        ctx.kernel.window_rects.keys().copied().collect()
    } else {
        vec![window]
    };
    if flags & 0x80 != 0 {
        let mut index = 0;
        while index < windows.len() && windows.len() < 4096 {
            let owner = windows[index];
            for child in ctx.kernel.controls.children_of(owner).into_iter().chain(
                ctx.kernel
                    .window_parents
                    .iter()
                    .filter_map(|(&child, &parent)| (parent == owner).then_some(child)),
            ) {
                if !windows.contains(&child) {
                    windows.push(child);
                }
            }
            index += 1;
        }
    }
    for window in windows {
        if flags & 2 != 0 {
            ctx.kernel
                .posted_messages
                .retain(|message| message.0 != window || message.1 != WM_PAINT);
            ctx.kernel.gdi.generated_window_paints.remove(&window);
        }
        if flags & 0x20 != 0 {
            ctx.kernel.gdi.pending_window_erases.remove(&window);
        }
        if flags & 1 != 0 {
            queue_window_paint(ctx.kernel, window);
            if flags & 4 != 0 {
                ctx.kernel.gdi.pending_window_erases.insert(window);
            }
        }
    }
    // Painting runs through the existing guest pump; invalidation does not
    // manufacture a frame or erase a window's retained pixels immediately.
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn set_region(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (window, region, repaint) = (ctx.arg_u32(0)?, ctx.arg_u32(1)?, ctx.arg_u32(2)?);
    if !live(ctx.kernel, window) {
        return fail(ctx, 1400);
    }
    // mstscax resets the ActiveX window to its default rectangular shape.
    // Arbitrary window regions need compositor and hit-test clipping support.
    if region != 0 {
        return fail(
            ctx,
            if ctx.kernel.gdi.regions.contains_key(&region) {
                50
            } else {
                6
            },
        );
    }
    if repaint != 0 {
        queue_window_paint(ctx.kernel, window);
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;

    #[test]
    fn properties_are_per_window_opaque_values_and_disappear_on_destruction() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        for hwnd in [0x111, 0x112] {
            kernel.window_rects.insert(hwnd, (0, 0, 100, 100));
        }
        cpu.write_mem(0x1100, &wide_to_bytes(&[65, 84, 76, 0]))
            .unwrap();
        for (hwnd, value) in [(0x111, 0x1234), (0x112, 0x5678)] {
            assert_eq!(
                call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "coredll.dll",
                    "SetProp",
                    &[hwnd, 0x1100, value]
                ),
                DispatchOutcome::ReturnedR0(1)
            );
            assert_eq!(
                call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "coredll.dll",
                    "GetPropW",
                    &[hwnd, 0x1100]
                ),
                DispatchOutcome::ReturnedR0(value)
            );
        }
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "RemoveProp",
                &[0x111, 0x1100]
            ),
            DispatchOutcome::ReturnedR0(0x1234)
        );
        kernel.discard_window_metadata(&0x112);
        assert!(kernel.ce.window_aux.properties.is_empty());
    }

    #[test]
    fn a_window_batch_applies_only_at_end_and_does_not_overwrite_stack_arguments() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        cpu.map_region(0x5000_0000, 0x10000, Prot::READ | Prot::WRITE)
            .unwrap();
        kernel.heap = pocket_kernel::Heap::new(0x5000_0000, 0x10000);
        for hwnd in [0x111, 0x112] {
            kernel.window_rects.insert(hwnd, (1, 2, 30, 40));
        }
        let DispatchOutcome::ReturnedR0(batch) = call(
            &mut cpu,
            &mut kernel,
            &mut dispatcher,
            "coredll.dll",
            "BeginDeferWindowPos",
            &[2],
        ) else {
            panic!("batch");
        };
        assert_ne!(batch, 0);
        for hwnd in [0x111, 0x112] {
            assert_eq!(
                call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "coredll.dll",
                    "DeferWindowPos",
                    &[batch, hwnd, 0, 10, 20, 50, 60, 0x14]
                ),
                DispatchOutcome::ReturnedR0(batch)
            );
        }
        assert_eq!(kernel.window_rects[&0x111], (1, 2, 30, 40));
        let stack = cpu.read_mem(0x4000, 16).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "EndDeferWindowPos",
                &[batch]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(kernel.window_rects[&0x111], (10, 20, 50, 60));
        assert_eq!(kernel.window_rects[&0x112], (10, 20, 50, 60));
        assert_eq!(cpu.read_mem(0x4000, 16).unwrap(), stack);
        assert!(kernel.ce.window_aux.batches.is_empty());
        assert_eq!(kernel.heap.msize(batch), None);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "EndDeferWindowPos",
                &[batch]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
    }
}
