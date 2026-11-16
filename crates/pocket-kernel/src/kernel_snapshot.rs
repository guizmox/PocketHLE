//! Process-owned WinCE data for a detached ARM checkpoint.

use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct KernelSnapshot {
    heap: HeapSnapshot,
    vfs: vfs::VfsFileSnapshot,
    mappings: Option<file_mapping::MappingSnapshot>,
    handles: handles::HandleSnapshot,
    registry: registry::RegistryProcessSnapshot,
    audio: audio::AudioSnapshot,
    gdi: gdi::GdiSnapshot,
    display: framebuffer::DisplaySnapshot,
    schedule: ScheduleSnapshot,
    zlib_allocator: u32,
    memory_profile: Option<(u32, u32)>,
    module_path: String,
    process_launch_enabled: bool,
    command_line_cache: Option<u32>,
    command_line: Option<String>,
    next_process_id: u32,
    random_seed: u32,
    resources: Vec<ResourceEntry>,
    image_base: u32,
    image_size: u32,
    image_entry: u32,
    dynamic_exports: HashMap<u32, HashMap<String, u32>>,
    image_imports: Vec<pocket_pe::ImportSymbol>,
    gles_local_ordinals: HashMap<String, HashMap<u16, String>>,
    runtime_thunks: HashMap<u32, Thunk>,
    module_attach_frames: Vec<ModuleAttachFrame>,
    static_module_roots: Vec<u32>,
    dll_notification_frame: Option<dll_lifecycle::DllNotificationFrame>,
    next_module_handle: u32,
    modules: Vec<LoadedModule>,
    next_module_base: u32,
    free_module_bases: Vec<u32>,
    fb_mapped: bool,
    guest_fb_pitch: u32,
    direct_fb_frames: u64,
    gapi_column_major: bool,
    gx_last_pushed_counter: u64,
    gx_draw_active: bool,
    gx_draw_thread: usize,
    gx_explicit_present: bool,
    gx_guest_signature: Option<u64>,
    gles_strings: std::collections::HashMap<String, u32>,
    wnd_proc: u32,
    window_class_procs: HashMap<String, u32>,
    window_background: Option<u32>,
    window_background_erase_pending: bool,
    pending_create: Option<(u32, u32)>,
    pending_startup: std::collections::VecDeque<(u32, u32, u32)>,
    find_handles: HashMap<u32, std::collections::VecDeque<(String, u64, bool, u32)>>,
    next_find_handle: u32,
    window_procs: HashMap<u32, u32>,
    window_userdata: HashMap<u32, u32>,
    window_classes: HashMap<u32, String>,
    window_titles: HashMap<u32, String>,
    window_class_controls: HashMap<String, controls::ControlClass>,
    control_class_prototypes: HashMap<(usize, u32), controls::ControlClass>,
    window_styles: HashMap<u32, (u32, u32)>,
    window_parents: HashMap<u32, u32>,
    window_threads: HashMap<u32, usize>,
    window_input_contexts: HashMap<u32, u32>,
    window_rects: HashMap<u32, (i32, i32, i32, i32)>,
    window_dcs: HashMap<u32, u32>,
    window_visibility: HashMap<u32, bool>,
    window_user_data: u32,
    synthetic_create_sent: bool,
    synthetic_size_sent: bool,
    create_frame: Option<WindowCallbackFrame>,
    create_frame_hwnd: u32,
    create_frame_stack: Vec<(WindowCallbackFrame, CreateStage, u32)>,
    create_stage: CreateStage,
    dialog_frame: Option<GuestCallFrame>,
    dialog_frame_hwnd: u32,
    active_window: u32,
    window_ids: HashMap<u32, u32>,
    scroll_info: HashMap<(u32, u32), [i32; 5]>,
    window_extra: HashMap<(u32, i32), u32>,
    dialog_default_frames: Vec<(usize, u32, GuestCallFrame)>,
    dialog_frame_stack: Vec<(GuestCallFrame, u32)>,
    parked_dialog_callbacks: HashMap<usize, DialogCallbackState>,
    window_destroy_frames: Vec<WindowDestroyFrame>,
    control_notify_frames: Vec<ControlNotifyFrame>,
    gesture_frames: Vec<(usize, u32, GuestCallFrame, u32)>,
    dialog_key_frames: Vec<DialogKeyFrame>,
    accelerator_frames: Vec<(usize, u32, GuestCallFrame)>,
    shell_status: Option<u32>,
    status_bar: Option<StatusBar>,
    controls: controls::Controls,
    modal: Option<msgbox::MessageBox>,
    shell_keyboard: shell_keyboard::ShellKeyboard,
    message_box_spins: u32,
    modal_dialog: Option<ModalDialog>,
    gapi_keys_queried: bool,
    pocketpc_processor: Option<processor_info::PocketPcProcessor>,
    gapi_action_key: u16,
    legacy_winsock: desktop_services::LegacyWinsock,
    notification_icons: HashMap<(u32, u32), desktop_services::NotificationIcon>,
    winsock_last_errors: HashMap<usize, u32>,
    gz_files: crate::gz::GzFiles,
    msg_queues: HashMap<u32, MsgQueue>,
    next_msg_queue_handle: u32,
    registry_notifications: registry::QueueNotifications,
    power_notifications: power::Notifications,
    device_notification_queues: HashMap<u32, u32>,
    next_device_notification: u32,
    battery_alerts: power::GizmondoAlerts,
    menus: HashMap<u32, HashMap<u32, u32>>,
    menu_items: HashMap<u32, Vec<MenuItem>>,
    window_menus: HashMap<u32, u32>,
    next_menu_handle: u32,
    sub_menus: HashMap<(u32, u32), u32>,
}

