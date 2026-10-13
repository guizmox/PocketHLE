# PocketHLE — architecture reference for coding agents

## Gizmondo VIB1 / NLED vibration bridge

The SDK Vibrator and RingToneControl examples use VIB1: and NLED count (0),
supports (1), settings (2) IOCTLs. COREDLL NLedGetDeviceInfo/NLedSetDevice share
that same service. There is one virtual motor, LedNum 0. Settings are seven
32-bit fields (28 bytes); blink times are microseconds, converted upward to
Android's millisecond resolution. The count structure is 4 bytes, supports 28.
Modes are off, continuous on and blink, with meta cycles. Oversized meta patterns
(>1024 on cycles) return ERROR_NOT_SUPPORTED; malformed inputs return 87.
Off/on do not require initialization of the blink-only fields. No intensity
field exists in this ABI. Android uses amplitude 128/255 when supported, otherwise
native fixed power on API24+; do not claim uniform physical strength across phones.

VfsShared owns the Service; shared opens/leases support process duplication and
access checks. Closing VIB1 MUST NOT cancel the motor: the SDK closes it before
Sleep and reopens it to stop. Stop releases the native Motor immediately; last
shared-device teardown also cancels and releases it. Failed first activation
releases its newly opened native motor. The JNI bridge retains only JavaVM and
GlobalRef to NativeBridge, never an Activity. Local JNI array references are scoped.
VibrationHost retains application-derived Vibrator only, bounded session waveforms
and no worker/timer thread. Android pause cancels output; resume restores the latest
requested state. closeAll gates late native calls, clears waveforms and cancels;
initialize reopens the gate for the next game, without reusing old session IDs.
Virtual-button feedback is skipped while a game waveform is active so it cannot
replace an indefinitely repeating game vibration. Hardware absence and native
failures return real LastError (50/5/21), not false success. Successful APIs preserve
LastError. Desktop uses the SDL controller vibration backend when a supported controller
is connected; unsupported hardware retains explicit errors.

Rust regressions cover ABI, sharing/duplication, LastError, output sizes, waveform
conversion and 100 activation/stop releases including failed starts and teardown.
The actual Android host source is compiled against Android14 APIs and exercised
on JVM against fake Android services for amplitude fallback, legacy API24, errors,
pause/resume, stop and stale IDs. These are not physical Android vibration tests.
VIBTEST is a freestanding ARM PE built without proprietary SDK code. Import its
ZIP to hear/feel a short continuous buzz, regular pulses, then paired pulses;
check VIBTEST.TXT and verify that Home/pause and Exit stop the motor.

## WinINet HTTP/HTTPS host bridge

The nine WinINet imports used by Colors are implemented in `wininet.rs`.
`KernelState.internet` owns process-local sessions, connections and requests;
closing a parent recursively cancels its requests. These are InternetCloseHandle
objects, not VFS CloseHandle objects. Dynamic wininet.dll exports have their own
module handle. Guest blocking calls retry through scheduler deadlines, retaining
one pending upload per (thread, thunk, SP). A retry MUST NOT duplicate the POST.
QueryInfoW lengths are bytes; successful strings exclude the final UTF-16 NUL.
HTTP error status is still a successful transport response. Empty pending data
is not EOF. Response buffers are capped at 64 KiB; uploads at 8 MiB, request
handles at 64/process. Unsupported FTP, guest async callbacks, SYSTEMTIME queries
and certificate-bypass flags report real errors, never fake success.

Windows uses async WinHTTP with per-session cookies. Only the worker closes
request handles after an initiating API call returns. Callback context owns body,
read buffer and parent native handles until HANDLE_CLOSING (last callback), even
when canceled mid-request. Closing a transfer wakes bounded-stream backpressure;
callback waits observe cancellation within 100 ms. Native timeouts are 30 seconds.
Android uses HttpURLConnection workers, bounded ring buffers and per-session
CookieManager. JNI transfers retain their session until request close; closing
wakes a blocked producer and schedules native disconnect without blocking the
emulator thread. INTERNET permission and legacy cleartext HTTP are required;
HTTPS retains platform certificate/hostname verification. No trust-all callback.
Original game servers are separate external dependencies; implementing the ABI
does not guarantee that historical services still exist. NETTEST ARM checks all
nine dynamic exports and real-host HTTP/HTTPS. The local validation transport
checks ARM ABI + loopback streaming/POST, and does not validate native TLS.

## GPS1 host location bridge

`gps::Service` belongs to the shared VFS device context, not to a single CPU.
The permission gate defaults off (`gps_enabled`). Open descriptions retain an
Arc capture and sharing lease through cross-process duplication; the service
retains only a Weak device reference. Last close/close_all releases the capture.
ReadFile requires the exact SDK packed LATEST_GPS_DATA size (180 bytes), with
unaligned little-endian fields and 32-bit BOOL. Latitude/longitude are degrees
×10^7; altitude, speed, course and errors use two decimal places. Timestamp uses
the documented 1972 UTC epoch; GPS week/time include the historical GPS–UTC leap
offset through the last leap second in 2017. No-fix snapshots have timestamp and
validation zero, with unknown errors UINT32_MAX. Old host fixes lose validation
after 30 seconds. Host position validity maps to FixValidated for game compatibility
(Colors checks it); this is not a claim of SiRF four-satellite validation. Satellite
counts and arrays remain zero, since generic host location does not expose them.
Only known MSL altitude is returned; ellipsoid altitude is not relabeled MSL.

Windows uses Geolocator events on an MTA with a single latest-position mailbox.
Consent preparation MUST run on the desktop UI thread while foregrounded;
`spawn_run` and enabled-option UI updates prepare it before the guest worker.
Capture Drop signals the worker, which removes both event subscriptions. Event
closures retain only Weak mailboxes. Missing permission/service/position is never
turned into a synthetic location. Android uses foreground LocationManager
subscriptions (fine GPS plus network fallback, or coarse network), application
context only, one HandlerThread and one latest packet per session. pause removes
updates; resume restarts live sessions; close/closeAll remove updates permanently.
Coarse and fine permissions must be requested together for precise Android12+
location. Prefer a recent more accurate GPS update over a worse network update.

The current GPS scope is CreateFile/ReadFile/CloseHandle/duplication and native
location; geofencing, SiRF/APM and undocumented version IOCTLs explicitly return
50 rather than pretending success. No host clock changes or GNS notifications.
GPSTEST ARM checks the binary ABI, errors and repeated open/close separately
from native fix availability. Colors' actual ARM GPS constructor/read/destructor
has been exercised with a deterministic provider; full gameplay and physical
Windows/Android fixes still require hardware testing.

PocketHLE runs Windows CE / Windows Mobile (Pocket PC) applications on
modern hosts by **high-level emulation**: the guest's ARM (or MIPS) code
is executed instruction-by-instruction, but every call into a Windows CE
DLL is intercepted at the import boundary and serviced by clean-room Rust
instead of by emulated OS code. There is no emulated `coredll.dll`, no
emulated kernel, and no emulated device drivers.

Read this file before changing anything. It records the invariants that
are *not* recoverable from reading a single file, and the places where an
obvious-looking change breaks a specific shipped game. Everything here
was verified against the tree it ships with — when you change behaviour,
update this file in the same commit.

## 1. Non-negotiable invariants

Violating any of these breaks games that currently work. They are listed
first because they are the ones most often broken by a plausible-looking
refactor.

1. **Dispatcher keys always carry the `.dll` suffix, lower-cased.**
   Handler keys look like `("coredll.dll", "MessageBoxW")`. Four
   independent consumers assume this: `WinCeDispatcher::resolve_handler`,
   `native_thunks::native_thunk_for`, `Dispatcher::constant_for`, and the
   hard-coded DLL list in `Process::map_into`
   (`crates/pocket-kernel/src/lib.rs:1620`). CeGCC images write the bare
   name (`COREDLL`), so `pocket-pe` normalizes it at the loader boundary —
   see §7.
2. **`Dispatcher::constant_for` must be deterministic per thunk.** The
   kernel calls it exactly once per import at load time and bakes the
   answer into the guest's IAT. A handler that needs to observe mutable
   state cannot be a constant.
3. **The guest entry point is entered with `WinMain` arguments already in
   registers.** The CE loader does this, so PocketHLE does too:
   `r0 = hInstance` (`PROCESS_INSTANCE_HANDLE`), `r1 = 0`,
   `r2 = lpCmdLine`, `r3 = nCmdShow` (`SW_SHOWNORMAL`). `lpCmdLine` is
   **always a valid pointer** — empty `L""` when there are no arguments,
   never `NULL`. Games dereference it without checking.
4. **`LR` on entry is `PROCESS_EXIT_TRAMPOLINE_VA`, not 0.** When
   `mainCRTStartup` eventually returns, the CPU lands on a hooked address
   and shuts down gracefully. With `LR = 0`, every game "crashes" at
   `pc=0x00000000` after a completely successful run.
5. **`eglSwapBuffers` is the only point GL output becomes visible**, and
   when GAPI is also mapped it must push pixels into the guest mapping.
   See §6 — this is a fixed bug with a regression test; don't reintroduce
   it.
6. **`WaitForSingleObject` must respect real event state.** Answering
   every wait with `WAIT_OBJECT_0` makes worker threads exit on their
   first iteration; Asphalt 2 3D opens its wave device and then never
   writes a single buffer. An `INFINITE` wait from a *worker* is also a
   scheduling point: it re-parks the thread at its own thunk so the call
   re-runs with its arguments intact — see §10.
7. **Worker threads never receive the window queue's synthetic traffic.**
   A worker running its own pump would dispatch forever and the thread
   that actually renders would never get the CPU back
   (`crates/pocket-winceapi/src/coredll.rs:6686` and `:6743`).
8. **`arch_all` is deliberately disabled on `unicorn-engine`.** Enabling
   it breaks the Android NDK cross-compile on QEMU's x86 `cpuid.h`.
9. **`CreateThread` returns to its creator; it never enters the new
   thread.** Every thread parks at its entry point, `CREATE_SUSPENDED` or
   not — see §10.
10. **`frame_counter` moves only when pixels change.** It is the host's
    "a new frame is ready" signal, and every frontend's frame budget is
    spent off it. `InvalidateRect` bumping it cost Toy Golf its entire
    `--max-frames` budget on identical black startup frames — see §6.
11. **`GetClassInfoW` must distinguish registered classes from unknown
    ones.** Chopper Fight probes its private classes before `RegisterClassW`;
    a false success leaves both windows without a guest procedure and the
    framebuffer never advances. Supported built-in controls still count as
    registered.
12. **GDI queries and text output reflect the requested DC's live state.**
    `GetCurrentObject` and `GetTextColor` read `GdiState`; Chopper Fight
    queries them while painting menus, so placeholder values lose selected
    objects and text colors. Text drawn into a DIB-backed memory DC must be
    flushed into the guest bitmap before a later `BitBlt`, or its menu
    labels disappear while the textures still render.
13. **CAB install paths remain rooted at the shared app directory.** Chopper
    Fight places files only under `bin/`, `resources/`, and `manual/`; infer
    their common parent, materialize `.000` destinations before unpacking
    nested ZIPs, and resolve `..` only while it stays inside that mount.
14. **`CreateDC(NULL, …)` is the screen DC on Windows CE.** Chopper Fight
    creates a display DC when it enters gameplay; return `GDI_SCREEN_DC` for
    that request and treat its `DeleteDC` as a no-op.
15. **`WM_LBUTTONUP` clears `MK_LBUTTON` from `wParam`.** Synthetic taps follow
    the Win32 message ABI; Chopper Fight's sprite controls need the release
    edge to arrive with the button state cleared.

## 2. Crate graph

Dependencies point downward; nothing below depends on anything above.

```
frontends/  pocket-cli      pocket-desktop (egui)   pocket-android-jni
                  \                |                      /
                   +-------- pocket-core -----------------+   orchestration:
                                  |                          load, run loop, frames
                   +--------------+----------------+
                   |              |                |
            pocket-winceapi  pocket-kernel   pocket-library
            (guest DLL         (state,         (game catalog,
             surface)           memory map,     install metadata)
                   |            run loop)
                   |              |      \
            pocket-gles     pocket-cpu    pocket-pe --- pocket-cab
            (GL backends)   (Unicorn)     (PE32 loader)  (installers)
```

| Crate | Responsibility |
| --- | --- |
| `pocket-cab` | Extracts CAB, InstallShield and zip installers; decides which install directories to mount. |
| `pocket-pe` | Parses and lays out PE32 images; collects imports, exports, resources, icons. Detects managed (.NET CF) images. |
| `pocket-cpu` | `Cpu` trait over two backends: `stub` (trace-only) and `unicorn` (Unicorn Engine 2). |
| `pocket-kernel` | The address space, `KernelState`, thunk/IAT machinery, the run loop, and host subsystems: VFS, registry, framebuffer, GDI, GAPI, audio, fonts, message boxes, native thunks. |
| `pocket-winceapi` | The guest-facing API surface. One module per emulated DLL; registers handlers into `WinCeDispatcher`. |
| `pocket-gles` | OpenGL ES 1.x emulation with software and native GPU backends: geometry, rasterization, textures, fixed-point and matrix math. Host-side only — knows nothing about guest memory. |
| `pocket-core` | Wires loader + CPU + kernel + dispatcher into an `Emulator` the frontends drive. |
| `pocket-library` | Game catalog: identifies titles, tracks install state and per-game metadata. |

Frontends: `pocket-cli` (headless, scriptable — the one to use for
debugging), `pocket-desktop` (egui), `pocket-android-jni` plus the Kotlin
app in `pocket-android`.

## 3. The guest address space

Every constant below lives in `crates/pocket-kernel/src/lib.rs`. The map
is fixed, not discovered — a game that hard-codes an address is relying on
these exact values.

| Region | Base | Size / stride | Purpose |
| --- | --- | --- | --- |
| Slot 0 alias | `0x0001_0000` | `0x0002_0000` | `SLOT_ALIAS_BASE`. WinCE ran the active process aliased into slot 0; images based at `0x0001_0000` are mapped here *as well as* at their own base. |
| Module region | `0x3000_0000` | stride `0x0100_0000`, end `0x4000_0000` | `MODULE_REGION_BASE`. Where an image goes when its requested base is taken or unusable. 16 slots. |
| Heap | `0x5000_0000` | `0x0400_0000` | `HEAP_BASE` / `HEAP_SIZE`. Backs `malloc`, `LocalAlloc`, `VirtualAlloc`, `HeapAlloc` — one bump/free allocator, not per-heap. |
| Stack | top `0x6000_0000` | `0x40000` | `DEFAULT_STACK_TOP` / `DEFAULT_STACK_SIZE`. Grows down. Each extra thread gets its own stack below this. |
| Thunk pool | `0x7000_0000` | `THUNK_STRIDE` = 32 | `THUNK_REGION_BASE`. One 32-byte slot per import. The slot address *is* the identity of the import — see §5. |
| Synthetic framebuffer | `0x7800_0000` | screen-sized | `SYNTHETIC_FRAMEBUFFER_BASE`. What `GXBeginDraw` hands the guest when there is no real device buffer. |
| Kernel traps | `0xF000_0000` | `0x0001_0000` | `KERNEL_TRAP_BASE`. Hooked addresses that are never real code. |
| Process exit | `0xF000_FF00` | — | `PROCESS_EXIT_TRAMPOLINE_VA`. Initial `LR`; see invariant 4. |
| Thread exit | `0xF000_FE00` | one per thread | `THREAD_EXIT_TRAMPOLINE_BASE`. Initial `LR` for spawned threads. |
| User KData page | `0xFFFF_C000` | `0x1000` | `USER_KDATA_PAGE_BASE`. CE's shared read-only kernel page. `USER_KDATA_STRUCT_VA` = `0xFFFF_C800`; the TLS array is at `USER_KDATA_TLS_ARRAY_VA` = `0xFFFF_CB00` with `TLS_SLOT_COUNT` = 64. Games read the tick count straight out of this page instead of calling `GetTickCount`, so it must be kept current every slice. |

Sentinel handles, deliberately outside every mapped region so a guest
that dereferences one faults loudly instead of corrupting data:

| Constant | Value |
| --- | --- |
| `FAKE_CURRENT_THREAD_HANDLE` | `0xC0DE_0001` |
| `FAKE_CURRENT_PROCESS_HANDLE` | `0xC0DE_0002` |
| `PROCESS_INSTANCE_HANDLE` | `0x1000_0000` |
| `GLES_CM_MODULE_HANDLE` | `0x1000_0004` |
| `GLES_CL_MODULE_HANDLE` | `0x1000_0005` |
| `HSS_MODULE_HANDLE` | `0x1000_0006` |

`pocket-pe` deliberately does **not** apply WinCE's XIP-in-ROM mapping or
per-process slot relocation. Each game gets a private flat 32-bit space,
so one contiguous mapping is enough; base relocations are applied only
when the requested image base is not free.

## 4. Loading, and the three IAT strategies

**Which file to load is a decision of its own.** A cabinet from this era
often ships one executable per 3D chip and leaves the choice to a setup
DLL that runs at install time. Call of Duty 2's `SETUPDLL.999` probes
`Software\NVIDIA Corporation\GFSDK` and `\Windows\wmv9decoder2700g.dll`,
then renames `cod2_goforce.exe` or `cod2_gles.exe` over the `cod2.exe`
its shortcut points at. PocketHLE runs no install-time DLLs, so following
the shortcut lands on the software renderer — a correct run at 13.7 fps
where the GoForce build does 39.1 fps on the same scenario, with the
whole GL ES layer sitting unused.
`pocket_library::accelerated_renderer_build` makes the choice the setup
DLL would have: when the shortcut target imports none of the GL ES
drivers we emulate, it looks for a sibling executable whose name extends
the target's stem, prefers the one importing the driver the cabinet
itself ships, and hands that back. The guest keeps seeing the installed
name — `GameEntry::launch_path` and the CLI's CAB path swap the file
behind the recorded `guest_exe_path`, so `GetModuleFileNameW` still
answers `\Program Files\COD2\cod2.exe`. Pinned by
`a_cabinet_that_ships_a_hardware_renderer_build_launches_it` and
`the_driver_the_cabinet_ships_decides_between_two_hardware_builds`.

`Process::map_into` (`crates/pocket-kernel/src/lib.rs:1453`) is the whole
load path. In order:

1. Map each section at `image_base + virtual_address`, honouring the
   section characteristics for permissions.
2. If the image is based at `SLOT_ALIAS_BASE`, map it a second time into
   the slot-0 alias window.
3. Map the thunk pool, one `THUNK_STRIDE`-byte slot per import.
4. Map the User KData page and prime the tick fields.
5. For each import, pick **one** of the three strategies below and write
   the resulting address into `iat_va`.
6. Register dynamic exports so `GetProcAddress` can resolve names that
   were never in the import directory.

The strategies, in the order they are tried:

| Order | Strategy | Condition | Cost |
| --- | --- | --- | --- |
| 1 | **Native ARM/VFP code** written into the thunk slot | `native_thunks::native_thunk_for` returns code. Gated on `dll.eq_ignore_ascii_case("coredll.dll")` (`native_thunks.rs:225`) and **never** used for MIPS. | Zero host round-trips. `memcpy`, `strlen`, float helpers. |
| 2 | **Constant return**: `mov r0, #imm; bx lr` | Native declined *and* `cpu.arch() == Arm` *and* `Dispatcher::constant_for` answers. | Zero host round-trips. Derby's imports hit this ~33% of the time. |
| 3 | **`bx lr` + code hook** → Rust `Handler` | Everything else. `ARM_BX_LR` = `[0x1e, 0xff, 0x2f, 0xe1]`; MIPS gets `MIPS_JR_RA`. | One host call per guest call. |

Strategy 2 is why invariant 2 exists: `constant_for` is consulted once,
at load time, and its answer is frozen into guest memory. A handler whose
return value depends on mutable state must be strategy 3.

The dynamic-export DLL list in step 6 is hard-coded at
`crates/pocket-kernel/src/lib.rs:1620`: `coredll.dll`, `commctrl.dll`,
`gx.dll`, `ddraw.dll`, `libgles_cm.dll`, `libgles_cl.dll`, `hss.dll`.
Adding a module to `pocket-winceapi` without adding it here means
`GetProcAddress` on it returns `NULL`.

## 5. Dispatch

A `Thunk` (`crates/pocket-kernel/src/lib.rs:1211`) is the unit of
identity for an intercepted import:

```rust
pub struct Thunk {
    pub thunk_va: u32,        // slot in the thunk pool — the primary key
    pub iat_va: u32,          // where in the guest IAT the address was written
    pub dll: String,          // lower-cased, WITH the .dll suffix
    pub binding: ImportBinding,
    pub friendly_name: String,
}
```

A handler returns a `DispatchOutcome` (`:521`):

