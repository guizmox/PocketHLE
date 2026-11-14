//! CE console IOCTLs and CRT routing used by the unmodified CE6 Pocket CMD.
use super::*;
use pocket_kernel::console::{ECHO_INPUT, LINE_INPUT, PROCESSED_OUTPUT};

pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", "SetStdioPathW", set_stdio_path);
    for name in ["fgetwc", "getwc"] {
        d.register_handler("coredll.dll", name, get_wide_character);
    }
    d.register_handler("coredll.dll", "getwchar", get_wchar);
}

pub(super) fn standard_id(stream: u32) -> Option<usize> {
    stream
        .checked_sub(STD_FILE_BASE)
        .filter(|id| *id < 3)
        .map(|id| id as usize)
}

/// FILE identities stay stable across freopen. Open descriptions (including
/// file offsets) are separately inherited by the child process context.
pub(super) fn stream_handle(ctx: &mut CallCtx<'_>, stream: u32) -> Option<u32> {
    let Some(id) = standard_id(stream) else {
        return Some(stream);
    };
    if ctx.kernel.console.closed[id] {
        return None;
    }
    if let Some(handle) = ctx.kernel.console.streams[id] {
        return Some(handle);
    }
    let path = ctx.kernel.console.paths[id].clone();
    let access = if id == 0 { 1 } else { 2 };
    let disposition = if id == 0 || is_console_path(&path) || is_null_path(&path) {
        3
    } else {
        2
    };
    match ctx
        .kernel
        .vfs
        .open_file(&path, access, 3, disposition, false, 0)
    {
        Ok(open) => {
            ctx.kernel.console.streams[id] = Some(open.handle);
            if ctx.kernel.vfs.stdio_text_modes[id] {
                ctx.kernel.vfs.mark_text_mode(open.handle);
            }
            if ctx.kernel.vfs.console_open(open.handle).is_some() && ctx.kernel.ce.version.is_some()
            {
                ctx.kernel.ensure_console_window();
            }
            Some(open.handle)
        }
        Err(error) => {
            set_thread_error(ctx, error);
            ctx.kernel.crt_stream_errors.insert(stream);
            None
        }
    }
}

fn is_console_path(path: &str) -> bool {
    matches!(
        path.replace('\\', "/")
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "con:" | "con0:" | "con1:"
    )
}
fn is_null_path(path: &str) -> bool {
    matches!(
        path.replace('\\', "/")
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "nul:" | "nul"
    )
}

pub(super) fn file_handle(ctx: &mut CallCtx<'_>, handle: u32) -> Option<u32> {
    stream_handle(
        ctx,
        if handle < 3 {
            STD_FILE_BASE + handle
        } else {
            handle
        },
    )
}

pub(super) fn stream_is_open(kernel: &KernelState, handle: u32) -> bool {
    kernel.vfs.is_open(handle)
        || kernel.vfs.console_open(handle).is_some()
        || kernel.vfs.is_null(handle)
}

pub(super) fn reopen(ctx: &mut CallCtx<'_>, path: &str, mode: &str, stream: u32) -> Option<u32> {
    let id = standard_id(stream)?;
    let handle = open_cstr_path(ctx, path, mode);
    if handle == 0 {
        return Some(0);
    }
    if let Some(old) = ctx.kernel.console.streams[id].replace(handle) {
        close_owned_handle(ctx.kernel, old);
    }
    ctx.kernel.console.paths[id] = path.into();
    ctx.kernel.console.closed[id] = false;
    ctx.kernel.vfs.stdio_text_modes[id] = !mode.contains('b');
    ctx.kernel.crt_stream_errors.remove(&stream);
    if ctx.kernel.vfs.console_open(handle).is_some() && ctx.kernel.ce.version.is_some() {
        ctx.kernel.ensure_console_window();
    }
    Some(stream)
}

fn set_stdio_path(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let id = ctx.arg_u32(0)?;
    let pointer = ctx.arg_u32(1)?;
    if id > 2 || pointer == 0 {
        return fail(ctx, 87);
    }
    let units = read_wstr(ctx, pointer, 260)?;
    if units.is_empty() || units.len() >= 260 {
        return fail(ctx, 87);
    }
    if let Some(old) = ctx.kernel.console.streams[id as usize].take() {
        close_owned_handle(ctx.kernel, old);
    }
    ctx.kernel.console.paths[id as usize] = String::from_utf16_lossy(&units);
    ctx.kernel.console.closed[id as usize] = false;
    ctx.kernel.crt_stream_errors.remove(&(STD_FILE_BASE + id));
    Ok(DispatchOutcome::ReturnedR0(1))
}

