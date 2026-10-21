//! Standard window scrollbar state, shared by the WinCE scroll APIs.
use crate::{CallCtx, WinCeDispatcher};
use pocket_kernel::{DispatchOutcome, KernelError};

pub(crate) fn register(d: &mut WinCeDispatcher) {
    for (name, handler) in [
        ("GetScrollInfo", get_info as crate::Handler),
        ("SetScrollInfo", set_info),
        ("GetScrollPos", get_pos),
        ("SetScrollPos", set_pos),
        ("GetScrollRange", get_range),
        ("SetScrollRange", set_range),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}
fn fail(ctx: &mut CallCtx<'_>, error: u32) -> Result<DispatchOutcome, KernelError> {
    crate::coredll::set_thread_error(ctx, error);
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn key(ctx: &mut CallCtx<'_>) -> Result<(u32, u32), KernelError> {
    Ok((ctx.arg_u32(0)?, ctx.arg_u32(1)?))
}
fn valid(ctx: &CallCtx<'_>, (hwnd, bar): (u32, u32)) -> bool {
    hwnd != 0
        && (bar <= 1
            || (bar == 2
                && ctx
                    .kernel
                    .controls
                    .get(hwnd)
                    .is_some_and(|c| c.class == pocket_kernel::controls::ControlClass::ScrollBar)))
        && (ctx.kernel.window_classes.contains_key(&hwnd)
            || ctx.kernel.controls.get(hwnd).is_some()
            || hwnd == 0xDEAD_0001)
}
fn state(ctx: &CallCtx<'_>, key: (u32, u32)) -> Option<[i32; 5]> {
    ctx.kernel.scroll_info.get(&key).copied().or_else(|| {
        if key.1 == 2
            && ctx
                .kernel
                .controls
                .get(key.0)
                .is_some_and(|c| c.class == pocket_kernel::controls::ControlClass::ScrollBar)
        {
            return Some([0, 100, 0, 0, 0]);
        }
        let style = ctx.kernel.window_styles.get(&key.0)?.0;
        (style & if key.1 == 0 { 0x0010_0000 } else { 0x0020_0000 } != 0)
            .then_some([0, 100, 0, 0, 0])
    })
}
fn clamp(s: &mut [i32; 5]) {
    if s[1] < s[0] {
        s[1] = s[0];
    }
    let span = i64::from(s[1]) - i64::from(s[0]) + 1;
    let page = i64::from(s[2] as u32).min(span);
    s[2] = page as u32 as i32;
    let max = i64::from(s[1]) - (page - 1).max(0);
    s[3] = i64::from(s[3]).clamp(i64::from(s[0]), max) as i32;
    s[4] = s[3];
}
fn store(ctx: &mut CallCtx<'_>, key: (u32, u32), s: [i32; 5]) {
    ctx.kernel.scroll_info.insert(key, s);
    if key.1 == 2 {
        if let Some(c) = ctx.kernel.controls.get_mut(key.0) {
            c.track_range = (s[0], s[1]);
            c.track_page = s[2];
            c.track_pos = s[3];
        }
    }
}
pub(crate) fn control_message(
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
        .is_some_and(|c| c.class == pocket_kernel::controls::ControlClass::ScrollBar)
    {
        return Ok(None);
    }
    if msg == 0xe4 {
        ctx.kernel.controls.get_mut(hwnd).unwrap().enabled = wp & 3 != 3;
        return Ok(Some(1));
    }
    let (handler, args): (crate::Handler, [u32; 4]) = match msg {
        0xe0 => (set_pos, [hwnd, 2, wp, lp]),
        0xe1 => (get_pos, [hwnd, 2, 0, 0]),
        0xe2 | 0xe6 => (set_range, [hwnd, 2, wp, lp]),
        0xe3 => (get_range, [hwnd, 2, wp, lp]),
        0xe9 => (set_info, [hwnd, 2, lp, wp]),
        0xea => (get_info, [hwnd, 2, lp, 0]),
        _ => return Ok(None),
    };
    use pocket_cpu::regs::ArmReg;
    let regs = [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3];
    let saved = regs
        .map(|r| ctx.cpu.read_reg(r))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    for (r, v) in regs.into_iter().zip(args) {
        ctx.cpu.write_reg(r, v)?;
    }
    let result = handler(ctx);
    for (r, v) in regs.into_iter().zip(saved) {
        ctx.cpu.write_reg(r, v)?;
    }
    result.map(|r| {
        Some(match r {
            DispatchOutcome::ReturnedR0(v) => v,
            _ => 0,
        })
    })
}
fn get_info(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let k = key(ctx)?;
    let output = ctx.arg_u32(2)?;
    if !valid(ctx, k) || output == 0 {
        return fail(ctx, 87);
    }
    let mut b = ctx.cpu.read_mem(output, 28)?;
    let size = u32::from_le_bytes(b[0..4].try_into().unwrap());
    let mask = u32::from_le_bytes(b[4..8].try_into().unwrap());
    if size != 28 || mask & !0x1f != 0 {
        return fail(ctx, 87);
    }
    let Some(s) = state(ctx, k) else {
        return fail(ctx, 1447);
    }; // ERROR_NO_SCROLLBARS
    for (offset, value, bit) in [
        (8, s[0], 1),
        (12, s[1], 1),
        (16, s[2], 2),
        (20, s[3], 4),
        (24, s[4], 16),
    ] {
        if mask & bit != 0 {
            b[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    ctx.cpu.write_mem(output, &b)?;
    Ok(DispatchOutcome::ReturnedR0(u32::from(mask & 0x17 != 0)))
}
fn set_info(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let k = key(ctx)?;
    let input = ctx.arg_u32(2)?;
    if !valid(ctx, k) || input == 0 {
        return fail(ctx, 87);
    }
    let b = ctx.cpu.read_mem(input, 28)?;
    let field = |i| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    let mask = field(4);
    if field(0) != 28 || mask & !0x1f != 0 {
        return fail(ctx, 87);
    }
    let mut s = state(ctx, k).unwrap_or([0, 100, 0, 0, 0]);
    if mask & 1 != 0 {
        s[0] = field(8) as i32;
        s[1] = field(12) as i32;
    }
    if mask & 2 != 0 {
        s[2] = field(16) as i32;
    }
    if mask & 4 != 0 {
        s[3] = field(20) as i32;
    }
    clamp(&mut s);
    store(ctx, k, s);
    Ok(DispatchOutcome::ReturnedR0(s[3] as u32))
}
fn get_pos(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let k = key(ctx)?;
    match state(ctx, k) {
        Some(s) if valid(ctx, k) => Ok(DispatchOutcome::ReturnedR0(s[3] as u32)),
        _ => fail(ctx, 1447),
    }
}
fn set_pos(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let k = key(ctx)?;
    if !valid(ctx, k) {
        return fail(ctx, 87);
    }
    let Some(mut s) = state(ctx, k) else {
        return fail(ctx, 1447);
    };
    let previous = s[3];
    s[3] = ctx.arg_u32(2)? as i32;
    clamp(&mut s);
    store(ctx, k, s);
    Ok(DispatchOutcome::ReturnedR0(previous as u32))
}
fn get_range(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let k = key(ctx)?;
    let min = ctx.arg_u32(2)?;
    let max = ctx.arg_u32(3)?;
    let Some(s) = state(ctx, k) else {
        return fail(ctx, 1447);
    };
    if !valid(ctx, k) || min == 0 || max == 0 {
        return fail(ctx, 87);
    }
    ctx.cpu.write_mem(min, &s[0].to_le_bytes())?;
    ctx.cpu.write_mem(max, &s[1].to_le_bytes())?;
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn set_range(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let k = key(ctx)?;
    let min = ctx.arg_u32(2)? as i32;
    let max = ctx.arg_u32(3)? as i32;
    if !valid(ctx, k) || min > max || i64::from(max) - i64::from(min) > i64::from(i32::MAX) {
        return fail(ctx, 87);
    }
    let mut s = state(ctx, k).unwrap_or([0, 100, 0, 0, 0]);
    s[0] = min;
    s[1] = max;
    clamp(&mut s);
    store(ctx, k, s);
    Ok(DispatchOutcome::ReturnedR0(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
    #[test]
    fn scroll_state_clamps_to_page_and_get_info_preserves_unrequested_fields() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        let hwnd = 0xDEAD_0001;
        kernel.window_styles.insert(hwnd, (0x0010_0000, 0));
        let fields = [28u32, 7, (-10i32) as u32, 20, 6, 100, 0];
        cpu.write_mem(
            0x1200,
            &fields
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "SetScrollInfo",
                &[hwnd, 0, 0x1200, 0]
            ),
            DispatchOutcome::ReturnedR0(15)
        );
        cpu.write_mem(0x1200, &[0xaa; 28]).unwrap();
        cpu.write_mem(0x1200, &28u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x1204, &4u32.to_le_bytes()).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "GetScrollInfo",
                &[hwnd, 0, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(cpu.read_mem(0x1208, 12).unwrap(), [0xaa; 12]);
        assert_eq!(cpu.read_u32_le(0x1214).unwrap(), 15);
        assert_eq!(cpu.read_u32_le(0x1218).unwrap(), 0xaaaa_aaaa);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "SetScrollPos",
                &[hwnd, 0, (-20i32) as u32, 1]
            ),
            DispatchOutcome::ReturnedR0(15)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "GetScrollPos",
                &[hwnd, 0]
            ),
            DispatchOutcome::ReturnedR0((-10i32) as u32)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "GetScrollInfo",
                &[hwnd, 1, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "coredll.dll",
                "GetLastError",
                &[]
            ),
            DispatchOutcome::ReturnedR0(1447)
        );
    }
}