impl KernelState {
    /// The first checkpoint profile supports a single ARM process using
    /// regular files and the fixed-function graphics runtime.
    pub fn capture_snapshot(&mut self) -> Result<KernelSnapshot, String> {
        self.capture_snapshot_with_suspended_parents(false)
    }

    /// The launcher may detach an isolated foreground program while its
    /// callers are synchronously suspended. Cross-process handles are rejected.
    pub fn capture_snapshot_with_suspended_parents(
        &mut self,
        detached: bool,
    ) -> Result<KernelSnapshot, String> {
        for (active, resource) in [
            (
                self.process_exit_code.is_some() || self.should_stop,
                "process shutdown",
            ),
            (
                self.pending_process_launch.is_some()
                    || !self.child_processes.is_empty()
                    || !self.scheduled_process_launches.is_empty()
                    || !self.process_launch_results.is_empty(),
                "child process",
            ),
            (
                !self.internet.handles.is_empty() || self.internet.gprs_connected,
                "network connection",
            ),
            (!self.serial_ports.is_empty(), "serial port"),
            (
                !self.crypto.providers.is_empty()
                    || !self.crypto.hashes.is_empty()
                    || !self.crypto.keys.is_empty()
                    || !self.crypto.public_keys.is_empty(),
                "cryptography object",
            ),
            (!self.cedb.cursors.is_empty(), "WinCE database cursor"),
            (
                !self.msxml.objects.is_empty() || !self.msxml.documents.is_empty(),
                "MSXML object",
            ),
            (
                self.sdl.initialized != 0 || !self.sdl.surfaces.is_empty(),
                "SDL state",
            ),
        ] {
            if active {
                return Err(format!(
                    "checkpoint contains unsupported live WinCE resources ({resource})"
                ));
            }
        }
        let zlib_allocator = self
            .zlib_streams
            .snapshot_allocator()
            .ok_or("checkpoint contains a live decompressor")?;
        let memory_profile = self.memory_division.as_ref().map(|ram| {
            let ram = ram.snapshot();
            (ram.total_pages, ram.store_pages)
        });
        let mut private_files = HashMap::new();
        let mappings = self.mapped_views.capture_snapshot(
            &self.vfs,
            &self.object_handles,
            &mut private_files,
        )?;
        let mapping_files = mappings
            .as_ref()
            .map(|mapping| mapping.file_handles())
            .unwrap_or_default();
        let handles = self
            .object_handles
            .capture_process_handles_with_mappings(detached, mappings.is_some())?;
        let mut schedule = self.capture_schedule();
        if detached {
            schedule.retain_process_objects(&handles);
        }
        Ok(KernelSnapshot {
            heap: self.heap.capture_snapshot(),
            vfs: self
                .vfs
                .capture_file_snapshot_with_mappings(&mapping_files, &private_files)?,
            mappings,
            handles,
            registry: self.registry.capture_process_snapshot()?,
            audio: self.audio.capture_snapshot()?,
            gdi: self.gdi.capture_snapshot()?,
            display: self.framebuffer.capture_snapshot()?,
            schedule,
            zlib_allocator,
            memory_profile,
            module_path: self.module_path.clone(),
            process_launch_enabled: self.process_launch_enabled,
            command_line_cache: self.command_line_cache,
            command_line: self.command_line.clone(),
            next_process_id: self.next_process_id,
            random_seed: self.random_seed,
            resources: self.resources.clone(),
            image_base: self.image_base,
            image_size: self.image_size,
            image_entry: self.image_entry,
            dynamic_exports: self.dynamic_exports.clone(),
            image_imports: self.image_imports.clone(),
            gles_local_ordinals: self.gles_local_ordinals.clone(),
            runtime_thunks: self.runtime_thunks.clone(),
            module_attach_frames: self.module_attach_frames.clone(),
            static_module_roots: self.static_module_roots.clone(),
            dll_notification_frame: self.dll_notification_frame.clone(),
            next_module_handle: self.next_module_handle,
            modules: self.modules.clone(),
            next_module_base: self.next_module_base,
            free_module_bases: self.free_module_bases.clone(),
            fb_mapped: self.fb_mapped,
            guest_fb_pitch: self.guest_fb_pitch,
            direct_fb_frames: self.direct_fb_frames,
            gapi_column_major: self.gapi_column_major,
            gx_last_pushed_counter: self.gx_last_pushed_counter,
            gx_draw_active: self.gx_draw_active,
            gx_draw_thread: self.gx_draw_thread,
            gx_explicit_present: self.gx_explicit_present,
            gx_guest_signature: self.gx_guest_signature,
            gles_strings: self.gles_strings.clone(),
            wnd_proc: self.wnd_proc,
            window_class_procs: self.window_class_procs.clone(),
            window_background: self.window_background,
            window_background_erase_pending: self.window_background_erase_pending,
            pending_create: self.pending_create,
            pending_startup: self.pending_startup.clone(),
            find_handles: self.find_handles.clone(),
            next_find_handle: self.next_find_handle,
            window_procs: self.window_procs.clone(),
            window_userdata: self.window_userdata.clone(),
            window_classes: self.window_classes.clone(),
            window_titles: self.window_titles.clone(),
            window_class_controls: self.window_class_controls.clone(),
            control_class_prototypes: self.control_class_prototypes.clone(),
            window_styles: self.window_styles.clone(),
            window_parents: self.window_parents.clone(),
            window_threads: self.window_threads.clone(),
            window_input_contexts: self.window_input_contexts.clone(),
            window_rects: self.window_rects.clone(),
            window_dcs: self.window_dcs.clone(),
            window_visibility: self.window_visibility.clone(),
            window_user_data: self.window_user_data,
            synthetic_create_sent: self.synthetic_create_sent,
            synthetic_size_sent: self.synthetic_size_sent,
            create_frame: self.create_frame.clone(),
            create_frame_hwnd: self.create_frame_hwnd,
            create_frame_stack: self.create_frame_stack.clone(),
            create_stage: self.create_stage,
            dialog_frame: self.dialog_frame,
            dialog_frame_hwnd: self.dialog_frame_hwnd,
            active_window: self.active_window,
            window_ids: self.window_ids.clone(),
            scroll_info: self.scroll_info.clone(),
            window_extra: self.window_extra.clone(),
            dialog_default_frames: self.dialog_default_frames.clone(),
            dialog_frame_stack: self.dialog_frame_stack.clone(),
            parked_dialog_callbacks: self.parked_dialog_callbacks.clone(),
            window_destroy_frames: self.window_destroy_frames.clone(),
            control_notify_frames: self.control_notify_frames.clone(),
            gesture_frames: self.gesture_frames.clone(),
            dialog_key_frames: self.dialog_key_frames.clone(),
            accelerator_frames: self.accelerator_frames.clone(),
            shell_status: self.shell_status,
            status_bar: self.status_bar.clone(),
            controls: self.controls.clone(),
            modal: self.modal.clone(),
            shell_keyboard: self.shell_keyboard.clone(),
            message_box_spins: self.message_box_spins,
            modal_dialog: self.modal_dialog.clone(),
            gapi_keys_queried: self.gapi_keys_queried,
            pocketpc_processor: self.pocketpc_processor,
            gapi_action_key: self.gapi_action_key,
            legacy_winsock: self.legacy_winsock.clone(),
            notification_icons: self.notification_icons.clone(),
            winsock_last_errors: self.winsock_last_errors.clone(),
            gz_files: self.gz_files.clone(),
            msg_queues: self.msg_queues.clone(),
            next_msg_queue_handle: self.next_msg_queue_handle,
            registry_notifications: self.registry_notifications.clone(),
            power_notifications: self.power_notifications.clone(),
            device_notification_queues: self.device_notification_queues.clone(),
            next_device_notification: self.next_device_notification,
            battery_alerts: self.battery_alerts.clone(),
            menus: self.menus.clone(),
            menu_items: self.menu_items.clone(),
            window_menus: self.window_menus.clone(),
            next_menu_handle: self.next_menu_handle,
            sub_menus: self.sub_menus.clone(),
        })
    }
}

