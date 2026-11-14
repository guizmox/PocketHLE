//! Failure contracts for CE certificate/SSPI providers which are absent here.
//! CE6 cetsc/mstscax must load their UI without executing ROM crypto drivers.
//! No certificate is trusted and no encrypted session is claimed by this layer.
use crate::{CallCtx, Handler, WinCeDispatcher};
use pocket_cpu::Prot;
use pocket_kernel::{DispatchOutcome, KernelError};

const UNSUPPORTED: u32 = 50;
const INVALID: u32 = 87;
const INVALID_SECURITY_HANDLE: u32 = 0x8009_0301;
const MISSING_PACKAGE: u32 = 0x8009_0305;

pub(crate) fn register(d: &mut WinCeDispatcher) {
    for (name, handler) in [
        ("CertCreateCertificateContext", unavailable as Handler),
        ("CertFreeCertificateContext", free_certificate),
        ("CertGetCertificateContextProperty", certificate_property),
        ("CryptDecodeObjectEx", decode),
        ("CertCloseStore", unavailable),
        ("CertFindCertificateInStore", unavailable),
        ("CertOpenStore", unavailable),
        ("CertCompareCertificate", unavailable),
        ("CertGetCertificateChain", chain),
        ("CertFreeCertificateChain", free_chain),
        ("CertDuplicateCertificateContext", unavailable),
        ("CertGetNameStringW", certificate_name),
        ("CertVerifySubjectCertificateContext", unavailable),
    ] {
        d.register_handler("crypt32.dll", name, handler);
    }
    for (name, handler) in [
        ("QuerySecurityPackageInfoW", package as Handler),
        ("AcquireCredentialsHandleW", acquire),
        ("InitializeSecurityContextW", initialize),
        ("FreeContextBuffer", free_buffer),
        ("FreeCredentialsHandle", security_unavailable),
        ("DeleteSecurityContext", security_unavailable),
        ("QueryContextAttributesW", security_unavailable),
        ("EncryptMessage", security_unavailable),
        ("DecryptMessage", security_unavailable),
    ] {
        d.register_handler("secur32.dll", name, handler);
    }
    // Messenger probes the device data-protection provider before its UI.
    // An absent provider returns FALSE and an empty output, never plaintext
    // labelled as protected data or TRUE with an uninitialized DATA_BLOB.
    for dll in ["coredll.dll", "crypt32.dll"] {
        d.register_handler(dll, "CryptProtectData", protect);
        d.register_handler(dll, "CryptUnprotectData", unprotect);
    }
}