| Variant | Meaning |
| --- | --- |
| `ReturnedR0(u32)` | Normal return. `void` handlers return 0; the guest ignores it. |
| `ReturnedR0R1(u32, u32)` | 64-bit return, low word in r0. |
| `JumpTo(va)` | Resume the guest at `va` instead of `LR`. Used to block modally by re-entering the same thunk — see `message_box_w`. |
| `Halt(reason)` | Stop the run loop. `ExitProcess`, `TerminateProcess`. |

**`resolve_handler` memoizes by `thunk_va` and import identity, including negative results**
(`crates/pocket-winceapi/src/lib.rs:280`). Runtime DLL slots may be reused after `FreeLibrary`: the cached DLL,
binding and friendly name must still match before a cached handler is used.
Changed import metadata at the same address replaces the cache entry. Handler
registration explicitly clears the cache.
Unresolved lookups fall back to `ignored_dll` → `ignored_dll_stub`.
`IGNORED_DLLS` (`:83`) currently holds only the `fmodce` prefix, with a
`FSOUND_GetVersion` → `0x4070_0000` override so version checks pass.

WinCE `coredll`, `aygshell` and the GLES libraries are frequently imported
**by ordinal**, with no name in the import directory at all. The ordinal →
name tables are JSON data files, not code:
`crates/pocket-winceapi/data/coredll-ordinals.json`,
`aygshell-ordinals.json`, and `crates/pocket-gles/data/`
`libgles_cl-ordinals.json` / `libgles_cm-ordinals.json`. A game whose
imports all show as `ord NNN` in the trace is missing an entry here, not a
handler.

One module per emulated DLL in `crates/pocket-winceapi/src/`: `aygshell`,
`commctrl`, `coredll`, `ddraw`, `dlgtemplate`, `game_dlls`, `gles`, `gx`,
`hss`, `ole32`, plus `ordinals` and `lib`. Host-side subsystems live in
`crates/pocket-kernel/src/`: `audio`, `controls`, `font`, `framebuffer`,
`gapi`, `gdi`, `msgbox`, `native_thunks`, `registry`, `tracker`, `vfs`.

## 6. Presentation — two paths

Everything a user sees arrives by exactly one of these:

**EGL / GLES.** The guest draws through `libGLES_CM` / `libGLES_CL` into
`pocket-gles`'s software rasterizer, and `eglSwapBuffers` converts the
RGBA8888 result to RGB565 and publishes it. Nothing before
`eglSwapBuffers` is visible — invariant 5.

**GAPI (`gx.dll`).** The guest asks `GXBeginDraw` for a raw framebuffer
pointer, writes pixels directly, and calls `GXEndDraw`.
`sync_guest_framebuffer` (`:1938`, called from the run loop at `:2288`)
reads that mapping back out of guest memory each slice.

**DirectDraw is a third way in, and its vtables are CE's, not the
desktop's.** `crates/pocket-winceapi/src/ddraw.rs` hands the guest COM
objects whose vtables it writes itself, so the slot order *is* the ABI.
Windows CE does not stub out the DirectDraw methods it lacks — they are
absent from the vtable, and everything after shifts down. Against the
Windows Mobile 5.0 SDK `ddraw.h`: `IDirectDraw` has no `Compact`,
`DuplicateSurface` or `Initialize` and gains five methods after
`WaitForVerticalBlank`; `IDirectDrawSurface` loses seven (including
`Initialize` and the `Blt` variants) and gains `GetDDInterface` and
`AlphaBlt`; the palette and clipper lose `Initialize`. `DDSURFACEDESC`
is 108 bytes with an `lXPitch` the desktop struct has no field for,
which puts `lpSurface` at 32 and the pixel format at 68. Copying the
desktop header into either place is the mistake this cost Tower Bloxx a
NULL clipper and a fault at `pc=0x00088d58`; the slots are pinned by
`the_vtables_follow_the_windows_ce_header`.

Each surface gets its own guest-heap pixels and carries them in its own
object, past the vtable pointer. Aliasing every surface onto the panel
looks like it works — a game drawing into its back buffer draws straight
onto the screen — right up to a game that clears the back buffer first:
Tower Bloxx starts each frame with a `DDBLT_COLORFILL`, which with one
shared buffer wiped the picture that the present blit then failed to put
back. `Unlock` publishes only for the primary; `Blt` and `Flip` publish
when the *destination* is the primary.

**When a game maps both**, `eglSwapBuffers` must also push the presented
pixels into the guest GAPI mapping. Call of Duty 2 does exactly this: it
sets up GAPI, then renders through GL, and without the push
`sync_guest_framebuffer` keeps reading the never-written GAPI mapping and
the screen stays black through a run that is otherwise completely
correct. The fix is guarded by `ctx.kernel.fb_mapped` in
`crates/pocket-winceapi/src/gles.rs` and pinned by
`swap_buffers_pushes_pixels_into_a_mapped_gapi_framebuffer`.

`frame_counter` is the liveness diagnostic *and* the host's "new pixels
are ready" signal — the CLI's frame-dump hook and `--max-frames` both
fire off it. `frame_counter=0` after a run that reported no errors means
the guest executed fine and nothing reached a presentation point — look
at the two paths above, not at the CPU.

**The display is 240x320 portrait unless something says otherwise**, which
is the Pocket PC these games mostly shipped on. Geometry is not a
preference a user can revise mid-run: a GAPI or GL ES title reads the
display size once during start-up and lays its whole scene out around it.
So a launcher that recognises the device an archive shipped on sets the
geometry before the run. `Launcher::native_screen`
(`frontends/pocket-cli/src/archive.rs`) carries that — a Gizmondo card
layout yields `GIZMONDO_SCREEN`, the console's 320x240 landscape LCD — and
`--screen` overrides it; `pocket-library`'s `ScreenPref::Landscape` is the
same fact for the desktop and Android launchers. At the portrait default,
Sticky Balls asked GL ES for a viewport the size of the display and
rendered a portrait slice of a landscape scene with its HUD off the edge.

**Rotation is presentation, geometry is not.** Some landscape-designed
games only draw correctly when they believe they are on a 240x320
portrait panel — JumpyBall and the Motorola Q build of Asphalt 2 both lay
out fine at the portrait default and come out wrong when `--screen`
forces 320x240, because the wrong branch of their start-up sizing runs.
For those, keep the guest portrait and turn the *picture*:
`pocket_library::RotationPref` (`none` / `cw90` / `half` / `ccw90`, per
game in `game.json`) never reaches the guest at all. The desktop applies
it as texture UVs in `rotation_uv` (`frontends/pocket-desktop/src/app.rs`)
and Android in `FrameRenderer` (`GameActivity.kt`), each letterboxing
against the *shown* size — width and height swap on a quarter turn. A
frontend that draws a rotated picture must apply the **inverse** turn to
pointer input, or taps land somewhere else on the guest screen: under
`Cw90`, a tap at the shown top-left is guest (0, 319).

The inverse failure is subtler and cost a day on Toy Golf: a handler that
bumps the counter *without* changing pixels spends the frame budget on
duplicates. `InvalidateRect` did exactly that, 99,490 times from Toy
Golf's idle loop, so every captured frame was the same black startup
image and the run ended before the game drew a thing — with 153,600
non-zero pixels sitting in the framebuffer the whole time. Only
`eglSwapBuffers` and `sync_guest_framebuffer` may move the counter.
`--dump-frame-stride` thins the captures of a GDI title that legitimately
blits far more often than it changes anything interesting.

The desktop Run screen's **Game-only fullscreen** action is a presentation
mode, not a guest screen-size change. It requests a fullscreen host viewport,
hides the launcher panels, virtual controls, status, and FPS overlay, then
paints against the full viewport `ctx.screen_rect()` and centers the rotated
framebuffer at the largest aspect-preserving size that fits. Using the
viewport rectangle instead of the panel's content rectangle keeps the game
centered after fullscreen resizing. Pointer input still maps through the same
inverse rotation. F11 exits and restores the fullscreen state from before the
mode; outside it, F11 keeps toggling ordinary borderless fullscreen.

Two rasterizer details that look like bugs and are not: an incomplete
texture samples as opaque white (matching GL ES), and the software GL
tests share a `TEST_LOCK` because the context is process-global.

Software GLES allocation optimizations (performance v1 delivery): fully inside
finite triangles with projected bounds at most 4x4 pixels bypass polygon
allocations/clipping; larger or intersecting primitives retain the clipper.
Lists, strips and fans stream in original order from assembled vertices.
CpuMem reads directly into reusable scratch with read_mem_into; failed reads
abort the draw without retaining guest pointers. Benchmarks are synthetic,
not game FPS, and run only when explicitly invoked.

Desktop and Android successful imports automatically open the existing game
name editor with the detected display name prefilled. Renaming persists by
stable game ID (not display-name matching, which may collide). Empty names
remain rejected; cancelling keeps the imported entry and detected name. This
applies to both Pocket PC and Gizmondo archives and raw executables.

**Texturing is a cascade, not one stage.** GL ES 1.1 multitexturing
applies the enabled units in ascending order, each combiner taking the
colour the previous unit produced, starting from the interpolated vertex
colour. `raster::draw_triangle` therefore takes a `&[TextureStage]` — one
entry per unit that has a complete texture bound and enabled, carrying
that unit's `glTexEnv` mode and colour — and `Vertex::texcoord` holds one
coordinate pair per unit. Sampling a single unit is not a simplification
that costs a little quality: Sticky Balls stores a ball as a
`GL_COMPRESSED_RGB_S3TC_DXT1_EXT` colour map, a format with no alpha
channel at all, on one stage, and a white-RGB texture whose shape lives
in its alpha on the next. The mask alone renders white balls; the colour
map alone leaves an opaque square around each one.

`MAX_TEXTURE_UNITS` (`context.rs`) is deliberately larger than the
`GL_MAX_TEXTURE_UNITS` we advertise. `glActiveTexture` past the maximum
is an error that leaves the selected unit *alone*, so an engine that
walks more stages than it asked about aims its per-unit calls at the last
stage it did select. X-Forge sets up stage 1 and then either binds a
second layer to stage 2 or clears stage 2 before the draw; with only two
stages tracked, that stage-2 traffic landed on stage 1 — the clear
unbound the texture just bound there, and Sticky Balls' sky came out as
an opaque white sheet. Tracking the stages games actually touch, plus a
spare, keeps the overflow harmless while a game that honours the query
still keeps to one stage.

## 7. Two guest toolchains

PocketHLE loads images from both the MSVC-era official toolchains and the
open-source CeGCC / mingw32ce one. They differ in ways that reach the
loader:

| | MSVC (eVC++ / VS2005-2008) | CeGCC / mingw32ce (GCC 4.1.0) |
| --- | --- | --- |
| Import DLL name | `COREDLL.dll` | `COREDLL` — **no extension** |
| CRT startup | `mainCRTStartup` | `crt3.c`, `__mingw_do_global_ctors` |
| Startup CRT calls | — | `_fpreset` before `main`, `_fcloseall` on the way into `ExitProcess` |
| Debug sections | MSVC-named | GNU-style numbered |

The suffix-less name is the one that matters. Every lookup path keys on
the lower-cased name **with** `.dll` (invariant 1), so a CeGCC image built
`("coredll", "malloc")` and missed `("coredll.dll", "malloc")` — and *every*
import silently degraded to an unimplemented stub. Because four
independent consumers share that assumption, the fix belongs at the single
boundary where the name enters the system, not in each consumer:
`normalize_import_dll` in `crates/pocket-pe/src/lib.rs` appends `.dll` to
a name that has no extension at all, and leaves anything already carrying
one alone (`.cpl` and `.drv` are real WinCE module extensions, and
`fmodce370.dll` must keep its dots).

`hello.exe` at the repository root is the CeGCC smoke test. A correct run
shows **zero** "unimplemented call" warnings, renders a "Test App /
Hello, World! / OK" message box, accepts Enter, and halts through
`ExitProcess`.

## 8. The synthetic message pump — read this before diagnosing a stall

There is no host message queue. `GetMessageW` and `PeekMessageW`
fabricate `WM_PAINT` / `WM_TIMER` traffic, and once
`synthetic_message_count` reaches `synthetic_message_budget` they
fabricate `WM_QUIT` (`crates/pocket-winceapi/src/coredll.rs:6682` and
`:6739`). The guest then runs its own perfectly ordinary shutdown.

A fabricated periodic `WM_PAINT` is not itself a background-erase request.
`DefWindowProcW` paints the class brush on the initial paint or after
`InvalidateRect(..., TRUE)`; an explicit `WM_ERASEBKGND` is always honored.
Otherwise a default window procedure can clear a frame rendered outside its
paint handler every 16 ms — Spider-Man - Toxic City exposed that as repeated
white flashes between its GDI blits.

The default differs by frontend, and this is the trap:

| Frontend | Budget |
| --- | --- |
| `pocket-cli` | **240 for most games** (`--message-budget`, `0` = unlimited); Cops & Robbers is auto-unlimited when the option is omitted |
| `pocket-desktop` | 0 — unlimited (`src/runner.rs:159`) |
| `pocket-android-jni` | 0 — unlimited (`src/runner.rs:332`) |

Cops & Robbers is the CLI exception: its GLU startup continues processing
messages past the 240-message cap. The cap sends `WM_QUIT` while the game is
still on its sound prompt, so the guest shuts down before its title/menu flow
and leaves `frame_counter` at 1. When the budget is omitted, the CLI detects
the game's installed module path and uses 0 (unlimited); an explicit
`--message-budget` still takes precedence. Other games keep the bounded 240
default, and the tap helper omits the flag unless the user supplies it.

**Frames stopping is not automatically a graphics bug.** Call of Duty 2
exhausts the 240-message budget during its menu fade-in, around frame 6.
It then saves `profiles.dat`, tears down GL and audio, and calls
`ExitProcess(0x42)` — a shutdown indistinguishable in the trace from the
user choosing Quit. Under `--message-budget 0` the same build keeps
rendering indefinitely, with non-black content growing monotonically as
the menu fades in.

So: when a game stops producing frames mid-load but exits *cleanly*, re-run
with `--message-budget 0` before touching anything else. If that fixes it,
the emulator was working and the CLI's cap was the whole story. The
default stays at 240 deliberately — it keeps headless and CI runs bounded.

**A guest that ignores `WM_QUIT` is halted rather than answered forever.**
`quit_or_halt` (`coredll.rs:7211`) hands out the quit and, once the budget
has been spent for `QUIT_POLL_GRACE` = 64 further polls, ends the run.
X-Forge (Ball Busters, Sticky Balls) only acts on a `WM_QUIT` that came
from `GetMessage`; the one its `PeekMessage` pump receives is dispatched
like any other message and dropped. Repeating it meant the game never
reached its render branch again and spun in the pump until `max_slices`
ran out — Ball Busters froze on the publisher logo at frame ~230 and spent
the rest of the run at a fortieth of its frame rate. A pump that *does*
honour the quit breaks out immediately and only comes back through here
while tearing down, so 64 is slack, not a second budget.

**Held keys repeat here, not in the frontends.** A frontend reports an
edge — key down, later key up — but a guest that samples input by polling
for `WM_KEYDOWN` sees one message and treats the key as tapped, which is
why menus navigated fine and nothing was playable. `key_repeat_if_due`
(`coredll.rs:7455`) fabricates a `WM_KEYDOWN` with lParam bit 30 (the
"previous state was down" auto-repeat flag) for each key in
`KernelState::held_keys`, after `KEY_REPEAT_DELAY_MS` = 400 and then every
`KEY_REPEAT_INTERVAL_MS` = 33, round-robin across the held set so two
held direction keys both keep arriving. The delay is Windows' own default
keyboard delay (`SPI_GETKEYBOARDDELAY` setting 1) and has to be that
long: JumpyBall steps its menu one row per `WM_KEYDOWN`, does not filter
the auto-repeat flag, and never polls `GetAsyncKeyState` — a trace of a
whole session shows `GXGetDefaultKeys` and nothing else — so every repeat
is another row. A deliberate tap on a keyboard or an on-screen D-pad
lasts 100-250 ms, which the earlier 120 ms delay turned into the two or
three rows of overshoot users reported. Removing the repeat instead is
not an option: with it gone, holding a direction did nothing at all in
that game. It is ordered after real and
posted messages and before the synthetic pump, and is suppressed entirely
while the guest has any built-in control (`kernel.controls`), whose own
`WM_KEYDOWN` handling would double up. `GetKeyState` /
`GetAsyncKeyState` read `pressed_keys` and are unaffected — this exists
for the message-polling half of the input path.

## 9. Run loop

`pocket-core` drives fixed slices of guest execution. Each slice: run the
CPU until the slice budget is spent or a hook fires, refresh the User
KData tick fields, then `sync_guest_framebuffer`. `--max-slices` bounds
the run (checked at `crates/pocket-kernel/src/lib.rs:2060`); `--max-frames`
bounds the frames captured. A `Halt` outcome ends the loop immediately.

Frame-indexed `--tap` / `--key` inputs stay queued until their target
rendered frame. If the guest idles, the hook may release a press only when
it is one frame ahead; a matching key-up / pointer-up may follow early only
after its press has been queued, so a future release cannot cancel an
undelivered press. Far-future presses stay queued, so a later-menu action
cannot land on Cops & Robbers' startup sound prompt.

`message_box_w` (`coredll.rs:8928`) is modal by re-entering its own thunk
via `JumpTo(ctx.thunk.thunk_va)`, capped at `MESSAGE_BOX_MAX_SPINS`
= 100 000 (`:8912`). Hundreds of identical `MessageBoxW ... status:"trampoline"`
records in a trace are that mechanism working, not a spin — they are what
lets the host present frames while the box is up.

## 10. Threads and the cooperative scheduler

Within each process, one guest thread runs at a time. There is no host
thread per guest thread; the scheduler is cooperative and the only scheduling points are
`Sleep`, `WaitForSingleObject`, `WaitForMultipleObjects`, `GetMessageW`
and `PeekMessageW`. A guest that spins without calling one of those
starves every other thread by construction.**A blocking primary-thread `GetMessageW` is itself a scheduling
point.** Rayman Ultimate parks its engine on a worker thread and runs
only the classic pump on the primary; the pump's synthetic `WM_PAINT`
traffic made `GetMessageW` succeed forever, and the parked worker was
never resumed (`frame_counter` stayed `0`).
`resume_worker_reenter` (`coredll.rs`) now checks for a parked worker
before the pump's synthetic answer, resuming it once and re-entering
the pump afterwards; `GuestThread::parked_in_pump` preserves the
primary thread's `GetMessageW` call site across that hand-off.

**`CreateThread` parks the new thread and returns the handle to its
creator** (invariant 9). It does not enter the thread, `CREATE_SUSPENDED`
or not — that flag only decides whether `started` is set, i.e. whether
the scheduler may pick the thread up yet. Entering immediately is the
obvious-looking implementation and it breaks Toy Golf: its audio thread
dereferences the mixer at `[r5,#0x38]` that its creator has not stored
yet, faulting on a NULL read at `0x00000038`.

Two park helpers, both in `coredll.rs`:

| Helper | Use |
| --- | --- |
| `park_worker_at(.., return_r0)` | The call is finished. Resume past it, optionally with a return value in `r0`. |
| `park_worker_and_reevaluate` | The call is *blocked*. Re-park at `ctx.thunk.thunk_va` so it re-runs later with its arguments untouched. |

The distinction is not cosmetic. An `INFINITE` `WaitForSingleObject` that
resumes through `park_worker_at` finds `r0` overwritten with a return
value and waits on that instead of its handle. When the *main* thread
issues an infinite wait, nothing can signal the object from there, so the
permissive `WAIT_OBJECT_0` stays — honouring it would deadlock.

`retire_wave_buffer` must deliver **every** `WaveCallbackKind`, including
`Event`, which signals the event rather than calling anything. That event
is typically a mixer thread's only back-pressure: leave it clear and the
wait falls through, the thread refills as fast as the CPU allows, never
yields, and the renderer never runs again. Toy Golf hung this way with
1,036,104 `waveOutWrite` calls and exactly one thread switch.

Worker threads never see the window queue's synthetic traffic
(invariant 7) — a worker running its own pump would dispatch forever.

## 11. Removable storage and stream devices

A game that shipped on a card checks the card is still there, and a
Windows CE program does that through the filesystem rather than through
any storage API. Three pieces make that work, all reached from
`CreateFileW`:

**`Vol:` is a handle on a volume, not a file.** CE exposes every mounted
volume as a `Vol:` pseudo-file inside it, so `CreateFileW("\\SD Card\\Vol:")`
returns something `DeviceIoControl` accepts for storage queries
(`crates/pocket-kernel/src/vfs.rs:32`). `Vfs` keeps those handles in a
table of their own, apart from open files — nothing reads or writes them,
and each carries the mount it named so a query can answer about *that*
volume.

**A volume has a serial, and for a Gizmondo card it is the card's own.**
`OpenVolume::serial()` never returns zero, because a guest that asks for
a serial and gets zero concludes the slot is empty. A Gizmondo card
states its serial in its own contents: beside the game directory sits a
four-byte marker file with the same name as that directory
(`\SD Card\GZGA200045\GZGA200045`), holding the serial of the card the
title was published on. Reporting that value is what makes the card in
the slot *be* the one the content was written for, which is what the
game is really asking. Any other volume gets an FNV-1a hash of its host
path, forced non-zero: arbitrary, but stable across runs.