fn fail(ctx: &mut CallCtx<'_>, error: u32) -> Result<DispatchOutcome, KernelError> {
    set_thread_error(ctx, error);
    Ok(DispatchOutcome::ReturnedR0(0))
}

pub(super) fn read_bytes(ctx: &mut CallCtx<'_>, handle: u32, bytes: &mut [u8]) -> Option<usize> {
    let console = ctx.kernel.vfs.console_open(handle)?;
    if console.access & 1 == 0 {
        return None;
    }
    let units = console
        .device
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .read_utf16(bytes.len(), false)?;
    let data = crate::crt_ansi::encode(&String::from_utf16_lossy(&units));
    let n = data.len().min(bytes.len());
    bytes[..n].copy_from_slice(&data[..n]);
    Some(n)
}

pub(super) fn write_bytes(ctx: &mut CallCtx<'_>, handle: u32, bytes: &[u8]) -> Option<usize> {
    let console = ctx.kernel.vfs.console_open(handle)?;
    if console.access & 2 == 0 {
        return None;
    }
    console
        .device
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .write_text(&crate::crt_ansi::decode(bytes));
    if ctx.kernel.ce.version.is_some() {
        ctx.kernel.ensure_console_window();
    }
    Some(bytes.len())
}

pub(super) fn write_text(ctx: &mut CallCtx<'_>, stream: u32, text: &str, _wide: bool) -> bool {
    let Some(handle) = stream_handle(ctx, stream) else {
        return false;
    };
    if let Some(label) = std_file_label(stream) {
        log::debug!("guest {label}: {}", text.trim_end_matches(['\r', '\n']));
    }
    if let Some(console) = ctx.kernel.vfs.console_open(handle) {
        if console.access & 2 == 0 {
            ctx.kernel.crt_stream_errors.insert(stream);
            return false;
        }
        console
            .device
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .write_text(text);
        if ctx.kernel.ce.version.is_some() {
            ctx.kernel.ensure_console_window();
        }
        return true;
    }
    // Wide CRT output to a file still uses the device's ANSI code page.
    // Keep it reversible through fgetws and CMD's TYPE/batch-file readers.
    let bytes = crate::crt_ansi::encode(text);
    write_stdio_raw(ctx, handle, &bytes) == Some(bytes.len())
}

/// A pending stdin read parks at the original call, and the idle trampoline
/// sleeps only after releasing the shared process-domain execution gate.
pub(super) fn wait_input(
    ctx: &mut CallCtx<'_>,
    handle: u32,
) -> Result<Option<DispatchOutcome>, KernelError> {
    let Some(console) = ctx.kernel.vfs.console_open(handle) else {
        return Ok(None);
    };
    ctx.kernel.ensure_console_window();
    if console.access & 1 == 0
        || console
            .device
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .input_available()
    {
        return Ok(None);
    }
    if let Some(index) = ctx.kernel.current_thread.checked_sub(1) {
        ctx.kernel.threads[index].sleep_until_ms = monotonic_ms().saturating_add(4);
        if let Some(outcome) = park_worker_and_reevaluate(ctx)? {
            return Ok(Some(outcome));
        }
    }
    if let Some(outcome) = resume_worker_reenter(ctx)? {
        return Ok(Some(outcome));
    }
    let mut regs = read_guest_regs(ctx.cpu)?;
    regs[15] = ctx.thunk.thunk_va;
    ctx.kernel.main_thread.saved_regs = Some(regs);
    Ok(Some(DispatchOutcome::JumpTo(
        pocket_kernel::THREAD_SCHEDULER_IDLE_VA,
    )))
}

