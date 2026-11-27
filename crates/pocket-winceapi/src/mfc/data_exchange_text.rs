//! CString DDX uses the existing EDIT HWND and CString copy-on-write owner.
use super::callbacks::Frame;
use super::*;
use pocket_cpu::regs::ArmReg;
pub(super) const GET_LIST: &str = "__PocketHLE_MFC_GetListCString";
pub(super) const GET_TEXT: &str = "__PocketHLE_MFC_GetWindowCString";
const SET_TEXT: &str = "__PocketHLE_MFC_SetWindowText";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        1006,
        "?DDX_LBString@@YAXPAVCDataExchange@@HAAVCString@@@Z",
        list_exchange,
    ),
    (
        1019,
        "?DDX_Text@@YAXPAVCDataExchange@@HAAVCString@@@Z",
        exchange,
    ),
    (695, "?AfxSetWindowText@@YAXPAUHWND__@@PBG@Z", set_text),
];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", SET_TEXT, set_text);
    d.register_handler("coredll.dll", GET_TEXT, get_text);
    d.register_handler("coredll.dll", GET_LIST, get_list);
}
fn exchange(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    exchange_impl(ctx, false)
}
fn list_exchange(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    exchange_impl(ctx, true)
}
fn exchange_impl(ctx: &mut CallCtx<'_>, list: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let dx = f.arg(ctx, 0)?;
    let value = f.arg(ctx, 2)?;
    if f.fresh {
        let save = word(ctx, dx)?;
        if save != 0 {
            super::strings::validate_assignment_target(ctx, value)?;
        } else {
            super::strings::value(ctx, value)?;
        }
        f.set(ctx, 0, save)?;
        let id = f.arg(ctx, 1)?;
        let prepare = if list {
            super::data_exchange::PREPARE
        } else {
            super::data_exchange::PREPARE_EDIT
        };
        return f.call_service(ctx, 1, prepare, &[dx, id]);
    }
    let result = ctx.cpu.read_reg(ArmReg::R0)?;
    match f.phase(ctx)? {
        1 => {
            f.set(ctx, 1, result)?;
            if f.get(ctx, 0)? != 0 {
                if list {
                    return f.call_service(ctx, 5, "SendMessageW", &[result, 0x188, 0, 0]);
                }
                f.call_service(ctx, 7, GET_TEXT, &[result, value])
            } else {
                let text = word(ctx, value)?;
                if list {
                    return f.call_service(
                        ctx,
                        4,
                        "SendMessageW",
                        &[result, 0x18c, u32::MAX, text],
                    );
                }
                f.call_service(ctx, 4, SET_TEXT, &[result, text])
            }
        }
        3 => finish_value(ctx, f, value),
        4 | 7 => f.finish(ctx, 0),
        5 if result == u32::MAX => {
            ctx.cpu.write_arg(0, value)?;
            super::strings::empty(ctx)?;
            finish_value(ctx, f, value)
        }
        5 => {
            f.set(ctx, 2, result)?;
            let hwnd = f.get(ctx, 1)?;
            f.call_service(ctx, 6, "SendMessageW", &[hwnd, 0x18a, result, 0])
        }
        6 => {
            let buffer = super::strings::reserve(ctx, value, result)?;
            let hwnd = f.get(ctx, 1)?;
            let index = f.get(ctx, 2)?;
            f.call_service(ctx, 3, "SendMessageW", &[hwnd, 0x189, index, buffer])
        }
        _ => Err(KernelError::Dispatch(
            "Invalid MFC text exchange continuation".into(),
        )),
    }
}
fn get_text(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    get_string(ctx, false)
}
fn get_list(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    get_string(ctx, true)
}
fn get_string(ctx: &mut CallCtx<'_>, list: bool) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let hwnd = f.arg(ctx, 0)?;
    let value = f.arg(ctx, if list { 2 } else { 1 })?;
    let index = if list { f.arg(ctx, 1)? } else { 0 };
    if f.fresh {
        super::strings::validate_assignment_target(ctx, value)?;
        if list {
            return f.call_service(ctx, 1, "SendMessageW", &[hwnd, 0x18a, index, 0]);
        }
        return f.call_service(ctx, 1, "GetWindowTextLengthW", &[hwnd]);
    }
    match f.phase(ctx)? {
        1 => {
            let length = ctx.cpu.read_reg(ArmReg::R0)?;
            let buffer = super::strings::reserve(ctx, value, length)?;
            if list {
                f.call_service(ctx, 2, "SendMessageW", &[hwnd, 0x189, index, buffer])
            } else {
                f.call_service(ctx, 2, "GetWindowTextW", &[hwnd, buffer, length + 1])
            }
        }
        2 => finish_value(ctx, f, value),
        _ => Err(KernelError::Dispatch(
            "Invalid window CString continuation".into(),
        )),
    }
}

