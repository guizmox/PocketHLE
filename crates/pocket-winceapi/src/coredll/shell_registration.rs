//! Native CE6 Explorer registers its desktop/taskbar with GWES during startup.
//! They remain guest windows; the OS desktop sentinel remains their parent.
use super::*;

pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", "RegisterDesktop", desktop);
    d.register_handler("coredll.dll", "RegisterTaskBar", taskbar);
    d.register_handler("coredll.dll", "RegisterTaskBarEx", taskbar_ex);
    d.register_handler("coredll.dll", "SignalStarted", signal_started);
}

fn valid(ctx: &mut CallCtx<'_>, hwnd: u32) -> bool {
    if hwnd == 0
        || ctx.kernel.window_classes.contains_key(&hwnd)
            && ctx
                .kernel
                .window_styles
                .get(&hwnd)
                .is_none_or(|s| s.0 & 0x4000_0000 == 0)
    {
        true
    } else {
        set_thread_error(ctx, 1400);
        false
    }
}

fn desktop(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    if !valid(ctx, hwnd) {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    ctx.kernel.ce.shell.desktop = (hwnd != 0).then_some(hwnd);
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn taskbar(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    let on_top = ctx
        .kernel
        .native_window_screen_rect(hwnd)
        .is_some_and(|r| r.1 == 0);
    set_taskbar(ctx, hwnd, on_top)
}
fn taskbar_ex(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hwnd = ctx.arg_u32(0)?;
    let on_top = ctx.arg_u32(1)? != 0;
    set_taskbar(ctx, hwnd, on_top)
}
fn set_taskbar(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    on_top: bool,
) -> Result<DispatchOutcome, KernelError> {
    if !valid(ctx, hwnd) {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    ctx.kernel.ce.shell.taskbar = (hwnd != 0).then_some((hwnd, on_top));
    Ok(DispatchOutcome::ReturnedR0(1))
}
fn signal_started(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let id = ctx.arg_u32(0)?;
    ctx.kernel.ce.shell.started.insert(id);
    Ok(DispatchOutcome::ReturnedR0(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};

    #[test]
    fn native_shell_windows_are_registered_without_reparenting_and_released_on_destroy() {
        let (mut cpu, mut k, mut d) = setup();
        k.ce.windows.enabled = true;
        k.gapi_action_key = pocket_kernel::gapi::VK_RETURN;
        k.window_classes.insert(0x201, "Native desktop".into());
        k.window_rects.insert(0x201, (0, 0, 320, 220));
        k.window_styles.insert(0x201, (0x10000000, 0));
        k.window_parents.insert(0x201, FAKE_DESKTOP_HWND);
        k.window_classes.insert(0x202, "Native taskbar".into());
        k.window_rects.insert(0x202, (0, 220, 320, 20));
        k.window_styles.insert(0x202, (0x90000000, 0));
        k.window_parents.insert(0x202, FAKE_DESKTOP_HWND);
        assert!(!k.is_desktop_window(0x201));
        assert!(k.pocket_pc_taskbar_visible());
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RegisterDesktop",
                &[0x201]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert!(k.is_desktop_window(0x201));
        assert_eq!(k.window_parents[&0x201], FAKE_DESKTOP_HWND);
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "GetDesktopWindow",
                &[]
            ),
            DispatchOutcome::ReturnedR0(FAKE_DESKTOP_HWND)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RegisterTaskBar",
                &[0x202]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(k.ce.shell.taskbar, Some((0x202, false)));
        assert!(!k.pocket_pc_taskbar_visible());
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RegisterTaskBarEx",
                &[0xbad, 1]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(k.ce.shell.taskbar, Some((0x202, false)));
        k.discard_window_metadata(&0x202);
        assert!(k.ce.shell.taskbar.is_none());
        assert!(k.pocket_pc_taskbar_visible());
        k.discard_window_metadata(&0x201);
        assert!(k.ce.shell.desktop.is_none());
    }
}