fn fail(ctx: &mut CallCtx<'_>, error: u32) -> Result<DispatchOutcome, KernelError> {
    crate::coredll::set_thread_error(ctx, error);
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn clear(ctx: &mut CallCtx<'_>, pointer: u32, size: u32) -> Result<bool, KernelError> {
    if pointer == 0
        || ctx
            .cpu
            .check_guest_access(pointer, size, Prot::WRITE)
            .is_err()
    {
        return Ok(false);
    }
    ctx.cpu.write_mem(pointer, &vec![0; size as usize])?;
    Ok(true)
}
fn unavailable(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    fail(ctx, UNSUPPORTED)
}
fn protect(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    data_protection(ctx, false)
}
fn unprotect(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    data_protection(ctx, true)
}
fn data_protection(ctx: &mut CallCtx<'_>, decrypt: bool) -> Result<DispatchOutcome, KernelError> {
    let input = ctx.arg_u32(0)?;
    let output = ctx.arg_u32(6)?;
    let description = if decrypt { ctx.arg_u32(1)? } else { 0 };
    if input == 0
        || ctx.cpu.check_guest_access(input, 8, Prot::READ).is_err()
        || output == 0
        || ctx.cpu.check_guest_access(output, 8, Prot::WRITE).is_err()
        || description != 0
            && ctx
                .cpu
                .check_guest_access(description, 4, Prot::WRITE)
                .is_err()
    {
        return fail(ctx, INVALID);
    }
    let size = ctx.cpu.read_u32_le(input)?;
    let data = ctx.cpu.read_u32_le(input + 4)?;
    let valid =
        size == 0 || data != 0 && ctx.cpu.check_guest_access(data, size, Prot::READ).is_ok();
    clear(ctx, output, 8)?;
    if description != 0 {
        clear(ctx, description, 4)?;
    }
    fail(ctx, if valid { UNSUPPORTED } else { INVALID })
}
fn free_certificate(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if ctx.arg_u32(0)? == 0 {
        return Ok(DispatchOutcome::ReturnedR0(1));
    }
    fail(ctx, INVALID)
}
fn free_chain(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn certificate_property(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let output = ctx.arg_u32(3)?;
    if !clear(ctx, output, 4)? {
        return fail(ctx, INVALID);
    }
    unavailable(ctx)
}
fn decode(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let output = ctx.arg_u32(6)?;
    let size = ctx.arg_u32(7)?;
    if size != 0 && !clear(ctx, size, 4)? {
        return fail(ctx, INVALID);
    }
    if ctx.arg_u32(4)? & 0x8000 != 0 && !clear(ctx, output, 4)? {
        return fail(ctx, INVALID);
    }
    unavailable(ctx)
}
fn chain(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let output = ctx.arg_u32(7)?;
    if !clear(ctx, output, 4)? {
        return fail(ctx, INVALID);
    }
    unavailable(ctx)
}
fn certificate_name(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let output = ctx.arg_u32(4)?;
    if ctx.arg_u32(5)? != 0 && !clear(ctx, output, 2)? {
        return fail(ctx, INVALID);
    }
    unavailable(ctx)
}
fn package(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let output = ctx.arg_u32(1)?;
    if !clear(ctx, output, 4)? {
        return Ok(DispatchOutcome::ReturnedR0(0x8009_035d));
    }
    Ok(DispatchOutcome::ReturnedR0(MISSING_PACKAGE))
}
fn acquire(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let credential = ctx.arg_u32(7)?;
    let expiry = ctx.arg_u32(8)?;
    if !clear(ctx, credential, 8)? || expiry != 0 && !clear(ctx, expiry, 8)? {
        return Ok(DispatchOutcome::ReturnedR0(0x8009_035d));
    }
    Ok(DispatchOutcome::ReturnedR0(MISSING_PACKAGE))
}
fn initialize(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    for (arg, size) in [(8, 8), (10, 4), (11, 8)] {
        let pointer = ctx.arg_u32(arg)?;
        if pointer != 0 && !clear(ctx, pointer, size)? {
            return Ok(DispatchOutcome::ReturnedR0(0x8009_035d));
        }
    }
    security_unavailable(ctx)
}
fn security_unavailable(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(INVALID_SECURITY_HANDLE))
}
fn free_buffer(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(if ctx.arg_u32(0)? == 0 {
        0
    } else {
        INVALID_SECURITY_HANDLE
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;

    #[test]
    fn absent_data_protection_never_reports_plaintext_as_encrypted() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        cpu.write_mem(0x1100, &[3, 0, 0, 0, 0, 0x12, 0, 0]).unwrap();
        cpu.write_mem(0x1200, b"abc").unwrap();
        for (name, description) in [("CryptProtectData", 0), ("CryptUnprotectData", 0x1400)] {
            cpu.write_mem(0x1300, &[0xa5; 8]).unwrap();
            assert_eq!(
                call(
                    &mut cpu,
                    &mut kernel,
                    &mut dispatcher,
                    "coredll.dll",
                    name,
                    &[0x1100, description, 0, 0, 0, 0, 0x1300]
                ),
                DispatchOutcome::ReturnedR0(0)
            );
            assert_eq!(kernel.thread_last_errors[&0], UNSUPPORTED);
            assert_eq!(cpu.read_mem(0x1300, 8).unwrap(), [0; 8]);
            assert_eq!(cpu.read_mem(0x1200, 3).unwrap(), b"abc");
            if description != 0 {
                assert_eq!(cpu.read_u32_le(description).unwrap(), 0);
            }
        }
    }

    #[test]
    fn missing_security_package_and_certificate_chain_clear_outputs_and_fail() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        cpu.write_mem(0x1300, &0xa5a5a5a5u32.to_le_bytes()).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "secur32.dll",
                "QuerySecurityPackageInfoW",
                &[0x1100, 0x1300]
            ),
            DispatchOutcome::ReturnedR0(MISSING_PACKAGE)
        );
        assert_eq!(cpu.read_u32_le(0x1300).unwrap(), 0);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "crypt32.dll",
                "CertGetCertificateChain",
                &[0, 0, 0, 0, 0, 0, 0, 0x1300]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_u32_le(0x1300).unwrap(), 0);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                "crypt32.dll",
                "CertVerifySubjectCertificateContext",
                &[0x1100, 0x1200, 0x1300]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
    }
}
