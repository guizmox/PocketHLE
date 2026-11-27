# WinCE record format (container version 12)

Gizmondo and PocketPC use this same process archive on Desktop and Android.
The records capture process-owned state through the existing kernel, USER, GDI
and VFS resource owners; shared device state is coordinated by the session.

## Stream and ownership

The native section with required tag 3 uses version 12. Its payload is a
little-endian u16 record count followed by records with u16 ID/flags, u16 payload
version and u64 payload length, then the typed payload. Bit 15 marks required
records; the remaining ID has a domain byte and a field byte. These numeric
keys are permanent. Never renumber keys when inserting or rearranging fields.

Records are required unless their schema declares an explicit absent-state
default. An unknown optional record can be skipped; an unknown required record,
unsupported known version, changed required flag, duplicate/zero ID, missing required state, truncated/extra payload or size
violation fails decoding before resource reconstruction. The count is bounded
at 4096 and all headers/data share the existing 512 MiB limit. The containing
session also retains its cumulative budget, including ARM and graphics data.

The writer computes a small length directory, then writes each typed payload
directly to the existing buffered compressed stream. The reader uses a bounded
view per record. Neither side stages a complete WinCE byte buffer. ARM block
reading, compression, atomic slot replacement and child-process coordination
keep their existing path; children call the same session/WinCE record codec.

The reusable mechanism lives in `pocket-snapshot::fields`. Owners declare their
schema with `snapshot_fields!`; there is no second list of runtime capture or
restore assignments. In-memory snapshots remain typed objects.

## Evolution contract

1. An API behavior fix or a host-only cache change does not itself require a
   schema change. Avoid storing host deadlines, pointers or rebuildable caches.
2. A new optional record needs an explicit, semantically safe absent-state
   default (`field: Type = expression`). Current mandatory guest resources must
   never be defaulted away simply to accept a file.
3. A payload schema change bumps that record's version. A local `decode = path`
   handler can read an older version into a temporary historical DTO and convert
   it to the current value. Other records and the container remain unchanged.
4. If an old state cannot be represented faithfully, reject it explicitly.
   Subdivision does not guarantee that arbitrary future semantic changes are
   compatible. Never reuse an ID for different state.
5. Nested payloads still use fixed-integer bincode. Changing one of their fields
   is not automatically compatible: provide a scoped migration and test it.
   The record helper can be reused by a nested owner when finer isolation is
   needed. Shared device-domain, CPU, EGL and media section versions remain
   independently owned; this change does not stabilize all their payloads.

Format compatibility is not a promise of arbitrary execution compatibility.
A change to guest/native thunk semantics, device requirements or resource ABI
can still need an owner migration or explicit preflight rejection. Keep those
requirements checks and GPU/backend reconstruction validation; do not accept an
unrepresentable state merely because its record headers are understood.

The former monolithic WinCE 11 reader is removed. Development slots must be
recreated for this transition. Future changes should preserve the new format's
reference fixtures through explicit defaults or local conversions, rather than
bumping the whole WinCE container for a game fix.

## Compatibility checks

The record tests freeze a literal v1 wire record and check reading older data
with a new optional field, skipping new optional data, field reordering and a
u32-to-u64 version migration. Negative cases cover missing required data,
unknown required IDs/versions/flags, duplicates, oversized lengths and every
truncation point. A full session test adds an optional WinCE record and cold
restores that session, then checks required-record and old-container rejection.
A synthetic process reference fixture exercises actual kernel/GDI/callback
state. It contains no game code or assets and must not be silently regenerated
when a compatibility test fails.

## Permanent record registry

The source of truth is `KernelSnapshot` in
[`kernel_snapshot.rs`](../crates/pocket-kernel/src/kernel_snapshot.rs). Versions
below are the current writer versions. A scoped decoder can accept an older
payload version where implemented. **Optional** means the reader uses the
schema's explicit default when that record is absent; writers still capture it.
See [`snapshot_fields!`](../crates/pocket-snapshot/src/fields.rs) for flag handling.

