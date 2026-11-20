# WinCE record format (container version 12)

Gizmondo and PocketPC use this same process archive on Desktop and Android.
The capture/restore resource owners and their validation are unchanged. This is
an envelope change, not a new emulated kernel or a duplicate snapshot engine.

## Stream and ownership

The native section with required tag 3 now uses version 12. Its payload is a
little-endian u16 record count followed by records with u16 ID/flags, u16 payload
version and u64 payload length, then the typed payload. Bit 15 marks required
records; the remaining ID has a domain byte and a field byte. These numeric
keys are permanent. Never renumber keys when inserting or rearranging fields.

All current guest-state records are required. An unknown optional record can be
skipped; an unknown required record, unsupported known version, changed required
flag, duplicate/zero ID, missing required state, truncated/extra payload or size
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

All records below are required and currently use payload version 1.

| ID | Domain | Field |
| --- | --- | --- |
| `0x0101` | Memory | `heap` |
| `0x0102` | Memory | `memory_profile` |
| `0x0201` | Files and handles | `vfs` |
| `0x0202` | Files and handles | `mappings` |
| `0x0203` | Files and handles | `handles` |
| `0x0204` | Files and handles | `find_handles` |
| `0x0205` | Files and handles | `next_find_handle` |
| `0x0301` | Process services | `registry` |
| `0x0302` | Process services | `crypto` |
| `0x0303` | Process services | `msxml` |
| `0x0304` | Process services | `sdl` |
| `0x0305` | Process services | `cedb` |
| `0x0306` | Process services | `internet` |
| `0x0307` | Process services | `zlib` |
| `0x0401` | Media | `audio` |
| `0x0402` | Media | `gz_files` |
| `0x0501` | Graphics | `gdi` |
| `0x0502` | Graphics | `display` |
| `0x0503` | Graphics | `fb_mapped` |
| `0x0504` | Graphics | `guest_fb_pitch` |
| `0x0505` | Graphics | `direct_fb_frames` |
| `0x0506` | Graphics | `gapi_column_major` |
| `0x0507` | Graphics | `gx_last_pushed_counter` |
| `0x0508` | Graphics | `gx_draw_active` |
| `0x0509` | Graphics | `gx_draw_thread` |
| `0x050a` | Graphics | `gx_explicit_present` |
| `0x050b` | Graphics | `gx_guest_signature` |
| `0x050c` | Graphics | `gles_strings` |
| `0x050d` | Graphics | `gapi_keys_queried` |
| `0x050e` | Graphics | `gapi_action_key` |
| `0x0601` | Scheduler and continuations | `schedule` |
| `0x0701` | Process and modules | `module_path` |
| `0x0702` | Process and modules | `process_launch_enabled` |
| `0x0703` | Process and modules | `family` |
| `0x0704` | Process and modules | `command_line_cache` |
| `0x0705` | Process and modules | `command_line` |
| `0x0706` | Process and modules | `next_process_id` |
| `0x0707` | Process and modules | `random_seed` |
| `0x0708` | Process and modules | `resources` |
| `0x0709` | Process and modules | `image_base` |
| `0x070a` | Process and modules | `image_size` |
| `0x070b` | Process and modules | `image_entry` |
| `0x070c` | Process and modules | `dynamic_exports` |
| `0x070d` | Process and modules | `image_imports` |
| `0x070e` | Process and modules | `gles_local_ordinals` |
| `0x070f` | Process and modules | `runtime_thunks` |
| `0x0710` | Process and modules | `module_attach_frames` |
| `0x0711` | Process and modules | `static_module_roots` |
| `0x0712` | Process and modules | `dll_notification_frame` |
| `0x0713` | Process and modules | `next_module_handle` |
| `0x0714` | Process and modules | `modules` |
| `0x0715` | Process and modules | `next_module_base` |
| `0x0716` | Process and modules | `free_module_bases` |
| `0x0801` | Windows and shell | `wnd_proc` |
| `0x0802` | Windows and shell | `window_class_procs` |
| `0x0803` | Windows and shell | `window_background` |
| `0x0804` | Windows and shell | `window_background_erase_pending` |
| `0x0805` | Windows and shell | `pending_create` |
| `0x0806` | Windows and shell | `pending_startup` |
| `0x0807` | Windows and shell | `window_procs` |
| `0x0808` | Windows and shell | `window_userdata` |
| `0x0809` | Windows and shell | `window_classes` |
| `0x080a` | Windows and shell | `window_titles` |
| `0x080b` | Windows and shell | `window_class_controls` |
| `0x080c` | Windows and shell | `control_class_prototypes` |
| `0x080d` | Windows and shell | `window_styles` |
| `0x080e` | Windows and shell | `window_parents` |
| `0x080f` | Windows and shell | `window_threads` |
| `0x0810` | Windows and shell | `window_input_contexts` |
| `0x0811` | Windows and shell | `window_rects` |
| `0x0812` | Windows and shell | `window_dcs` |
| `0x0813` | Windows and shell | `window_visibility` |
| `0x0814` | Windows and shell | `window_user_data` |
| `0x0815` | Windows and shell | `synthetic_create_sent` |
| `0x0816` | Windows and shell | `synthetic_size_sent` |
| `0x0817` | Windows and shell | `create_frame` |
| `0x0818` | Windows and shell | `create_frame_hwnd` |
| `0x0819` | Windows and shell | `create_frame_stack` |
| `0x081a` | Windows and shell | `create_stage` |
| `0x081b` | Windows and shell | `dialog_frame` |
| `0x081c` | Windows and shell | `dialog_frame_hwnd` |
| `0x081d` | Windows and shell | `active_window` |
| `0x081e` | Windows and shell | `window_ids` |
| `0x081f` | Windows and shell | `scroll_info` |
| `0x0820` | Windows and shell | `window_extra` |
| `0x0821` | Windows and shell | `dialog_default_frames` |
| `0x0822` | Windows and shell | `dialog_frame_stack` |
| `0x0823` | Windows and shell | `parked_dialog_callbacks` |
| `0x0824` | Windows and shell | `window_destroy_frames` |
| `0x0825` | Windows and shell | `control_notify_frames` |
| `0x0826` | Windows and shell | `gesture_frames` |
| `0x0827` | Windows and shell | `dialog_key_frames` |
| `0x0828` | Windows and shell | `accelerator_frames` |
| `0x0829` | Windows and shell | `shell_status` |
| `0x082a` | Windows and shell | `status_bar` |
| `0x082b` | Windows and shell | `controls` |
| `0x082c` | Windows and shell | `modal` |
| `0x082d` | Windows and shell | `shell_keyboard` |
| `0x082e` | Windows and shell | `message_box_spins` |
| `0x082f` | Windows and shell | `modal_dialog` |
| `0x0830` | Windows and shell | `notification_icons` |
| `0x0831` | Windows and shell | `menus` |
| `0x0832` | Windows and shell | `menu_items` |
| `0x0833` | Windows and shell | `window_menus` |
| `0x0834` | Windows and shell | `next_menu_handle` |
| `0x0835` | Windows and shell | `sub_menus` |
| `0x0901` | Devices and notifications | `serial` |
| `0x0902` | Devices and notifications | `pocketpc_processor` |
| `0x0903` | Devices and notifications | `legacy_winsock` |
| `0x0904` | Devices and notifications | `winsock_last_errors` |
| `0x0905` | Devices and notifications | `msg_queues` |
| `0x0906` | Devices and notifications | `next_msg_queue_handle` |
| `0x0907` | Devices and notifications | `registry_notifications` |
| `0x0908` | Devices and notifications | `power_notifications` |
| `0x0909` | Devices and notifications | `device_notification_queues` |
| `0x090a` | Devices and notifications | `next_device_notification` |
| `0x090b` | Devices and notifications | `battery_alerts` |

