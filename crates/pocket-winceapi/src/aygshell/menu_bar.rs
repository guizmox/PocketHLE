//! Shell bar creation owns only resource menus and its display projection.
//! HWND/menu state uses existing kernel owners, including snapshot metadata.
use super::*;
use crate::coredll::menu_api;
const OWNED_ROOT: i32 = -1510;
const OWNED_DISPLAY: i32 = -1511;
const SOURCE_ROOT: i32 = -1512;

pub(crate) fn source_menu(k: &pocket_kernel::KernelState, hwnd: u32, display: u32) -> u32 {
    k.window_extra
        .get(&(hwnd, SOURCE_ROOT))
        .copied()
        .filter(|&h| h != 0)
        .unwrap_or(display)
}
pub(crate) fn discard(k: &mut pocket_kernel::KernelState, hwnd: u32) {
    // The projected toolbar borrows submenus from its source menu. Removing
    // that projection must never recursively delete a caller-owned submenu.
    if let Some(display) = k.window_extra.remove(&(hwnd, OWNED_DISPLAY)) {
        k.menu_items.remove(&display);
        k.menus.remove(&display);
    }
    if let Some(root) = k.window_extra.remove(&(hwnd, OWNED_ROOT)) {
        menu_api::destroy_owned(k, root);
    }
    k.window_extra.remove(&(hwnd, SOURCE_ROOT));
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
fn resource(
    ctx: &mut CallCtx<'_>,
    module: u32,
    kind: u32,
    id: u32,
) -> Result<Option<Vec<u8>>, KernelError> {
    use pocket_pe::ResourceKey;
    let Some((entry, base)) = crate::coredll::lookup_resource(
        ctx.kernel,
        module,
        &ResourceKey::Id(kind),
        &ResourceKey::Id(id),
    ) else {
        return Ok(None);
    };
    if entry.size > 1048576 {
        return Ok(None);
    }
    let Some(address) = base.checked_add(entry.data_rva) else {
        return Ok(None);
    };
    // Image reads use the common loader/VFS-backed image owner. Typed page-in
    // failures must propagate instead of looking like a missing menu resource.
    Ok(Some(ctx.cpu.read_mem(address, entry.size)?))
}
struct Prepared {
    display: u32,
    source: u32,
    owns_source: bool,
    owns_display: bool,
}
impl Prepared {
    fn discard(self, kernel: &mut pocket_kernel::KernelState) {
        if self.owns_display {
            kernel.menu_items.remove(&self.display);
            kernel.menus.remove(&self.display);
        }
        if self.owns_source {
            menu_api::destroy_owned(kernel, self.source);
        }
    }
    fn publish(&self, kernel: &mut pocket_kernel::KernelState, hwnd: u32) {
        if self.owns_source {
            kernel.window_extra.insert((hwnd, OWNED_ROOT), self.source);
        }
        if self.owns_display {
            kernel
                .window_extra
                .insert((hwnd, OWNED_DISPLAY), self.display);
        }
        if self.source != 0 {
            kernel.window_extra.insert((hwnd, SOURCE_ROOT), self.source);
        }
    }
}
fn prepare(
    ctx: &mut CallCtx<'_>,
    flags: u32,
    id: u32,
    module: u32,
) -> Result<Option<Prepared>, KernelError> {
    if flags & 1 != 0 {
        let display = menu_api::alloc(ctx.kernel, Vec::new());
        return Ok((display != 0).then_some(Prepared {
            display,
            source: 0,
            owns_source: false,
            owns_display: true,
        }));
    }
    if flags & 16 != 0 {
        if ctx.kernel.menu_items.contains_key(&id) {
            return Ok(Some(Prepared {
                display: id,
                source: id,
                owns_source: false,
                owns_display: false,
            }));
        }
        let Some(bytes) = resource(ctx, module, 4, id)? else {
            return Ok(None);
        };
        let Some(source) = menu_api::from_bytes(ctx.kernel, &bytes) else {
            return Ok(None);
        };
        return Ok(Some(Prepared {
            display: source,
            source,
            owns_source: true,
            owns_display: false,
        }));
    }
    let Some(toolbar) = resource(ctx, module, 10, id)? else {
        return Ok(None);
    };
    if toolbar.len() < 4 {
        return Ok(None);
    }
    let menu_id = u16::from_le_bytes(toolbar[..2].try_into().unwrap()) as u32;
    let count = u16::from_le_bytes(toolbar[2..4].try_into().unwrap()) as usize;
    if count > 4096 || toolbar.len() < 4 + count * 14 {
        return Ok(None);
    }
    let Some(menu_bytes) = resource(ctx, module, 4, menu_id)? else {
        return Ok(None);
    };
    let Some(template) = menu_api::MenuTemplate::parse(&menu_bytes) else {
        return Ok(None);
    };
    // Read only labels actually needed by the toolbar, before committing any
    // menu handles. Existing MENU labels must not page in unused STRINGTABLEs.
    let mut records = Vec::with_capacity(count);
    for record in toolbar[4..4 + count * 14].chunks_exact(14) {
        let fields: [u16; 7] = std::array::from_fn(|i| {
            u16::from_le_bytes(record[i * 2..i * 2 + 2].try_into().unwrap())
        });
        let has_label = if fields[3] & 8 != 0 {
            let Some(label) = template.root_label(fields[6] as usize) else {
                return Ok(None);
            };
            !label.is_empty()
        } else {
            false
        };
        let text = if has_label {
            Vec::new()
        } else {
            match resource(ctx, module, 6, (u32::from(fields[4]) >> 4) + 1)? {
                Some(bytes) => crate::coredll::decode_resource_string(&bytes, u32::from(fields[4])),
                None => Vec::new(),
            }
        };
        records.push((fields, text));
    }
    let Some(source) = template.install(ctx.kernel) else {
        return Ok(None);
    };
    let roots = ctx.kernel.menu_items[&source].clone();
    let mut items = Vec::with_capacity(count);
    for (fields, text) in records {
        let mut item = if fields[3] & 8 != 0 {
            let Some(root) = roots.get(fields[6] as usize) else {
                menu_api::destroy_owned(ctx.kernel, source);
                return Ok(None);
            };
            root.clone()
        } else {
            Default::default()
        };
        item.id = u32::from(fields[1]);
        if item.text.is_empty() {
            item.text = text;
        }
        items.push(item);
    }
    let display = menu_api::alloc(ctx.kernel, items);
    if display == 0 {
        menu_api::destroy_owned(ctx.kernel, source);
        return Ok(None);
    }
    Ok(Some(Prepared {
        source,
        display,
        owns_source: true,
        owns_display: true,
    }))
}
pub(crate) fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pointer = ctx.arg_u32(0)?;
    if ctx
        .cpu
        .check_guest_access(
            pointer,
            32,
            pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE,
        )
        .is_err()
    {
        return zero(ctx);
    }
    if ctx
        .cpu
        .check_guest_access(pointer + 28, 4, pocket_cpu::Prot::WRITE)
        .is_err()
    {
        return zero(ctx);
    }
    let bytes = ctx.cpu.read_mem(pointer, 32)?;
    let get = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let size = get(0);
    if !matches!(size, 32 | 36) {
        return zero(ctx);
    }
    if size == 36
        && ctx
            .cpu
            .check_guest_access(pointer, 36, pocket_cpu::Prot::READ)
            .is_err()
    {
        return zero(ctx);
    }
    let (parent, flags, id, module) = (get(4), get(8), get(12), get(16));
    if !crate::coredll::window_exists(ctx.kernel, parent) {
        return zero(ctx);
    }
    let Some(hwnd) = (0xdead0b01..0xdead0c00).find(|h| !ctx.kernel.window_classes.contains_key(h))
    else {
        return zero(ctx);
    };
    let color = if flags & 4 != 0 && size == 36 {
        ctx.cpu.read_u32_le(pointer + 32)?
    } else {
        0xc0c0c0
    };
    let Some(prepared) = prepare(ctx, flags, id, module)? else {
        return zero(ctx);
    };
    if let Err(error) = ctx.cpu.write_mem(pointer + 28, &hwnd.to_le_bytes()) {
        prepared.discard(ctx.kernel);
        return Err(error.into());
    }
    let menu = prepared.display;
    prepared.publish(ctx.kernel, hwnd);
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
    let background = (((color & 255) >> 3) << 11
        | (((color >> 8) & 255) >> 2) << 5
        | ((color >> 16) & 255) >> 3) as u16;
    ctx.kernel.gdi.shell_menus.bars.insert(
        hwnd,
        pocket_kernel::shell_menu::Bar {
            owner: parent,
            menu,
            sip: flags & 8 == 0 && !ctx.kernel.gdi.shell_menus.sip_button_hidden,
            background,
        },
    );
    log::debug!("window geometry shell bar hwnd={hwnd:x} owner={parent:x} flags={flags:x} resource={id:x} menu={menu:x} items={:?}",ctx.kernel.menu_items.get(&menu));
    ctx.kernel.framebuffer.mark_dirty();
    ok(ctx)
}