| ID | Version | Presence | Domain | Field |
| --- | --- | --- | --- | --- |
| `0x0101` | 1 | Required | Memory | `heap` |
| `0x0102` | 1 | Required | Memory | `memory_profile` |
| `0x0201` | 1 | Required | Files and handles | `vfs` |
| `0x0202` | 1 | Required | Files and handles | `mappings` |
| `0x0203` | 1 | Required | Files and handles | `handles` |
| `0x0204` | 1 | Required | Files and handles | `find_handles` |
| `0x0205` | 1 | Required | Files and handles | `next_find_handle` |
| `0x0206` | 1 | Optional | Files and handles | `temporary_files` |
| `0x0301` | 1 | Required | Process services | `registry` |
| `0x0302` | 1 | Required | Process services | `crypto` |
| `0x0303` | 1 | Required | Process services | `msxml` |
| `0x0304` | 1 | Required | Process services | `sdl` |
| `0x0305` | 1 | Required | Process services | `cedb` |
| `0x0306` | 1 | Required | Process services | `internet` |
| `0x0307` | 1 | Required | Process services | `zlib` |
| `0x0401` | 1 | Required | Media | `audio` |
| `0x0402` | 1 | Required | Media | `gz_files` |
| `0x0501` | 2 | Optional | Graphics | `gdi` |
| `0x0502` | 1 | Required | Graphics | `display` |
| `0x0503` | 1 | Required | Graphics | `fb_mapped` |
| `0x0504` | 1 | Required | Graphics | `guest_fb_pitch` |
| `0x0505` | 1 | Required | Graphics | `direct_fb_frames` |
| `0x0506` | 1 | Required | Graphics | `gapi_column_major` |
| `0x0507` | 1 | Required | Graphics | `gx_last_pushed_counter` |
| `0x0508` | 1 | Required | Graphics | `gx_draw_active` |
| `0x0509` | 1 | Required | Graphics | `gx_draw_thread` |
| `0x050a` | 1 | Required | Graphics | `gx_explicit_present` |
| `0x050b` | 1 | Required | Graphics | `gx_guest_signature` |
| `0x050c` | 1 | Required | Graphics | `gles_strings` |
| `0x050d` | 1 | Required | Graphics | `gapi_keys_queried` |
| `0x050e` | 1 | Required | Graphics | `gapi_action_key` |
| `0x050f` | 1 | Optional | Graphics | `shell_toolbars` |
| `0x0601` | 1 | Required | Scheduler and continuations | `schedule` |
| `0x0701` | 1 | Required | Process and modules | `module_path` |
| `0x0702` | 1 | Required | Process and modules | `process_launch_enabled` |
| `0x0703` | 1 | Required | Process and modules | `family` |
| `0x0704` | 1 | Required | Process and modules | `command_line_cache` |
| `0x0705` | 1 | Required | Process and modules | `command_line` |
| `0x0706` | 1 | Required | Process and modules | `next_process_id` |
| `0x0707` | 1 | Required | Process and modules | `random_seed` |
| `0x0708` | 1 | Required | Process and modules | `resources` |
| `0x0709` | 1 | Required | Process and modules | `image_base` |
| `0x070a` | 1 | Required | Process and modules | `image_size` |
| `0x070b` | 1 | Required | Process and modules | `image_entry` |
| `0x070c` | 1 | Required | Process and modules | `dynamic_exports` |
| `0x070d` | 1 | Required | Process and modules | `image_imports` |
| `0x070e` | 1 | Required | Process and modules | `gles_local_ordinals` |
| `0x070f` | 1 | Required | Process and modules | `runtime_thunks` |
| `0x0710` | 1 | Required | Process and modules | `module_attach_frames` |
| `0x0711` | 1 | Required | Process and modules | `static_module_roots` |
| `0x0712` | 1 | Required | Process and modules | `dll_notification_frame` |
| `0x0713` | 1 | Required | Process and modules | `next_module_handle` |
| `0x0714` | 1 | Required | Process and modules | `modules` |
| `0x0715` | 1 | Required | Process and modules | `next_module_base` |
| `0x0716` | 1 | Required | Process and modules | `free_module_bases` |
| `0x0801` | 1 | Required | Windows and shell | `wnd_proc` |
| `0x0802` | 1 | Required | Windows and shell | `window_class_procs` |
| `0x0803` | 1 | Required | Windows and shell | `window_background` |
| `0x0804` | 1 | Required | Windows and shell | `window_background_erase_pending` |
| `0x0805` | 1 | Required | Windows and shell | `pending_create` |
| `0x0806` | 1 | Required | Windows and shell | `pending_startup` |
| `0x0807` | 1 | Required | Windows and shell | `window_procs` |
| `0x0808` | 1 | Required | Windows and shell | `window_userdata` |
| `0x0809` | 1 | Required | Windows and shell | `window_classes` |
| `0x080a` | 1 | Required | Windows and shell | `window_titles` |
| `0x080b` | 1 | Required | Windows and shell | `window_class_controls` |
| `0x080c` | 1 | Required | Windows and shell | `control_class_prototypes` |
| `0x080d` | 1 | Required | Windows and shell | `window_styles` |
| `0x080e` | 1 | Required | Windows and shell | `window_parents` |
| `0x080f` | 1 | Required | Windows and shell | `window_threads` |
| `0x0810` | 1 | Required | Windows and shell | `window_input_contexts` |
| `0x0811` | 1 | Required | Windows and shell | `window_rects` |
| `0x0812` | 1 | Required | Windows and shell | `window_dcs` |
| `0x0813` | 1 | Required | Windows and shell | `window_visibility` |
| `0x0814` | 1 | Required | Windows and shell | `window_user_data` |
| `0x0815` | 1 | Required | Windows and shell | `synthetic_create_sent` |
| `0x0816` | 1 | Required | Windows and shell | `synthetic_size_sent` |
| `0x0817` | 1 | Required | Windows and shell | `create_frame` |
| `0x0818` | 1 | Required | Windows and shell | `create_frame_hwnd` |
| `0x0819` | 1 | Required | Windows and shell | `create_frame_stack` |
| `0x081a` | 1 | Required | Windows and shell | `create_stage` |
| `0x081b` | 1 | Required | Windows and shell | `dialog_frame` |
| `0x081c` | 1 | Required | Windows and shell | `dialog_frame_hwnd` |
| `0x081d` | 1 | Required | Windows and shell | `active_window` |
| `0x081e` | 1 | Required | Windows and shell | `window_ids` |
| `0x081f` | 1 | Required | Windows and shell | `scroll_info` |
| `0x0820` | 1 | Required | Windows and shell | `window_extra` |
| `0x0821` | 1 | Required | Windows and shell | `dialog_default_frames` |
| `0x0822` | 1 | Required | Windows and shell | `dialog_frame_stack` |
| `0x0823` | 1 | Required | Windows and shell | `parked_dialog_callbacks` |
| `0x0824` | 1 | Required | Windows and shell | `window_destroy_frames` |
| `0x0825` | 1 | Required | Windows and shell | `control_notify_frames` |
| `0x0826` | 1 | Required | Windows and shell | `gesture_frames` |
| `0x0827` | 1 | Required | Windows and shell | `dialog_key_frames` |
| `0x0828` | 1 | Required | Windows and shell | `accelerator_frames` |
| `0x0829` | 1 | Required | Windows and shell | `shell_status` |
| `0x082a` | 1 | Required | Windows and shell | `status_bar` |
| `0x082b` | 1 | Required | Windows and shell | `controls` |
| `0x082c` | 1 | Required | Windows and shell | `modal` |
| `0x082d` | 1 | Required | Windows and shell | `shell_keyboard` |
| `0x082e` | 1 | Required | Windows and shell | `message_box_spins` |
| `0x082f` | 1 | Required | Windows and shell | `modal_dialog` |
| `0x0830` | 1 | Required | Windows and shell | `notification_icons` |
| `0x0831` | 1 | Required | Windows and shell | `menus` |
| `0x0832` | 1 | Required | Windows and shell | `menu_items` |
| `0x0833` | 1 | Required | Windows and shell | `window_menus` |
| `0x0834` | 1 | Required | Windows and shell | `next_menu_handle` |
| `0x0835` | 1 | Required | Windows and shell | `sub_menus` |
| `0x0836` | 3 | Optional | Windows and shell | `edit_controls` |
| `0x0837` | 1 | Optional | Windows and shell | `indeterminate_checks` |
| `0x0838` | 1 | Optional | Windows and shell | `window_create_scratch` |
| `0x0839` | 1 | Optional | Windows and shell | `clipboard` |
| `0x083a` | 1 | Optional | Windows and shell | `document_lists` |
| `0x0901` | 1 | Required | Devices and notifications | `serial` |
| `0x0902` | 1 | Required | Devices and notifications | `pocketpc_processor` |
| `0x0903` | 1 | Required | Devices and notifications | `legacy_winsock` |
| `0x0904` | 1 | Required | Devices and notifications | `winsock_last_errors` |
| `0x0905` | 1 | Required | Devices and notifications | `msg_queues` |
| `0x0906` | 1 | Required | Devices and notifications | `next_msg_queue_handle` |
| `0x0907` | 1 | Required | Devices and notifications | `registry_notifications` |
| `0x0908` | 1 | Required | Devices and notifications | `power_notifications` |
| `0x0909` | 1 | Required | Devices and notifications | `device_notification_queues` |
| `0x090a` | 1 | Required | Devices and notifications | `next_device_notification` |
| `0x090b` | 1 | Required | Devices and notifications | `battery_alerts` |

## Validation sources

Record framing, optional defaults and migration checks live in
[`pocket-snapshot::fields`](../crates/pocket-snapshot/src/fields.rs). Process
capture/restore checks live beside
[`KernelSnapshot`](../crates/pocket-kernel/src/kernel_snapshot.rs); full-session
checks live in [`pocket-core`](../crates/pocket-core/src/snapshot.rs). Follow the
[shared snapshot guide](SNAPSHOTS.md) for restore ownership and preflight rules,
and the [validation index](testing/README.md) for available dated evidence.
