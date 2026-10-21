use super::tests::{fresh_kernel, lifecycle_test_module, scanf_cpu, thunk_at};
use super::*;
use pocket_cpu::Cpu;
use pocket_kernel::{imaging::Image, Framebuffer};

fn call(ctx: &mut CallCtx<'_>, handler: crate::Handler, args: &[u32]) -> u32 {
    let regs = [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3];
    for (i, value) in args.iter().copied().enumerate() {
        if i < 4 {
            ctx.cpu.write_reg(regs[i], value).unwrap();
        } else {
            let sp = ctx.cpu.read_reg(ArmReg::Sp).unwrap();
            ctx.cpu
                .write_mem(sp + (i as u32 - 4) * 4, &value.to_le_bytes())
                .unwrap();
        }
    }
    match handler(ctx).unwrap() {
        DispatchOutcome::ReturnedR0(v) => v,
        other => panic!("unexpected {other:?}"),
    }
}
fn entry(ty: u32, id: u32, address: u32, raw: &[u8]) -> ResourceEntry {
    ResourceEntry {
        ty: ResourceKey::Id(ty),
        name: ResourceKey::Id(id),
        data_rva: address,
        size: raw.len() as u32,
        codepage: 0,
    }
}
fn dib24(w: u32, h: u32, pixels: &[u32]) -> Vec<u8> {
    assert_eq!(pixels.len(), (w * h) as usize);
    let stride = (w * 3 + 3) & !3;
    let mut raw = vec![0u8; 40 + (stride * h) as usize];
    raw[..4].copy_from_slice(&40u32.to_le_bytes());
    raw[4..8].copy_from_slice(&w.to_le_bytes());
    raw[8..12].copy_from_slice(&h.to_le_bytes());
    raw[12..14].copy_from_slice(&1u16.to_le_bytes());
    raw[14..16].copy_from_slice(&24u16.to_le_bytes());
    for y in 0..h {
        for x in 0..w {
            let p = pixels[(y * w + x) as usize];
            let i = 40 + ((h - 1 - y) * stride + x * 3) as usize;
            raw[i..i + 3].copy_from_slice(&[p as u8, (p >> 8) as u8, (p >> 16) as u8]);
        }
    }
    raw
}
fn icon32(color: u32) -> Vec<u8> {
    let mut raw = vec![0u8; 64];
    raw[..4].copy_from_slice(&40u32.to_le_bytes());
    raw[4..8].copy_from_slice(&2u32.to_le_bytes());
    raw[8..12].copy_from_slice(&4u32.to_le_bytes());
    raw[12..14].copy_from_slice(&1u16.to_le_bytes());
    raw[14..16].copy_from_slice(&32u16.to_le_bytes());
    for i in 0..4 {
        raw[40 + i * 4..43 + i * 4].copy_from_slice(&[
            color as u8,
            (color >> 8) as u8,
            (color >> 16) as u8,
        ]);
    }
    raw[60] = 0x80; // top-left AND bit; all alpha bytes are zero.
    raw
}
fn icon_group(id: u16, bytes: usize) -> Vec<u8> {
    let mut group = vec![0u8; 20];
    group[2..4].copy_from_slice(&1u16.to_le_bytes());
    group[4..6].copy_from_slice(&1u16.to_le_bytes());
    group[6] = 2;
    group[7] = 2;
    group[10..12].copy_from_slice(&1u16.to_le_bytes());
    group[12..14].copy_from_slice(&32u16.to_le_bytes());
    group[14..18].copy_from_slice(&(bytes as u32).to_le_bytes());
    group[18..20].copy_from_slice(&id.to_le_bytes());
    group
}
fn pixel(kernel: &KernelState, x: u32, y: u32) -> u16 {
    let i = ((y * kernel.framebuffer.width + x) * 2) as usize;
    u16::from_le_bytes([
        kernel.framebuffer.pixels[i],
        kernel.framebuffer.pixels[i + 1],
    ])
}
#[test]
fn image_list_resource_strip_draws_transparent_tiles_and_drawex_crops() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    kernel.framebuffer = Framebuffer::new(8, 4);
    kernel.framebuffer.fill(0xffff);
    let bitmap = dib24(
        4,
        2,
        &[
            0xff00ff, 0xff0000, 0x00ff00, 0x0000ff, 0xff0000, 0xffffff, 0x0000ff, 0x00ff00,
        ],
    );
    cpu.write_mem(0x2000, &bitmap).unwrap();
    kernel.resources.push(entry(2, 164, 0x2000, &bitmap));
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let list = call(
        &mut ctx,
        images::load_list,
        &[0x10000000, 164, 2, 10, 0xff00ff, 0, 0],
    );
    assert_ne!(list, 0);
    assert_eq!(call(&mut ctx, images::count, &[list]), 2);
    assert_eq!(
        call(&mut ctx, images::draw, &[list, 0, GDI_SCREEN_DC, 1, 0, 1]),
        1
    );
    assert_eq!(pixel(ctx.kernel, 1, 0), 0xffff);
    assert_eq!(pixel(ctx.kernel, 2, 0), 0xf800);
    assert_eq!(
        call(
            &mut ctx,
            images::draw_ex,
            &[list, 1, GDI_SCREEN_DC, 4, 1, 1, 1, u32::MAX, 0xff000000, 1]
        ),
        1
    );
    assert_eq!(pixel(ctx.kernel, 4, 1), 0x07e0);
    assert_eq!(pixel(ctx.kernel, 5, 1), 0xffff);
    assert_eq!(
        call(&mut ctx, images::draw, &[list, 2, GDI_SCREEN_DC, 0, 0, 1]),
        0
    );
    assert_eq!(call(&mut ctx, images::destroy, &[list]), 1);
    assert_eq!(call(&mut ctx, images::count, &[list]), 0);
}
#[test]
fn icons_decode_and_masks_and_resolve_their_owning_module() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let red = icon32(0xff0000);
    let green = icon32(0x00ff00);
    let group = icon_group(5, red.len());
    cpu.write_mem(0x2000, &red).unwrap();
    cpu.write_mem(0x2800, &group).unwrap();
    kernel.resources = vec![entry(3, 5, 0x2000, &red), entry(14, 1, 0x2800, &group)];
    cpu.map_region(0x30000000, 0x1000, Prot::READ | Prot::WRITE)
        .unwrap();
    cpu.write_mem(0x30000100, &green).unwrap();
    cpu.write_mem(0x30000200, &group).unwrap();
    let mut module = lifecycle_test_module(0x30000000, 0);
    module.resources = vec![entry(3, 5, 0x100, &green), entry(14, 1, 0x200, &group)];
    kernel.modules.push(module);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let first = call(&mut ctx, images::load_icon, &[0x10000000, 1]);
    let second = call(&mut ctx, images::load_icon, &[0x30000000, 1]);
    assert_ne!(first, 0);
    assert_ne!(first, second);
    assert_eq!(ctx.kernel.gdi.icons[&first].alpha[0], 0);
    assert_eq!(ctx.kernel.gdi.icons[&second].pixels[31], 0x07e0);
    assert_eq!(ctx.kernel.gdi.icons[&second].alpha[31], 255);
    let list = call(&mut ctx, images::create, &[2, 2, 1, 1, 1]);
    assert_eq!(
        call(&mut ctx, images::replace_icon, &[list, u32::MAX, second]),
        0
    );
    assert_eq!(
        call(&mut ctx, images::replace_icon, &[0, u32::MAX, second]),
        u32::MAX
    );
    assert_eq!(call(&mut ctx, images::destroy_icon, &[second]), 1);
    assert_eq!(
        ctx.kernel.gdi.image_lists[&list].images[0].pixels[1],
        0x07e0
    );
    assert_eq!(call(&mut ctx, images::load_icon, &[0x30000000, 77]), 0);
}
#[test]
fn icon_partial_alpha_is_used_but_all_zero_alpha_falls_back_to_and_mask() {
    let mut raw = icon32(0xff0000);
    let image = images::decode_icon(&raw).unwrap();
    assert_eq!(image.alpha, vec![0, 255, 255, 255]);
    raw[51] = 128;
    let image = images::decode_icon(&raw).unwrap();
    assert_eq!(image.alpha[0], 128);
    assert_eq!(image.alpha[1], 0);
    let mut bitmap = pocket_kernel::gdi::Bitmap::new(2, 2);
    bitmap.pixels.fill(0xff);
    image.draw(&mut Surface::Bitmap(&mut bitmap), 0, 0, 2, 2, [0, 0, 2, 2]);
    assert_ne!(&bitmap.pixels[..2], &[0xff, 0xff]);
    assert_ne!(&bitmap.pixels[..2], &[0, 0xf8]);
    assert_eq!(&bitmap.pixels[2..4], &[0xff, 0xff]);
}
#[test]
fn image_list_storage_is_independent_and_invalid_replace_never_succeeds() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let a = call(&mut ctx, images::create, &[16, 16, 1, 4, 4]);
    let b = call(&mut ctx, images::create, &[32, 32, 1, 1, 1]);
    assert_ne!(a, b);
    let icon = ctx.kernel.gdi.register_icon(Image::new(2, 2).unwrap());
    assert_eq!(
        call(&mut ctx, images::replace_icon, &[a, u32::MAX, icon]),
        0
    );
    assert_eq!(call(&mut ctx, images::count, &[a]), 1);
    assert_eq!(call(&mut ctx, images::count, &[b]), 0);
    assert_eq!(
        call(&mut ctx, images::replace_icon, &[a, 1, icon]),
        u32::MAX
    );
    assert_eq!(call(&mut ctx, images::replace_icon, &[a, 0, icon]), 0);
    assert_eq!(call(&mut ctx, images::get_size, &[a, 0x2000, 0x2004]), 1);
    assert_eq!(ctx.cpu.read_u32_le(0x2000).unwrap(), 16);
    assert_eq!(call(&mut ctx, images::set_size, &[a, 24, 24]), 1);
    assert_eq!(call(&mut ctx, images::count, &[a]), 0);
}
#[test]
fn list_view_renders_its_livitem_image_and_hit_testing_uses_image_height() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    kernel.framebuffer = Framebuffer::new(80, 50);
    let hwnd = kernel.controls.create(
        FAKE_HWND,
        ControlClass::ListView,
        51,
        String::new(),
        0x50000001,
        0,
        0,
        80,
        46,
    );
    let mut image = Image::new(20, 20).unwrap();
    image.pixels.fill(0x07e0);
    let icon = kernel.gdi.register_icon(image);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let list = call(&mut ctx, images::create, &[20, 20, 1, 1, 1]);
    call(&mut ctx, images::replace_icon, &[list, u32::MAX, icon]);
    list_view_message(&mut ctx, hwnd, 0x1003, 1, list).unwrap();
    assert_eq!(
        list_view_message(&mut ctx, hwnd, 0x1002, 1, 0).unwrap(),
        list
    );
    ctx.cpu
        .write_mem(0x2200, &[b'S', 0, b't', 0, b'o', 0, b'p', 0, 0, 0])
        .unwrap();
    for index in 0u32..2 {
        let fields = [3, index, 0, 0, 0, 0x2200, 5, 0, 0];
        ctx.cpu
            .write_mem(
                0x2100,
                &fields
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        assert_eq!(
            list_view_message(&mut ctx, hwnd, 0x104d, 0, 0x2100).unwrap(),
            index
        );
    }
    ctx.kernel.composite_controls();
    assert_eq!(pixel(ctx.kernel, 3, 3), 0x07e0);
    assert_eq!(pixel(ctx.kernel, 3, 25), 0x07e0);
    ctx.kernel.controls.pointer_down(5, 26);
    assert_eq!(
        ctx.kernel.controls.get(hwnd).unwrap().list_selected,
        Some(1)
    );
    let counter = ctx.kernel.framebuffer.frame_counter;
    ctx.kernel.composite_controls();
    assert_eq!(ctx.kernel.framebuffer.frame_counter, counter);
    assert_eq!(pixel(ctx.kernel, 3, 25), 0x07e0);
    assert_eq!(pixel(ctx.kernel, 1, 23), 0x001f);
}
#[test]
fn image_list_draw_flushes_a_guest_dib_after_clipped_drawing() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let bitmap = kernel.gdi.create_compatible_bitmap(3, 2);
    {
        let b = kernel.gdi.bitmap_mut(bitmap).unwrap();
        b.dib_bits_va = Some(0x2000);
        b.bpp = 16;
        b.dib_bottom_up = false;
        b.dib_row_stride = 8;
        b.dib_rgb555 = false;
    }
    cpu.write_mem(0x2000, &[0xff; 16]).unwrap();
    let dc = kernel.gdi.create_memory_dc();
    kernel.gdi.dc_mut(dc).unwrap().selected_bitmap = Some(bitmap);
    let mut image = Image::new(2, 2).unwrap();
    image.pixels.fill(0xf800);
    image.alpha[0] = 0;
    let icon = kernel.gdi.register_icon(image);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let list = call(&mut ctx, images::create, &[2, 2, 1, 1, 1]);
    call(&mut ctx, images::replace_icon, &[list, u32::MAX, icon]);
    assert_eq!(call(&mut ctx, images::draw, &[list, 0, dc, 1, 0, 1]), 1);
    assert_eq!(
        ctx.cpu.read_mem(0x2002, 4).unwrap(),
        vec![0xff, 0xff, 0, 0xf8]
    );
    assert_eq!(ctx.cpu.read_mem(0x2006, 2).unwrap(), vec![0xff, 0xff]);
}
#[test]
fn scroll_window_preserves_overlapping_pixels_and_reports_the_uncovered_region() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    kernel.framebuffer = Framebuffer::new(4, 3);
    for (i, p) in kernel.framebuffer.pixels.chunks_exact_mut(2).enumerate() {
        p.copy_from_slice(&(i as u16 + 1).to_le_bytes());
    }
    let region = kernel.gdi.create_region(vec![]);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(
        call(
            &mut ctx,
            window_graphics::scroll_window_ex,
            &[FAKE_HWND, 1, 0, 0, 0, region, 0x2000, 0]
        ),
        2
    );
    assert_eq!(
        (0..4).map(|x| pixel(ctx.kernel, x, 0)).collect::<Vec<_>>(),
        vec![1, 1, 2, 3]
    );
    assert_eq!(ctx.kernel.gdi.regions[&region], vec![[0, 0, 1, 3]]);
    assert_eq!(
        ctx.cpu.read_mem(0x2000, 16).unwrap(),
        [0i32, 0, 1, 3]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>()
    );
    let before = ctx.kernel.framebuffer.pixels.clone();
    let counter = ctx.kernel.framebuffer.frame_counter;
    assert_eq!(
        call(
            &mut ctx,
            window_graphics::scroll_window_ex,
            &[FAKE_HWND, 0, 0, 0, 0, region, 0x2000, 0]
        ),
        1
    );
    assert_eq!(ctx.kernel.framebuffer.pixels, before);
    assert_eq!(ctx.kernel.framebuffer.frame_counter, counter);
    assert!(ctx.kernel.gdi.regions[&region].is_empty());
}
#[test]
fn scroll_window_clip_can_receive_pixels_from_outside_clip_and_moves_own_children() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    kernel.framebuffer = Framebuffer::new(4, 4);
    for (i, p) in kernel.framebuffer.pixels.chunks_exact_mut(2).enumerate() {
        p.copy_from_slice(&(i as u16 + 1).to_le_bytes());
    }
    let child = kernel.controls.create(
        FAKE_HWND,
        ControlClass::Button,
        1,
        String::new(),
        0x50000000,
        1,
        1,
        1,
        1,
    );
    let other = kernel.controls.create(
        0xdead0009,
        ControlClass::Button,
        2,
        String::new(),
        0x50000000,
        1,
        1,
        1,
        1,
    );
    kernel.window_rects.insert(child, (1, 1, 1, 1));
    kernel.window_parents.insert(child, FAKE_HWND);
    cpu.write_mem(
        0x2000,
        &[1i32, 0, 4, 4]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(
        call(
            &mut ctx,
            window_graphics::scroll_window_ex,
            &[FAKE_HWND, 1, 0, 0, 0x2000, 0, 0, 1]
        ),
        1
    );
    assert_eq!(pixel(ctx.kernel, 1, 0), 1);
    assert_eq!(ctx.kernel.controls.get(child).unwrap().x, 2);
    assert_eq!(ctx.kernel.controls.get(other).unwrap().x, 1);
    assert_eq!(ctx.kernel.window_rects[&child], (2, 1, 1, 1));
    assert!(ctx
        .kernel
        .posted_messages
        .iter()
        .any(|m| m.0 == child && m.1 == 3));
}
#[test]
fn scroll_erase_is_deferred_and_does_not_erase_retained_pixels() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    kernel.framebuffer = Framebuffer::new(4, 3);
    kernel.framebuffer.fill(0xf800);
    kernel.window_background = Some(0xffffff);
    kernel.window_background_erase_pending = false;
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(
        call(
            &mut ctx,
            window_graphics::scroll_window_ex,
            &[FAKE_HWND, 1, 0, 0, 0, 0, 0, 6]
        ),
        2
    );
    assert_eq!(pixel(ctx.kernel, 0, 0), 0xf800);
    assert_eq!(
        call(&mut ctx, begin_paint, &[FAKE_HWND, 0x2000]),
        GDI_SCREEN_DC
    );
    assert_eq!(pixel(ctx.kernel, 0, 0), 0xffff);
    assert_eq!(pixel(ctx.kernel, 1, 0), 0xf800);
    assert!(ctx.kernel.gdi.scroll_erases.is_empty());
}
#[test]
fn class_cursor_value_is_shared_by_windows_of_the_same_class() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    kernel.window_classes.insert(0xdead1000, "MapClass".into());
    kernel.window_classes.insert(0xdead1001, "mapclass".into());
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(
        call(
            &mut ctx,
            window_graphics::set_class_long,
            &[0xdead1000, (-12i32) as u32, FAKE_ICON]
        ),
        0
    );
    assert_eq!(
        call(
            &mut ctx,
            window_graphics::get_class_long,
            &[0xdead1001, (-12i32) as u32]
        ),
        FAKE_ICON
    );
    assert_eq!(
        call(
            &mut ctx,
            window_graphics::set_class_long,
            &[0xdead1001, (-12i32) as u32, 0]
        ),
        FAKE_ICON
    );
}
#[test]
fn file_times_write_real_modification_time_and_compare_unsigned_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resource.bmp");
    std::fs::write(&path, b"fixture").unwrap();
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    kernel.vfs.mount("\\", dir.path());
    let handle = kernel
        .vfs
        .open("\\resource.bmp", pocket_kernel::vfs::Access::Read, false)
        .unwrap();
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    ctx.cpu.write_mem(0x2000, &[0xcc; 24]).unwrap();
    assert_eq!(
        call(
            &mut ctx,
            window_graphics::get_file_time,
            &[handle, 0, 0, 0x2010]
        ),
        1
    );
    assert_eq!(ctx.cpu.read_mem(0x2000, 16).unwrap(), vec![0xcc; 16]);
    let time = u64::from_le_bytes(ctx.cpu.read_mem(0x2010, 8).unwrap().try_into().unwrap());
    assert!(time > 116444736000000000);
    ctx.cpu.write_mem(0x2000, &u64::MAX.to_le_bytes()).unwrap();
    ctx.cpu.write_mem(0x2008, &1u64.to_le_bytes()).unwrap();
    assert_eq!(
        call(
            &mut ctx,
            window_graphics::compare_file_time,
            &[0x2000, 0x2008]
        ),
        1
    );
    assert_eq!(
        call(
            &mut ctx,
            window_graphics::compare_file_time,
            &[0x2008, 0x2000]
        ),
        u32::MAX
    );
    assert_eq!(
        call(
            &mut ctx,
            window_graphics::compare_file_time,
            &[0x2008, 0x2008]
        ),
        0
    );
    let before = ctx.cpu.read_mem(0x2010, 8).unwrap();
    assert_eq!(
        call(&mut ctx, window_graphics::get_file_time, &[0, 0, 0, 0x2010]),
        0
    );
    assert_eq!(ctx.cpu.read_mem(0x2010, 8).unwrap(), before);
}

