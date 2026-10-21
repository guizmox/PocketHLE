//! Pocket PC shell extensions (`aygshell.dll`).
//!
//! SDK ordinals and owned shell window identities.

use pocket_kernel::gdi::STOCK_SYSTEM_FONT;
use pocket_kernel::{DispatchOutcome, KernelError};

use crate::{CallCtx, WinCeDispatcher};

/// Handle we hand back as the menu bar / command bar window. Games
/// stash it in a global and `SendMessageW` to it later; our window
/// handlers tolerate an HWND they've never seen.
pub const FAKE_MENUBAR_HWND: u32 = 0xDEAD_0B01;

/// `SPI_GETSIPINFO` — the `SHSipInfo` action PPC2002 Solitaire uses
/// to query the soft input panel before toggling it.
const SPI_GETSIPINFO: u32 = 0xE1;

pub fn register(d: &mut WinCeDispatcher) {
    let dll = "aygshell.dll";
    for f in [
        "SHFullScreen",
        "SHCreateMenuBar",
        "SHCreateMenuBarEx",
        "SHCreateMenuBarInternal",
        "SHLoadImageFile",
        "SHHandleWMActivate",
        "SHHandleWMSettingChange",
        "SHInitDialog",
        "SHSipPreference",
        "SHSipInfo",
        "SHRecognizeGesture",
        "SHCloseApps",
        "SHDoneButton",
        "SHIdleTimerReset",
        "SHEnableSoftkey",
        "SHGetDocumentsFolder",
        "SHSetAppKeyWndAssoc",
    ] {
        let handler = match f {
            "SHCreateMenuBar" | "SHCreateMenuBarEx" | "SHCreateMenuBarInternal" => {
                sh_create_menu_bar
            }
            "SHInitDialog" => sh_init_dialog,
            "SHLoadImageFile" => sh_load_image_file,
            "SHSipInfo" => sh_sip_info,
            "SHCloseApps" => sh_close_apps,
            "SHSipPreference" => crate::shell_keyboard::preference,
            _ => ok,
        };
        d.register_handler(dll, f, handler);
    }

    // PPcAtaxx imports `aygshell.dll` purely by ordinal. The
    // ordinals don't appear in the public WM 5/6 SDK, but ord 21
    // shows up in the leaked WM 2003 lib (`aygshell.lib`) as the
    // helper that maps to "give the menu bar adornment about an
    // edit/menu split" — i.e. a PPC-style SHFullScreen variant.
    // PocketHLE has no real shell, so just succeeding is enough to
    // get past the call site.
    // 341 / 344 are the ordinals Gameloft's SDL port (Sonic Unleashed)
    // imports for its full-screen / task-bar handling.
    //
    // 4, 9 and 74 used to be blanket-stubbed here too. They now have
    // real handlers in the PPC2002 block below, which still answers
    // TRUE for 9 — so Pocket DeathMatch keeps the return value its
    // startup path checks.
    for ord in [12u16, 13, 14, 21, 40, 49, 50, 65, 71, 72, 80, 84, 341, 344] {
        d.register_handler(dll, &format!("ord:{ord}"), ok);
    }
    d.register_handler(dll, "ord:75", sh_load_image_file);
    // Racing Days requests 0x480000 bytes and treats FALSE as insufficient RAM.
    d.register_handler(dll, "ord:10", sh_close_apps);

    // ---- Pocket PC 2002 ordinal space -------------------------------
    //
    // Signatures recovered from `Solitaire` (PPC2002); addresses are
    // RVAs in that binary.
    //
    //   #4  0x12878  SHSipInfo(dwAction, uParam, pvParam, fWinIni) —
    //                called with dwAction=0x100e1, a 48-byte SIPINFO
    //                zeroed on the stack, and cbSize pre-filled to 48.
    //   #9  0x11398  no args, returns BOOL. WinMain does
    //                `movs r3, r0; beq <teardown>` right after, so a
    //                zero return sends the app straight to its
    //                seven-`LocalFree` exit path. This is the
    //                `SHInitExtraControls`-style "wire up the shell
    //                helpers" call every PPC2002 app makes just after
    //                `ImmDisableIME`.
    //   #22 0x12780  (hwnd, ?, fShow, ?) — paired with #33 inside an
    //                activate/hibernate helper; the return is ignored.
    //   #32 0x17984  (hwnd, pszText, pszCaption, ?, uType) returning a
    //                dialog result — the caller compares against 6
    //                (IDYES). A `SHMessageBoxCheck` equivalent.
    //   #33 0x1278c  (hwnd, ?) -> BOOL. A TRUE makes the caller
    //                `PostMessageW(hwnd, WM_CLOSE, 0, 0)`, so this one
    //                MUST answer FALSE or the game closes itself.
    //   #53 0x11db4  (hwnd, ?, 1) -> HFONT, forwarded straight into
    //                `WM_SETFONT` (msg 0x30) on a child control.
    //   #54 0x119dc  (hdc, lprc, 0, n) — shell draw helper called
    //                between `GetClientRect` and `EndPaint`. Purely
    //                decorative; a no-op leaves the board intact.
    //   #56 0x11dd4  SHInitDialog(SHINITDLGINFO*) with
    //                {dwMask=1, hDlg, dwFlags=0xd} — NOT SHFullScreen.
    //   #74 0x117a8  SHCreateMenuBar(SHMENUBARINFO*): the caller reads
    //                back +0x1c and stores it as the menu bar HWND, so
    //                we have to write a handle there.
    d.register_handler(dll, "ord:4", sh_sip_info);
    d.register_handler(dll, "ord:9", ok);
    d.register_handler(dll, "ord:22", ok);
    d.register_handler(dll, "ord:32", sh_message_box_check);
    d.register_handler(dll, "ord:33", zero);
    d.register_handler(dll, "ord:53", sh_get_ui_font);
    d.register_handler(dll, "ord:54", ok);
    d.register_handler(dll, "ord:34", sh_create_menu_bar);
    d.register_handler(dll, "ord:56", sh_init_dialog);
    d.register_handler(dll, "ord:21", crate::shell_keyboard::preference);
    d.register_handler(dll, "ord:74", sh_create_menu_bar);
}

