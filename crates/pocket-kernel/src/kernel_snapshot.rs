//! Process-owned WinCE checkpoint data; shared domains are coordinated separately.

use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct ProcessFamily {
    children: HashMap<u32, ChildProcess>,
    pending: Option<ProcessLaunch>,
    suspended: Option<ProcessLaunch>,
    scheduled: HashMap<String, (std::time::SystemTime, String)>,
    results: HashMap<(usize, u32, u32), Result<u32, u32>>,
}

// Stable IDs: high byte is the domain, low byte the field. Never renumber or
// reuse an ID; bump only the affected record version when its payload changes.
pocket_snapshot::snapshot_fields! {
pub struct KernelSnapshot {
    // Memory.
    0x0101, 1 => heap: HeapSnapshot;
    0x0102, 1 => memory_profile: Option<(u32, u32)>;
    // Files and handles.
    0x0201, 1 => vfs: vfs::VfsFileSnapshot;
    0x0202, 1 => mappings: Option<file_mapping::MappingSnapshot>;
    0x0203, 1 => handles: handles::HandleSnapshot;
    0x0204, 1 => find_handles: HashMap<u32, std::collections::VecDeque<(String, u64, bool, u32)>>;
    0x0205, 1 => next_find_handle: u32;
    0x0206, 1 => temporary_files: vfs::TemporaryFiles = vfs::TemporaryFiles::default();
    // Process services.
    0x0301, 1 => registry: registry::RegistryProcessSnapshot;
    0x0302, 1 => crypto: crypto::CryptoState;
    0x0303, 1 => msxml: msxml::MsxmlState;
    0x0304, 1 => sdl: sdl::SdlState;
    0x0305, 1 => cedb: cedb::ContactSnapshot;
    0x0306, 1 => internet: internet::InternetSnapshot;
    0x0307, 1 => zlib: zlib::ZlibStreams;
    // Media.
    0x0401, 1 => audio: audio::AudioSnapshot;
    0x0402, 1 => gz_files: crate::gz::GzFiles;
    // Graphics.
    0x0501, 2 => gdi: gdi::GdiSnapshot, decode = gdi::read_snapshot_record;
    0x0502, 1 => display: framebuffer::DisplaySnapshot;
    0x0503, 1 => fb_mapped: bool;
    0x0504, 1 => guest_fb_pitch: u32;
    0x0505, 1 => direct_fb_frames: u64;
    0x0506, 1 => gapi_column_major: bool;
    0x0507, 1 => gx_last_pushed_counter: u64;
    0x0508, 1 => gx_draw_active: bool;
    0x0509, 1 => gx_draw_thread: usize;
    0x050a, 1 => gx_explicit_present: bool;
    0x050b, 1 => gx_guest_signature: Option<u64>;
    0x050c, 1 => gles_strings: std::collections::HashMap<String, u32>;
    0x050d, 1 => gapi_keys_queried: bool;
    0x050e, 1 => gapi_action_key: u16;
    0x050f, 1 => shell_toolbars: HashMap<u32, shell_menu::Toolbar> = HashMap::new();
    // Scheduler and continuations.
    0x0601, 1 => schedule: ScheduleSnapshot;
    // Process and modules.
    0x0701, 1 => module_path: String;
    0x0702, 1 => process_launch_enabled: bool;
    0x0703, 1 => family: ProcessFamily;
    0x0704, 1 => command_line_cache: Option<u32>;
    0x0705, 1 => command_line: Option<String>;
    0x0706, 1 => next_process_id: u32;
    0x0707, 1 => random_seed: u32;
    0x0708, 1 => resources: Vec<ResourceEntry>;
    0x0709, 1 => image_base: u32;
    0x070a, 1 => image_size: u32;
    0x070b, 1 => image_entry: u32;
    0x070c, 1 => dynamic_exports: HashMap<u32, HashMap<String, u32>>;
    0x070d, 1 => image_imports: Vec<pocket_pe::ImportSymbol>;
    0x070e, 1 => gles_local_ordinals: HashMap<String, HashMap<u16, String>>;
    0x070f, 1 => runtime_thunks: HashMap<u32, Thunk>;
    0x0710, 1 => module_attach_frames: Vec<ModuleAttachFrame>;
    0x0711, 1 => static_module_roots: Vec<u32>;
    0x0712, 1 => dll_notification_frame: Option<dll_lifecycle::DllNotificationFrame>;
    0x0713, 1 => next_module_handle: u32;
    0x0714, 1 => modules: Vec<LoadedModule>;
    0x0715, 1 => next_module_base: u32;
    0x0716, 1 => free_module_bases: Vec<u32>;
    0x0717, 1 => disabled_dll_thread_notifications: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    // Windows and shell.
    0x0801, 1 => wnd_proc: u32;
    0x0802, 1 => window_class_procs: HashMap<String, u32>;
    0x0803, 1 => window_background: Option<u32>;
    0x0804, 1 => window_background_erase_pending: bool;
    0x0805, 1 => pending_create: Option<(u32, u32)>;
    0x0806, 1 => pending_startup: std::collections::VecDeque<(u32, u32, u32)>;
    0x0807, 1 => window_procs: HashMap<u32, u32>;
    0x0808, 1 => window_userdata: HashMap<u32, u32>;
    0x0809, 1 => window_classes: HashMap<u32, String>;
    0x080a, 1 => window_titles: HashMap<u32, String>;
    0x080b, 1 => window_class_controls: HashMap<String, controls::ControlClass>;
    0x080c, 1 => control_class_prototypes: HashMap<(usize, u32), controls::ControlClass>;
    0x080d, 1 => window_styles: HashMap<u32, (u32, u32)>;
    0x080e, 1 => window_parents: HashMap<u32, u32>;
    0x080f, 1 => window_threads: HashMap<u32, usize>;
    0x0810, 1 => window_input_contexts: HashMap<u32, u32>;
    0x0811, 1 => window_rects: HashMap<u32, (i32, i32, i32, i32)>;
    0x0812, 1 => window_dcs: HashMap<u32, u32>;
    0x0813, 1 => window_visibility: HashMap<u32, bool>;
    0x0814, 1 => window_user_data: u32;
    0x0815, 1 => synthetic_create_sent: bool;
    0x0816, 1 => synthetic_size_sent: bool;
    0x0817, 1 => create_frame: Option<WindowCallbackFrame>;
    0x0818, 1 => create_frame_hwnd: u32;
    0x0819, 1 => create_frame_stack: Vec<(WindowCallbackFrame, CreateStage, u32)>;
    0x081a, 1 => create_stage: CreateStage;
    0x081b, 1 => dialog_frame: Option<GuestCallFrame>;
    0x081c, 1 => dialog_frame_hwnd: u32;
    0x081d, 1 => active_window: u32;
    0x081e, 1 => window_ids: HashMap<u32, u32>;
    0x081f, 1 => scroll_info: HashMap<(u32, u32), [i32; 5]>;
    0x0820, 1 => window_extra: HashMap<(u32, i32), u32>;
    0x0821, 1 => dialog_default_frames: Vec<(usize, u32, GuestCallFrame)>;
    0x0822, 1 => dialog_frame_stack: Vec<(GuestCallFrame, u32)>;
    0x0823, 1 => parked_dialog_callbacks: HashMap<usize, DialogCallbackState>;
    0x0824, 1 => window_destroy_frames: Vec<WindowDestroyFrame>;
    0x0825, 1 => control_notify_frames: Vec<ControlNotifyFrame>;
    0x0826, 1 => gesture_frames: Vec<(usize, u32, GuestCallFrame, u32)>;
    0x0827, 1 => dialog_key_frames: Vec<DialogKeyFrame>;
    0x0828, 1 => accelerator_frames: Vec<(usize, u32, GuestCallFrame)>;
    0x0829, 1 => shell_status: Option<u32>;
    0x082a, 1 => status_bar: Option<StatusBar>;
    0x082b, 1 => controls: controls::Controls;
    0x082c, 1 => modal: Option<msgbox::MessageBox>;
    0x082d, 1 => shell_keyboard: shell_keyboard::ShellKeyboard;
    0x082e, 1 => message_box_spins: u32;
    0x082f, 1 => modal_dialog: Option<ModalDialog>;
    0x0830, 1 => notification_icons: HashMap<(u32, u32), desktop_services::NotificationIcon>;
    0x0831, 1 => menus: HashMap<u32, HashMap<u32, u32>>;
    0x0832, 1 => menu_items: HashMap<u32, Vec<MenuItem>>;
    0x0833, 1 => window_menus: HashMap<u32, u32>;
    0x0834, 1 => next_menu_handle: u32;
    0x0835, 1 => sub_menus: HashMap<(u32, u32), u32>;
    0x0836, 3 => edit_controls: HashMap<u32, controls::edit::EditState> = HashMap::new();
    0x0837, 1 => indeterminate_checks: std::collections::HashSet<u32> = std::collections::HashSet::new();
    0x0838, 1 => window_create_scratch: HashMap<(usize, u32, u32), u32> = HashMap::new();
    0x0839, 1 => clipboard: crate::clipboard::Clipboard = crate::clipboard::Clipboard::default();
    0x083a, 1 => document_lists: HashMap<u32, controls::doc_list::DocList> = HashMap::new();
    // Devices and notifications.
    0x0901, 1 => serial: serial::SerialSnapshot;
    0x0902, 1 => pocketpc_processor: Option<processor_info::PocketPcProcessor>;
    0x0903, 1 => legacy_winsock: desktop_services::LegacyWinsock;
    0x0904, 1 => winsock_last_errors: HashMap<usize, u32>;
    0x0905, 1 => msg_queues: HashMap<u32, MsgQueue>;
    0x0906, 1 => next_msg_queue_handle: u32;
    0x0907, 1 => registry_notifications: registry::QueueNotifications;
    0x0908, 1 => power_notifications: power::Notifications;
    0x0909, 1 => device_notification_queues: HashMap<u32, u32>;
    0x090a, 1 => next_device_notification: u32;
    0x090b, 1 => battery_alerts: power::GizmondoAlerts;
}
}