#[test]
fn rgb565_bitfield_icon_reads_external_masks_before_the_xor_and_and_planes() {
    let mut raw = vec![0u8; 60];
    raw[..4].copy_from_slice(&40u32.to_le_bytes());
    raw[4..8].copy_from_slice(&1i32.to_le_bytes());
    raw[8..12].copy_from_slice(&2i32.to_le_bytes());
    raw[12..14].copy_from_slice(&1u16.to_le_bytes());
    raw[14..16].copy_from_slice(&16u16.to_le_bytes());
    raw[16..20].copy_from_slice(&3u32.to_le_bytes());
    for (i, mask) in [0xf800u32, 0x07e0, 0x001f].iter().enumerate() {
        raw[40 + i * 4..44 + i * 4].copy_from_slice(&mask.to_le_bytes());
    }
    raw[52..54].copy_from_slice(&0xf800u16.to_le_bytes());
    let icon = images::decode_icon(&raw).unwrap();
    assert_eq!(icon.pixels, vec![0xf800]);
    assert_eq!(icon.alpha, vec![255]);
    raw[56] = 0x80;
    assert_eq!(images::decode_icon(&raw).unwrap().alpha, vec![0]);
}

#[test]
fn icon_list_messages_attach_images_and_grid_navigation_matches_hit_testing() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    kernel.framebuffer = Framebuffer::new(160, 100);
    let hwnd = kernel.controls.create(
        FAKE_HWND,
        ControlClass::ListView,
        42,
        String::new(),
        0x50006004,
        0,
        0,
        150,
        90,
    );
    let mut image = Image::new(32, 32).unwrap();
    image.pixels.fill(0xf800);
    let icon = kernel.gdi.register_icon(image);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let list = call(&mut ctx, images::create, &[32, 32, 1, 1, 1]);
    call(&mut ctx, images::replace_icon, &[list, u32::MAX, icon]);
    // Numeric WinCE message values from commctrl.h: SET=FIRST+3, GET=FIRST+2.
    assert_eq!(
        list_view_message(&mut ctx, hwnd, 0x1003, 0, list).unwrap(),
        0
    );
    assert_eq!(
        list_view_message(&mut ctx, hwnd, 0x1002, 0, 0).unwrap(),
        list
    );
    assert_eq!(
        list_view_message(&mut ctx, hwnd, 0x1001, 0, 0x00efe3d6).unwrap(),
        1
    );
    {
        let c = ctx.kernel.controls.get_mut(hwnd).unwrap();
        c.list_items = vec![("One".into(), 1), ("Two".into(), 2), ("Three".into(), 3)];
        c.list_images = vec![0, 0, 0];
        c.list_selected = Some(0);
    }
    ctx.kernel.composite_controls();
    assert_eq!(pixel(ctx.kernel, 22, 4), 0xf800);
    assert_eq!(pixel(ctx.kernel, 97, 4), 0xf800);
    assert_eq!(pixel(ctx.kernel, 1, 1), colorref_to_rgb565(0x00efe3d6));
    ctx.kernel.controls.focus = hwnd;
    ctx.kernel.controls.key_down(0x27);
    assert_eq!(
        ctx.kernel.controls.get(hwnd).unwrap().list_selected,
        Some(1)
    );
    ctx.kernel.controls.key_down(0x25);
    assert_eq!(
        ctx.kernel.controls.get(hwnd).unwrap().list_selected,
        Some(0)
    );
    ctx.kernel.controls.pointer_down(100, 10);
    assert_eq!(
        ctx.kernel.controls.get(hwnd).unwrap().list_selected,
        Some(1)
    );
    let approx = list_view_message(&mut ctx, hwnd, 0x1040, u32::MAX, u32::MAX).unwrap();
    assert_eq!((approx & 0xffff, approx >> 16), (150, 150));
    ctx.cpu.write_mem(0x2300, &100i32.to_le_bytes()).unwrap();
    ctx.cpu.write_mem(0x2304, &10i32.to_le_bytes()).unwrap();
    assert_eq!(
        list_view_message(&mut ctx, hwnd, 0x1012, 0, 0x2300).unwrap(),
        1
    );
    assert_eq!(ctx.cpu.read_u32_le(0x230c).unwrap(), 1);
    assert_eq!(list_view_message(&mut ctx, hwnd, 0x1013, 2, 0).unwrap(), 1);
    // The same explicit position is returned to the guest and used by drawing/hit testing.
    assert_eq!(
        list_view_message(&mut ctx, hwnd, 0x100f, 1, (20 << 16) | 40).unwrap(),
        1
    );
    assert_eq!(
        list_view_message(&mut ctx, hwnd, 0x1010, 1, 0x2200).unwrap(),
        1
    );
    assert_eq!(ctx.cpu.read_u32_le(0x2200).unwrap(), 40);
    assert_eq!(ctx.cpu.read_u32_le(0x2204).unwrap(), 20);
}