pub(super) fn read_wide_line(
    ctx: &mut CallCtx<'_>,
    stream: u32,
    output: u32,
    capacity: u32,
) -> Result<Option<DispatchOutcome>, KernelError> {
    let Some(handle) = stream_handle(ctx, stream) else {
        return Ok(Some(DispatchOutcome::ReturnedR0(0)));
    };
    let Some(console) = ctx.kernel.vfs.console_open(handle) else {
        return Ok(None);
    };
    if output == 0
        || capacity <= 1
        || capacity > 0x100000
        || ctx
            .cpu
            .check_guest_access(output, capacity * 2, Prot::WRITE)
            .is_err()
    {
        ctx.kernel.crt_stream_errors.insert(stream);
        return Ok(Some(DispatchOutcome::ReturnedR0(0)));
    }
    if console.access & 1 == 0 {
        ctx.kernel.crt_stream_errors.insert(stream);
        return Ok(Some(DispatchOutcome::ReturnedR0(0)));
    }
    if let Some(outcome) = wait_input(ctx, handle)? {
        return Ok(Some(outcome));
    }
    let mut units = console
        .device
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .read_utf16(capacity as usize - 1, true)
        .unwrap_or_default();
    if units.is_empty() {
        return Ok(Some(DispatchOutcome::ReturnedR0(0)));
    }
    units.push(0);
    ctx.cpu.write_mem(output, &wide_to_bytes(&units))?;
    Ok(Some(DispatchOutcome::ReturnedR0(output)))
}

fn get_wide_character(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let stream = ctx.arg_u32(0)?;
    wide_character(ctx, stream)
}
fn get_wchar(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    wide_character(ctx, STD_FILE_BASE)
}
fn wide_character(ctx: &mut CallCtx<'_>, stream: u32) -> Result<DispatchOutcome, KernelError> {
    let Some(handle) = stream_handle(ctx, stream) else {
        return Ok(DispatchOutcome::ReturnedR0(u32::MAX));
    };
    if let Some(console) = ctx.kernel.vfs.console_open(handle) {
        if console.access & 1 == 0 {
            return Ok(DispatchOutcome::ReturnedR0(u32::MAX));
        }
        if let Some(outcome) = wait_input(ctx, handle)? {
            return Ok(outcome);
        }
        let value = console
            .device
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .read_utf16(1, false)
            .and_then(|v| v.first().copied())
            .map_or(u32::MAX, u32::from);
        return Ok(DispatchOutcome::ReturnedR0(value));
    }
    let mut byte = [0];
    let count = read_stdio_bytes(ctx, handle, &mut byte);
    Ok(DispatchOutcome::ReturnedR0(if count == 0 {
        u32::MAX
    } else {
        crate::crt_ansi::decode(&byte)
            .encode_utf16()
            .next()
            .map_or(u32::MAX, u32::from)
    }))
}

