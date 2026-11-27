//! Shared per-thread handle identities; temporary wrappers never own OS handles.
use super::callbacks::{virtual_function, Frame};
use super::*;
#[derive(Clone, Copy)]
pub(super) enum Kind {
    Dc,
    Gdi,
    Wnd,
    Menu,
}
impl Kind {
    fn class(self) -> &'static str {
        match self {
            Self::Dc => "CDC",
            Self::Gdi => "CGdiObject",
            Self::Wnd => "CWnd",
            Self::Menu => "CMenu",
        }
    }
    fn offsets(self) -> &'static [u32] {
        match self {
            Self::Dc => &[4, 8],
            Self::Gdi | Self::Menu => &[4],
            Self::Wnd => &[0x20],
        }
    }
    pub fn root(self, ctx: &CallCtx<'_>, handle: u32) -> String {
        format!(
            "__PocketHLE_MFC_{}:{}:{handle:08x}",
            self.class(),
            ctx.kernel.current_thread
        )
    }
    fn temporary(self, ctx: &CallCtx<'_>) -> String {
        format!(
            "__PocketHLE_MFC_{}_Temporary:{}:",
            self.class(),
            ctx.kernel.current_thread
        )
    }
}
pub(super) fn from_handle(
    ctx: &mut CallCtx<'_>,
    kind: Kind,
    handle: u32,
) -> Result<u32, KernelError> {
    if handle == 0 {
        return Ok(0);
    }
    let owner = super::application::owner(ctx)?;
    if matches!(kind, Kind::Wnd) {
        if let Some(object) = super::windows::permanent(ctx, handle)? {
            return Ok(object);
        }
    }
    if matches!(kind, Kind::Menu) {
        if let Some(object) = super::menus::permanent(ctx, handle)? {
            return Ok(object);
        }
    }
    if let Some(&object) = ctx.kernel.dynamic_exports[&owner].get(&kind.root(ctx, handle)) {
        return Ok(object);
    }
    let key = format!("{}{handle:08x}", kind.temporary(ctx));
    if let Some(&object) = ctx.kernel.dynamic_exports[&owner].get(&key) {
        for &offset in kind.offsets() {
            put(ctx, object + offset, handle)?;
        }
        return Ok(object);
    }
    let valid = match kind {
        Kind::Dc => ctx.kernel.gdi.dc(handle).is_some(),
        Kind::Gdi => ctx.kernel.gdi.get(handle).is_some(),
        Kind::Wnd => crate::coredll::window_exists(ctx.kernel, handle),
        Kind::Menu => ctx.kernel.menu_items.contains_key(&handle),
    };
    if !valid {
        return Err(KernelError::Dispatch(format!(
            "MFC {}::FromHandle received an invalid handle",
            kind.class()
        )));
    }
    let table = super::classes::vtable(ctx, kind.class())?;
    let size = match kind {
        Kind::Dc => 20,
        Kind::Gdi | Kind::Menu => 8,
        Kind::Wnd => 72,
    };
    let object = alloc_cpp(ctx, size)?;
    let result = (|| {
        if matches!(kind, Kind::Wnd) {
            super::windows::initialize_target(ctx, object, "CWnd", 72)?;
        } else {
            ctx.cpu.write_mem(object, &vec![0; size as usize])?;
            put(ctx, object, table)?;
        }
        for &offset in kind.offsets() {
            put(ctx, object + offset, handle)?;
        }
        Ok::<_, KernelError>(())
    })();
    if let Err(e) = result {
        ctx.kernel.heap.free(object);
        return Err(e);
    }
    ctx.kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .insert(key, object);
    Ok(object)
}
pub(super) fn clear_temporary(
    ctx: &mut CallCtx<'_>,
    kind: Kind,
) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let owner = super::application::owner(ctx)?;
    let prefix = kind.temporary(ctx);
    let next = ctx.kernel.dynamic_exports[&owner]
        .iter()
        .find_map(|(k, &v)| k.starts_with(&prefix).then(|| (k.clone(), v)));
    let Some((key, object)) = next else {
        return f.finish(ctx, 0);
    };
    let count = f.get(ctx, 0)?;
    if count >= 4096 {
        return Err(KernelError::Dispatch(
            "MFC temporary-map cleanup exceeds its bound".into(),
        ));
    }
    f.set(ctx, 0, count + 1)?;
    let target = virtual_function(ctx, object, 1)?;
    for &offset in kind.offsets() {
        put(ctx, object + offset, 0)?;
    }
    ctx.kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .remove(&key);
    f.call_args(ctx, 1, target, &[object, 1])
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn gdi_permanent_identity_precedes_temporary_and_cleanup_preserves_borrowed_handles() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:104",
            &[0x2000],
        );
        let free = k.heap.free_bytes();
        let bitmap = k.gdi.create_compatible_bitmap(2, 2);
        let first = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1203",
            &[bitmap],
        ));
        let again = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1203",
            &[bitmap],
        ));
        assert_eq!(first, again);
        assert_eq!(cpu.read_u32_le(first + 4).unwrap(), bitmap);
        k.current_thread = 2;
        let other = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1203",
            &[bitmap],
        ));
        assert_ne!(first, other);
        k.current_thread = 0;
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:800",
                &[0x2000, bitmap]
            )),
            1
        );
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1203",
                &[bitmap]
            )),
            0x2000
        );
        drive(&mut cpu, &mut k, &mut d, entry(1045), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        assert!(k.heap.msize(first).is_none());
        assert!(k.heap.msize(other).is_some());
        assert!(k.gdi.bitmap(bitmap).is_some());
        assert_eq!(cpu.read_u32_le(0x2004).unwrap(), bitmap);
        k.current_thread = 2;
        drive(&mut cpu, &mut k, &mut d, entry(1045), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        assert_eq!(k.heap.free_bytes(), free);
        k.current_thread = 0;
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1061",
                &[0x2000]
            )),
            bitmap
        );
        assert!(k.gdi.bitmap(bitmap).is_some());
        assert!(!k.dynamic_exports[&1]
            .contains_key(&format!("__PocketHLE_MFC_CGdiObject:0:{bitmap:08x}")));
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:800",
            &[0x2000, bitmap],
        );
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1040",
                &[0x2000]
            )),
            1
        );
        assert!(k.gdi.bitmap(bitmap).is_none());
        assert!(!k.dynamic_exports[&1]
            .contains_key(&format!("__PocketHLE_MFC_CGdiObject:0:{bitmap:08x}")));
    }
    #[test]
    fn nested_temp_map_locks_keep_both_gdi_and_dc_wrappers_until_the_last_unlock() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let bitmap = k.gdi.create_compatible_bitmap(2, 2);
        let dc = k.gdi.create_memory_dc();
        let gdi_wrapper = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1203",
            &[bitmap],
        ));
        let dc_wrapper = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1202",
            &[dc],
        ));
        for _ in 0..2 {
            call(&mut cpu, &mut k, &mut d, "mfcce300.dll", "ord:667", &[]);
        }
        cpu.write_arg(0, 1).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(771), |pc, _| panic!(
                "unexpected {pc:x}"
            )),
            1
        );
        assert!(k.heap.msize(gdi_wrapper).is_some() && k.heap.msize(dc_wrapper).is_some());
        cpu.write_arg(0, 1).unwrap();
        assert_eq!(
            drive(&mut cpu, &mut k, &mut d, entry(771), |pc, _| panic!(
                "unexpected {pc:x}"
            )),
            0
        );
        assert!(k.heap.msize(gdi_wrapper).is_none() && k.heap.msize(dc_wrapper).is_none());
        assert!(k.gdi.bitmap(bitmap).is_some() && k.gdi.dc(dc).is_some());
    }
}

