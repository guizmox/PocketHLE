//! WinCE CRT aggregate return ABI: the caller supplies div_t storage in r0.
use crate::{CallCtx, WinCeDispatcher};
use pocket_cpu::Prot;
use pocket_kernel::{DispatchOutcome, KernelError};

pub(crate) fn register(dispatcher: &mut WinCeDispatcher) {
    dispatcher.register_handler("coredll.dll", "div", div);
    // Windows CE long and int are both signed 32-bit values.
    dispatcher.register_handler("coredll.dll", "ldiv", div);
}

fn quotient_remainder(numerator: i32, denominator: i32) -> Option<[u8; 8]> {
    let quotient = numerator.checked_div(denominator)?;
    let remainder = numerator.checked_rem(denominator)?;
    let mut result = [0; 8];
    result[..4].copy_from_slice(&quotient.to_le_bytes());
    result[4..].copy_from_slice(&remainder.to_le_bytes());
    Some(result)
}

fn div(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let destination = ctx.arg_u32(0)?;
    let numerator = ctx.arg_u32(1)? as i32;
    let denominator = ctx.arg_u32(2)? as i32;
    // C leaves division by zero and an unrepresentable quotient undefined.
    // Report the invalid operation rather than panic in the host or fabricate
    // a successful quotient. Valid inputs follow truncation towards zero.
    let bytes = quotient_remainder(numerator, denominator)
        .ok_or_else(|| KernelError::Loader("CRT div: undefined integer division".into()))?;
    ctx.cpu.check_guest_access(destination, 8, Prot::WRITE)?;
    ctx.cpu.write_mem(destination, &bytes)?;
    Ok(DispatchOutcome::ReturnedR0(destination))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_quotient_and_remainder_follow_c_rules() {
        for (n, d, q, r) in [
            (4990, 50, 99i32, 40i32),
            (7, -3, -2, 1),
            (-7, 3, -2, -1),
            (-7, -3, 2, -1),
            (0, 3, 0, 0),
            (i32::MIN, 1, i32::MIN, 0),
        ] {
            let bytes = quotient_remainder(n, d).unwrap();
            assert_eq!(&bytes[..4], &q.to_le_bytes());
            assert_eq!(&bytes[4..], &r.to_le_bytes());
        }
        assert!(quotient_remainder(1, 0).is_none());
        assert!(quotient_remainder(i32::MIN, -1).is_none());
    }
    #[test]
    fn aggregate_result_is_written_and_its_pointer_returned() {
        use crate::bluetooth::tests::{call, setup};
        use pocket_cpu::Cpu;
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        for name in ["div", "ldiv"] {
            cpu.write_mem(0x1200, &[0xaa; 12]).unwrap();
            assert_eq!(
                call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "COREDLL.dll",
                    name,
                    &[0x1200, 0x137e, 50, 0xdfe]
                ),
                DispatchOutcome::ReturnedR0(0x1200)
            );
            assert_eq!(
                cpu.read_mem(0x1200, 8).unwrap(),
                quotient_remainder(4990, 50).unwrap()
            );
            assert_eq!(cpu.read_mem(0x1208, 4).unwrap(), [0xaa; 4]);
        }
    }
}