**`IOCTL_DISK_GET_STORAGEID` (0x0007_1c24) is a two-call protocol.** It
fills a `STORAGE_IDENTIFICATION`: four DWORDs — size, flags, then *byte
offsets from the start of the header* to a manufacturer and a serial
string — followed by the strings. Two details are load-bearing
(`coredll.rs:1945`):

- The strings do not fit in the bare header, so the first call fails with
  the required size in `dwSize`; the caller reallocates and asks again.
  Truncating instead would hand back a shortened decimal serial, which
  parses as a different and wrong number.
- `dwFlags` bit 1 means "serial number invalid". Leave it clear.

Ball Busters reads the serial as
`strtoul((char *)id + id->dwSerialNumOffset, NULL, 10)`, so answering
with a zeroed buffer gave offset 0 and serial 0 — an empty slot, and the
game's "SD card removed" screen instead of its menu.

**`MAS1:` is the Gizmondo's MP3 decoder.** CE exposes the Micronas MAS
chip behind the console's audio as a stream device; a title plays music
by opening `MAS1:`, configuring it with a `DeviceIoControl`, and writing
MP3 frames to it. Nothing here decodes MP3, but the device still has to
open and accept both, because a missing one is not a case these games
handle: Ball Busters builds its music player by opening the device first
and the file second, and on failure leaves the player zeroed — then calls
it anyway on the next loading tick and dereferences a NULL stream.
Swallowing the frames is what gets the game past its loading screen.

Both pseudo-devices resolve *before* path resolution, since neither is a
file and `resolve` would otherwise fail them.

## 12. Audio — two transports

`AudioEngine` (`crates/pocket-kernel/src/audio.rs`) is fed by two
independent paths that mix together. Which one a game uses decides where
a silence bug lives.

**`waveOut` (`coredll`).** The guest decodes audio itself and hands over
finished PCM. `waveOutWrite` → `push_wave_samples`. This is a *stream*: the
guest owns timing and back-pressure, so `CALLBACK_EVENT` must really
signal or the mixer thread spins (§10).

Host callback blocks are part of the refill latency. Stuntcar Extreme submits
8192-byte stereo 16-bit buffers at 22050 Hz with CALLBACK_EVENT: each holds
2048 frames / 92.9 ms. A simulated 100 ms host callback reproduces the uploaded
PCM's 7.1 ms holes every 100 ms. Several returned buffers previously collapsed
into one signal on its auto-reset callback event. Keep subsequent driver
notifications in WaveOutState.event_done while that event is signalled, then
deliver them separately during retirement service once the event is clear.
WHDR_DONE still follows actual sample consumption. Ordinary SetEvent remains
binary; manual-reset callback events retain their existing coalescing behavior.
Discard undelivered entries when their wave device closes or event disappears.
The native Stuntcar simulation no longer has those periodic holes even with
100 ms host blocks; retain the batched completion regression test.

Use the driver's default CPAL buffer size. The earlier 10 ms request did not
resolve the Windows report and is removed. Never repeat samples, change
playback speed, retire headers early or add a game-name exception.
The temporary host callback probe was removed after Windows validation of
the event-completion correction. No diagnostic environment flag remains.

*Buffer completion is not a message-pump event.* On WinCE the driver's
own thread reports a drained buffer, so a game may wait for one without
pumping messages — and games do. Zuma's sound engine stops a stream by
setting a request byte and spinning on `Sleep(0)` until its
`waveOutProc` sets an acknowledgement byte (`ZumaPPC.exe` 1.50: the spin
is at `0x000e80e8`, the acknowledgement at `0x000e896c`), which means the
loop can only end if a buffer-done callback is delivered *from inside
`Sleep`*. `service_wave_out` therefore runs from three places —
`waveOutWrite`, the message pump, and `Sleep` — the three points where a
guest hands the CPU back. Before `Sleep` was one of them the game wedged
at 100% CPU on shutdown, after nine to eighty frames, which reads as
catastrophic performance rather than as a hang.

`CALLBACK_FUNCTION` delivery re-enters the guest, so the detour has to
survive the callback calling an API that also delivers callbacks —
Zuma's `waveOutProc` calls `Sleep(0)` before acknowledging. As in
`create_window_ex_w`, SP discriminates: a nested call runs on a
deeper stack, and only SP back at the saved value less
`WAVE_PROC_STACK_BYTES` (the reserved fifth argument) means
`waveOutProc` has really returned. Restoring on the nested call would
move SP out from under the running callback.

**HSS (`hss.dll`).** Hekkus Sound System is a freeware C++ mixer bundled
with a great many Pocket PC games. The guest hands over a *filename* and
expects the library to decode it, so `crates/pocket-winceapi/src/hss.rs`
owns a decoder for both formats HSS accepts: PCM `.wav` for effects and
Protracker modules for music. Games commonly rename modules to `.tkm`, so
`decode_clip` tries both decoders against the *content* — the extension
is not load-bearing on a device and it isn't here either.

HSS is C++ methods, so every handler takes `this` in `r0`. The guest-side
object is opaque: whatever the real `hss.dll` would have written there, we
never write. All state is host-side in `HssState`, keyed by that pointer.

Two mangled-name traps, both of which cost a whole debugging session on
JumpyBall:

* `load` is overloaded. `?load@hssSound@@QAAHPBG@Z` takes
  `const wchar_t*`; `?load@hssSound@@QAAHPAX_N@Z` takes `void*, bool`.
  Registering one is not registering the other.
* The volumes are setter/getter *pairs* that differ only in the mangled
  signature — `?volumeSounds@hssSpeaker@@QAAXI@Z` sets,
  `?volumeSounds@hssSpeaker@@QAAIXZ` gets. JumpyBall calls both halves
  and reads back what it wrote.

A stub that returns success for a name the game does import is worse than
no stub: the game proceeds as though it has audio and there is no warning
in the trace to find.

**The Protracker renderer** is `crates/pocket-kernel/src/tracker.rs` —
`Module::parse` then `Module::render(rate, max_seconds)`, no
guest-memory awareness, so it is testable on its own. Two things it must
get right that are easy to get wrong:

* The sample number is split across two nibbles — high in cell byte 0,
  low in the *high* nibble of byte 2. Getting this wrong selects an empty
  slot and renders silence.