#[test]
fn owner_draw_button_notifies_its_parent_with_a_real_child_dc_and_restores_the_call() {
    let mut cpu = scanf_cpu();
    cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
        .unwrap();
    let mut kernel = fresh_kernel();
    kernel.framebuffer = Framebuffer::new(80, 50);
    let hwnd = kernel.controls.create(
        FAKE_HWND,
        ControlClass::Button,
        77,
        String::new(),
        0x5000000b,
        4,
        6,
        24,
        20,
    );
    kernel.window_procs.insert(FAKE_HWND, 0x2000);
    kernel.window_parents.insert(hwnd, FAKE_HWND);
    kernel.window_rects.insert(hwnd, (4, 6, 24, 20));
    let thunk = thunk_at(0x70000100);
    cpu.write_reg(ArmReg::R0, hwnd).unwrap();
    cpu.write_reg(ArmReg::R1, WM_PAINT).unwrap();
    cpu.write_reg(ArmReg::R2, 0).unwrap();
    cpu.write_reg(ArmReg::R3, 0).unwrap();
    cpu.write_reg(ArmReg::Lr, 0x4560).unwrap();
    let sp = cpu.read_reg(ArmReg::Sp).unwrap();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert!(matches!(
        def_window_proc_w(&mut ctx).unwrap(),
        DispatchOutcome::JumpTo(0x2000)
    ));
    assert_eq!(ctx.cpu.read_reg(ArmReg::R0).unwrap(), FAKE_HWND);
    assert_eq!(ctx.cpu.read_reg(ArmReg::R1).unwrap(), 0x2b);
    assert_eq!(ctx.cpu.read_reg(ArmReg::R2).unwrap(), 77);
    let ptr = ctx.cpu.read_reg(ArmReg::R3).unwrap();
    let fields: Vec<_> = ctx
        .cpu
        .read_mem(ptr, 48)
        .unwrap()
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    assert_eq!(&fields[..6], &[4, 77, 0, 1, 0, hwnd]);
    assert_eq!(&fields[7..11], &[0, 0, 24, 20]);
    let dc = fields[6];
    // Exercise the same drawing path as a guest GDI callback, including coverage.
    surface_for_dc(ctx.kernel, dc)
        .unwrap()
        .put_pixel(0, 0, 0x07e0);
    ctx.cpu.write_reg(ArmReg::R0, 1).unwrap();
    ctx.cpu.write_reg(ArmReg::Lr, 0x8888).unwrap();
    assert!(matches!(
        def_window_proc_w(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(0)
    ));
    assert_eq!(ctx.cpu.read_reg(ArmReg::Sp).unwrap(), sp);
    assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), 0x4560);
    assert!(ctx.kernel.gdi.owner_draw_frames.is_empty());
    assert!(ctx.kernel.heap.msize(ptr).is_none());
    ctx.kernel.composite_controls();
    assert_eq!(pixel(ctx.kernel, 4, 6), 0x07e0);
}

#[test]
fn nested_popup_keyboard_ignores_its_owners_focus_and_keeps_native_child_focus() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let old = 0xdead1000;
    let popup = 0xdead1001;
    let map = 0xdead1002;
    kernel.window_procs.insert(old, 0x2100);
    kernel.window_procs.insert(popup, 0x2200);
    kernel.window_procs.insert(map, 0x2300);
    kernel.window_styles.insert(old, (0x90000000, 0));
    kernel.window_styles.insert(popup, (0x90000000, 0));
    kernel.window_styles.insert(map, (0x50000000, 0));
    kernel.window_parents.insert(popup, old); // popup owner is not a child-parent chain
    kernel.window_parents.insert(map, popup);
    let list = kernel.controls.create(
        old,
        ControlClass::ListView,
        1,
        String::new(),
        0x50000001,
        0,
        0,
        100,
        80,
    );
    kernel.window_procs.insert(list, 0x2400);
    kernel.controls.focus = list;
    kernel.active_window = popup;
    assert_eq!(input_window(&kernel), popup);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert!(controls_take_input(&mut ctx, InputEvent::KeyDown { vk: 0x27 }).is_none());
    call(&mut ctx, set_focus, &[map]);
    assert_eq!(ctx.kernel.controls.focus, map);
    assert_eq!(input_window(ctx.kernel), map);
    ctx.kernel.window_visibility.insert(map, false);
    assert_eq!(input_window(ctx.kernel), popup);
}

#[test]
fn dialog_init_without_tab_stops_moves_focus_out_of_the_owner_dialog() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let popup = 0xdead1001;
    kernel.window_procs.insert(popup, 0x2400);
    kernel.window_styles.insert(popup, (0x90000000, 0));
    kernel.window_visibility.insert(popup, true);
    kernel.controls.focus = 0xdead0e10;
    kernel.dialog_frame_hwnd = popup;
    kernel.dialog_frame = Some(GuestCallFrame {
        args: [0, 0, 0, 0],
        sp: cpu.read_reg(ArmReg::Sp).unwrap(),
        lr: 0x4444,
    });
    cpu.write_reg(ArmReg::R0, 1).unwrap();
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert!(
        matches!(create_dialog_indirect_param_w(&mut ctx).unwrap(),DispatchOutcome::ReturnedR0(v) if v == popup)
    );
    assert_eq!(ctx.kernel.controls.focus, popup);
    assert_eq!(input_window(ctx.kernel), popup);
}

#[test]
fn visible_native_child_gets_an_initial_paint_after_its_size_notification() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let hwnd = 0xdead1008;
    kernel.window_rects.insert(hwnd, (10, 10, 298, 183));
    kernel.window_styles.insert(hwnd, (0x50000000, 0));
    kernel.window_procs.insert(hwnd, 0x2300);
    kernel.window_visibility.insert(hwnd, true);
    kernel.window_parents.insert(hwnd, FAKE_HWND);
    kernel.create_stage = CreateStage::Create;
    kernel.create_frame_hwnd = hwnd;
    kernel.create_frame = Some(pocket_kernel::WindowCallbackFrame {
        frame: GuestCallFrame {
            args: [0, 0, 0, 0],
            sp: cpu.read_reg(ArmReg::Sp).unwrap(),
            lr: 0x4444,
        },
        thread: kernel.current_thread,
    });
    cpu.write_reg(ArmReg::R0, 0).unwrap();
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(
        create_window_ex_w(&mut ctx).unwrap(),
        DispatchOutcome::JumpTo(0x2300)
    );
    assert_eq!(ctx.cpu.read_reg(ArmReg::R1).unwrap(), WM_SIZE);
    assert_eq!(ctx.cpu.read_reg(ArmReg::R3).unwrap(), 298 | (183 << 16));
    assert!(!ctx
        .kernel
        .posted_messages
        .iter()
        .any(|m| m.0 == hwnd && m.1 == WM_PAINT));
    ctx.cpu.write_reg(ArmReg::R0, 0).unwrap();
    assert_eq!(
        create_window_ex_w(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(hwnd)
    );
    let messages: Vec<_> = ctx
        .kernel
        .posted_messages
        .iter()
        .filter(|m| m.0 == hwnd)
        .map(|m| m.1)
        .collect();
    assert_eq!(messages, vec![WM_PAINT]);
    ctx.kernel.posted_messages.clear();
    call(&mut ctx, move_window, &[hwnd, 12, 14, 100, 80, 1]);
    assert!(ctx
        .kernel
        .posted_messages
        .iter()
        .any(|m| m.0 == hwnd && m.1 == WM_PAINT));
}

#[test]
fn dialog_init_preserves_explicit_focus_on_its_non_tabstop_icon_menu() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let popup = 0xdead1001;
    kernel.window_procs.insert(popup, 0x2400);
    kernel.window_styles.insert(popup, (0x90000000, 0));
    kernel.window_visibility.insert(popup, true);
    let list = kernel.controls.create(
        popup,
        ControlClass::ListView,
        77,
        String::new(),
        0x50006004,
        0,
        0,
        150,
        80,
    );
    kernel.controls.focus = list; // SetFocus performed by the guest WM_INITDIALOG handler.
    kernel.dialog_frame_hwnd = popup;
    kernel.dialog_frame = Some(GuestCallFrame {
        args: [0, 0, 0, 0],
        sp: cpu.read_reg(ArmReg::Sp).unwrap(),
        lr: 0x4444,
    });
    cpu.write_reg(ArmReg::R0, 1).unwrap();
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    create_dialog_indirect_param_w(&mut ctx).unwrap();
    assert_eq!(ctx.kernel.controls.focus, list);
}

#[test]
fn keyboard_messages_keep_the_focused_builtin_child_as_their_target() {
    let mut kernel = fresh_kernel();
    let dialog = 0xdead1001;
    kernel.window_procs.insert(dialog, 0x2400);
    kernel.window_styles.insert(dialog, (0x50000000, 0));
    kernel.window_parents.insert(dialog, FAKE_HWND);
    kernel.window_visibility.insert(dialog, true);
    kernel.active_window = FAKE_HWND;
    let list = kernel.controls.create(
        dialog,
        ControlClass::ListView,
        77,
        String::new(),
        0x50000000,
        0,
        0,
        200,
        80,
    );
    kernel.controls.focus = list;
    assert!(!kernel.window_procs.contains_key(&list));
    assert_eq!(input_window(&kernel), list);
    kernel.window_visibility.insert(dialog, false);
    assert_eq!(input_window(&kernel), FAKE_HWND);
}

#[test]
fn showing_a_parent_invalidates_visible_native_children_but_not_owned_popups() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let parent = 0xdead1001;
    let child = 0xdead1002;
    let popup = 0xdead1003;
    for hwnd in [parent, child, popup] {
        kernel.window_procs.insert(hwnd, 0x2400);
        kernel.window_visibility.insert(hwnd, hwnd != parent);
    }
    kernel.window_styles.insert(parent, (0x80000000, 0));
    kernel.window_styles.insert(child, (0x50000000, 0));
    kernel.window_styles.insert(popup, (0x90000000, 0));
    kernel.window_parents.insert(child, parent);
    kernel.window_parents.insert(popup, parent);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    call(&mut ctx, show_window, &[parent, 5]);
    let paints: Vec<_> = ctx
        .kernel
        .posted_messages
        .iter()
        .filter(|m| m.1 == WM_PAINT)
        .map(|m| m.0)
        .collect();
    assert!(paints.contains(&parent));
    assert!(paints.contains(&child));
    assert!(!paints.contains(&popup));
}

#[test]
fn showing_an_existing_dialog_brings_its_pixels_above_newer_dialogs() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let older = 0xdead1001;
    let newer = 0xdead1002;
    for (hwnd, text) in [(older, "OLD"), (newer, "NEW")] {
        kernel.window_procs.insert(hwnd, 0x2400);
        kernel.window_styles.insert(hwnd, (0x90000000, 0));
        kernel.window_visibility.insert(hwnd, true);
        kernel
            .controls
            .add_panel(pocket_kernel::controls::DialogPanel {
                hwnd,
                x: 0,
                y: 0,
                w: 100,
                h: 80,
                visible: true,
                border: false,
                caption: None,
                ok_pressed: false,
            });
        let h = kernel.controls.create(
            hwnd,
            ControlClass::Edit,
            1,
            text.into(),
            0x50000000,
            0,
            0,
            60,
            20,
        );
        kernel.controls.get_mut(h).unwrap().style |= 0x00000800;
    }
    kernel.composite_controls();
    let newer_pixels = kernel.framebuffer.snapshot_ppm();
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    call(&mut ctx, show_window, &[older, 5]);
    ctx.kernel.composite_controls();
    let older_pixels = ctx.kernel.framebuffer.snapshot_ppm();
    assert_ne!(newer_pixels, older_pixels);
    call(&mut ctx, show_window, &[newer, 8]); // SW_SHOWNA must not raise it.
    ctx.kernel.composite_controls();
    assert_eq!(older_pixels, ctx.kernel.framebuffer.snapshot_ppm());
    call(&mut ctx, set_active_window, &[newer]);
    ctx.kernel.composite_controls();
    assert_eq!(newer_pixels, ctx.kernel.framebuffer.snapshot_ppm());
}

#[test]
fn transparent_image_preserves_keyed_pixels_and_scales_bitmap_or_dc_sources() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let bitmap = kernel.gdi.create_compatible_bitmap(2, 1);
    kernel
        .gdi
        .bitmap_mut(bitmap)
        .unwrap()
        .pixels
        .copy_from_slice(&[0x1f, 0xf8, 0, 0xf8]); // magenta, red
    let dc = kernel.gdi.create_memory_dc();
    kernel.gdi.select_into(dc, bitmap);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    for src in [bitmap, dc] {
        ctx.kernel.framebuffer.pixels.fill(0);
        if let Some(mut surface) = surface_for_dc(ctx.kernel, GDI_SCREEN_DC) {
            surface.fill_rect(0, 0, 8, 4, 0x07e0);
        }
        assert_eq!(
            call(
                &mut ctx,
                transparent_image,
                &[GDI_SCREEN_DC, 1, 1, 4, 2, src, 0, 0, 2, 1, 0xff00ff]
            ),
            1
        );
        assert_eq!(pixel(ctx.kernel, 1, 1), 0x07e0);
        assert_eq!(pixel(ctx.kernel, 2, 2), 0x07e0);
        assert_eq!(pixel(ctx.kernel, 3, 1), 0xf800);
        assert_eq!(pixel(ctx.kernel, 4, 2), 0xf800);
        assert_eq!(pixel(ctx.kernel, 5, 2), 0x07e0);
    }
    assert_eq!(
        call(
            &mut ctx,
            transparent_image,
            &[GDI_SCREEN_DC, 0, 0, 2, 1, dc, 1, 0, 1, 1, 0xff00ff]
        ),
        1
    );
    assert_eq!(pixel(ctx.kernel, 0, 0), 0xf800);
    assert_eq!(pixel(ctx.kernel, 1, 0), 0xf800);
}

