//! Retail frame-close policy over shared document, window and thread owners.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const HIDE: &str = "__PocketHLE_MFC_HideApplication";
pub(super) const POST_QUIT: &str = "__PocketHLE_MFC_PostQuitMessage";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        2086,
        "?PostNcDestroy@CFrameWnd@@MAAXXZ",
        super::window_lifetime::post_nc_delete,
    ),
    (1796, "?OnAppExit@CWinApp@@IAAXXZ", app_exit),
    (1867, "?OnDestroy@CFrameWnd@@IAAXXZ", on_destroy),
    (1826, "?OnClose@CFrameWnd@@IAAXXZ", close),
    (1610, "?HideApplication@CWinApp@@QAAXXZ", hide),
    (684, "?AfxPostQuitMessage@@YAXH@Z", post_quit),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", HIDE, hide);
    d.register_handler("coredll.dll", POST_QUIT, post_quit);
}
fn app_exit(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let app = f.arg(ctx, 0)?;
        let frame = word(ctx, app + 0x20)?;
        let hwnd = word(ctx, frame + 0x20)?;
        // Send WM_CLOSE through USER: document/derived-window vetoes must run.
        return f.call_service(ctx, 1, "SendMessageW", &[hwnd, 0x10, 0, 0]);
    }
    f.finish(ctx, 0)
}

fn post_quit(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    if f.fresh {
        let thread = super::application::thread_object(ctx)?;
        let terminate = if thread == 0 {
            0
        } else {
            word(ctx, thread + 0x58)?
        };
        if terminate != 0 {
            return f.call_args(ctx, 1, terminate, &[1, 1]);
        }
    }
    if f.fresh || f.phase(ctx)? == 1 {
        let code = f.arg(ctx, 0)?;
        return f.call_service(ctx, 2, "PostQuitMessage", &[code]);
    }
    if f.phase(ctx)? != 2 {
        return Err(KernelError::Dispatch(
            "Invalid MFC quit continuation".into(),
        ));
    }
    f.finish(ctx, 0)
}
fn hide(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let app = f.arg(ctx, 0)?;
    if f.fresh {
        let frame = word(ctx, app + 0x20)?;
        let hwnd = word(ctx, frame + 0x20)?;
        f.set(ctx, 0, hwnd)?;
        return f.call_service(ctx, 1, "ShowWindow", &[hwnd, 0]);
    }
    if f.phase(ctx)? == 1 {
        let hwnd = f.get(ctx, 0)?;
        return f.call_service(ctx, 2, "SetWindowPos", &[hwnd, 1, 0, 0, 0, 0, 0x13]);
    }
    if f.phase(ctx)? != 2 {
        return Err(KernelError::Dispatch(
            "Invalid MFC hide continuation".into(),
        ));
    }
    f.finish(ctx, 0)
}
fn active_document(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let object = f.arg(ctx, 0)?;
    f.call_virtual(ctx, 2, object, 47, &[])
}
fn application(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let module = super::application::state(ctx)?;
    let app = word(ctx, module + 4)?;
    let frame = f.arg(ctx, 0)?;
    if app == 0 || word(ctx, app + 0x20)? != frame {
        return document(ctx, f);
    }
    f.set(ctx, 1, app)?;
    if f.get(ctx, 0)? == 0 {
        return f.call_virtual(ctx, 4, app, 34, &[]);
    }
    f.call_service(ctx, 5, HIDE, &[app])
}
fn destroy(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let frame = f.arg(ctx, 0)?;
    f.call_virtual(ctx, 12, frame, 23, &[])
}
fn document(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let doc = f.get(ctx, 0)?;
    if doc == 0 || word(ctx, doc + 0x4c)? == 0 {
        return destroy(ctx, f);
    }
    f.call_virtual(ctx, 8, doc, 25, &[])
}
fn next_view(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let doc = f.get(ctx, 0)?;
    if word(ctx, f.address + 128)? == 0 {
        return f.call_virtual(ctx, 13, doc, 32, &[]);
    }
    let visits = f.get(ctx, 2)?;
    if visits >= 4096 {
        return Err(KernelError::Dispatch(
            "MFC frame-close view traversal exceeds its bound".into(),
        ));
    }
    f.set(ctx, 2, visits + 1)?;
    f.call_virtual(ctx, 9, doc, 26, &[f.address + 128])
}
fn close(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let object = f.arg(ctx, 0)?;
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    if f.fresh {
        let can_close = word(ctx, object + 0xb0)?;
        if can_close != 0 {
            return f.call_args(ctx, 1, can_close, &[object]);
        }
        return active_document(ctx, f);
    }
    match f.phase(ctx)? {
        1 if result == 0 => f.finish(ctx, 0),
        1 => active_document(ctx, f),
        2 => {
            f.set(ctx, 0, result)?;
            if result == 0 {
                application(ctx, f)
            } else {
                f.call_virtual(ctx, 3, result, 36, &[object])
            }
        }
        3 if result == 0 => f.finish(ctx, 0),
        3 => application(ctx, f),
        4 if result == 0 => f.finish(ctx, 0),
        4 => {
            let app = f.get(ctx, 1)?;
            f.call_service(ctx, 5, HIDE, &[app])
        }
        5 => {
            let app = f.get(ctx, 1)?;
            f.call_service(ctx, 6, super::document_manager::APP_CLOSE_ALL, &[app, 0])
        }
        6 => {
            let module = super::application::state(ctx)?;
            // Existing module object count/user-control fields implement this
            // close decision; this does not fabricate an OLE runtime or object.
            if word(ctx, module + 0x34)? != 0 {
                put(ctx, module + 0x38, 0)?;
                return f.finish(ctx, 0);
            }
            let app = f.get(ctx, 1)?;
            if ctx.cpu.read_mem(module + 0x14, 1)?[0] == 0 && word(ctx, app + 0x20)? == 0 {
                return f.call_service(ctx, 7, POST_QUIT, &[0]);
            }
            document(ctx, f)
        }
        7 | 12 | 13 => f.finish(ctx, 0),
        8 => {
            put(ctx, f.address + 128, result)?;
            next_view(ctx, f)
        }
        9 => f.call_service(ctx, 10, super::hierarchy::PARENT, &[result]),
        10 if result == object => next_view(ctx, f),
        10 => {
            let doc = f.get(ctx, 0)?;
            f.call_virtual(ctx, 11, doc, 38, &[object])
        }
        11 => destroy(ctx, f),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC frame-close continuation".into(),
        )),
    }
}

