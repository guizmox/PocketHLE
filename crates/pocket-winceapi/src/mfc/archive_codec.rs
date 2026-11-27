//! Count/CString wire formats over the one virtual archive stream.
use super::super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
pub(in crate::mfc) const READ_COUNT: &str = "__PocketHLE_MFC_ArchiveReadCount";
pub(in crate::mfc) const WRITE_COUNT: &str = "__PocketHLE_MFC_ArchiveWriteCount";
pub(in crate::mfc) const READ_STRING: &str = "__PocketHLE_MFC_ArchiveReadString";
pub(in crate::mfc) const WRITE_STRING: &str = "__PocketHLE_MFC_ArchiveWriteString";
pub(super) fn register(d: &mut WinCeDispatcher) {
    for (name, handler) in [
        (READ_COUNT, read_count as crate::Handler),
        (WRITE_COUNT, write_count),
        (READ_STRING, read_string),
        (WRITE_STRING, write_string),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}
fn exact(ctx: &mut CallCtx<'_>, a: u32, n: u32) -> Result<(), KernelError> {
    if ctx.cpu.read_reg(ArmReg::R0)? != n {
        Err(fault(ctx, a, 3))
    } else {
        Ok(())
    }
}
pub(super) fn read_count(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let a = f.arg(ctx, 0)?;
    let p = f.address + 128;
    if f.fresh {
        return f.call_service(ctx, 1, stream::READ, &[a, p, 2]);
    }
    let phase = f.phase(ctx)?;
    exact(ctx, a, if phase == 1 { 2 } else { 4 })?;
    if phase == 1 {
        let n = u32::from(ctx.cpu.read_u16_le(p)?);
        if n == 0xffff {
            return f.call_service(ctx, 2, stream::READ, &[a, p, 4]);
        }
        return f.finish(ctx, n);
    }
    let n = word(ctx, p)?;
    f.finish(ctx, n)
}
pub(super) fn write_count(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let a = f.arg(ctx, 0)?;
    if f.fresh {
        let n = f.arg(ctx, 1)?;
        let p = f.address + 128;
        let bytes = if n < 0xffff {
            (n as u16).to_le_bytes().to_vec()
        } else {
            let mut v = vec![0xff, 0xff];
            v.extend_from_slice(&n.to_le_bytes());
            v
        };
        ctx.cpu.write_mem(p, &bytes)?;
        return f.call_service(ctx, 1, stream::WRITE, &[a, p, bytes.len() as u32]);
    }
    f.finish(ctx, 0)
}
fn body(ctx: &mut CallCtx<'_>, f: Frame, a: u32, len: u32) -> Result<DispatchOutcome, KernelError> {
    if len > 0x100000 {
        return Err(fault(ctx, a, 5));
    }
    let n = len * if f.get(ctx, 0)? != 0 { 2 } else { 1 };
    let p = alloc(ctx, n.max(1))?;
    f.own(ctx, p)?;
    f.set(ctx, 1, p)?;
    f.set(ctx, 2, n)?;
    f.call_service(ctx, 4, stream::READ, &[a, p, n])
}
pub(super) fn read_string(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let a = f.arg(ctx, 0)?;
    let string = f.arg(ctx, 1)?;
    if f.fresh {
        if let Err(error) = strings::validate_assignment_target(ctx, string) {
            f.finish(ctx, 0)?;
            return Err(error);
        }
    }
    let p = f.address + 128;
    if f.fresh {
        return f.call_service(ctx, 1, stream::READ, &[a, p, 1]);
    }
    match f.phase(ctx)? {
        1 => {
            exact(ctx, a, 1)?;
            let n = ctx.cpu.read_mem(p, 1)?[0];
            if n == 0xff {
                f.call_service(ctx, 2, stream::READ, &[a, p, 2])
            } else {
                body(ctx, f, a, u32::from(n))
            }
        }
        2 => {
            exact(ctx, a, 2)?;
            match ctx.cpu.read_u16_le(p)? {
                0xfffe => {
                    if f.get(ctx, 0)? != 0 {
                        return Err(fault(ctx, a, 5));
                    }
                    f.set(ctx, 0, 1)?;
                    f.call_service(ctx, 1, stream::READ, &[a, p, 1])
                }
                0xffff => f.call_service(ctx, 3, stream::READ, &[a, p, 4]),
                n => body(ctx, f, a, u32::from(n)),
            }
        }
        3 => {
            exact(ctx, a, 4)?;
            let n = word(ctx, p)?;
            body(ctx, f, a, n)
        }
        4 => {
            let n = f.get(ctx, 2)?;
            exact(ctx, a, n)?;
            let p = f.get(ctx, 1)?;
            let bytes = ctx.cpu.read_mem(p, n)?;
            let text: Vec<u16> = if f.get(ctx, 0)? != 0 {
                bytes
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect()
            } else {
                crate::crt_ansi::decode(&bytes).encode_utf16().collect()
            };
            strings::assign_units(ctx, string, &text)?;
            f.finish(ctx, a)
        }
        _ => Err(error("invalid CString reader continuation")),
    }
}
pub(super) fn write_string(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let a = f.arg(ctx, 0)?;
    if f.fresh {
        let string = f.arg(ctx, 1)?;
        let text = strings::value(ctx, string)?;
        let len = text.len() as u32;
        let mut bytes = vec![0xff, 0xfe, 0xff];
        if len < 0xff {
            bytes.push(len as u8);
        } else if len < 0xfffe {
            bytes.push(0xff);
            bytes.extend_from_slice(&(len as u16).to_le_bytes());
        } else {
            bytes.extend_from_slice(&[0xff, 0xff, 0xff]);
            bytes.extend_from_slice(&len.to_le_bytes());
        }
        for c in text {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        let p = alloc(ctx, bytes.len() as u32)?;
        f.own(ctx, p)?;
        ctx.cpu.write_mem(p, &bytes)?;
        return f.call_service(ctx, 1, stream::WRITE, &[a, p, bytes.len() as u32]);
    }
    f.finish(ctx, a)
}