impl KernelState {
    /// Queue releases for input already delivered to the emulated message pump.
    /// Fresh launches start with neutral polling and repeat state.
    pub fn snapshot_input_releases(&self) -> Vec<InputEvent> {
        let (x, y) = self.controls.cursor_position;
        let (x, y) = (
            x.clamp(0, i32::from(u16::MAX)) as u16,
            y.clamp(0, i32::from(u16::MAX)) as u16,
        );
        let mut releases: Vec<_> = self
            .held_keys
            .iter()
            .map(|&vk| InputEvent::KeyUp { vk })
            .collect();
        if self.pressed_keys[1] {
            releases.push(InputEvent::PointerUp { x, y });
        }
        for (key, message) in [(2, 0x205), (4, 0x208)] {
            if self.pressed_keys[key] {
                releases.push(InputEvent::MouseMessage {
                    message,
                    x,
                    y,
                    data: 0,
                });
            }
        }
        releases
    }

    pub fn restore_snapshot_input(&mut self, releases: Vec<InputEvent>) -> Result<(), String> {
        if releases.len() > 259
            || !releases.iter().all(|event| {
                matches!(
                    event,
                    InputEvent::KeyUp { .. }
                        | InputEvent::PointerUp { .. }
                        | InputEvent::MouseMessage {
                            message: 0x205 | 0x208,
                            ..
                        }
                )
            })
        {
            return Err("invalid snapshot input releases".into());
        }
        self.reset_foreground_input();
        self.pending_input.extend(releases);
        Ok(())
    }
}

