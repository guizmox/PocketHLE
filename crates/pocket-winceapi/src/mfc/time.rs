//! CE3 CTime is a 32-bit time_t; use the shared WinCE host clock service.
use super::callbacks::{service, Frame};
use super::*;
use pocket_cpu::{regs::ArmReg, Prot};
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] =
    &[(1254, "?GetCurrentTime@CTime@@SA?AV1@XZ", current)];
fn current(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let destination = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu.check_guest_access(destination, 4, Prot::WRITE)?;
        let target = service(ctx, "time")?;
        return f.call_args(ctx, 1, target, &[0]);
    }
    let seconds = ctx.cpu.read_reg(ArmReg::R0)?;
    put(ctx, destination, seconds)?;
    f.finish(ctx, destination)
}
#[cfg(test)]
mod tests {
    use super::super::window_dispatch::tests::{drive, entry};
    use pocket_cpu::Cpu;
    #[test]
    fn current_time_writes_the_hidden_result_from_the_real_clock() {
        let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
        let seconds = || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as u32
        };
        let before = seconds();
        cpu.write_arg(0, 0x2000).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(1254), |pc, _| panic!(
                "unexpected clock callback {pc:x}"
            )),
            0x2000
        );
        let value = cpu.read_u32_le(0x2000).unwrap();
        assert!((before..=seconds()).contains(&value));
    }
}
