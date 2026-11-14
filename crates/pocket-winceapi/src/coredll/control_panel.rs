//! CE6 control.exe discovers CPlApplet through the normal module/resource ABI.
//! Its ROM CPL cannot be relocated as native code. Map only read-only resources
//! and supply applets that operate on the shared, private CE device settings.
use super::*;
use pocket_kernel::system_sound as sound_settings;
use pocket_kernel::{
    ce::ControlPanel,
    controls::{DialogPanel, TabItem},
    registry::RegistryValue,
};

#[derive(Clone, Copy)]
struct Applet {
    name: &'static str,
    description: &'static str,
    icon: u32,
    string: u32,
}
const APPLETS: &[Applet] = &[
    Applet {
        name: "Owner",
        description: "Change owner information.",
        icon: 5040,
        string: 0x8040,
    },
    Applet {
        name: "Display",
        description: "Change display brightness.",
        icon: 5070,
        string: 0x8100,
    },
    Applet {
        name: "Keyboard",
        description: "Change keyboard repeat settings.",
        icon: 5020,
        string: 0x8020,
    },
    Applet {
        name: "Mouse",
        description: "Change double-click speed.",
        icon: 5090,
        string: 0x8150,
    },
    Applet {
        name: "System",
        description: "View system information and identify the device.",
        icon: 5060,
        string: 0x8060,
    },
    Applet {
        name: "Volume & Sounds",
        description: "Choose volume and system sounds.",
        icon: 5110,
        string: 0x8160,
    },
    Applet {
        name: "Input Panel",
        description: "Show or hide the on-screen keyboard.",
        icon: 5140,
        string: 0x8210,
    },
];

pub(super) fn register(d: &mut WinCeDispatcher) {
    for &(dll, _) in pocket_kernel::ce::CPL_MODULES {
        d.register_handler(dll, "CPlApplet", applet);
    }
    d.register_handler("coredll.dll", "GetDoubleClickTime", double_click_time);
    d.register_handler("coredll.dll", "SetDoubleClickTime", set_double_click_time);
}

pub(super) fn load(
    ctx: &mut CallCtx<'_>,
    request: &str,
) -> Result<Option<DispatchOutcome>, KernelError> {
    let name = module_file_name(request);
    let Some((_, hle)) = pocket_kernel::ce::CPL_MODULES
        .iter()
        .find(|(dll, _)| *dll == name)
    else {
        return Ok(None);
    };
    if !ctx.kernel.ce.system_modules.contains(&name) {
        return Ok(None);
    }
    let Some(exports) = ctx.kernel.dynamic_exports.get(hle).cloned() else {
        return Ok(None);
    };
    let Some(path) = ctx.kernel.find_module_file(request) else {
        set_thread_error(ctx, 126);
        return Ok(Some(DispatchOutcome::ReturnedR0(0)));
    };
    let Some(base) = map_runtime_module(ctx, &path, request, true)? else {
        return Ok(Some(DispatchOutcome::ReturnedR0(0)));
    };
    let module = ctx
        .kernel
        .modules
        .iter_mut()
        .find(|m| m.handle == base)
        .unwrap();
    // Keep the normal runtime reference counts and resource addresses, but no
    // ROM instruction, import or DllMain is made executable or called.
    module.data_file = false;
    module.attached = true;
    module.image_entry = 0;
    ctx.kernel.dynamic_exports.insert(base, exports);
    log::info!(
        "Control Panel: {} resources at 0x{base:08x}, {} HLE applets",
        name,
        APPLETS.len()
    );
    Ok(Some(DispatchOutcome::ReturnedR0(base)))
}