fn finish_value(
    ctx: &mut CallCtx<'_>,
    f: Frame,
    value: u32,
) -> Result<DispatchOutcome, KernelError> {
    ctx.cpu.write_arg(0, value)?;
    ctx.cpu.write_arg(1, u32::MAX)?;
    super::strings::release_buffer(ctx)?;
    f.finish(ctx, 0)
}

fn set_text(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let hwnd = f.arg(ctx, 0)?;
    let pointer = f.arg(ctx, 1)?;
    if f.fresh {
        let value = super::strings::wide(ctx, pointer)?;
        // Match the native helper: bounded comparison avoids redundant text
        // notifications for short unchanged labels; longer strings are sent.
        if value.len() <= 256 {
            let buffer = alloc(ctx, 512)?;
            if let Err(error) = f.own(ctx, buffer) {
                ctx.kernel.heap.free(buffer);
                return Err(error);
            }
            f.set(ctx, 0, buffer)?;
            return f.call_service(ctx, 1, "GetWindowTextW", &[hwnd, buffer, 256]);
        }
        return f.call_service(ctx, 2, "SetWindowTextW", &[hwnd, pointer]);
    }
    if f.phase(ctx)? == 1 {
        let buffer = f.get(ctx, 0)?;
        if super::strings::wide(ctx, buffer)? == super::strings::wide(ctx, pointer)? {
            return f.finish(ctx, 0);
        }
        return f.call_service(ctx, 2, "SetWindowTextW", &[hwnd, pointer]);
    }
    f.finish(ctx, 0)
}

