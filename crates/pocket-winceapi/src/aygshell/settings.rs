//! SIP setting changes resize real USER windows using the shared SIP geometry.
use crate::guest_callbacks::{self, Frame, WINCE};
use crate::{CallCtx, WinCeDispatcher};
use pocket_kernel::{DispatchOutcome, KernelError};

pub(super) fn register(d: &mut WinCeDispatcher) {
    for name in ["ord:83", "#83"] {
        d.register_handler("aygshell.dll", name, handle);
    }
}
pub(crate) fn handle(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let owner = guest_callbacks::owner(ctx, WINCE.continue_name)?;
    let f = Frame::enter(ctx, WINCE, owner)?;
    let hwnd = f.arg(ctx, 0)?;
    let info = f.arg(ctx, 3)?;
    if f.fresh {
        if !crate::coredll::window_exists(ctx.kernel, hwnd) {
            crate::coredll::set_thread_error(ctx, 1400);
            return f.finish(ctx, 0);
        }
        ctx.cpu
            .check_guest_access(info, 12, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
        if ctx.cpu.read_u32_le(info)? != 12 {
            crate::coredll::set_thread_error(ctx, 87);
            return f.finish(ctx, 0);
        }
        if f.arg(ctx, 1)? != 0xe0 {
            // SPI_SETSIPINFO
            return f.finish(ctx, 1);
        }
        let bits = ctx.cpu.read_u32_le(info + 8)?;
        let up = u32::from(ctx.kernel.shell_keyboard.visible);
        ctx.cpu
            .write_mem(info + 8, &((bits & !1) | up).to_le_bytes())?;
        ctx.cpu.write_mem(f.address + 128, &48u32.to_le_bytes())?;
        crate::shell_keyboard::sip_info(ctx, f.address + 128)?;
        let target = guest_callbacks::service(ctx, "SystemParametersInfoW")?;
        return f.call_args(ctx, 1, target, &[0x30, 0, f.address + 176, 0]);
    }
    match f.phase(ctx)? {
        1 => {
            let mut rect = [0i32; 4];
            for (i, value) in rect.iter_mut().enumerate() {
                let sip = ctx.cpu.read_u32_le(f.address + 136 + i as u32 * 4)? as i32;
                let work = ctx.cpu.read_u32_le(f.address + 176 + i as u32 * 4)? as i32;
                *value = if i < 2 { sip.max(work) } else { sip.min(work) };
            }
            let target = guest_callbacks::service(ctx, "SetWindowPos")?;
            f.call_args(
                ctx,
                2,
                target,
                &[
                    hwnd,
                    0,
                    rect[0] as u32,
                    rect[1] as u32,
                    rect[2].saturating_sub(rect[0]).max(0) as u32,
                    rect[3].saturating_sub(rect[1]).max(0) as u32,
                    0x14,
                ],
            )
        }
        2 => {
            let result = ctx.cpu.read_reg(pocket_cpu::regs::ArmReg::R0)?;
            f.finish(ctx, result)
        }
        _ => Err(KernelError::Dispatch(
            "Invalid shell setting-change continuation".into(),
        )),
    }
}