fn ret(value: u32) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(value))
}
fn wide_field(bytes: &mut [u8], text: &str) {
    let room = bytes.len() / 2 - 1;
    for (slot, unit) in bytes
        .chunks_exact_mut(2)
        .zip(text.encode_utf16().take(room))
    {
        slot.copy_from_slice(&unit.to_le_bytes());
    }
}
fn module(ctx: &CallCtx<'_>) -> u32 {
    ctx.kernel
        .module_by_name(&ctx.thunk.dll)
        .map_or(0, |m| m.handle)
}
fn label(ctx: &mut CallCtx<'_>, instance: u32, id: u32, fallback: &str) -> String {
    let (entries, base) = ctx.kernel.resource_scope(instance);
    let block = id / 16 + 1;
    let entry = entries
        .iter()
        .find(|e| e.ty == ResourceKey::Id(6) && e.name == ResourceKey::Id(block))
        .cloned();
    let text = entry
        .filter(|e| e.size <= 65536)
        .and_then(|e| ctx.cpu.read_mem(base + e.data_rva, e.size).ok())
        .and_then(|bytes| {
            let mut p = 0;
            for index in 0..16 {
                let n = u16::from_le_bytes(bytes.get(p..p + 2)?.try_into().ok()?) as usize;
                p += 2;
                let raw = bytes.get(p..p + n * 2)?;
                if index == id % 16 {
                    return Some(String::from_utf16_lossy(
                        &raw.chunks_exact(2)
                            .map(|c| u16::from_le_bytes(c.try_into().unwrap()))
                            .collect::<Vec<_>>(),
                    ));
                }
                p += n * 2;
            }
            None
        });
    text.filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback.into())
}
fn applet(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let message = ctx.arg_u32(1)?;
    if message == 1 {
        return ret(1);
    }
    if message == 2 {
        return ret(APPLETS.len() as u32);
    }
    if matches!(message, 6 | 7) {
        return ret(0);
    }
    let index = ctx.arg_u32(2)? as usize;
    let Some(info) = APPLETS.get(index).copied() else {
        return ret(0);
    };
    let pointer = ctx.arg_u32(3)?;
    match message {
        3 => {
            if pointer == 0
                || ctx
                    .cpu
                    .check_guest_access(pointer, 16, Prot::WRITE)
                    .is_err()
            {
                set_thread_error(ctx, 87);
                return ret(0);
            }
            // Dynamic descriptors let the host select localized strings and
            // caller-owned icons from the supplied CE image.
            ctx.cpu.write_mem(
                pointer,
                &[0, 0, 0, index as u32]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )?;
            ret(0)
        }
        8 => {
            if pointer == 0
                || ctx
                    .cpu
                    .check_guest_access(pointer, 468, Prot::WRITE)
                    .is_err()
            {
                set_thread_error(ctx, 87);
                return ret(0);
            }
            let instance = module(ctx);
            let name = label(ctx, instance, info.string, info.name);
            let description = label(ctx, instance, info.string + 1, info.description);
            let icon =
                images::resource_icon(ctx, instance, &ResourceKey::Id(info.icon), false, 32, 32)?
                    .map_or(0, |image| ctx.kernel.gdi.register_icon(image));
            let mut bytes = [0u8; 468];
            for (offset, value) in [(0, 468u32), (12, index as u32), (16, icon)] {
                bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
            wide_field(&mut bytes[20..84], &name);
            wide_field(&mut bytes[84..212], &description);
            ctx.cpu.write_mem(pointer, &bytes)?;
            ret(1)
        }
        5 | 9 | 10 => run(ctx, index, info),
        _ => ret(0),
    }
}

fn text(k: &KernelState, key: &str, name: &str, fallback: &str) -> String {
    match k.registry.value(key, name) {
        Some(RegistryValue::Sz(s)) => s,
        _ => fallback.into(),
    }
}
fn number(k: &KernelState, key: &str, name: &str, fallback: u32) -> u32 {
    match k.registry.value(key, name) {
        Some(RegistryValue::Dword(n)) => n,
        _ => fallback,
    }
}
fn page_titles(index: usize) -> &'static [&'static str] {
    match index {
        0 => &["Identification", "Notes"],
        1 => &["Backlight"],
        2 => &["Repeat"],
        3 => &["Double-Click"],
        4 => &["General", "Memory", "Device Name"],
        5 => &["Volume", "Sounds"],
        _ => &["Input Method"],
    }
}

fn select_page(k: &mut KernelState, state: &mut ControlPanel, index: usize) -> bool {
    if index >= state.pages.len() {
        return false;
    }
    state.selected_page = index;
    for (page, controls) in state.pages.iter().enumerate() {
        for &hwnd in controls {
            if let Some(c) = k.controls.get_mut(hwnd) {
                c.visible = page == index;
                c.style = (c.style & !0x10000000) | if c.visible { 0x10000000 } else { 0 };
            }
        }
    }
    if let Some(c) = k.controls.get_mut(state.tab) {
        c.tab.selected = Some(index);
        c.tab.focus = Some(index);
    }
    if state.applet == 4 && index == 1 {
        let text = memory_summary(k);
        if let Some(c) = state.pages[1].first().and_then(|&h| k.controls.get_mut(h)) {
            c.text = text;
        }
    }
    // A page made visible must immediately lose its previous inherited-hidden
    // state, before focus, hit testing or a guest visibility query runs.
    k.refresh_control_visibility();
    if k.controls.focus != state.tab && !k.controls.is_visible(k.controls.focus).unwrap_or(false) {
        k.controls.focus = state.pages[index]
            .iter()
            .copied()
            .find(|&h| {
                k.controls.get(h).is_some_and(|c| {
                    c.enabled && matches!(c.class, ControlClass::Edit | ControlClass::Button)
                })
            })
            .unwrap_or(state.tab);
    }
    true
}

fn memory_summary(k: &KernelState) -> String {
    if let Some(division) = &k.memory_division {
        let snapshot = division.snapshot();
        let page = division.page_size();
        return format!(
            "Program memory: {} KB\nAvailable: {} KB\nStorage memory: {} KB\nAvailable: {} KB",
            snapshot.ram_pages() * page / 1024,
            snapshot.program_free() * page / 1024,
            snapshot.store_pages * page / 1024,
            snapshot.store_free() * page / 1024,
        );
    }
    format!(
        "Program memory: {} KB\nAvailable: {} KB",
        k.heap.capacity_bytes() / 1024,
        k.heap.physical_free_bytes() / 1024,
    )
}