pub(super) fn control(ctx: &mut CallCtx<'_>) -> Result<Option<DispatchOutcome>, KernelError> {
    let original = ctx.arg_u32(0)?;
    let code = ctx.arg_u32(1)?;
    if code & 0xffff_0000 != 0x0102_0000 && ctx.kernel.vfs.console_open(original).is_none() {
        return Ok(None);
    }
    let Some(handle) = file_handle(ctx, original) else {
        return fail(ctx, 6).map(Some);
    };
    let Some(console) = ctx.kernel.vfs.console_open(handle) else {
        return if original < 3 {
            fail(ctx, 1).map(Some)
        } else {
            Ok(None)
        };
    };
    if ctx.kernel.object_handles.is_closed(handle) {
        return fail(ctx, 6).map(Some);
    }
    let (input, input_len, output, output_len, returned) = (
        ctx.arg_u32(2)?,
        ctx.arg_u32(3)?,
        ctx.arg_u32(4)?,
        ctx.arg_u32(5)?,
        ctx.arg_u32(6)?,
    );
    if returned != 0 && !vfs_probe_output(ctx, returned, 4) {
        return fail(ctx, 87).map(Some);
    }
    if returned != 0 {
        ctx.cpu.write_mem(returned, &0u32.to_le_bytes())?;
    }
    ctx.kernel.ensure_console_window();
    ctx.kernel.refresh_console_window();
    let input_word = |ctx: &mut CallCtx<'_>| -> Result<Option<u32>, KernelError> {
        if input_len < 4 || input == 0 || ctx.cpu.check_guest_access(input, 4, Prot::READ).is_err()
        {
            return Ok(None);
        }
        Ok(Some(ctx.cpu.read_u32_le(input)?))
    };
    let mut result = Vec::new();
    match code {
        0x0102_0004 => {
            let Some(mode) = input_word(ctx)? else {
                return fail(ctx, 87).map(Some);
            };
            if mode & !(ECHO_INPUT | LINE_INPUT | PROCESSED_OUTPUT) != 0 {
                return fail(ctx, 87).map(Some);
            }
            console
                .device
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .set_mode(mode);
        }
        0x0102_0008 => result.extend_from_slice(
            &console
                .device
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .mode
                .to_le_bytes(),
        ),
        0x0102_000c => {
            if input_len > 4096
                || (input_len != 0
                    && (input == 0
                        || ctx
                            .cpu
                            .check_guest_access(input, input_len, Prot::READ)
                            .is_err()))
            {
                return fail(ctx, 87).map(Some);
            }
            let bytes = ctx.cpu.read_mem(input, input_len)?;
            let bytes = bytes.split(|b| *b == 0).next().unwrap_or_default();
            console
                .device
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .set_title(crate::crt_ansi::decode(bytes));
        }
        0x0102_0010 => {
            result = crate::crt_ansi::encode(
                &console
                    .device
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .title,
            );
            result.push(0);
        }
        0x0102_0014 => console
            .device
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear_screen(),
        0x0102_0018 => console
            .device
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .flush_input(),
        0x0102_001c => result.extend_from_slice(
            &(console
                .device
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .rows as u32)
                .to_le_bytes(),
        ),
        0x0102_0020 => {
            // The SDK passes the callback address itself, not &callback.
            if input_len != 4
                || (input != 0 && ctx.cpu.check_guest_access(input, 2, Prot::EXEC).is_err())
            {
                return fail(ctx, 87).map(Some);
            }
            ctx.kernel.console.control_handler = input;
        }
        0x0102_0024 => result.extend_from_slice(
            &(console
                .device
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .columns as u32)
                .to_le_bytes(),
        ),
        0x0102_0028 => {
            let Some(event) = input_word(ctx)? else {
                return fail(ctx, 87).map(Some);
            };
            let key = if event == 0 {
                0
            } else {
                event_key(ctx.kernel, event)
            };
            if key == u32::MAX {
                return fail(ctx, 6).map(Some);
            }
            ctx.kernel.console.control_event = key;
        }
        0x0102_03fc => {} // The shared open description deregisters on its last close.
        _ => return fail(ctx, 50).map(Some),
    }
    if !result.is_empty() {
        if output == 0 || output_len < result.len() as u32 {
            return fail(ctx, 122).map(Some);
        }
        if !vfs_probe_output(ctx, output, result.len() as u32) {
            return fail(ctx, 87).map(Some);
        }
        ctx.cpu.write_mem(output, &result)?;
        if returned != 0 {
            ctx.cpu
                .write_mem(returned, &(result.len() as u32).to_le_bytes())?;
        }
    }
    Ok(Some(DispatchOutcome::ReturnedR0(1)))
}

fn native_message(ctx: &mut CallCtx<'_>, message: (u32, u32, u32, u32)) -> Result<(), KernelError> {
    let args = [
        ctx.arg_u32(0)?,
        ctx.arg_u32(1)?,
        ctx.arg_u32(2)?,
        ctx.arg_u32(3)?,
    ];
    for (i, value) in [message.0, message.1, message.2, message.3]
        .into_iter()
        .enumerate()
    {
        ctx.cpu.write_arg(i as u8, value)?;
    }
    let result = classic_window::default_message(ctx, message.0);
    for (i, value) in args.into_iter().enumerate() {
        ctx.cpu.write_arg(i as u8, value)?;
    }
    result.map(|_| ())
}

fn close_console(ctx: &mut CallCtx<'_>) -> Result<Option<DispatchOutcome>, KernelError> {
    if let Some(entry) = ctx.kernel.begin_dll_notifications(
        ctx.cpu,
        0,
        pocket_kernel::dll_lifecycle::DllContinuation::ExitProcess(0),
    )? {
        return Ok(Some(DispatchOutcome::JumpTo(entry)));
    }
    ctx.kernel.finish_dll_process_exit(ctx.cpu, 0)?;
    Ok(Some(DispatchOutcome::Halt))
}