#[test]
fn window_paint_keyed_corners_reveal_current_map_but_painted_black_stays_opaque() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let child = kernel.controls.create(
        FAKE_HWND,
        ControlClass::Static,
        1097,
        String::new(),
        0x50000000,
        3,
        4,
        2,
        1,
    );
    kernel.window_rects.insert(child, (3, 4, 2, 1));
    kernel.window_parents.insert(child, FAKE_HWND);
    let source = kernel.gdi.create_compatible_bitmap(2, 1);
    kernel
        .gdi
        .bitmap_mut(source)
        .unwrap()
        .pixels
        .copy_from_slice(&[0x1f, 0xf8, 0, 0]);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let dc = window_dc(&mut ctx, child);
    assert_eq!(
        call(
            &mut ctx,
            transparent_image,
            &[dc, 0, 0, 2, 1, source, 0, 0, 2, 1, 0xff00ff]
        ),
        1
    );
    let target = ctx.kernel.gdi.dc(dc).unwrap().selected_bitmap.unwrap();
    assert_eq!(
        ctx.kernel.gdi.bitmap(target).unwrap().painted.as_deref(),
        Some(&[0, 1][..])
    );
    for map in [0x07e0u16, 0xf800] {
        ctx.kernel
            .framebuffer
            .pixels
            .chunks_exact_mut(2)
            .for_each(|p| p.copy_from_slice(&map.to_le_bytes()));
        ctx.kernel.composite_controls();
        assert_eq!(pixel(ctx.kernel, 3, 4), map);
        assert_eq!(pixel(ctx.kernel, 4, 4), 0);
    }
    surface_for_dc(ctx.kernel, dc)
        .unwrap()
        .fill_rect(0, 0, 2, 1, 0);
    assert_eq!(
        call(
            &mut ctx,
            transparent_image,
            &[dc, 0, 0, 2, 1, source, 0, 0, 2, 1, 0xff00ff]
        ),
        1
    );
    ctx.kernel.framebuffer.pixels.fill(255);
    ctx.kernel.composite_controls();
    assert_eq!(pixel(ctx.kernel, 3, 4), 0);
}

#[test]
fn static_bitmap_uses_image_size_instead_of_clipping_to_dialog_units() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    kernel.framebuffer = Framebuffer::new(320, 240);
    let child = kernel.controls.create(
        FAKE_HWND,
        ControlClass::Static,
        1236,
        String::new(),
        0x5000000e,
        0,
        0,
        280,
        292,
    );
    let image = kernel.gdi.create_compatible_bitmap(320, 240);
    kernel
        .gdi
        .bitmap_mut(image)
        .unwrap()
        .pixels
        .chunks_exact_mut(2)
        .for_each(|p| p.copy_from_slice(&0x07e0u16.to_le_bytes()));
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(
        control_message(&mut ctx, child, 0x172, 0, image).unwrap(),
        0
    );
    let control = ctx.kernel.controls.get(child).unwrap();
    assert_eq!((control.w, control.h), (320, 240));
    ctx.kernel.composite_controls();
    assert_eq!(pixel(ctx.kernel, 319, 239), 0x07e0);
    assert_eq!(
        control_message(&mut ctx, child, 0x172, 0, 0).unwrap(),
        image
    );
}

#[test]
fn static_bitmap_center_and_realsize_styles_keep_their_distinct_geometry() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let centered = kernel.controls.create(
        FAKE_HWND,
        ControlClass::Static,
        1,
        String::new(),
        0x5000020e,
        30,
        20,
        40,
        20,
    );
    let fixed = kernel.controls.create(
        FAKE_HWND,
        ControlClass::Static,
        2,
        String::new(),
        0x5000004e,
        0,
        0,
        8,
        4,
    );
    let large = kernel.gdi.create_compatible_bitmap(80, 40);
    let small = kernel.gdi.create_compatible_bitmap(2, 1);
    kernel
        .gdi
        .bitmap_mut(small)
        .unwrap()
        .pixels
        .copy_from_slice(&[0, 0xf8, 0xe0, 7]);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    control_message(&mut ctx, centered, 0x172, 0, large).unwrap();
    let c = ctx.kernel.controls.get(centered).unwrap();
    assert_eq!((c.x, c.y, c.w, c.h), (10, 10, 80, 40));
    control_message(&mut ctx, fixed, 0x172, 0, small).unwrap();
    let c = ctx.kernel.controls.get(fixed).unwrap();
    assert_eq!((c.x, c.y, c.w, c.h), (0, 0, 8, 4));
    ctx.kernel.composite_controls();
    assert_eq!(pixel(ctx.kernel, 3, 3), 0xf800);
    assert_eq!(pixel(ctx.kernel, 7, 3), 0x07e0);
}

#[test]
fn ellipse_is_curved_and_null_brush_preserves_the_interior() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let background = kernel.gdi.create_solid_brush(0xff00ff);
    let pen = kernel.gdi.create_pen(0x0000ff, 2);
    kernel.gdi.select_into(GDI_SCREEN_DC, background);
    kernel.gdi.select_into(GDI_SCREEN_DC, pen);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    call(&mut ctx, ellipse, &[GDI_SCREEN_DC, 2, 2, 14, 14]);
    assert_eq!(pixel(ctx.kernel, 2, 2), 0);
    assert_eq!(pixel(ctx.kernel, 7, 7), 0xf81f);
    assert_eq!(pixel(ctx.kernel, 7, 2), 0xf800);
    ctx.kernel.gdi.select_into(GDI_SCREEN_DC, STOCK_NULL_BRUSH);
    if let Some(mut s) = surface_for_dc(ctx.kernel, GDI_SCREEN_DC) {
        s.fill_rect(0, 0, 20, 20, 0x07e0);
    }
    call(&mut ctx, ellipse, &[GDI_SCREEN_DC, 2, 2, 14, 14]);
    assert_eq!(pixel(ctx.kernel, 7, 7), 0x07e0);
    assert_eq!(pixel(ctx.kernel, 7, 2), 0xf800);
    call(&mut ctx, rectangle, &[GDI_SCREEN_DC, 2, 2, 14, 14]);
    assert_eq!(pixel(ctx.kernel, 7, 7), 0x07e0);
    let null = call(&mut ctx, create_pen, &[5, 10, 0xff00ff]);
    ctx.kernel.gdi.select_into(GDI_SCREEN_DC, null);
    let before = ctx.kernel.framebuffer.snapshot_ppm();
    call(&mut ctx, ellipse, &[GDI_SCREEN_DC, 0, 0, 20, 20]);
    call(&mut ctx, rectangle, &[GDI_SCREEN_DC, 0, 0, 20, 20]);
    assert_eq!(before, ctx.kernel.framebuffer.snapshot_ppm());
}

#[test]
fn filtered_render_probe_does_not_turn_paint_into_a_mouse_click() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    kernel.pending_startup.clear();
    kernel.pending_create = None;
    kernel.pending_message = Some((FAKE_HWND, WM_PAINT, 0, 0));
    kernel.posted_messages.push_back((FAKE_HWND, 0x111, 77, 0));
    kernel.synthetic_paint_next_ms = 0;
    cpu.write_mem(0x2000, &[0xcc; 28]).unwrap();
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(
        call(&mut ctx, peek_message_w, &[0x2000, 0, 0x201, 0x201, 2]),
        0
    );
    assert_eq!(ctx.cpu.read_mem(0x2000, 28).unwrap(), vec![0xcc; 28]);
    assert_eq!(
        ctx.kernel.pending_message,
        Some((FAKE_HWND, WM_PAINT, 0, 0))
    );
    assert_eq!(ctx.kernel.posted_messages.len(), 1);
    assert_eq!(ctx.kernel.synthetic_paint_next_ms, 0);
}

#[test]
fn filtered_peek_searches_past_unrelated_messages_and_uses_remove_bit() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    kernel.pending_startup.clear();
    kernel.pending_create = None;
    kernel.pending_message = None;
    let child = 0xdead1001;
    let popup = 0xdead1002;
    kernel.window_styles.insert(child, (0x50000000, 0));
    kernel.window_styles.insert(popup, (0x90000000, 0));
    kernel.window_parents.insert(child, FAKE_HWND);
    kernel.window_parents.insert(popup, FAKE_HWND);
    kernel.posted_messages.extend([
        (FAKE_HWND, WM_PAINT, 0, 0),
        (popup, 0x100, 66, 1),
        (child, 0x100, 65, 1),
    ]);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    for mode in [2, 2, 3] {
        assert_eq!(
            call(
                &mut ctx,
                peek_message_w,
                &[0x2000, FAKE_HWND, 0x100, 0x102, mode]
            ),
            1
        );
        assert_eq!(ctx.cpu.read_u32_le(0x2000).unwrap(), child);
        assert_eq!(ctx.cpu.read_u32_le(0x2008).unwrap(), 65);
    }
    assert_eq!(ctx.kernel.posted_messages.len(), 2);
    assert_eq!(ctx.kernel.posted_messages[0].1, WM_PAINT);
    assert_eq!(ctx.kernel.posted_messages[1].0, popup);
    assert_eq!(
        call(
            &mut ctx,
            peek_message_w,
            &[0x2000, FAKE_HWND, 0x100, 0x102, 2]
        ),
        0
    );
}

#[test]
fn filtered_peek_observes_keyboard_without_consuming_it_during_a_mouse_probe() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    kernel.pending_startup.clear();
    kernel.pending_create = None;
    kernel.pending_message = None;
    kernel
        .pending_input
        .push_back(InputEvent::KeyDown { vk: 27 });
    kernel.pending_input.push_back(InputEvent::KeyUp { vk: 27 });
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(
        call(&mut ctx, peek_message_w, &[0x2000, 0, 0x201, 0x201, 2]),
        0
    );
    assert_eq!(ctx.kernel.pending_input.len(), 2);
    for _ in 0..2 {
        assert_eq!(
            call(&mut ctx, peek_message_w, &[0x2000, 0, 0x100, 0x102, 2]),
            1
        );
        assert_eq!(ctx.cpu.read_u32_le(0x2004).unwrap(), 0x100);
        assert_eq!(ctx.cpu.read_u32_le(0x2008).unwrap(), 27);
        assert!(!ctx.kernel.pressed_keys[27]);
    }
    assert_eq!(
        call(&mut ctx, peek_message_w, &[0x2000, 0, 0x100, 0x102, 3]),
        1
    );
    assert_eq!(ctx.kernel.pending_input.len(), 1);
    assert!(ctx.kernel.pressed_keys[27]);
    assert_eq!(
        call(&mut ctx, peek_message_w, &[0x2000, 0, 0x100, 0x102, 3]),
        1
    );
    assert_eq!(ctx.cpu.read_u32_le(0x2004).unwrap(), 0x101);
    assert!(!ctx.kernel.pressed_keys[27]);
}

#[test]
fn filtered_peek_preserves_unmatched_timers_and_always_reports_quit() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    kernel.pending_startup.clear();
    kernel.pending_create = None;
    kernel.pending_message = None;
    kernel.timers.insert((0, FAKE_HWND, 77), (100, 0, 0));
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(
        call(&mut ctx, peek_message_w, &[0x2000, 0, 0x201, 0x201, 2]),
        0
    );
    assert_eq!(ctx.kernel.timers[&(0, FAKE_HWND, 77)].1, 0);
    assert_eq!(
        call(&mut ctx, peek_message_w, &[0x2000, 0, 0x113, 0x113, 2]),
        1
    );
    assert_eq!(ctx.kernel.timers[&(0, FAKE_HWND, 77)].1, 0);
    assert_eq!(
        call(&mut ctx, peek_message_w, &[0x2000, 0, 0x113, 0x113, 3]),
        1
    );
    assert!(ctx.kernel.timers[&(0, FAKE_HWND, 77)].1 > 0);
    ctx.kernel.posted_messages.push_back((0, WM_QUIT, 9, 0));
    assert_eq!(
        call(
            &mut ctx,
            peek_message_w,
            &[0x2000, FAKE_HWND, 0x201, 0x201, 3]
        ),
        1
    );
    assert_eq!(ctx.cpu.read_u32_le(0x2004).unwrap(), WM_QUIT);
}

#[test]
fn filtered_worker_peek_keeps_other_thread_messages_queued() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let mut worker = GuestThread::new(
        0x2100, 0, 0x6000, 0x1000, 0xf000fe00, 0x2100, 0xdead7c00, [0; 17],
    );
    worker.messages.extend([(WM_PAINT, 0, 0), (0x101, 65, 0)]);
    kernel.threads.push(worker);
    kernel.current_thread = 1;
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    for remove in [2, 3] {
        assert!(matches!(
            message_queue::peek(&mut ctx, 0x2000, u32::MAX, 0x101, 0x101, remove).unwrap(),
            DispatchOutcome::ReturnedR0(1)
        ));
        assert_eq!(ctx.cpu.read_u32_le(0x2000).unwrap(), 0);
        assert_eq!(ctx.cpu.read_u32_le(0x2004).unwrap(), 0x101);
    }
    assert_eq!(ctx.kernel.threads[0].messages.len(), 1);
    assert_eq!(ctx.kernel.threads[0].messages[0].0, WM_PAINT);
}