fn begin(ctx: &mut CallCtx<'_>, index: usize, info: Applet) -> Result<ControlPanel, KernelError> {
    let frame = GuestCallFrame {
        args: [
            ctx.arg_u32(0)?,
            ctx.arg_u32(1)?,
            ctx.arg_u32(2)?,
            ctx.arg_u32(3)?,
        ],
        sp: ctx.cpu.read_reg(ArmReg::Sp)?,
        lr: ctx.cpu.read_reg(ArmReg::Lr)?,
    };
    let hwnd = (0xdead2100..0xdead2200)
        .find(|h| !ctx.kernel.window_classes.contains_key(h))
        .ok_or_else(|| KernelError::Loader("Control Panel window handles exhausted".into()))?;
    let (sw, sh) = (
        ctx.kernel.framebuffer.width as i32,
        ctx.kernel.framebuffer.height as i32 - 26,
    );
    let (w, h) = (sw.min(440), sh.min(296));
    let (x, y) = ((sw - w) / 2, (sh - h) / 2);
    let instance = module(ctx);
    let caption = label(ctx, instance, info.string + 2, info.name);
    let previous_active = ctx.kernel.active_window;
    let previous_focus = ctx.kernel.controls.focus;
    ctx.kernel.controls.add_panel(DialogPanel {
        hwnd,
        x,
        y,
        w,
        h,
        visible: true,
        border: true,
        caption: Some(caption),
        ok_pressed: false,
        caption_height: None,
        caption_command: Some(2),
        caption_help: false,
    });
    ctx.kernel.window_classes.insert(hwnd, "#32770".into());
    ctx.kernel.window_rects.insert(hwnd, (x, y, w, h));
    ctx.kernel.window_styles.insert(hwnd, (0x90000000, 0));
    ctx.kernel.window_parents.insert(hwnd, frame.args[0]);
    ctx.kernel
        .window_threads
        .insert(hwnd, ctx.kernel.current_thread);
    ctx.kernel.window_visibility.insert(hwnd, true);
    ctx.kernel.active_window = hwnd;
    let mut fields = Vec::new();
    let titles = page_titles(index);
    let mut pages = vec![Vec::new(); titles.len()];
    let bottom = h - DialogPanel::CAPTION_H - 2;
    let page_bottom = bottom - 74;
    let tab = ctx.kernel.controls.create(
        hwnd,
        ControlClass::Tab,
        11,
        String::new(),
        0x50010000,
        10,
        8,
        w - 22,
        page_bottom - 8,
    );
    ctx.kernel.controls.get_mut(tab).unwrap().tab.items = titles
        .iter()
        .map(|title| TabItem {
            text: (*title).into(),
            image: -1,
            ..Default::default()
        })
        .collect();
    ctx.kernel
        .window_threads
        .insert(tab, ctx.kernel.current_thread);
    if index == 4 {
        let version = ctx.kernel.ce.version.unwrap_or((6, 0, 0));
        let description = format!(
            "Windows CE {}.{} (Build {})\nDisplay: {} x {}",
            version.0,
            version.1,
            version.2,
            sw,
            sh + 26
        );
        for (page, text) in [(0, description), (1, memory_summary(ctx.kernel))] {
            let child = ctx.kernel.controls.create(
                hwnd,
                ControlClass::Static,
                100 + page as u32,
                text,
                0x50000000,
                20,
                44,
                w - 42,
                page_bottom - 50,
            );
            pages[page].push(child);
        }
    }
    let mut values: Vec<(&str, String, bool)> = match index {
        0 => [
            ("Name", "Owner"),
            ("Company", "Company"),
            ("Telephone", "Phone"),
            ("E-mail", "Email"),
        ]
        .into_iter()
        .map(|(label, name)| {
            (
                label,
                text(ctx.kernel, r"HKCU\ControlPanel\Owner", name, ""),
                false,
            )
        })
        .collect(),
        1 => vec![(
            "Brightness (1-31)",
            number(ctx.kernel, r"HKCU\ControlPanel\Backlight", "OnLevel", 11).to_string(),
            false,
        )],
        2 => vec![
            (
                "Initial delay (ms)",
                number(
                    ctx.kernel,
                    r"HKCU\ControlPanel\Keyboard",
                    "InitialDelay",
                    400,
                )
                .to_string(),
                false,
            ),
            (
                "Repeat interval (ms)",
                number(ctx.kernel, r"HKCU\ControlPanel\Keyboard", "RepeatRate", 80).to_string(),
                false,
            ),
        ],
        3 => vec![(
            "Double-click (ms)",
            number(
                ctx.kernel,
                r"HKCU\ControlPanel\Pen",
                "DoubleTapTimeout",
                500,
            )
            .to_string(),
            false,
        )],
        4 => vec![
            (
                "Device name",
                text(ctx.kernel, r"HKLM\Ident", "Name", "PocketHLE"),
                false,
            ),
            (
                "Description",
                text(ctx.kernel, r"HKLM\Ident", "Desc", "Windows CE device"),
                false,
            ),
        ],
        5 => vec![
            (
                "Volume (0-100)",
                ((number(ctx.kernel, r"HKCU\ControlPanel\Volume", "Volume", u32::MAX) & 0xffff)
                    * 100
                    / 65535)
                    .to_string(),
                false,
            ),
            (
                "Enable application sounds",
                (number(ctx.kernel, r"HKCU\ControlPanel\Volume", "Mute", 7) & 2 != 0).to_string(),
                true,
            ),
            (
                "Enable system events",
                (number(ctx.kernel, r"HKCU\ControlPanel\Volume", "Mute", 7) & 4 != 0).to_string(),
                true,
            ),
            (
                "Sound scheme",
                sound_settings::current_scheme(&ctx.kernel.registry),
                false,
            ),
        ],
        _ => vec![(
            "Show on-screen keyboard",
            (number(ctx.kernel, r"HKCU\ControlPanel\SIP", "Visible", 0) != 0).to_string(),
            true,
        )],
    };
    if index == 0 {
        values.push((
            "Notes",
            text(ctx.kernel, r"HKCU\ControlPanel\Owner", "Notes", ""),
            false,
        ));
    }
    let field_pages: Vec<usize> = match index {
        0 => vec![0, 0, 0, 0, 1],
        4 => vec![2, 2],
        5 => vec![0, 1, 1, 1],
        _ => vec![0; values.len()],
    };
    let sound_schemes = if index == 5 {
        sound_settings::schemes(&ctx.kernel.registry)
    } else {
        Vec::new()
    };
    let mut rows = vec![0; pages.len()];
    for (i, (label, value, check)) in values.into_iter().enumerate() {
        let page = field_pages[i];
        let count = field_pages.iter().filter(|&&p| p == page).count() as i32;
        let content_bottom = page_bottom - if index == 5 && page == 1 { 32 } else { 0 };
        let step = ((content_bottom - 44) / count.max(1)).clamp(14, 32);
        let y = 40 + rows[page] * step;
        rows[page] += 1;
        let handle = if check {
            let handle = ctx.kernel.controls.create(
                hwnd,
                ControlClass::Button,
                20 + i as u32,
                label.into(),
                0x50010003,
                20,
                y,
                w - 42,
                (step - 2).min(24),
            );
            ctx.kernel.controls.get_mut(handle).unwrap().checked = value == "true";
            handle
        } else {
            let notes = index == 0 && i == 4;
            let label_handle = ctx.kernel.controls.create(
                hwnd,
                ControlClass::Static,
                40 + i as u32,
                label.into(),
                0x50000000,
                20,
                y + 3,
                if notes { w - 42 } else { (w / 2 - 20).max(1) },
                (step - 2).min(22),
            );
            pages[page].push(label_handle);
            let combo = index == 5 && i == 3;
            let handle = ctx.kernel.controls.create(
                hwnd,
                if combo {
                    ControlClass::ComboBox
                } else {
                    ControlClass::Edit
                },
                20 + i as u32,
                value,
                if combo {
                    0x50810003
                } else if notes {
                    0x50811044
                } else {
                    0x50810080
                },
                if notes { 20 } else { w / 2 },
                if notes { y + 22 } else { y },
                if notes { w - 42 } else { w / 2 - 22 },
                if notes {
                    page_bottom - y - 30
                } else {
                    (step - 2).min(25)
                },
            );
            if combo {
                let control = ctx.kernel.controls.get_mut(handle).unwrap();
                let selected = control.text.clone();
                control.list_items = sound_schemes
                    .iter()
                    .enumerate()
                    .map(|(index, name)| {
                        let label = match name.to_ascii_lowercase().as_str() {
                            ".defaultsounds" => "Default sounds",
                            ".allsounds" => "All sounds",
                            ".nosounds" => "No sounds",
                            _ => name,
                        };
                        (label.to_owned(), index as u32)
                    })
                    .collect();
                control.list_selected = sound_schemes
                    .iter()
                    .position(|name| name.eq_ignore_ascii_case(&selected));
                if let Some(selection) = control.list_selected {
                    control.text = control.list_items[selection].0.clone();
                }
            }
            handle
        };
        ctx.kernel
            .window_threads
            .insert(handle, ctx.kernel.current_thread);
        fields.push(handle);
        pages[page].push(handle);
    }
    if index == 5 {
        let test = ctx.kernel.controls.create(
            hwnd,
            ControlClass::Button,
            4,
            "Test sound".into(),
            0x50010000,
            20,
            page_bottom - 31,
            120,
            25,
        );
        pages[1].push(test);
    }
    let status = ctx.kernel.controls.create(
        hwnd,
        ControlClass::Static,
        50,
        String::new(),
        0x50000000,
        12,
        bottom - 66,
        w - 26,
        28,
    );
    for (i, (id, label)) in [(1, "OK"), (2, "Cancel"), (3, "Apply")]
        .into_iter()
        .enumerate()
    {
        ctx.kernel.controls.create(
            hwnd,
            ControlClass::Button,
            id,
            label.into(),
            0x50010000,
            (w - 258).max(8) + i as i32 * 82,
            bottom - 34,
            76,
            26,
        );
    }
    let mut state = ControlPanel {
        hwnd,
        frame,
        applet: index,
        thread: ctx.kernel.current_thread,
        thunk: ctx.thunk.thunk_va,
        previous_active,
        previous_focus,
        fields,
        sound_schemes,
        tab,
        pages,
        selected_page: 0,
        status,
    };
    ctx.kernel.controls.focus = 0;
    select_page(ctx.kernel, &mut state, 0);
    system_sound::play_event(ctx.kernel, sound_settings::SystemEvent::Open);
    repaint_controls(ctx);
    ctx.kernel.framebuffer.mark_dirty();
    Ok(state)
}
fn field_values(k: &KernelState, state: &ControlPanel) -> Vec<(String, bool)> {
    state
        .fields
        .iter()
        .map(|&h| {
            k.controls
                .get(h)
                .map(|c| (c.text.clone(), c.checked))
                .unwrap_or_default()
        })
        .collect()
}
fn sound_form(
    k: &KernelState,
    state: &ControlPanel,
    values: &[(String, bool)],
) -> Result<(u32, u32, String), String> {
    let volume = values
        .first()
        .and_then(|v| v.0.trim().parse::<u32>().ok())
        .filter(|v| *v <= 100)
        .ok_or("Enter a value from 0 to 100.")?
        * 65535
        / 100;
    let applications = values.get(1).ok_or("Missing application sound setting.")?.1;
    let events = values.get(2).ok_or("Missing system event setting.")?.1;
    let choice = state
        .fields
        .get(3)
        .and_then(|h| k.controls.get(*h))
        .and_then(|c| c.list_selected)
        .and_then(|i| state.sound_schemes.get(i))
        .ok_or("Choose a sound scheme.")?;
    let mute = number(k, sound_settings::VOLUME_KEY, "Mute", 7);
    Ok((
        volume | volume << 16,
        (mute & !6) | if applications { 2 } else { 0 } | if events { 4 } else { 0 },
        choice.clone(),
    ))
}
fn apply(k: &mut KernelState, state: &ControlPanel) -> Result<(), String> {
    let old_device_name = text(k, r"HKLM\Ident", "Name", "");
    let values = field_values(k, state);
    let numeric = |i: usize, min, max| -> Result<u32, String> {
        values[i]
            .0
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|n| (min..=max).contains(n))
            .ok_or_else(|| format!("Enter a value from {min} to {max}."))
    };
    let mut changes = Vec::new();
    match state.applet {
        0 => {
            for (i, name) in ["Owner", "Company", "Phone", "Email"]
                .into_iter()
                .enumerate()
            {
                if values[i].0.encode_utf16().count() > 255 {
                    return Err("Owner information is too long.".into());
                }
                changes.push((
                    r"HKCU\ControlPanel\Owner",
                    name,
                    RegistryValue::Sz(values[i].0.clone()),
                ));
            }
            if let Some((notes, _)) = values.get(4) {
                if notes.encode_utf16().count() > 4096 {
                    return Err("Owner notes are too long.".into());
                }
                changes.push((
                    r"HKCU\ControlPanel\Owner",
                    "Notes",
                    RegistryValue::Sz(notes.clone()),
                ));
            }
        }
        1 => {
            let level = numeric(0, 1, 31)?;
            for name in ["Level", "OnLevel"] {
                changes.push((
                    r"HKCU\ControlPanel\Backlight",
                    name,
                    RegistryValue::Dword(level),
                ));
            }
        }
        2 => {
            changes.push((
                r"HKCU\ControlPanel\Keyboard",
                "InitialDelay",
                RegistryValue::Dword(numeric(0, 100, 2000)?),
            ));
            changes.push((
                r"HKCU\ControlPanel\Keyboard",
                "RepeatRate",
                RegistryValue::Dword(numeric(1, 10, 1000)?),
            ));
        }
        3 => changes.push((
            r"HKCU\ControlPanel\Pen",
            "DoubleTapTimeout",
            RegistryValue::Dword(numeric(0, 100, 1000)?),
        )),
        4 => {
            if values[0].0.is_empty()
                || values[0].0.len() > 255
                || !values[0]
                    .0
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err("Use letters, digits, - or _ for the device name.".into());
            }
            if values[1].0.encode_utf16().count() > 255 {
                return Err("The description is too long.".into());
            }
            for (i, name) in ["Name", "Desc"].into_iter().enumerate() {
                changes.push((r"HKLM\Ident", name, RegistryValue::Sz(values[i].0.clone())));
            }
        }
        5 => {
            let (volume, mute, scheme) = sound_form(k, state, &values)?;
            changes.push((
                r"HKCU\ControlPanel\Volume",
                "Volume",
                RegistryValue::Dword(volume),
            ));
            changes.push((
                r"HKCU\ControlPanel\Volume",
                "Mute",
                RegistryValue::Dword(mute),
            ));
            changes.push((
                sound_settings::EVENT_KEY,
                ".Scheme",
                RegistryValue::Sz(scheme),
            ));
        }
        _ => changes.push((
            r"HKCU\ControlPanel\SIP",
            "Visible",
            RegistryValue::Dword(u32::from(values[0].1)),
        )),
    }
    // Validate the whole form before applying it. Save to the profile registry,
    // which is shared by Explorer and every independently running child.
    for (key, name, value) in changes {
        if !k.registry.set_value(key, name, value) {
            return Err("Not enough device storage to save settings.".into());
        }
    }
    k.registry
        .flush()
        .map_err(|error| format!("Cannot save settings: {error}"))?;
    k.sync_backlight();
    k.sync_ce_settings();
    if state.applet == 4 && text(k, r"HKLM\Ident", "Name", "") != old_device_name {
        k.object_handles.emit_app_event(13);
    }
    Ok(())
}