impl KernelState {
    /// Capture one process; device captures coordinate shared resources before
    /// invoking the same process-owned serializer.
    pub fn capture_snapshot(&mut self) -> Result<KernelSnapshot, String> {
        self.capture_snapshot_inner(false, Instant::now())
    }

    pub fn capture_device_process(&mut self, at: Instant) -> Result<KernelSnapshot, String> {
        self.capture_snapshot_inner(true, at)
    }

    pub fn capture_checkpoint_at(&mut self, at: Instant) -> Result<KernelSnapshot, String> {
        self.capture_snapshot_inner(false, at)
    }
    fn capture_snapshot_inner(
        &mut self,
        coordinated: bool,
        at: Instant,
    ) -> Result<KernelSnapshot, String> {
        for (active, resource) in [
            (
                self.process_exit_code.is_some() || self.should_stop,
                "process shutdown",
            ),
            (
                !coordinated
                    && (self.pending_process_launch.is_some()
                        || self.suspended_process_launch.is_some()
                        || !self.child_processes.is_empty()
                        || !self.scheduled_process_launches.is_empty()
                        || !self.process_launch_results.is_empty()),
                "child process",
            ),
        ] {
            if active {
                return Err(format!(
                    "checkpoint contains unsupported live WinCE resources ({resource})"
                ));
            }
        }
        self.crypto.validate_snapshot()?;
        self.msxml.validate_snapshot()?;
        self.sdl.validate_snapshot()?;
        let memory_profile = self.memory_division.as_ref().map(|ram| {
            let ram = ram.snapshot();
            (ram.total_pages, ram.store_pages)
        });
        let mut private_files = HashMap::new();
        let mappings = self.mapped_views.capture_snapshot(
            &self.vfs,
            &self.object_handles,
            &mut private_files,
            coordinated,
        )?;
        if coordinated {
            for object in self.object_handles.checkpoint_owned_backings() {
                self.vfs
                    .snapshot_object_handle(&object, &mut private_files)?;
            }
        }
        let mapping_files = mappings
            .as_ref()
            .map(|mapping| mapping.file_handles())
            .unwrap_or_default();
        let handles = self
            .object_handles
            .capture_process_handles_with_mappings(mappings.is_some(), coordinated)?;
        let schedule = self.capture_schedule();
        Ok(KernelSnapshot {
            heap: self.heap.capture_snapshot(),
            vfs: self.vfs.capture_file_snapshot_with_mappings(
                &mapping_files,
                &private_files,
                coordinated,
                at,
            )?,
            temporary_files: self.vfs.capture_temporary_files()?,
            mappings,
            handles,
            registry: self.registry.capture_process_snapshot()?,
            cedb: self.cedb.capture_checkpoint()?,
            internet: self.internet.capture_checkpoint()?,
            crypto: self.crypto.clone(),
            msxml: self.msxml.clone(),
            sdl: self.sdl.clone(),
            serial: serial::SerialSnapshot::capture(&self.serial_ports, at)?,
            audio: self.audio.capture_snapshot_at(at)?,
            gdi: self.gdi.capture_snapshot()?,
            shell_toolbars: self.gdi.shell_toolbars.clone(),
            display: self.framebuffer.capture_snapshot()?,
            schedule,
            zlib: self.zlib_streams.capture_snapshot(),
            memory_profile,
            module_path: self.module_path.clone(),
            process_launch_enabled: self.process_launch_enabled,
            family: ProcessFamily {
                children: self.child_processes.clone(),
                pending: self.pending_process_launch.clone(),
                suspended: self.suspended_process_launch.clone(),
                scheduled: self.scheduled_process_launches.clone(),
                results: self.process_launch_results.clone(),
            },
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
            disabled_dll_thread_notifications: self.disabled_dll_thread_notifications.clone(),
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
            window_create_scratch: self.window_create_scratch.clone(),
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
            clipboard: self.clipboard.clone(),
            document_lists: self.controls.doc_lists.clone(),
            edit_controls: self.controls.snapshot_edits(),
            indeterminate_checks: self.controls.snapshot_checks(),
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
        self.sdl.release_snapshot_inputs();
        Ok(())
    }
}

impl KernelSnapshot {
    /// Recreate only VFS-allocated temporary names, with rollback until the
    /// entire CPU/GPU/device reconstruction succeeds. Ordinary files retain
    /// the existing path/identity policy.
    pub fn prepare_temporary_files(
        &self,
        vfs: &mut vfs::Vfs,
    ) -> Result<vfs::SnapshotFileCommit, String> {
        vfs.prepare_temporary_files(&self.temporary_files)
    }
    pub fn prime_device_restore(
        &self,
        vfs: &vfs::Vfs,
        domain: &DeviceRestoreDomain,
    ) -> Result<(), String> {
        self.vfs.prime_device_restore(vfs, &domain.files)
    }
    pub fn take_device_audio(
        &mut self,
        keys: &std::collections::BTreeSet<u32>,
    ) -> std::collections::BTreeMap<u32, audio::MasSnapshot> {
        self.audio.take_device_mas(keys)
    }
    pub fn omit_device_wait_objects(&mut self) {
        self.schedule.omit_device_objects();
    }
    pub fn is_device_process(&self) -> bool {
        self.handles.is_device_process()
    }
    pub fn program_path(&self) -> &str {
        &self.module_path
    }
    pub fn process_id(&self) -> u32 {
        self.handles.process_id()
    }

    /// The receiving state is newly configured by normal launch, including
    /// mounts and the registry backing. Discard it if restoration fails.
    pub fn restore(
        self,
        state: &mut KernelState,
    ) -> Result<Option<file_mapping::MappingSnapshot>, String> {
        self.restore_with_domain(state, None)
    }

    pub fn restore_with_domain(
        self,
        state: &mut KernelState,
        domain: Option<&DeviceRestoreDomain>,
    ) -> Result<Option<file_mapping::MappingSnapshot>, String> {
        if self
            .disabled_dll_thread_notifications
            .iter()
            .any(|handle| !self.modules.iter().any(|module| module.handle == *handle))
        {
            return Err("DLL thread notification opt-out has no loaded module".into());
        }
        for (hwnd, list) in &self.document_lists {
            if self.controls.get(*hwnd).is_none_or(|control| {
                control.class != controls::ControlClass::ListView
                    || control.list_items.len() != list.paths.len()
            }) {
                return Err("checkpoint document list has no matching list control".into());
            }
            if list.filters.len() > 64
                || list.paths.len() > 0x100000
                || list.directory.len() > 32768
                || list.directory.contains('\0')
                || list
                    .paths
                    .iter()
                    .any(|path| path.len() > 32768 || path.contains('\0'))
                || list.filters.iter().any(|patterns| {
                    patterns.len() > 256
                        || patterns
                            .iter()
                            .any(|pattern| pattern.len() > 32768 || pattern.contains('\0'))
                })
                || (!list.filters.is_empty()
                    && (list.filter_index == 0 || list.filter_index as usize > list.filters.len()))
            {
                return Err("checkpoint document list exceeds its supported extent".into());
            }
        }
        self.clipboard.validate(&self.heap)?;
        if self
            .clipboard
            .opened_by
            .is_some_and(|thread| !self.schedule.contains_thread(thread))
        {
            return Err("checkpoint clipboard lock has no matching thread".into());
        }
        self.controls.validate_edits(&self.edit_controls)?;
        self.controls.validate_checks(&self.indeterminate_checks)?;
        let mut creation_buffers = std::collections::HashSet::new();
        for (&(thread, sp, hwnd), &buffer) in &self.window_create_scratch {
            let active = self
                .create_frame
                .into_iter()
                .map(|f| (f, self.create_frame_hwnd))
                .chain(
                    self.create_frame_stack
                        .iter()
                        .map(|(f, _, hwnd)| (*f, *hwnd)),
                )
                .any(|(f, h)| f.thread == thread && f.sp == sp && h == hwnd);
            if !active || !creation_buffers.insert(buffer) || !self.heap.has_allocation(buffer, 64)
            {
                return Err("invalid window creation scratch owner".into());
            }
        }

        self.crypto.validate_snapshot()?;
        self.msxml.validate_snapshot()?;
        self.sdl.validate_snapshot()?;
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
        self.cedb.restore(&mut state.cedb, &mut state.vfs)?;
        self.internet.restore(&mut state.internet)?;
        state.registry.restore_process_snapshot(self.registry);
        state.crypto = self.crypto;
        state.msxml = self.msxml;
        state.sdl = self.sdl;
        state.serial_ports = self.serial.restore(&state.vfs)?;
        state.audio = self
            .audio
            .restore_with_shared_mas(domain.map(|domain| &domain.mas))?;
        state.object_handles = self.handles.restore(&state.vfs, &state.audio)?;
        state.gdi = self.gdi.restore()?;
        for (hwnd, toolbar) in &self.shell_toolbars {
            let bar = state
                .gdi
                .shell_menus
                .bars
                .get(hwnd)
                .ok_or("saved toolbar has no shell window")?;
            let items = self
                .menu_items
                .get(&bar.menu)
                .ok_or("saved toolbar has no menu projection")?;
            toolbar.validate(&state.gdi, items.len())?;
        }
        state.gdi.shell_toolbars = self.shell_toolbars;
        state.framebuffer = self.display.restore()?;
        self.schedule
            .restore(state)
            .map_err(|error| error.to_string())?;
        state.zlib_streams = self.zlib.restore_snapshot()?;
        state.module_path = self.module_path;
        state.process_launch_enabled = self.process_launch_enabled;
        state.child_processes = self.family.children;
        state.pending_process_launch = self.family.pending;
        state.suspended_process_launch = self.family.suspended;
        for request in state
            .pending_process_launch
            .iter_mut()
            .chain(state.suspended_process_launch.iter_mut())
        {
            request.executable = state
                .vfs
                .resolve(&request.guest_path)
                .ok_or("saved pending process executable is missing")?;
        }
        state.scheduled_process_launches = self.family.scheduled;
        state.process_launch_results = self.family.results;
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
        state.disabled_dll_thread_notifications = self.disabled_dll_thread_notifications;
        state.dll_notification_frame = self.dll_notification_frame;
        state.next_module_handle = self.next_module_handle;
        state.modules = self.modules;
        state.next_module_base = self.next_module_base;
        state.free_module_bases = self.free_module_bases;
        state.fb_mapped = self.fb_mapped;
        state.guest_fb_pitch = self.guest_fb_pitch;
        state.direct_fb_frames = self.direct_fb_frames;
        // A cold frontend publishes the captured CPU image before observing
        // subsequent guest writes. Pending host upload work is not guest state.
        state.pending_gapi_copy = false;
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
        state.window_create_scratch = self.window_create_scratch;
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
        state.controls.doc_lists = self.document_lists;
        state.clipboard = self.clipboard;
        state.controls.restore_edits(self.edit_controls);
        state.controls.restore_checks(self.indeterminate_checks);
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

impl KernelSnapshot {
    pub fn suspended_child_handle(&self) -> Result<Option<u32>, String> {
        let Some(request) = &self.family.suspended else {
            return Ok(None);
        };
        let child = self
            .family
            .children
            .get(&request.process_handle)
            .ok_or("suspended caller has no child record")?;
        if request.concurrent
            || request.creation_flags != 0
            || child.exit_code.is_some()
            || child.thread_handle != request.thread_handle
            || self.family.pending.is_some()
        {
            return Err("invalid suspended caller continuation".into());
        }
        Ok(Some(request.process_handle))
    }
}