#[test]
fn draw_text_measurement_wraps_without_touching_destination() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let dc = kernel.gdi.create_memory_dc();
    let bm = kernel.gdi.create_compatible_bitmap(60, 32);
    kernel.gdi.select_into(dc, bm);
    kernel.gdi.bitmap_mut(bm).unwrap().pixels.fill(0x5a);
    cpu.write_mem(
        0x2000,
        &wide_to_bytes(&"Go west on Madison".encode_utf16().collect::<Vec<_>>()),
    )
    .unwrap();
    cpu.write_mem(
        0x2100,
        &[3i32, 2, 63, 32]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(
        call(&mut ctx, draw_text_w, &[dc, 0x2000, 18, 0x2100, 0x410]),
        16
    );
    let raw = ctx.cpu.read_mem(0x2100, 16).unwrap();
    let r: Vec<_> = raw
        .chunks_exact(4)
        .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    assert_eq!(r, vec![3, 2, 63, 18]);
    assert!(ctx
        .kernel
        .gdi
        .bitmap(bm)
        .unwrap()
        .pixels
        .iter()
        .all(|b| *b == 0x5a));
    assert!(!ctx.kernel.gdi.bitmap(bm).unwrap().host_dirty);
}

#[test]
fn draw_text_clips_glyphs_and_background_to_the_supplied_rectangle() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let dc = kernel.gdi.create_memory_dc();
    let bm = kernel.gdi.create_compatible_bitmap(24, 16);
    kernel.gdi.select_into(dc, bm);
    kernel.gdi.bitmap_mut(bm).unwrap().pixels.fill(0x5a);
    let meta = kernel.gdi.dc_mut(dc).unwrap();
    meta.bk_transparent = false;
    meta.bk_color = 0;
    meta.text_color = 0xffffff;
    cpu.write_mem(0x2000, &wide_to_bytes(&[65, 65, 65]))
        .unwrap();
    cpu.write_mem(
        0x2100,
        &[4i32, 3, 11, 8]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(
        call(&mut ctx, draw_text_w, &[dc, 0x2000, 3, 0x2100, 0x20]),
        8
    );
    let pixels = &ctx.kernel.gdi.bitmap(bm).unwrap().pixels;
    for y in 0..16 {
        for x in 0..24 {
            let p = &pixels[(y * 24 + x) * 2..(y * 24 + x + 1) * 2];
            if !(4..11).contains(&x) || !(3..8).contains(&y) {
                assert_eq!(p, &[0x5a, 0x5a]);
            }
        }
    }
    assert!(pixels.chunks_exact(2).any(|p| p == [0xff, 0xff]));
}

#[test]
fn begin_paint_calls_child_erase_once_and_restores_guest_call() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let hwnd = 0xdead1001;
    kernel.window_rects.insert(hwnd, (10, 20, 80, 30));
    kernel.window_procs.insert(hwnd, 0x3100);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    ctx.cpu.write_reg(ArmReg::R0, hwnd).unwrap();
    ctx.cpu.write_reg(ArmReg::R1, 0x2000).unwrap();
    ctx.cpu.write_reg(ArmReg::Lr, 0x4320).unwrap();
    assert_eq!(
        begin_paint(&mut ctx).unwrap(),
        DispatchOutcome::JumpTo(0x3100)
    );
    assert_eq!(ctx.cpu.read_reg(ArmReg::R1).unwrap(), WM_ERASEBKGND);
    let dc = ctx.cpu.read_reg(ArmReg::R2).unwrap();
    assert_ne!(dc, GDI_SCREEN_DC);
    // Guest handled its own erase. BeginPaint resumes without another callback.
    ctx.cpu.write_reg(ArmReg::R0, 1).unwrap();
    assert_eq!(
        begin_paint(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(dc)
    );
    assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), 0x4320);
    assert_eq!(ctx.cpu.read_u32_le(0x2004).unwrap(), 0);
    assert_eq!(ctx.cpu.read_u32_le(0x2010).unwrap(), 80);
    assert!(ctx.kernel.gdi.paint_erase_frames.is_empty());
    assert!(!ctx.kernel.gdi.pending_window_erases.contains(&hwnd));
}

#[test]
fn default_child_erase_uses_its_class_brush_without_clearing_the_screen() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let hwnd = 0xdead1001;
    kernel.window_rects.insert(hwnd, (10, 20, 80, 30));
    kernel.window_classes.insert(hwnd, "Child".into());
    kernel
        .gdi
        .class_values
        .insert(("child".into(), -10), STOCK_BLACK_BRUSH);
    kernel.framebuffer.pixels.fill(0x5a);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let dc = window_dc(&mut ctx, hwnd);
    let bm = ctx.kernel.gdi.dc(dc).unwrap().selected_bitmap.unwrap();
    ctx.kernel.gdi.bitmap_mut(bm).unwrap().pixels.fill(0x7f);
    assert_eq!(
        call(&mut ctx, def_window_proc_w, &[hwnd, WM_ERASEBKGND, dc, 0]),
        1
    );
    assert!(ctx
        .kernel
        .gdi
        .bitmap(bm)
        .unwrap()
        .pixels
        .iter()
        .all(|p| *p == 0));
    assert!(ctx.kernel.framebuffer.pixels.iter().all(|p| *p == 0x5a));
    ctx.kernel.gdi.class_values.remove(&("child".into(), -10));
    assert_eq!(
        call(&mut ctx, def_window_proc_w, &[hwnd, WM_ERASEBKGND, dc, 0]),
        0
    );
}

#[test]
fn default_overlapped_geometry_reaches_create_size_and_window_rect() {
    let mut cpu = scanf_cpu();
    cpu.map_region(0x50000000, 0x10000, Prot::READ | Prot::WRITE)
        .unwrap();
    let mut kernel = fresh_kernel();
    kernel.framebuffer = Framebuffer::new(320, 240);
    kernel
        .window_class_procs
        .insert("Fathammer X-Forge".into(), 0x3100);
    cpu.write_mem(
        0x2000,
        &wide_to_bytes(&"Fathammer X-Forge\0".encode_utf16().collect::<Vec<_>>()),
    )
    .unwrap();
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let args = [
        0, 0x2000, 0, 0x10000000, 0x80000000, 0x80000000, 0x80000000, 0x80000000, 0, 0, 0, 0,
    ];
    for (i, v) in args.into_iter().enumerate() {
        if i < 4 {
            ctx.cpu
                .write_reg([ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3][i], v)
                .unwrap();
        } else {
            let sp = ctx.cpu.read_reg(ArmReg::Sp).unwrap();
            ctx.cpu
                .write_mem(sp + (i as u32 - 4) * 4, &v.to_le_bytes())
                .unwrap();
        }
    }
    assert_eq!(
        create_window_ex_w(&mut ctx).unwrap(),
        DispatchOutcome::JumpTo(0x3100)
    );
    let hwnd = ctx.cpu.read_reg(ArmReg::R0).unwrap();
    let cs = ctx.cpu.read_reg(ArmReg::R3).unwrap();
    assert_eq!(ctx.kernel.window_rects[&hwnd], (0, 0, 320, 240));
    assert_eq!(ctx.cpu.read_u32_le(cs + 16).unwrap(), 240);
    assert_eq!(ctx.cpu.read_u32_le(cs + 20).unwrap(), 320);
    assert_eq!(ctx.cpu.read_u32_le(cs + 24).unwrap(), 0);
    assert_eq!(ctx.cpu.read_u32_le(cs + 28).unwrap(), 0);
    ctx.cpu.write_reg(ArmReg::R0, 0).unwrap();
    assert_eq!(
        create_window_ex_w(&mut ctx).unwrap(),
        DispatchOutcome::JumpTo(0x3100)
    );
    assert_eq!(ctx.cpu.read_reg(ArmReg::R1).unwrap(), WM_SIZE);
    assert_eq!(ctx.cpu.read_reg(ArmReg::R3).unwrap(), 320 | (240 << 16));
    ctx.cpu.write_reg(ArmReg::R0, 0).unwrap();
    assert_eq!(
        create_window_ex_w(&mut ctx).unwrap(),
        DispatchOutcome::ReturnedR0(hwnd)
    );
    assert_eq!(call(&mut ctx, get_window_rect, &[hwnd, 0x2100]), 1);
    assert_eq!(ctx.cpu.read_u32_le(0x2108).unwrap(), 320);
    assert_eq!(ctx.cpu.read_u32_le(0x210c).unwrap(), 240);
    assert!(!ctx.kernel.pending_startup.iter().any(|m| m.0 == WM_SIZE));
}

#[test]
fn default_geometry_keeps_explicit_sizes_and_does_not_fullscreen_children() {
    assert_eq!(
        initial_window_rect(0, (10, 20, 80, 60), (320, 240)),
        (10, 20, 80, 60)
    );
    assert_eq!(
        initial_window_rect(0, (10, 20, i32::MIN, 999), (320, 240)),
        (10, 20, 310, 220)
    );
    for style in [0x40000000, 0x80000000] {
        assert_eq!(
            initial_window_rect(style, (i32::MIN, 99, i32::MIN, 999), (320, 240)),
            (0, 0, 0, 0)
        );
        assert_eq!(
            initial_window_rect(style, (-5, 3, 40, 50), (320, 240)),
            (-5, 3, 40, 50)
        );
    }
}

#[test]
fn activating_dialog_keeps_child_dialogs_above_parent_background() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let (parent, child, grandchild, owned, other) =
        (0xdead1001, 0xdead1002, 0xdead1003, 0xdead1004, 0xdead1005);
    for hwnd in [parent, child, grandchild, owned, other] {
        kernel.window_procs.insert(hwnd, 0x3100);
        kernel.window_visibility.insert(hwnd, true);
        kernel.window_styles.insert(
            hwnd,
            (
                if hwnd == child || hwnd == grandchild {
                    0x50000000
                } else {
                    0x90000000
                },
                0,
            ),
        );
        kernel
            .controls
            .add_panel(pocket_kernel::controls::DialogPanel {
                hwnd,
                x: 0,
                y: 0,
                w: 100,
                h: 80,
                visible: true,
                border: false,
                caption: None,
                ok_pressed: false,
            });
    }
    kernel.window_parents.insert(child, parent);
    kernel.window_parents.insert(grandchild, child);
    kernel.window_parents.insert(owned, parent);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    call(&mut ctx, set_active_window, &[parent]);
    assert_eq!(
        ctx.kernel.controls.panel_handles().collect::<Vec<_>>(),
        vec![owned, other, parent, child, grandchild]
    );
    call(&mut ctx, set_active_window, &[other]);
    assert_eq!(ctx.kernel.controls.panel_handles().last(), Some(other));
    // Raising the parent again brings the whole child subtree, not its owned popup.
    call(&mut ctx, show_window, &[parent, 1]);
    assert_eq!(
        ctx.kernel.controls.panel_handles().collect::<Vec<_>>(),
        vec![owned, other, parent, child, grandchild]
    );
    let mut layers = Vec::new();
    let mut surf = Surface::Screen(&mut ctx.kernel.framebuffer);
    ctx.kernel
        .controls
        .render_composited(&mut surf, |h, _| layers.push(h));
    assert_eq!(layers, vec![0, owned, other, parent, child, grandchild]);
}

#[test]
fn shell_keyboard_routes_text_through_guest_messages_and_done_through_keys() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let parent = 0xdead1001;
    let edit = kernel.controls.create(
        parent,
        ControlClass::Edit,
        1544,
        "WA".into(),
        0x50010000,
        0,
        0,
        100,
        20,
    );
    kernel.controls.focus = edit;
    kernel.active_window = 0xdead1999;
    cpu.map_region(0x10000, 0x1000, Prot::READ | Prot::WRITE)
        .unwrap();
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    ctx.cpu
        .write_mem(
            0x10000,
            &wide_to_bytes(&"GMSHELLCLASS\0".encode_utf16().collect::<Vec<_>>()),
        )
        .unwrap();
    assert_eq!(
        call(&mut ctx, find_window_w, &[0x10000, 0]),
        pocket_kernel::shell_keyboard::HWND
    );
    ctx.cpu
        .write_mem(
            0x2100,
            &[0u32, 10, 0x2200]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    ctx.cpu
        .write_mem(0x2200, &[0x90, 1, 1, 0, 0, 0, 0, 0, 0, 0])
        .unwrap();
    let shell = pocket_kernel::shell_keyboard::HWND;
    assert_eq!(call(&mut ctx, send_message_w, &[shell, 0x4a, 0, 0x2100]), 1);
    assert!(ctx.kernel.shell_keyboard.visible);
    assert_eq!(ctx.kernel.shell_keyboard.target, edit);
    call(&mut ctx, send_message_w, &[shell, 0x100, 0x27, 0]);
    call(&mut ctx, send_message_w, &[shell, 0x100, 0xd, 0]);
    assert_eq!(ctx.kernel.controls.get(edit).unwrap().text, "WA");
    assert_eq!(
        ctx.kernel.posted_messages.pop_front(),
        Some((edit, 0x102, b'B' as u32, 1))
    );
    call(&mut ctx, send_message_w, &[shell, 0x100, 0x11, 0]);
    assert_eq!(
        ctx.kernel.posted_messages.pop_front(),
        Some((edit, 0x102, 8, 1))
    );
    call(&mut ctx, send_message_w, &[shell, 0x100, 0x20, 0]);
    assert!(!ctx.kernel.shell_keyboard.visible);
    assert_eq!(
        ctx.kernel.posted_messages.pop_front(),
        Some((parent, 0x200, 0, 0))
    );
    assert!(ctx.kernel.posted_messages.is_empty());
}