/// Console windows have an OS-owned procedure, and CMD blocks in CRT reads
/// instead of pumping GetMessage. Service only this window's ordered input;
/// other windows, modal dialogs and Explorer's menu keep their own input.
pub(crate) fn service_input(ctx: &mut CallCtx<'_>) -> Result<Option<DispatchOutcome>, KernelError> {
    if let Some((thread, thunk, frame)) = ctx.kernel.console.callback {
        if thread == ctx.kernel.current_thread
            && thunk == ctx.thunk.thunk_va
            && frame.sp == ctx.cpu.read_reg(ArmReg::Sp)?
        {
            ctx.kernel.console.callback = None;
            for (i, value) in frame.args.into_iter().enumerate() {
                ctx.cpu.write_arg(i as u8, value)?;
            }
            ctx.cpu.write_reg(ArmReg::Lr, frame.lr)?;
        }
    }
    let hwnd = ctx.kernel.console.view.as_ref().map(|view| view.hwnd);
    let Some(device) = ctx.kernel.vfs.console_device() else {
        return Ok(None);
    };
    if let Some(hwnd) = hwnd {
        if ctx.kernel.modal.is_none()
            && ctx.kernel.active_window == hwnd
            && window_is_visible(ctx.kernel, hwnd)
        {
            for _ in 0..64 {
                let Some(event) = take_pending_input(ctx) else {
                    break;
                };
                update_key_state(ctx, event);
                if crate::shell_keyboard::input(ctx, event) {
                    continue;
                }
                if let Some(message) = classic_window::take_input(ctx, event) {
                    if let Some(message) = message {
                        native_message(ctx, message)?;
                    }
                    continue;
                }
                match event {
                    InputEvent::Text { character } => {
                        // Host Enter/Backspace are key events; a simultaneous text
                        // event must not commit/delete the same character twice.
                        let duplicate = matches!(character, '\r' | '\n')
                            && ctx.kernel.pressed_keys[13]
                            || character == '\x08' && ctx.kernel.pressed_keys[8];
                        if !duplicate {
                            device
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .character(character);
                        }
                    }
                    InputEvent::KeyDown { vk } => {
                        let control = ctx.kernel.pressed_keys[0x11]
                            || ctx.kernel.pressed_keys[0xa2]
                            || ctx.kernel.pressed_keys[0xa3];
                        if vk == 0x73 && ctx.kernel.pressed_keys[0x12] {
                            return close_console(ctx);
                        }
                        device
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .key(vk, control);
                    }
                    InputEvent::KeyUp { .. }
                    | InputEvent::PointerDown { .. }
                    | InputEvent::PointerUp { .. }
                    | InputEvent::PointerMove { .. }
                    | InputEvent::TouchSteeringPosition { .. }
                    | InputEvent::StylusAim { .. }
                    | InputEvent::KeyboardRotation { .. } => {}
                    InputEvent::MouseMessage { message, data, .. } => {
                        if message == 0x20a {
                            device.lock().unwrap_or_else(|e| e.into_inner()).wheel(data);
                        }
                    }
                }
            }
            // CMD has no GetMessage pump while fgetws waits. Reuse the shared CE
            // repeat cadence for editing keys, after ordered host releases.
            if let Some((_, vk, _)) = key_repeat_if_due(ctx) {
                if matches!(vk, 8 | 9 | 13 | 0x21..=0x28 | 0x2e) {
                    let control = ctx.kernel.pressed_keys[0x11]
                        || ctx.kernel.pressed_keys[0xa2]
                        || ctx.kernel.pressed_keys[0xa3];
                    device
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .key(vk as u16, control);
                }
            }
        }
        // Nonclient buttons and the SIP queue ordinary window messages too.
        for _ in 0..64 {
            let Some(index) = ctx.kernel.posted_messages.iter().position(|m| m.0 == hwnd) else {
                break;
            };
            let message = ctx.kernel.posted_messages.remove(index).unwrap();
            if message.1 == 0x10 {
                return close_console(ctx);
            }
            if message.1 == 0x102 {
                if let Some(ch) = char::from_u32(message.2) {
                    device
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .character(ch);
                }
            } else {
                native_message(ctx, message)?;
            }
        }
    }
    let revision = device
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .interrupt_revision;
    if revision != ctx.kernel.console.interrupt_seen {
        ctx.kernel.console.interrupt_seen = revision;
        if let Some(mut event) = ctx.kernel.events.get_mut(&ctx.kernel.console.control_event) {
            event.signalled = true;
        }
        let callback = ctx.kernel.console.control_handler;
        if callback != 0 && ctx.kernel.console.callback.is_none() {
            let frame = GuestCallFrame {
                args: [
                    ctx.arg_u32(0)?,
                    ctx.arg_u32(1)?,
                    ctx.arg_u32(2)?,
                    ctx.arg_u32(3)?,
                ],
                lr: ctx.cpu.read_reg(ArmReg::Lr)?,
                sp: ctx.cpu.read_reg(ArmReg::Sp)?,
            };
            ctx.kernel.console.callback =
                Some((ctx.kernel.current_thread, ctx.thunk.thunk_va, frame));
            ctx.cpu.write_reg(ArmReg::Lr, ctx.thunk.thunk_va)?;
            return Ok(Some(DispatchOutcome::JumpTo(callback)));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
