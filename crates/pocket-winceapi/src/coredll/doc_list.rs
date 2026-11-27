//! PocketPC document lists reuse the shared list control and guest VFS.
use super::*;
use pocket_kernel::controls::doc_list::DocList;
pub(super) const CREATE: &str = "__PocketHLE_DocListCreate";
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("doclist.dll", "DocList_Create", create);
    d.register_handler("coredll.dll", CREATE, create);
}
fn filters(ctx: &mut CallCtx<'_>, mut pointer: u32) -> Result<Vec<Vec<String>>, KernelError> {
    if pointer == 0 {
        return Ok(vec![vec!["*".into()]]);
    }
    let mut parts = Vec::new();
    for _ in 0..128 {
        let text = read_wstr(ctx, pointer, 4096)?;
        pointer = pointer
            .checked_add((text.len() as u32 + 1) * 2)
            .ok_or_else(|| KernelError::Dispatch("Document filter overflow".into()))?;
        if text.is_empty() {
            break;
        }
        parts.push(String::from_utf16_lossy(&text));
    }
    let mut filters: Vec<_> = parts
        .chunks_exact(2)
        .map(|pair| {
            pair[1]
                .split(';')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect();
    if filters.is_empty() {
        filters.push(vec!["*".into()]);
    }
    Ok(filters)
}
fn refresh(ctx: &mut CallCtx<'_>, hwnd: u32) -> Result<bool, KernelError> {
    let Some(state) = ctx.kernel.controls.doc_lists.get(&hwnd) else {
        return Ok(false);
    };
    if !state.updates_enabled {
        return Ok(true);
    }
    let directory = state.directory.clone();
    let selected = ctx
        .kernel
        .controls
        .get(hwnd)
        .and_then(|c| c.list_selected)
        .and_then(|i| state.paths.get(i))
        .cloned();
    let masks = state
        .filters
        .get(state.filter_index.saturating_sub(1) as usize)
        .cloned()
        .unwrap_or_else(|| vec!["*".into()]);
    let recursive = state.recursive;
    let mut pending = vec![(directory.clone(), String::new())];
    let mut visited = 0usize;
    let mut entries = Vec::new();
    while let Some((folder, relative)) = pending.pop() {
        visited += 1;
        if visited > 4096 {
            set_thread_error(ctx, 8);
            return Ok(false);
        }
        let Some(children) = ctx.kernel.vfs.list_dir(&folder) else {
            set_thread_error(ctx, 3);
            return Ok(false);
        };
        for (name, _, is_directory) in children {
            let display = if relative.is_empty() {
                name.clone()
            } else {
                format!("{relative}\\{name}")
            };
            let path = file_dialog::join(&directory, &display);
            if path.encode_utf16().count() >= 260 {
                continue;
            }
            if is_directory {
                if recursive {
                    pending.push((path, display));
                }
            } else if masks.iter().any(|mask| wildcard_match(mask, &name)) {
                if entries.len() >= 0x100000 {
                    set_thread_error(ctx, 8);
                    return Ok(false);
                }
                entries.push(display);
            }
        }
    }
    entries.sort_by_key(|name| name.to_lowercase());
    if state.descending {
        entries.reverse();
    }
    let paths: Vec<_> = entries
        .iter()
        .map(|name| file_dialog::join(&directory, name))
        .collect();
    let control = ctx
        .kernel
        .controls
        .get_mut(hwnd)
        .ok_or_else(|| KernelError::Dispatch("Document list lost its HWND".into()))?;
    control.list_items = entries.into_iter().map(|name| (name, 0)).collect();
    control.list_selected = selected.and_then(|path| {
        paths
            .iter()
            .position(|candidate| candidate.eq_ignore_ascii_case(&path))
    });
    control.list_scroll_y = 0;
    ctx.kernel.controls.doc_lists.get_mut(&hwnd).unwrap().paths = paths;
    repaint_controls(ctx);
    Ok(true)
}
fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let pointer = ctx.arg_u32(0)?;
    let bytes = ctx.cpu.read_mem(pointer, 24)?;
    let word = |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    if word(0) != 24 {
        set_thread_error(ctx, 87);
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let parent = word(4);
    if !window_exists(ctx.kernel, parent) {
        set_thread_error(ctx, 1400);
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let directory = if word(8) == 0 {
        "\\My Documents".into()
    } else {
        String::from_utf16_lossy(&read_wstr(ctx, word(8), 260)?)
    };
    let (directory, recursive) = resolve_directory(&directory);
    if ctx.kernel.vfs.list_dir(&directory).is_none() {
        set_thread_error(ctx, 3);
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let filters = filters(ctx, word(12))?;
    let index = u16::from_le_bytes(bytes[16..18].try_into().unwrap()) as u32;
    if index as usize > filters.len() {
        set_thread_error(ctx, 87);
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    let id = u16::from_le_bytes(bytes[18..20].try_into().unwrap()) as u32;
    let rect = ctx.kernel.window_rects.get(&parent).copied().unwrap_or((
        0,
        0,
        ctx.kernel.framebuffer.width as i32,
        ctx.kernel.framebuffer.height as i32,
    ));
    let hwnd = ctx.kernel.controls.create(
        parent,
        ControlClass::ListView,
        id,
        String::new(),
        0x50000003,
        0,
        0,
        (rect.2 - rect.0).max(1),
        (rect.3 - rect.1).max(1),
    );
    ctx.kernel.controls.doc_lists.insert(
        hwnd,
        DocList {
            directory,
            filters,
            filter_index: index.max(1),
            paths: Vec::new(),
            updates_enabled: true,
            descending: false,
            recursive,
        },
    );
    if let Err(error) = refresh(ctx, hwnd) {
        ctx.kernel.controls.destroy(hwnd);
        return Err(error);
    }
    Ok(DispatchOutcome::ReturnedR0(hwnd))
}
fn selected(ctx: &CallCtx<'_>, hwnd: u32) -> Option<String> {
    let index = ctx.kernel.controls.get(hwnd)?.list_selected?;
    ctx.kernel
        .controls
        .doc_lists
        .get(&hwnd)?
        .paths
        .get(index)
        .cloned()
}
/// DLM_GETSELECTEDPATHNAME, DLM_SETFOLDER and ordinary list selection all
/// operate on this exact control owner; no MFC-side filesystem cache exists.
pub(super) fn message(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    message: u32,
    w: u32,
    l: u32,
) -> Result<Option<u32>, KernelError> {
    if !ctx.kernel.controls.doc_lists.contains_key(&hwnd) {
        return Ok(None);
    }
    let result = match message {
        0x658 => u32::from(refresh(ctx, hwnd)?), // Refresh
        0x65b => {
            ctx.kernel
                .controls
                .doc_lists
                .get_mut(&hwnd)
                .unwrap()
                .updates_enabled = false;
            0
        }
        0x65c => {
            ctx.kernel
                .controls
                .doc_lists
                .get_mut(&hwnd)
                .unwrap()
                .updates_enabled = true;
            u32::from(refresh(ctx, hwnd)?)
        }
        0x659 => {
            let state = ctx.kernel.controls.doc_lists.get_mut(&hwnd).unwrap();
            if w == 0 || w as usize > state.filters.len() {
                set_thread_error(ctx, 87);
                return Ok(Some(0));
            }
            state.filter_index = w;
            u32::from(refresh(ctx, hwnd)?)
        }
        0x666 => ctx.kernel.controls.doc_lists[&hwnd].filter_index,
        0x664 => {
            let directory = String::from_utf16_lossy(&read_wstr(ctx, l, 260)?);
            let (directory, recursive) = resolve_directory(&directory);
            if ctx.kernel.vfs.list_dir(&directory).is_none() {
                set_thread_error(ctx, 3);
                return Ok(Some(0));
            }
            let state = ctx.kernel.controls.doc_lists.get_mut(&hwnd).unwrap();
            state.directory = directory;
            state.recursive = recursive;
            u32::from(refresh(ctx, hwnd)?)
        }
        0x65e => {
            if let Some(path) = selected(ctx, hwnd) {
                let units: Vec<_> = path.encode_utf16().chain(Some(0)).collect();
                if w == 0 || units.len() > w as usize {
                    set_thread_error(ctx, 122);
                    return Ok(Some(0));
                }
                ctx.cpu.write_mem(
                    l,
                    &units
                        .into_iter()
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>(),
                )?;
                1
            } else {
                0
            }
        }
        0x667 | 0x65f => {
            let path = String::from_utf16_lossy(&read_wstr(ctx, l, 260)?);
            let state = &ctx.kernel.controls.doc_lists[&hwnd];
            let path = file_dialog::join(&state.directory, &path);
            let index = state
                .paths
                .iter()
                .position(|candidate| candidate.eq_ignore_ascii_case(&path));
            if let Some(index) = index {
                ctx.kernel.controls.get_mut(hwnd).unwrap().list_selected =
                    if message == 0x65f && w == 0 {
                        None
                    } else {
                        Some(index)
                    };
                repaint_controls(ctx);
                1
            } else {
                0
            }
        }
        0x665 => u32::from(selected(ctx, hwnd).is_some()),
        0x66b => {
            if let Some(path) = selected(ctx, hwnd) {
                if !ctx.kernel.vfs.delete_file(&path) {
                    set_thread_error(ctx, 5);
                    0
                } else {
                    u32::from(refresh(ctx, hwnd)?)
                }
            } else {
                0
            }
        }
        0x1004 => ctx.kernel.controls.doc_lists[&hwnd].paths.len() as u32,
        0x100c => {
            let state = &ctx.kernel.controls.doc_lists[&hwnd];
            let start = if w == u32::MAX {
                0
            } else {
                w.saturating_add(1) as usize
            };
            let selected = ctx.kernel.controls.get(hwnd).unwrap().list_selected;
            (start..state.paths.len())
                .find(|index| l & 2 == 0 || selected == Some(*index))
                .map_or(u32::MAX, |i| i as u32)
        }
        _ => return Ok(None),
    };
    Ok(Some(result))
}
pub(super) fn notification(
    ctx: &mut CallCtx<'_>,
    hwnd: u32,
    parent: u32,
    id: u32,
    index: i32,
) -> Option<Option<(u32, u32, u32, u32)>> {
    let path = ctx
        .kernel
        .controls
        .doc_lists
        .get(&hwnd)?
        .paths
        .get(usize::try_from(index).ok()?)?
        .clone();
    let path: Vec<_> = path
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    let key = (hwnd, -1001);
    let required = 48 + path.len() as u32;
    let old = ctx.kernel.window_extra.get(&key).copied();
    let (pointer, replaced) = if let Some(pointer) = old.filter(|pointer| {
        ctx.kernel
            .heap
            .msize(*pointer)
            .is_some_and(|size| size >= required)
    }) {
        (pointer, false)
    } else {
        (ctx.kernel.heap.alloc(required)?, true)
    };
    let mut bytes = Vec::with_capacity(required as usize);
    for word in [hwnd, id, 11000, 0, pointer + 48, 0, 0, 0, 0, 0, 0, 0] {
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend(path);
    if ctx.cpu.write_mem(pointer, &bytes).is_err() {
        if replaced {
            ctx.kernel.heap.free(pointer);
        }
        return Some(None);
    }
    if replaced {
        ctx.kernel.window_extra.insert(key, pointer);
        if let Some(old) = old {
            ctx.kernel.heap.free(old);
        }
    }
    Some(Some((parent, 0x4e, id, pointer)))
}

fn resolve_directory(folder: &str) -> (String, bool) {
    if folder == "All Folders" {
        ("\\My Documents".into(), true)
    } else {
        (file_dialog::join("\\My Documents", folder), false)
    }
}