fn ok(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn zero(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(0))
}

/// `SHCreateMenuBar(SHMENUBARINFO *pmb)`.
///
/// The PPC2002 struct is 36 bytes:
///
/// ```text
///   +0x00 cbSize        (36)
///   +0x04 hwndParent
///   +0x08 dwFlags
///   +0x0c nToolBarId
///   +0x10 hInstRes
///   +0x14 nBmpId
///   +0x18 cBmpImages
///   +0x1c hwndMB        <- out
///   +0x20 clrBk
/// ```
///
fn resource(ctx: &mut CallCtx<'_>, module: u32, kind: u32, id: u32) -> Option<Vec<u8>> {
    use pocket_pe::ResourceKey;
    let (e, base) = crate::coredll::lookup_resource(
        ctx.kernel,
        module,
        &ResourceKey::Id(kind),
        &ResourceKey::Id(id),
    )?;
    if e.size > 1048576 {
        return None;
    }
    ctx.cpu.read_mem(base.checked_add(e.data_rva)?, e.size).ok()
}
fn sh_create_menu_bar(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(0)?;
    let Ok(info) = ctx.cpu.read_mem(p, 32) else {
        return zero(ctx);
    };
    let get = |o| u32::from_le_bytes(info[o..o + 4].try_into().unwrap());
    if !matches!(get(0), 32 | 36) {
        return zero(ctx);
    }
    let (parent, flags, id, module) = (get(4), get(8), get(12), get(16));
    let Some(hwnd) = (0xdead0b01..0xdead0c00).find(|h| !ctx.kernel.window_classes.contains_key(h))
    else {
        return zero(ctx);
    };
    let mut toolbar_resource = None;
    let menu = if flags & 1 != 0 {
        0
    } else {
        let menu_id = if flags & 16 != 0 {
            id
        } else {
            let Some(bytes) = resource(ctx, module, 10, id) else {
                return zero(ctx);
            };
            if bytes.len() < 4 {
                return zero(ctx);
            }
            let menu_id = u16::from_le_bytes(bytes[..2].try_into().unwrap()) as u32;
            toolbar_resource = Some(bytes);
            menu_id
        };
        if flags & 16 != 0 && ctx.kernel.menu_items.contains_key(&menu_id) {
            menu_id
        } else {
            let Some(bytes) = resource(ctx, module, 4, menu_id) else {
                return zero(ctx);
            };
            let Some(h) = crate::coredll::menu_api::from_bytes(ctx.kernel, &bytes) else {
                return zero(ctx);
            };
            h
        }
    };
    if let Some(bytes) = toolbar_resource {
        let count = u16::from_le_bytes(bytes[2..4].try_into().unwrap()) as usize;
        if count > 4096 || bytes.len() < 4 + count * 14 {
            return zero(ctx);
        }
        let roots = ctx
            .kernel
            .menu_items
            .get(&menu)
            .cloned()
            .unwrap_or_default();
        let mut buttons = Vec::new();
        for record in bytes[4..4 + count * 14].chunks_exact(14) {
            let word = |i| u16::from_le_bytes(record[i..i + 2].try_into().unwrap()) as u32;
            let mut item = if word(6) & 8 != 0 {
                roots.get(word(12) as usize).cloned().unwrap_or_default()
            } else {
                Default::default()
            };
            item.id = word(2);
            if item.text.is_empty() {
                let sid = word(8);
                if let Some(block) = resource(ctx, module, 6, (sid >> 4) + 1) {
                    let mut offset = 0usize;
                    for slot in 0..16 {
                        let Some(length) = block
                            .get(offset..offset + 2)
                            .map(|v| u16::from_le_bytes(v.try_into().unwrap()) as usize)
                        else {
                            break;
                        };
                        offset += 2;
                        let Some(raw) = block.get(offset..offset + length * 2) else {
                            break;
                        };
                        if slot == sid & 15 {
                            item.text = raw
                                .chunks_exact(2)
                                .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
                                .collect();
                            break;
                        }
                        offset += length * 2;
                    }
                }
            }
            buttons.push(item);
        }
        ctx.kernel.menu_items.insert(menu, buttons);
    }
    if ctx.cpu.write_mem(p + 28, &hwnd.to_le_bytes()).is_err() {
        return zero(ctx);
    }
    let proc = ctx
        .kernel
        .dynamic_exports
        .values()
        .find_map(|e| e.get("DefWindowProcW").copied())
        .unwrap_or(0);
    ctx.kernel
        .window_classes
        .insert(hwnd, "ToolbarWindow32".into());
    ctx.kernel.window_procs.insert(hwnd, proc);
    ctx.kernel.window_parents.insert(hwnd, parent);
    ctx.kernel
        .window_threads
        .insert(hwnd, ctx.kernel.current_thread);
    // Shell-owned popup: its screen strip lies outside the application's client area.
    let visible = flags & 2 == 0;
    ctx.kernel
        .window_styles
        .insert(hwnd, (0x80000000 | if visible { 0x10000000 } else { 0 }, 0));
    ctx.kernel.window_visibility.insert(hwnd, visible);
    let w = ctx.kernel.framebuffer.width as i32;
    let h = ctx.kernel.framebuffer.height as i32;
    ctx.kernel.window_rects.insert(
        hwnd,
        (
            0,
            h - pocket_kernel::shell_menu::HEIGHT,
            w,
            pocket_kernel::shell_menu::HEIGHT,
        ),
    );
    let color = if flags & 4 != 0 && get(0) == 36 {
        ctx.cpu
            .read_mem(p + 32, 4)
            .ok()
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .unwrap_or(0xc0c0c0)
    } else {
        0xc0c0c0
    };
    let background = (((color & 255) >> 3) << 11
        | (((color >> 8) & 255) >> 2) << 5
        | ((color >> 16) & 255) >> 3) as u16;
    ctx.kernel.gdi.shell_menus.bars.insert(
        hwnd,
        pocket_kernel::shell_menu::Bar {
            owner: parent,
            menu,
            sip: flags & 8 == 0,
            background,
        },
    );
    log::debug!("window geometry shell bar hwnd={hwnd:x} owner={parent:x} flags={flags:x} resource={id:x} menu={menu:x} items={:?}",ctx.kernel.menu_items.get(&menu));
    ctx.kernel.framebuffer.mark_dirty();
    ok(ctx)
}
pub(crate) fn menu_input(
    ctx: &mut CallCtx<'_>,
    ev: pocket_kernel::InputEvent,
) -> Option<Option<(u32, u32, u32, u32)>> {
    let Some(handle) = ctx.kernel.active_shell_bar() else {
        ctx.kernel.gdi.shell_menus.close();
        return None;
    };
    let w = ctx.kernel.framebuffer.width as i32;
    let h = ctx.kernel.framebuffer.height as i32;
    let action = ctx.kernel.gdi.shell_menus.input(
        handle,
        ev,
        w,
        h,
        &ctx.kernel.menu_items,
        &ctx.kernel.menus,
    )?;
    ctx.kernel.framebuffer.mark_dirty();
    use pocket_kernel::shell_menu::Action;
    match action {
        Action::Command(owner, id) => Some(Some((owner, 0x111, id, 0))),
        Action::Open(owner, menu) => Some(Some((owner, 0x117, menu, 0))),
        Action::Sip => {
            crate::shell_keyboard::sip_show(ctx, !ctx.kernel.shell_keyboard.visible);
            Some(None)
        }
        Action::Consumed => Some(None),
    }
}
fn sh_init_dialog(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let p = ctx.arg_u32(0)?;
    let Ok(b) = ctx.cpu.read_mem(p, 12) else {
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    let get = |o| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let (mask, hwnd, flags) = (get(0), get(4), get(8));
    if !ctx.kernel.window_classes.contains_key(&hwnd) && ctx.kernel.controls.panel(hwnd).is_none() {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    if mask & 1 != 0 && flags & 8 != 0 {
        crate::shell_keyboard::sip_show(ctx, false);
    }
    if mask & 1 != 0 && flags & 0x14 != 0 {
        let (w, h) = (
            ctx.kernel.framebuffer.width as i32,
            ctx.kernel.framebuffer.height as i32,
        );
        ctx.kernel.window_rects.insert(hwnd, (0, 0, w, h));
        if let Some(s) = ctx.kernel.window_styles.get_mut(&hwnd) {
            s.0 &= !0x00c00000;
        }
        if let Some(p) = ctx.kernel.controls.panel_mut(hwnd) {
            p.x = 0;
            p.y = 0;
            p.w = w;
            p.h = h;
            p.border = false;
            p.caption = None;
        }
        crate::coredll::queue_window_size(ctx.kernel, hwnd, w, h);
        crate::coredll::queue_window_paint(ctx.kernel, hwnd);
    }
    Ok(DispatchOutcome::ReturnedR0(1))
}

/// `SHSipInfo(dwAction, uParam, pvParam, fWinIni)`.
///
/// Both historical action encodings share the real SIP state.
fn sh_sip_info(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let action = ctx.arg_u32(0)? & !0x10000;
    let pv = ctx.arg_u32(2)?;
    match action {
        SPI_GETSIPINFO => crate::shell_keyboard::sip_info(ctx, pv),
        0xE0 => crate::shell_keyboard::set_info(ctx, pv),
        _ => Ok(DispatchOutcome::ReturnedR0(0)),
    }
}

/// aygshell's shell-owned message box. The PPC2002 Solitaire call site
/// compares the result against `IDYES` (6).
///
/// `IDNO` is the conservative answer: it's what a user gets by
/// dismissing the prompt, and it avoids the destructive branch (the
/// one call site guards a "start over?" style question).
fn sh_message_box_check(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    const IDNO: u32 = 7;
    Ok(DispatchOutcome::ReturnedR0(IDNO))
}

/// Returns the shell UI font. The caller forwards it straight into
/// `WM_SETFONT`, and our GDI layer maps an unknown font handle onto
/// the system font anyway, so the stock handle is exactly right.
fn sh_get_ui_font(_ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(STOCK_SYSTEM_FONT))
}

fn sh_load_image_file(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let path_ptr = ctx.arg_u32(0)?;
    if path_ptr == 0 {
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let mut raw = Vec::new();
    for i in 0..260u32 {
        let bytes = match ctx.cpu.read_mem(path_ptr + i * 2, 2) {
            Ok(bytes) => bytes,
            Err(_) => break,
        };
        let unit = u16::from_le_bytes([bytes[0], bytes[1]]);
        if unit == 0 {
            break;
        }
        raw.push(unit);
    }
    let path = String::from_utf16_lossy(&raw);
    let Some(host_path) = ctx.kernel.vfs.resolve(&path) else {
        log::debug!("SHLoadImageFile({path:?}) -> NULL");
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    let bytes = match std::fs::read(&host_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            log::debug!("SHLoadImageFile({path:?}) read failed: {error}");
            return Ok(DispatchOutcome::ReturnedR0(0));
        }
    };
    let decoded = match image::load_from_memory(&bytes) {
        Ok(image) => image.to_rgba8(),
        Err(error) => {
            log::debug!("SHLoadImageFile({path:?}) decode failed: {error}");
            return Ok(DispatchOutcome::ReturnedR0(0));
        }
    };
    let (width, height) = decoded.dimensions();
    let handle = ctx.kernel.gdi.create_compatible_bitmap(width, height);
    let Some(bitmap) = ctx.kernel.gdi.bitmap_mut(handle) else {
        return Ok(DispatchOutcome::ReturnedR0(0));
    };
    for y in 0..height {
        for x in 0..width {
            let pixel = decoded.get_pixel(x, y).0;
            let offset = ((y * width + x) * 2) as usize;
            let rgb565 = pocket_kernel::framebuffer::pack_rgb565(pixel[0], pixel[1], pixel[2]);
            bitmap.pixels[offset..offset + 2].copy_from_slice(&rgb565.to_le_bytes());
        }
    }
    log::debug!("SHLoadImageFile({path:?}) -> 0x{handle:08x} ({width}x{height})");
    Ok(DispatchOutcome::ReturnedR0(handle))
}

/// Enough memory already available is success; do not allocate it, close the
/// current title, or report success for an impossible request. Cross-process
/// shell memory reclamation is not implemented.
fn sh_close_apps(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let requested = ctx.arg_u32(0)?;
    let physical = ctx
        .kernel
        .memory_division
        .as_ref()
        .map(|d| d.snapshot().program_free().saturating_mul(d.page_size()))
        .unwrap_or(24 * 1024 * 1024);
    let available = physical.min(ctx.kernel.heap.free_bytes());
    Ok(DispatchOutcome::ReturnedR0(u32::from(
        requested <= available,
    )))
}
#[cfg(test)]
mod memory_tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    #[test]
    fn shell_empty_bar_has_real_geometry_visibility_and_destruction() {
        use pocket_cpu::Cpu;
        let (mut cpu, mut k, mut d) = setup();
        k.framebuffer = pocket_kernel::Framebuffer::new(240, 320);
        k.active_window = 7;
        k.window_classes.insert(7, "Frame".into());
        k.window_visibility.insert(7, true);
        let info = [32u32, 7, 3, 0, 0, 0, 0, 0];
        cpu.write_mem(
            0x2000,
            &info
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "aygshell.dll",
                "SHCreateMenuBar",
                &[0x2000]
            ),
            DispatchOutcome::ReturnedR0(1)
        ));
        let handle = u32::from_le_bytes(cpu.read_mem(0x201c, 4).unwrap().try_into().unwrap());
        assert_eq!(k.window_rects[&handle], (0, 294, 240, 26));
        assert_eq!(k.active_shell_bar(), None);
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "ShowWindow",
            &[handle, 5],
        );
        assert_eq!(k.active_shell_bar(), Some(handle));
        k.menu_items.insert(
            123,
            vec![pocket_kernel::MenuItem {
                id: 55,
                submenu: 124,
                ..Default::default()
            }],
        );
        k.gdi.shell_menus.bars.get_mut(&handle).unwrap().menu = 123;
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SendMessageW",
                &[handle, 0x592, 0, 0]
            ),
            DispatchOutcome::ReturnedR0(123)
        ));
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "SendMessageW",
                &[handle, 0x591, 0, 55]
            ),
            DispatchOutcome::ReturnedR0(124)
        ));

        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "DestroyWindow",
            &[handle],
        );
        assert!(!k.gdi.shell_menus.bars.contains_key(&handle));
        assert_eq!(k.active_shell_bar(), None);
    }
    #[test]
    fn shell_memory_request_checks_capacity_without_allocating_or_closing() {
        let (mut cpu, mut k, mut d) = setup();
        // The shared API fixture deliberately has only a 64 KiB heap.
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "aygshell.dll",
                "ord:10",
                &[0x480000]
            ),
            DispatchOutcome::ReturnedR0(0)
        ));
        k.heap = pocket_kernel::Heap::new(0x100000, 8 * 1024 * 1024);
        let before = k.heap.free_bytes();
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "aygshell.dll",
                "ord:10",
                &[0x480000]
            ),
            DispatchOutcome::ReturnedR0(1)
        ));
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "aygshell.dll",
                "SHCloseApps",
                &[u32::MAX]
            ),
            DispatchOutcome::ReturnedR0(0)
        ));
        assert_eq!(k.heap.free_bytes(), before);
        assert!(!k.should_stop);
    }
}