#[cfg(test)]
mod window_tests {
    use super::super::tests::{framework_setup, result};
    use super::super::window_dispatch::tests::{drive, entry};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn temporary_window_cleanup_detaches_without_destroying_or_replacing_a_permanent_window() {
        let (mut cpu, mut k, mut d) = framework_setup();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:206",
            &[0x2000],
        );
        let h = 0xdead1000;
        k.window_classes.insert(h, "Window".into());
        let first = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1207",
            &[h],
        ));
        assert_eq!(cpu.read_u32_le(first + 4).unwrap(), 1);
        assert_eq!(cpu.read_u32_le(first + 0x14).unwrap(), 1);
        assert_eq!(cpu.read_u32_le(first + 0x20).unwrap(), h);
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1207",
                &[h]
            )),
            first
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:804",
            &[0x2000, h],
        );
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1207",
                &[h]
            )),
            0x2000
        );
        drive(&mut cpu, &mut k, &mut d, entry(1048), |pc, _| {
            panic!("unexpected {pc:x}")
        });
        assert!(k.heap.msize(first).is_none());
        assert!(k.window_classes.contains_key(&h));
        assert_eq!(cpu.read_u32_le(0x2020).unwrap(), h);
        assert_eq!(
            k.dynamic_exports[&1]["__PocketHLE_MFC_Window:dead1000"],
            0x2000
        );
    }
}