fn done_and_base(ctx: &mut CallCtx<'_>, f: Frame) -> Result<DispatchOutcome, KernelError> {
    let frame = f.arg(ctx, 0)?;
    if word(ctx, frame + 0xc8)? != 0 {
        let hwnd = word(ctx, frame + 0x20)?;
        return f.call_service(ctx, 4, super::done_button::SHELL, &[hwnd, 2]);
    }
    f.call_service(ctx, 5, super::window_lifetime::ON_DESTROY, &[frame])
}
fn on_destroy(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let frame = f.arg(ctx, 0)?;
    if f.fresh {
        return f.call_service(ctx, 1, super::control_bars::DESTROY_BARS, &[frame]);
    }
    match f.phase(ctx)? {
        1 => {
            if word(ctx, frame + 0x5c)? == 0 {
                return done_and_base(ctx, f);
            }
            let hwnd = word(ctx, frame + 0x20)?;
            f.call_service(ctx, 2, super::frame_loading::GET_MENU, &[hwnd])
        }
        2 => {
            let menu = ctx.cpu.read_reg(ArmReg::R0)?;
            let default = word(ctx, frame + 0x5c)?;
            if menu == default {
                return done_and_base(ctx, f);
            }
            let hwnd = word(ctx, frame + 0x20)?;
            f.call_service(ctx, 3, super::frame_loading::SET_MENU, &[hwnd, default])
        }
        3 => done_and_base(ctx, f),
        4 => {
            let owner = word(ctx, frame + 0xc8)?;
            if owner != 0 {
                put(ctx, owner + 0x44, 0)?;
            }
            put(ctx, frame + 0xc8, 0)?;
            f.call_service(ctx, 5, super::window_lifetime::ON_DESTROY, &[frame])
        }
        5 => f.finish(ctx, 0),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC frame-destroy continuation".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        archive::tests::Fixture,
        window_dispatch::tests::{drive_with, entry},
    };
    use pocket_cpu::{regs::ArmReg, Cpu};
    fn table(f: &mut Fixture, object: u32, copy: u32, slots: &[(u32, u32)]) {
        let v = f.word(object);
        let bytes = f.cpu.read_mem(v, 56 * 4).unwrap();
        f.cpu.write_mem(copy, &bytes).unwrap();
        for &(slot, target) in slots {
            f.cpu
                .write_mem(copy + slot * 4, &target.to_le_bytes())
                .unwrap();
        }
        f.cpu.write_mem(object, &copy.to_le_bytes()).unwrap();
    }
    #[test]
    fn frame_close_honors_callback_and_document_vetoes_and_automatic_document_lifetime() {
        for (hook, allowed, doc, auto_delete) in [
            (true, false, false, false),
            (false, false, true, false),
            (false, true, false, false),
            (false, true, true, false),
            (false, true, true, true),
        ] {
            let mut f = Fixture::new(&[]);
            f.call("ord:103", &[0x2400]);
            f.call("ord:85", &[0x3000]);
            f.call("ord:644", &[]);
            table(&mut f, 0x2400, 0x3800, &[(47, 0x6000), (23, 0x6100)]);
            table(
                &mut f,
                0x3000,
                0x3c00,
                &[(36, 0x6200), (25, 0x6300), (32, 0x6400)],
            );
            f.cpu
                .write_mem(0x304c, &u32::from(auto_delete).to_le_bytes())
                .unwrap();
            if hook {
                f.cpu.write_mem(0x24b0, &0x6500u32.to_le_bytes()).unwrap();
            }
            let baseline = f.kernel.heap.free_bytes();
            for _ in 0..16 {
                f.cpu.write_arg(0, 0x2400).unwrap();
                let mut visited = Vec::new();
                let value = drive_with(
                    &mut f.cpu,
                    &mut f.kernel,
                    &mut f.dispatcher,
                    entry(1826),
                    256,
                    |pc, cpu, _, _| {
                        visited.push(pc);
                        match pc {
                            0x6500 => 0,
                            0x6000 => {
                                if doc {
                                    0x3000
                                } else {
                                    0
                                }
                            }
                            0x6200 => {
                                assert_eq!(cpu.read_arg(1).unwrap(), 0x2400);
                                u32::from(allowed)
                            }
                            0x6300 | 0x6400 => 0,
                            0x6100 => {
                                assert_eq!(cpu.read_arg(0).unwrap(), 0x2400);
                                1
                            }
                            _ => panic!("unexpected close callback {pc:x}"),
                        }
                    },
                );
                assert_eq!(value, 0);
                let expected = if hook {
                    vec![0x6500]
                } else if !doc {
                    vec![0x6000, 0x6100]
                } else if !allowed {
                    vec![0x6000, 0x6200]
                } else if auto_delete {
                    vec![0x6000, 0x6200, 0x6300, 0x6400]
                } else {
                    vec![0x6000, 0x6200, 0x6100]
                };
                assert_eq!(visited, expected);
                assert_eq!(f.kernel.heap.free_bytes(), baseline);
                assert_eq!(f.cpu.read_reg(ArmReg::Sp).unwrap(), 0x4000);
            }
        }
    }
    #[test]
    fn application_close_veto_preserves_visibility_and_live_object_count_retires_user_control() {
        for (allow, objects) in [(false, 0u32), (true, 0), (true, 1)] {
            let mut f = Fixture::new(&[]);
            f.call("ord:201", &[0x2600, 0]);
            f.call("ord:103", &[0x2400]);
            let module = f.call("ord:644", &[]);
            f.cpu.write_mem(0x2620, &0x2400u32.to_le_bytes()).unwrap();
            f.cpu
                .write_mem(0x2420, &0xdead1000u32.to_le_bytes())
                .unwrap();
            f.cpu
                .write_mem(module + 0x34, &objects.to_le_bytes())
                .unwrap();
            f.kernel.window_classes.insert(0xdead1000, "FRAME".into());
            f.kernel.window_styles.insert(0xdead1000, (0x10000000, 0));
            f.kernel.window_rects.insert(0xdead1000, (4, 8, 100, 200));
            f.kernel.window_visibility.insert(0xdead1000, true);
            table(&mut f, 0x2400, 0x3800, &[(23, 0x6100)]);
            table(&mut f, 0x2600, 0x3c00, &[(34, 0x6000)]);
            f.cpu.write_arg(0, 0x2400).unwrap();
            let mut destroyed = 0;
            drive_with(
                &mut f.cpu,
                &mut f.kernel,
                &mut f.dispatcher,
                entry(1826),
                256,
                |pc, _, _, _| match pc {
                    0x6000 => u32::from(allow),
                    0x6100 => {
                        destroyed += 1;
                        1
                    }
                    _ => panic!("unexpected {pc:x}"),
                },
            );
            assert_eq!(f.kernel.window_visibility[&0xdead1000], !allow);
            assert_eq!(destroyed, u32::from(allow && objects == 0));
            assert_eq!(f.kernel.window_rects[&0xdead1000], (4, 8, 100, 200));
            assert_eq!(f.word(module + 0x38), u32::from(!allow || objects == 0));
        }
    }
    #[test]
    fn quit_calls_the_current_threads_termination_hook_before_posting_the_exit_code() {
        let mut f = Fixture::new(&[]);
        f.call("ord:201", &[0x2600, 0]);
        f.cpu.write_mem(0x2658, &0x6000u32.to_le_bytes()).unwrap();
        f.cpu.write_arg(0, 17).unwrap();
        let mut calls = 0;
        let value = drive_with(
            &mut f.cpu,
            &mut f.kernel,
            &mut f.dispatcher,
            entry(684),
            128,
            |pc, cpu, kernel, _| {
                assert_eq!(pc, 0x6000);
                assert_eq!(cpu.read_arg(0).unwrap(), 1);
                assert_eq!(cpu.read_arg(1).unwrap(), 1);
                assert_eq!(kernel.process_exit_code, None);
                calls += 1;
                0
            },
        );
        assert_eq!(calls, 1);
        assert_eq!(value, 0);
        assert!(f.kernel.posted_messages.contains(&(0, 0x12, 17, 0)));
    }
}

