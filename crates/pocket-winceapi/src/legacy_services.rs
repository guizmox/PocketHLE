//! CE notification-area bookkeeping and the Winsock 1 local host database.
//! No host tray UI, Bluetooth dependency, external DNS or IPv4 socket transport.
use crate::{CallCtx, WinCeDispatcher};
use pocket_cpu::Prot;
use pocket_kernel::{DispatchOutcome, KernelError};

pub fn register(d: &mut WinCeDispatcher) {
    for dll in ["winsock.dll", "wsock32.dll", "ws2_32.dll"] {
        for (name, handler) in [
            ("gethostname", hostname as crate::Handler),
            ("gethostbyname", host_by_name),
            ("WSAStartup", startup),
            ("WSACleanup", cleanup),
            ("WSAGetLastError", last_error),
            ("WSASetLastError", set_error),
        ] {
            d.register_handler(dll, name, handler);
        }
    }
    for name in ["Shell_NotifyIcon", "Shell_NotifyIconW"] {
        d.register_handler("coredll.dll", name, notify_icon);
    }
}
fn access(ctx: &CallCtx<'_>, p: u32, n: u32, prot: Prot) -> bool {
    p != 0 && p.checked_add(n).is_some() && ctx.cpu.check_guest_access(p, n, prot).is_ok()
}
fn error(ctx: &mut CallCtx<'_>, code: u32, result: u32) -> Result<DispatchOutcome, KernelError> {
    ctx.kernel
        .winsock_last_errors
        .insert(ctx.kernel.current_thread, code);
    Ok(DispatchOutcome::ReturnedR0(result))
}
fn last_error(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(
        ctx.kernel
            .winsock_last_errors
            .get(&ctx.kernel.current_thread)
            .copied()
            .unwrap_or(0),
    ))
}
fn set_error(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let code = ctx.arg_u32(0)?;
    ctx.kernel
        .winsock_last_errors
        .insert(ctx.kernel.current_thread, code);
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn startup(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let version = ctx.arg_u32(0)? as u16;
    let p = ctx.arg_u32(1)?;
    if !matches!(version, 0x101 | 0x102 | 0x2 | 0x202) {
        return error(ctx, 10092, 10092);
    }
    if !access(ctx, p, 400, Prot::WRITE) {
        return error(ctx, 10014, 10014);
    }
    let Some(count) = ctx.kernel.legacy_winsock.startups.checked_add(1) else {
        return error(ctx, 10055, 10055);
    };
    let mut data = [0u8; 400];
    data[..2].copy_from_slice(&version.to_le_bytes());
    data[2..4].copy_from_slice(&0x202u16.to_le_bytes());
    let description = b"PocketHLE local host database";
    data[4..4 + description.len()].copy_from_slice(description);
    ctx.cpu.write_mem(p, &data)?;
    ctx.kernel.legacy_winsock.startups = count;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn cleanup(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if ctx.kernel.legacy_winsock.startups == 0 {
        return error(ctx, 10093, u32::MAX);
    }
    ctx.kernel.legacy_winsock.startups -= 1;
    if ctx.kernel.legacy_winsock.startups == 0 {
        for (_, p) in ctx.kernel.legacy_winsock.hostent_buffers.drain() {
            ctx.kernel.heap.free(p);
        }
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn hostname(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if ctx.kernel.legacy_winsock.startups == 0 {
        return error(ctx, 10093, u32::MAX);
    }
    let p = ctx.arg_u32(0)?;
    let len = ctx.arg_u32(1)?;
    let name = b"PocketHLE\0";
    if len > i32::MAX as u32
        || len < name.len() as u32
        || !access(ctx, p, name.len() as u32, Prot::WRITE)
    {
        return error(ctx, 10014, u32::MAX);
    }
    ctx.cpu.write_mem(p, name)?;
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn host_by_name(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    if ctx.kernel.legacy_winsock.startups == 0 {
        return error(ctx, 10093, 0);
    }
    let p = ctx.arg_u32(0)?;
    let mut name = Vec::new();
    for offset in 0..256 {
        let Some(a) = p.checked_add(offset) else {
            return error(ctx, 10014, 0);
        };
        if !access(ctx, a, 1, Prot::READ) {
            return error(ctx, 10014, 0);
        }
        let c = ctx.cpu.read_mem(a, 1)?[0];
        if c == 0 {
            break;
        }
        name.push(c);
        if offset == 255 {
            return error(ctx, 10014, 0);
        }
    }
    let Ok(name) = std::str::from_utf8(&name) else {
        return error(ctx, 11001, 0);
    };
    // The virtual device has loopback only. Never invent an external DNS answer
    // or report the host computer's LAN address as the guest's network adapter.
    if !name.eq_ignore_ascii_case("PocketHLE") && !name.eq_ignore_ascii_case("localhost") {
        return error(ctx, 11001, 0);
    }
    let thread = ctx.kernel.current_thread;
    let base = if let Some(&p) = ctx.kernel.legacy_winsock.hostent_buffers.get(&thread) {
        p
    } else {
        let DispatchOutcome::ReturnedR0(p) = crate::coredll::do_alloc(ctx, 288)? else {
            unreachable!()
        };
        if p == 0 {
            return error(ctx, 10055, 0);
        }
        ctx.kernel.legacy_winsock.hostent_buffers.insert(thread, p);
        p
    };
    if !access(ctx, base, 288, Prot::WRITE) {
        return error(ctx, 10014, 0);
    }
    let mut data = [0u8; 288];
    data[0..4].copy_from_slice(&(base + 32).to_le_bytes());
    data[4..8].copy_from_slice(&(base + 16).to_le_bytes()); // empty alias list
    data[8..10].copy_from_slice(&2u16.to_le_bytes()); // AF_INET
    data[10..12].copy_from_slice(&4u16.to_le_bytes());
    data[12..16].copy_from_slice(&(base + 20).to_le_bytes());
    data[20..24].copy_from_slice(&(base + 28).to_le_bytes()); // address then NULL
    data[28..32].copy_from_slice(&[127, 0, 0, 1]); // network byte order
    data[32..32 + name.len()].copy_from_slice(name.as_bytes());
    ctx.cpu.write_mem(base, &data)?;
    Ok(DispatchOutcome::ReturnedR0(base))
}
fn shell_error(ctx: &mut CallCtx<'_>, code: u32) -> Result<DispatchOutcome, KernelError> {
    crate::coredll::set_thread_error(ctx, code);
    Ok(DispatchOutcome::ReturnedR0(0))
}
fn notify_icon(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let action = ctx.arg_u32(0)?;
    let p = ctx.arg_u32(1)?;
    if action > 2 {
        return shell_error(ctx, 50);
    }
    if !access(ctx, p, 152, Prot::READ) {
        return shell_error(ctx, 87);
    }
    let bytes = ctx.cpu.read_mem(p, 152)?;
    let word = |n: usize| u32::from_le_bytes(bytes[n..n + 4].try_into().unwrap());
    if word(0) != 152 {
        return shell_error(ctx, 87);
    }
    let key = (word(4), word(8));
    let flags = word(12);
    if action == 2 {
        return if ctx.kernel.notification_icons.remove(&key).is_some() {
            Ok(DispatchOutcome::ReturnedR0(1))
        } else {
            shell_error(ctx, 2)
        };
    }
    if flags & !7 != 0 {
        return shell_error(ctx, 50);
    }
    if flags & 1 != 0 && (key.0 == 0 || !ctx.kernel.window_rects.contains_key(&key.0)) {
        return shell_error(ctx, 1400);
    }
    let image = if flags & 2 != 0 && word(20) != 0 {
        let Some(i) = ctx.kernel.gdi.icons.get(&word(20)).cloned() else {
            return shell_error(ctx, 6);
        };
        Some(i)
    } else {
        None
    };
    let exists = ctx.kernel.notification_icons.contains_key(&key);
    if action == 0 && exists {
        return shell_error(ctx, 183);
    }
    if action == 1 && !exists {
        return shell_error(ctx, 2);
    }
    if !exists && ctx.kernel.notification_icons.len() >= 256 {
        return shell_error(ctx, 8);
    }
    let icon = ctx.kernel.notification_icons.entry(key).or_default();
    if flags & 1 != 0 {
        icon.callback = word(16);
    }
    if flags & 2 != 0 {
        icon.image = image;
    }
    if flags & 4 != 0 {
        icon.tooltip = String::from_utf16_lossy(
            &bytes[24..]
                .chunks_exact(2)
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .take_while(|&v| v != 0)
                .collect::<Vec<_>>(),
        );
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;
    #[test]
    fn compatibility_update_legacy_local_host_lookup_validates_buffers_and_reuses_thread_storage() {
        let (mut cpu, mut k, mut d) = setup();
        k.vfs.bluetooth.service.enable(false);
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "winsock.dll",
                "gethostname",
                &[0x1200, 64]
            ),
            DispatchOutcome::ReturnedR0(u32::MAX)
        );
        assert_eq!(k.winsock_last_errors[&0], 10093);
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "winsock.dll",
                "WSAStartup",
                &[0x101, 0x2000]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        cpu.write_mem(0x1200, &[0x55; 20]).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "winsock.dll",
                "gethostname",
                &[0x1200, 2]
            ),
            DispatchOutcome::ReturnedR0(u32::MAX)
        );
        assert_eq!(cpu.read_mem(0x1200, 20).unwrap(), [0x55; 20]);
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "winsock.dll",
                "gethostname",
                &[0x1200, 64]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(cpu.read_mem(0x1200, 10).unwrap(), b"PocketHLE\0");
        let allocation = k.heap.alloc(288).unwrap();
        cpu.map_region(allocation & !4095, 8192, Prot::READ | Prot::WRITE)
            .unwrap();
        k.heap.free(allocation);
        let DispatchOutcome::ReturnedR0(p) = call(
            &mut cpu,
            &mut k,
            &mut d,
            "winsock.dll",
            "gethostbyname",
            &[0x1200],
        ) else {
            panic!()
        };
        assert_ne!(p, 0);
        assert_eq!(cpu.read_mem(p + 8, 4).unwrap(), [2, 0, 4, 0]);
        let list = cpu.read_u32_le(p + 12).unwrap();
        let address = cpu.read_u32_le(list).unwrap();
        assert_eq!(cpu.read_mem(address, 4).unwrap(), [127, 0, 0, 1]);
        assert_eq!(cpu.read_u32_le(list + 4).unwrap(), 0);
        let aliases = cpu.read_u32_le(p + 4).unwrap();
        assert_eq!(cpu.read_u32_le(aliases).unwrap(), 0);
        for _ in 0..16 {
            assert_eq!(
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "winsock.dll",
                    "gethostbyname",
                    &[0x1200]
                ),
                DispatchOutcome::ReturnedR0(p)
            );
        }
        cpu.write_mem(0x1200, b"no-such-host.invalid\0").unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "winsock.dll",
                "gethostbyname",
                &[0x1200]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(k.winsock_last_errors[&0], 11001);
        assert_eq!(
            call(&mut cpu, &mut k, &mut d, "winsock.dll", "WSACleanup", &[]),
            DispatchOutcome::ReturnedR0(0)
        );
        assert!(k.heap.msize(p).is_none());
        assert!(k.legacy_winsock.hostent_buffers.is_empty());
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "winsock.dll",
                "WSAStartup",
                &[0x101, 0x2000]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        cpu.write_mem(0x1200, b"localhost\0").unwrap();
        let DispatchOutcome::ReturnedR0(p) = call(
            &mut cpu,
            &mut k,
            &mut d,
            "winsock.dll",
            "gethostbyname",
            &[0x1200],
        ) else {
            panic!()
        };
        k.main_thread.exit_code = Some(0);
        k.reclaim_finished_stacks(&mut cpu).unwrap();
        assert!(k.heap.msize(p).is_none());
        assert!(k.legacy_winsock.hostent_buffers.is_empty());
    }
    #[test]
    fn compatibility_update_notify_icon_lifecycle_partial_updates_and_invalid_data_are_atomic() {
        let (mut cpu, mut k, mut d) = setup();
        let hwnd = 0xdead1003;
        k.window_rects.insert(hwnd, (0, 0, 20, 20));
        k.gdi.icons.insert(
            0xdead1234,
            pocket_kernel::imaging::Image::new(2, 2).unwrap(),
        );
        let mut data = [0u8; 152];
        for (offset, value) in [
            (0, 152u32),
            (4, hwnd),
            (8, 5001),
            (12, 7),
            (16, 0x401),
            (20, 0xdead1234),
        ] {
            data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        data[24..28].copy_from_slice(&[b'G', 0, 0, 0]);
        cpu.write_mem(0x1200, &data).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "Shell_NotifyIcon",
                &[0, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        k.gdi.icons.remove(&0xdead1234);
        assert!(k.notification_icons[&(hwnd, 5001)].image.is_some());
        data[12..16].copy_from_slice(&1u32.to_le_bytes());
        data[16..20].copy_from_slice(&0x402u32.to_le_bytes());
        cpu.write_mem(0x1200, &data).unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "Shell_NotifyIcon",
                &[1, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(k.notification_icons[&(hwnd, 5001)].tooltip, "G");
        assert_eq!(k.notification_icons[&(hwnd, 5001)].callback, 0x402);
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "Shell_NotifyIcon",
                &[0, 0]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(k.notification_icons.len(), 1);
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "Shell_NotifyIcon",
                &[2, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        assert!(k.notification_icons.is_empty());
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "Shell_NotifyIcon",
                &[2, 0x1200]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
    }
}