#[cfg(test)]
mod tests {
    use super::super::archive::tests::Fixture;
    use super::super::window_dispatch::tests::entry;
    use pocket_cpu::{Cpu, Prot};
    use pocket_kernel::controls::ControlClass;
    use pocket_kernel::Dispatcher;
    fn setup() -> (Fixture, u32) {
        let mut f = Fixture::new(&[]);
        f.kernel.window_classes.insert(7, "Window".into());
        f.call("ord:206", &[0x2400]);
        f.call("ord:804", &[0x2400, 7]);
        f.call("ord:75", &[0x2600, 0x2400, 0]);
        let h = f.kernel.controls.create(
            7,
            ControlClass::Edit,
            10,
            String::new(),
            0x50000004,
            0,
            0,
            120,
            80,
        );
        // Controls created directly by the fixture still need the USER window
        // procedure installed by CreateWindowEx in a real application.
        let base = f.kernel.dynamic_exports[&1]["DefWindowProcW"];
        f.kernel.window_classes.insert(h, "EDIT".into());
        f.kernel.window_procs.insert(h, base);
        (f, h)
    }
    fn wide(f: &mut Fixture, text: &str) {
        let bytes: Vec<_> = text
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        f.cpu.write_mem(0x1000, &bytes).unwrap();
    }
    fn text(f: &mut Fixture, cell: u32) -> Vec<u16> {
        let p = f.word(cell);
        let length = f.word(p - 8);
        f.cpu
            .read_mem(p, length * 2)
            .unwrap()
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
            .collect()
    }
    #[test]
    fn unicode_text_exchange_uses_cstring_copy_on_write_and_releases_all_callback_temporaries() {
        let (mut f, h) = setup();
        let mut baseline = None;
        for i in 0..32 {
            let original = if i % 2 == 0 {
                "Unicode é 😀\r\nnext".into()
            } else {
                "w".repeat(300)
            };
            wide(&mut f, &original);
            f.call(
                "ord:180",
                &[0x2800, 0x1000, original.encode_utf16().count() as u32],
            );
            f.call("ord:175", &[0x2900, 0x2800]);
            f.cpu.write_mem(0x2600, &0u32.to_le_bytes()).unwrap();
            f.call("ord:1019", &[0x2600, 10, 0x2800]);
            assert_eq!(f.kernel.controls.get(h).unwrap().text, original);
            f.call("ord:1019", &[0x2600, 10, 0x2800]);
            assert_eq!(f.word(0x2608), h);
            assert_eq!(f.word(0x260c), 1);
            let updated = format!("Changed {i} 😀");
            f.kernel.controls.get_mut(h).unwrap().text = updated.clone();
            f.cpu.write_mem(0x2600, &1u32.to_le_bytes()).unwrap();
            f.call("ord:1019", &[0x2600, 10, 0x2800]);
            assert_eq!(
                text(&mut f, 0x2800),
                updated.encode_utf16().collect::<Vec<_>>()
            );
            assert_eq!(
                text(&mut f, 0x2900),
                original.encode_utf16().collect::<Vec<_>>()
            );
            f.call("ord:297", &[0x2800]);
            f.call("ord:297", &[0x2900]);
            let free = f.kernel.heap.free_bytes();
            assert_eq!(*baseline.get_or_insert(free), free);
            assert!(!f.kernel.dynamic_exports[&1]
                .keys()
                .any(|key| key.starts_with("__PocketHLE_MFC_Frame:")));
        }
    }
    #[test]
    fn list_string_exchange_selects_by_prefix_and_reads_the_real_selection_or_empty_state() {
        let (mut f, h) = setup();
        let control = f.kernel.controls.get_mut(h).unwrap();
        control.class = ControlClass::ListBox;
        control.list_items = vec![("Alpha".into(), 0), ("Beta 😀".into(), 0)];
        let mut baseline = None;
        for _ in 0..32 {
            wide(&mut f, "Beta");
            f.call("ord:180", &[0x2800, 0x1000, 4]);
            f.call("ord:175", &[0x2900, 0x2800]);
            f.cpu.write_mem(0x2600, &0u32.to_le_bytes()).unwrap();
            f.call("ord:1006", &[0x2600, 10, 0x2800]);
            assert_eq!(f.kernel.controls.get(h).unwrap().list_selected, Some(1));
            assert_eq!(f.word(0x260c), 0);
            f.cpu.write_mem(0x2600, &1u32.to_le_bytes()).unwrap();
            f.call("ord:1006", &[0x2600, 10, 0x2800]);
            assert_eq!(
                text(&mut f, 0x2800),
                "Beta 😀".encode_utf16().collect::<Vec<_>>()
            );
            f.kernel.controls.get_mut(h).unwrap().list_selected = None;
            f.call("ord:1006", &[0x2600, 10, 0x2800]);
            assert!(text(&mut f, 0x2800).is_empty());
            assert_eq!(
                text(&mut f, 0x2900),
                "Beta".encode_utf16().collect::<Vec<_>>()
            );
            f.call("ord:297", &[0x2800]);
            f.call("ord:297", &[0x2900]);
            let free = f.kernel.heap.free_bytes();
            assert_eq!(*baseline.get_or_insert(free), free);
        }
    }
    #[test]
    fn protected_save_destination_fails_before_preparing_or_mutating_the_edit() {
        let (mut f, h) = setup();
        f.call("ord:181", &[0x2800]);
        f.cpu.write_mem(0x2600, &1u32.to_le_bytes()).unwrap();
        f.cpu.write_mem(0x2608, &0xfeedu32.to_le_bytes()).unwrap();
        f.kernel.controls.get_mut(h).unwrap().text = "unchanged".into();
        f.cpu.protect_region(0x2000, 0x1000, Prot::READ).unwrap();
        for (i, value) in [0x2600, 10, 0x2800].into_iter().enumerate() {
            f.cpu.write_arg(i as u8, value).unwrap();
        }
        assert!(f
            .dispatcher
            .dispatch(&mut f.cpu, &entry(1019), &mut f.kernel)
            .is_err());
        assert_eq!(f.word(0x2608), 0xfeed);
        assert_eq!(f.kernel.controls.get(h).unwrap().text, "unchanged");
    }
}