#[cfg(test)]
mod destroy_tests {
    use super::super::archive::tests::Fixture;
    use pocket_cpu::Cpu;
    #[test]
    fn frame_destruction_restores_the_borrowed_default_menu_and_retires_the_done_owner() {
        let mut f = Fixture::new(&[]);
        f.call("ord:103", &[0x2400]);
        f.call("ord:206", &[0x3000]);
        f.call("ord:651", &[]);
        f.kernel.window_classes.insert(7, "FRAME".into());
        f.kernel.window_styles.insert(7, (0x10000000, 0));
        assert_eq!(f.call("ord:804", &[0x2400, 7]), 1);
        let mut baseline = None;
        for _ in 0..32 {
            for (at, value) in [
                (0x245c, 0x1234u32),
                (0x2424, 0x5678),
                (0x24c8, 0x3000),
                (0x3044, 1),
            ] {
                f.cpu.write_mem(at, &value.to_le_bytes()).unwrap();
            }
            f.kernel.window_styles.get_mut(&7).unwrap().1 |= 0x80000000;
            f.call("ord:1867", &[0x2400]);
            assert_eq!(f.word(0x2424), 0x1234);
            assert_eq!(f.word(0x24c8), 0);
            assert_eq!(f.word(0x3044), 0);
            assert_eq!(f.kernel.window_styles[&7].1 & 0x80000000, 0);
            let free = f.kernel.heap.free_bytes();
            assert_eq!(*baseline.get_or_insert(free), free);
            assert!(f.kernel.window_classes.contains_key(&7));
        }
    }
}

