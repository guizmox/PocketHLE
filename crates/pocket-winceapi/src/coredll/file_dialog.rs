//! WinCE common file dialogs backed by the same guest VFS as CreateFile.
use super::*;
use pocket_kernel::controls::{DialogPanel, FileDialog};

pub(super) fn open(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    run(ctx, false)
}
pub(super) fn save(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    run(ctx, true)
}
pub(super) fn error(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(
        ctx.kernel.controls.common_dialog_error,
    ))
}
fn string(ctx: &mut CallCtx<'_>, ptr: u32, max: u32) -> Result<String, KernelError> {
    if ptr == 0 {
        return Ok(String::new());
    }
    Ok(String::from_utf16_lossy(&read_wstr(ctx, ptr, max)?)
        .trim_end_matches('\0')
        .to_string())
}
fn join(dir: &str, name: &str) -> String {
    let path = if name.starts_with(['\\', '/']) {
        name.to_string()
    } else {
        format!("{}\\{}", dir.trim_end_matches(['\\', '/']), name)
    };
    let mut parts = Vec::new();
    for part in path.split(['\\', '/']) {
        match part {
            "" | "." => (),
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    format!("\\{}", parts.join("\\"))
}
fn parent(path: &str) -> String {
    path.rsplit_once('\\')
        .map(|(p, _)| if p.is_empty() { "\\" } else { p })
        .unwrap_or("\\")
        .to_string()
}
fn refresh(ctx: &mut CallCtx<'_>, state: &mut FileDialog) {
    let mut entries: Vec<_> = ctx
        .kernel
        .vfs
        .list_dir(&state.directory)
        .unwrap_or_default()
        .into_iter()
        .filter(|(name, _, dir)| {
            *dir || state.filters.iter().any(|mask| wildcard_match(mask, name))
        })
        .map(|(name, _, dir)| (name, dir))
        .collect();
    entries.sort_by_key(|(name, dir)| (!*dir, name.to_lowercase()));
    if let Some(images) = ctx.kernel.gdi.image_lists.get_mut(&state.image_list) {
        images.clear();
    }
    let mut icons = Vec::new();
    if state.image_list != 0 {
        for (name, directory) in &entries {
            icons.push(images::file_icon(
                ctx.kernel,
                &join(&state.directory, name),
                if *directory { 0x10 } else { 0x20 },
                16,
            ));
        }
        let images = ctx
            .kernel
            .gdi
            .image_lists
            .get_mut(&state.image_list)
            .unwrap();
        images.images = icons;
        images.changed();
    }
    if let Some(list) = ctx.kernel.controls.get_mut(state.list) {
        list.list_items = entries
            .iter()
            .map(|(name, dir)| {
                (
                    if *dir && state.image_list == 0 {
                        format!("[{name}]")
                    } else {
                        name.clone()
                    },
                    0,
                )
            })
            .collect();
        list.list_selected = None;
        list.list_scroll_y = 0;
        list.list_states.clear();
        list.list_images = (0..entries.len()).map(|i| i as i32).collect();
    }
    ctx.kernel.controls.get_mut(state.path_label).unwrap().text = state.directory.clone();
    state.entries = entries;
    state.overwrite = None;
    ctx.kernel
        .controls
        .get_mut(state.status_label)
        .unwrap()
        .text
        .clear();
    repaint_controls(ctx);
    ctx.kernel.framebuffer.mark_dirty();
}
fn status(ctx: &mut CallCtx<'_>, state: &FileDialog, text: &str) {
    ctx.kernel
        .controls
        .get_mut(state.status_label)
        .unwrap()
        .text = text.to_string();
    repaint_controls(ctx);
    ctx.kernel.framebuffer.mark_dirty();
}
fn begin(ctx: &mut CallCtx<'_>, save: bool) -> Result<Option<FileDialog>, KernelError> {
    ctx.kernel.controls.common_dialog_error = 0;
    let ofn = ctx.arg_u32(0)?;
    let Ok(raw) = ctx.cpu.read_mem(ofn, 76) else {
        ctx.kernel.controls.common_dialog_error = 1;
        set_thread_error(ctx, 87);
        return Ok(None);
    };
    let word = |o| u32::from_le_bytes(raw[o..o + 4].try_into().unwrap());
    if !matches!(word(0), 76 | 88) || word(28) == 0 || word(32) == 0 || word(32) > 32768 {
        ctx.kernel.controls.common_dialog_error = 1;
        set_thread_error(ctx, 87);
        return Ok(None);
    }
    let filename = string(ctx, word(28), word(32))?;
    let initial = string(ctx, word(44), 1024)?;
    let title = string(ctx, word(48), 256)?;
    let extension = string(ctx, word(60), 64)?
        .trim_start_matches('.')
        .chars()
        .take(3)
        .collect::<String>();
    let mut filter_choices = Vec::new();
    let filter = word(12);
    if filter != 0 {
        let mut strings = Vec::new();
        let mut ptr = filter;
        for _ in 0..64 {
            let value = string(ctx, ptr, 1024)?;
            if value.is_empty() {
                break;
            }
            ptr += (value.encode_utf16().count() as u32 + 1) * 2;
            strings.push(value);
        }
        for pair in strings.chunks_exact(2) {
            let masks = pair[1]
                .split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            if !masks.is_empty() {
                filter_choices.push((pair[0].clone(), masks));
            }
        }
    }
    if filter_choices.is_empty() {
        filter_choices.push(("All files (*.*)".into(), vec!["*".into()]));
    }
    let filter_index = (word(24).max(1) as usize - 1).min(filter_choices.len() - 1);
    let filters = filter_choices[filter_index].1.clone();
    let mut directory = if initial.is_empty() {
        ctx.kernel.vfs.default_dir().to_string()
    } else {
        join(ctx.kernel.vfs.default_dir(), &initial)
    };
    let mut name = filename.clone();
    if filename.contains(['\\', '/']) {
        let path = join(&directory, &filename);
        directory = parent(&path);
        name = path.rsplit('\\').next().unwrap_or("").to_string();
    }
    if ctx.kernel.vfs.list_dir(&directory).is_none() {
        directory = ctx.kernel.vfs.default_dir().to_string();
    }
    directory = join("\\", &directory);
    let Some(hwnd) = (0xdead2000..0xdead2100).find(|h| !ctx.kernel.window_classes.contains_key(h))
    else {
        return Ok(None);
    };
    let (screen_w, screen_h) = (
        ctx.kernel.framebuffer.width as i32,
        ctx.kernel.framebuffer.height as i32 - pocket_kernel::shell_menu::HEIGHT,
    );
    let classic = ctx.kernel.ce.windows.enabled && screen_w >= 360 && screen_h >= 260;
    let (w, h) = if classic {
        ((screen_w - 24).min(540), (screen_h - 24).min(330))
    } else {
        (screen_w, screen_h)
    };
    let (x, y) = if classic {
        ((screen_w - w) / 2, (screen_h - h) / 2)
    } else {
        (0, 0)
    };
    ctx.kernel.controls.add_panel(DialogPanel {
        hwnd,
        x,
        y,
        w,
        h,
        visible: true,
        border: true,
        caption: Some(if title.is_empty() {
            if save { "Save" } else { "Open" }.into()
        } else {
            title
        }),
        ok_pressed: false,
        caption_height: None,
        caption_command: Some(if classic { 2 } else { 1 }),
        caption_help: false,
    });
    ctx.kernel.window_classes.insert(hwnd, "#32770".into());
    ctx.kernel.window_parents.insert(hwnd, word(4));
    ctx.kernel.window_rects.insert(hwnd, (x, y, w, h));
    ctx.kernel
        .window_threads
        .insert(hwnd, ctx.kernel.current_thread);
    ctx.kernel.window_visibility.insert(hwnd, true);
    ctx.kernel.window_styles.insert(hwnd, (0x90000000, 0));
    let mut control = |class, id, text, x, y, cw, ch| {
        ctx.kernel
            .controls
            .create(hwnd, class, id, text, 0x50010000, x, y, cw, ch)
    };
    let (path_label, list, edit, status_label, filter_combo) = if classic {
        let (cw, ch) = (w - 2, h - 2 - DialogPanel::CAPTION_H);
        control(ControlClass::Static, 11, "Folder:".into(), 8, 5, 50, 20);
        let path = control(
            ControlClass::Static,
            10,
            directory.clone(),
            64,
            5,
            cw - 126,
            20,
        );
        control(ControlClass::Button, 3, "Up".into(), cw - 54, 2, 46, 24);
        let list = control(
            ControlClass::ListView,
            4,
            String::new(),
            8,
            34,
            cw - 16,
            ch - 134,
        );
        control(ControlClass::Static, 12, "Name:".into(), 8, ch - 87, 60, 20);
        let edit = control(ControlClass::Edit, 5, name, 74, ch - 89, cw - 174, 24);
        control(ControlClass::Static, 13, "Type:".into(), 8, ch - 59, 60, 20);
        let filter = control(
            ControlClass::ComboBox,
            14,
            String::new(),
            74,
            ch - 61,
            cw - 174,
            110,
        );
        let status = control(
            ControlClass::Static,
            6,
            String::new(),
            8,
            ch - 30,
            cw - 16,
            20,
        );
        control(
            ControlClass::Button,
            1,
            if save { "Save" } else { "Open" }.into(),
            cw - 88,
            ch - 89,
            80,
            24,
        );
        control(
            ControlClass::Button,
            2,
            "Cancel".into(),
            cw - 88,
            ch - 61,
            80,
            24,
        );
        (path, list, edit, status, filter)
    } else {
        let path = control(
            ControlClass::Static,
            10,
            directory.clone(),
            4,
            3,
            w - 44,
            22,
        );
        control(ControlClass::Button, 3, "Up".into(), w - 38, 2, 32, 22);
        let list = control(
            ControlClass::ListBox,
            4,
            String::new(),
            4,
            29,
            w - 10,
            (h - 122).max(20),
        );
        let edit = control(ControlClass::Edit, 5, name, 4, h - 87, w - 10, 23);
        let status = control(
            ControlClass::Static,
            6,
            String::new(),
            4,
            h - 62,
            w - 10,
            16,
        );
        control(
            ControlClass::Button,
            1,
            if save { "Save" } else { "Open" }.into(),
            4,
            h - 43,
            (w - 16) / 2,
            25,
        );
        control(
            ControlClass::Button,
            2,
            "Cancel".into(),
            w / 2 + 2,
            h - 43,
            (w - 16) / 2,
            25,
        );
        (path, list, edit, status, 0)
    };
    let image_list = if classic {
        ctx.kernel.gdi.create_image_list(16, 16)
    } else {
        0
    };
    if classic {
        let view = ctx.kernel.controls.get_mut(list).unwrap();
        view.style |= 0x00800001;
        view.list_image_lists[1] = image_list;
        view.list_image_sizes[1] = (16, 16);
        let filter = ctx.kernel.controls.get_mut(filter_combo).unwrap();
        filter.style |= 3;
        filter.list_items = filter_choices
            .iter()
            .map(|(title, _)| (title.clone(), 0))
            .collect();
        filter.list_selected = Some(filter_index);
    }
    for child in ctx.kernel.controls.children_of(hwnd) {
        ctx.kernel
            .window_threads
            .insert(child, ctx.kernel.current_thread);
    }
    let state = FileDialog {
        thread: ctx.kernel.current_thread,
        thunk: ctx.thunk.thunk_va,
        frame: GuestCallFrame {
            args: [
                ctx.arg_u32(0)?,
                ctx.arg_u32(1)?,
                ctx.arg_u32(2)?,
                ctx.arg_u32(3)?,
            ],
            sp: ctx.cpu.read_reg(ArmReg::Sp)?,
            lr: ctx.cpu.read_reg(ArmReg::Lr)?,
        },
        ofn,
        hwnd,
        previous_active: ctx.kernel.active_window,
        previous_focus: ctx.kernel.controls.focus,
        directory,
        entries: Vec::new(),
        filters,
        filter_choices,
        filter_combo,
        image_list,
        extension,
        flags: word(52),
        save,
        list,
        edit,
        path_label,
        status_label,
        overwrite: None,
    };
    ctx.kernel.active_window = hwnd;
    ctx.kernel.controls.focus = list;
    crate::shell_keyboard::sip_show(ctx, false);
    Ok(Some(state))
}
fn accept(ctx: &mut CallCtx<'_>, state: &mut FileDialog) -> Result<Option<u32>, KernelError> {
    let name = ctx
        .kernel
        .controls
        .get(state.edit)
        .unwrap()
        .text
        .trim()
        .to_string();
    if name.is_empty() {
        return Ok(None);
    }
    let mut path = join(&state.directory, &name);
    if name.contains(['*', '?']) {
        if state.save {
            status(ctx, state, "Choose a file name");
            return Ok(None);
        }
        let pattern = path.rsplit('\\').next().unwrap_or("*").to_string();
        state.directory = parent(&path);
        state.filters = pattern
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        refresh(ctx, state);
        return Ok(None);
    }
    if ctx.kernel.vfs.list_dir(&path).is_some() {
        state.directory = path;
        ctx.kernel
            .controls
            .get_mut(state.edit)
            .unwrap()
            .text
            .clear();
        refresh(ctx, state);
        return Ok(None);
    }
    let base = path.rsplit('\\').next().unwrap_or("");
    if !base.contains('.') && !state.extension.is_empty() {
        path.push('.');
        path.push_str(&state.extension);
    }
    let dir = parent(&path);
    let base = path.rsplit('\\').next().unwrap_or("");
    let listing = ctx.kernel.vfs.list_dir(&dir);
    let exists = listing.as_ref().is_some_and(|items| {
        items
            .iter()
            .any(|(n, _, d)| !*d && n.eq_ignore_ascii_case(base))
    });
    if listing.is_none() && state.flags & 0x800 != 0 {
        status(ctx, state, "Folder does not exist");
        return Ok(None);
    }
    if !state.save && !exists && state.flags & 0x1000 != 0 {
        status(ctx, state, "File does not exist");
        return Ok(None);
    }
    if state.save && exists && state.flags & 2 != 0 && state.overwrite.as_ref() != Some(&path) {
        state.overwrite = Some(path);
        status(ctx, state, "File exists. Save again to replace.");
        return Ok(None);
    }
    let output = ctx.cpu.read_u32_le(state.ofn + 28)?;
    let capacity = ctx.cpu.read_u32_le(state.ofn + 32)?;
    let mut units: Vec<u16> = path.encode_utf16().collect();
    units.push(0);
    if units.len() > capacity as usize {
        ctx.kernel.controls.common_dialog_error = 0x3003;
        set_thread_error(ctx, 87);
        if capacity != 0 {
            ctx.cpu
                .write_mem(output, &(units.len().min(65535) as u16).to_le_bytes())?;
        }
        return Ok(Some(0));
    }
    ctx.cpu.write_mem(
        output,
        &units
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )?;
    let title_ptr = ctx.cpu.read_u32_le(state.ofn + 36)?;
    let title_cap = ctx.cpu.read_u32_le(state.ofn + 40)?;
    if title_ptr != 0 && title_cap > 0 {
        let bytes: Vec<_> = base
            .encode_utf16()
            .take(title_cap.saturating_sub(1) as usize)
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        ctx.cpu.write_mem(title_ptr, &bytes)?;
    }
    let offset = path[..path.len() - base.len()].encode_utf16().count() as u16;
    let ext = match base.rfind('.') {
        Some(i) if i + 1 < base.len() => offset + base[..=i].encode_utf16().count() as u16,
        Some(_) => 0,
        None => path.encode_utf16().count() as u16,
    };
    ctx.cpu.write_mem(
        state.ofn + 56,
        &[offset.to_le_bytes(), ext.to_le_bytes()].concat(),
    )?;
    let filter_index = ctx
        .kernel
        .controls
        .get(state.filter_combo)
        .and_then(|c| c.list_selected)
        .map(|i| i as u32 + 1)
        .unwrap_or_else(|| ctx.cpu.read_u32_le(state.ofn + 24).unwrap_or(0).max(1));
    ctx.cpu
        .write_mem(state.ofn + 24, &filter_index.to_le_bytes())?;
    if state.flags & 8 == 0 {
        ctx.kernel.vfs.set_default_dir(&dir);
    }
    Ok(Some(1))
}
fn finish(
    ctx: &mut CallCtx<'_>,
    state: FileDialog,
    result: u32,
) -> Result<DispatchOutcome, KernelError> {
    crate::shell_keyboard::sip_show(ctx, false);
    ctx.kernel.gdi.image_lists.remove(&state.image_list);
    discard_window_metadata(ctx, &state.hwnd);
    ctx.kernel.active_window = state.previous_active;
    ctx.kernel.controls.focus = state.previous_focus;
    for (reg, value) in [ArmReg::R0, ArmReg::R1, ArmReg::R2, ArmReg::R3]
        .into_iter()
        .zip(state.frame.args)
    {
        ctx.cpu.write_reg(reg, value)?;
    }
    ctx.cpu.write_reg(ArmReg::Sp, state.frame.sp)?;
    ctx.cpu.write_reg(ArmReg::Lr, state.frame.lr)?;
    queue_visible_window_paints(ctx.kernel, state.previous_active);
    repaint_controls(ctx);
    ctx.kernel.framebuffer.mark_dirty();
    Ok(DispatchOutcome::ReturnedR0(result))
}
fn run(ctx: &mut CallCtx<'_>, save: bool) -> Result<DispatchOutcome, KernelError> {
    let mut state = if let Some(state) = ctx.kernel.controls.file_dialog.take() {
        if state.thread != ctx.kernel.current_thread
            || state.thunk != ctx.thunk.thunk_va
            || state.frame.sp != ctx.cpu.read_reg(ArmReg::Sp)?
        {
            ctx.kernel.controls.file_dialog = Some(state);
            return Ok(DispatchOutcome::ReturnedR0(0));
        }
        state
    } else {
        let Some(mut state) = begin(ctx, save)? else {
            return Ok(DispatchOutcome::ReturnedR0(0));
        };
        refresh(ctx, &mut state);
        state
    };
    crate::shell_keyboard::service(ctx);
    while let Some(index) = ctx
        .kernel
        .posted_messages
        .iter()
        .position(|m| m.0 == state.edit && m.1 == 0x102)
    {
        let (_, msg, wp, lp) = ctx.kernel.posted_messages.remove(index).unwrap();
        control_message(ctx, state.edit, msg, wp, lp)?;
    }
    while let Some(event) = take_pending_input(ctx) {
        update_key_state(ctx, event);
        if crate::shell_keyboard::input(ctx, event) {
            continue;
        }
        if let InputEvent::PointerDown { x, y } = event {
            if i32::from(y) >= ctx.kernel.framebuffer.height as i32 - 26
                && u32::from(x) >= ctx.kernel.framebuffer.width - 26
            {
                crate::shell_keyboard::sip_show(ctx, !ctx.kernel.shell_keyboard.visible);
                continue;
            }
        }
        if let InputEvent::Text { character } = event {
            if ctx.kernel.controls.focus == state.edit {
                control_message(ctx, state.edit, 0x102, character as u32, 1)?;
            }
            continue;
        }
        let action = match event {
            InputEvent::PointerDown { x, y } => {
                if ctx.kernel.controls.hit_test(i32::from(x), i32::from(y)) == Some(state.edit) {
                    ctx.kernel.controls.focus = state.edit;
                    crate::shell_keyboard::sip_show(ctx, true);
                }
                ctx.kernel
                    .controls
                    .pointer_down_in(state.hwnd, i32::from(x), i32::from(y))
            }
            InputEvent::PointerUp { x, y } => {
                ctx.kernel
                    .controls
                    .pointer_up_in(state.hwnd, i32::from(x), i32::from(y))
            }
            InputEvent::KeyDown { vk: 0x1b } => Some(ControlAction::Clicked {
                parent: state.hwnd,
                hwnd: 0,
                id: 2,
            }),
            InputEvent::KeyDown { vk: 0x0d } if ctx.kernel.controls.focus == state.filter_combo => {
                ctx.kernel.controls.key_down(0x0d)
            }
            InputEvent::KeyDown { vk: 0x0d } => {
                if state.image_list != 0
                    && ctx.kernel.controls.focus == state.list
                    && ctx
                        .kernel
                        .controls
                        .get(state.list)
                        .unwrap()
                        .list_selected
                        .is_some()
                {
                    ctx.kernel.controls.key_down(0x0d)
                } else {
                    Some(ControlAction::Clicked {
                        parent: state.hwnd,
                        hwnd: 0,
                        id: 1,
                    })
                }
            }
            InputEvent::KeyDown { vk: 8 } if ctx.kernel.controls.focus == state.list => {
                Some(ControlAction::Clicked {
                    parent: state.hwnd,
                    hwnd: 0,
                    id: 3,
                })
            }
            InputEvent::KeyDown { vk } => ctx.kernel.controls.key_down_in(state.hwnd, vk),
            _ => None,
        };
        match action {
            Some(ControlAction::Clicked { id: 2, .. }) => return finish(ctx, state, 0),
            Some(ControlAction::Clicked { id: 1, .. }) => {
                if let Some(result) = accept(ctx, &mut state)? {
                    return finish(ctx, state, result);
                }
            }
            Some(ControlAction::Clicked { id: 3, .. }) => {
                state.directory = parent(state.directory.trim_end_matches('\\'));
                ctx.kernel
                    .controls
                    .get_mut(state.edit)
                    .unwrap()
                    .text
                    .clear();
                refresh(ctx, &mut state);
            }
            Some(ControlAction::Notify { hwnd, index, .. })
                if hwnd == state.filter_combo && index >= 0 =>
            {
                if let Some((_, filters)) = state.filter_choices.get(index as usize) {
                    state.filters = filters.clone();
                    refresh(ctx, &mut state);
                }
            }
            Some(ControlAction::Notify {
                hwnd, index, code, ..
            }) if hwnd == state.list && index >= 0 => {
                if let Some((name, dir)) = state.entries.get(index as usize).cloned() {
                    let activate = matches!(code as i32, -3 | -4);
                    if dir && (state.image_list == 0 || activate) {
                        state.directory = join(&state.directory, &name);
                        ctx.kernel
                            .controls
                            .get_mut(state.edit)
                            .unwrap()
                            .text
                            .clear();
                        refresh(ctx, &mut state);
                    } else {
                        ctx.kernel.controls.get_mut(state.edit).unwrap().text = name;
                        state.overwrite = None;
                        if !dir && activate {
                            if let Some(result) = accept(ctx, &mut state)? {
                                return finish(ctx, state, result);
                            }
                        }
                    }
                }
            }
            _ => (),
        }
        repaint_controls(ctx);
        ctx.kernel.framebuffer.mark_dirty();
    }
    ctx.kernel.controls.file_dialog = Some(state);
    if let Some(outcome) = wait_display_until(
        ctx,
        std::time::Instant::now() + std::time::Duration::from_millis(1),
    )? {
        return Ok(outcome);
    }
    Ok(DispatchOutcome::JumpTo(ctx.thunk.thunk_va))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::{stub::StubCpu, Cpu};

    fn wide(value: &str) -> Vec<u8> {
        value
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    fn output(cpu: &mut StubCpu, address: u32, count: u32) -> String {
        let bytes = cpu.read_mem(address, count * 2).unwrap();
        String::from_utf16_lossy(
            &bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
                .take_while(|&u| u != 0)
                .collect::<Vec<_>>(),
        )
    }

    fn fixture() -> (tempfile::TempDir, StubCpu, KernelState, WinCeDispatcher) {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("Programs/Tools")).unwrap();
        std::fs::write(root.path().join("Root.EXE"), b"application").unwrap();
        std::fs::write(root.path().join("Notes.txt"), b"notes").unwrap();
        std::fs::write(root.path().join("Programs/Tools/tool.exe"), b"tool").unwrap();
        std::fs::write(root.path().join("Programs/Tools/note.txt"), b"note").unwrap();
        let (mut cpu, mut kernel, dispatcher) = setup();
        kernel.vfs.mount_save_dir(r"\Data\", root.path());
        kernel.ce.windows.enabled = true;
        kernel.framebuffer = pocket_kernel::framebuffer::deferred::DeferredFramebuffer::new(
            pocket_kernel::framebuffer::Framebuffer::new(640, 480),
        )
        .unwrap();
        cpu.write_mem(0x1800, &wide(r"\Data")).unwrap();
        cpu.write_mem(
            0x1900,
            &wide("Programs (*.exe)\0*.exe\0All files (*.*)\0*.*\0"),
        )
        .unwrap();
        cpu.write_mem(0x2000, &[0; 512]).unwrap();
        let mut ofn = [0u8; 76];
        for (offset, value) in [
            (0, 76u32),
            (12, 0x1900),
            (24, 1),
            (28, 0x2000),
            (32, 256),
            (36, 0x2200),
            (40, 64),
            (44, 0x1800),
            (52, 0x1808),
        ] {
            ofn[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        cpu.write_mem(0x1000, &ofn).unwrap();
        (root, cpu, kernel, dispatcher)
    }

    fn pump(
        cpu: &mut StubCpu,
        kernel: &mut KernelState,
        dispatcher: &mut WinCeDispatcher,
    ) -> DispatchOutcome {
        call(
            cpu,
            kernel,
            dispatcher,
            "coredll.dll",
            "GetOpenFileNameW",
            &[0x1000],
        )
    }

    fn tap(kernel: &mut KernelState, hwnd: u32, local_x: i32, local_y: i32) {
        let (x, y, _, _) = kernel.controls.screen_rect(hwnd).unwrap();
        let (x, y) = ((x + local_x) as u16, (y + local_y) as u16);
        kernel.pending_input.extend([
            InputEvent::PointerDown { x, y },
            InputEvent::PointerUp { x, y },
        ]);
    }

    #[test]
    fn browse_navigates_nested_folders_and_returns_the_selected_file_and_filter() {
        let (root, mut cpu, mut kernel, mut dispatcher) = fixture();
        let previous = 0xdead0080;
        let previous_focus = kernel.controls.create(
            previous,
            ControlClass::Edit,
            20,
            "Run command".into(),
            0x50000000,
            0,
            0,
            100,
            22,
        );
        kernel.active_window = previous;
        kernel.controls.focus = previous_focus;
        assert!(matches!(
            pump(&mut cpu, &mut kernel, &mut dispatcher),
            DispatchOutcome::JumpTo(_)
        ));
        let dialog = kernel.controls.file_dialog.as_ref().unwrap();
        let (hwnd, list, edit, combo, images) = (
            dialog.hwnd,
            dialog.list,
            dialog.edit,
            dialog.filter_combo,
            dialog.image_list,
        );
        assert_eq!(
            dialog.entries,
            [("Programs".into(), true), ("Root.EXE".into(), false)]
        );
        let children = kernel.controls.children_of(hwnd);
        let first_row_y = kernel.controls.get(list).unwrap().list_header_height() + 8;
        tap(&mut kernel, list, 10, first_row_y);
        assert!(matches!(
            pump(&mut cpu, &mut kernel, &mut dispatcher),
            DispatchOutcome::JumpTo(_)
        ));
        assert_eq!(
            kernel.controls.file_dialog.as_ref().unwrap().directory,
            r"\Data"
        );
        assert_eq!(kernel.controls.get(edit).unwrap().text, "Programs");
        kernel
            .pending_input
            .push_back(InputEvent::KeyDown { vk: 13 });
        assert!(matches!(
            pump(&mut cpu, &mut kernel, &mut dispatcher),
            DispatchOutcome::JumpTo(_)
        ));
        assert_eq!(
            kernel.controls.file_dialog.as_ref().unwrap().directory,
            r"\Data\Programs"
        );
        kernel.controls.get_mut(list).unwrap().list_selected = Some(0);
        kernel
            .pending_input
            .push_back(InputEvent::KeyDown { vk: 13 });
        assert!(matches!(
            pump(&mut cpu, &mut kernel, &mut dispatcher),
            DispatchOutcome::JumpTo(_)
        ));
        assert_eq!(
            kernel.controls.file_dialog.as_ref().unwrap().directory,
            r"\Data\Programs\Tools"
        );
        kernel
            .pending_input
            .push_back(InputEvent::KeyDown { vk: 8 });
        assert!(matches!(
            pump(&mut cpu, &mut kernel, &mut dispatcher),
            DispatchOutcome::JumpTo(_)
        ));
        assert_eq!(
            kernel.controls.file_dialog.as_ref().unwrap().directory,
            r"\Data\Programs"
        );
        let up = children
            .iter()
            .copied()
            .find(|&h| kernel.controls.get(h).unwrap().id == 3)
            .unwrap();
        tap(&mut kernel, up, 10, 10);
        assert!(matches!(
            pump(&mut cpu, &mut kernel, &mut dispatcher),
            DispatchOutcome::JumpTo(_)
        ));
        assert_eq!(
            kernel.controls.file_dialog.as_ref().unwrap().directory,
            r"\Data"
        );
        assert!(kernel.controls.get(edit).unwrap().text.is_empty());
        kernel.controls.get_mut(edit).unwrap().text = r"Programs\Tools".into();
        kernel.controls.focus = edit;
        kernel
            .pending_input
            .push_back(InputEvent::KeyDown { vk: 13 });
        assert!(matches!(
            pump(&mut cpu, &mut kernel, &mut dispatcher),
            DispatchOutcome::JumpTo(_)
        ));
        assert_eq!(
            kernel.controls.file_dialog.as_ref().unwrap().directory,
            r"\Data\Programs\Tools"
        );
        assert_eq!(
            kernel.controls.file_dialog.as_ref().unwrap().entries,
            [("tool.exe".into(), false)]
        );
        kernel.controls.focus = combo;
        kernel
            .pending_input
            .push_back(InputEvent::KeyDown { vk: 0x28 });
        assert!(matches!(
            pump(&mut cpu, &mut kernel, &mut dispatcher),
            DispatchOutcome::JumpTo(_)
        ));
        assert_eq!(
            kernel.controls.file_dialog.as_ref().unwrap().entries,
            [("note.txt".into(), false), ("tool.exe".into(), false)]
        );
        kernel.controls.focus = list;
        kernel.controls.get_mut(list).unwrap().list_selected = Some(1);
        kernel
            .pending_input
            .push_back(InputEvent::KeyDown { vk: 13 });
        assert_eq!(
            pump(&mut cpu, &mut kernel, &mut dispatcher),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(
            output(&mut cpu, 0x2000, 256),
            r"\Data\Programs\Tools\tool.exe"
        );
        assert_eq!(output(&mut cpu, 0x2200, 64), "tool.exe");
        assert_eq!(cpu.read_u32_le(0x1018).unwrap(), 2);
        let offset = u16::from_le_bytes(cpu.read_mem(0x1038, 2).unwrap().try_into().unwrap());
        assert_eq!(
            usize::from(offset),
            r"\Data\Programs\Tools\".encode_utf16().count()
        );
        assert!(kernel.controls.file_dialog.is_none());
        assert!(!kernel.gdi.image_lists.contains_key(&images));
        assert!(!kernel.window_classes.contains_key(&hwnd));
        assert!(children
            .iter()
            .all(|h| !kernel.window_threads.contains_key(h) && kernel.controls.get(*h).is_none()));
        assert_eq!(kernel.active_window, previous);
        assert_eq!(kernel.controls.focus, previous_focus);
        assert_eq!(kernel.vfs.default_dir(), "\\");
        assert_eq!(
            std::fs::read(root.path().join("Programs/Tools/tool.exe")).unwrap(),
            b"tool"
        );
    }

    #[test]
    fn save_edits_the_initial_name_and_accepts_an_existing_empty_parent() {
        let (root, mut cpu, mut kernel, mut dispatcher) = fixture();
        std::fs::create_dir(root.path().join("Programs/Empty")).unwrap();
        cpu.write_mem(0x2000, &wide("Doc1")).unwrap();
        let save =
            |cpu: &mut StubCpu, kernel: &mut KernelState, dispatcher: &mut WinCeDispatcher| {
                call(
                    cpu,
                    kernel,
                    dispatcher,
                    "coredll.dll",
                    "GetSaveFileNameW",
                    &[0x1000],
                )
            };
        assert!(matches!(
            save(&mut cpu, &mut kernel, &mut dispatcher),
            DispatchOutcome::JumpTo(_)
        ));
        let edit = kernel.controls.file_dialog.as_ref().unwrap().edit;
        tap(&mut kernel, edit, 10, 10);
        for _ in 0..4 {
            kernel
                .pending_input
                .extend([InputEvent::KeyDown { vk: 8 }, InputEvent::KeyUp { vk: 8 }]);
        }
        kernel.pending_input.extend(
            r"Programs\Empty\Сохранение.txt"
                .chars()
                .map(|character| InputEvent::Text { character }),
        );
        kernel
            .pending_input
            .push_back(InputEvent::KeyDown { vk: 13 });
        assert_eq!(
            save(&mut cpu, &mut kernel, &mut dispatcher),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(
            output(&mut cpu, 0x2000, 256),
            r"\Data\Programs\Empty\Сохранение.txt"
        );
        assert!(kernel.controls.file_dialog.is_none());
        assert_eq!(kernel.controls.common_dialog_error, 0);
        assert!(std::fs::read_dir(root.path().join("Programs/Empty"))
            .unwrap()
            .next()
            .is_none());
    }

    #[test]
    fn browse_owns_input_over_the_run_thread_and_releases_its_dialog_on_destruction() {
        let (_root, mut cpu, mut kernel, _dispatcher) = fixture();
        let thunk = super::super::tests::dummy_thunk();
        kernel.current_thread = 1;
        cpu.write_reg(ArmReg::R0, 0x1000).unwrap();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        let state = begin(&mut ctx, false).unwrap().unwrap();
        let (hwnd, images) = (state.hwnd, state.image_list);
        let children = ctx.kernel.controls.children_of(hwnd);
        assert_eq!(ctx.kernel.window_threads[&hwnd], 1);
        assert!(children
            .iter()
            .all(|h| ctx.kernel.window_threads.get(h) == Some(&1)));
        ctx.kernel.controls.file_dialog = Some(state);
        let event = InputEvent::KeyDown { vk: 0x28 };
        ctx.kernel.pending_input.push_back(event);
        ctx.kernel.current_thread = 0;
        assert_eq!(take_pending_input(&mut ctx), None);
        assert_eq!(ctx.kernel.pending_input.front(), Some(&event));
        ctx.kernel.current_thread = 1;
        assert_eq!(take_pending_input(&mut ctx), Some(event));
        ctx.kernel.discard_window_metadata(&hwnd);
        assert!(ctx.kernel.controls.file_dialog.is_none());
        assert!(!ctx.kernel.gdi.image_lists.contains_key(&images));
        assert!(children
            .iter()
            .all(|h| !ctx.kernel.window_threads.contains_key(h)));
    }

    #[test]
    fn legacy_file_click_selects_before_enter_accepts() {
        let (_root, mut cpu, mut kernel, mut dispatcher) = fixture();
        kernel.ce.windows.enabled = false;
        assert!(matches!(
            pump(&mut cpu, &mut kernel, &mut dispatcher),
            DispatchOutcome::JumpTo(_)
        ));
        let list = kernel.controls.file_dialog.as_ref().unwrap().list;
        let row = kernel.controls.get(list).unwrap().choice_row_height;
        tap(&mut kernel, list, 10, row + 8);
        assert!(matches!(
            pump(&mut cpu, &mut kernel, &mut dispatcher),
            DispatchOutcome::JumpTo(_)
        ));
        kernel
            .pending_input
            .push_back(InputEvent::KeyDown { vk: 13 });
        assert_eq!(
            pump(&mut cpu, &mut kernel, &mut dispatcher),
            DispatchOutcome::ReturnedR0(1)
        );
        assert_eq!(output(&mut cpu, 0x2000, 256), r"\Data\Root.EXE");
    }

    #[test]
    fn open_save_filter_and_cancel_preserve_the_guest_contract() {
        let root = std::env::temp_dir().join(format!(
            "pockethle-dialog-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Existing.SC2"), b"city").unwrap();
        std::fs::write(root.join("Other.txt"), b"other").unwrap();
        let (mut cpu, mut k, mut d) = setup();
        k.vfs.mount_save_dir(r"\My Documents\", &root);
        let wide = |value: &str| {
            value
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        };
        cpu.write_mem(0x1800, &wide(r"\My Documents")).unwrap();
        cpu.write_mem(0x1900, &wide("Cities\0 *.SC2 \0")).unwrap();
        cpu.write_mem(0x1a00, &wide("SC2")).unwrap();
        cpu.write_mem(0x2000, &[0; 512]).unwrap();
        let mut ofn = [0u8; 76];
        for (offset, value) in [
            (0, 76u32),
            (12, 0x1900),
            (24, 1),
            (28, 0x2000),
            (32, 256),
            (44, 0x1800),
            (52, 0x1808),
            (60, 0x1a00),
        ] {
            ofn[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        cpu.write_mem(0x1000, &ofn).unwrap();
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "GetOpenFileNameW",
                &[0x1000]
            ),
            DispatchOutcome::JumpTo(_)
        ));
        let dialog = k.controls.file_dialog.as_ref().unwrap();
        assert_eq!(dialog.entries, vec![("Existing.SC2".into(), false)]);
        let edit = dialog.edit;
        k.controls.get_mut(edit).unwrap().text = "Existing.SC2".into();
        k.pending_input.push_back(InputEvent::KeyDown { vk: 13 });
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "GetOpenFileNameW",
                &[0x1000]
            ),
            DispatchOutcome::ReturnedR0(1)
        ));
        let output = cpu.read_mem(0x2000, 512).unwrap();
        let units: Vec<_> = output
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
            .take_while(|u| *u != 0)
            .collect();
        assert_eq!(
            String::from_utf16_lossy(&units),
            r"\My Documents\Existing.SC2"
        );
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "GetOpenFileNameW",
                &[0x1000]
            ),
            DispatchOutcome::JumpTo(_)
        ));
        k.pending_input.push_back(InputEvent::KeyDown { vk: 0x1b });
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "GetOpenFileNameW",
                &[0x1000]
            ),
            DispatchOutcome::ReturnedR0(0)
        ));
        assert_eq!(cpu.read_mem(0x2000, 512).unwrap(), output);
        cpu.write_mem(0x2000, &wide("New City")).unwrap();
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "GetSaveFileNameW",
                &[0x1000]
            ),
            DispatchOutcome::JumpTo(_)
        ));
        k.pending_input.push_back(InputEvent::KeyDown { vk: 13 });
        assert!(matches!(
            call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "GetSaveFileNameW",
                &[0x1000]
            ),
            DispatchOutcome::ReturnedR0(1)
        ));
        let bytes = cpu.read_mem(0x2000, 512).unwrap();
        let units: Vec<_> = bytes
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
            .take_while(|u| *u != 0)
            .collect();
        assert_eq!(
            String::from_utf16_lossy(&units),
            r"\My Documents\New City.SC2"
        );
        assert!(k.controls.file_dialog.is_none());
        assert_eq!(k.controls.common_dialog_error, 0);
        assert_eq!(std::fs::read(root.join("Existing.SC2")).unwrap(), b"city");
        std::fs::remove_dir_all(root).unwrap();
    }
}
