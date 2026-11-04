use super::tests::{call, fresh_cpu, guard, SCRATCH};
use super::*;
use crate::gx::tests::fresh_kernel;
use pocket_cpu::{stub::StubCpu, Cpu};
use pocket_gles::*;
use pocket_kernel::KernelState;

fn create(cpu: &mut StubCpu, k: &mut KernelState, share: u32) -> u32 {
    let h = call(
        cpu,
        k,
        egl_create_context,
        &[DISPLAY_HANDLE, CONFIG_HANDLE, share, 0],
    );
    assert_ne!(h, 0);
    h
}
fn bind(cpu: &mut StubCpu, k: &mut KernelState, context: u32, surface: u32) {
    assert_eq!(
        call(
            cpu,
            k,
            egl_make_current,
            &[DISPLAY_HANDLE, surface, surface, context]
        ),
        EGL_TRUE
    );
}
fn texture(c: &mut Context, name: u32, rgba: [u8; 4]) {
    c.bind_texture(GL_TEXTURE_2D, name);
    c.tex_image_2d(GL_TEXTURE_2D, 0, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, &rgba);
    c.tex_parameter(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
}
fn quad(cpu: &mut StubCpu) {
    let vertices: [f32; 12] = [-1., -1., 0., 1., -1., 0., 1., 1., 0., -1., 1., 0.];
    cpu.write_mem(
        SCRATCH,
        &vertices
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
}
fn render(cpu: &mut StubCpu) -> [u8; 4] {
    with_ctx(|c| {
        c.set_capability(GL_TEXTURE_2D, true);
        c.bind_buffer(GL_ARRAY_BUFFER, 0);
        c.set_vertex_pointer(3, GL_FLOAT, 0, SCRATCH);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        c.draw_arrays(&mut CpuMem(cpu), GL_TRIANGLE_FAN, 0, 4);
        c.sync_gpu();
        c.target.color[(4 * 8 + 4) * 4..(4 * 8 + 4) * 4 + 4]
            .try_into()
            .unwrap()
    })
}

#[test]
fn independent_contexts_preserve_matrices_arrays_errors_alignment_and_objects() {
    let _g = guard();
    reset_for_test(8, 8);
    let (mut cpu, mut k) = (fresh_cpu(), fresh_kernel());
    with_ctx(|c| {
        c.translate(2., 3., 4.);
        c.current_color = [0., 1., 0., 1.];
        c.set_vertex_pointer(3, GL_FLOAT, 0, 0x1234);
        texture(c, 17, [255, 0, 0, 255]);
        c.bind_buffer(GL_ARRAY_BUFFER, 23);
        c.buffer_data(GL_ARRAY_BUFFER, 4, Some(&[1, 2, 3, 4]), GL_STATIC_DRAW);
        c.set_error(GL_INVALID_ENUM);
    });
    PACK_ALIGNMENT.with(|a| a.set(1));
    UNPACK_ALIGNMENT.with(|a| a.set(8));
    let b = create(&mut cpu, &mut k, 0);
    assert_ne!(b, CONTEXT_HANDLE);
    bind(&mut cpu, &mut k, b, SURFACE_HANDLE);
    with_ctx(|c| {
        assert_eq!(*c.modelview.current(), pocket_gles::matrix::IDENTITY);
        assert_eq!(c.current_color, [1.; 4]);
        assert_eq!(c.vertex_array.pointer, 0);
        assert_eq!(c.take_error(), GL_NO_ERROR);
        assert!(!c.is_texture(17));
        assert!(!c.is_buffer(23));
        texture(c, 17, [0, 0, 255, 255]);
    });
    assert_eq!(PACK_ALIGNMENT.with(Cell::get), 4);
    assert_eq!(UNPACK_ALIGNMENT.with(Cell::get), 4);
    bind(&mut cpu, &mut k, CONTEXT_HANDLE, SURFACE_HANDLE);
    with_ctx(|c| {
        assert_eq!(&c.modelview.current()[12..15], &[2., 3., 4.]);
        assert_eq!(c.current_color, [0., 1., 0., 1.]);
        assert_eq!(c.vertex_array.pointer, 0x1234);
        assert_eq!(c.take_error(), GL_INVALID_ENUM);
        assert_eq!(c.textures[&17].rgba, [255, 0, 0, 255]);
        assert_eq!(c.buffers[&23].data, [1, 2, 3, 4]);
    });
    assert_eq!(PACK_ALIGNMENT.with(Cell::get), 1);
    assert_eq!(UNPACK_ALIGNMENT.with(Cell::get), 8);
    let parent = suspend();
    reset_for_test(2, 2);
    resume(parent);
    bind(&mut cpu, &mut k, b, SURFACE_HANDLE);
    with_ctx(|c| assert_eq!(c.textures[&17].rgba, [0, 0, 255, 255]));
}

fn namespace_roundtrip(gpu: bool) {
    reset_for_test(8, 8);
    let (mut cpu, mut k) = (fresh_cpu(), fresh_kernel());
    set_gpu_renderer(gpu);
    quad(&mut cpu);
    with_ctx(|c| texture(c, 17, [255, 0, 0, 255]));
    assert_eq!(render(&mut cpu), [255, 0, 0, 255]);
    let b = create(&mut cpu, &mut k, 0);
    bind(&mut cpu, &mut k, b, SURFACE_HANDLE);
    with_ctx(|c| texture(c, 17, [0, 0, 255, 255]));
    assert_eq!(render(&mut cpu), [0, 0, 255, 255]);
    bind(&mut cpu, &mut k, CONTEXT_HANDLE, SURFACE_HANDLE);
    assert_eq!(render(&mut cpu), [255, 0, 0, 255]);
}
#[test]
fn equal_object_names_in_unshared_contexts_render_independently_software() {
    let _g = guard();
    namespace_roundtrip(false);
}
#[test]
#[ignore = "requires host OpenGL"]
fn equal_object_names_in_unshared_contexts_render_independently_gpu() {
    let _g = guard();
    namespace_roundtrip(true);
}

fn shared_roundtrip(gpu: bool) {
    reset_for_test(8, 8);
    let (mut cpu, mut k) = (fresh_cpu(), fresh_kernel());
    set_gpu_renderer(gpu);
    quad(&mut cpu);
    with_ctx(|c| {
        texture(c, 17, [255, 0, 0, 255]);
        c.bind_buffer(GL_ARRAY_BUFFER, 23);
        c.buffer_data(GL_ARRAY_BUFFER, 4, Some(&[1, 2, 3, 4]), GL_STATIC_DRAW);
        c.set_vertex_pointer(3, GL_FLOAT, 0, 0);
        c.bind_buffer(GL_ARRAY_BUFFER, 0);
        texture(c, 0, [0, 255, 0, 255]);
        c.bind_texture(GL_TEXTURE_2D, 17);
    });
    let b = create(&mut cpu, &mut k, CONTEXT_HANDLE);
    bind(&mut cpu, &mut k, b, SURFACE_HANDLE);
    with_ctx(|c| {
        assert!(c.is_texture(17));
        assert!(c.is_buffer(23));
        assert_eq!(c.texture_units[0].bound_texture, 0);
        assert!(!c.textures.contains_key(&0));
        c.bind_texture(GL_TEXTURE_2D, 17);
        c.delete_textures(&[17]);
        c.delete_buffers(&[23]);
        assert!(!c.is_texture(17));
        assert!(!c.is_buffer(23));
        texture(c, 17, [0, 0, 255, 255]);
        c.bind_buffer(GL_ARRAY_BUFFER, 23);
        c.buffer_data(GL_ARRAY_BUFFER, 4, Some(&[5, 6, 7, 8]), GL_STATIC_DRAW);
        assert_ne!(c.texture_units[0].bound_texture, 17);
        assert_ne!(c.array_buffer, 23);
    });
    assert_eq!(render(&mut cpu), [0, 0, 255, 255]);
    bind(&mut cpu, &mut k, CONTEXT_HANDLE, SURFACE_HANDLE);
    with_ctx(|c| {
        assert_eq!(c.texture_units[0].bound_texture, 17);
        assert_eq!(c.textures[&17].rgba, [255, 0, 0, 255]);
        assert_eq!(c.buffers[&c.vertex_array.buffer].data, [1, 2, 3, 4]);
        assert_eq!(c.buffer_object_name(c.vertex_array.buffer), 23);
        assert_eq!(c.query_state(0x8069).unwrap().numbers(), vec![17.]);
        c.bind_buffer(GL_ARRAY_BUFFER, 0);
        c.set_vertex_pointer(3, GL_FLOAT, 0, SCRATCH);
    });
    assert_eq!(render(&mut cpu), [255, 0, 0, 255]);
    with_ctx(|c| {
        c.bind_texture(GL_TEXTURE_2D, 0);
        assert_eq!(c.textures[&0].rgba, [0, 255, 0, 255]);
        c.bind_texture(GL_TEXTURE_2D, 17); // Now bind the newly named object.
    });
    assert_eq!(render(&mut cpu), [0, 0, 255, 255]);
    bind(&mut cpu, &mut k, b, SURFACE_HANDLE);
    with_ctx(|c| {
        assert!(!c.textures.contains_key(&17));
        assert!(!c.buffers.contains_key(&23));
    });
    let transitive = create(&mut cpu, &mut k, b);
    assert_eq!(
        call(
            &mut cpu,
            &mut k,
            egl_destroy_context,
            &[DISPLAY_HANDLE, CONTEXT_HANDLE]
        ),
        EGL_TRUE
    );
    assert_eq!(
        call(&mut cpu, &mut k, egl_destroy_context, &[DISPLAY_HANDLE, b]),
        EGL_TRUE
    );
    bind(&mut cpu, &mut k, transitive, SURFACE_HANDLE);
    with_ctx(|c| {
        c.bind_texture(GL_TEXTURE_2D, 17);
        assert!(c.is_texture(17));
    });
    assert_eq!(render(&mut cpu), [0, 0, 255, 255]);
}
#[test]
fn shared_object_deletion_name_reuse_texture_zero_and_transitive_lifetime_software() {
    let _g = guard();
    shared_roundtrip(false);
}
#[test]
#[ignore = "requires host OpenGL"]
fn shared_object_deletion_name_reuse_texture_zero_and_transitive_lifetime_gpu() {
    let _g = guard();
    shared_roundtrip(true);
}

#[test]
fn guest_threads_have_independent_bindings_errors_and_exclusive_surface_ownership() {
    let _g = guard();
    reset_for_test(8, 8);
    let (mut cpu, mut k) = (fresh_cpu(), fresh_kernel());
    let b = create(&mut cpu, &mut k, 0);
    with_ctx(|c| c.current_color = [1., 0., 0., 1.]);
    set_egl_error(EGL_BAD_PARAMETER);
    enter_thread(1);
    assert_eq!(call(&mut cpu, &mut k, egl_get_current_context, &[]), 0);
    assert_eq!(call(&mut cpu, &mut k, egl_get_error, &[]), EGL_SUCCESS);
    assert_eq!(
        call(
            &mut cpu,
            &mut k,
            egl_make_current,
            &[DISPLAY_HANDLE, SURFACE_HANDLE, SURFACE_HANDLE, b]
        ),
        EGL_FALSE
    );
    assert_eq!(call(&mut cpu, &mut k, egl_get_error, &[]), EGL_BAD_ACCESS);
    cpu.write_mem(
        SCRATCH,
        &[EGL_WIDTH, 2, EGL_HEIGHT, 2, EGL_NONE]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let off = call(
        &mut cpu,
        &mut k,
        egl_create_pbuffer,
        &[DISPLAY_HANDLE, CONFIG_HANDLE, SCRATCH],
    );
    bind(&mut cpu, &mut k, b, off);
    with_ctx(|c| c.current_color = [0., 0., 1., 1.]);
    enter_thread(0);
    assert_eq!(
        call(&mut cpu, &mut k, egl_get_current_context, &[]),
        CONTEXT_HANDLE
    );
    with_ctx(|c| assert_eq!(c.current_color, [1., 0., 0., 1.]));
    assert_eq!(
        call(&mut cpu, &mut k, egl_get_error, &[]),
        EGL_BAD_PARAMETER
    );
    assert_eq!(
        call(
            &mut cpu,
            &mut k,
            egl_make_current,
            &[DISPLAY_HANDLE, SURFACE_HANDLE, SURFACE_HANDLE, b]
        ),
        EGL_FALSE
    );
    assert_eq!(call(&mut cpu, &mut k, egl_get_error, &[]), EGL_BAD_ACCESS);
    call(&mut cpu, &mut k, egl_terminate, &[DISPLAY_HANDLE]);
    call(&mut cpu, &mut k, egl_release_thread, &[]);
    enter_thread(1);
    assert_eq!(call(&mut cpu, &mut k, egl_get_current_context, &[]), b);
    with_ctx(|c| assert_eq!(c.current_color, [0., 0., 1., 1.]));
    call(&mut cpu, &mut k, egl_release_thread, &[]);
    assert!(SURFACES.with(|s| s.borrow().contexts.is_empty()));
    assert!(SURFACES.with(|s| s.borrow().images.is_empty()));
    enter_thread(0);
}

#[test]
fn pbuffer_attributes_zero_sizes_and_texture_capabilities_are_consistent() {
    let _g = guard();
    reset_for_test(8, 8);
    let (mut cpu, mut k) = (fresh_cpu(), fresh_kernel());
    let zero = call(
        &mut cpu,
        &mut k,
        egl_create_pbuffer,
        &[DISPLAY_HANDLE, CONFIG_HANDLE, 0],
    );
    assert_ne!(zero, 0);
    bind(&mut cpu, &mut k, CONTEXT_HANDLE, zero);
    with_ctx(|c| {
        assert_eq!((c.target.width, c.target.height), (0, 0));
        c.clear(GL_COLOR_BUFFER_BIT);
    });
    for (attrs, want, error) in [
        (vec![EGL_WIDTH, u32::MAX, EGL_NONE], 0, EGL_BAD_PARAMETER),
        (
            vec![EGL_WIDTH, 2048, EGL_HEIGHT, 2, EGL_NONE],
            0,
            EGL_BAD_ALLOC,
        ),
        (vec![0x3080, 0x305e, EGL_NONE], 0, EGL_BAD_MATCH),
        (vec![0x3080, 0xdead, EGL_NONE], 0, EGL_BAD_ATTRIBUTE),
    ] {
        cpu.write_mem(
            SCRATCH,
            &attrs
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                egl_create_pbuffer,
                &[DISPLAY_HANDLE, CONFIG_HANDLE, SCRATCH]
            ),
            want
        );
        assert_eq!(call(&mut cpu, &mut k, egl_get_error, &[]), error);
    }
    let attrs = [
        EGL_WIDTH,
        2048,
        EGL_HEIGHT,
        2,
        EGL_LARGEST_PBUFFER,
        1,
        EGL_NONE,
    ];
    cpu.write_mem(
        SCRATCH,
        &attrs
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let large = call(
        &mut cpu,
        &mut k,
        egl_create_pbuffer,
        &[DISPLAY_HANDLE, CONFIG_HANDLE, SCRATCH],
    );
    assert_ne!(large, 0);
    for (attr, want) in [
        (EGL_WIDTH, 1024),
        (EGL_HEIGHT, 2),
        (EGL_LARGEST_PBUFFER, 1),
        (0x3080, 0x305c),
        (0x3081, 0x305c),
        (0x3082, 0),
        (0x3083, 0),
    ] {
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                egl_query_surface,
                &[DISPLAY_HANDLE, large, attr, SCRATCH]
            ),
            EGL_TRUE
        );
        assert_eq!(cpu.read_u32_le(SCRATCH).unwrap(), want);
    }
    assert_eq!(
        call(
            &mut cpu,
            &mut k,
            egl_texture_surface,
            &[DISPLAY_HANDLE, large, 0x3084]
        ),
        EGL_FALSE
    );
    assert_eq!(call(&mut cpu, &mut k, egl_get_error, &[]), EGL_BAD_MATCH);
}

fn native_pixmap_roundtrip(gpu: bool) {
    reset_for_test(8, 8);
    let (mut cpu, mut k) = (fresh_cpu(), fresh_kernel());
    set_gpu_renderer(gpu);
    set_internal_resolution(2);
    let bm = k.gdi.register_dib(pocket_kernel::gdi::Bitmap::new_dib(
        2,
        2,
        24,
        SCRATCH + 256,
        8,
        true,
        vec![],
        false,
    ));
    cpu.write_mem(
        SCRATCH + 256,
        &[0, 0, 255, 0, 0, 255, 0, 0, 0, 0, 255, 0, 0, 255, 0, 0],
    )
    .unwrap();
    let pix = call(
        &mut cpu,
        &mut k,
        egl_create_pixmap,
        &[DISPLAY_HANDLE, CONFIG_HANDLE, bm, 0],
    );
    assert_ne!(pix, 0);
    bind(&mut cpu, &mut k, CONTEXT_HANDLE, pix);
    with_ctx(|c| {
        c.sync_gpu();
        assert_eq!(&c.target.color[..4], &[255, 0, 0, 255]);
        c.clear_depth = 0.25;
        c.clear(GL_DEPTH_BUFFER_BIT);
    });
    cpu.write_mem(
        SCRATCH + 256,
        &[255, 0, 0, 255, 0, 0, 0, 0, 255, 0, 0, 255, 0, 0, 0, 0],
    )
    .unwrap();
    assert_eq!(call(&mut cpu, &mut k, egl_wait_native, &[0x305b]), EGL_TRUE);
    quad(&mut cpu);
    with_ctx(|c| {
        c.set_capability(GL_DEPTH_TEST, true);
        c.set_capability(GL_TEXTURE_2D, false);
        c.current_color = [1., 0., 0., 1.];
        c.set_vertex_pointer(3, GL_FLOAT, 0, SCRATCH);
        c.set_client_state(GL_VERTEX_ARRAY, true);
        c.draw_arrays(&mut CpuMem(&mut cpu), GL_TRIANGLE_FAN, 0, 4);
    });
    with_ctx(|c| {
        c.sync_gpu();
        assert_eq!(&c.target.color[..4], &[0, 0, 255, 255]);
        if !gpu {
            assert!(c.target.depth.iter().all(|d| *d == 0.25));
        }
        c.clear_color = [0., 1., 0., 1.];
        c.clear(GL_COLOR_BUFFER_BIT);
    });
    assert_eq!(call(&mut cpu, &mut k, egl_wait, &[]), EGL_TRUE);
    assert_eq!(cpu.read_mem(SCRATCH + 256, 3).unwrap(), [0, 255, 0]);
}
#[test]
fn native_pixmap_guest_dib_writes_and_gl_writeback_software() {
    let _g = guard();
    native_pixmap_roundtrip(false);
}
#[test]
#[ignore = "requires host OpenGL"]
fn native_pixmap_guest_dib_writes_and_gl_writeback_gpu() {
    let _g = guard();
    native_pixmap_roundtrip(true);
}

#[test]
fn invalid_swap_and_copy_calls_never_present_or_mutate_current_binding() {
    let _g = guard();
    reset_for_test(8, 8);
    let (mut cpu, mut k) = (fresh_cpu(), fresh_kernel());
    let frames = k.framebuffer.frame_counter;
    for (args, error) in [
        ([0xdead, SURFACE_HANDLE], EGL_BAD_DISPLAY),
        ([DISPLAY_HANDLE, 0xdead], EGL_BAD_SURFACE),
    ] {
        assert_eq!(call(&mut cpu, &mut k, egl_swap_buffers, &args), EGL_FALSE);
        assert_eq!(call(&mut cpu, &mut k, egl_get_error, &[]), error);
        assert_eq!(k.framebuffer.frame_counter, frames);
        assert_eq!(
            call(&mut cpu, &mut k, egl_get_current_context, &[]),
            CONTEXT_HANDLE
        );
    }
    assert_eq!(
        call(
            &mut cpu,
            &mut k,
            egl_copy_buffers,
            &[0xdead, SURFACE_HANDLE, 0]
        ),
        EGL_FALSE
    );
    assert_eq!(call(&mut cpu, &mut k, egl_get_error, &[]), EGL_BAD_DISPLAY);
}

#[test]
fn dispatcher_restores_egl_bindings_for_the_scheduled_guest_thread() {
    use pocket_kernel::{DispatchOutcome, Dispatcher, Thunk};
    use pocket_pe::ImportBinding;
    let _g = guard();
    let mut d = WinCeDispatcher::new();
    reset_for_test(8, 8);
    let (mut cpu, mut k) = (fresh_cpu(), fresh_kernel());
    let thunk = Thunk {
        thunk_va: 0x70000000,
        iat_va: 0,
        dll: "libEGL.dll".into(),
        binding: ImportBinding::Name("eglGetCurrentContext".into()),
        friendly_name: None,
    };
    k.current_thread = 1;
    assert!(matches!(
        d.dispatch(&mut cpu, &thunk, &mut k).unwrap(),
        DispatchOutcome::ReturnedR0(0)
    ));
    k.current_thread = 0;
    assert!(matches!(
        d.dispatch(&mut cpu, &thunk, &mut k).unwrap(),
        DispatchOutcome::ReturnedR0(CONTEXT_HANDLE)
    ));
}

#[test]
fn window_handles_are_not_resurrected_and_non_pbuffer_queries_preserve_the_output() {
    let _g = guard();
    reset_for_test(8, 8);
    let (mut cpu, mut k) = (fresh_cpu(), fresh_kernel());
    cpu.write_mem(SCRATCH, &0xaaaaaaaau32.to_le_bytes())
        .unwrap();
    for attr in [EGL_LARGEST_PBUFFER, 0x3080, 0x3081, 0x3082, 0x3083] {
        assert_eq!(
            call(
                &mut cpu,
                &mut k,
                egl_query_surface,
                &[DISPLAY_HANDLE, SURFACE_HANDLE, attr, SCRATCH]
            ),
            EGL_TRUE
        );
        assert_eq!(cpu.read_u32_le(SCRATCH).unwrap(), 0xaaaaaaaa);
    }
    assert_eq!(
        call(
            &mut cpu,
            &mut k,
            egl_destroy_surface,
            &[DISPLAY_HANDLE, SURFACE_HANDLE]
        ),
        EGL_TRUE
    );
    call(&mut cpu, &mut k, egl_release_thread, &[]);
    let replacement = call(
        &mut cpu,
        &mut k,
        egl_create_window_surface,
        &[DISPLAY_HANDLE, CONFIG_HANDLE, 0x1234, 0],
    );
    assert_ne!(replacement, 0);
    assert_ne!(replacement, SURFACE_HANDLE);
    assert_eq!(
        call(
            &mut cpu,
            &mut k,
            egl_query_surface,
            &[DISPLAY_HANDLE, SURFACE_HANDLE, EGL_WIDTH, SCRATCH]
        ),
        EGL_FALSE
    );
    assert_eq!(call(&mut cpu, &mut k, egl_get_error, &[]), EGL_BAD_SURFACE);
    bind(&mut cpu, &mut k, CONTEXT_HANDLE, replacement);
}

#[test]
fn host_render_policies_apply_to_parked_contexts_and_mipmap_without_texture_is_legal() {
    let _g = guard();
    reset_for_test(8, 8);
    let (mut cpu, mut k) = (fresh_cpu(), fresh_kernel());
    let b = create(&mut cpu, &mut k, 0);
    set_gizmondo_goforce_driver(true);
    set_intel2700g_depth_bias(true);
    set_internal_resolution(2);
    bind(&mut cpu, &mut k, b, SURFACE_HANDLE);
    with_ctx(|c| {
        assert!(c.gizmondo_goforce_20051013);
        assert!(c.state.intel2700g_depth_bias);
    });
    let attrs = [EGL_WIDTH, 2, EGL_HEIGHT, 2, 0x3082, 1, EGL_NONE];
    cpu.write_mem(
        SCRATCH,
        &attrs
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let pbuffer = call(
        &mut cpu,
        &mut k,
        egl_create_pbuffer,
        &[DISPLAY_HANDLE, CONFIG_HANDLE, SCRATCH],
    );
    assert_ne!(pbuffer, 0);
    assert_eq!(
        call(
            &mut cpu,
            &mut k,
            egl_query_surface,
            &[DISPLAY_HANDLE, pbuffer, 0x3082, SCRATCH]
        ),
        EGL_TRUE
    );
    assert_eq!(cpu.read_u32_le(SCRATCH).unwrap(), 1);
}

#[test]
fn a_terminated_guest_thread_does_not_keep_exclusive_context_or_surface_ownership() {
    let _g = guard();
    reset_for_test(8, 8);
    let (mut cpu, mut k) = (fresh_cpu(), fresh_kernel());
    enter_thread(1);
    k.main_thread.exit_code = Some(0);
    bind(&mut cpu, &mut k, CONTEXT_HANDLE, SURFACE_HANDLE);
    assert_eq!(
        call(&mut cpu, &mut k, egl_get_current_context, &[]),
        CONTEXT_HANDLE
    );
    call(&mut cpu, &mut k, egl_release_thread, &[]);
    enter_thread(0);
}

#[test]
fn implicit_loader_thread_uses_sole_goforce_context_without_stealing_binding() {
    let _guard = guard();
    reset_for_test(2, 2);
    set_gizmondo_goforce_driver(true);
    enter_gl_thread(1);
    assert!(!needs_thread_switch(0));
    with_ctx(|c| texture(c, 17, [21, 43, 65, 255]));
    enter_thread(0);
    with_ctx(|c| assert_eq!(c.textures[&17].rgba, [21, 43, 65, 255]));
    SURFACES.with(|s| {
        let s = s.borrow();
        assert_eq!(s.contexts[&CONTEXT_HANDLE].owner, Some(0));
        assert!(!s.threads.contains_key(&1));
    });
    // Explicit EGL entry switches to the thread's own unbound state.
    enter_thread(1);
    assert_eq!(SURFACES.with(|s| s.borrow().current), 0);
    enter_thread(0);
    enter_gl_thread(1);
    assert_eq!(SURFACES.with(|s| s.borrow().current), 0);
    enter_thread(0);
    with_ctx(|c| assert_eq!(c.textures[&17].rgba, [21, 43, 65, 255]));
}

#[test]
fn implicit_loader_thread_keeps_standard_and_multicontext_isolation() {
    let _guard = guard();
    for (goforce, additional) in [(false, false), (true, true)] {
        reset_for_test(2, 2);
        set_gizmondo_goforce_driver(goforce);
        with_ctx(|c| texture(c, 17, [21, 43, 65, 255]));
        if additional {
            let mut cpu = fresh_cpu();
            let mut k = fresh_kernel();
            create(&mut cpu, &mut k, 0);
        }
        enter_gl_thread(1);
        assert_eq!(SURFACES.with(|s| s.borrow().current), 0);
        with_ctx(|c| assert!(!c.is_texture(17)));
        enter_thread(0);
        with_ctx(|c| assert_eq!(c.textures[&17].rgba, [21, 43, 65, 255]));
    }
}