* Source samples carry a large DC offset (means of +65 to +99 out of
  ±128 in JumpyBall's tracks). An Amiga's AC-coupled output discarded it;
  a modern DAC will not. The DC blocker on the mix bus is why music does
  not arrive as a thump — don't remove it.

Modules render once at load, not on the audio callback: measured at
0.04–0.05 s per 30 s of audio in release. `MODULE_SECONDS` is
deliberately generous so the renderer stops on the *order table* rather
than the clock — a song cut short puts the loop seam mid-phrase, which is
far more audible than the memory costs. Decodes are cached by path
behind an `Arc`, because a game re-loads the same track into a fresh
object on every level change.

**Voice groups.** `play_voice_with` takes `VoiceParams { looped, group,
volume }`. The groups exist so `stopMusics` can silence music without
cutting the sound effects that are still playing — JumpyBall calls it on
every level change. Voice mixing is `#[cfg(feature = "audio-cpal")]`;
without that feature `play_voice` degrades to `push_samples`.

**`--dump-audio-to` records at submission time**, inside
`add_voice`/`push_samples` — not as a real-time mixdown. A capture WAV
therefore shows what the guest *submitted*, in submission order, and its
header takes the format of the first clip seen. A game that starts four
tracks over an 8-frame run yields a capture of four full-length tracks
laid end to end — 499 seconds of WAV. This is the right tool for "did
any PCM reach the engine", and the wrong tool for "what would a user
hear".

**A capture proves submission, not audibility.** The two questions come
apart, and "the WAV looks fine but there is no sound" is the normal way
an audio bug presents. The host side is `run_audio_worker`, and *every*
one of its failure paths — no default device, `default_output_config`
failing, an unsupported sample format, `build_output_stream` failing,
`stream.play()` failing, the thread failing to spawn, or the
`audio-cpal` feature being off — leaves the run silent while the guest
carries on submitting samples and noticing nothing. They all log at
`warn` for that reason: a silent run is a user-visible failure, not a
detail. The one line that confirms real output is

```
AudioEngine: opened "<device>" at 44100 Hz / 2 ch (F32)
```

If that line is absent, the problem is the host device, not the decoder.

**The desktop GUI has no console on Windows.** `main.rs` is built with
`windows_subsystem = "windows"` so launching it does not flash up a
terminal, which also means stderr goes nowhere and log output reaches
nobody. It therefore tees `log` to `<library root>/pockethle-gui.log`,
truncated per launch. When diagnosing "no sound in the GUI but the CLI
is fine", read that file first — and prefer reproducing through the CLI,
which has a console and takes the same code path.

## 13. Frontend input and launcher settings

The guest side of input is §5's path: frontend → `KernelState::pending_input`
→ `take_pending_input` → `controls_take_input` → `input_to_message`. What
each frontend puts *into* that path is a user preference, and both
launchers persist it in `<root>/config.json` (`LauncherConfig`) or
`<root>/games/<id>/game.json` (`GameSettings`).

**Keybindings are global, in `config.json`.** `pocket_library::keybindings`
maps a host key name to a `GuestButton` and on to the GAPI VK the guest
sees (arrows 0x25..0x28, `RETURN` 0x0D, vkA..vkStart 0xD1..0xD4). The
stored names are whatever **`egui::Key::name()`** produces, because that is
what the desktop's rebinding UI writes and what the runtime looks up:
`"Up"`, `"Down"`, `"Left"`, `"Right"`, `"1"`, `"2"` — *not* `"ArrowUp"` or
`"Num1"`, which `Key::from_name` accepts but `name()` never emits. A
default that used the other spelling left the whole D-pad dead while
looking perfectly reasonable in the file, so lookups go through
`keys_match` / `canonical_key_name`, which fold case and both arrow
spellings. `set_keys` steals a key from any other button, so one host key
can only ever drive one guest button.

**Mouse hold: do not ask egui whether the pointer is down on a widget.**
`Response::is_pointer_button_down_on` is click-oriented — eframe/egui
0.27's `MAX_CLICK_DURATION` (0.8 s) clears `potential_click_id`, so a
button the user is still physically holding reports itself released after
about a second. `pointer_held_in`
(`frontends/pocket-desktop/src/app.rs:1168`) instead reads the raw
`primary_down()` plus `press_origin()` inside the widget rect, which holds
for as long as the user does.

**The on-screen pad and the physical keyboard hold keys in separate
sets.** `HeldButtons` (`frontends/pocket-desktop/src/app.rs`) keeps one
`HashSet<u16>` per `InputSource`, tells the guest a button went down only
when *no* source held it yet, and reports the release only once *every*
source has let go. While both shared one set, `vbutton` — which runs on
every frame and releases its button as soon as the pointer is not on its
rect — cancelled whatever the keyboard was holding with a `WM_KEYUP` in
the next frame. The visible symptom was JumpyBall: steering with a held
arrow key worked until the user touched the on-screen D-pad once, after
which the keyboard did nothing at all for the rest of the run, because
the guest saw every `WM_KEYDOWN` immediately undone. `release_all_keys`
drains both sets on window close so no direction is left stuck down.
Android already worked this way — `acquireGuestKey` / `releaseGuestKey`
refcount each VK across its on-screen, physical-keyboard and gamepad-axis
sets — so this only ever affected the desktop.

**Android inflates `activity_game.xml` exactly once.** `GameActivity` is
declared with `configChanges="orientation|keyboardHidden|screenSize"` —
deliberately, so a rotation does not tear down the running guest and its
GL surface — which also means a `res/layout-land/` variant would never be
used. The layout is therefore one FrameLayout with the virtual gamepad
*overlaying* the surface, and `updateControlsLayout()` pads the game area
by the control-strip height in portrait only; in landscape the buttons
float over the picture. As a vertical LinearLayout instead, the 156dp
D-pad and the status panel claimed a fixed share of a ~360dp-high
landscape window, squeezing the surface to a sliver and pushing the
buttons over the toolbar. `onConfigurationChanged` re-submits the last
frame so it is re-letterboxed for the new window shape, and does it
without going through `paintFrame` so a rotation is not counted as a
rendered frame in the FPS overlay.

Two more `LauncherConfig` fields exist for what that overlay costs the
player: `show_backend_log` hides the in-game status panel ("Backend:
Unicorn (ARM)…"), and `controls_opacity` (0.1..=1.0, clamped on both
sides) becomes the gamepad's alpha. Android edits the latter as an integer
percentage and stores the fraction.

**Gamepads reuse the same guest keys.** `onGenericMotionEvent` folds the
D-pad hat and the left stick (`AXIS_HAT_X/Y`, `AXIS_X/Y`, past
`AXIS_DEADZONE` = 0.5) into the arrow VKs through the same refcounted
acquire/release as the on-screen buttons, tracked in `heldAxisKeys` so a
stick returning to centre releases exactly what it pressed.
`releaseHeldInput` clears it.

**`config.json` is also the FFI contract.** Kotlin's `LauncherConfig` /
`GameSettings` in `GameEntry.kt` mirror the Rust structs field for field,
and a mirror that drops a field silently resets it on the next Android
write — which is why `keybindings` is carried through verbatim as an
opaque JSON string even though Android has no rebinding UI: an Android
settings change must not wipe the host-key map a user set up on the PC.
Add a field to one side and add it to the other in the same change.

## 14. Working on this repo

```bash
cargo build --release -p pocket-cli --features unicorn   # → target/release/pockethle
cargo test --workspace
cargo clippy --workspace --all-targets

# CeGCC smoke test — expect zero "unimplemented call" warnings
./target/release/pockethle -v run hello.exe --cpu unicorn --max-slices 5000 \
  --key 3:enter --dump-frames-to /tmp/hello-frames --max-frames 6

# A GLES game, with the message cap lifted
./target/release/pockethle -v run /tmp/cod2-install/cod2_gles.exe \
  --rom-dir /tmp/cod2-install \
  --module-path '\Program Files\COD2\cod2_gles.exe' \
  --key 1:enter --key 2:enter --key 3:enter \
  --message-budget 0 \
  --dump-frames-to /tmp/cod2-unlimited --max-frames 40 \
  --max-slices 40000000
```

Diagnose in this order — cheapest first, and each step rules out the ones
below it:

1. Did it exit cleanly? Re-run with `--message-budget 0` (§8).
2. Any "unimplemented call" warnings? Those are missing handlers, or a
   missing ordinal-table entry if the names show as `ord NNN` (§5).
3. `frame_counter=0` with no errors? A presentation problem, not a CPU
   one (§6). If `GetClassInfoW` succeeds for an unregistered app class and
   `CreateWindowExW` reports `wndproc=0`, fix that false class hit before
   changing the message pump. Mini-Dogfight 1.5 also stores fullscreen GUI
   dimensions as `100%`; `_wtol` must parse the numeric prefix or its
   `StretchBlt` destination becomes 0x0 and the menu stays black.
4. `frame_counter` huge but every captured frame identical? Something is
   bumping the counter without drawing. Raise `--dump-frame-stride` to
   confirm, then find the handler (§6).
5. Hangs with one thread doing all the work? A missing wake-up, not a
   slow CPU. Count "scheduling worker" lines in the trace (§10).
6. Does the game map both GAPI and GL? Check the `eglSwapBuffers` push.
7. Only then read guest disassembly.

Conventions:

* Match the surrounding comment density. Comments here explain *why a
  game needs this*, naming the title — that is what makes them worth
  keeping.
* Fix a shared wrong assumption at the boundary where the data enters, not
  in each consumer.
* Write the regression test so that reverting the fix fails it. Name it
  after the behaviour, not the function.
* Frames that prove a fix go in `proof/<game>/` with a short README. Note
  that COD2's framebuffer needs a 90° rotation to be read right-side up.
* No credentials, keys or tokens in anything written, logged or committed.
* Do not commit unless asked, and never push directly to the target
  branch — changes go through a Pull Request.

**Legacy CAB install roots are component-wise common ancestors.** Some
MSCE cabinets put every file in subdirectories (`bin/`, `resources/GUI/`,
`resources/scenes/`) and contain no file directly in the install root.
Inferring the root only from directories that contain a file yields
`None`; materialization then falls back to basenames and resource
`FindFirstFileW` searches fail. Keep this behavior pinned by
`nested_install_subdirectories_share_their_true_root` in `pocket-cab`.


## 15. Image and thread memory lifetimes

The ARM EXE loader reserves CPU image pages, completes aliases and IAT
fixups, then discards the section payloads in Process.image. That object
retains section metadata; resources move to KernelState.resources. The CPU
keeps backing bytes only for pages not yet committed; see §19. MIPS and
CPU wrappers without deferred-page support retain the eager fallback.

Runtime modules retain their exact section and thunk mappings in
`LoadedModule.resident_regions`. `FreeLibrary` decrements the load count;
the last reference calls `DllMain(base, DLL_PROCESS_DETACH, NULL)` before
unmapping and refunding resident RAM. A rejected PROCESS_ATTACH and partial
mapping failures also release their mappings. Module slots are reused, so
CPU hooks, execution/permission history, translated code and dispatcher
cache identities must not survive as stale state for another image.
Resource satellites carry an owner reference and are released with their
parent unless explicitly retained elsewhere. HLE system modules are pinned
for the process lifetime. Native dependencies are recursively bound and
retained through graph reachability, as described in §18.

Thread stack lifetime is separate from handle lifetime. Natural return,
`ExitThread` and `TerminateThread` release the stack; handles and exit codes
remain available to waits, duplicates and `GetExitCodeThread`. Private
stacks return to the already mapped heap arena and refund their resident
page charge. External stacks are unmapped and their address slots reused;
the main stack is unmapped when its thread exits even if workers continue.
Creation failures roll back their reservation. Keep the compatibility guard
pages and preserve the rule that CreateThread returns to its creator.

## 16. Guest RAM diagnostic and failure contracts

`tools/ramtest` contains a freestanding ARM diagnostic, generated PE fixtures
and an importable Gizmondo ZIP. Its title marker selects the existing profile;
never add a game-name branch for the diagnostic. `pocket-core/tests/ram_guest.rs`
runs the actual shipped image with Unicorn and verifies its report and teardown.
Rebuild fixtures with `build.py` whenever the guest C or imports change.

LocalFree failure returns its input handle; HeapFree failure returns zero.
Size queries and realloc failures must set the calling thread's error and
preserve the original allocation. A failed realloc copy releases the new
reservation before propagating a CPU error. Reallocated tails follow the
existing zero-filled heap policy. This still uses one backing process heap;
independent HeapCreate arenas and movable local handles are not implemented.
LoadLibrary errors distinguish absent modules, invalid images, exhausted RAM
and rejected attach. Missing exports and invalid module handles set errors.

Off-screen DirectDraw allocation failure must return E_OUTOFMEMORY and a null
output, never substitute the panel mapping. Check dimension multiplication
before allocation. Failed object allocation rolls back partial heap blocks and
the new pixel buffer. Test fixtures must give the allocator the same arena
capacity as the mapped CPU heap. Sleep already switches directly to a ready
worker; scheduler tests must preserve that behavior.

Host-run child process completion is deferred until `run_process` has dropped
the child emulator, including its CPU image and heap RAM charges. Use
`HandleTable::defer_process_exit` before guest execution and
`complete_process_exit` after teardown. Guest exit codes are recorded earlier
but process waiters and remote process exit queries must not observe them yet.
Thread exit remains independently observable. Preserve the delayed-cleanup
regression: a parent cannot finish its process wait while four child pages
are still charged. VFSTEST keeps its strict NAND/RAM equality check.

Cross-process handle tests cover closure of the original and intermediate
aliases, parent teardown, retained shared state, final reference cleanup and
rejected use of closed aliases. They do not imply general concurrent process
execution or remote thread control, which remain outside the current model.

The guest diagnostic writes its report to `\Flash Disk\RAMTEST.TXT`, not
beside its executable on the read-only SD card. The Unicorn integration test
must use a read-only SD mount and writable Flash Disk, as the desktop runner does.


## 17. Native DLL thread and normal process notifications

`dll_lifecycle` retains registers, SP, FPSCR and the calling thread's error
while native DllMain runs through the dedicated return hook at 0xF0000008.
Do not use the descending worker-exit trampoline slots for callback returns.
Worker attach is delivered on its first execution before its entry point,
not from CreateThread. No retroactive attach is sent when LoadLibrary runs
on an existing thread. Attach visits executable DLLs in load order; clean
thread detach visits them in reverse order, on the exiting thread, before
its stack is reclaimed. Passive resource satellites receive no callbacks.
Callback return values for thread notifications and process detach are ignored.

ExitProcess and top-level EXE return deliver PROCESS_DETACH with a non-null
reserved argument. Explicit FreeLibrary keeps reserved NULL. All native DLLs
stay mapped until the entire process-detach sequence returns, then mappings,
thunks and RAM charges are released, regardless of remaining load references.
The calling stack survives until then too. Main ExitThread while workers remain
receives THREAD_DETACH; the last worker triggers process detach on its own
stack rather than attempting callbacks on the already released main stack.
Process exit does not send per-thread detach to the other live threads.

Forced TerminateThread suppresses its thread detach; TerminateProcess and the
CE implicit termination trap keep their forced teardown path without guest DLL
callbacks. Host Stop and slice-budget exhaustion are not normal process exit.
A callback sequence excludes scheduling other workers, and API-boundary
preemption does not interrupt it. A yielded worker callback retains its frame
until its owner resumes. Synchronizing across threads, recursively loading DLLs
or calling FreeLibrary from DllMain remains outside the documented safe usage
of the CE entry point; do not advertise those as supported loader behavior.

The shipped diagnostic checks callback reason/order, thread identity,
reserved arguments, FALSE return handling, saved error state, natural worker
return, ExitThread, termination of a suspended worker, and process cleanup.
Unicorn fixtures additionally cover explicit ExitProcess and the main-exits-first
case. RAMTEST.TXT covers 128 checks; DLLTEST.TXT receives its final PASS during
process detach, after the user dismisses the final message box.


## 18. Runtime native dependency graphs

LoadLibrary maps the complete native dependency graph before entering any
PROCESS_ATTACH callback. HLE imports retain their existing hooks; native
imports bind directly to relocated guest exports, by name or ordinal. Resolve
new dependencies beside the importing DLL first (case-insensitive filename),
then through the existing module search. Publish exports before traversing
imports so cycles bind without allocating duplicate copies. Native forwarder
exports remain rejected by prepare_runtime_module; static EXE import loading
is not added by this runtime loader. Image page commitment follows §19.

LoadedModule.dependencies stores unique native import edges. Its refcount
counts explicit LoadLibrary references, not imported edges. A dependency first
loaded by an import starts with zero explicit references. A cached explicit
LoadLibrary adds one, even when the module originally came from an import.
FreeLibrary collects modules unreachable from remaining explicit roots;
cycles are therefore reclaimed together without leaking reference counts.
Resource satellites retain their existing owner-reference contract.

New modules are ordered dependency-first (DFS postorder, with each cycle
visited once). PROCESS_ATTACH follows that order; normal detach reverses it.
Keep every collected mapping and initialized dependency resident until all
callbacks finish, then unmap and refund the whole unreachable group.
The attached flag excludes partially initialized modules from thread/process
notifications. On failed attach, only completed new attaches receive detach,
in reverse order, before releasing the entire new graph. Already resident
initialized dependencies survive a failed importer load.

Missing dependency returns error 126, missing export 127, rejected attach
1114. Mapping/binding failures roll back all images created by that load,
exports, hooks, resident RAM and the module-slot allocator; existing modules
are preserved. Do not turn unresolved native imports into successful HLE
stubs. DllMain recursive LoadLibrary/FreeLibrary usage remains outside the
supported CE entry-point contract described in §17.

The shipped ARM diagnostic checks direct imports, ordinal imports, shared
and explicitly retained dependencies, missing dependencies/exports, rejected
attach and circular imports. DEPTEST.TXT records and verifies the complete
attach/detach sequence and ends with DEPTEST_RESULT PASS. All three executable
exit variants verify this in addition to RAMTEST.TXT and DLLTEST.TXT.


## 19. Demand commitment of ARM image pages

Cpu::map_image_region reserves page-aligned image ranges and defers backend
storage. A cold page contains only its initialized backing bytes (or no bytes
for zero-fill) and its original protection. First guest fetch/read/write or
an HLE host read/write commits that page, restores initialized data and zeros
the rest, then discards the backing copy. Subsequent writes remain in the
resident backend page; never restore old backing over dirty data. The PE is
still parsed and relocated at load time; this is deferred physical commitment,
not a new disk-streaming reader or an eviction mechanism.

Unicorn's virtual TLB fill materializes cold pages before supplying a physical
translation. The architectural-TLB fallback uses its existing invalid-memory
hook. Do not install per-instruction or valid read/write hooks: those disable
normal fast paths. Host helpers must page in too; Unicorn's host memory calls
can reenter TLB hooks, so never hold an ImagePages/GuestMap RefCell borrow
across mem_write or mem_unmap. StubCpu follows the same host-access semantics.
The legacy low slot alias remains eager and MIPS keeps its existing loader.

ImageMemory is a shared per-process physical-page counter attached to the
same MemoryDivision as heaps/stacks. Heap::program_pages includes it. Reserve
ranges do not consume program RAM. Each successful page-in acquires one page;
allocation or initialization failure releases the charge and leaves the page
retryable. Profile switches include resident image pages and roll back on
insufficient capacity; rebinding the same device must not double-charge.

Runtime DLL mapping records complete virtual ranges for cleanup, but only
thunks and image pages actually accessed are charged. Bind imports and
preflight required entry pages before starting attach callbacks. Image
page-in OOM during that transaction returns LoadLibrary NULL/error 8 and
releases all new reservations/exports/charges/slots. Guest-access page-in
OOM stops with an explicit CPU fault rather than silently overcommitting RAM.
Do not eagerly materialize a whole image merely to query exports or unload it.

Unmap validates the entire image range, removes resident backend pages and
cold backing, refunds only resident charges, and clears hooks/TLB/code caches
before address reuse. Pages cannot reappear after unload; a reload restores
original initializers and zero-fill instead of the previous dirty data.

The RAMTEST v5 image warms its own code/data before baseline measurements so
unrelated first-use pages cannot change an allocation/refund comparison.
pageprobe.dll separately tests cold code, initialized data, zero-fill, repeated
accesses, untouched tails, dirty-data persistence and unload/reload. The three
Unicorn process-exit variants still run all DLL lifecycle/dependency tests.


## 20. TLS context and API errors

TLS slot allocation is per process (64-bit bitmap); slot values are per thread.
The KData lpvTls pointer still addresses the active guest array at 0xFFFFCB00.
KernelState.tls_owner identifies that array's owner. On a context switch, save
its contents and restore the target thread's snapshot (or zeros on first entry)
before executing any guest code or DllMain. Read the actual guest window rather
than reconstructing values from TlsSetValue calls: CE CRTs write it inline.
The central run-loop boundary covers all scheduler transitions, including a
worker-to-worker handoff. TLS handlers also synchronize for direct API dispatch.
The fast path for the same owner performs no CPU memory access.

TlsAlloc and TlsFree clear the selected slot in the active window and every
saved thread, avoiding stale values after reuse. Publish the bitmap change
only after the guest write succeeds. Exhaustion returns TLS_OUT_OF_INDEXES
and error 8. Invalid Get/Set/Free indices and Free on an unallocated slot fail
with error 87. CE Get/Set deliberately validate only the range 0..63, not the
allocation bitmap. Successful TlsGetValue clears the calling thread's error
(also when returning NULL); successful Alloc/Set/Free preserve it.

Keep TLS available through normal thread/process detach notifications. Drop
the saved array only after the thread is finished; never re-save a finished
outgoing owner. Process teardown clears snapshots and the slot bitmap after
normal detach. Values are opaque pointers: cleanup never frees pointees.
Host snapshots do not introduce a second guest mapping or physical RAM charge.
Each process owns its own KData window, bitmap and snapshots; switching back
to a preserved launcher retains its TLS state with the rest of its CPU/state.

WaitForMultipleObjects validates count and the handle-array pointer before
reading it. Parameter failures return WAIT_FAILED/error 87; null, invalid-value
and closed handles return WAIT_FAILED/error 6. Failure must not consume a
signalled event or semaphore. Preserve typed image-page OOM instead of treating
it as an invalid pointer. Existing compatibility for unmodeled wait-object
classes is unchanged; this audit covers TLS and the RAM/thread/handle error
contracts, not a claim that every emulated WinCE API is fully conformant.

The ARM v6 diagnostic runs 128 checks, including actual main/two-worker TLS
switches, inline access, zero initialization/reuse, LastError, exhaustion,
invalid waits and TLS availability/modification in native DllMain attach and
detach. All three process-exit variants verify reports and empty TLS snapshots
at teardown. No production instrumentation or game-name special case is added.


## 21. Independent processes and remote thread controls

Generic CreateProcessW launches a CPU/kernel on its own host thread. A shared
handle domain assigns unique process/thread IDs and provides a gate around
API dispatch and teardown. Do not hold this gate during guest CPU execution,
frame hooks or startup acknowledgement: recursive SDCreateProcess and remote
startup would deadlock. SDCreateProcess retains the preserved-parent foreground
handoff, independent of the generic CreateProcessW behavior.

Creation is transactional: resolve/validate the image, construct the child,
attach shared handles and physical RAM, initialize WinMain/GetCommandLineW,
acknowledge readiness, write PROCESS_INFORMATION, then release the startup gate.
An invalid output pointer must never execute the child; initialization/OOM
failure returns an API error and refunds resources. Dead callers cancel held
children. CE fInheritHandles must be FALSE; duplication remains explicit.
Supported creation flags are 0 and CREATE_SUSPENDED; other flags fail with
ERROR_NOT_SUPPORTED instead of pretending to implement debugging.

Remote SuspendThread/ResumeThread update authoritative counts in the domain
and queue requests for the owning CPU. Apply requests at slice boundaries and
before dispatching an already reached API hook. Save the exact continuation
before parking; never execute the stale hook after changing its owner/PC.
Remote termination does not execute guest detach callbacks. Preserve primary
thread exit status while workers keep the process alive; final worker/process
exit signals the appropriate object and releases stacks/TLS/abandoned mutexes.
A child may outlive its parent: drop the parent's emulator before joining
children, while shared handles/RAM remain alive through child references.

The desktop session shares stop/input but only its foreground process submits
frames. All process jobs must finish before completing the session. API state
is per process; thread-local multimedia stays on the owning host thread.
ARM v7 verifies all 132 RAM checks plus the separate process/orphan reports in
three exit variants; the real desktop runner executes the same process probes.
No temporary instrumentation or game-specific exception is introduced.

## 22. VFS contracts and per-device storage

`vfs_contract.rs` owns guest creation/share contracts, error results and volume
quotas. Keep `VfsShared` in the process launch context: open-description leases
and attributes must be shared by parent, child and duplicated handles. A lease
ends only when its last alias/export is dropped. Check share compatibility in
both directions; a denied open must not truncate or create a file.

Guest creation must not create missing parent directories. CRT append seeks to
EOF on every write, including after an explicit seek. Directory removal is real
and rejects nonempty directories. Rename must not overwrite a destination.
Respect the most specific mount and canonical path boundary, including read-only
overlays and symlinks. Find records carry the same attributes as attribute queries.
Extra attributes are shared session state, not persistent filesystem metadata.

Flash Disk has a 32 MiB logical file-data quota, independent of physical RAM and
the object store. Serialize size-growth validation with writes/truncation; count
sparse logical lengths and refund through current file sizes after truncate/delete.
SD capacity is synthetic (minimum 64 MiB, rounded upward from content size).
Do not report the host disk's capacity as the emulated device's capacity.

Guest output buffers require `Cpu::check_guest_access` before consuming input or
mutating files. Loader `read_mem`/`write_mem` intentionally bypass guest protection
and are not permission probes. Stub and Unicorn check resident and cold image
permissions without committing cold pages merely to validate a pointer.

VFSTEST v1 exercises 103 ARM checks, child sharing/duplication, protected outputs,
quota exhaustion/refund and cleanup. Native integration runs it twice and verifies
RAM restoration; the actual desktop runner also executes it. Keep the private
probe-directory refusal and never clean unknown game files. RAMTEST v7's 132
checks remain regression coverage. No temporary production instrumentation.

## 23. DirectDraw DXPAK and compact Mobile interfaces

Do not identify a surface ABI solely by its CE Surface5 IID: both SDK families
use it. QueryInterface from a retained Surface4 starts an ambiguous view with
compact slots. Its first slot-19 Lock fixes the compact ABI; a writable 124-byte
DDSURFACEDESC2 passed to slot 25 fixes the retained Surface5 ABI (Lock 25,
Unlock 32, AlphaBlt 45). Views own their vtables and share pixel storage; selecting
one view must not rewrite another. Ordinary compact surfaces keep compact tables.

For 124-byte descriptors, DDSCAPS_PRIMARYSURFACE is 0x200 and OFFSCREENPLAIN
is 0x40. The compact 108-byte header uses 0x40 for its primary flag. Keep both
creation and returned descriptors consistent; otherwise a primary surface is
allocated off-screen and never published. A real HWND plus cooperative-level
flags at DirectDraw slot 20 is the older SetCooperativeLevel call, not a pair
of GetAvailableVidMem output pointers.

FIFA's observed failure was slot 25 dispatched as SetPalette, followed by 240
invalid memcpy scanlines and a NULL slot-32 call. Validate its actual ARM startup
and the compact/retained COM regression tests. Do not add title-name branches,
swallow the invalid memcpy, or retain temporary tracing in production.

Classic Compendium additionally leaves the Lock output uninitialized. On an
ambiguous retained view, NULL RECT/event plus WRITEONLY flags and a writable
124-byte output identify this Lock even without dwSize. After selecting the
retained ABI, always return DDSURFACEDESC2; random previous output contents must
not choose compact offsets. Tests cover first and subsequent dirty outputs and
the untouched byte after the 124-byte structure.

Retained slot 7 is BltFast, whereas compact slot 7 is Flip. A known source
surface in r3 plus bounded x/y and BltFast flags selects the retained view.
Implement the source rectangle to destination-coordinate copy and publish the
primary surface; drawing only into off-screen memory is not a successful boot.
The ARM game must visibly reach language selection and advance through input.

## 24. Key release ordering and repeat cadence

Polling key state must consider the newest pending event for the queried key
only (including the existing C1/D1 aliases). A pending release overrides an
older pressed state; unrelated releases never press other keys, and unrelated
presses never hide the queried key. Preserve held keys and diagonal input.

Keyboard repeat deadlines advance from now after each held-key round, rather
than catching up missed intervals like animation timers. A late host slice must
not deliver a burst of menu moves. Keep the 400 ms initial delay and the 33 ms
hold cadence. Do not change WM_TIMER/WM_PAINT scheduling for this fix.
Regressions cover release-before-message delivery, unrelated input, aliases,
held-key round-robin and delayed repeat without catch-up. No temporary tracing.

## 25. DirectDraw display timing and complete primary frames

WaitForVerticalBlank must not be a success-only stub. Model a 60 Hz display
clock using host monotonic time; BEGIN waits for the next blank edge and END
waits for its end. Preserve deadlines and arguments across thunk retries.
A waiting main thread yields to ready workers; a waiting worker parks with a
scheduler deadline and resumes the same call. Do not block the API gate for
an entire refresh interval. Do not advance missed presentation deadlines in
catch-up bursts after slow slices or parent resumption.

Pace full-primary write Lock/Unlock presentations, even when pixels are
unchanged, and synchronized Flip. Explicit vertical waits pay for the next
presentation once; avoid imposing a second wait. Partial/read-only locks and
off-screen sprite composition are not display presentations. Preserve
DDFLIP_NOVSYNC. The 60 Hz value is the modeled display policy, not a measured
Gizmondo panel specification.

While a primary surface is locked, memcpy scanlines and run-loop readback
must not publish partial images or inflate FPS. Publish the completed image
at Unlock, keep frame_counter tied to actual pixel changes, and preserve the
unlocked GAPI/direct-write path. COM views share the lock through their pixel
storage. Keep input ordering and repeat fixes: their regressions reproduce
independent defects. No title-name hacks or temporary production tracing.

Validate edge timing, static-image presentation pacing, retry argument
preservation, worker wakeup and locked-scanline suppression, plus actual ARM
FIFA/Classic startup and the existing ARM RAM/VFS integrations. The Windows
GUI tap/hold behavior and audio require the user's emulator test.

## 26. Frontend input polling is independent of pixel readback

FrameHook also drains frontend input and stop requests. Never gate all hook
calls on frame_counter changes or PRESENT_POLL_BACKOFF. A static DirectDraw
menu then holds an already released key for up to 250 ms, triggering Classic
Compendium's own frame-based repeat. FIFA's GetAsyncKeyState edge detection
can collapse a release/repress that arrives in that same delayed input batch.

Poll the frontend at FRONTEND_POLL_INTERVAL (4 ms), or immediately when a
frame or child launch requires it. Keep expensive pixel readback on its own
4 ms / 250 ms leased cadence and leave GUI snapshot throttling intact. Static
images must not bump frame_counter just to obtain input. Preserve ordered
keydown/keyup events, the existing key-state semantics and 60 Hz DirectDraw
presentation timing. Do not introduce a 30 FPS title override or button debounce
to hide this scheduling defect.

The static-image regression queues two press/release sequences under an
active direct-presentation lease. It must complete before the 250 ms readback
interval; it fails with the old hook gating. Actual ARM Classic with one 100 ms
Down press moves Chess to Checkers at 60 Hz after this fix. FIFA's interactive
rapid-tap result still requires the user's Windows GUI verification. No private
timing diagnostics or experimental cadence settings belong in delivery.

## 27. Console-only fullscreen and native GPU reconstruction

F11 toggles borderless fullscreen, F10 captures a game screenshot, and Escape
leaves fullscreen during a game. These shortcuts are consumed by the launcher;
virtual Gizmondo piano buttons retain their guest key codes. Release held input
when switching fullscreen and restore the preceding window size on exit.

Fullscreen gameplay shows only the screen on a black background, without skin,
toolbar, status or FPS overlay. Gizmondo uses 320x240 at the largest fitting
integer factor in physical pixels, centered with black bars. Compute the factor
after applying pixels_per_point so Windows DPI never creates fractional scaling.
PocketPC retains its native dimensions and selected rotation.

The default GPU filter is original edge-adaptive Catmull-Rom reconstruction
with local color bounds to limit ringing; it is not xBRZ. Native-sized textures
are uploaded only when snapshots change, and the GPU shades the displayed
rectangle. Fullscreen also offers nearest, bilinear, bicubic and Lanczos modes.
Do not allocate a fullscreen-sized CPU buffer each frame. Keep the ordinary
egui texture as fallback if shader creation fails. Screenshots capture the painted game rectangle after filtering and rotation
(see section 29), excluding the fullscreen bars and interface.

Validate integer layout across monitor sizes and DPI, compile the actual
eframe/glow frontend, and compile/render the delivered shaders in an OpenGL
context. Real Windows transitions, display-driver performance and screenshots
need an interactive emulator check. Ship no validation harness or diagnostics.

## 28. Optional SMAA and xBRZ filters

Every filter label includes its visual behavior and cost. Preserve the original
reconstruction default and the user's choice across window/fullscreen switches.
SMAA uses the reference three-pass 1x algorithm, area/search lookup textures,
and native-resolution intermediates. Apply it before display reconstruction
(sharp variant) or bilinear display scaling (soft variant). This is spatial SMAA,
not temporal SMAA T2x; no motion vectors or history are available. Cache output
until a snapshot or preset changes. Clear edge/weight buffers each evaluation
and restore the caller's framebuffer, viewport, scissor, blend and clear color.
A failed SMAA setup must leave reconstruction available.

xBRZ is xbrz-rs 0.1.0, an actual CPU implementation of xBRZ 1.8. Preprocess
only fresh snapshots at fixed x3 to bound CPU and memory costs independently
of monitor size, then bilinearly sample the result into the original integer
fullscreen rectangle. Never replace last_frame_snapshot with the enlarged
image: touch coordinates, aspect ratio and native geometry use the original
image. Screenshots read the displayed output without changing that snapshot. Switching away from xBRZ must re-upload native dimensions.
Keep the dependency version pinned and its GPL-3.0-only notice and license
in frontends/pocket-desktop/licenses. Preserve SMAA's permission notices.

Validate the actual renderer with an OpenGL context: all modes, mode switches,
cached repaints, output orientation, smoothing, GL errors and state restoration.
Check diagonal blending and constant-color preservation in the reference SMAA
passes, and native-frame immutability in xBRZ preprocessing. Do not ship the
headless EGL validation executable or temporary Cargo/profile modifications.

## 29. Capture the displayed game pixels

F10 and the screenshot toolbar button queue a readback of the game rectangle
in the next paint callback, immediately after its draw, for both the custom GPU
renderer and ordinary egui textured meshes. Never recreate the image with a CPU
filter for a screenshot: that differs from SMAA, xBRZ and the GPU reconstruction.
Read physical pixels using PaintCallbackInfo so viewport rounding, Windows DPI,
fullscreen scaling and rotation match what was painted. Intersect the game
rectangle with its clip and the screen bounds; a clipped window captures only
the visible game area, without adjacent skin/UI or fullscreen bars.

Read from the current draw framebuffer and restore the previous read framebuffer,
pixel-pack buffer and pack settings. Flip bottom-up OpenGL rows to top-down PNG
rows and make alpha opaque while preserving displayed RGB. Encode/save the PNG
on a background worker, report completion/errors to the app and wake repaint.
Allow one outstanding screenshot; cancel an unpainted request when the game
ends or its screen becomes unavailable. Files stay in the library screenshots
directory. No capture readback happens during normal rendering.

Tests compare screenshots with actual GPU output for all eight filter modes,
including DPI and clip boundaries, then decode the saved PNG to verify its RGB
content and asynchronous completion. Keep that headless GL executable outside
the delivered patch. Interactive F10/window/fullscreen checks remain required
on Windows. No temporary instrumentation belongs in delivery.

## 30. PocketPC presentation and shared display controls

Desktop PocketPC uses the upstream unframed framebuffer and side pad: cardinal
D-pad, Action/A/B/C/1/2/Turbo. Gizmondo retains its skin. The obsolete
pocketpc_layout.rs module is removed. Pointer buttons and host arrows use inverse
user presentation rotation; a native landscape framebuffer is not rotated twice.
Keep source-aware held input and release keys before orientation changes.

Both platforms retain filters, physical x1/x2 window scaling, native-resolution
stylus mapping, integer fullscreen fitting, capture and shared FPS/status options.
Fullscreen preserves the rotated guest aspect ratio; it does not impose Gizmondo
4:3 on PocketPC. Android PocketPC restores its separate upstream control layout,
with persistent Rotate/Exit actions and per-game rotation. Its effective rotated
frame dimensions determine portrait/landscape independently of phone rotation.

Validate portrait/landscape, all rotations, x2 at different DPI, held controls,
stylus mapping, Turbo, fullscreen and return to the library.

## 31. Targeted missing-API reporting and scrollable options

Emulator options exposes log_unimplemented_apis, default true for both new
and pre-existing configs. Saving applies the AtomicBool shared by Runner clones
immediately, including suspended parents and running children. Append records
to pockethle-unimplemented-apis.log beside pockethle-gui.log in the library root.
Use launch/end records and missing-handler events with game, actual process
path, DLL/API, timestamp, first four argument registers, caller, thunk, PID/TID
and halt/return-zero action. Resolve friendly ordinal names when available.

This is independent of the verbose all-API trace and its log-level filter.
Capture registers only for actual missing calls when reporting is enabled;
implemented hot paths must not gain four extra register reads. Do not classify
intentionally registered constant stubs, ignored DLLs or handler errors as missing
handlers. De-duplicate identical API/call-site events per process to avoid log
flooding, compare metadata when thunk slots are reused, and count repeat calls
in the process end record. Flush new events so a crash leaves useful evidence.
No file creation or writes while disabled. An I/O error disables that process's
sink and warns once in the normal log; it must not alter guest return behavior.

Wrap the complete Emulator options editor, including keyboard bindings and
Save/Cancel, in a vertical ScrollArea so the mouse wheel can reach lower options.
Config tests cover default-on and persistence-off; reporter/dispatcher tests
cover live toggling, deduplication, reused imports, arguments and halt semantics.

VFSTEST v2 initializes both GlobalMemoryStatus output buffers before its NAND
quota baseline and reports exact before/after RAM values. Keep the strict
quota.nand_not_ram equality and all 103 checks; do not add a tolerance that
conceals genuine NAND/RAM accounting defects. Windows v1's isolated quota
failure was not reproduced in native ARM or the actual frontend runner here.
The v2 report is needed to identify the direction and size of that variation.


## 32. Desktop preferences, platform tabs and PocketPC directional controls

LauncherConfig.upscale_filter stores a stable filter identifier, saved when the
user selects a filter and restored on launcher startup. Old configurations and
unknown identifiers fall back to GPU reconstruction. Keep all eight filter IDs
stable; screenshots and fullscreen use the restored selection too.

Library tabs use the same is_gizmondo_game classifier as the running device
layout. Keep the details/selection restricted to the visible platform, show
both tab counts and allow imports from an empty tab. Successful imports select
the imported game's platform. No game files move when switching tabs.

For PPC keyboard arrows apply the inverse user presentation rotation to the
guest VK. A natively landscape framebuffer already has landscape coordinates,
so its orientation must not be applied again to keyboard input. PocketPC pointer-pad arrows remain upright in the original side panel and
use the same inverse user presentation rotation as keyboard arrows. Their
visible arrow and the matching host arrow key must agree. Action buttons retain their
bindings; Gizmondo keys retain their existing behavior. Release held input on
rotation or PPC framebuffer geometry changes. Preserve geometry-based checks
for all four rotations with portrait and landscape native framebuffers.


## 33. Wave loop break, CE power status and offline Winsock

waveOutBreakLoop finishes the current iteration of the existing single-WAVEHDR
loop without resetting PCM, pause state or live refresh. Shorten the loop end
and all following pending cursors for that HWAVEOUT only. Do not publish DONE
until playback reaches the shortened endpoint. Invalid handles return
MMSYSERR_INVALHANDLE; a valid device without an active loop succeeds. Preserve
Stuntcar's queued auto-reset event completions and CPAL driver-default buffering.

GetSystemPowerStatusEx writes the 24-byte CE ARM structure, returning BOOL.
Ex2 writes 56 bytes including ABI padding, returning 56; buffers larger than
56 keep their extra bytes intact. Reject NULL, undersized, overflowing, unmapped
and read-only outputs with zero and thread GetLastError=87 before writing.
Status projects the host battery through kernel::power. Windows calls
GetSystemPowerStatus; Android reads the sticky ACTION_BATTERY_CHANGED Intent
without registering a receiver or requiring permission. A shared one-second
cache is used unless fUpdate requests a refresh. Failed reads report unknown,
never an invented critical battery. No-battery hosts report flag 128/percent 255.
Backup battery is absent, optional telemetry is unknown. Successful calls
preserve GetLastError. Android retains only application context and one JNI
class reference, with no timers, listeners, workers or per-game resources.
KernelState owns bounded power notification subscriptions. RequestPowerNotifications
validates queue, filter and packet capacity and returns a unique process handle;
StopPowerNotifications removes it (stale handles fail with GetLastError=6).
PBT_POWERSTATUSCHANGE (4) tracks supply changes; PBT_POWERINFOCHANGE (8) sends
POWER_BROADCAST with a 24-byte POWER_BROADCAST_POWER_INFO at offset 12.
Only requested changes are queued. Full queues keep the latest change pending;
CloseMsgQueue and process teardown release registrations. Existing frontend
polling, message pumps and wait APIs service notifications; no new worker/timer.
ReadMsgQueue now preserves messages on small/invalid buffers, returns BOOL FALSE
with ERROR_TIMEOUT=1460, and cooperatively waits rather than inventing success.
SDK Examples/SysMessages/SysMessages.h names LOWBATERY_MSG and CRITICALBATERY_MSG;
the Developer Guide section 2.14 also spells BATTERY. Both spellings are accepted
when registered, with one message ID chosen per severity to avoid duplicates.
The existing HWND_BROADCAST message queue carries zero params, as the SDK sample
uses only message IDs. Alert transitions are emitted once on entering low/critical,
including low startup once a WndProc exists; recovery/charging rearms them. Unknown
reads do not fabricate an alert or rearm. No forced shutdown is synthesized.
CHARGERFAULT_MSG requires actual charger fault telemetry not exposed by the Windows
power-status API; unknown AC status must not be mistaken for a charger failure.

ws2.dll implements an explicit offline boundary, not network/Bluetooth play.
WSAStartup fails directly with WSASYSNOTREADY (10091), without writing WSADATA.
recv and WSACleanup fail with SOCKET_ERROR and WSANOTINITIALISED (10093).
WSAGetLastError and WSASetLastError use separate per-thread Winsock storage;
reads preserve the value and do not modify GetLastError. Never report recv=0
for unavailable networking: zero means graceful peer EOF. Unknown networking
APIs still enter the missing-API report; do not hide them behind DLL-wide stubs.

SDK(3).zip developer guide sections 2.9 and 2.14 confirms CE Bluetooth APIs
and cached GetSystemPowerStatusEx2 use. Bluetooth.cpp initializes Winsock 2.2
for device discovery, but exchanges data via RFCOMM virtual COM/ReadFile.
This archive does not include the platform winsock2.h or ws2 export library;
do not invent ordinal mappings from desktop Winsock.

## 34. General API completion and Gizmondo ROM exports (2026-10-09)

CopyFileW is a dispatched operation, never a baked TRUE thunk. Vfs::copy_file
opens the source with read sharing, checks actual host identity before truncating
the destination, copies through VFS read/write, and observes RAM charging,
read-only mounts, file leases and the 32 MiB Flash quota. Host quota preflight
preserves an old destination on capacity failure. Other I/O failures during an
overwrite can leave a partial destination; no transactional overwrite is promised.

Registry values can be attached to a versioned JSON snapshot. Desktop and Android
runners select registry-gizmondo.json or registry-pocketpc.json below their root.
Load after attaching device RAM, before applying installer defaults; saved values
win except InstallDir, which must follow the current installation. Child processes
reuse the already attached shared store. Persist values and key display names,
never process handles or RAM-charge objects. RegFlushKey validates HKEY and returns
LSTATUS directly. Normal/error run termination also flushes. Temporary writes are
synced then renamed on the same volume; corrupt input blocks launch and is kept.
Concurrent emulator instances writing one registry snapshot are not coordinated.

SetTimer stores independent timers keyed by (thread, HWND, ID); replacement resets
only that timer's deadline. KillTimer really removes its timer; DestroyWindow removes
associated timers. Preserve the existing 1 ms lower interval limit for CE games.
Coalesce overdue intervals rather than generating bursts. WM_TIMER lParam carries
TIMERPROC, and DispatchMessage calls it with (HWND, WM_TIMER, ID, MSG.time), restoring
the existing call frame on callback return. Workers see only their own explicit
timers; they still never receive synthetic window paint traffic. A blocking worker
GetMessage uses the timer deadline to become eligible for the scheduler again.

ws2-ordinals.json (79 exports) and btd-ordinals.json (83 exports) come from the
supplied Gizmondo ws2.dll and btd.dll export directories. Aliases for implemented
Winsock handlers use those ROM ordinals. These tables do not implement the APIs:
Winsock remains the explicit offline boundary described above. Native Windows/
Android Bluetooth, device discovery, RFCOMM virtual COM and the BTD driver context
APIs are not delivered in this general-API patch.

## 35. Bluetooth Classic SDK path and native transports (2026-10-09)

The offline boundary in section 33 is superseded only when a Bluetooth backend
exists. WSAStartup negotiates supported CE versions and validates/writes the 400
byte ARM WSADATA before incrementing a per-process reference count. No-backend
startup still returns 10091; recv without startup still fails with 10093. A direct
socket recv is not implemented and returns 10038 after startup, never a fake EOF.
The Windows host Winsock stack is initialized lazily, independently of guest refs.

The supported game path is the supplied SDK Bluetooth.cpp: BT_MSG broadcast,
WSAStartup, gethostname, WSALookupServiceBegin/Next/End, RegisterDevice("COM",
index,"btd.dll",PORTEMUPortParams), CreateFile(COMn:), SetCommMask(EV_RXCHAR),
WaitCommEvent, ReadFile/WriteFile, DeregisterDevice. Discovery runs on a host thread;
pending guest I/O cooperatively retries with its call arguments intact and a
scheduler deadline. Completed retries remove their deadline. Query output uses
32-bit guest pointers and packed 30-byte SOCKADDR_BTH; short buffers report the
required size without advancing the result index. Winsock errors remain separate
from WinCE LastError. WS2 also has a resident module handle for dynamic imports.

Bluetooth stream descriptions are VFS Device objects with access and share leases,
so duplicated handles retain the real stream across process namespaces. Service
registration generations have distinct lease keys: the SDK deregisters ports
without closing its old handles, and these cancelled handles must not prevent a
new COM4 registration. Deregistration/BT_MSG off closes host resources and makes
old pending operations fail with 995. No bytes available means pending, not EOF.
Synchronous COM writes retain their byte snapshot and offset across retries;
native partial writes/backpressure must not truncate a packet or resend its prefix.

Windows uses Bluetooth inquiry and nonblocking AF_BTH RFCOMM Winsock connections.
Servers bind a dynamically allocated channel and publish an SDP service; closing
the registration removes the SDP record and sockets. Android uses secure
BluetoothSocket, bounded RX/TX queues, and separate connect/read/write threads;
close cancels accept/connect and blocked queues. Its JNI class is captured from
the caller before spawning the emulator, avoiding FindClass/class-loader failures.

bluetooth_enabled defaults false. Emulator options exposes it on desktop and the
Android settings screen exposes the equivalent switch. BT_MSG controls only the
emulated service while this setting permits hardware access; it does not forcibly
turn the OS radio on/off. Android requests scan/connect (or legacy location)
permissions before starting an opted-in game. Denied permissions/radio off remain
real errors. Two PocketHLE hosts agree on a stable service UUID per guest channel;
an explicit SDK service GUID is honored. Real Gizmondo interoperability with its
fixed physical RFCOMM channel is not verified or guaranteed by this mapping.

Scope limits: general IP sockets are not implemented. Direct guest Winsock
RFCOMM sockets on Windows are covered by §37. Raw BTD HCI/L2CAP/SDP driver exports, service/filter
lookup queries, overlapped COM I/O, non-default MTU/quota requests, and UART modem/
DCB control remain unsupported. EV_RXCHAR and mask reset are supported. Legacy REMOTE_DCB/KEEP_DCD
flags are accepted for SDK compatibility; they do not expose modem/DCB behavior.

pockethle-bt-test is an optional standalone native-radio diagnostic, never run by
the GUI automatically. scan/server/client modes use the same transport and test
a real two-way ping/pong on guest channel 2. Software tests cover the full SDK
contract, invalid buffers, pending reads, mask cancellation, sharing/duplication,
deregister/re-register, query sizing and separate error domains. Native Windows
and JNI Rust modules have been type-checked; no physical-radio test or full
Android APK build was possible in this environment. Hardware validation remains
required before treating multiplayer compatibility as established.


## 36. Desktop physical controllers

The desktop frontend uses bundled, statically linked SDL2 (sdl2 crate 0.38.0).
SDL's HIDAPI Switch driver initializes the Nintendo Switch Pro USB/Bluetooth
protocol. Only SDL input/event subsystems are initialized: egui owns the window
and CPAL still owns audio. SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS=1 is required
because egui's window is not an SDL window; actual guest/capture focus is gated
by egui. SDL_GAMECONTROLLER_USE_BUTTON_LABELS=0 preserves position-based names.
SDL_JOYSTICK_HIDAPI and SDL_JOYSTICK_HIDAPI_SWITCH are enabled before startup.
A worker collects transitions every 4 ms and wakes egui; it ignores repeats,
uses stick hysteresis (press 0.60 / release 0.35), and stops when the GUI drops
its monitor. No controller polling or timing changes enter the guest kernel.
Keyboard, pointer and controller holds remain independent. Multiple physical
controls driving one guest VK are aggregated; release/disconnect cannot cancel
another source. Focus loss, settings and run lifecycle release guest keys.
Directions use the existing PocketPC inverse rotation.

LauncherConfig.gamepad_bindings is an independent persistent map of physical
control names to GuestButton. Missing legacy fields receive defaults; explicitly
empty maps stay empty. Capturing a control moves it to the selected command.
Keyboard bindings are preserved. Settings shows connected devices, selects the
first automatically, allows choosing another and resetting/removing bindings.
Face labels identify positions and Switch/Xbox lettering. D-pad and left stick
are mapped by default; right stick and additional buttons can be captured.
Connection is managed by the host OS. This does not depend on the emulated
Gizmondo Bluetooth setting. Actual Switch Pro hardware validation remains a
user-side check. Native virtual SDL controller tests verify queued press/release
without a physical controller. Capture updates the settings draft when an event
arrives, so it survives UI redraws; a last-input indicator gives immediate user
feedback without writing probe logs. Generic unmapped joysticks have raw button,
axis and hat capture; normalized controller events are not duplicated by their
underlying joystick events. CMake 4 users set CMAKE_POLICY_VERSION_MINIMUM=3.5
when building SDL's bundled sources. Deliver raw console commands, not BAT/CMD scripts.

## 37. Guest Winsock RFCOMM sockets

ws2/sockets.rs registers socket/bind/listen/connect/accept/send/recv/select,
closesocket/shutdown/ioctlsocket, options and local/peer address queries as
stateful handlers. Handles are process-local IDs; native socket IDs never reach
ARM. Windows uses real AF_BTH/RFCOMM sockets, always nonblocking internally.
Guest blocking waits yield via the existing scheduler; receive/send timeouts
and select deadlines persist across retries. Partial sends return their actual
byte count. No-data returns WOULD_BLOCK/waits; zero receive means real EOF.
FD_SET is count plus 32-bit IDs, max 64; readiness polling does not consume data.
Pointers and address outputs are validated before accepting/consuming data.

SOCKADDR_BTH on ARM/CE is 40 bytes: family at 0, u64 address at 8, GUID at 16,
port at 32. Host padding differs and must be translated. Inquiry now returns
40-byte addresses too; 30-byte packed and 32-byte caller layouts remain readable.
Socket options go to the native provider; TCP-only options on RFCOMM may return
WSAENOPROTOOPT rather than fake success. FIONBIO changes guest semantics only.
Startup refcounts and WSA errors stay per process/thread. Final WSACleanup and
process teardown drop sockets; disabling the shared Bluetooth service closes
native sockets in all processes through weak registrations. No strong cycles.

WSALookupServiceNextW preserves the caller's buffer capacity on success; it
only reports the required size on WSAEFAULT. Chicane's ARM inquiry loop
initializes its 1024-byte capacity once outside the loop. Shrinking that DWORD
after a short device name makes the next longer result fail and prematurely
ends inquiry. The Rust regression enumerates increasing names with one shared
capacity; tools/bttest/chicane-inquiry-replay.py reproduces this branch with
the supplied game executable and controlled API responses, without hardware.

Chicane first opens BTH1: (CreateFileW, OPEN_EXISTING, share=0) and sends
DeviceIoControl 0x01012120 with zero input/output sizes. Stop is 0x01012124.
Both are FILE_ANY_ACCESS controls. Its actual ARM initializer at 0x2d264 returns
false before gethostname if either CreateFile or the start IOCTL fails; the
constructor's ASCII "Unknown" then remains the contestant name. BTH1 has a
shared open description/lease, process duplication, sharing checks and ordinary
CloseHandle/close_all cleanup. Closing the handle alone does not stop the driver;
the explicit stop closes the shared service's ports/sockets. Start checks the
user's allowed gate and backend readiness, then enables the emulated driver;
it never forces the physical radio on. Success preserves LastError; disabled
permission returns 5, absent/not-ready backend 21, invalid payload/output pointer
87 and unsupported control/stream operations 50. Bytes returned is zero.

Raw-socket providers exist on Windows and Android. Android's JNI JavaSocket
uses RfcommHost, with nonblocking guest operations and worker-owned BluetoothSocket
connect/accept/read/write. Chicane calls socket(AF_BTH=32, SOCK_STREAM=1,
BTHPROTO_RFCOMM=3); its inquiry uses LUP_CONTAINERS and name/address results,
so implementing only the SDK COM example does not cover it. Explicit guest
GUIDs become Android service UUIDs; otherwise the stable PortParams UUID maps
logical guest channels (including SDK channel 2) to Android SDP services. Windows
raw listeners publish that UUID via WSASetService; clients resolve it with port=0,
so Android's allocated physical channel is usable by Windows too. Listeners remove
SDP registration on Drop. Both COM and raw Windows hosts acquire a shared radio
visibility lease; its refcount is synchronized with restoration of prior radio
states on last close. Radio handles and failed-start leases are released too.
Original-console physical-channel interoperability is outside this mapping.
Windows raw sockets require authentication, as Android's secure sockets do;
Windows may require pairing in system settings before the game's connect. There are at most 64 registered endpoints, 8 unclaimed peers
per listener, 64KiB receive and 16 x 4KiB transmit queues per active connection.
MSG_PEEK and readiness do not consume input; SO_ERROR exposes asynchronous
connect failure. Socket buffer size options are advisory, capped at 64KiB;
TCP-only options report WSAENOPROTOOPT. Android has no Bluetooth half-close:
shutdown gates the requested local direction; SD_BOTH closes the native socket.
JNI global references belong to each endpoint and are released on Drop.
closeAll gates late creation, removes endpoints before closing them to avoid
lock inversion, closes unclaimed peers, interrupts workers and clears queues.
GameActivity destruction also closes COM sockets and cancels inquiry. Retained
context is application-only; the Activity used for discovery UI is weak.
Android12+ requests BLUETOOTH_ADVERTISE alongside CONNECT/SCAN. Hosting opens
Android's consent dialog for 300-second discoverability; no radio/consent bypass.
Inquiry waits for ACTION_DISCOVERY_FINISHED (15-second cap), rather than checking
isDiscovering immediately after asynchronous startDiscovery. Receivers are
unregistered in finally and closeAll wakes an outstanding inquiry.
General IP sockets, WSA event/async APIs and SDP service queries are outside
this pass. Physical multiplayer tests remain required. The optional
pockethle-bt-test socket-server/socket-client modes exercise real channel-2
RFCOMM sockets and ping/pong on two Windows hosts without changing game behavior.

CRT _set_new_handler and _query_new_handler store/return a process-local callback;
C++ set_new_handler has a separate callback slot. They must not be constant
thunks. This pass does not change the existing allocator's OOM callback policy.


## 38. Gizmondo CAM1 camera and host capture

The supplied Gizmondo SDK camera sample uses CreateFileW("CAM1:") and
DeviceIoControl HAL functions 2101..2106 (METHOD_BUFFERED, FILE_ANY_ACCESS).
SETFORMAT/GETFORMAT transfer two SIZE records: capture 640x480, preview
positive multiples of 8 up to 640x480. VINFRAMEINFO is 16 bytes (width, height,
frame count IN/OUT, timeout ms). Preview is bottom-up little-endian RGB565;
still capture is 640x480 planar Y/U/V I420, using limited-range BT.601. SDK
states YUV420 without explicitly documenting chroma plane order; physical
Gizmondo parity for I420 remains unverified. No proprietary SDK code is shipped.

camera.rs owns a shared device and a latest-frame mailbox backend. Hardware
frames never expose host pointers. Preview delivery is limited to 20fps per
shared device. A serial is not consumed twice; returned frame count counts
delivered frames. A pending guest request yields through the scheduler, with
its timeout retained by (thread, thunk, SP). timeout 0 polls and returns 1460.
Invalid guest buffers are checked before consuming the frame. Stop and final
handle close release capture; duplicate/cross-process VFS handles retain the
same device and sharing lease. Disabling the service stops capture across
processes. Service stores the device weakly, avoiding a capture lifetime cycle.
Undocumented 2107 GETTHREADS / 2108 WRITE / 2109 READ and overlapped requests
return ERROR_NOT_SUPPORTED rather than fabricated success.

Windows uses Media Foundation on a dedicated MTA thread. Only a bounded latest
RGB frame reaches the guest; an agile COM reference permits Shutdown to cancel
a pending reader when stopped. RGB32 signed stride is handled (including
bottom-up contiguous buffers). Native geometry is resampled to guest geometry.
Android uses Camera2 and YUV_420_888 ImageReader with row/pixel strides and
crop handling. JNI stores JavaVM/GlobalRef before launching the emulator.
CameraHost prefers the rear camera; Windows selects the first enumerated
webcam. There is no camera-selection UI in this pass. camera_enabled defaults
false and is exposed under Emulator options / Android settings. It grants
permission to the driver, and does not start capture by itself. Android requests
CAMERA runtime permission before opted-in game startup. Denial is a real error.
Pause releases physical capture; resume reopens wanted sessions. Generations
close stale asynchronous callbacks, and all Images are closed in finally.

Tests cover buffers, formats, colors, errors, retries/deadlines, preview cadence,
duplicate lifetimes, and actual ARM CAMTEST execution with synthetic frames.
The optional tools/camtest ARM package writes CAMTEST.TXT, a RGB565 BMP preview
and an I420 capture to Flash Disk. Native Windows and JNI integration were
Rust type-checked. Android logging alone was omitted in a temporary Linux
validation harness; production logging was retained. No physical webcam/mobile
camera test or Android APK build was possible here. CAM1 support does not
implement PocketPC/DirectShow camera interfaces or undocumented sensor controls.


## 39. Catapult continuous camera preview and mixer cleanup

Catapult's supplied ARM image reads CAM1 successfully but clears its
acquired-frame flag when the tracker returns zero markers. Its render path
therefore uploads a camera texture only after a detection. A controlled
white/marker/white input reproduced this: zero updates outside the marker
interval, 33 uploads within it. This is a game-code compatibility repair,
not a camera-driver, scheduler or GLES cadence change. pocket-pe recognizes
the entire 48-instruction routine suffix in ARM executable sections and
replaces only the flag-clearing STRB with an ARM NOP in the loaded copy.
On-disk files, marker counts and tracking transforms remain unchanged.
Unknown image versions do not receive the repair. Never match by game name,
absolute VA, or a short opcode pair; keep negative-signature/idempotence tests.
Temporary camera/texture probes belong only to the local validation runner
and must not be distributed.

Chicane's cleanup calls coredll mixerClose(NULL). No mixerOpen handles are
currently issued: mixerClose rejects every supplied handle with
MMSYSERR_INVALHANDLE (5), preserving GetLastError and never closing a wave
or VFS handle. This implements cleanup validation, not the full WinMM mixer
family; mixerOpen/line/control APIs remain outside this patch.


## 40. CAM1 preview row orientation

The Gizmondo SDK Camera sample supplies -height to CreateBltDIB, which negates
it again: the resulting RGB565 DIB has positive height (bottom-up storage).
The initial CAM1 implementation incorrectly sent top-down preview rows.
A four-color host frame reproduced an upside-down Catapult camera view while
menu text remained upright. Reverse output rows only in camera::preview;
columns retain their order, native Windows/Android Frame RGB stays top-down,
and still-capture I420 retains its previous layout. Do not rotate the GLES
framebuffer, apply a game-name exception, or reverse host buffers globally.
This shared CAM1 ABI fix applies to Agaju's preview too; its attached executable
requests a 64x64 preview format. Its full gameplay assets were not attached,
so physical Agaju validation remains user-side. The CAMTEST BMP now declares
positive height to agree with the driver's bytes; older CAMTEST builds may
save a vertically inverted BMP even though their API checks pass. Tests check
all four preview corners and preserve left/right and capture orientation.


## 41. Colors multiplayer endpoint, identity and 64-bit time

Colors uses the historical host us.mygiz.gizmondo.com and POST route
/applications/games/colors/open/command.do. LauncherConfig carries
colors_server_url defaults to https://vps-50cdd0fc.vps.ovh.ca; missing or
blank legacy values migrate to that origin on load, while custom origins are
preserved. Android Kotlin uses the same default and blank migration. Clearing
the field restores the public default on the next load. colors_terminal_id
remains empty by default; preserve the matching Android Kotlin JSON keys. Desktop and Android
apply configuration only for Gizmondo after registry persistence is attached.
The URL is an HTTP(S) origin, without a path, credentials or query.

Route only that host AND route when WinINet starts the backend request. Keep
the guest RequestSpec unchanged, clone the outgoing spec, update port/secure
flag, and drop stale Host headers. Other services are not redirected.
KernelState owns the route; do not introduce a process-global endpoint.

HKLM\GTShell\TerminalID is the player identity. Empty launcher ID preserves
an existing value; first launch generates and immediately flushes one to the
installation registry. Explicit ASCII alphanumeric, '-', '_', '.' ID overrides
it persistently. Separate players require distinct libraries/registries or IDs.
Seed GNS as http://us.mygiz.gizmondo.com/ when absent, and migrate the old
bare-host seed. Colors f41e4 extracts the host between URL slashes: a bare
host produces an empty InternetConnectW server. Preserve other GNS values.
Never regenerate identity every boot.

LauncherConfig.gprs_enabled defaults to true, including older JSON configs.
Desktop and Android set the per-process data gate on Gizmondo launch. For a
Gizmondo process with the data option configured, HWND_BROADCAST / registered
GPRS_CONNECT receives
registered GPRS_STATUS, wParam=0x1f9c and lParam=1 when data is enabled and a
host HTTP backend exists; disabled/disconnect returns 0x1f9d and 0. This is
transport availability, not proof that the server is reachable. Actual HTTP
errors remain real. Do not change the registered-message hash or pretend to
connect the historical cellular network. Other processes retain ordinary PostMessage behavior. Colors f39a0 waits on this shell notification
before InternetConnectW; missing it leaves Logging in indefinitely.

Register coredll.dll/_atoi64 (ordinal 1418 is resolved by the existing ordinal
map). Parse the signed decimal prefix and return the full i64 register pair
R0 low / R1 high. Timestamps exceed 32 bits; reusing atoi truncates them.
Overflow is deterministic saturation; this is not a claim about undefined
legacy CRT overflow. Keep unrelated CRT handlers unchanged.

Regression tests exercise configuration serialization, identity persistence,
route scoping, TLS/port metadata, signed 64-bit values, real WinINet POST
submission, fragmented reads and cleanup. Original Colors ARM component tests
live in the separate server project and require the user's executable. Windows
desktop and Android packaging and a full interactive Turf War session are not
validated by a Linux CLI boot or component-parser success.


## 42. Desktop Gizmondo options navigation

The desktop Settings menu has Emulator Settings followed immediately by
Gizmondo options. Screen::GizmondoOptions renders GPS, GPRS/data, Colors server/ID,
Bluetooth and camera controls; Screen::Settings renders general emulator
options and keyboard/controller bindings. Both views edit a full LauncherConfig
draft and share Save/Cancel persistence, so saving either preserves fields
from the other. They have separate scroll IDs. Only the general settings
screen participates in keyboard/controller binding capture; opening Gizmondo
options releases held keys and clears pending captures. GPRS/data is saved through
the same draft and takes effect on the next launch. Android JSON mirrors its
boolean default, while this menu organization applies to the desktop launcher.

## Windows and Android fixed GPS position

Gizmondo options exposes an explicit simulated fixed GPS setting on Windows only,
with decimal latitude [-90,90] and longitude [-180,180]. Android exposes the same simulator in its separate Gizmondo menu. LauncherConfig persists
gps_fixed_enabled (default false), gps_fixed_latitude and gps_fixed_longitude.
At the next Gizmondo launch the desktop runner validates the coordinates and
sets the per-VFS gps::Service override, enabling GPS1 independently of the host
GPS permission checkbox. Host consent preparation is skipped while this mode is
selected. Android applies the same per-VFS override before GPS1 opens and skips real GPS permission requests while using it (other features may still require location permission for legacy Bluetooth). The override is never
installed globally and therefore does not leak between emulators.
FixedCapture creates a new current Unix timestamp at each read; existing packet
serialization supplies the GPS1 1972 clock and signed coordinates. Horizontal
error is an explicit simulator value of 5 m; speed is zero. Altitude and satellite
data remain unknown. Turning off simulation restores the ordinary host GPS
choice at the next launch. Invalid stored coordinates fail the launch clearly.
Kernel tests cover no-host operation, validity, coordinate ranges, fresh clock,
permission revocation and restoring the host path. Launcher tests cover backward
compatibility and persistence of the simulator settings.

Validation: four kernel GPS tests and all 38 launcher-library tests pass. The
patched Colors executable also reaches Turf War's map using FixedCapture at
Sydney (-33.8688,151.2093), with no host GPS backend installed. The actual local
HTTP server receives precisely these coordinates. Map tiles are fixtures; the
Linux native game test does not compile or exercise the Windows settings UI.
Desktop UI/runner syntax is checked separately; a Windows build remains to run.

GPS diagnostics now use the standard launcher INFO log, not just RunOutcome's
UI summary. Desktop startup records gizmondo/host/fixed settings, and the Windows
fixed override records activation. GPS1 records open errors and read-state
transitions (error, initialized, validated, GPS1972 time and horizontal accuracy).
Unchanged reads do not repeat the diagnostic. No satellite counts are fabricated.
These diagnostics do not establish the cause of a user's blocked validation
until their resulting logs and server clock/version are available.

Colors large map responses: native string construction calls vsprintf with the
entire XML as its format. A silent 16 KiB format scan cap truncated a 96,824-byte
getmapview response, losing closing XML/CDATA and producing Server Error after
Retrieving map data. Unbounded narrow CRT format/string scans now stop at NUL,
unreadable memory or the 32-bit address boundary, without arbitrary 16/64 KiB
limits. Explicit destination capacities and bounded string operations remain.
Regression checks 100,026-byte strlen/strcpy/vsprintf and a strstr match after
100,000 bytes; 14 string tests and printf precision regression pass.
Native Linux Unicorn run with patched Colors, production fixed GPS service,
actual local HTTP and saved real OSM GIF now renders the Turf Wars map: XML
96,824 bytes, GIF 62,693 bytes. Requested GPS coordinates were
45.62191112730281, 6.773974779758014; replayed GIF depicts Paris, so this test
verifies payload handling rather than live OSM rendering at those coordinates.
No production CLI adapter or server change. Windows desktop build remains
to run; no claim of Windows UI or physical Synology execution.


## Android frontend and native packaging

Android library/settings activities are portrait. Gizmondo gameplay uses immersive
sensor-landscape; PocketPC follows its rotated game dimensions. Settings remain
outside gameplay, with an optional noninteractive FPS overlay. Stop waits for the native worker and audio readers
before returning to the library, preserving registry/save flushes and JNI session lifetime.
Audio generations prevent a paused reader from restarting alongside its replacement.
The game surface occupies a separate center rectangle; independent multitouch child
buttons live in side rails. Actions are RIGHT, D-pad LEFT, shoulders above each rail.
Five traced native function icons match the desktop skin; the fifth sends VK_F11.
Physical keyboard/controller maps use the shared stable config names and are editable
in a separate settings submenu. Explicitly empty bindings stay empty. F10 captures
actual GL output after filter/rotation, without introducing a gameplay settings bar.

Gizmondo launch forces 320x240, preserves the SD-card layout, and mounts writable
library/flash at \Flash Disk. It applies fixed GPS, GPRS/data, Colors origin/identity,
Bluetooth and camera permissions through the existing native adapters. PocketPC keeps
its configured guest dimensions and presentation rotation. Android DTO saves preserve
the entire incoming JSON alongside edited fields, including PC controller/logging fields.
Rename uses Library::rename_game and never changes IDs, paths or save locations.

Display scale android_display_scale defaults to 0 (fit); 1..4 are native pixel multiples,
clamped to the available center surface. Never change guest dimensions for zoom.
DisplayGeometry supplies the same integer rectangle to GL, stylus mapping and readback.
All eight desktop filter IDs are used: exact reconstruction/bicubic/Lanczos shader,
reference three-pass SMAA and lookup textures, xbrz-rs 0.1.0 at internal x3, bilinear,
nearest. SMAA/xBRZ work only on fresh snapshots; cached textures redraw on resize.
GLES 3 is required. Shader/FBO failures are reported, never silently mislabeled.

Native packaging enables Unicorn and video-static by default. No successful gameplay
fallback to the trace-only Stub. Build scripts use per-target NDK CMake wrappers,
pinned FFmpeg 8.0.1 (same codecs as desktop), compiler-rt builtins and 16 KiB ELF
alignment. Never enable Unicorn arch_all. No global Cargo target configuration.
Gradle preBuild rejects missing bridges; ELF and APK alignment/signature checks run
in the build scripts. The dedicated Android workflow is manual; CI also builds the APK on main pushes and pull requests. See docs/ANDROID-BUILD.md.

Validation: all Kotlin sources type-checked against Android 14 framework and actual
AndroidX dependencies/generated R; AAPT2 compiles/links resources; 240 display
configurations pass inverse touch/corner/letterbox checks; 28 headless Mesa GLES
renders check seven GPU modes in all rotations, including all three SMAA passes.
JNI Rust type-check (Linux, no native/default features) and 38 library tests pass.
No Android ABI .so, full Gradle/D8 APK, physical phone or Android Turf Wars session
was built/run in this environment. These remain required integration checks.
This UI/build update did not add a general CLR/.NET CF backend. Android raw
Winsock RFCOMM was subsequently added for Chicane; see section 37.

Android peripheral software checks: 4 kernel GPS + 1 GPS ABI, 6 kernel camera + 6 camera ABI and 3 Bluetooth serial tests passed. These do not validate Android physical sensors or Bluetooth radio.

COREDLL `_wcsrev` reverses 16-bit wchar_t units in place, returns its input address, leaves NUL/guard bytes intact and propagates invalid memory access. Two targeted tests cover empty/null, odd/even lengths, unaligned addresses, non-ASCII/surrogate units and page boundaries.

Android validation update (2026-10-10): both release native ABI builds passed with NDK r28c, real Unicorn and static FFmpeg. Full Gradle/Java/Kotlin/D8 assembleDebug succeeded with Build Tools 35.0.0 explicitly selected. APK v2 signature, zipalign 16 KiB, both packaged ELF alignments/JIT symbols and assets verified. No physical-device launch, camera/GPS/radio or Android Turf Wars session was executed. pkg-config is required by Unicorn configure; native script now checks it before compilation.

### Android presentation and documentation invariants

Project documentation and Android user-facing text are maintained in English.
The portrait game library and separate settings menus precede immersive landscape
sessions. Gizmondo action buttons remain on the physical right, D-pad on the
physical left, with L/R above the corresponding groups; gameplay layout is LTR
independently of device locale and landscape rotation.

`DisplayGeometry.kt` owns the integer viewport shared by GL output, screenshot
readback and inverse touch mapping. Auto selects the largest positive integer
native-size multiple that fits the game area. Explicit factors are capped at
that integer; unused pixels form letterboxes. Gizmondo always uses native 320×240.
An undersized viewport clips at ×1 rather than fractionally resizing. Keep these
rules synchronized with `docs/ANDROID-BUILD.md` and Android settings labels.

### Android child processes and RAM/VFS diagnostic parity

The Android session runner enables generic CreateProcessW and drives each child
on its own host thread, using the same shared handle domain, VFS leases/object
store and MemoryDivision as its parent. Child setup acknowledges readiness before
the parent commits PROCESS_INFORMATION and releases the startup gate. Command
lines, CREATE_SUSPENDED, remote thread controls and descendants retain the core
contracts. Keep the original SD-card mount root when launching a nested executable.
SDCreateProcess retains the card-bound, preserved-parent foreground handoff.

Only the foreground process consumes UI input and publishes frames/audio. Stop
is shared by all jobs, including children waiting for startup; session completion
joins all descendants. Drop each emulator and refund its private RAM, then publish
its process completion BEFORE joining its children. Otherwise an orphan waiting
for that parent times out. Do not announce process completion before RAM teardown.

The previous Android runner left process_launch_enabled false. Executing VFSTEST
with that configuration reproduces all 24 failures in the supplied Android report,
including the later rename/delete/quota failures. The enabled runner passes all
103 checks twice with complete cleanup; RAMTEST passes 132 checks and the separate
process, orphan, DLL and dependency reports. These are actual ARM programs driven
by the Android Rust runner on a Linux host, not Android APK/VM/device execution.
Keep that validation distinction explicit. No shared Windows kernel/VFS behavior
was changed to compensate for the Android launcher omission.

### Temporary Android Colors registration probes

The Android runner installs register-only observers when the loaded executable
matches complete registration/user-parser signatures, the XML callback entry
and the local TWPlayerValid branch from the supplied Colors image. Unknown
images are untouched. These observers distinguish XML-result, profile-result,
registration rejection, the TWPlayerValid setter and accepted completion.
They do not set flags, skip instructions, alter registers or suppress errors.
Remove these temporary Android probes after identifying the device failure.

Cpu::add_register_trace is an optional non-stopping ARM diagnostic capability;
Unicorn owns these hooks in the same list as other code hooks so unmapping
removes them. No desktop caller installs them. Preserve --watch stop semantics.
The CPU regression verifies that an observed ADD executes and retains its
result, with only the subsequent ordinary hook stopping execution.

The user's Android HTTP/server logs show getuserinfo ok, dissolvegang rejected
for no membership, removeplayer ok, NoSuchUserException, and setuserinfo ok.
All response bytes reach the guest. This establishes the reset loop, not why
the registration callback fails. Replaying the original ARM setuserinfo
callback with a generated server response reaches the TWPlayerValid setter;
the test intercepts that setter, so it does not validate persistent config.cfg.
Do not claim that local save failure is the immediate cause without the traces.

The corrected matcher uses the runner's retained PE image, not Process.image:
the kernel deliberately frees section data after mapping. The October 10 device
trace reaches the accepted registration path (profile result 1). Follow-up
observers distinguish the post-registration TWPlayerValid check at 0xdd930
from the network error path, and observe SaveFile read/Character lookup/write
and config-save calls. A write return is observed without asserting its ABI
or persistent success. The local validation failure path selects
TW_UNKNOWN_ERROR at 0xdd948; it has not yet been observed on the device.

### Android on-screen control feedback

GizmondoControls supplies independent StateListDrawable backgrounds: a blue
highlight lasts until release/cancel. Clear platform background tint so themed
buttons do not obscure that highlight. Virtual key ACTION_DOWN requests one
30 ms vibration per newly held key (amplitude 180 where supported, default
amplitude otherwise, including API 24/25); do not repeat it during holds or on
release. Respect Android's system/view haptic settings and preserve split multitouch
and guest-key reference counting. Exit uses normal Android touch/click handling.
releaseHeldInput clears child pressed states as well as held guest input.

Android AudioTrack pulls use approximately 10 ms of whole PCM frames, with a
requested effective output buffer of 20 ms (clamped by Android).
Do not restore a fixed 4096-sample pull: Colors submits one 8192-byte mono
16-bit live loop, so full-turn pulls hide progress from its circular mixer.
The regression demonstrates unchanged modulo position for full-turn pulls,
then intermediate positions and updated PCM with 220-sample pulls at 22050 Hz.
This test validates the clock/refresh mechanism, not physical Android SFX.

Android audio allocates at least getMinBufferSize, then calls setBufferSizeInFrames
to limit the effective queue; log the actual frame count and requested/selected
performance mode rather than claiming a guaranteed latency. Request LOW_LATENCY
only on API 26+ (minSdk remains 24), and use audio thread priority. Keep the
10 ms whole-frame pulls that preserve Colors live-loop cursor progress.

Android AudioTrack uses AudioManager's reported output sample rate, falling back
to getNativeOutputSampleRate, then the guest rate. AudioResampler converts PCM16
with a rational source position and retained last frame across JNI pulls;
never reset interpolation phase at each chunk. Preserve guest-format 10 ms
pulls and guest playback cursors; only host PCM is converted. Silence lengths
and AudioTrack buffers use output frames. Tests in tools/check-android-audio-resampler.kt
cover known interpolation, stereo, bypass and whole-vs-chunked conversion/frame
counts for 22050/44100/48000 Hz. Run with kotlinc against AudioResampler.kt.
Device evidence before this change: 22050 Hz, 660 buffer frames, performance=0;
physical low-latency acceptance and perceptual delay remain to be verified.

The Android feeder must re-query nativeAudioFormat throughout playback. FIFA
GZGA200024 launches AUTORUN.EXE through SDCreateProcess; foreground audio taps
can therefore change independently of the Activity/session. Rebuild AudioTrack
and the streaming resampler when the advertised format changes (including 0
while the child is not ready). Flush old queued PCM. Verify format again after
a JNI pull to discard a chunk received across a foreground-format transition.
Do not configure output once from the launcher's first stream and keep using
that format for every child. A compiled host harness executes the actual
startAudio body against fake Android APIs and verifies 44100 stereo -> 22050
mono reconfiguration, pull sizes, output, stale-chunk discard and release.
The supplied FIFA log proves child launch + child 22050 mono open; it omits
the initial AudioTrack format, so do not claim a measured pitch ratio from it.


### CI build prerequisites (October 2026)

CI pins Rust 1.90.0 so formatting and compilation use the same toolchain.
Keep rustfmt checks and -D warnings: without video-static, DirectShow packets
are only consumed, so Packet has a feature-specific dead_code allowance.
The Unicorn CLI check reproduces that warning as an error before the fix.
Linux workspace/desktop jobs must build pinned static FFmpeg before Cargo.
Windows builds FFmpeg with the existing MSVC PowerShell script and the actual
MSYS2 action output path; preserve LF checkout for Bash scripts. Do not use
MinGW libraries with the MSVC Rust target. Android CI uses the existing native
and APK scripts (NDK 28.2.13676358, API/build tools 35, cargo-ndk 3.5.4),
including both ABIs, static video, ELF/APK alignment and signature checks.
The YAML/dependency/shell checks and local core validation do not establish
a successful GitHub-hosted Linux, Windows or Android workflow execution.
Android native script merges global RUSTFLAGS into its target-specific linker
flags and unsets the global variable for Cargo; otherwise CI's -D warnings
overrides the 16 KiB ELF alignment arguments.
Local checks: rustfmt and strict Unicorn CLI check pass; core/CPU/kernel library
tests pass. The actual Kotlin feeder harness still rebuilds output across a
44100 stereo -> 22050 mono transition with the requested buffer now 20 ms.

CI log follow-up: generate RAMTEST/VFSTEST ARM PE fixtures with the checked-in
Python scripts before workspace test or clippy --all-targets compilation;
include_bytes requires them even when no tests run. Install clang/lld for this.
SDK setup must explicitly request platform-tools rather than the removed tools
package. CMAKE_POLICY_VERSION_MINIMUM=3.5 permits bundled SDL's older CMake
minimum on CMake 4 Windows hosts. Keep Cargo cache and artifact paths under
target/cargo, separate from FFmpeg sources under target/native. Clippy remains
advisory (the original job already had continue-on-error); report its lints as
warnings and tolerate unknown version-specific Clippy lint names. Normal build
and test jobs retain global -D warnings. These fixes target the supplied
GitHub log; full hosted workflow validation still requires a fresh run.

Android Unicorn bindgen cross-compilation: host libclang must use the NDK
sysroot, API 24 target and ABI-specific system headers. The native build script
sets BINDGEN_EXTRA_CLANG_ARGS_<underscored-target> separately for arm64-v8a
and armeabi-v7a; this avoids /usr/include/stdint.h and missing Linux multiarch
headers. ARMv7 uses Clang target armv7a-linux-androideabi24 and NDK header
directory arm-linux-androideabi. Quote paths for Bindgen's shlex parser.
These variables are passed only to the individual cargo ndk invocation;
Windows/Linux host builds retain their own headers. A host shell harness
checks both actual script invocations, paths with spaces, warning flags and
16 KiB linker alignment. It does not compile Android binaries or an APK.

### Live output and MAS volume controls

Colors calls waveOutSetVolume with device ID 0 for the output level, and its
original ARM MAS volume wrapper (0xa5978 in the supplied image) sends packed
16-bit left/right levels through MASG_SET_VOLUME (0x001d1028). Both previously
returned success without changing output PCM. Apply gains when rendering, never
when submitting or refreshing buffers: queued PCM and live wave loops must
respond immediately, and muted streams must keep advancing their cursors.

AudioEngine retains the output-device level for existing/future waveOut opens.
A waveOut handle sets only that instance; device ID 0 replaces all instance
levels and controls MAS/legacy/HSS output through the same emulated device.
GetVolume returns the applied packed value; unknown handles and null output
pointers return MMRESULT errors. WAVEOUTCAPS advertises volume/LR volume.
Reset preserves volume while dropping that stream's PCM. The mixer is shared
by CPAL desktop output and Android AudioTap; host system volume is untouched.

MAS volume belongs to MasPlayback, shared with imported aliases and retained
before START or after STOP. Each newly queued MAS stream inherits it, and
SET_VOLUME updates already queued PCM. Keep VFS GET_VOLUME replies synchronized
with the request. Gain changes do not reset phases, queues, callbacks or clocks.
Tests cover live-loop mute/restore, reset/new-open inheritance, independent L/R,
MAS queues/aliases, and actual dispatched WinMM/MAS calls plus PCM output.
The supplied Colors ARM wrapper is replayed separately to verify its IOCTL
encoding. Full game-menu playback and native Windows/Android builds require
the complete game assets and target-device checks.


### Launcher controls, position fill and shared executable information

The desktop binding editor uses three columns (control, keyboard, controller).
Each assignment list has its own bounded horizontal scroll area and unwrapped
labels; capture buttons stay below that list, outside the scroll. Retain stable
IDs per guest button and the existing key/controller capture and removal rules.
A headless egui harness runs the actual editor with 80 keyboard and 30 controller
assignments at 480, 800 and 1200 pixel widths and checks single-line labels.

Android control feedback uses a 30 ms pulse with amplitude 180 when supported;
check the system touch-feedback setting and the view's haptic flag first.
Declare VIBRATE. Older devices use the duration API; missing/failed vibrators
fall back to LONG_PRESS. Keep feedback on new ACTION_DOWN only.

The Fill button beside the manual GPS preferences requests coarse/fine consent
if neither permission is granted. DevicePositionRequest uses the newest valid
cached GPS/network/passive fix (GPS requires fine consent), or subscribes to
active GPS/network providers for at most 20 seconds. Success fills and persists
both coordinates and displays the original fix timestamp; an old cache is not
presented as a newly measured fix. Filling does not enable fixed GPS mode or
change game GPS sessions. Stop/timeout/success remove all listeners; canceled
or invalid/late updates never write coordinates. No Google services dependency.

GameAnalysis and analyze_game now belong to pocket-library::analysis; desktop
and Android use the same binary/resource scan and evidence sections. Keep
VERSIONINFO versions and company distinct from the user-edited library title.
Detected capabilities/language strings are evidence, not runtime guarantees.
Android's Informations menu calls NativeBridge.analyzeGame off the UI thread
and shows identity, executable, graphics, hardware, game data, languages and
technical evidence in a scrollable dialog. The JNI export must ship with the
updated Kotlin app; rebuild native libraries before the APK. Play is English.

Colors rerouting was audited and is already restricted to the case-insensitive
host us.mygiz.gizmondo.com AND /applications/games/colors/open/command.do.
Other hosts/paths and raw socket data are not redirected by this rule. The
existing scoped-routing test passes; no networking behavior was changed.

Validation: 41 launcher-library tests (including metadata serialization,
UTF-16 resource alignment and malformed PE offsets), host JNI type check,
actual-editor egui harness, and compilation of all Android Kotlin sources
against API 35/AndroidX pass. Kotlin compilation uses generated resource IDs,
not a Gradle APK build. A fake-platform harness executes the actual GPS request
and haptic body; physical vibration/location and target builds remain untested.


### CI ARM diagnostic test scheduling

The supplied GitHub cargo-test log ends at android_runner_ramtest_processes_and_orphan_complete:
90 seconds expire, then finish() requests a stop and produces a clean shutdown
summary. That summary is not proof that the diagnostic completed on its own.
ALSA's missing-card warnings are not the failing assertion.

The CI test command now runs the entire workspace in release/locked mode with
--test-threads=1. ARM process diagnostics have short guest wait deadlines and
multiple emulated process trees; avoid running competing host tests on limited
CI vCPUs. Do not skip diagnostics, relax guest assertions or extend the timeout.
On timeout, run_diagnostic captures all Flash Disk text reports BEFORE calling
finish(), so forced exit callbacks cannot overwrite the stalled-stage evidence.

Local evidence: the original exact RAMTEST timeout was not reproduced. Repeated
parallel runner suites under compilation load did reproduce a VFSTEST second-run
fixture-directory failure. All eight Android runner tests pass sequentially in
dev and release, using freshly generated Clang 18 ARM fixtures (RAMTEST: 132
checks; VFSTEST: 103 checks and repeat cleanup). Local release validation omits
CPAL/static video; the complete hosted workspace job still requires a new run.
The regenerated fixture files also enable all 167 kernel and 255 WinCE API
library tests to pass sequentially, including the two loader tests that were
previously unavailable without pageprobe.dll/depleaf.dll.


### Android continuous touch D-pad

GizmondoControls uses one 140dp TouchDpad target instead of four independent
arrow Buttons. DpadDirections maps eight equal angular sectors to the four
existing guest directional keys: diagonals hold two contacts, never opposite
contacts. The default centre has a 45%-radius neutral zone (31.5dp at 140dp size); coordinates outside the
view release directions. Sliding sends only changed key edges, with no repeat
or per-frame polling. The first pointer owns the pad until release/cancel;
additional fingers cannot take over. FrameLayout retains split multitouch so
action buttons work while the thumb remains on the pad.

GameActivity routes pad edges through existing guest key reference counts,
preserving simultaneous keyboard/controller input. Sector changes release
old contacts before acquiring new ones, and request one existing haptic pulse
when a new contact appears. Pause/teardown clear ownership and pressed sectors;
no timers, workers or native resources are allocated by the D-pad.

Validation: actual D-pad/control Kotlin sources compile against Android14 API
classes. A host sweep checks 19600 positions, cardinal/diagonal/dead-zone and
outside behaviour. Harnesses execute the actual touch handler and Activity
callback with fake platform/input sinks for sliding, holds, up/cancel/pause,
second-finger ownership and shared hardware key references. These checks do
not constitute an APK build or a physical Android gameplay test.

The validated Android layout anchors the pad 8dp above the lower edge and the
action diamond 10dp above it. Shoulder buttons are 124dp wide by 132dp tall,
8dp from the top (three times a standard 44dp button height). The central top
toolbar includes five function buttons with 8dp spacing, then a 28dp gap and
a distinct orange 64dp Exit. Centre the entire 344dp toolbar so hardware
functions shift left; scale only that toolbar to fit narrower landscape space.
Actual measurement/layout bodies pass host checks across four landscape sizes
and three densities for padding, shoulder sizes, ordering and no overlaps.

The D-pad neutral disc is drawn with the same deadZoneRadius property as hit
mapping (defaulting to DEAD_ZONE_RADIUS). Its radius grew from 11.2dp to 31.5dp following device feedback about
accidental contacts near the centre. Keep the complete square touch target,
including areas between the drawn arms, available for directional/diagonal
activation outside that disc. No additional debounce, delay or repeat is added.

### Per-game D-pad tuning, SDL vibration and Gizmondo backlight

GameSettings.dpad_deadzone is shared JSON (`small`, `medium`, `large`), defaults
large and survives per-game settings edits on both frontends. Android touch
neutral radii are 0.16/0.305/0.45; rendering and hit mapping use the same radius.
Directional stick press/release thresholds are respectively 0.35/0.20,
0.60/0.35 and 0.75/0.50. Hysteresis prevents edge chatter. Digital D-pad contacts
remain direct; trigger thresholds remain unchanged. Dead-zone tuning cannot
create an analogue centre for a controller whose D-pad only exposes digital
contacts.

Desktop installs an SDL vibration backend into the existing Gizmondo vibration
service. Only the SDL owner thread touches native controller handles. A bounded
command queue (32) and motor table (64) service SDK waveform requests; fixed
half-scale motor strength replaces hardware amplitude tuning. Native pulses
expire after 100ms and renew every 40ms while requested. Selection changes,
loss of focus, game exit and worker shutdown stop output. Motor drop removes its
waveform, and monitor drop joins its worker. Unsupported hardware returns 50;
disconnected/unavailable backends return 21 through existing LastError handling.
Closing a guest handle alone preserves SDK-active vibration; service-off and
last shared service destruction release the native motor.

Colors' brightness slider uses SDK OEMINC.H, HKCU ControlPanel\Backlight,
OnLevel/DimLevel and BackLightChangeEvent (EventModify SET). Its ARM setter at
0x135e0 writes OnLevel 1/11/21/31 and DimLevel 0/0/10/20; getter is 0x136cc.
VK_BACKLIGHT is F3. Do not intercept that key with a host brightness cycle: the
game owns its menu. Registry defaults are Level/OnLevel 11, DimLevel 0 (the SDK
hardware default was 24). Runners
synchronise OnLevel during existing frontend polling. Presentation maps RGB
channels with level 11 reproducing the unmodified image and level 1 blending
one fifth toward black (20% dimming). The visual ceiling at slider 31 now uses
the previous slider-21 appearance (one twelfth white blend). The new slider 21
blends one twenty-fourth toward white, halfway from neutral to slider 31.
The four positions display channel 128 as 103/128/133/138, with levels 1 and 2
unchanged. Levels above neutral 11 are scaled across the entire upper range.
Missing/wrong-type values, 0 and out-of-range DWORDs fall back to 11 on every
frontend sync, including after a previous dim setting. Device snapshot loading
also repairs missing/invalid Level and OnLevel and missing DimLevel before
Colors reads its initial slider position; valid saved levels are preserved. The bounded blends
preserve contrast without clipping to a uniform black/white image; native
RGB565 surfaces remain unchanged.
A changed level increments the presentation counter even on static menus.
Screenshots and upscalers therefore receive the same adjusted image. Shared
registry values survive child/parent transitions. Host display brightness and
platform-wide power management are not modified.

Validation for this change: 174 kernel, 259 WinCE API and 42 library tests pass
without default features; seven controller tests compile the actual desktop
SDL/rumble sources and pass, including an SDL virtual controller short press and
100 motor open/apply/drop cycles. Replay of the supplied Colors ARM setter and
getter confirms all four slider values and EVENT_SET. The complete Android
Kotlin sources compile against Android14 framework/AndroidX classes (generated
resource identifiers, not an APK build); JVM checks cover 58,800 touch positions,
stick hysteresis and per-game JSON preservation. Physical Switch Pro vibration,
Windows release builds and an Android APK/device test remain target validation.
Desktop cargo check also passes on Linux without default features. The local
SDL native check disables unused X11/Wayland/DBus support; the emulator's window
and presentation remain owned by egui, not SDL. No repository build flags or
platform dependencies were changed for this environment-only verification.

### SDK date/time, shell status, accelerators and DirectShow completion

The October 11 SDK audit compared all eleven example projects with registered
handlers. GetDateFormatW/A and GetTimeFormatW/A now format validated SYSTEMTIME
values, including NULL/current-time inputs, NUL-inclusive size queries and
untouched buffers on insufficient capacity. Supported locales are virtual
English-US/default and French-France. Invalid locales/parameters return 87,
unsupported flags 1004, short output buffers 122; success preserves LastError.
Custom quoted formats and the SDK's DATE_LONGDATE/time flags are supported.
Alternate calendars and suppression flags with a custom time pattern are
explicitly rejected, not silently accepted. The virtual local clock remains
UTC. GetSystemTime/GetLocalTime now report the actual civil date instead of the
previous fixed January 1 date.

GT_GETSTATUS broadcasts queue GT_STATUS with GPS option (bit 1), shared Bluetooth
power (bit 3), and the requested/available host HTTP data gate (bit 4). Android airplane/radio/SIM snapshots and Windows simulated status are described
in the Host device status section below. ActiveSync remains disconnected. Subsequent message-pump/wait
calls publish status transitions after a status request, without repeating
unchanged states. Both SendMessageW and PostMessageW handle GPRS_CONNECT;
transport availability is not proof of a reachable game server. Shell state
and interrupted accelerator callbacks are process-owned and cleared at exit.

LoadAcceleratorsW/A returns the address of a real RT_ACCELERATOR resource;
there is no table allocation or fake universal handle. TranslateAcceleratorW/A
matches virtual-key/character messages and exact Shift/Ctrl/Alt modifiers,
then synchronously dispatches WM_COMMAND with notification code 1. Its callback
round-trip restores the original LR and returns TRUE independently of the
window procedure's return. Frames are bounded at 64, keyed by thread/thunk/SP,
and retired with thread stacks. Loaded tables disappear with their module.
Create/Copy/DestroyAcceleratorTable are outside this resource-only SDK scope.

DirectShow exposes IMediaSeeking (its own IID tail), IMediaPosition and
IBasicVideo, sharing the graph's COM identity/refcount. MEDIA_TIME is the
supported seeking format (100 ns units); position interface values are seconds.
Windows CE ARM doubles use aligned R2/R3; ConvertTimeFormat's 64-bit source is
on the stack. The native single-owner FFmpeg wrapper supplies actual duration,
native dimensions, frame interval and bitrate and supports av_seek_frame plus
codec flushing. Seeking restarts a bounded producer and stops the old audio;
audio is trimmed from decoded PCM to the requested interval. Rate 1 is supported;
other rates, segments, no-flush seeking and other time formats return errors.
The full IDispatch automation layer and caption BSTRs remain unsupported.

Video-window position/visibility and BasicVideo source/destination rectangles
control real RGB565 composition, clipped to the emulated video window. Default
playback preserves aspect ratio and bilinear scaling. Unchanged frames are not
recomposited on every service poll or published as new frames. GetCurrentImage
returns a bottom-up 24-bit DIB with a size query and pointer/capacity validation.
Audio-only RenderFile works for the SDK's RingTone example. Pause/resume keeps
the clock; completion preserves the final position; Stop/Run can reopen a source.

Last Release frees every guest interface and vtable. Movie Drop/Stop/replacement
cancel the producer and close its receiver, releasing FFmpeg contexts on their
owning worker, including a blocked channel send. Native dimensions are bounded
at 8192 per axis; the PCM accumulation limit is 128 MiB; the video channel holds
at most three packets. Decoder work observes cancellation between packets;
there is no guest-memory access from producer threads. The synthetic WMV test
fixture contains generated testsrc pixels and a sine wave, not proprietary data.

Validation: all WinCE API tests with the actual pinned static FFmpeg and all
kernel tests pass with Rust warnings denied. Tests cover real WMV metadata,
seeking with bounded audio/video, five repeated native decodes, 100 COM graph
creation/query/release cycles, DIB contents, date/time errors, accelerator
callback return and shell transitions. Regenerate original RAMTEST DLL fixtures
before testing a fresh clone; the delivered patch includes the four fixtures
needed by the loader unit tests. Host JNI type checking passes. These checks
are not a native Windows release build or an Android APK/device validation.

### Host device status and launcher DirectShow preview (2026-10-11)
Android reads airplane/SIM/service state through application-context JNI snapshots cached one second; denial means unknown, no subscriptions/IDs. GT_STATUS carries airplane/radio only (no SIM bit). Windows defaults SIM ready/radio off/airplane off; ActiveSync stays disconnected. GSM/TAPI/RIL stack remains out of scope. GLES marks the replaced framebuffer dirty before DirectShow service so cached preview frames are recomposed after menu drawing. Regression rectangle matches attached launcher (204,40,101,82).

Launcher startup preview also requires WinCE EC_VIDEOFRAMEREADY (0x49), as confirmed in SDK Movie and attached ARM binary (0x15934 comparison). Emit once on first decoded frame, even offscreen, reset per RenderFile, GetEvent consumes before completion without consuming WaitForCompletion. No event-owned memory; notification disabled flags still allow polling. Launcher initially parks window at (844,40,101,82) and moves it to (204,40,101,82) only upon readiness.

### Native GLES implementation reference

The renderer architecture, memory/resource ownership, fallback contracts,
optimizations and presentation timing are maintained in [GPU.md](GPU.md).
Read that document before modifying pocket-gles or GLES presentation handlers.

### Android FPS overlay
Existing config.show_fps now controls an English Show FPS preference under
Emulator Settings and a small noninteractive TextView in the game's top-right
area. The renderer counts each newly drawn FrameSnapshot identity once on its
GL thread, publishing a volatile total; repaint, rotation, screenshot and GL
context recreation of the same snapshot do not increment it. This reports new
frames drawn, not guest loop iterations or the physical compositor refresh.
When disabled, counting and UI updates are skipped. The existing Activity poll
updates the label at most once per second; no extra timer, thread or JNI call.
Sample wall time is monotonic and resets at resume to exclude background time.
Zero-frame startup stays hidden, static output becomes 0 FPS, pause and exit hide
the overlay. It is not drawn into the emulated framebuffer or screenshots.

Pure Kotlin checks cover frame identity, elapsed-time normalization, idle output,
pause and counter resets. The actual renderer compiles against Android framework
classes with only project neighbours stubbed; full APK/device remains validation.
Display processing retains level-1 brightness's 20% black
blend, with the second slider position neutral/default and the upper levels intact.


## Suspended-process idle gate fairness (2026-10-11)

Android RAMTEST's intermittent CI timeout stops after remote_suspend_counts.
All processes share the handle domain's API gate. schedule_idle previously slept
1 ms inside dispatch while owning that gate, letting a suspended child repeatedly
reacquire it and starve its parent's pending API calls. Move idle sleep to the
kernel after releasing the gate, before frontend polling. Keep idle cadence,
Sleep durations, suspension counts and control application unchanged. No timeout
increase or skipped test. Native ARM runner reproduction requires Unicorn and
libclang; local build lacks libclang, so full diagnostic confirmation is pending.

## Frontend feature updates (2026-10-12)

Simulated GPS offers manual coordinates and one random uniform-area spherical fix within 1 km of the Eiffel Tower per launch. gps_fixed_random persists across Windows/Android settings. The same shared provider setup works on Linux. Manual coordinates remain stored and native permission bypass/lifecycle are unchanged.

Game hardware information detects Bluetooth, Internet, GPS, camera, vibration, power, SIM, battery and backlight using device strings, named imports and the existing CE4.2 coredll ordinal map for Gizmondo. No static evidence is not proof of absence. Scene camera symbols are not camera-device evidence.

Valid BMP labels beside a Gizmondo executable are converted to cartridge.png for desktop/Android cartridge artwork only; their filenames never supply game titles. Input size and dimensions are bounded; invalid BMPs retain the existing fallback. Demo launchers remain Demo Card. No bitmap or copyrighted game payload is bundled.

Demo import prompts Keep launcher / Separate games. Each separated entry owns a game directory, settings and save namespace, retains original card-relative paths and shared resources, and excludes other games directory trees. Native splits run off the Android/desktop UI thread. Source launcher removal follows successful title creation. Split entries retain Gizmondo platform identity even without a GZGA marker.

Android Pocket PC sessions restore the upstream eea7ac6 activity layout/control subtree and styles, including Turbo/F3. Portrait reserves the measured controls height; landscape overlays controls on the full-height game area. The original fullscreen toggle and modes are retained; Rotate and Exit remain accessible while the toolbar is hidden. Display turns persist in per-game settings and effective framebuffer dimensions determine locked portrait/landscape, independently of phone rotation. Pointer and directional input follow inverse presentation rotation, releasing held input before turns. COD2 sideways portrait framebuffers default to CW90 on import; existing imports can use Rotate. Guest resolution and GL matrices remain unchanged.

Linux and Windows share the desktop frontend, settings, layouts, filters, controller handling and imports. Simulated GPS is available on Linux. Native camera/Bluetooth/location/Internet implementations are platform-specific; matching UI does not imply Linux hardware bridge parity. See DESKTOP-PARITY.md.

Compatibility tables reflect the maintainer-reported OK status for the maintained collection and retail catalogue names. They are not new independent gameplay runs or proof of all modes. Original proof directories remain intact.

About/credits expose https://github.com/guizmox/PocketHLE on Windows/Linux and Android, while retaining original project attribution.

Selected demo-card title IDs prefer the executable's GZxxNNNNNN ancestor directory. Shared launcher markers cannot supply a split game's ID. Desktop cartridge labels, bottom captions and shared game information use the same helper. Existing imports need no migration or reimport.

Static game names come from the maintainer's Liste_Jeux.xlsx IDs/titles, with the explicit 200030 Motocross / 200041 Furious Phil correction. Known static titles supply names at import; unknown codes retain the regular import fallback, independently of BMP filenames. BMP artwork is retained. Existing ID-shaped names migrate on library load, preserving custom names, IDs, settings and save paths. No release/build version is inferred from a shared title ID. Unknown codes use the previous fallback. The initially unclear GZGA22222 source row was later corrected by the maintainer to GZGA200069 (Catapult).

## Desktop PocketPC presentation restoration (2026-10-12)

PocketPC uses the original upstream side-panel presentation (eea7ac6): an
unframed framebuffer beside a 3x3 cardinal D-pad, Action/A/B, C/1/2 and Turbo.
Gizmondo remains on its existing photographed skin. The current shared menu
provides rotation/fullscreen/game options, with filters, screenshot capture,
controller bindings and integer screen scaling retained. Stylus coordinates
continue through the current inverse-rotation mapping. Pointer-held keys stay
separate from keyboard/controller sources, and clipping excludes hidden buttons.
Window fitting accounts for the side panel rather than PocketPC housing.
This desktop path is shared by Windows/Linux; Android is unchanged.
Validation: restored UI methods typechecked against actual egui and pocket-library
with warnings denied; original pad layout/button inventory compared to upstream;
Gizmondo view verified byte-identical to lot08 R3. Complete Windows compilation
and interactive UI validation still require the target environment.

The obsolete desktop pocketpc_layout module/source is removed. Side-pad rotation tests replace shell geometry checks.

## Documentation organization

GPU implementation details live in [GPU.md](GPU.md); the user-facing Gizmondo
guide is [GIZMONDO.md](GIZMONDO.md). [README.md](README.md) indexes documentation.
Release ZIP notices/manifests are transport metadata, not project documentation.
Keep tool-local diagnostic READMEs and proof assets with their sources.

## Android CAM1 orientation

Gizmondo camera packets rotate their Y/U/V planes using Camera2 sensor orientation,
lens facing and current display rotation before preview or tracking consumes them.
The existing UI poll catches reverse-landscape 180-degree changes even when Android
does not issue onConfigurationChanged. Each packet snapshots one rotation for all
planes; mailbox clearing and stale-publication checks prevent old orientation reuse.
No mirror is introduced and the existing CAM1 row convention is retained.
Non-Gizmondo sessions retain raw camera orientation. No Activity is stored in
CameraHost, no new listener/thread/timer or camera reopen is needed for a turn.

Orientation reference: https://developer.android.com/media/camera/camera2/camera-preview

## Diagnostic lifecycle and timing coverage (2026-10-12)

RAMTEST v8 retains all previous suites, adds 64 mixed resource cycles and eight
process launch/exit/reclaim cycles (136 checks). VFSTEST v3 adds 64 mixed file
cycles and exact refunds plus a versioned 12-byte run marker (108 checks).
The marker is retained outside its temporary probe directory; a malformed marker
fails without replacement. A restart between runs verifies persistence.
TIMETEST v1 measures Sleep/waits/timer callbacks/audio callbacks with nested Sleep,
40 waveOut lifetimes and EGL identical-frame/late-resume pacing (44 checks).
Guest clock quantization has explicit lower tolerances; host slowness is measured
and warned, not treated as proof of a timing-contract violation.

CAM/GPS/NET/VIB/BT diagnostics add repeated lifetimes while retaining original
checks. BTTEST sources/build are recovered from its original shipped diagnostic,
retaining bounded Unicode/MAC inquiry output. Kernel-only camera/network tests
count native-provider destruction/cancellation over 100 repeated mock cycles.
The new ARM camera cycle caught DeviceIoControl retaining an old LastError
after a handle was closed. It now rejects unknown/closed VFS device handles
with ERROR_INVALID_HANDLE before reading unused buffers. A focused unit and
actual ARM cycle cover this correction. No game tracing or resource monitor is
introduced. These checks do not prove no physical driver/JIT/OS allocation leak.

CI regenerates TIMETEST beside RAM/VFS fixtures before integration tests. The
contract runner is documented in tools/gizmondo-diagnostics/README.md; default
ignored native GPU tests remain explicitly unexecuted unless requested.

The hardware guest integration suite executes CAM/GPS/NET/VIB/BT ARM binaries
with controlled providers. Camera/location/motor/session/transfer live counters
must return to zero; Bluetooth exercises startup and inquiry cancellation. This
checks guest ABI and ownership, not physical sensors, radio exchange or TLS.
Guest suites run sequentially to avoid competing emulation deadlines. DirectShow
unit fixtures share a test-only gate around their process-global graph registry
so parallel tests cannot reset one another's graphs. Production graph behavior
is unchanged by this test gate.


## Global Colors language preference

Desktop and Android Gizmondo options expose English UI for colors_language
(english/french/german/italian/spanish), default English for existing configs.
Colors preferences are grouped at the bottom of Gizmondo options.
Eligibility and launch activation use game_gizmondo_title_id == GZGA200015 or GZGA270015,
never a display-name match. At the next launch, Colors alone redirects read-side
bstrings.eng resolution to a case-insensitive sibling bstrings.fre/ger/ita/spa
(fra/deu/esp aliases accepted). Missing or ambiguous translations retain English.
No executable or installed resource is rewritten. Write/create destinations stay
on the original exact path. The alias is process-local, not part of VfsShared,
so another game sharing device services does not inherit it. Translation files
must already exist beside bstrings.eng; this does not supply translations or
rewrite entries inside data.zcd. Original game archives are required for in-game
validation and any version-specific missing string keys.

The static title table includes the 26 previously missing GZGA200 IDs extracted
from the supplied MultiBoot launcher. Existing maintainer titles and IDs stay
unchanged, including exact GZGA270 and GZGA280 entries; no prefix substitution
is inferred. Both known Colors IDs enable the global text-language preference.

User-requested local alternative-version catalogue IDs are GZGA900056 for
Agaju (E3 demo) and GZGA900069 for Catapult (august 2005). These are custom
launcher labels, not inferred official title IDs or patches to game binaries.

The maintainer confirms GZGA200069 as Catapult. That exact ID is registered;
the ambiguous GZGA22222 entry is removed rather than guessed or retained as an alias.

Game identity no longer falls back to GZGA-looking strings inside executables.
Analysis uses the selected executable/card directory marker via the shared title
ID helper. Binary metadata/hardware inspection remains available, but neither
binary content nor BMP filenames supply a game title. Existing user-edited
display names are retained; unknown IDs keep the ordinary import fallback.


## Wrapped MultiBoot card mounts

Desktop/Android initial sessions use GameEntry::gizmondo_card_root: for a selected
non-split autorun.exe with sibling multiboot.hog inside the extraction tree, the
launcher's own directory is the SD/Storage Card root. Rooted resource reads and
root-level GZGA enumeration therefore see the real card even inside a ZIP wrapper.
Children inherit that card root through the existing launch context. Split game
entries retain their original extraction-root mapping. This fixes the reproduced
localisation/english.loc NULL-file crash at 0x21db8 (LR 0x175c4) with the supplied
launcher in a "14 - Complete Set of Games" wrapper, without relaxing VFS path
isolation or adding a guest NULL-handle workaround.

Catalogue additions: local GZGA800056 = Agaju (august 2005), GZGA900016 = Chicane
(alternative), GZGA900015 = Colors (early demo); maintainer GZGA200098 = Alien Hominid.

Local GZGA900013 labels the user-supplied Richard Burns Rally (april 2005) variant.

Local GZGA900049 labels the user-supplied Hit & Myth (may 2005) variant.


## GZGA collection imports and library UI exclusion

Library::import_zip_entries returns all imported entries. ZIP-root supported
`autorun.exe` plus sibling `multiboot.hog` takes priority as a demo card. Otherwise
all GZGA###### executable directories are grouped by the executable's nearest
GZGA ancestor, including one-game and mixed/wrapped archives. Non-GZGA executables
do not prevent the collection import and are not copied into each game. Nested
launcher/HOG pairs do not identify the outer ZIP as a demo card. Data-only GZGA
folders without a guest executable retain their resource role in ordinary imports.
import_zip remains a compatible single-reference API but imports the entire
collection and returns its first entry. Collection entries carry source_cab
prefix gzga-collection: so they remain Gizmondo even without marker files.
The common title helper resolves their selected executable's own GZGA ancestor.

Desktop import completion carries Vec<GameEntry>; bulk imports skip the single
rename prompt. library_busy gates all UI/input before drawing interactive
controls during the worker operation and unlocks on success/error/cancellation.
The demo-card choice also gates underlying UI. Android importZip returns an
array for a collection and the existing object for ordinary imports/demo cards;
MainActivity handles both. A noncancelable busy dialog covers import/copy and
split, dismissed in coroutine finally blocks. No launcher binary or emulation
rendering change is involved.

The desktop VFSTEST runner cleanup assertion now permits exactly VFSTEST.TXT
and PocketHLE-VFSTEST-PERSIST-v3.bin, verifies the 108-check PASS summary and
validates the marker's 12 bytes (magic 0x56544633, version 1, first-run count 1).
The marker is intentional persistent state, not a leaked temporary probe file.

GZGA collection imports copy only each runnable GZGA directory, ignoring all
external files/directories. Shared resources apply only to demo-card splits.
Flattening must preserve the SD volume identity: OpenVolume::declared_serial
decodes a four-byte direct GZGA marker as well as GZGA/GZGA markers. Toy Golf
terminates before rendering when the reported volume serial does not match.

Desktop and JNI mount each install_dirs prefix through GameEntry::install_mount_dir.
For SD Card / Storage Card serial aliases, an existing physical GZGA directory
wins over the flattened extraction root, case-insensitively. This preserves
Supernaturals nested serial marker while direct-file Toy Golf imports retain
root aliases. Do not blindly remount serial prefixes at extracted.

The five native ARM integration suites in pocket-core/tests/ram_guest.rs share
a test-only suite mutex. They have real wall-clock guest wait deadlines and
launch their own concurrent emulators, so simultaneous whole suites overload
small CI runners. Intra-suite process/thread concurrency and guest deadlines
are unchanged. Child process reports are printed before the parent assertion.

DirectShow Run/Pause state transitions honor IVideoWindow AutoShow (slot 13,
default true). Colors calls put_Visible(false) while preparing its intro and
expects Run to reveal it. Idempotent Run/Pause must not re-show a window hidden
during playback. AutoShow false is honored. Official contract:
https://learn.microsoft.com/en-us/previous-versions/ms786976(v=vs.85)

Catalogue additions (2026-10-13): local GZGA800015 = Colors (E3 demo),
GZGA700016 = Chicane (early demo), GZGA900032 = Conflict Vietnam (Interim Demo).
The maintainer requests GZGA200097 = Unfinished Business (Gametrac Demo);
that exact identifier was unallocated in the table. All previous mappings are
preserved. These are title catalogue entries, not executable/card-serial patches.

GUI imports use Library::prepare_import followed by apply_prepared_import.
Preparation runs the existing importers once in an isolated temporary library,
including nested-container imports; Installed IDs must be confirmed before
publication. PreparedImport owns the temporary tree and drops it on cancellation.
Both desktop and Android keep library interaction disabled through confirmation.
The JNI bridge stores owned preparations behind opaque, library-bound tokens,
which the Activity cancels on destruction. Updates retain current per-game saves,
shared Flash Disk contents, custom titles and settings using the existing atomic
publication/rollback mechanism. Never bypass the newly-installed-ID confirmation
check when applying a prepared import. Demo-card updates do not trigger split.

Catalogue date corrections migrate only the exact former automatic labels:
GZGA900069 Catapult (august 2005), GZGA800056 Agaju (august 2005).

The provided Supernaturals demo's camera far plane (4.5) clips its +/-4 sky cube.
The guarded tools/patch-supernaturals-skybox.py changes only the instruction at
VA 0x23920/file 0x12D20 to far=16, for one SHA256-identified executable. The camera
is used by Journal and may be shared with gameplay. No rendering exception is
introduced: proper six-plane clipping from 64276c2f remains intact. See
docs/GIZMONDO.md for reproduction scope and the patch's exact binary identity.

SDK alternative-title path aliases (2026-10-13): both frontends mount
GameEntry::install_mounts, retaining declared aliases and adding exact original
GZGA marker filenames/directories found at extraction root or beside the EXE.
Physical card directories map to themselves. Marker files map to their parent.
Do not infer these paths from display names or remap all serials at extracted:
Colors E3 needs its original GZGA200015 alias alongside catalogue GZGA800015,
while Supernaturals needs its actual nested directory. This is launch-time only
and works for previously imported variants without changing game IDs or saves.

zlib.dll HLE uses process-owned kernel::zlib streams and the explicit WinCE
56-byte z_stream ABI; Unfinished Business's attached DLL confirms ordinals
2/19/20/22 (compress/inflate/inflateEnd/inflateInit_). Counts/pointers/totals
must reflect partial progress, including buffer/error results; state cleanup is
inflateEnd/process teardown. This subset does not emulate every zlib export.

SDL.dll named-import HLE is an initial SDL 1.2 video/timer/input subset.
Its guest structures and heap surfaces are process-owned kernel::sdl state;
32-bit SDL_Surface is 60 bytes and SDL_PixelFormat 40 bytes. Real guest ARM
Init/SetVideoMode/FillRect/Flip was checked against the exact framebuffer image.
Unsupported audio/GL/joystick/palette/RWops and SDL2/ordinal-only imports are not
silently advertised as supported. See docs/GIZMONDO.md for limits. This module
is unrelated to bundled host SDL2 controller support. Timer-test rearm intervals
are long to avoid wall-clock failures under CI descheduling; due/coalescing is
set explicitly by the test and production timer behavior is unchanged.

Corrective source ZIPs include patch-files.txt (one modified/added relative path
per line) and patch-remove-files.txt when needed. Preserve the maintainer's
PowerShell timestamp command; do not substitute a different delivery workflow.

Android Colors E3 protected-read diagnostic: the supplied log faults in the
native strcmp thunk at 0x70000300, caller 0x9509c, with R0=0x37316b and protected
page 0x373000. Local runs with native Unicorn 2.1.4/2.1.5 reach the E3 menu;
the user reproduced the conflict on Windows and confirmed that another Colors
version had written the shared Flash Disk config. The SDK parser's signed key
hash becomes a negative bucket index while loading that config before the menu.
Local alternative numeric IDs GZGAx00xxx (x != 2) mount Flash Disk from
GameEntry::gizmondo_flash_dir: games/<entry.id>/saves/flash, in both GUI runners.
Standard titles retain library/flash. Identify variants by numeric ID or
declared SD/Storage Card install alias, never by mutable display names or the
original SDK marker. Do not copy incompatible shared data into a new ALT area.
Old shared files remain intact; normal prepared reimports preserve the ALT
Flash Disk recursively with the rest of saves. Child processes use the same
entry identity. No game, division helper or memory-protection patch is needed.
CPU faults also report R0/R1 heap or
virtual-reservation ownership and committed-page status without reading those
buffers. VirtualAlloc/VirtualFree emit debug-level allocation/range records.
Android Settings / Log verbosity=3 enables API trace; the default info level
cannot show these records. This diagnostic does not change memory protections,
allocation behavior, renderer selection or any game executable.


Local ALT card aliases and Flash isolation share original_card_id's generic
numeric rule: exact GZGAx00xxx -> GZGA200xxx for x != 2. Unknown catalogue titles
still work; malformed/non-numeric IDs and official 270/280 numbers do not match.
GameEntry::install_mounts adds the original SD/Storage Card prefixes from entry
ID and declared exact card aliases after real marker discovery, never replacing
an existing original mount. Canonical assets do not mean shared saves: retain
ALT entry.id for games/<id>/saves/flash. Hit & Myth's provided repack nested
GZGA200049 marker inside a renamed GZGA900049 directory, so original root marker
scanning alone had missed the alias. Local native software/GPU both reach its
language screen after a lengthy boot; no frozen guest or binary repair shown.