fn finish(ctx: &mut CallCtx<'_>, state: ControlPanel) -> Result<DispatchOutcome, KernelError> {
    system_sound::play_event(ctx.kernel, sound_settings::SystemEvent::Close);
    discard_window_metadata(ctx, &state.hwnd);
    ctx.kernel.active_window = state.previous_active;
    ctx.kernel.controls.focus = state.previous_focus;
    for (i, value) in state.frame.args.into_iter().enumerate() {
        ctx.cpu.write_arg(i as u8, value)?;
    }
    ctx.cpu.write_reg(ArmReg::Sp, state.frame.sp)?;
    ctx.cpu.write_reg(ArmReg::Lr, state.frame.lr)?;
    queue_visible_window_paints(ctx.kernel, state.previous_active);
    repaint_controls(ctx);
    ctx.kernel.framebuffer.mark_dirty();
    ret(u32::from(matches!(state.frame.args[1], 9 | 10)))
}
fn run(ctx: &mut CallCtx<'_>, index: usize, info: Applet) -> Result<DispatchOutcome, KernelError> {
    let mut state = if let Some(state) = ctx.kernel.ce.control_panel.take() {
        if state.thread != ctx.kernel.current_thread
            || state.thunk != ctx.thunk.thunk_va
            || state.frame.sp != ctx.cpu.read_reg(ArmReg::Sp)?
        {
            ctx.kernel.ce.control_panel = Some(state);
            return ret(0);
        }
        state
    } else {
        begin(ctx, index, info)?
    };
    crate::shell_keyboard::service(ctx);
    while let Some(event) = take_pending_input(ctx) {
        update_key_state(ctx, event);
        if crate::shell_keyboard::input(ctx, event) {
            continue;
        }
        let action = match event {
            InputEvent::PointerDown { x, y } => {
                ctx.kernel
                    .controls
                    .pointer_down_in(state.hwnd, i32::from(x), i32::from(y))
            }
            InputEvent::PointerUp { x, y } => {
                ctx.kernel
                    .controls
                    .pointer_up_in(state.hwnd, i32::from(x), i32::from(y))
            }
            InputEvent::Text { character } => {
                let focus = ctx.kernel.controls.focus;
                control_message(ctx, focus, 0x102, character as u32, 1)?;
                None
            }
            InputEvent::KeyDown { vk }
                if matches!(vk, 0x1b | 0x0d)
                    && ctx
                        .kernel
                        .controls
                        .get(ctx.kernel.controls.focus)
                        .is_some_and(|c| {
                            c.parent == state.hwnd
                                && c.class == ControlClass::ComboBox
                                && c.choice_open
                        }) =>
            {
                ctx.kernel.controls.key_down_in(state.hwnd, vk)
            }
            InputEvent::KeyDown { vk: 0x1b } => Some(ControlAction::Clicked {
                parent: state.hwnd,
                hwnd: 0,
                id: 2,
            }),
            InputEvent::KeyDown { vk: 0x09 } if ctx.kernel.pressed_keys[0x11] => {
                let next = if ctx.kernel.pressed_keys[0x10] {
                    (state.selected_page + state.pages.len() - 1) % state.pages.len()
                } else {
                    (state.selected_page + 1) % state.pages.len()
                };
                select_page(ctx.kernel, &mut state, next);
                None
            }
            InputEvent::KeyDown { vk: 0x0d }
                if ctx
                    .kernel
                    .controls
                    .get(ctx.kernel.controls.focus)
                    .is_some_and(|c| c.class == ControlClass::Edit && c.style & 4 != 0) =>
            {
                let focus = ctx.kernel.controls.focus;
                control_message(ctx, focus, 0x102, 13, 1)?;
                None
            }
            InputEvent::KeyDown { vk: 0x0d } => Some(ControlAction::Clicked {
                parent: state.hwnd,
                hwnd: 0,
                id: 1,
            }),
            InputEvent::KeyDown { vk: 0x08 }
                if ctx
                    .kernel
                    .controls
                    .get(ctx.kernel.controls.focus)
                    .is_some_and(|c| c.class == ControlClass::Edit) =>
            {
                let focus = ctx.kernel.controls.focus;
                control_message(ctx, focus, 0x102, 8, 1)?;
                None
            }
            InputEvent::KeyDown { vk }
                if vk != 0x09
                    && ctx
                        .kernel
                        .controls
                        .get(ctx.kernel.controls.focus)
                        .is_some_and(|c| c.class == ControlClass::Edit) =>
            {
                // Text/IME supplies printable characters separately. Applying
                // both a physical key and Text doubled CPL field input in the GUI.
                None
            }
            InputEvent::KeyDown { vk } => ctx.kernel.controls.key_down_in(state.hwnd, vk),
            _ => None,
        };
        if let Some(ControlAction::Notify {
            hwnd, code, index, ..
        }) = action
        {
            if hwnd == state.tab && code == (-552i32) as u32 && index >= 0 {
                select_page(ctx.kernel, &mut state, index as usize);
            }
        }
        if let Some(ControlAction::Clicked { id, .. }) = action {
            if id == 4 && state.applet == 5 {
                let message =
                    match sound_form(ctx.kernel, &state, &field_values(ctx.kernel, &state)) {
                        Ok((volume, mute, scheme)) => {
                            let mut registry = ctx.kernel.registry.isolated_copy();
                            registry.set_value(
                                sound_settings::VOLUME_KEY,
                                "Volume",
                                RegistryValue::Dword(volume),
                            );
                            registry.set_value(
                                sound_settings::VOLUME_KEY,
                                "Mute",
                                RegistryValue::Dword(mute),
                            );
                            registry.set_value(
                                sound_settings::EVENT_KEY,
                                ".Scheme",
                                RegistryValue::Sz(scheme),
                            );
                            if system_sound::preview(ctx.kernel, &registry) {
                                "Sound test complete.".into()
                            } else {
                                "Cannot play the sound from the selected image.".into()
                            }
                        }
                        Err(error) => error,
                    };
                ctx.kernel.controls.get_mut(state.status).unwrap().text = message;
            }
            if id == 2 {
                return finish(ctx, state);
            }
            if matches!(id, 1 | 3) {
                match apply(ctx.kernel, &state) {
                    Ok(()) if id == 1 => return finish(ctx, state),
                    Ok(()) => {
                        ctx.kernel.controls.get_mut(state.status).unwrap().text =
                            "Settings saved.".into()
                    }
                    Err(error) => ctx.kernel.controls.get_mut(state.status).unwrap().text = error,
                }
            }
        }
        repaint_controls(ctx);
        ctx.kernel.framebuffer.mark_dirty();
    }
    ctx.kernel.ce.control_panel = Some(state);
    if let Some(outcome) = wait_display_until(
        ctx,
        std::time::Instant::now() + std::time::Duration::from_millis(1),
    )? {
        return Ok(outcome);
    }
    Ok(DispatchOutcome::JumpTo(ctx.thunk.thunk_va))
}
fn double_click_time(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    ret(number(
        ctx.kernel,
        r"HKCU\ControlPanel\Pen",
        "DoubleTapTimeout",
        500,
    )
    .clamp(1, 5000))
}
fn set_double_click_time(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let time = ctx.arg_u32(0)?;
    let time = if time == 0 { 500 } else { time.min(5000) };
    let saved = ctx.kernel.registry.set_value(
        r"HKCU\ControlPanel\Pen",
        "DoubleTapTimeout",
        RegistryValue::Dword(time),
    );
    if saved {
        ctx.kernel.controls.double_click_time_ms = u64::from(time);
    }
    ctx.kernel.sync_ce_settings();
    ret(u32::from(saved))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(ctx: &mut CallCtx<'_>, values: &[u32]) {
        for (i, &value) in values.iter().enumerate() {
            ctx.cpu.write_arg(i as u8, value).unwrap();
        }
    }

    #[test]
    fn owner_tabs_keep_edits_apply_hidden_fields_and_cancel_without_discarding_saved_values() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut kernel = super::super::tests::fresh_kernel();
        kernel.active_window = 0x111;
        kernel.controls.focus = 0x222;
        let thunk = super::super::tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        args(&mut ctx, &[0x111, 5, 0, 0]);
        let mut state = begin(&mut ctx, 0, APPLETS[0]).unwrap();
        assert_eq!(state.pages.len(), 2);
        assert_eq!(
            ctx.kernel.controls.get(state.tab).unwrap().tab.items[1].text,
            "Notes"
        );
        let name = state.fields[0];
        let notes = state.fields[4];
        ctx.kernel.controls.get_mut(name).unwrap().text = "Alice".into();
        assert_eq!(ctx.kernel.controls.is_visible(notes), Some(false));
        assert!(select_page(ctx.kernel, &mut state, 1));
        assert_eq!(ctx.kernel.controls.is_visible(name), Some(false));
        assert_eq!(ctx.kernel.controls.is_visible(notes), Some(true));
        assert_eq!(ctx.kernel.controls.focus, notes);
        ctx.kernel.controls.get_mut(notes).unwrap().text = "First line\nSecond line".into();
        ctx.kernel.ce.control_panel = Some(state);
        ctx.kernel.pending_input.extend([
            InputEvent::KeyDown { vk: 0x0d },
            InputEvent::KeyUp { vk: 0x0d },
            InputEvent::KeyDown { vk: 0x41 },
            InputEvent::Text { character: 'A' },
            InputEvent::KeyUp { vk: 0x41 },
        ]);
        run(&mut ctx, 0, APPLETS[0]).unwrap();
        let state = ctx.kernel.ce.control_panel.take().unwrap();
        assert_eq!(
            ctx.kernel.controls.get(notes).unwrap().text,
            "First line\nSecond line\r\nA"
        );
        ctx.kernel.ce.control_panel = Some(state);
        ctx.kernel.pending_input.extend([
            InputEvent::KeyDown { vk: 0x08 },
            InputEvent::KeyUp { vk: 0x08 },
            InputEvent::KeyDown { vk: 0x08 },
            InputEvent::KeyUp { vk: 0x08 },
        ]);
        run(&mut ctx, 0, APPLETS[0]).unwrap();
        let mut state = ctx.kernel.ce.control_panel.take().unwrap();
        assert_eq!(
            ctx.kernel.controls.get(notes).unwrap().text,
            "First line\nSecond line"
        );
        ctx.kernel.controls.get_mut(notes).unwrap().text = "First line\nSecond line".into();
        assert!(select_page(ctx.kernel, &mut state, 0));
        assert_eq!(
            ctx.kernel.controls.get(notes).unwrap().text,
            "First line\nSecond line"
        );
        apply(ctx.kernel, &state).unwrap();
        assert_eq!(
            ctx.kernel
                .registry
                .value(r"HKCU\ControlPanel\Owner", "Owner"),
            Some(RegistryValue::Sz("Alice".into()))
        );
        assert_eq!(
            ctx.kernel
                .registry
                .value(r"HKCU\ControlPanel\Owner", "Notes"),
            Some(RegistryValue::Sz("First line\nSecond line".into()))
        );
        ctx.kernel.controls.get_mut(name).unwrap().text = "Unsaved".into();
        assert!(select_page(ctx.kernel, &mut state, 1));
        ctx.kernel.controls.get_mut(notes).unwrap().text = "Unsaved notes".into();
        let tab = state.tab;
        finish(&mut ctx, state).unwrap();
        assert_eq!(ctx.kernel.active_window, 0x111);
        assert_eq!(ctx.kernel.controls.focus, 0x222);
        assert!(ctx.kernel.controls.get(tab).is_none());
        assert!(ctx.kernel.controls.get(notes).is_none());
        assert_eq!(
            ctx.kernel
                .registry
                .value(r"HKCU\ControlPanel\Owner", "Owner"),
            Some(RegistryValue::Sz("Alice".into()))
        );
        assert_eq!(
            ctx.kernel
                .registry
                .value(r"HKCU\ControlPanel\Owner", "Notes"),
            Some(RegistryValue::Sz("First line\nSecond line".into()))
        );
    }

    #[test]
    fn applet_pointer_and_keyboard_switch_tabs_and_save_all_pages() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut kernel = super::super::tests::fresh_kernel();
        let thunk = super::super::tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        args(&mut ctx, &[0, 5, 5, 0]);
        let state = begin(&mut ctx, 5, APPLETS[5]).unwrap();
        let volume = state.fields[0];
        let sounds = state.fields[1];
        ctx.kernel.controls.get_mut(volume).unwrap().text = "50".into();
        ctx.kernel.controls.get_mut(sounds).unwrap().checked = false;
        let c = ctx.kernel.controls.get(state.tab).unwrap();
        let hit = (0..c.w).find(|&x| c.tab_hit(x, 12) == Some(1)).unwrap();
        let (x, y, _, _) = ctx.kernel.controls.screen_rect(state.tab).unwrap();
        let (x, y) = ((x + hit + 2) as u16, (y + 12) as u16);
        ctx.kernel.ce.control_panel = Some(state);
        ctx.kernel.pending_input.extend([
            InputEvent::PointerDown { x, y },
            InputEvent::PointerUp { x, y },
        ]);
        run(&mut ctx, 5, APPLETS[5]).unwrap();
        let state = ctx.kernel.ce.control_panel.as_ref().unwrap();
        assert_eq!(state.selected_page, 1);
        assert_eq!(ctx.kernel.controls.is_visible(volume), Some(false));
        assert_eq!(ctx.kernel.controls.is_visible(sounds), Some(true));
        ctx.kernel.pending_input.extend([
            InputEvent::KeyDown { vk: 0x11 },
            InputEvent::KeyDown { vk: 0x09 },
            InputEvent::KeyUp { vk: 0x09 },
            InputEvent::KeyUp { vk: 0x11 },
        ]);
        run(&mut ctx, 5, APPLETS[5]).unwrap();
        let mut state = ctx.kernel.ce.control_panel.take().unwrap();
        assert_eq!(state.selected_page, 0);
        assert!(select_page(ctx.kernel, &mut state, 1));
        apply(ctx.kernel, &state).unwrap();
        let expected = 32767u32 | 32767u32 << 16;
        assert_eq!(
            ctx.kernel
                .registry
                .value(r"HKCU\ControlPanel\Volume", "Volume"),
            Some(RegistryValue::Dword(expected))
        );
        assert_eq!(
            ctx.kernel
                .registry
                .value(r"HKCU\ControlPanel\Volume", "Mute"),
            Some(RegistryValue::Dword(5))
        );
        finish(&mut ctx, state).unwrap();
    }

    #[test]
    fn system_pages_show_real_memory_and_keep_device_edits_off_read_only_pages() {
        let mut cpu = super::super::tests::scanf_cpu();
        let mut kernel = super::super::tests::fresh_kernel();
        let thunk = super::super::tests::dummy_thunk();
        let mut ctx = CallCtx {
            cpu: &mut cpu,
            kernel: &mut kernel,
            thunk: &thunk,
        };
        args(&mut ctx, &[0, 5, 4, 0]);
        let mut state = begin(&mut ctx, 4, APPLETS[4]).unwrap();
        assert_eq!(state.pages.len(), 3);
        assert_eq!(ctx.kernel.controls.is_visible(state.fields[0]), Some(false));
        let memory = state.pages[1][0];
        let before = ctx.kernel.controls.get(memory).unwrap().text.clone();
        ctx.kernel.heap.alloc(1024).unwrap();
        select_page(ctx.kernel, &mut state, 1);
        assert_ne!(ctx.kernel.controls.get(memory).unwrap().text, before);
        assert_eq!(
            ctx.kernel.controls.get(memory).unwrap().text,
            memory_summary(ctx.kernel)
        );
        select_page(ctx.kernel, &mut state, 2);
        let field = state.fields[0];
        assert_eq!(ctx.kernel.controls.is_visible(field), Some(true));
        assert_eq!(ctx.kernel.controls.focus, state.tab);
        let (x, y, _, _) = ctx.kernel.controls.screen_rect(field).unwrap();
        ctx.kernel
            .controls
            .pointer_down_in(state.hwnd, x + 2, y + 2);
        assert_eq!(ctx.kernel.controls.focus, field);
        assert!(!select_page(ctx.kernel, &mut state, 3));
        assert_eq!(state.selected_page, 2);
        finish(&mut ctx, state).unwrap();
    }

    #[test]
    fn control_panel_validates_before_saving_and_updates_shared_audio_and_registry() {
        let mut kernel = super::super::tests::fresh_kernel();
        kernel.ce.version = Some((6, 0, 0));
        let volume = kernel.controls.create(
            1,
            ControlClass::Edit,
            20,
            "101".into(),
            0x50000000,
            0,
            0,
            100,
            20,
        );
        let enabled = kernel.controls.create(
            1,
            ControlClass::Button,
            21,
            String::new(),
            0x50000003,
            0,
            30,
            100,
            20,
        );
        kernel.controls.get_mut(enabled).unwrap().checked = true;
        let events = kernel.controls.create(
            1,
            ControlClass::Button,
            22,
            String::new(),
            0x50000003,
            0,
            60,
            100,
            20,
        );
        kernel.controls.get_mut(events).unwrap().checked = true;
        let scheme = kernel.controls.create(
            1,
            ControlClass::ComboBox,
            23,
            String::new(),
            0x50000003,
            0,
            90,
            100,
            20,
        );
        kernel.controls.get_mut(scheme).unwrap().list_selected = Some(1);
        let state = ControlPanel {
            hwnd: 1,
            thread: 0,
            thunk: 0,
            frame: GuestCallFrame {
                args: [0; 4],
                sp: 0,
                lr: 0,
            },
            applet: 5,
            previous_active: 0,
            previous_focus: 0,
            fields: vec![volume, enabled, events, scheme],
            sound_schemes: vec![
                ".DefaultSounds".into(),
                ".AllSounds".into(),
                ".NoSounds".into(),
            ],
            tab: 0,
            pages: vec![vec![volume], vec![enabled, events, scheme]],
            selected_page: 0,
            status: 0,
        };
        let (child, _) = kernel.object_handles.new_child().unwrap();
        let mut other = super::super::tests::fresh_kernel();
        other.attach_handle_context(kernel.child_handle_context(child).unwrap());
        other.ce.version = Some((6, 0, 0));
        assert!(apply(&mut kernel, &state).is_err());
        assert_eq!(
            other.registry.value(r"HKCU\ControlPanel\Volume", "Volume"),
            None
        );
        kernel.controls.get_mut(volume).unwrap().text = "0".into();
        apply(&mut kernel, &state).unwrap();
        assert_eq!(
            other.registry.value(sound_settings::EVENT_KEY, ".Scheme"),
            Some(RegistryValue::Sz(".AllSounds".into()))
        );
        assert_eq!(
            other.registry.value(r"HKCU\ControlPanel\Volume", "Volume"),
            Some(RegistryValue::Dword(0))
        );
        assert_eq!(kernel.audio.wave_volume(0), Some(0));
        other.sync_ce_settings();
        assert_eq!(other.audio.wave_volume(0), Some(0));
        kernel.controls.get_mut(volume).unwrap().text = "100".into();
        kernel.controls.get_mut(enabled).unwrap().checked = false;
        apply(&mut kernel, &state).unwrap();
        assert_eq!(kernel.audio.wave_volume(0), Some(0));
        assert_eq!(
            other.registry.value(r"HKCU\ControlPanel\Volume", "Volume"),
            Some(RegistryValue::Dword(u32::MAX))
        );
    }
}