#[cfg(test)]
mod app_exit_tests {
    use super::super::archive::tests::Fixture;
    use super::super::window_dispatch::tests::{drive_with, entry};
    use pocket_cpu::Cpu;
    #[test]
    fn application_exit_sends_wm_close_to_the_main_window_and_preserves_a_veto() {
        let mut f = Fixture::new(&[]);
        f.call("ord:201", &[0x2400, 0]);
        f.call("ord:103", &[0x3000]);
        f.cpu.write_mem(0x2420, &0x3000u32.to_le_bytes()).unwrap();
        f.cpu.write_mem(0x3020, &7u32.to_le_bytes()).unwrap();
        f.kernel.window_classes.insert(7, "FRAME".into());
        f.kernel.window_procs.insert(7, 0x6100);
        let mut count = 0;
        f.cpu.write_arg(0, 0x2400).unwrap();
        drive_with(
            &mut f.cpu,
            &mut f.kernel,
            &mut f.dispatcher,
            entry(1796),
            256,
            |pc, cpu, _, _| {
                assert_eq!(pc, 0x6100);
                assert_eq!(cpu.read_arg(0).unwrap(), 7);
                assert_eq!(cpu.read_arg(1).unwrap(), 0x10);
                assert_eq!(cpu.read_arg(2).unwrap(), 0);
                assert_eq!(cpu.read_arg(3).unwrap(), 0);
                count += 1;
                0
            },
        );
        assert_eq!(count, 1);
        assert!(f.kernel.window_classes.contains_key(&7));
        assert!(f.kernel.posted_messages.iter().all(|m| m.1 != 0x12));
        assert!(super::super::handler_named("?OnAppExit@CWinApp@@IAAXXZ").is_some());
    }
}