#[test]
fn shell_keyboard_rejects_unknown_copydata_and_does_not_capture_unrelated_queries() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    ctx.cpu
        .write_mem(
            0x2000,
            &wide_to_bytes(&"SomeGame\0".encode_utf16().collect::<Vec<_>>()),
        )
        .unwrap();
    assert_eq!(call(&mut ctx, find_window_w, &[0x2000, 0]), 0);
    let shell = pocket_kernel::shell_keyboard::HWND;
    ctx.cpu
        .write_mem(
            0x2100,
            &[0u32, 3, 0x2200]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    ctx.cpu.write_mem(0x2200, &[0xff, 0xff, 1]).unwrap();
    assert_eq!(call(&mut ctx, send_message_w, &[shell, 0x4a, 0, 0x2100]), 0);
    assert_eq!(call(&mut ctx, send_message_w, &[shell, 0x100, 0xd, 0]), 0);
    assert!(!ctx.kernel.shell_keyboard.visible);
    assert!(ctx.kernel.posted_messages.is_empty());
}

#[test]
fn shell_keyboard_follows_the_current_page_instead_of_editing_hidden_state() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let old = kernel.controls.create(
        0xdead1001,
        ControlClass::Edit,
        1544,
        "WA".into(),
        0x50010000,
        0,
        0,
        100,
        20,
    );
    let current = kernel.controls.create(
        0xdead1002,
        ControlClass::Edit,
        1544,
        String::new(),
        0x50010000,
        0,
        0,
        100,
        20,
    );
    kernel.window_visibility.insert(0xdead1001, false);
    kernel.window_styles.insert(old, (0x50010000, 0));
    kernel.window_styles.insert(current, (0x50010000, 0));
    kernel.shell_keyboard.visible = true;
    kernel.shell_keyboard.target = old;
    kernel.controls.focus = current;
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let shell = pocket_kernel::shell_keyboard::HWND;
    call(&mut ctx, send_message_w, &[shell, 0x100, 0x41, 0]);
    assert_eq!(
        ctx.kernel.posted_messages.pop_front(),
        Some((current, 0x102, 0x41, 1))
    );
    assert_eq!(ctx.kernel.shell_keyboard.target, current);
    assert_eq!(ctx.kernel.controls.get(old).unwrap().text, "WA");
}

#[test]
fn shell_keyboard_can_open_before_focus_and_tab_between_address_fields() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let shell = pocket_kernel::shell_keyboard::HWND;
    ctx.cpu
        .write_mem(
            0x2100,
            &[0u32, 3, 0x2200]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    ctx.cpu.write_mem(0x2200, &[0x90, 0, 1]).unwrap();
    assert_eq!(call(&mut ctx, send_message_w, &[shell, 0x4a, 0, 0x2100]), 1);
    assert!(ctx.kernel.shell_keyboard.requested);
    assert!(!ctx.kernel.shell_keyboard.visible);
    assert_eq!(ctx.kernel.shell_keyboard.target, 0);
    let number = ctx.kernel.controls.create(
        0xdead1001,
        ControlClass::Edit,
        1,
        String::new(),
        0x50010000,
        0,
        0,
        40,
        20,
    );
    let street = ctx.kernel.controls.create(
        0xdead1001,
        ControlClass::Edit,
        2,
        String::new(),
        0x50010000,
        50,
        0,
        100,
        20,
    );
    ctx.kernel.controls.focus = number;
    crate::shell_keyboard::service(&mut ctx);
    assert!(ctx.kernel.shell_keyboard.visible);
    call(&mut ctx, send_message_w, &[shell, 0x100, 0x09, 0]);
    assert_eq!(ctx.kernel.controls.focus, street);
    call(&mut ctx, send_message_w, &[shell, 0x100, 0x37, 0]);
    assert_eq!(
        ctx.kernel.posted_messages.pop_front(),
        Some((street, 0x102, 0x37, 1))
    );
    assert!(ctx.kernel.controls.get(number).unwrap().text.is_empty());
}

#[test]
fn plain_edit_does_not_eat_parent_shortcuts_before_native_pretranslation() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let parent = 0xdead1001;
    kernel.window_procs.insert(parent, 0x3100);
    kernel.active_window = parent;
    let edit = kernel.controls.create(
        parent,
        ControlClass::Edit,
        1,
        String::new(),
        0x50010000,
        0,
        0,
        100,
        20,
    );
    kernel.controls.focus = edit;
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(
        controls_take_input(&mut ctx, InputEvent::KeyDown { vk: 0x20 }),
        None
    );
    assert!(ctx.kernel.controls.get(edit).unwrap().text.is_empty());
    // Only a character that survives application translation is inserted.
    ctx.cpu.write_reg(ArmReg::R0, edit).unwrap();
    ctx.cpu.write_reg(ArmReg::R1, 0x102).unwrap();
    ctx.cpu.write_reg(ArmReg::R2, 0x41).unwrap();
    ctx.cpu.write_reg(ArmReg::R3, 1).unwrap();
    assert!(matches!(
        def_window_proc_w(&mut ctx).unwrap(),
        DispatchOutcome::JumpTo(0x3100)
    ));
    assert_eq!(ctx.kernel.controls.get(edit).unwrap().text, "A");
}

#[test]
fn hidden_focused_result_list_keeps_the_visible_page_in_input_routing() {
    let mut kernel = fresh_kernel();
    let sheet = 0xdead1006;
    let page = 0xdead100b;
    kernel.active_window = sheet;
    kernel.window_procs.insert(sheet, 0x3100);
    kernel.window_procs.insert(page, 0x3200);
    kernel.window_styles.insert(sheet, (0x10000000, 0));
    kernel.window_styles.insert(page, (0x50000000, 0));
    kernel.window_parents.insert(page, sheet);
    let list = kernel.controls.create(
        page,
        ControlClass::ListView,
        1466,
        String::new(),
        0x50010000,
        0,
        0,
        100,
        20,
    );
    kernel.controls.focus = list;
    assert_eq!(input_window(&kernel), list);
    kernel.controls.get_mut(list).unwrap().visible = false;
    assert_eq!(input_window(&kernel), page);
    // A hidden old page must not receive keys after a page change.
    kernel.window_visibility.insert(page, false);
    assert_eq!(input_window(&kernel), sheet);
    // Ownership of a different popup is not an input-parent relationship.
    kernel.window_styles.insert(page, (0x90000000, 0));
    kernel.window_visibility.insert(page, true);
    assert_eq!(input_window(&kernel), sheet);
}

#[test]
fn modal_return_does_not_send_console_keys_to_the_previous_popup() {
    let mut kernel = fresh_kernel();
    let active = 0xdead1006;
    let old_popup = 0xdead1005;
    kernel.active_window = active;
    kernel.window_styles.insert(active, (0x10000000, 0));
    kernel.window_styles.insert(old_popup, (0x90000000, 0));
    kernel.window_parents.insert(old_popup, active);
    let stale = kernel.controls.create(
        old_popup,
        ControlClass::ListView,
        1,
        String::new(),
        0x50010000,
        0,
        0,
        100,
        20,
    );
    let hidden = kernel.controls.create(
        active,
        ControlClass::Edit,
        2,
        String::new(),
        0x40010000,
        0,
        0,
        100,
        20,
    );
    let target = kernel.controls.create(
        active,
        ControlClass::ListView,
        3,
        String::new(),
        0x50010000,
        0,
        0,
        100,
        20,
    );
    kernel.controls.focus = stale;
    restore_active_dialog_focus(&mut kernel);
    assert_eq!(kernel.controls.focus, target);
    // Explicit focus inside the current dialog remains application-owned,
    // even if the application hides that control on its last page.
    kernel.controls.focus = hidden;
    restore_active_dialog_focus(&mut kernel);
    assert_eq!(kernel.controls.focus, hidden);
}

#[test]
fn filtered_peek_repairs_foreign_focus_only_when_removing_a_key() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    kernel.pending_create = None;
    kernel.pending_message = None;
    kernel.pending_startup.clear();
    let active = 0xdead1006;
    kernel.active_window = active;
    kernel.window_styles.insert(active, (0x10000000, 0));
    let stale = kernel.controls.create(
        0xdead1005,
        ControlClass::Edit,
        1,
        String::new(),
        0x50010000,
        0,
        0,
        100,
        20,
    );
    let target = kernel.controls.create(
        active,
        ControlClass::Edit,
        2,
        String::new(),
        0x50010000,
        0,
        0,
        100,
        20,
    );
    kernel.controls.focus = stale;
    kernel
        .pending_input
        .push_back(InputEvent::KeyDown { vk: 0x20 });
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(
        message_queue::peek(&mut ctx, 0x2000, 0, 0x201, 0x201, 2).unwrap(),
        DispatchOutcome::ReturnedR0(0)
    );
    assert_eq!(ctx.kernel.controls.focus, stale);
    assert_eq!(
        message_queue::peek(&mut ctx, 0x2000, 0, 0x100, 0x100, 2).unwrap(),
        DispatchOutcome::ReturnedR0(1)
    );
    assert_eq!(ctx.cpu.read_u32_le(0x2000).unwrap(), target);
    assert_eq!(ctx.kernel.controls.focus, stale);
    assert_eq!(ctx.kernel.pending_input.len(), 1);
    assert_eq!(
        message_queue::peek(&mut ctx, 0x2000, 0, 0x100, 0x100, 3).unwrap(),
        DispatchOutcome::ReturnedR0(1)
    );
    assert_eq!(ctx.cpu.read_u32_le(0x2000).unwrap(), target);
    assert_eq!(ctx.kernel.controls.focus, target);
    assert!(ctx.kernel.pending_input.is_empty());
}

#[test]
fn explicit_non_tabstop_focus_is_restored_after_a_modal_return() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let active = 0xdead1006;
    kernel.active_window = active;
    kernel.window_styles.insert(active, (0x10000000, 0));
    let target = kernel.controls.create(
        active,
        ControlClass::ListView,
        1536,
        String::new(),
        0x50000000,
        0,
        0,
        100,
        20,
    );
    let stale = kernel.controls.create(
        0xdead1005,
        ControlClass::Edit,
        1,
        String::new(),
        0x50010000,
        0,
        0,
        100,
        20,
    );
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    call(&mut ctx, set_focus, &[target]);
    ctx.kernel.controls.focus = 0;
    restore_active_dialog_focus(ctx.kernel);
    assert_eq!(ctx.kernel.controls.focus, target);
    ctx.kernel.controls.focus = stale;
    assert_eq!(input_window(ctx.kernel), target);
    assert_eq!(ctx.kernel.controls.focus, stale);
    restore_active_dialog_focus(ctx.kernel);
    assert_eq!(ctx.kernel.controls.focus, target);
    ctx.kernel.controls.focus = stale;
    ctx.kernel.controls.get_mut(target).unwrap().visible = false;
    restore_active_dialog_focus(ctx.kernel);
    assert_eq!(ctx.kernel.controls.focus, stale);
    ctx.kernel.controls.get_mut(target).unwrap().visible = true;
    ctx.kernel.controls.get_mut(target).unwrap().enabled = false;
    restore_active_dialog_focus(ctx.kernel);
    assert_eq!(ctx.kernel.controls.focus, stale);
}

#[test]
fn choice_controls_keep_strings_selection_and_item_data() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let combo = kernel.controls.create(
        0xdead1001,
        ControlClass::ComboBox,
        1496,
        String::new(),
        0x50010103,
        0,
        0,
        100,
        80,
    );
    let list = kernel.controls.create(
        0xdead1001,
        ControlClass::ListBox,
        1497,
        String::new(),
        0x50010001,
        0,
        0,
        100,
        80,
    );
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    for (p, text) in [(0x2000, "Zulu"), (0x2100, "Ångström"), (0x2200, "Alpha")] {
        let mut chars: Vec<_> = text.encode_utf16().collect();
        chars.push(0);
        ctx.cpu.write_mem(p, &wide_to_bytes(&chars)).unwrap();
    }
    assert_eq!(
        call(&mut ctx, def_window_proc_w, &[combo, 0x143, 0, 0x2000]),
        0
    );
    assert_eq!(call(&mut ctx, def_window_proc_w, &[combo, 0x14e, 0, 0]), 0);
    assert_eq!(
        call(&mut ctx, def_window_proc_w, &[combo, 0x151, 0, 0x12345678]),
        0
    );
    assert_eq!(
        call(&mut ctx, def_window_proc_w, &[combo, 0x143, 0, 0x2200]),
        0
    );
    assert_eq!(call(&mut ctx, def_window_proc_w, &[combo, 0x147, 0, 0]), 1);
    assert_eq!(
        call(&mut ctx, def_window_proc_w, &[combo, 0x150, 1, 0]),
        0x12345678
    );
    assert_eq!(
        call(&mut ctx, def_window_proc_w, &[combo, 0x148, 1, 0x2300]),
        4
    );
    assert_eq!(
        ctx.cpu.read_mem(0x2300, 10).unwrap(),
        wide_to_bytes(&[90, 117, 108, 117, 0])
    );
    assert_eq!(call(&mut ctx, def_window_proc_w, &[combo, 0x144, 0, 0]), 1);
    assert_eq!(call(&mut ctx, def_window_proc_w, &[combo, 0x147, 0, 0]), 0);
    assert_eq!(
        call(&mut ctx, def_window_proc_w, &[combo, 0x150, 9, 0]),
        u32::MAX
    );
    assert_eq!(
        call(&mut ctx, def_window_proc_w, &[list, 0x180, 0, 0x2100]),
        0
    );
    assert_eq!(call(&mut ctx, def_window_proc_w, &[list, 0x18a, 0, 0]), 8);
    assert_eq!(call(&mut ctx, def_window_proc_w, &[list, 0x186, 0, 0]), 0);
    assert_eq!(call(&mut ctx, def_window_proc_w, &[list, 0x188, 0, 0]), 0);
    assert_eq!(call(&mut ctx, def_window_proc_w, &[list, 0x184, 0, 0]), 0);
    assert_eq!(
        call(&mut ctx, def_window_proc_w, &[list, 0x188, 0, 0]),
        u32::MAX
    );
}