## Validation on 20 November 2026

The cumulative source passed 1260 Rust unit tests: 36 core, 420 kernel,
799 WinCE API and 5 shared snapshot codec tests. 37 context/hardware tests
remain ignored. The frozen process fixture passed a cold kernel restore with
its pending GDI update region and suspended message identity intact. Its SHA-256
is `36a5efca0e46b36360dce2ebe06072bb8ab4ee211cb34e55e9e8fbdef74d48fd`
(57970 bytes, compressed synthetic metadata; no game assets).

Real guest executables were captured to container-12 native streams and
restored in fresh emulator processes: Stuntcar Extreme (Gizmondo/GLES1), MoonGBA
(PocketPC/GDI), and Electopia (PocketPC/GLES2, 800×480). The restored render loops
continued without a CPU exception. The first Electopia probe omitted its GLES2
profile and reached an initialization error; after matching the launcher's GPU
profile/screen setup, the actual GLES2 capture and cold restore passed. No
product launch workaround was needed. Diagnostic code and game snapshots are
excluded from the source archive.

Desktop and Android JNI host checks passed with Unicorn and optional host
audio/static-video features disabled. Rust 1.90 formatting and whitespace checks
passed. Native graphics runs use headless Mesa, not physical Android/Windows
hardware. No fresh APK or native ABI release build is claimed here.
