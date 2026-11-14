//! The untouched CE6 Explorer publishes a 74-method shell provider at startup.
//! Preserve its owned method/signature tables rather than returning a fake handle.
use crate::{CallCtx, Handler, WinCeDispatcher};
use pocket_cpu::Prot;
use pocket_kernel::{DispatchOutcome, KernelError};

pub fn register(d: &mut WinCeDispatcher) {
    for (name, handler) in [
        ("CreateAPISet", create as Handler),
        ("RegisterAPISet", publish),
        ("QueryAPISetID", query),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}
fn fail(ctx: &mut CallCtx<'_>, error: u32, result: u32) -> Result<DispatchOutcome, KernelError> {
    crate::coredll::set_thread_error(ctx, error);
    Ok(DispatchOutcome::ReturnedR0(result))
}
fn bytes(ctx: &mut CallCtx<'_>, pointer: u32, size: u32) -> Result<Vec<u8>, u32> {
    if pointer == 0
        || pointer.checked_add(size).is_none()
        || ctx
            .cpu
            .check_guest_access(pointer, size, Prot::READ)
            .is_err()
    {
        return Err(87);
    }
    ctx.cpu.read_mem(pointer, size).map_err(|_| 87)
}
fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let name_ptr = ctx.arg_u32(0)?;
    let count = ctx.arg_u32(1)?;
    let methods_ptr = ctx.arg_u32(2)?;
    let signatures_ptr = ctx.arg_u32(3)?;
    if count == 0 || count > 256 {
        return fail(ctx, 87, 0);
    }
    let data = (|| -> Result<_, u32> {
        let name: [u8; 4] = bytes(ctx, name_ptr, 4)?.try_into().unwrap();
        if name.contains(&0) {
            return Err(87);
        }
        let methods: Vec<u32> = bytes(ctx, methods_ptr, count * 4)?
            .chunks_exact(4)
            .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
            .collect();
        for &method in &methods {
            if method != 0
                && ctx
                    .cpu
                    .check_guest_access(method & !1, 2, Prot::EXEC)
                    .is_err()
            {
                return Err(87);
            }
        }
        // CE6 uses ULONGLONG signatures, even on ARM32. Reading DWORDs loses
        // pointer descriptors and reads the wrong signature after method zero.
        let signatures = bytes(ctx, signatures_ptr, count * 8)?
            .chunks_exact(8)
            .map(|v| u64::from_le_bytes(v.try_into().unwrap()))
            .collect();
        Ok((name, methods, signatures))
    })();
    let (name, methods, signatures) = match data {
        Ok(data) => data,
        Err(error) => return fail(ctx, error, 0),
    };
    let Some(handle) = ctx
        .kernel
        .object_handles
        .create_api_set(name, methods, signatures)
    else {
        return fail(ctx, 8, 0);
    };
    Ok(DispatchOutcome::ReturnedR0(handle))
}
fn publish(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let id = ctx.arg_u32(1)?;
    match ctx.kernel.object_handles.register_api_set(handle, id) {
        Ok(()) => Ok(DispatchOutcome::ReturnedR0(1)),
        Err(error) => fail(ctx, error, 0),
    }
}
fn query(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pointer = ctx.arg_u32(0)?;
    let name = match bytes(ctx, pointer, 4) {
        Ok(data) => data.try_into().unwrap(),
        Err(error) => return fail(ctx, error, u32::MAX),
    };
    Ok(DispatchOutcome::ReturnedR0(
        ctx.kernel
            .object_handles
            .query_api_set(name)
            .unwrap_or(u32::MAX),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
    use pocket_kernel::handles::HandleObject;
    fn value(outcome: DispatchOutcome) -> u32 {
        let DispatchOutcome::ReturnedR0(value) = outcome else {
            panic!("unexpected continuation");
        };
        value
    }
    #[test]
    fn native_api_provider_copies_64bit_signatures_shares_readiness_and_releases_owned_handles() {
        let (mut cpu, mut k, mut d) = setup();
        k.ce.version = Some((6, 0, 0));
        cpu.map_region(0x8000, 4096, Prot::READ | Prot::EXEC)
            .unwrap();
        cpu.write_mem(0x1000, b"TEST").unwrap();
        cpu.write_mem(
            0x1100,
            &[0x8001u32, 0x8010]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let signatures = [0x11223344_55667788u64, 0xaabbccdd_eeff0011];
        cpu.write_mem(
            0x1200,
            &signatures
                .into_iter()
                .flat_map(u64::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let h = value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "CreateAPISet",
            &[0x1000, 2, 0x1100, 0x1200],
        ));
        assert_ne!(h, 0);
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "QueryAPISetID",
                &[0x1000]
            ),
            DispatchOutcome::ReturnedR0(u32::MAX)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RegisterAPISet",
                &[h, 87]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "WaitForAPIReady",
                &[87, 0]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        let provider = k.object_handles.api_set_provider(87).unwrap();
        assert_eq!(provider.owner, k.object_handles.process_id());
        assert_eq!(provider.methods, [0x8001, 0x8010]);
        assert_eq!(provider.signatures, signatures);
        cpu.write_mem(0x1200, &[0; 16]).unwrap();
        assert_eq!(
            k.object_handles.api_set_provider(87).unwrap().signatures,
            signatures
        );
        let (_, mut child) = k.object_handles.new_child().unwrap();
        assert_eq!(child.query_api_set(*b"TEST"), Some(87));
        assert_eq!(child.register_api_set(h, 88), Err(6));
        let duplicate = k.object_handles.duplicate(HandleObject::ApiSet(h)).unwrap();
        assert_eq!(
            call(&mut cpu, &mut k, &mut d, "coredll.dll", "CloseHandle", &[h]),
            DispatchOutcome::ReturnedR0(1)
        );
        assert!(child.api_set_provider(87).is_some());
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "CloseHandle",
                &[duplicate]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert!(child.api_set_provider(87).is_none());
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "WaitForAPIReady",
                &[87, 0]
            ),
            DispatchOutcome::ReturnedR0(258)
        );
        let h = value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "CreateAPISet",
            &[0x1000, 2, 0x1100, 0x1200],
        ));
        k.object_handles.register_api_set(h, 87).unwrap();
        child.duplicate(HandleObject::ApiSet(h)).unwrap();
        k.object_handles.set_exit(None, 0);
        assert!(child.api_set_provider(87).is_none());
    }
    #[test]
    fn bad_api_tables_and_conflicting_registration_do_not_create_ready_providers() {
        let (mut cpu, mut k, mut d) = setup();
        cpu.map_region(0x8000, 4096, Prot::READ | Prot::EXEC)
            .unwrap();
        cpu.write_mem(0x1000, b"TEST").unwrap();
        cpu.write_mem(0x1100, &0x8000u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x1200, &[0; 8]).unwrap();
        for (count, signatures) in [(0, 0x1200), (257, 0x1200), (1, 0x4ffc)] {
            assert_eq!(
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "coredll.dll",
                    "CreateAPISet",
                    &[0x1000, count, 0x1100, signatures]
                ),
                DispatchOutcome::ReturnedR0(0)
            );
            assert!(k.object_handles.owned().is_empty());
        }
        let h = value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "CreateAPISet",
            &[0x1000, 1, 0x1100, 0x1200],
        ));
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "RegisterAPISet",
                &[h, 0x80000000]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(k.thread_last_errors[&0], 50);
        assert!(k.object_handles.api_set_provider(87).is_none());
        k.object_handles.register_api_set(h, 87).unwrap();
        let other = value(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "CreateAPISet",
            &[0x1000, 1, 0x1100, 0x1200],
        ));
        assert_eq!(k.object_handles.register_api_set(other, 87), Err(183));
        assert_eq!(k.object_handles.register_api_set(other, 88), Err(183));
        assert_eq!(k.object_handles.query_api_set(*b"TEST"), Some(87));
    }
}