#[test]
fn choice_keyboard_selection_uses_command_notifications_not_listview_notify() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let parent = 0xdead1001;
    let combo = kernel.controls.create(
        parent,
        ControlClass::ComboBox,
        1496,
        String::new(),
        0x50010003,
        0,
        0,
        100,
        80,
    );
    kernel.controls.get_mut(combo).unwrap().list_items =
        vec![("First".into(), 0), ("Second".into(), 0)];
    kernel.controls.focus = combo;
    let action = kernel.controls.key_down(0x28).unwrap();
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(
        control_action_message(&mut ctx, action),
        Some(Some((parent, WM_COMMAND, 0x10000 | 1496, combo)))
    );
    assert_eq!(
        ctx.kernel.controls.get(combo).unwrap().list_selected,
        Some(0)
    );
    // A programmatic selection does not manufacture a user notification.
    let before = ctx.kernel.posted_messages.len();
    call(&mut ctx, def_window_proc_w, &[combo, 0x14e, 1, 0]);
    assert_eq!(ctx.kernel.posted_messages.len(), before);
}

#[test]
fn screen_client_conversion_accounts_for_modal_origin_and_caption() {
    use pocket_kernel::controls::DialogPanel;
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let hwnd = 0xdead1001;
    kernel.controls.add_panel(DialogPanel {
        hwnd,
        x: 60,
        y: 30,
        w: 200,
        h: 100,
        visible: true,
        border: true,
        caption: Some("Dialog".into()),
        ok_pressed: false,
    });
    let origin = kernel.controls.panel(hwnd).unwrap().client_origin();
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    ctx.cpu
        .write_mem(
            0x2000,
            &[(-5i32).to_le_bytes(), 17i32.to_le_bytes()].concat(),
        )
        .unwrap();
    assert_eq!(
        call(&mut ctx, coordinates::client_to_screen, &[hwnd, 0x2000]),
        1
    );
    assert_eq!(ctx.cpu.read_u32_le(0x2000).unwrap() as i32, origin.0 - 5);
    assert_eq!(ctx.cpu.read_u32_le(0x2004).unwrap() as i32, origin.1 + 17);
    assert_eq!(
        call(&mut ctx, coordinates::screen_to_client, &[hwnd, 0x2000]),
        1
    );
    assert_eq!(ctx.cpu.read_u32_le(0x2000).unwrap() as i32, -5);
    assert_eq!(ctx.cpu.read_u32_le(0x2004).unwrap(), 17);
    assert_eq!(
        call(&mut ctx, coordinates::client_to_screen, &[0x12345, 0x2000]),
        0
    );
    assert_eq!(ctx.cpu.read_u32_le(0x2000).unwrap() as i32, -5);
}

#[test]
fn activating_a_child_does_not_replace_the_active_modal_dialog() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let dialog = 0xdead1001;
    kernel.active_window = dialog;
    let child = kernel.controls.create(
        dialog,
        ControlClass::Static,
        1,
        String::new(),
        0x50000000,
        0,
        0,
        24,
        24,
    );
    kernel.window_procs.insert(child, 0x3100);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(call(&mut ctx, set_active_window, &[child]), 0);
    assert_eq!(ctx.kernel.active_window, dialog);
}

#[test]
fn modal_pump_delivers_its_native_paint_without_draining_the_underlying_window() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let thunk = thunk_at(0x70000100);
    kernel.modal_dialog = Some(ModalDialog {
        previous_active: FAKE_HWND,
        previous_focus: 0,
        frame: GuestCallFrame {
            args: [0; 4],
            lr: 0x5000,
            sp: 0x8000,
        },
        hwnd: FAKE_MODAL_HWND,
        proc: 0x3100,
        result: None,
        initialised: true,
        dispatched: None,
        spins: 0,
    });
    kernel.window_procs.insert(FAKE_MODAL_HWND, 0x3100);
    kernel
        .posted_messages
        .push_back((FAKE_HWND, WM_PAINT, 0, 0));
    kernel
        .posted_messages
        .push_back((FAKE_MODAL_HWND, WM_PAINT, 0, 0));
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(
        dialog_box_indirect_param_w(&mut ctx).unwrap(),
        DispatchOutcome::JumpTo(0x3100)
    );
    assert_eq!(ctx.cpu.read_reg(ArmReg::R0).unwrap(), FAKE_MODAL_HWND);
    assert_eq!(ctx.cpu.read_reg(ArmReg::R1).unwrap(), WM_PAINT);
    assert_eq!(ctx.cpu.read_reg(ArmReg::Lr).unwrap(), thunk.thunk_va);
    assert_eq!(
        ctx.kernel.posted_messages.front(),
        Some(&(FAKE_HWND, WM_PAINT, 0, 0))
    );
}

#[test]
fn trackbar_range_position_and_console_notifications_are_stateful() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let h = kernel.controls.create(
        FAKE_DIALOG_HWND,
        ControlClass::TrackBar,
        1403,
        String::new(),
        0x50010000,
        0,
        0,
        150,
        24,
    );
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let mut send = |msg, wp, lp| control_message(&mut ctx, h, msg, wp, lp).unwrap();
    send(0x406, 1, (10 << 16) | 2);
    send(0x405, 1, 99);
    assert_eq!(send(0x400, 0, 0), 10);
    assert_eq!(send(0x417, 0, 3), 1);
    assert_eq!(send(0x418, 0, 0), 3);
    ctx.kernel.controls.focus = h;
    let action = ctx.kernel.controls.key_down(0x25).unwrap();
    assert_eq!(
        control_action_message(&mut ctx, action),
        Some(Some((FAKE_DIALOG_HWND, 0x114, 0, h)))
    );
    assert_eq!(control_message(&mut ctx, h, 0x400, 0, 0).unwrap(), 7);
}

#[test]
fn modal_teardown_discards_child_paints_and_native_userdata() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let h = kernel.controls.create(
        FAKE_MODAL_HWND,
        ControlClass::Button,
        1211,
        "Accept".into(),
        0x5001000b,
        0,
        0,
        72,
        36,
    );
    kernel.window_procs.insert(h, 0x4b007600);
    kernel.window_userdata.insert(h, 0x123400);
    kernel.posted_messages.push_back((h, WM_PAINT, 0, 0));
    kernel
        .posted_messages
        .push_back((FAKE_HWND, WM_PAINT, 0, 0));
    kernel.modal_dialog = Some(ModalDialog {
        previous_active: FAKE_HWND,
        previous_focus: 0,
        frame: GuestCallFrame {
            args: [0; 4],
            sp: 0x8000,
            lr: 0x5000,
        },
        hwnd: FAKE_MODAL_HWND,
        proc: 0x3100,
        result: Some(1),
        initialised: true,
        dispatched: None,
        spins: 0,
    });
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(
        finish_modal_dialog(&mut ctx, 1).unwrap(),
        DispatchOutcome::ReturnedR0(1)
    );
    assert!(!ctx.kernel.window_procs.contains_key(&h));
    assert!(!ctx.kernel.window_userdata.contains_key(&h));
    assert!(ctx.kernel.controls.get(h).is_none());
    assert_eq!(
        ctx.kernel
            .posted_messages
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![(FAKE_HWND, WM_PAINT, 0, 0)]
    );
}

#[test]
fn console_play_opens_and_commits_combo_instead_of_accepting_the_dialog() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let h = kernel.controls.create(
        FAKE_DIALOG_HWND,
        ControlClass::ComboBox,
        1444,
        String::new(),
        0x50010003,
        0,
        0,
        160,
        25,
    );
    let c = kernel.controls.get_mut(h).unwrap();
    c.list_items = vec![("First".into(), 0), ("Second".into(), 0)];
    c.list_selected = Some(0);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_ne!(control_message(&mut ctx, h, 0x87, 13, 0).unwrap() & 4, 0);
    control_message(&mut ctx, h, WM_KEYDOWN, 13, 0).unwrap();
    assert_eq!(control_message(&mut ctx, h, 0x157, 0, 0).unwrap(), 1);
    assert!(ctx.kernel.controls.get(h).unwrap().choice_height() > 25);
    control_message(&mut ctx, h, WM_KEYDOWN, 0x28, 0).unwrap();
    assert_eq!(control_message(&mut ctx, h, 0x147, 0, 0).unwrap(), 1);
    control_message(&mut ctx, h, WM_KEYDOWN, 13, 0).unwrap();
    assert_eq!(control_message(&mut ctx, h, 0x157, 0, 0).unwrap(), 0);
    assert_eq!(control_message(&mut ctx, h, 0x87, 27, 0).unwrap() & 4, 0);
}

#[test]
fn changing_an_owner_drawn_caption_invalidates_its_native_cached_paint() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let h = kernel.controls.create(
        FAKE_HWND,
        ControlClass::Button,
        1211,
        "OK".into(),
        0x5001000b,
        0,
        0,
        72,
        36,
    );
    kernel
        .gdi
        .owner_draw_states
        .insert(h, (false, true, false, 72, 36));
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    set_control_text(&mut ctx, h, "Accept");
    ctx.kernel.composite_controls();
    assert!(ctx
        .kernel
        .posted_messages
        .iter()
        .any(|m| m.0 == h && m.1 == WM_PAINT));
    assert_eq!(ctx.kernel.controls.get(h).unwrap().text, "Accept");
}

#[test]
fn custom_native_child_surface_survives_dialog_and_control_compositing() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    kernel.framebuffer = Framebuffer::new(320, 240);
    let panel = 0xdead1000;
    let map = 0xdead1001;
    kernel
        .controls
        .add_panel(pocket_kernel::controls::DialogPanel {
            hwnd: panel,
            x: 0,
            y: 0,
            w: 320,
            h: 240,
            visible: true,
            border: false,
            caption: None,
            ok_pressed: false,
        });
    kernel.window_parents.insert(map, panel);
    kernel.window_rects.insert(map, (40, 50, 280, 165));
    kernel.window_visibility.insert(map, true);
    let overlay = kernel.controls.create(
        panel,
        ControlClass::Button,
        1,
        String::new(),
        0x50000000,
        100,
        60,
        20,
        20,
    );
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let dc = window_dc(&mut ctx, map);
    surface_for_dc(ctx.kernel, dc)
        .unwrap()
        .fill_rect(0, 0, 280, 165, 0x07e0);
    ctx.kernel.composite_controls();
    assert_eq!(pixel(ctx.kernel, 40, 50), 0x07e0);
    assert_eq!(pixel(ctx.kernel, 319, 214), 0x07e0);
    assert_ne!(pixel(ctx.kernel, 105, 65), 0x07e0);
    ctx.kernel.controls.get_mut(overlay).unwrap().visible = false;
    ctx.kernel.composite_controls();
    assert_eq!(pixel(ctx.kernel, 105, 65), 0x07e0);
}

#[test]
fn native_pointer_uses_hit_window_and_client_origin_instead_of_key_focus() {
    let mut kernel = fresh_kernel();
    let panel = 0xdead1000;
    let map = 0xdead1001;
    kernel
        .controls
        .add_panel(pocket_kernel::controls::DialogPanel {
            hwnd: panel,
            x: 20,
            y: 30,
            w: 200,
            h: 150,
            visible: true,
            border: true,
            caption: Some("Map".into()),
            ok_pressed: false,
        });
    kernel.active_window = panel;
    kernel.window_rects.insert(map, (5, 7, 100, 80));
    kernel.window_styles.insert(map, (0x50000000, 0));
    kernel.window_parents.insert(map, panel);
    kernel.window_visibility.insert(map, true);
    let button = kernel.controls.create(
        panel,
        ControlClass::Button,
        1,
        String::new(),
        0x50000000,
        110,
        100,
        50,
        20,
    );
    kernel.controls.focus = button;
    let origin = kernel.controls.panel(panel).unwrap().client_origin();
    let x = (origin.0 + 5 + 10) as u16;
    let y = (origin.1 + 7 + 11) as u16;
    assert_eq!(
        coordinates::origin(&kernel, map),
        Some((origin.0 + 5, origin.1 + 7))
    );
    assert_eq!(
        addressed_input(&kernel, InputEvent::PointerDown { x, y }),
        Some((map, WM_LBUTTONDOWN, MK_LBUTTON, (11 << 16) | 10))
    );
    assert_eq!(
        addressed_input(&kernel, InputEvent::KeyDown { vk: 37 })
            .unwrap()
            .0,
        button
    );
    kernel.window_visibility.insert(map, false);
    assert_eq!(
        addressed_input(&kernel, InputEvent::PointerUp { x, y }),
        Some((panel, WM_LBUTTONUP, 0, (18 << 16) | 15))
    );
}