impl KernelSnapshot {
    /// The receiving state is newly configured by normal launch, including
    /// mounts and the registry backing. Discard it if restoration fails.
    pub fn restore(
        self,
        state: &mut KernelState,
    ) -> Result<Option<file_mapping::MappingSnapshot>, String> {
        if self.mappings.is_none()
            && self
                .handles
                .owned_objects()
                .any(|object| matches!(object, handles::HandleObject::Mapping(_)))
        {
            return Err("checkpoint is missing its WinCE mappings".into());
        }
        let mapping_files = self
            .mappings
            .as_ref()
            .map(|mapping| mapping.file_handles())
            .unwrap_or_default();
        let ram = state.memory_division.clone();
        let store_pages = match (self.memory_profile, ram.as_ref()) {
            (None, None) => None,
            (Some((saved_total, saved_store)), Some(ram))
                if saved_total == ram.snapshot().total_pages =>
            {
                Some(saved_store)
            }
            _ => return Err("saved device RAM capacity differs from the launch profile".into()),
        };
        state.heap = Heap::new(self.image_base, 1);
        if let (Some(store_pages), Some(ram)) = (store_pages, ram.as_ref()) {
            if ram.store_pages() != store_pages {
                ram.resize(store_pages).map_err(|error| {
                    format!("cannot restore the saved device RAM split: {error:?}")
                })?;
            }
        }
        state.heap = self.heap.restore(ram).map_err(|error| error.to_string())?;
        state
            .vfs
            .restore_file_snapshot_with_mappings(self.vfs, &mapping_files)?;
        state.registry.restore_process_snapshot(self.registry);
        state.audio = self.audio.restore()?;
        state.object_handles = self.handles.restore(&state.vfs, &state.audio)?;
        state.gdi = self.gdi.restore()?;
        state.framebuffer = self.display.restore()?;
        self.schedule
            .restore(state)
            .map_err(|error| error.to_string())?;
        state.zlib_streams = zlib::ZlibStreams::restore_allocator(self.zlib_allocator);
        state.module_path = self.module_path;
        state.process_launch_enabled = self.process_launch_enabled;
        state.command_line_cache = self.command_line_cache;
        state.command_line = self.command_line;
        state.next_process_id = self.next_process_id;
        state.random_seed = self.random_seed;
        state.resources = self.resources;
        state.image_base = self.image_base;
        state.image_size = self.image_size;
        state.image_entry = self.image_entry;
        state.dynamic_exports = self.dynamic_exports;
        state.image_imports = self.image_imports;
        state.gles_local_ordinals = self.gles_local_ordinals;
        state.runtime_thunks = self.runtime_thunks;
        state.module_attach_frames = self.module_attach_frames;
        state.static_module_roots = self.static_module_roots;
        state.dll_notification_frame = self.dll_notification_frame;
        state.next_module_handle = self.next_module_handle;
        state.modules = self.modules;
        state.next_module_base = self.next_module_base;
        state.free_module_bases = self.free_module_bases;
        state.fb_mapped = self.fb_mapped;
        state.guest_fb_pitch = self.guest_fb_pitch;
        state.direct_fb_frames = self.direct_fb_frames;
        state.gapi_column_major = self.gapi_column_major;
        state.gx_last_pushed_counter = self.gx_last_pushed_counter;
        state.gx_draw_active = self.gx_draw_active;
        state.gx_draw_thread = self.gx_draw_thread;
        state.gx_explicit_present = self.gx_explicit_present;
        state.gx_guest_signature = self.gx_guest_signature;
        state.gles_strings = self.gles_strings;
        state.wnd_proc = self.wnd_proc;
        state.window_class_procs = self.window_class_procs;
        state.window_background = self.window_background;
        state.window_background_erase_pending = self.window_background_erase_pending;
        state.pending_create = self.pending_create;
        state.pending_startup = self.pending_startup;
        state.find_handles = self.find_handles;
        state.next_find_handle = self.next_find_handle;
        state.window_procs = self.window_procs;
        state.window_userdata = self.window_userdata;
        state.window_classes = self.window_classes;
        state.window_titles = self.window_titles;
        state.window_class_controls = self.window_class_controls;
        state.control_class_prototypes = self.control_class_prototypes;
        state.window_styles = self.window_styles;
        state.window_parents = self.window_parents;
        state.window_threads = self.window_threads;
        state.window_input_contexts = self.window_input_contexts;
        state.window_rects = self.window_rects;
        state.window_dcs = self.window_dcs;
        state.window_visibility = self.window_visibility;
        state.window_user_data = self.window_user_data;
        state.synthetic_create_sent = self.synthetic_create_sent;
        state.synthetic_size_sent = self.synthetic_size_sent;
        state.create_frame = self.create_frame;
        state.create_frame_hwnd = self.create_frame_hwnd;
        state.create_frame_stack = self.create_frame_stack;
        state.create_stage = self.create_stage;
        state.dialog_frame = self.dialog_frame;
        state.dialog_frame_hwnd = self.dialog_frame_hwnd;
        state.active_window = self.active_window;
        state.window_ids = self.window_ids;
        state.scroll_info = self.scroll_info;
        state.window_extra = self.window_extra;
        state.dialog_default_frames = self.dialog_default_frames;
        state.dialog_frame_stack = self.dialog_frame_stack;
        state.parked_dialog_callbacks = self.parked_dialog_callbacks;
        state.window_destroy_frames = self.window_destroy_frames;
        state.control_notify_frames = self.control_notify_frames;
        state.gesture_frames = self.gesture_frames;
        state.dialog_key_frames = self.dialog_key_frames;
        state.accelerator_frames = self.accelerator_frames;
        state.shell_status = self.shell_status;
        state.status_bar = self.status_bar;
        state.controls = self.controls;
        state.modal = self.modal;
        state.shell_keyboard = self.shell_keyboard;
        state.message_box_spins = self.message_box_spins;
        state.modal_dialog = self.modal_dialog;
        state.gapi_keys_queried = self.gapi_keys_queried;
        state.pocketpc_processor = self.pocketpc_processor;
        state.gapi_action_key = self.gapi_action_key;
        state.legacy_winsock = self.legacy_winsock;
        state.notification_icons = self.notification_icons;
        state.winsock_last_errors = self.winsock_last_errors;
        state.gz_files = self.gz_files;
        state.msg_queues = self.msg_queues;
        state.next_msg_queue_handle = self.next_msg_queue_handle;
        state.registry_notifications = self.registry_notifications;
        state.power_notifications = self.power_notifications;
        state.device_notification_queues = self.device_notification_queues;
        state.next_device_notification = self.next_device_notification;
        state.battery_alerts = self.battery_alerts;
        state.menus = self.menus;
        state.menu_items = self.menu_items;
        state.window_menus = self.window_menus;
        state.next_menu_handle = self.next_menu_handle;
        state.sub_menus = self.sub_menus;
        Ok(self.mappings)
    }
}