#[cfg(test)]
mod tests {
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::{Cpu, Prot};
    use pocket_kernel::{DispatchOutcome, MenuItem};
    use pocket_pe::{ResourceEntry, ResourceKey};
    fn info(cpu: &mut dyn Cpu, flags: u32, id: u32) {
        let fields = [32u32, 7, flags, id, 0, 0, 0, 0];
        cpu.write_mem(
            0x2000,
            &fields
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    fn menu_template() -> Vec<u8> {
        let mut data = vec![0u8; 4];
        for (flags, id, text) in [(0x90u16, 0u16, "File"), (0x80, 11, "Open")] {
            data.extend_from_slice(&flags.to_le_bytes());
            if flags & 16 == 0 {
                data.extend_from_slice(&id.to_le_bytes());
            }
            for ch in text.encode_utf16().chain([0]) {
                data.extend_from_slice(&ch.to_le_bytes());
            }
        }
        data
    }
    #[test]
    fn empty_bars_have_independent_menus_and_release_only_their_own_handles() {
        let (mut cpu, mut k, mut d) = setup();
        k.window_classes.insert(7, "Frame".into());
        let mut bars = Vec::new();
        let baseline = k.menu_items.len();
        for _ in 0..2 {
            info(&mut cpu, 1, 0);
            assert_eq!(
                call(
                    &mut cpu,
                    &mut k,
                    &mut d,
                    "aygshell.dll",
                    "SHCreateMenuBar",
                    &[0x2000]
                ),
                DispatchOutcome::ReturnedR0(1)
            );
            let hwnd = cpu.read_u32_le(0x201c).unwrap();
            bars.push((hwnd, k.gdi.shell_menus.bars[&hwnd].menu));
        }
        assert_ne!(bars[0].1, 0);
        assert_ne!(bars[0].1, bars[1].1);
        for (hwnd, menu) in bars {
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "DestroyWindow",
                &[hwnd],
            );
            assert!(!k.menu_items.contains_key(&menu));
            assert!(!k.window_extra.keys().any(|(h, _)| *h == hwnd));
        }
        assert_eq!(k.menu_items.len(), baseline);
    }
    #[test]
    fn borrowed_hmenu_and_nested_submenus_survive_bar_destruction() {
        let (mut cpu, mut k, mut d) = setup();
        k.window_classes.insert(7, "Frame".into());
        let submenu = super::menu_api::alloc(
            &mut k,
            vec![MenuItem {
                id: 11,
                ..Default::default()
            }],
        );
        let root = super::menu_api::alloc(
            &mut k,
            vec![MenuItem {
                submenu,
                ..Default::default()
            }],
        );
        info(&mut cpu, 16, root);
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "aygshell.dll",
                "SHCreateMenuBar",
                &[0x2000]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        let hwnd = cpu.read_u32_le(0x201c).unwrap();
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "DestroyWindow",
            &[hwnd],
        );
        assert!(k.menu_items.contains_key(&root));
        assert!(k.menu_items.contains_key(&submenu));
    }
    #[test]
    fn owned_resource_menu_projection_keeps_source_roots_until_bar_destruction() {
        let (mut cpu, mut k, mut d) = setup();
        k.window_classes.insert(7, "Frame".into());
        k.image_base = 0x1000;
        let menu = menu_template();
        cpu.write_mem(0x3000, &menu).unwrap();
        let mut toolbar = [0u8; 18];
        toolbar[..2].copy_from_slice(&128u16.to_le_bytes());
        toolbar[2..4].copy_from_slice(&1u16.to_le_bytes());
        toolbar[6..8].copy_from_slice(&55u16.to_le_bytes());
        toolbar[10..12].copy_from_slice(&8u16.to_le_bytes());
        cpu.write_mem(0x3200, &toolbar).unwrap();
        for (kind, rva, len) in [(4, 0x2000, menu.len()), (10, 0x2200, toolbar.len())] {
            k.resources.push(ResourceEntry {
                ty: ResourceKey::Id(kind),
                name: ResourceKey::Id(128),
                data_rva: rva,
                size: len as u32,
                codepage: 0,
                native_leaves: Vec::new(),
            });
        }
        k.resources.push(ResourceEntry {
            ty: ResourceKey::Id(6),
            name: ResourceKey::Id(1),
            data_rva: 0x1000000,
            size: 4,
            codepage: 0,
            native_leaves: Vec::new(),
        });
        info(&mut cpu, 0, 128);
        let baseline = k.menu_items.len();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "aygshell.dll",
                "SHCreateMenuBar",
                &[0x2000]
            ),
            DispatchOutcome::ReturnedR0(1)
        );
        let hwnd = cpu.read_u32_le(0x201c).unwrap();
        let display = k.gdi.shell_menus.bars[&hwnd].menu;
        let source = super::source_menu(&k, hwnd, display);
        assert_ne!(source, display);
        assert_eq!(
            k.menu_items[&source][0].text,
            "File".encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(k.menu_items[&display][0].id, 55);
        assert_eq!(
            k.menu_items[&source][0].submenu,
            k.menu_items[&display][0].submenu
        );
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "DestroyWindow",
            &[hwnd],
        );
        assert_eq!(k.menu_items.len(), baseline);
        assert_eq!(k.menus.len(), baseline);
        // Malformed toolbar is rejected before allocating any menu tree.
        cpu.write_mem(0x3202, &4097u16.to_le_bytes()).unwrap();
        info(&mut cpu, 0, 128);
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "aygshell.dll",
                "SHCreateMenuBar",
                &[0x2000]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(k.menu_items.len(), baseline);
        assert!(k.gdi.shell_menus.bars.is_empty());
    }
    #[test]
    fn protected_output_does_not_allocate_or_publish_a_menu_bar() {
        let (mut cpu, mut k, mut d) = setup();
        k.window_classes.insert(7, "Frame".into());
        info(&mut cpu, 1, 0);
        cpu.protect_region(0x2000, 0x1000, Prot::READ).unwrap();
        let next = k.next_menu_handle;
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "aygshell.dll",
                "SHCreateMenuBar",
                &[0x2000]
            ),
            DispatchOutcome::ReturnedR0(0)
        );
        assert!(k.menu_items.is_empty());
        assert!(k.gdi.shell_menus.bars.is_empty());
        assert_eq!(k.next_menu_handle, next);
    }
}