#[test]
fn child_z_order_apis_respect_nozorder_and_do_not_move_controls() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let first = kernel.controls.create(
        FAKE_HWND,
        ControlClass::Button,
        1,
        String::new(),
        0x50000000,
        10,
        10,
        40,
        20,
    );
    let second = kernel.controls.create(
        FAKE_HWND,
        ControlClass::Button,
        2,
        String::new(),
        0x50000000,
        10,
        10,
        40,
        20,
    );
    let thunk = thunk_at(0x70000100);
    let sp = cpu.read_reg(ArmReg::Sp).unwrap();
    let stack = sp + cpu.stack_arg_offset();
    let mut invoke = |kernel: &mut KernelState, flags: u32| {
        cpu.write_reg(ArmReg::R0, first).unwrap();
        cpu.write_reg(ArmReg::R1, 0).unwrap();
        cpu.write_reg(ArmReg::R2, 999).unwrap();
        cpu.write_reg(ArmReg::R3, 999).unwrap();
        cpu.write_mem(stack, &999u32.to_le_bytes()).unwrap();
        cpu.write_mem(stack + 4, &999u32.to_le_bytes()).unwrap();
        cpu.write_mem(stack + 8, &flags.to_le_bytes()).unwrap();
        set_window_pos(&mut CallCtx {
            cpu: &mut cpu,
            kernel,
            thunk: &thunk,
        })
        .unwrap();
    };
    invoke(&mut kernel, 7); // SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER
    assert_eq!(kernel.controls.hit_test(20, 15), Some(second));
    invoke(&mut kernel, 3);
    assert_eq!(kernel.controls.hit_test(20, 15), Some(first));
    let c = kernel.controls.get(first).unwrap();
    assert_eq!((c.x, c.y, c.w, c.h), (10, 10, 40, 20));
    drop(invoke);
    cpu.write_reg(ArmReg::R0, second).unwrap();
    bring_window_to_top(&mut CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    })
    .unwrap();
    assert_eq!(kernel.controls.hit_test(20, 15), Some(second));
}

#[test]
fn font_height_drives_extent_and_textout_without_changing_stock_metrics() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let thunk = thunk_at(0x70000100);
    let dc = kernel.gdi.create_memory_dc();
    let bitmap = kernel.gdi.create_compatible_bitmap(100, 40);
    kernel.gdi.select_into(dc, bitmap);
    assert!(selected_text_font(&kernel, dc).is_none());
    let mut raw = [0u8; 92];
    raw[..4].copy_from_slice(&(-24i32).to_le_bytes());
    raw[16..20].copy_from_slice(&700i32.to_le_bytes());
    for (i, u) in "Arial".encode_utf16().enumerate() {
        raw[28 + i * 2..30 + i * 2].copy_from_slice(&u.to_le_bytes());
    }
    cpu.write_mem(0x1100, &raw).unwrap();
    write_wide_str(&mut cpu, 0x1200, 16, "WW").unwrap();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let font = call(&mut ctx, create_font_indirect, &[0x1100]);
    ctx.kernel.gdi.select_into(dc, font);
    assert_eq!(
        selected_text_font(ctx.kernel, dc).unwrap().face_name(),
        "Arial"
    );
    assert_eq!(
        call(
            &mut ctx,
            get_text_extent_ex_point_w,
            &[dc, 0x1200, 2, 30, 0x1300, 0x1400, 0x1500]
        ),
        1
    );
    let size = ctx.cpu.read_mem(0x1500, 8).unwrap();
    let width = i32::from_le_bytes(size[..4].try_into().unwrap());
    let height = i32::from_le_bytes(size[4..].try_into().unwrap());
    assert!((40..=50).contains(&width));
    assert!((24..=32).contains(&height));
    assert_eq!(ctx.cpu.read_mem(0x1300, 4).unwrap(), 1u32.to_le_bytes());
    assert_eq!(ctx.cpu.read_mem(0x1404, 4).unwrap(), width.to_le_bytes());
    call(&mut ctx, get_object_w, &[font, 92, 0x1600]);
    assert_eq!(ctx.cpu.read_mem(0x1600, 92).unwrap(), raw);
    ctx.kernel.gdi.dc_mut(dc).unwrap().text_color = 0x00ffffff;
    ctx.kernel.gdi.dc_mut(dc).unwrap().bk_transparent = true;
    call(&mut ctx, text_out_w, &[dc, 0, 0, 0x1200, 2]);
    let bm = ctx.kernel.gdi.bitmap(bitmap).unwrap();
    assert!(bm.pixels[8 * 100 * 2..24 * 100 * 2].iter().any(|&b| b != 0));
    assert!(bm.pixels[32 * 100 * 2..].iter().all(|&b| b == 0));
}

#[test]
fn font_height_drawtext_wraps_and_measures_the_same_cells() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let thunk = thunk_at(0x70000100);
    let dc = kernel.gdi.create_memory_dc();
    let bitmap = kernel.gdi.create_compatible_bitmap(60, 40);
    kernel.gdi.select_into(dc, bitmap);
    let font = kernel.gdi.create_font(-16);
    kernel.gdi.select_into(dc, font);
    kernel.gdi.dc_mut(dc).unwrap().text_color = 0x00ffffff;
    kernel.gdi.dc_mut(dc).unwrap().bk_transparent = true;
    write_wide_str(&mut cpu, 0x1200, 16, "WWWWWW").unwrap();
    cpu.write_mem(
        0x1400,
        &[
            0i32.to_le_bytes(),
            0i32.to_le_bytes(),
            36i32.to_le_bytes(),
            40i32.to_le_bytes(),
        ]
        .concat(),
    )
    .unwrap();
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let height = call(&mut ctx, draw_text_w, &[dc, 0x1200, 6, 0x1400, 0x410]);
    assert!((50..=60).contains(&height)); // Three lines of two proportional W glyphs.
    let bounds = ctx.cpu.read_mem(0x1400 + 8, 8).unwrap();
    let width = i32::from_le_bytes(bounds[..4].try_into().unwrap());
    assert!((28..=36).contains(&width));
    assert_eq!(
        i32::from_le_bytes(bounds[4..].try_into().unwrap()),
        height as i32
    );
    assert_eq!(
        call(&mut ctx, draw_text_w, &[dc, 0x1200, 6, 0x1400, 0x10]),
        height
    );
    let bm = ctx.kernel.gdi.bitmap(bitmap).unwrap();
    assert!(bm.pixels[16 * 60 * 2..32 * 60 * 2].iter().any(|&b| b != 0));
    for row in bm.pixels.chunks_exact(120) {
        assert!(row[72..].iter().all(|&b| b == 0));
    }
}

#[test]
fn standard_closed_combo_backing_cannot_cover_buttons_below_it() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let combo = kernel.controls.create(
        FAKE_HWND,
        ControlClass::ComboBox,
        1,
        "UK English".into(),
        0x50000003,
        72,
        40,
        150,
        180,
    );
    kernel.controls.create(
        FAKE_HWND,
        ControlClass::Button,
        2,
        "OK".into(),
        0x50000000,
        62,
        190,
        80,
        34,
    );
    kernel.window_rects.insert(combo, (72, 40, 150, 180));
    kernel.window_parents.insert(combo, FAKE_HWND);
    kernel.composite_controls();
    let expected = pixel(&kernel, 100, 200);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let dc = window_dc(&mut ctx, combo);
    surface_for_dc(ctx.kernel, dc)
        .unwrap()
        .fill_rect(0, 0, 150, 180, 0x001f);
    ctx.kernel.composite_controls();
    assert_eq!(pixel(ctx.kernel, 100, 200), expected);
    assert_ne!(expected, 0x001f);
    assert_eq!(call(&mut ctx, get_client_rect, &[combo, 0x1100]), 1);
    assert_eq!(ctx.cpu.read_u32_le(0x110c).unwrap(), 20);
}

#[test]
fn crt_descriptors_preserve_stream_mode_and_reject_invalid_values() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    for fd in 0..3 {
        assert_eq!(call(&mut ctx, crt_fileno, &[STD_FILE_BASE + fd]), fd);
        assert_eq!(call(&mut ctx, crt_setmode, &[fd, 0x8000]), 0x4000);
        assert_eq!(call(&mut ctx, crt_setmode, &[fd, 0x4000]), 0x8000);
        assert_eq!(call(&mut ctx, crt_setmode, &[fd, 0]), u32::MAX);
        assert!(ctx.kernel.vfs.stdio_text_modes[fd as usize]);
    }
    assert_eq!(call(&mut ctx, crt_fileno, &[0]), u32::MAX);
    assert_eq!(call(&mut ctx, crt_setmode, &[999, 0x8000]), u32::MAX);
}

#[test]
fn dialog_item_message_resolves_child_and_forwards_fifth_argument() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    let child = 0xdead1008;
    kernel.window_ids.insert(child, 1003);
    kernel.window_parents.insert(child, FAKE_HWND);
    kernel.window_procs.insert(child, 0x2300);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    let sp = ctx.cpu.read_reg(ArmReg::Sp).unwrap();
    ctx.cpu.write_mem(sp, &0x12345678u32.to_le_bytes()).unwrap();
    for (r, v) in [
        (ArmReg::R0, FAKE_HWND),
        (ArmReg::R1, 1003),
        (ArmReg::R2, 0xf1),
        (ArmReg::R3, 1),
    ] {
        ctx.cpu.write_reg(r, v).unwrap();
    }
    assert_eq!(
        send_dlg_item_message_w(&mut ctx).unwrap(),
        DispatchOutcome::JumpTo(0x2300)
    );
    for (r, v) in [
        (ArmReg::R0, child),
        (ArmReg::R1, 0xf1),
        (ArmReg::R2, 1),
        (ArmReg::R3, 0x12345678),
    ] {
        assert_eq!(ctx.cpu.read_reg(r).unwrap(), v);
    }
}

#[test]
fn synthetic_paints_skip_hidden_primary_and_child_windows() {
    let mut kernel = fresh_kernel();
    let main = 0xdead1001;
    let child = 0xdead1002;
    kernel.active_window = FAKE_HWND;
    for hwnd in [FAKE_HWND, main, child] {
        kernel.window_procs.insert(hwnd, 0x2300);
        kernel.window_styles.insert(
            hwnd,
            (
                if hwnd == child {
                    0x50000000
                } else {
                    0x10000000
                },
                0,
            ),
        );
        kernel.window_visibility.insert(hwnd, hwnd != FAKE_HWND);
    }
    kernel.window_parents.insert(child, main);
    assert_eq!(synthetic_paint_target(&kernel), main);
    kernel.window_visibility.insert(main, false);
    assert_eq!(synthetic_paint_target(&kernel), 0);
}

#[test]
fn selected_clip_is_copied_and_limits_drawing_until_reset() {
    let mut cpu = scanf_cpu();
    let mut kernel = fresh_kernel();
    kernel.framebuffer = Framebuffer::new(8, 4);
    kernel.framebuffer.fill(0xffff);
    let dc = pocket_kernel::gdi::GDI_SCREEN_DC;
    let region = kernel.gdi.create_region(vec![[1, 1, 3, 3], [5, 0, 7, 2]]);
    let thunk = thunk_at(0x70000100);
    let mut ctx = CallCtx {
        cpu: &mut cpu,
        kernel: &mut kernel,
        thunk: &thunk,
    };
    assert_eq!(call(&mut ctx, select_clip_rgn, &[dc, region]), 3);
    ctx.kernel.gdi.delete(region);
    {
        let mut surface = surface_for_dc(ctx.kernel, dc).unwrap();
        surface.fill_rect(0, 0, 8, 4, 0);
    }
    for y in 0..4 {
        for x in 0..8 {
            let inside =
                ((1..3).contains(&x) && (1..3).contains(&y)) || ((5..7).contains(&x) && y < 2);
            assert_eq!(pixel(ctx.kernel, x, y), if inside { 0 } else { 0xffff });
        }
    }
    assert_eq!(call(&mut ctx, get_clip_box, &[dc, 0x2100]), 3);
    assert_eq!(
        ctx.cpu.read_mem(0x2100, 16).unwrap(),
        [1i32, 0, 7, 3]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(call(&mut ctx, select_clip_rgn, &[dc, 0]), 2);
    surface_for_dc(ctx.kernel, dc)
        .unwrap()
        .fill_rect(0, 0, 8, 4, 0x1234);
    assert_eq!(pixel(ctx.kernel, 0, 0), 0x1234);
    assert_eq!(call(&mut ctx, select_clip_rgn, &[dc, region]), 0);
}
