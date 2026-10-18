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
    list_view_message(&mut ctx, hwnd, 0x1002, 1, list).unwrap();
    assert_eq!(
        list_view_message(&mut ctx, hwnd, 0x1003, 1, 0).unwrap(),
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
