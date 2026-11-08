# Detailed implementation notes and historical evidence

These retained notes preserve ABI details, game reproductions, workload numbers
and validation limits from earlier deliveries. They are **not a current installation
recipe or a list of active settings**. Start with [README.md](README.md) and the
current domain guide; consult the relevant note when changing that subsystem.
Current policy wins over an older snapshot. Dates and reported test counts keep
their original scope. Do not treat a local test, a past maintainer report or an
old pending task as current all-platform certification.

## Source groups

- [Architecture and implementation notes](#history-agents)
- [Graphics implementation and experiments](#history-gpu)
- [CPU profiling cleanup](#history-cpu)
- [Blocking message-pump measurements](#history-message-pump)
- [Diagnostic/resolution delivery](#history-diagnostics)
- [Old internal-resolution delivery](#history-resolution)
- [Old desktop-panel delivery](#history-desktop-controls)
- [CERF design comparison](#history-cerf)
- [Navigator build 27](#history-navigator)

The old WGL context cache/rebinding experiment and configurable anisotropy below
are superseded/removed. The old synchronous-readback-only description is also
superseded by shared/deferred presentation. The standalone diagnostic paths are
historical; use HARDWARE DIAG for current public diagnostics. Earlier release
numbers and patch-application instructions describe their original deliveries.

<a id="history-agents"></a>
## Architecture and implementation notes

These are the sections moved out of the architecture entry point. Numbered core
contracts still retained there are not copied here, except the original invariant
snapshot needed to explain corrected presentation/FPS terminology.

#### Deferred GAPI memory and GPU presentation (2026-11-04)

See docs/GPU.md. A mapped pointer does not imply pixels have been accessed.
ARM virtual-TLB Unicorn may retain the newest EGL image and populate mapped
GAPI bytes only on a guest data access or HLE memory access. Do not bypass the
checked memory helpers, let polling trigger readback, execute pending data, or
replay an older deferred image after GDI has changed CPU authority. The kernel
mirrors such CPU mutations before guest resume. Other backends/layouts retain
the eager fallback. Existing slot fences, epochs and guest frame accounting stay.

### PocketHLE — architecture reference for coding agents


#### Modal progress dialogs and guest scheduling (2026-10-21)

An idle DialogBoxIndirectParamW loop is a guest scheduling point, just like
GetMessageW. It blocks its caller, not the whole process. Use the existing
bounded wait_display_until continuation to park a worker at its exact thunk,
or run a ready worker when the modal caller is main. Preserve the callback
registers on suspension and the original GuestCallFrame until EndDialog.
Assign the modal panel and its child controls to the creating guest thread;
another thread's message pump must not consume their posted paints. Remove
the modal polling deadline when restoring the original caller.

Quake 3's worker shows "Partitioning Memory..." while main waits for its
initialization and then allocates its pools. A host sleep in the modal handler
never yielded the guest CPU, so main never reached those allocations. With
cooperative modal waiting main proceeds and reports the separate, still
unimplemented CreateFileForMappingW path. Do not describe this scheduling
fix as file-mapping support, or change Quake's config/RAM to hide that gap.


#### CAB import recovery, wide scanf and monitor geometry (2026-10-20)

Users import the original CAB unchanged. pocket-cab::extract_for_install first
uses strict extraction, then recovers only one stale nonzero CFDATA checksum
in an uncompressed, non-spanned, non-reserved, structurally valid CAB <=128 MiB.
Recovery changes four checksum metadata bytes in an owned memory copy, never
payload or the user's file. Multiple errors, truncation, compressed CAB errors
and other parse errors remain failures. extract_all/extract_with_header stay
strict for diagnostics. Desktop/Android library import and CLI CAB install share
the installation API. Do not deliver a replacement Snails CAB or require a
Python repair step. This is not cryptographic proof of the payload's provenance.

swscanf uses wide string/character destinations by default, with narrow h and
explicit wide l modifiers; sscanf/fscanf keep their byte/stream semantics.
NFS parses its INI keywords into WCHAR strings: old narrow %s stores prevented
all keyword matches. Preserve assignment suppression, width, numeric sizes and
UTF-16 output termination; %c has no terminator. Wide text field widths and %n
use UTF-16 units for supported text conversions; do not claim full locale/IME
or arbitrary Unicode scanset support from this byte-oriented scanner.

MonitorFromWindow/GetMonitorInfo model one primary guest display, using the
actual framebuffer dimensions and bounded MONITORINFO/MONITORINFOEXW output.
PowerVR Racer ignores the GetMonitorInfo return value, reading rcMonitor.right
and bottom directly at 0x127f4/0x12804. Missing handlers left dimensions
uninitialized and caused later invalid-memory cleanup. Monitor geometry is
independent of host presentation rotation and renderer backend.


#### CAB registry setup, GAPI dialogs and dialog text (2026-10-20)

Desktop and Android call Emulator::complete_cab_registry_setup before the first
application instruction, only for CAB-sourced Pocket PC imports. A bounded,
isolated ARM helper executes the package's original .999 DLL attach,
Install_Init, Install_Exit (all seven ABI arguments), and detach. The supported
profile is registry/time APIs only; do not execute file/GUI/service installers
partially. Existing IonFx resource extraction remains separate. Registry values
commit atomically as a delta after successful completion, respecting object-store
quota and conflicting live writes. The success marker (DLL SHA256 + install
path) is in the same persistent device registry. Never invent Racing Days' Data
value or reset an existing installation/evaluation record. Original code creates
its own initial state. Preserve thread-local media state around the helper.

A Pocket PC GXOpenDisplay window owns its directly drawn pixels. When that
window is an HLE dialog, its default panel face/native backing must not erase
its GAPI frame. Keep its child controls and later dialogs composited. Close or
destroy releases this ownership. Gizmondo's established presentation is unchanged.
Snails uses precisely this modeless-dialog rendering path.

GXGetDefaultKeys receives the ARM struct-result pointer in r0 and iOptions in
r1. GX_NORMALKEYS=2 and GX_LANDSCAPEKEYS=3 are enum values. Pocket PC landscape
tables map logical Up/Down/Left/Right to physical Left/Right/Down/Up, preserving
face keys and 0x60-byte layout. Desktop/Android already inverse-rotate host
input at presentation; never apply a second frontend rotation. The legacy
Gizmondo key table stays unchanged. GeoRally EX explicitly queries option 3.

GetDlgItemTextW resolves a child within the specified parent and sends
WM_GETTEXT using the existing message route (including native subclasses).
Lengths are UTF-16 units excluding the terminating zero. NFS's supplied
registration code passes its unmodified ARM validator; a missing GetDlgItemTextW
handler previously left the code buffer empty and caused rejection. Never patch
that validator or manufacture a registration value. Its next resource load
requests NFS.ini/TXT from NFS.inl, which stores NFS.INI/TXT. Resource lookup
keeps exact matches first, then normalizes ASCII case within the same module
before the existing cross-module fallback. Keep integer keys distinct and do
not change language/resource data or rewrite commercial files.


#### Pocket PC SIP, display profile and library artwork (2026-10-20)

The shared shell keyboard now also services COREDLL SipShowIM/SipGetInfo/SipSetInfo
and AYGSHELL SHSipInfo/SHSipPreference. Keep its Gizmondo GMSHELL protocol and
160-pixel controller panel unchanged. Pocket PC EDIT taps request the SIP;
read-only/disabled/hidden controls must never become text targets. Both ordinary
and DialogBoxIndirectParamW-owned modal pumps service the same keyboard input.
Modal pumps otherwise bypass the ordinary message path: forgetting them makes
SimCity registration show a caret but no keyboard. Posted WM_CHAR remains guest
owned, including validation of registration codes. Never bypass that validation.
SIPINFO is 48 bytes; reject undersized buffers without writing, share the actual
state and visible desktop/panel rectangles, accept both 0xE0/E1 and 0x100E0/E1
action encodings. Setting changes post WM_SETTINGCHANGE/SPI_SETSIPINFO.

KeyboardRotation is a host presentation event, never a guest key. Desktop and
Android send it at launch and when rotation changes. Render the SIP upright in
display coordinates, then map to the framebuffer; map taps through the same
transform. SIP rendering belongs to KernelState::presentation_framebuffer, a
copy used only while the panel is visible; never burn its pixels into the guest
framebuffer. Desktop, Android and CLI snapshots use that common presentation.
A captured keyboard press must swallow its move/release, including
when Done closes the panel. Pocket PC keyboard height is bounded to half the
display and 128 pixels. Text is committed separately from game key bindings;
desktop metadata exposes focused EDIT input and typing releases held game keys.
Android uses the shared touch panel; host Android IME composition is not claimed.

ScreenPref::Portrait retains the legacy serialized default but is now labelled
Automatic. Its launcher policy selects a VGA device (480x640) when the PE has an
actual CEUX/HI_RES_AWARE resource containing WORD 1, otherwise QVGA. This marker
declares native-resolution awareness; it is not proof of a VGA-only requirement.
Explicit Qvga, Vga, VgaLandscape and other screen choices override the policy.
Gizmondo still forces 320x240 before guest entry. Desktop and Android runners and
library captions share this resolution logic. Do not inspect title strings or
change touch coordinates independently of device geometry. GeoRally EX has the
marker and its ARM touch converter uses VGA constants: QVGA selects the wrong
areas even though GLES scales the visual menu to the smaller framebuffer.

AYGSHELL ordinal 10 and SHCloseApps share a capacity check. Racing Days requests
0x480000 bytes and branches directly to its insufficient-RAM message on FALSE.
Success requires available program RAM and heap capacity; the call neither
allocates the requested bytes nor closes the running title. Cross-process
shell memory reclamation is not implemented.

Pocket PC library icons are fitted inside a common device frame, with their
aspect ratio preserved. Missing icons keep the Pocket PC label. Gizmondo card
artwork is unchanged. Display captions report the resolved device dimensions.


Colors startup's explicit "Unlock Turf Wars + Tag Editor" action is shared
through pocket-library::colors_save, called off the UI thread on desktop/Deck
(X in the startup dialog) and Android JNI. Never apply it automatically at launch
or while a game is running. It changes encrypted config.cfg TurfWarUnlocked and TagEditorUnlocked to 1
and never modifies the existing ISV, Money, ServerMoney, or online balance.
Repeated clicks must preserve solo gains and synchronization checkpoints and
must never contact the server or grant money. No save is invented if absent.
Only changed config is backed up and atomically replaced; validate both formats
before writing. Preserve registration identity, missions and all other settings.
Use GameEntry::gizmondo_flash_dir; reject ambiguous paths and symlinks.

pocket-core::colors_wallet separates solo and online balances for the exact
retail hash documented in COLORS.md, patching only the loaded image. Preserve
all instruction/layout/hash checks and leave unknown binaries untouched. Native
mode-0/1/2 responses checkpoint ServerMoney to current solo Money without copying
online funds to solo. Online calculation adds only positive solo delta. The
server starting-money floor is a first-registration rule, not a recurring grant.
Legacy synchronization keeps its original checkpoint timing and has no new
transaction IDs. See COLORS.md for original ARM addresses and validation limits.
The subsequently supplied complete Colors archive reaches the 3D tutorial. Direct
entry/return timing identifies 0x912b8 as a substantial recurring guest cost,
consistent with model-pose interpolation (inference from disassembly). See COLORS.md
for measurements and limits. All temporary profiling hooks are removed from production.

Absolute CRT file paths must stay rooted for writes as well as reads.
Battlestations' supplied autorun.exe save routine at 0x2bb14 first opens
`\\Flash Disk\\MyGames\\Midway\\Midway96.sav` in wb mode; on failure it
creates MyGames and Midway, retries, and writes 0x660 bytes. The CRT basename
fallback previously created Flash Disk/Midway96.sav instead, falsely succeeding
and skipping those directory calls. Rooted reads may subsequently reject that
misplaced file when MyGames already exists. Keep the initial NULL result for
missing parents and let the guest create its directories; never redirect an
absolute write into another mount or truncate an unrelated basename match.
Both fopen and _wfopen share this path, on desktop/Deck and Android. Relative
path compatibility lookup and the VFS read-side resource fallback are unchanged.
Regression tests cover empty/existing MyGames, save round-trip, slash spellings,
wb/ab/r+b failures, read-only cards and preservation of existing root saves.
The supplied ARM save routine was invoked locally in isolation with a diagnostic
entry point, not through full gameplay (only the EXE was supplied). Existing
misplaced saves are not automatically moved or overwritten.

Startup update checks query guizmox/PocketHLE published stable Releases only,
off the UI thread, never Actions artifacts. Desktop uses pocket-library::updates
with the optional release-updates HTTPS feature; Android uses ReleaseUpdates
and the workspace Cargo version generated into BuildConfig.UPDATE_VERSION.
Keep shared version/asset fixtures aligned and do not compare Android's legacy
test-build label against stable tags. UI notices wait for catalogue availability;
Deck A/B must not launch a game behind the update prompt. No package installer,
library mutation or permissive TLS verifier belongs in this path. See UPDATES.md.

RGBA readback of the exposed RGB565/alpha-less EGL surfaces returns alpha 255
for in-bounds pixels. Internal RGBA alpha remains available to the renderer;
never overwrite it or texture alpha to implement this conversion. Stuntcar
Extreme's cached HUD labels rely on opaque surface readback to preserve their
blue colour key through CPU image conversion. See docs/GPU.md for reproduction
and scope; no executable/resource patch or title-specific branch is needed.


#### Gizmondo VIB1 / NLED vibration bridge

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


#### WinINet HTTP/HTTPS host bridge

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


#### GPS1 host location bridge

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


#### 6. Presentation — two paths

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


#### 13. Frontend input and launcher settings

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


#### 27. Console-only fullscreen and native GPU reconstruction

Historical shortcut snapshot (superseded by the [current host-key table](FRONTENDS.md#visual-input-editor--desktop-and-android)):
F11 toggled borderless fullscreen, F10 captured a game screenshot, and Escape
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


#### 28. Optional SMAA and xBRZ filters

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


#### 29. Capture the displayed game pixels

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


#### 30. PocketPC presentation and shared display controls

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


#### 31. Targeted missing-API reporting and scrollable options

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



#### 32. Desktop preferences, platform tabs and PocketPC directional controls

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



#### 33. Wave loop break, CE power status and offline Winsock

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
without registering a receiver or requiring permission. The shared host
projection normalizes known percentages to guest flags: critical 0..5%,
low 6..20%, normal 21..66%, high 67..100%. Android uses the same thresholds.
Charging and absent-battery flags remain honored. Alerts use known percentages
before host LOW/CRITICAL bits, so Windows cannot raise low above 20%; when the
percentage is unknown, known host severity flags remain the fallback. The
percentage itself is unchanged. A shared one-second
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


#### 34. General API completion and Gizmondo ROM exports (2026-10-09)

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


#### 35. Bluetooth Classic SDK path and native transports (2026-10-09)

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



#### 36. Desktop physical controllers

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


#### 37. Guest Winsock RFCOMM sockets

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



#### 38. Gizmondo CAM1 camera and host capture

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



#### 39. Catapult continuous camera preview and mixer cleanup

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



#### 40. CAM1 preview row orientation

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



#### 41. Colors multiplayer endpoint, identity and 64-bit time

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



#### 42. Desktop Gizmondo options navigation

Historical UI layout (the dropdown hierarchy is now replaced by the
[desktop button toolbar](FRONTENDS.md#desktop-button-toolbar)):
The desktop Settings menu had Emulator Settings followed immediately by
Gizmondo options. Screen::GizmondoOptions renders GPS, GPRS/data, Colors server/ID,
Bluetooth and camera controls; Screen::Settings renders general emulator
options and keyboard/controller bindings. Both views edit a full LauncherConfig
draft and share Save/Cancel persistence, so saving either preserves fields
from the other. They have separate scroll IDs. Only the general settings
screen participates in keyboard/controller binding capture; opening Gizmondo
options releases held keys and clears pending captures. GPRS/data is saved through
the same draft and takes effect on the next launch. Android JSON mirrors its
boolean default, while this menu organization applies to the desktop launcher.


#### Windows and Android fixed GPS position

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



#### Android frontend and native packaging

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

Android display size is automatic; the former manual Display scale setting and
configuration field are removed. PocketPC uses every fitting integer multiple of
the logical LCD, independently of GLES internal density. Gizmondo uses automatic
produced-image integer fitting; controller fullscreen uses aspect fit. If even one
logical PocketPC image cannot fit, downscale proportionally. Never change guest
dimensions for presentation sizing.
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

##### Android presentation and documentation invariants

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

##### Android child processes and RAM/VFS diagnostic parity

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

##### Temporary Android Colors registration probes

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

##### Android on-screen control feedback

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


##### CI build prerequisites (October 2026)

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

##### Live output and MAS volume controls

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


##### Launcher controls, position fill and shared executable information

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


##### CI ARM diagnostic test scheduling

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


##### Android continuous touch D-pad

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

##### Per-game D-pad tuning, SDL vibration and Gizmondo backlight

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

##### SDK date/time, shell status, accelerators and DirectShow completion

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

##### Host device status and launcher DirectShow preview (2026-10-11)
Android reads airplane/SIM/service state through application-context JNI snapshots cached one second; denial means unknown, no subscriptions/IDs. GT_STATUS carries airplane/radio only (no SIM bit). Windows defaults SIM ready/radio off/airplane off; ActiveSync stays disconnected. GSM/TAPI/RIL stack remains out of scope. GLES marks the replaced framebuffer dirty before DirectShow service so cached preview frames are recomposed after menu drawing. Regression rectangle matches attached launcher (204,40,101,82).

Launcher startup preview also requires WinCE EC_VIDEOFRAMEREADY (0x49), as confirmed in SDK Movie and attached ARM binary (0x15934 comparison). Emit once on first decoded frame, even offscreen, reset per RenderFile, GetEvent consumes before completion without consuming WaitForCompletion. No event-owned memory; notification disabled flags still allow polling. Launcher initially parks window at (844,40,101,82) and moves it to (204,40,101,82) only upon readiness.

##### Native GLES implementation reference

The renderer architecture, memory/resource ownership, fallback contracts,
optimizations and presentation timing are maintained in [GPU.md](GPU.md).
Read that document before modifying pocket-gles or GLES presentation handlers.

##### Android FPS overlay
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



#### Suspended-process idle gate fairness (2026-10-11)

Android RAMTEST's intermittent CI timeout stops after remote_suspend_counts.
All processes share the handle domain's API gate. schedule_idle previously slept
1 ms inside dispatch while owning that gate, letting a suspended child repeatedly
reacquire it and starve its parent's pending API calls. Move idle sleep to the
kernel after releasing the gate, before frontend polling. Keep idle cadence,
Sleep durations, suspension counts and control application unchanged. No timeout
increase or skipped test. Native ARM runner reproduction requires Unicorn and
libclang; local build lacks libclang, so full diagnostic confirmation is pending.


#### Frontend feature updates (2026-10-12)

Simulated GPS offers manual coordinates and one random uniform-area spherical fix within 800 m of the Eiffel Tower per launch. gps_fixed_random persists across Windows/Android settings. The same shared provider setup works on Linux. Manual coordinates remain stored and native permission bypass/lifecycle are unchanged.

Game hardware information detects Bluetooth, Internet, GPS, camera, vibration, power, SIM, battery and backlight using device strings, named imports and the existing CE4.2 coredll ordinal map for Gizmondo. No static evidence is not proof of absence. Scene camera symbols are not camera-device evidence.

Valid BMP labels beside a Gizmondo executable are converted to cartridge.png for desktop/Android cartridge artwork only; their filenames never supply game titles. Input size and dimensions are bounded; invalid BMPs retain the existing fallback. Demo launchers remain Demo Card. No bitmap or copyrighted game payload is bundled.

Demo import prompts Keep launcher / Separate games. Each separated entry owns a game directory, settings and save namespace, retains original card-relative paths and shared resources, and excludes other games directory trees. Native splits run off the Android/desktop UI thread. Source launcher removal follows successful title creation. Split entries retain Gizmondo platform identity even without a GZGA marker.

Android Pocket PC sessions restore the upstream eea7ac6 activity layout/control subtree and styles, including Turbo/F3. Portrait reserves the measured controls height; landscape overlays controls on the full-height game area. The original fullscreen toggle and modes are retained; Rotate and Exit remain accessible while the toolbar is hidden. Display turns persist in per-game settings and effective framebuffer dimensions determine locked portrait/landscape, independently of phone rotation. Pointer and directional input follow inverse presentation rotation, releasing held input before turns. COD2 sideways portrait framebuffers default to CW90 on import; existing imports can use Rotate. Guest resolution and GL matrices remain unchanged.

Linux and Windows share the desktop frontend, settings, layouts, filters, controller handling and imports. Simulated GPS is available on Linux. Native camera/Bluetooth/location/Internet implementations are platform-specific; matching UI does not imply Linux hardware bridge parity. See DESKTOP-PARITY.md.

Compatibility tables reflect the maintainer-reported OK status for the maintained collection and retail catalogue names. They are not new independent gameplay runs or proof of all modes. Original proof directories remain intact.

About/credits expose https://github.com/guizmox/PocketHLE on Windows/Linux and Android, while retaining original project attribution.

Selected demo-card title IDs prefer the executable's GZxxNNNNNN ancestor directory. Shared launcher markers cannot supply a split game's ID. Desktop cartridge labels, bottom captions and shared game information use the same helper. Existing imports need no migration or reimport.

Static game names come from the maintainer's Liste_Jeux.xlsx IDs/titles, with the explicit 200030 Motocross / 200041 Furious Phil correction. Known static titles supply names at import; unknown codes retain the regular import fallback, independently of BMP filenames. BMP artwork is retained. Existing ID-shaped names migrate on library load, preserving custom names, IDs, settings and save paths. No release/build version is inferred from a shared title ID. Unknown codes use the previous fallback. The initially unclear GZGA22222 source row was later corrected by the maintainer to GZGA200069 (Catapult).


#### Desktop PocketPC presentation restoration (2026-10-12)

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


#### Documentation organization

GPU implementation details live in [GPU.md](GPU.md); the user-facing Gizmondo
guide is [GIZMONDO.md](GIZMONDO.md). [README.md](README.md) indexes documentation.
Release ZIP notices/manifests are transport metadata, not project documentation.
Keep tool-local diagnostic READMEs and proof assets with their sources.


#### Android CAM1 orientation

Gizmondo camera packets rotate their Y/U/V planes using Camera2 sensor orientation,
lens facing and current display rotation before preview or tracking consumes them.
The existing UI poll catches reverse-landscape 180-degree changes even when Android
does not issue onConfigurationChanged. Each packet snapshots one rotation for all
planes; mailbox clearing and stale-publication checks prevent old orientation reuse.
No mirror is introduced and the existing CAM1 row convention is retained.
Non-Gizmondo sessions retain raw camera orientation. No Activity is stored in
CameraHost, no new listener/thread/timer or camera reopen is needed for a turn.

Orientation reference: https://developer.android.com/media/camera/camera2/camera-preview


#### Diagnostic lifecycle and timing coverage (2026-10-12)

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



#### Colors startup options

Desktop and Android show an English-language Colors startup menu on every Run
request, before entering emulation. The language list comes from this card's
physical bstrings siblings (eng/fre/fra/ger/deu/ita/spa/esp), deduplicated by
language, never from another installed variant or an editable display name.
Only normal Colors cards GZGA200015 and GZGA270015 are eligible; demo/ALT
cards launch directly and do not inherit these language/GPS/server overrides. The remembered colors_language is preselected if available;
otherwise English. One-language cards show their language without a selector.
Start persists only these launch preferences before running; Cancel does not
persist draft edits or start emulation. Already confirmed pending runs do not
reopen the menu after stopping another game.

Fake GPS starts disabled in each new menu. Enabling it reuses the existing
once-per-launch fix near Eiffel Tower (within 800 m), or validated manual latitude
and longitude. Remember coordinates/mode, but require an explicit enable for
each launch. Simulation belongs to Colors only; other games retain host GPS
when enabled. Desktop host-access preparation and Android permission requests
must honor this same Colors identity guard, so simulation bypasses device GPS.
Android startup includes Fill GPS using DevicePositionRequest: copy the newest
valid cached device fix first, otherwise request a fix with the existing 20s
timeout. Permission denial/unavailability leave coordinates unchanged; filling
never enables Fake GPS or persists the draft. Close/Start/onStop cancel capture;
a pending permission callback cannot update a dismissed menu. The multiplayer
server has a separate spaced section heading; its previous value is saved.
Gizmondo options retain hardware activations. The Colors player-ID editor is
hidden; persisted installation identity generation remains unchanged.

VFS read-side bstrings.eng aliases remain process-local, use only case-insensitive
siblings beside the resolved resource, and preserve English on missing or
ambiguous translation. No EXE/resource or write destination changes. No supplied
translations or rewriting inside data.zcd. Device validation still requires
original assets and version-specific string keys.

The static title table includes the 26 previously missing GZGA200 IDs extracted
from the supplied MultiBoot launcher. Existing maintainer titles and IDs stay
unchanged, including exact GZGA270 and GZGA280 entries; no prefix substitution
is inferred. Only normal GZGA2… Colors IDs enable the startup menu.

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



#### Wrapped MultiBoot card mounts

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



#### GZGA collection imports and library UI exclusion

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


Hit & Myth Texas splash freeze: prior local tests set the explicit diagnostic
POCKETHLE_SLICE_TIMEOUT_MS=20, masking an API-free native-clock polling loop.
Without it the guest froze before starting audio and stopped servicing the host.
Normal kernel slices now use the existing reusable wall-clock SliceTimer with
20ms quantum (5ms while live audio workers need it). No per-instruction counter,
new thread per emu_start, game patch or native-clock ABI change is introduced.
Unchanged timeout settings return immediately to avoid redundant timer-state
locking on each API dispatch. API-free loops can refresh the host-managed tick
page and service input/Exit; preserve registers/PC when resuming a timed-out slice.
Validation must run WITHOUT POCKETHLE_SLICE_TIMEOUT_MS; otherwise it cannot
prove this repair. A real ARM poll test checks preemption/resume and unchanged
registers before the host updates its cell; the kernel host-hook test also checks
preemption is enabled with no audio. Recheck native splash/menu and Exit behavior.



#### Colors startup notices, game completion and Sonic modifier polling

Colors startup retains only its unlock button; completion/errors appear in an
acknowledged popup on desktop/Deck and Android. Do not add balance descriptions,
launcher status/welcome text or backup notices to the startup form. File backups
remain mandatory internally. Deck A/B dismiss the notice without also launching
the game; queued navigation is cleared while the notice is open.

A desktop/Deck RunFinished event returns the active Run screen to the catalogue
and restores its saved window sizing. Preserve a pending replacement launch and
avoid redirecting users who have already navigated into settings/library.

The supplied Sonic/SEGA ARM input routine at 0x882d0 polls VK_LSHIFT (0xA0) and
VK_LCONTROL (0xA2), while Gizmondo buttons emit generic SHIFT/CONTROL. Key-state
queries accept generic modifier state for either side; generic queries combine
left/right states, and a specific opposite-side key alone does not count. Pending
releases apply separately to each source key so releasing one side does not
release a held opposite side. Existing C1/D1 aliases and ordinary key ordering
remain unchanged. Apply to shared GetKeyState/GetAsyncKeyState, with no title
fingerprint, game patch or changed global button bindings.

##### Nested native modal DispatchMessage calls (Navigator)

DispatchMessageW continuations are a stack per guest thread, not a single
frame. A callback returns only at its saved SP; a deeper DispatchMessage
from a native modal loop pushes another continuation. Navigator previously
restored the outer SP while its nested address wizard was still alive,
leaving CWnd objects below SP and crashing at 0x80DD4. Kernel trap returns
use the same stack-level check; process/thread teardown removes all frames.
CLI diagnostics POCKETHLE_TRACE_VAS (comma-separated hex VAs) observe without
altering execution. POCKETHLE_DUMP_MEMORY=HEX_VA:HEX_SIZE:PATH saves at most
16 MiB after normal exit or a guest fault; neither is enabled by default.

POCKETHLE_INPUT_FILE optionally enables an append-only CLI control file
(up to 64 KiB, complete lines only): `key TOKEN`, `tap X,Y`, `quit`.
It uses the existing scheduled InputEvent path and holds/release timings;
no guest instructions or title-specific button bindings are changed.

Mounted-volume spellings without an initial slash (Navigator) resolve at
the matching explicit mount prefix, for reads, attributes, directory
enumeration and writes alike. This fixes FindFirstFileW("SD Card/.../datacd")
rejecting a directory that CRT fopen's rooted retry could already open.
An explicit `.\\` prefix stays module-relative, ordinary filenames keep the
module default directory, and read-only mounts/write containment still apply.

The live CLI control file also accepts `windows` for a guest window/control
snapshot and `gps LAT,LON` to update the fixed simulator position. The latter
requires an already enabled GPS service; it does not grant location access.

ListView rendering and pointer hit testing share the same row height.
Navigator's Start Driving row was drawn at y=181 but taps were divided by
24 instead of the rendered 14 pixels, selecting Enter Destination instead.
A regression checks both the chosen item/notification and its painted highlight.

DialogBoxIndirectParamW stores its original arguments, SP and LR before
calling a guest DialogProc, and restores that continuation on EndDialog,
default IDOK/IDCANCEL and headless timeout. A callback may return with POP {PC}, leaving LR at its last helper call;
only the saved blocking caller continuation supplies a valid return address. Idle DialogBox/MessageBox retries
wait 1 ms so their iteration bound leaves time for host button input.

The host modal pump handles Escape as IDCANCEL even without child focus,
Tab visits only visible enabled WS_TABSTOP children of that modal, and
Return activates its focused button or its own default button. Underlying
window controls never receive those modal commands. This preserves shared
Gizmondo Soft2/Soft1/Play bindings. The external GMSHELLCLASS on-screen
keyboard protocol is still missing; host typing is not console-only input.

Closing the parked modal also restores its saved active window and child
focus. Otherwise GetMessage routes later Gizmondo keys to the dialog's
already destroyed window, even though the navigation map still repaints.
The continuation regression covers normal EndDialog and headless timeout,
including an LR left at the last ARM helper and displaced input ownership.

Desktop Gizmondo launches preserve the actual executable directory relative
 to the mounted card. Flattened imports retain declared install aliases.
Reporting a nested CoPilot.exe at \SD Card\CoPilot.exe makes it open
\SD Card\pid.cfg, fail, and terminate with code zero before native DLLs
load. The path helper is shared with the headless GUI-mount probe; nested
install prefixes also use install_mount_dir instead of hiding the real
application folder with the extraction root. Child process guest paths still
have final precedence. Runtime Windows remains untested in this environment.


#### Navigator graphics cumulative build 27 (2026-10-18)

Retain the build26 startup/module-path/modal/input fixes. Resource-backed images are in kernel::imaging and winceapi::coredll::images, shared by named COREDLL/COMMCTRL APIs; never restore constant image-list or LoadIcon application handles. Resolve resources through resource_scope(hinstance), including native DLLs. List-view LVIF_IMAGE values and associated normal/small image lists participate in composition and row hit testing; maintain DIB readback/writeback and the compositor frame_counter invariant. Tests live in coredll/graphics_tests.rs and kernel/imaging.rs. ScrollWindowEx is in coredll/window_graphics.rs: overlap-safe pixel movement, uncovered region output, child movement and deferred erasure; do not replace it with unconditional zero or blanket repaint. GetFileTime exposes available VFS dates, CompareFileTime is unsigned FILETIME ordering, class graphics attributes are shared by class. Full Win32 image-list flags and icon-grid layout remain partial; see NAVIGATOR-RENDU-BUILD27.md.

RAMTEST native import/paging tests require the original tools/ramtest/dist/GZRT999999 DLLs, shipped by the upstream tools/ramtest/dist/PocketHLE-RAMTEST.zip. Missing fixtures can masquerade as loader/LastError regressions: restore original fixtures rather than weakening tests. This source-only reprise has no Navigator game data; menu appearance/navigation/Gizmondo buttons still require actual game replay.


#### TomTom local host lookup, notification icons and unknown ZIP platform (2026-10-20)

Legacy WINSOCK.dll calls are handled by winceapi::legacy_services; keep the ws2.dll
Bluetooth implementation unchanged. WSAStartup/cleanup have their own process
reference count, independent of the Bluetooth permission/radio. gethostname returns
the virtual device name PocketHLE. gethostbyname supports that name and localhost
only, with a reusable per-guest-thread ARM32 HOSTENT containing loopback, guest
pointers, an empty alias list and a terminated IPv4 address list. Unknown/external
names return NULL and WSAHOST_NOT_FOUND; this is not DNS or IPv4 socket transport.
Final WSACleanup frees its buffers. Never return success without writing outputs.
Shell_NotifyIcon accepts the CE 152-byte NOTIFYICONDATA and models ADD/MODIFY/DELETE
by (HWND,uID), copying icon pixels and applying only flagged fields. Window teardown
removes its icons. Host tray rendering/click callbacks are not implemented; no
synthetic clicks or fake network/location are generated.

Static GPS evidence includes NMEA/serial use when a COM/serial-API indication and
GPS-specific vocabulary coexist. A COM8 string or generic serial import alone is
not sufficient. Existing GPS1/API detection remains available to both frontends.

Prepared ZIP imports expose per-entry unresolved platform choices; CAB sources and
recognized Gizmondo cards skip the question. Desktop and Android ask in English
before publishing. The optional GameEntry.platform field stores an explicit choice
and takes precedence over heuristics; existing manifests without it remain valid.
Reimport preserves an earlier explicit choice and settings. Selection/cancellation
operates on private staging; update confirmation remains a separate step. CLI
import APIs retain their noninteractive default. JNI selection checks token owner
and unresolved game ID without consuming another pending import. Diagnostics/tests
must distinguish shared-core checks from native GUI/Android hardware validation.


##### Pocket PC action key and initial focus (2026-10-20)

Desktop and Android select the Pocket PC input profile after loading every
non-Gizmondo process. It advertises VK_RETURN as GXKeyList.vkA and delivers
that same key in both filtered and unfiltered message retrieval. Legacy and
Gizmondo retain their exact 0xD1 table/remapping, including direct core clients
unless they explicitly call Emulator::set_pocket_pc_input(true). Do not infer
that querying GAPI means a program has no explicit VK_RETURN bindings: COD2
reads the GAPI direction keys but binds Fire to Enter.

In that Pocket PC profile, initial ShowWindow sends WM_SETFOCUS through its
existing synchronous WndProc continuation after WM_SIZE, then restores the
original caller registers. Remove the queued startup copy at that point.
Never leave this initial sent notification for the application PeekMessage
loop: COD2 interprets focus gain there as restoration after a prior loss and
restores a zero saved volume. The Gizmondo/legacy startup sequence is retained.
Keep callback thread/SP ownership and nested continuation restoration intact.


##### Pocket PC quick rotation (2026-10-20)
The desktop Pocket PC run pane exposes Rotate 90° next to Run output, outside
the top toolbar so narrow windows still expose it. Cycle the existing
RotationPref order (0/90/180/270), release held keys and pointer in the old
orientation, persist using the existing per-game settings path, and request
window refitting. Rendering UVs, inverse touch mapping and screenshot rotation
remain the existing implementations. Gizmondo takes its separate UI branch.
Do not infer image orientation from framebuffer aspect ratio: games such as
Into Outland draw their own landscape content into a portrait GAPI buffer
without importing WinCE display-orientation APIs. Retain explicit choices.
Android already has its dedicated Pocket PC rotation button and persistence.


##### CAB installer-compressed resources (2026-10-20)
Both canonical CAB name-materialization paths finalize explicitly referenced
zlib .cmp resources (GeoRally EX's setup DLL case). Require a CAB .999 PE
installer exporting Install_Exit with embedded zlib evidence, plus an exact
ASCII/UTF-16 compressed basename reference. Decompress asset extensions only;
do not execute installer code or scan ZIP imports. Verify zlib stream end,
checksum, no trailing data and bounded per-file/import output before publishing.
Keep compressed inputs and never replace an existing uncompressed destination.
This is CAB installation processing, not a VFS filename alias or runtime hack.


##### Explicit Pocket PC hardware controls (2026-10-20)
GuestButton adds pocket_pc_app1..4 (VK_APP1..4, 0xC1..0xC4). Expose P1..P4
alongside the existing Pocket PC virtual pad on Desktop and Android, and in
keyboard/controller assignment choices on both platforms. New keyboard entries
start empty, and factory controller mappings stay unchanged. Do not globally
translate Ctrl/Space/Shift or alter existing Gizmondo bindings: other titles
use those literal keys. GAPI-advertised keys and VK_APP keys are distinct.
Into Outland's ARM decoder accepts directions and C3/C4 but ignores Enter,
Ctrl, Space and Shift. Its IN.REC supplied replay drives the demo instead of
normal play; remove it only from a user-requested test copy, never silently at
import. No game-name-based remapping or asset deletion is part of this change.

Control assignment UIs group rows as Shared controls / Emulator shortcuts /
Gizmondo only / Pocket PC only on Desktop and Android. Shared Android labels
name both the generic control and its Gizmondo equivalent, so console playback
symbols do not masquerade as Pocket PC action semantics. These are presentation
groups only; stored button identifiers and all existing bindings are unchanged.

PVRTC1 upload behavior and its shared Software/GPU fallback are documented in
GPU.md under PVRTC1 texture uploads (2026-10-20). Keep decoder changes scoped
to 0x8C00..0x8C03 and preserve existing format behavior/enumeration order.

##### Shared sections, privilege and registry enumeration (2026-10-21)

File mapping views use shared host-backed pages, not copies into private guest
allocations. Section handles, views and CPU mappings independently retain the
section and its VFS lease. Closing a handle must not invalidate a live view.
CreateFileForMapping transfers its special file handle on both mapping success
and failure; ordinary file handles are not consumed. Pinned files cannot change
length. Preserve VFS sharing, permissions, object-store quota and RAM charges.
MapViewOfFile uses its own address arena; VirtualFree must reject these views.
All CPU execution and host accesses to shared mappings use the device execution
gate. SharedMemory raw access and map_shared_region require this serialization.

ARM SetKMode changes the calling thread's CPSR mode and returns the old mode.
Restore CPSR before banked SP/LR when restoring contexts. Do not replace failed
CPU detection with a title-specific CPU result or skip guest instructions.

RegEnumValueW must terminate with ERROR_NO_MORE_ITEMS and distinguish UTF-16
name capacities from byte data capacities. RegQueryInfoKeyW has 12 arguments,
including lpReserved. Registry snapshots accept older snapshots without optional
value_display metadata; this metadata preserves original value-name spelling.
An older executable with strict snapshot parsing may reject the newer field.

Visibility inherited from parents is derived, never written into a child's own
WS_VISIBLE state. Paint and input must use the same ancestor visibility. An
owned popup is not a WS_CHILD descendant of its owner. Hiding a parent cancels
capture in hidden controls; showing it must preserve explicit child visibility.

Modeless child-dialog panels participate in pointer targeting even without a
window_rects entry. Their empty client area routes to their own DialogProc,
using client coordinates (border/caption excluded). Do not send these events
to the owner/ancestor frame or make an unrelated owned popup capture them.
Snails uses a WS_CHILD GAPI dialog and reacts to WM_LBUTTONUP there. GAPI
presentation cadence is documented alongside EGL in docs/GPU.md.

##### Pocket PC stylus stick and supplied middleware (2026-10-21)

StylusAim is a host-only velocity event, never a fabricated guest VK. The
Pocket PC input profile consumes it into bounded real stylus gestures at most
once per 16 ms. The stick lifts/recentres at an edge so absolute-coordinate
applications can keep dragging without a reverse jump. Clamp elapsed time
after stalls; release on zero, focus loss, rotation, real touch takeover and
device removal. A real touch and the SIP take priority. Gizmondo/legacy input
must ignore this event. Child return clears the parent's parked stick state.

LauncherConfig.stylus_stick is separate from guest hardware keys. Defaults are
O=up, L=down, K=left, M=right, and RightStick directions on a controller.
Desktop and Android preserve explicit empty bindings and apply inverse display
rotation exactly once to the velocity. Normalize diagonals and keep independent
keyboard, controller and touch contributions. Digital arrows remain movement;
the touch aiming stick is on the right, on Pocket PC layouts only. Android's
programmatic controls retain declared resource IDs in values/controls_ids.xml.

Ignored middleware has a native preference when its DLL is actually supplied.
Use the existing native-import/dependency/attach/IAT lifecycle rather than
returning fake success. Explicit HLE handlers still win; the legacy missing-DLL
fallback is retained. Snails' supplied fmodce.dll opens a real waveOut live ring
and its native guest mixer produces PCM; no commercial decoder/data is copied
into PocketHLE. This changes supplied FMOD middleware behavior generically and
does not establish compatibility with every FMOD game or hardware audio driver.



#### Pocket PC control layout revision (2026-10-21)

This section supersedes the earlier Pocket PC overlay/quick-rotation layout
notes, not their guest input contracts. Android PocketPcLayout reserves a
separate surface rectangle and PocketPcControls covers only its child touch
targets. Portrait starts the 3:4 native image at the top, with two action rows
below; the auxiliary keys sit between the left TouchDpad and right stylus
stick. Landscape puts two tall action banks beside the screen, with an 18dp
safety gap before the thumb controls. The actual guest framebuffer dimensions,
including VGA, determine the image ratio; never force a title into QVGA.

Android Pocket PC host layout follows FULL_SENSOR. Rotate changes only the
per-game image by 90 degrees; persist GameSettings.rotation, update renderer,
SIP and touch geometry, and release held input before switching. Directional
keys and stylus velocities use the same single inverse image turn; individual
input sources must not rotate before the central sender. All game layouts hide
system bars, including the PPC layout with controls, and reapply immersive mode
on focus. Respect display-cutout safe insets.

PPC presentation uses integer multiples 1..4 of the QVGA unit. VGA 640x480 or
480x640 uses half its source dimensions as that unit; map touches back to every
native source pixel. Other sizes and Gizmondo keep their native unit. A source
VGA image may therefore display at 960x720 without changing its guest mode.

Desktop and Deck share fixed left/right action banks around the screen, with
small Tab/Esc/Turbo and orange Exit/Rotate above it. Rotate retains the existing
per-game image/UV/input transform. Pocket PC window/fullscreen fitting uses
physical pixels and factors 1..4 only; Gizmondo keeps its previous uncapped
integer fullscreen policy and its own skin. Preserve source-aware key holds,
native touch mapping and configurable keyboard/controller stylus bindings.
The controls do not alter the guest display mode, CPU, renderer or sound.

ConsoleControlStyle owns the exact former Gizmondo button drawable factory:
all original gradient colors, borders, radii and pressed states are retained.
Each child receives its own drawable. Both platforms' Exit and Rotate controls
are orange; emulated keys remain slate. Do not tint all controls orange or
replace Gizmondo's established input paths.

Validation: full Android main Kotlin sources compile against actual API35 and
AndroidX jars (generated resource-ID/BuildConfig stubs, not an APK); two JVM
geometry/mapping regressions cover portrait/landscape, fractional density,
non-overlap, thumb gaps, integer scale and rotated QVGA/VGA input. Three Rust
integer-fit tests pass, including the unchanged Gizmondo fit policy. Desktop
and Steam Deck bins pass cargo check with --no-default-features --features
steam-deck --locked on Linux. Native Windows/Android/Steam Deck UI, multitouch,
fullscreen transitions and actual game play still need device validation.


#### SimCity dialog/GDI investigation (2026-10-21)

Top-level HWND allocation reserves owner references even if their frame has
not been registered. Reusing such an HWND can create a popup-owner cycle.
Retain ordinal DLGTEMPLATE item titles; SS_BITMAP controls load their bitmap
resource before WM_INITDIALOG, shared by module/resource and owned by GDI.
Logical palettes have owned entries and per-DC selection; the RGB565 device
still advertises no writable hardware palette. AYGSHELL ordinal69 resolves
to the existing SHDoneButton handler.

CreateBitmap copies caller-provided top-down WORD-aligned DDB bits, rather
than retaining the pointer or creating an empty bitmap. Supported source depths
are monochrome and 16/24/32 bit; indexed 4/8-bit DDB input is rejected rather
than inventing a device palette. Modeless template dialogs and their children
need local backing DCs even when window_rects has no entry; consult Controls
geometry. Otherwise the compositor erases native drawing with dialog grey.

Into Outland: actual ARM intro followed by animated 3D demo observed locally
at 240x320 and 480x640, after about 12 seconds. Android-only black output has
not been reproduced or established fixed. No per-title framebuffer workaround.


#### Android Pocket PC touch ownership (2026-10-21)

PocketPcControls spans the entire game root, including an empty surface area.
Ordinary Android sibling dispatch assigns additional fingers to an existing
full-window overlay target before consulting its empty regions. PocketPcLayout
therefore owns each pointer from its DOWN through UP/CANCEL and emits separate
MotionEvent streams to content_column and the controls. Remap POINTER_DOWN/UP
to DOWN/UP when a destination has only one finger; preserve IDs and coordinates.
Do not forward the whole event to both destinations or manufacture a stylus
release when only a button finger lifts. Native stylus input tracks its pointer
ID, rather than assuming Android pointer index zero remains stable. Pause
relative stick aiming during direct stylus contact and restore it on release.
Seven JVM geometry/rotation/routing tests and full main-source Kotlin compilation
pass; Android device multitouch and APK build remain to be validated.



#### Pocket PC menus, SIP and common file dialogs (2026-10-22)

Shell bars support deletion/insertion of native toolbar buttons. CE MFC uses
0xffff as the append index, text pointers and a submenu handle in dwData.
GetMenuItemInfo accepts the legacy zero-initialized 44-byte MENUITEMINFO layout
as well as explicit 44/48-byte layouts. Keep all menu resource ownership generic.

SHRecognizeGesture waits across API boundaries; release or movement cancels
without swallowing original input. SHRG_RETURNCMD returns GN_CONTEXTMENU after
a hold; parent notifications restore the synchronous caller continuation.
TrackPopupMenu and TrackPopupMenuEx have distinct owner argument positions.
Their popups share ordered menu state, disabled items, nested menus, cancellation,
command flags and WM_INITMENUPOPUP. Popup pixels live only in presentation copies.

Explicit PocketPC SIP requests work on custom windows without native EDITs;
disabled, read-only and hidden EDITs remain ineligible. Gizmondo's GMSHELL input
profile is unchanged. Plain STATICs without SS_NOTIFY pass stylus input through.

GetOpenFileNameW/GetSaveFileNameW use the guest VFS and real shell controls.
Never fabricate a selected file or save: cancellation preserves caller buffers;
acceptance returns a chosen UTF-16 guest path, leaving I/O to the guest. Trim
filter masks, browse directories, handle wildcard filters, validate requested
existing paths and output capacity, and confirm overwrites. Preserve filename
and extension offsets. CE hook/template fields are reserved. PocketPC runners
mount persistent My Documents separately from assets and Gizmondo storage.

SysTabControl32 is a generic native control: Unicode item labels/data, insertion,
deletion, programmatic selection, shared hit-test/item rectangles and multiline
layout, TCM_ADJUSTRECT, and stylus/keyboard selection. Programmatic TCM_SETCURSEL
must not notify. User changes call TCN_SELCHANGING before changing selection,
honor the parent veto, then call TCN_SELCHANGE; the application owns its pages.
Both ordinary dispatch and the modal pump preserve these notifications and
thread/stack-local callback frames. Do not paint over the page display area or
add title-specific Budget controls. Unsupported tab styles (vertical/owner draw)
are not claimed as full common-controls coverage.



#### Native stylus capture (2026-10-22)

SetCapture/GetCapture/ReleaseCapture maintain real guest ownership, distinct
from the HLE button press latch. Preserve the previous owner return value,
GetCapture's thread scope and synchronous WM_CAPTURECHANGED on release or
transfer, including nested guest callbacks and register restoration. Route
captured pointer events in the owner's client coordinates even over a sibling
control, retain negative coordinates, and do not synthesize that sibling's click.
Pointer down/up also update VK_LBUTTON; hover must not claim MK_LBUTTON after
release. Destroying a control or dialog releases its ownership. Never emulate
these APIs with FAKE_HWND constants or add a game-specific input route.

SHRecognizeGesture caches a successfully recognized hold until the actual
stylus/action release is consumed. MFC first asks SHRG_RETURNCMD and then
reissues the same SHRGINFO as SHRG_NOTIFYPARENT. The second call must notify
immediately, without waiting through another recognition timeout or cancelling
because the initial release is queued. Clear recognition for a new stroke,
on consumed release, and when the target control is destroyed.

GetCursorPos returns the consumed stylus screen position. MSG.pt and per-thread
GetMessagePos preserve screen coordinates independently of client-relative
mouse LPARAM, including negative captured coordinates. Keyboard messages use
the current stylus point; querying another thread must not reuse its last MSG.

IMGDECMP bitmap outputs are DIB sections: honor iBitDepth, DWORD-aligned
bottom-up rows and guest-readable bmBits returned by GetObject. Keep a separate
RGB565 host cache, and release the DIB heap allocation on DeleteObject. Never
return a DDB with NULL bits to a caller requesting decoded pixel data.

sndPlaySound takes two arguments; PlaySound takes three. Keep separate ANSI
and Unicode wrappers. SND_MEMORY is a RIFF/WAVE image, independent of PE
resources. Test the complete SND_RESOURCE mask, not its shared MEMORY bit.


#### Posted broadcasts and Gizmondo data notifications (2026-10-22)

HWND_BROADCAST (0xffff) is a recipient set, never a window that can be passed
to DispatchMessageW. enqueue_window_message snapshots all registered top-level
windows, including hidden/pop-up windows, excludes WS_CHILD, and posts one
copy with the real HWND to each owning thread. PostMessageW and the Gizmondo
shell's GPRS_STATUS/GT_STATUS notifications share this generic route. Keep the
256-message queue bound and reject a guest broadcast atomically if it cannot fit.
Do not restore fallback dispatch to arbitrary WndProc for invalid/desktop HWNDs;
that caused the CoPilot startup crash. This fixes notification delivery, not
HTTP/TLS or a proof that a remote server is reachable.


#### Raw framebuffer polling and host presentation changes (2026-10-22)

Both desktop and Android queue KeyboardRotation on Pocket PC startup, including
zero turns. Into Outland opens GAPI then writes through ExtEscape's raw buffer
without GXBeginDraw/GXEndDraw. Marking rotation as a native-pixel write advances
frame_counter without gx_last_pushed_counter and permanently disables raw
readback. Native reproduction: normal image without rotation; with the exact
frontend startup event, black with counter=1 and last_guest=0 while audio runs.
Use KernelState::invalidate_presentation for host-only SIP visibility, case and
rotation changes. It preserves equality only if guest/host pixels were already
synchronized; do not acknowledge pending real GDI writes. Backlight changes
follow the same rule. Do not remove sync_guest_framebuffer's real pixel guard,
change the game's config, force a renderer, or add a title-specific exception.


#### Android Pocket PC portrait presentation (2026-10-22)

PocketPcGeometry reserves one top row for Tab/Esc/Turbo, then a larger gap
before the orange Exit/Rotate actions. The display room lies between that row
and the existing bottom action/thumb controls. PocketPcLayout centers the
scaled surface horizontally and vertically in its room in both orientations.
Keep the content view's actual bounds as the stylus routing rectangle; no guest
resolution/coordinate changes or Gizmondo layout changes belong in this update.


#### CAB ANSI install names (2026-10-22)

MSCE counted strings must preserve Windows-1252 accents (and valid UTF-8).
Fade CAB file 834 contains byte C9 in ZSSQ VÉZIMP.IFJ; lossy UTF-8 created
a different filename. Apply the decoding fix to new imports only. No startup
repair function or launcher repair calls: existing installations are untouched.
Do not add title-specific VFS aliases.


#### Settings visibility (2026-10-22)

Desktop and Android hide the CPU backend selector and both dispatch budgets
in normal settings. Preserve serialized values; this is a UI-only change.
Hide game screen geometry when the shared Gizmondo classifier identifies
the title, leaving PocketPC's display profiles accessible. GPU labels no
longer say experimental in global or per-game settings.


#### CI diagnostic fixture counters (2026-10-22)

Uploaded CI logs show current RAMTEST v9 PASS checks=0x91, VFSTEST v4
PASS checks=0x82, and intentionally disabled process-launch VFSTEST
FAIL checks=0x81 failures=0x19. Keep exact counters and zero-failure
assertions in positive runner tests, including desktop VFSTEST.
CoPilot config priority uses only the first available config; use iterator
next rather than a for loop with unconditional break (clippy never_loop).


#### Steam Deck fullscreen and sound CI (2026-10-22)

Deck fullscreen fills the largest aspect-preserving rectangle with fractional
scaling, including downscaling VGA; do not cap it at whole multiples. Other
desktop editions retain their integer policy. Guest dimensions/stylus transform
stay unchanged. New Deck button mappings are only proposed diagrams pending
user validation; no input mapping changes in this patch.
The legacy PlaySound regression test checks captured submitted PCM rather
than draining AudioTap's raw ring: CPAL queues voices separately. Assert all
three API calls' exact PCM and sample rate with both feature configurations.


#### Approved Deck controls (2026-10-22)

Historical mapping snapshot; use the [current factory profiles](FRONTENDS.md#factory-input-profiles) for present behavior.

View (left/Select): release before 650ms toggles fullscreen; hold sends ESC
in the original layout. Superseded in 0.4.4: View hold now exits to the
library for both profiles, with no Brightness guest key. Menu (right/Start): tap cycles
filter, hold exits to library. Hold consumes tap; no guest Start/Select leaks.
Gizmondo L1 sends Piano2 volume; all other saved Gizmondo bindings stay.
PocketPC R2=Action,L2=APP2,R1=APP3,L1=APP4,A=A,B=B,X=C,Y=TAB. These reserved
controls take precedence over older L1 fullscreen assignments and stylus
bindings; stick/dpad mappings and inverse pointer transform remain unchanged.


#### Deck fullscreen shortcut across launcher and game (2026-10-22)

View/Select short press toggles fullscreen in the launcher UI as well as
in game. Preserve a focused UI View timer across per-frame guest-key release;
clear it on focus loss. UI long press must not send guest ESC/brightness.
Migrate saved factory L1 fullscreen to View in the Steam Deck config, preserving
other custom assignments. Never let an old L1 fullscreen intercept game volume
or APP4; L1 must not toggle fullscreen in the UI either.


#### FILETIME local conversion (2026-10-23)

FileTimeToLocalFileTime is registered in datetime.rs. Convert against the
existing emulated UTC/no-DST timezone, preserving all 64 bits and 100-ns
precision; do not use the host timezone independently of GetLocalTime and
GetTimeZoneInformation. It reads exactly two guest pointer arguments, writes
8 bytes on success, and reports invalid/inaccessible buffers via LastError.
No Warcraft-specific handling. Update this conversion together with clock
and timezone APIs if configurable emulated timezones are added later.



#### Desktop installer import containers (2026-10-23)
`pocket-library::installer` decodes host installers without executing them.
Format detection uses PE resources, valid zlib headers, NSIS first headers and ZIP
central directories; never game names, fixed offsets, or filenames. Guest PE
machine types retain the existing standalone path. Shared decoded-byte, entry
count and nesting limits apply. Temporary paths are generated, never archive paths.
Recovered cabinets pass the normal CAB importer and CE/architecture checks.
Ambiguous multi-CAB installers report an error rather than guessing the game.
The existing NSIS reader remains the fallback for modern/solid variants.
InstallShield 6–13 header/volume sets inside ZIP/SFX are decoded without host
execution. Require the matching volume, bounded Deflate blocks, exact expanded
size and matching MD5. Split/obfuscated/linked/old-compression variants remain
explicit errors. Format layout follows the MIT Unshield reference (notice in
docs/third-party). No filename, game-specific offsets or archive modification.
Multiple CE cabinets are offered through PreparedImport.cabinet_choices on both
Desktop and Android, including unsupported/unknown CPU labels. The user selects
one; never silently prefer ARM or merge architecture variants. Selection happens
in private staging and ordinary update confirmation remains after selection.
Dropping/cancelling clears staging. Direct/CLI imports report a choice requirement.


#### WinCE CRT div/ldiv aggregate return (2026-10-23)
ARM WinCE div and ldiv return an 8-byte {signed quotient, signed remainder}
structure through caller-owned storage in r0; numerator is r1, denominator r2.
Write exactly eight bytes and return the storage pointer in r0. Both int/long
are 32-bit. Truncate towards zero; remainder has numerator sign. Reject undefined
zero/overflow operations without host panics. This differs from __rt_sdiv's
divisor/numerator register convention and register pair return. Never change
__rt_sdiv or patch GhostWorld callers to compensate for a missing CRT aggregate.


#### Registry parent keys (2026-10-23)
Creating a deep registry key or seeding a CAB value must materialise every
ancestor in the same quota-checked transaction. Descendant keys cannot exist
without their parents. Keep display spelling and case-insensitive lookup.
Springer's existing CAB LevelPacks/DefaultPack exposed this generic bug: it
opens LevelPacks before enumeration and correctly exits if that parent is absent.
Do not seed invented game values or special-case that path.


#### Nested Microsoft cabinet wrappers (2026-10-23)
Desktop setup packages may embed a Microsoft cabinet whose files are desktop
Setup.exe/Setup.ini and one or more actual CE cabinets. Recursively decode
nested CAB members with the same depth/byte limits; keep architecture selection
for multiple CE payloads. SetupKursk_ARM supplied this container profile.


#### CE startup command line (2026-10-23)
GetCommandLineW must expose the explicit startup argument string (the same
string passed to CE WinMain), defaulting to empty when no arguments were supplied.
The CE CRT obtains argv[0] separately from GetModuleFileNameW. Returning the
module path as extra arguments broke Stratagus with "too many files", especially
under Program Files. Preserve explicit argument text and its stable cached pointer.


#### HSS memory clips and channel ABI (2026-10-23)
The PAX_N load overload receives WAV/MOD bytes, not UTF-16 paths. Bound reads
by the format's declared length (including MOD patterns/sample headers), copy
before the guest releases memory, and do not cache by reusable guest addresses.
HSS speaker channel lookup must return a stable object with a real method table
(volume, pan, position getter/setter, length), backed by the actual mixer voice
group. Keep per-channel frequency, volume, playback/pause/loop/position state.
Never return zero-filled pseudo objects: Street Duel calls the virtual volume
setter immediately. Memory audio loading and channel ABI are generic HSS fixes.


#### IMGDECMP completion and IImageRender (2026-10-23)
Decode bitmap data before calling pfnImageProgress with bComplete=TRUE and the
original lParam. Supply a real reference-counted IImageRender with Draw, GetBitmap,
GetOrigWidth/Height, GetBits, ImageFail and IUnknown; no dummy COM pointer. Keep
callback continuations separate from data callbacks, restore the original ABI
frame, and release only the internal reference on callback return. Transfer
HBITMAP ownership when requested, preserve caller-owned bitmaps on renderer release.
Kursk supplies a progress callback even though it requests HBITMAP output.


#### Active compatibility validation checkpoint (2026-10-23)
Springer: registry parent creation fix delivered separately; native 8s run stayed
alive and rendered 987 frames. Kursk: nested CAB extraction fix delivered; real
menu visible after IMGDECMP callback/IImageRender implementation; corrected
image-decoder-v3 compiles and four IMGDECMP unit tests pass. Street Duel: HSS
memory WAV/MOD decode and stable real channel vtables delivered; music capture
3,217,536 frames, peak 18,938; four HSS tests pass. Further menu interaction under
validation. Stratagus: empty default CE command line fix delivered; no more
"too many files". Subsequent 240x320 probe hit framebuffer overrun at 0x78026000;
user explicitly requested STOP TESTING STRATAGUS and will test it themselves.
Do not rerun Stratagus. Snails: added as fifth requested fix, reproduce Missions
loading and repeated audio with supplied original CAB; work still pending.
Standalone corrected archives are in deliverables and saved persistently.


#### SaveDC and RestoreDC (2026-10-23)
Save full modeled DC state, including selected objects and clip rectangles, on
a per-DC stack. Positive restore levels are absolute; negatives are relative.
Consume the restored level and all levels above it. Invalid handles/levels fail
without changing live state. Deleting a DC discards its saved levels. No pixel
copy or game-specific handling is involved.


#### Live wave audio during dialog initialization (2026-10-23)
Snails mission loading reproduced an identical 3072-sample loop for seconds
with an AudioTap device clock. WM_INITDIALOG blocked worker scheduling while
waveOut continued playing. Dialog init/default continuations now park with
the owning guest thread at every existing scheduler context switch. Restore
frame, HWND, nested stack and default-dialog frames together. Another thread
must never consume the suspended caller's continuation. Other callback guards
remain in place. Validate with device-clock capture, not only silent/headless
virtual completion. Initial fix delivered before validation as requested.

Snails validation checkpoint: native original fmodce.dll, Missions/story/mission/
intro taps at 5/8/11/15 seconds. AudioTap drained in a separate host thread every
10ms at guest format. Baseline captured exact repeated 3072-sample fragments for
about six seconds. Dialog ownership fix plus live ring refresh at bounded
scheduler polls markedly reduced repetition; a shorter repeat remains around
the transition in the unoptimized headless run. Do not claim complete removal.
After changes: real gameplay reached, 1152 frames in 34s, no crash. Full winceapi
unit suite: 513 passed, 0 failed, 4 ignored. Includes new interleaved main/worker
nested-dialog continuation test. Kernel worker normal-return paths also restore
the main dialog state. Diagnostic probe code is not a production deliverable.
Use latest cumulative snapshot; older archives overwrite common source files.
Next: compare remaining brief repeat on optimized/device run and determine if
native FMOD is decoding longer than its ring or waiting on a guest timer. Do not
silence playback or invent PCM to conceal starvation.


#### Installer profiles checkpoint (2026-10-23, new attachments)
Current source restored from crates(9).zip. New generic profiles: Wise sequential
raw DEFLATE streams with per-stream CRC and executable/script/bitmap sizes;
PocketSetup PS2 16KiB chunks with signed compressed/raw lengths. Nested CAB
selection remains shared; Anthelion2 contains QVGA and VGA ARM CABs. Ignore
instruction constants resembling MSCF by validating structural header before
parsing a PE-embedded candidate. ChristmasRush Wise stream extraction confirmed
through exact EOF/CRC in an independent inspection; Tots'n'Togs reconstructed
CAB exactly 2221508 bytes from 136 blocks including raw blocks. Rust validation
pending at first delivery. Diesel ZIP includes ARM/MIPS/SH3 cabinets; do not
silently select one architecture. Runtime investigation and new APIs pending.


#### Requested CE mouse injection and wave ID (2026-10-23)
mouse_event queues real movement/left click events through the existing stylus
path, normalizes absolute 0..65535 to current guest display, clamps signed
relative movement, and emits right/middle/wheel messages with button modifiers.
CE dwExtraInfo is documented unsupported. Pointer acceleration is neutral in
the modeled device. waveOutGetID validates a live HWAVEOUT and writable UINT,
returns the concrete single output ID zero (including WAVE_MAPPER opens), and
rejects invalid/closed handles without writing. Initial delivery before tests.
Anthelion2's outer CAB contains all-zero placeholder CAB files and files.dat /
parts.dat. Implemented shared 512-byte block reconstruction after reading the
packager's x86 decoder: files.dat = u8 count + count*64 NUL-terminated names;
parts.dat = 512 data bytes, u8 reference count, then u32 references where low
byte is target index and high 24 bits are byte offset. Reconstruct memory only,
validate indices/bounds and parse resulting real CABs. Two actual import choices
now verified: Anthelion 2 Beta and Anthelion 2 VGA Beta. Christmas Rush and
Tots'n'Togs actual imports verified. Desktop CAB-only ZIP distributions now enter
the same architecture choice route; portable ZIPs with guest executables/assets
retain the ordinary importer. Initial new ZIP choice delivery before validation.
Validation: library suite 117 passed; actual Anthelion2 QVGA and VGA choices
both import; Christmas Rush and Tots'n'Togs import. Diesel desktop ZIP exposes
SH3, MIPS, ARM choices; ARM imported to diesel-library, install_dir from CAB is
\\Program Files\\Games\\Diesel FPS Technology DEMO\\. mouse_event targeted test
and waveOutGetID targeted test pass. Native CPU build preparing runtime probe;
first 9-job GCC run failed on large MIPS64 objects; rebuilding with two jobs.
Do not change production compilation settings to hide host test environment.
Mouse injection follow-up: right/middle/wheel now share pending_input with
left/movement, rather than a separate posted-message queue. This preserves
ordering across calls and samples button/modifier state when each event is
consumed. A MouseMessage input variant carries auxiliary events; native WinCE
routes it through existing pointer targeting and SDL maps it to real SDL1.2
button/wheel events. No frontend remapping/profile changes. New delivery
before rerunning regressions. Earlier API v1 is superseded.


#### Final validation / Diesel cause (2026-10-23)
Diesel legacy ZIP import experimentally selected fpsdemo.SH3.CAB (first archive
member), executable PE machine 0x01A2, despite ARM and MIPS variants. New desktop
CAB bundle preparation exposes all choices; selecting ARM produces machine
0x01C0 and DieselFPS.exe. Native ARM probe with CAB's real install prefix ran
18 seconds and reached the 3D scene (no crash, no process exit). No game-specific
runtime or graphics adjustment. Existing SH3 library entries are not rewritten;
user must reimport the ZIP and choose ARM.
Validation after ordered mouse input change: winceapi 515 passed, 4 ignored;
additional SDL auxiliary mouse/drag/wheel order test passed. Library 117 passed,
actual three installers imported including both Anthelion variants. Android JNI
host Rust check --no-default-features passes after handling auxiliary pointer
events in the managed left-stylus-only mini-game path. This is a type check,
not an Android APK/device run. Native probe and import helper are diagnostics;
do not ship diagnostic root Cargo/workspace changes or their game data.
Latest grouped delivery supersedes initial installer/API interim archives.


#### Indexed DIB writeback and scrolling audio (2026-10-23)
Native timing isolated the dominant map-scroll stall to ExtTextOutW(ETO_OPAQUE,
len=0) on a 720x960 8-bit DIB: drawing ~12us, guest readback ~1.5ms, palette
writeback ~0.8s. encode_pixels_to_dib scanned all 256 colours for each pixel.
Use a local RGB565->palette-index cache, preserving nearest-colour distances,
first-index ties and palette changes. Exhaustive 65536-colour regression test.
The experimental asynchronous wave callback change was WITHDRAWN after a native
run crashed. Restore original CPU, kernel and callback scheduling. Do not apply
interim audio-driver v1/v2 or audio-GDI v3 over this final source. Keep only the
GDI optimization; no title-specific path or changed guest audio semantics.
Final native 40-second replay reached the map, scrolled right 24-30s and left
32-38s, exited normally. Host audio sink drained at real-time 22050Hz mono.
During map window 21-39s, largest scheduler sampling gap fell from 988.9ms to
30.4ms; no queue starvation observed after optimization (baseline estimated
2.30s of underrun from queued samples and scheduling gaps). Simulated native
sink, not a physical Windows/Android audio-device validation. Both buffers
continued requeueing with original callback delivery. Final change is GDI only.


#### GAPI unfinished draw presentation (2026-10-23)
Tots'N'Togs imports GXBeginDraw/GXEndDraw and renders its menu in guest memory.
Raw framebuffer polling and memcpy/memset presentation did not respect that
interval, potentially publishing partially rendered frames. Track gx_draw_active
from successful BeginDraw until completed EndDraw (including display waits).
Skip raw polling and bulk-write presentation during it; close clears ownership.
Keep polling/direct memcpy fallback unchanged outside GAPI drawing. No game branch.
Initial native capture reaches main menu; visible whole-screen flicker has not
been reproduced conclusively on this host. Validate before calling it resolved.
Validation: 517 winceapi tests passed, 4 ignored; kernel regression verifies
unfinished guest pixels stay unpublished while raw polling resumes afterwards.
Bulk-copy regression covers both DirectDraw and GAPI locks. Native 10s run
reaches the menu and animates normally; no crash. Physical-device flicker
resolution still requires user confirmation.


#### Persisted orphan registry keys / Springer follow-up (2026-10-23)
Prior create_key/set_value parent fix did not repair already saved snapshots.
Frontend skips CAB values already persisted, so even reimport left LevelPacks
absent while LevelPacks/DefaultPack existed; Springer exited without missing APIs.
Reproduced with actual freshly imported game + old-format orphan snapshot.
Fully parented same values launch normally. configure_persistence now restores
missing ancestors of every merged key before RAM quota charging and publication.
Preserve values, canonical lookup and available ancestor display spelling. No
new game values, game check, registry deletion or reinstallation requirement.
Springer follow-up validation: same orphan-key snapshot exits before correction
(frames=1), then reaches Select GAME and START LEVEL and stays active through
8s / 1003 framebuffer changes with correction. Native CPU, simulated audio sink.
15 registry tests pass, including orphan parent enumeration, unchanged saved
values and spelling, persistence round-trip, and existing quota transactions.


#### ShellExecuteEx CE executable launch (2026-10-23)
New shell_execute child module implements 60-byte Unicode CE SHELLEXECUTEINFO,
NULL/open verbs for .exe through the same queue_child_launch as CreateProcessW.
Real asynchronous loader acknowledgement before success; hInstApp/error,
GetLastError, SEE_MASK_NOCLOSEPROCESS, temporary thread/process handle lifetime.
NO_UI accepted, unsupported masks/document associations return errors; the
unsupported CE current-directory field is ignored,
never success stubs or host process execution. Ignored optional CE fields remain
ignored. Standard launch visibility is used; nShow variants are not implemented.
MSRW Kart unregistered startup asks to execute bundled regprog.exe after OK.
No registration bypass or invented license values. Initial source delivery before
validation as requested. Desktop/Android runners already service child requests;
headless diagnostic requires explicitly enabling and servicing launches.
Final validation: 520 winceapi tests passed, 4 ignored (direct test binary with
workspace TMPDIR). Three new shell tests cover requested/temporary handles,
loader failure and ignored CE lpDirectory with real missing-file resolution.
Actual MSRW Kart click OK queues its bundled regprog.exe with empty arguments;
headless child runner loads it with shared handle/VFS context, acknowledges load,
parent exits normally, child shows 'Welcome for registration' and Start button,
remains active for 8 seconds. No game activation attempted. Diagnostic runner
changes are not production files. Shell document associations and special nShow
visibility variants remain explicitly outside this implementation.


#### Wave output capabilities / Spirit demo (2026-10-23)
waveOutGetDevCaps wrongly advertised dwFormats=1 and wChannels=1, although the
mixer accepts PCM8/16 mono/stereo at 11025/22050/44100 Hz. Advertise the twelve
standard flags (0x0fff), stereo capacity=2, preserving size-limited CE layout.
Spirit opens waveOut successfully, gets its device ID, then checks dwFormats
bit 0x80 (WAVE_FORMAT_2S16) and exits(0) if absent, before graphics initialization.
No game patch, fake audio or changed sample decoding. Source saved before tests.
Spirit validation: native 45s run passes loading (~13s on this host), renders
animated 'scene spirit resurrection force 2003', sustained ~59 framebuffer
changes/s, two waveform buffers requeued. Simulated real-time 22050Hz stereo16
sink captures nonzero music (peak 12565). No physical audio-device validation.
520 winceapi tests passed, 4 ignored. This is capability metadata correction,
not a change to samples, callbacks, GAPI or rendering. Loading is not immediate.


#### Quake3 performance investigation (2026-10-23, no shipped runtime change)
Original supplied Q3CE reaches menu. Opt-in temporary dispatcher/CPU timers:
Baseline sequential GPU-requested (llvmpipe host) 30s: 1,900,190 dispatches,
22.983s in CPU run calls (includes stop/start), 4.099s in API handlers.
Steady menu eglSwapBuffers 10-20s:60; 20-30s:60 =>6fps. Huge memcpy count,
most hot rows len4 from UIARM at 0x30005e58..0x30006034 and elsewhere.
Existing native_thunks BYTE_COPY only handles length1, all other lengths trap.
Isolated diagnostic replaces that one-byte path by 4-byte LDR/STR (NOT a complete
production optimization). Same original game/config, same profiled host backend:
10-20s:346 swaps=34.6fps;20-30s:385=38.5fps; menu visually intact. No in-game
measurement, no physical GPU/Android/SteamDeck comparison. Do not generalize fps.
Strong evidence of tiny memcpy trap overhead; implement bounded small-copy path
preserving length1, overlap/memmove, unaligned/page/protection/Thumb behavior,
framebuffer notification and bulk host fallback before delivery. Further costly
cases: len16 packing at Q3CE0x7478c; ~529 glDrawElements per menu frame.
The experiment and all instrumentation were REMOVED from production sources.
Reproduction snapshots/logs archived separately; do not apply them as a patch.


#### Production small-copy path (2026-10-23)
Implement 1-byte and 4-byte memcpy/memmove paths without changing THUNK_STRIDE.
Address-aware native_thunk_at branches to a shared helper at tick page+0x100.
Tick page executable; tick data at offset0 unchanged. Out-of-range branch falls
back to original one-byte native thunk. Four-byte aligned load/store, unaligned
byte gather before stores, overlap-safe; no reads beyond four bytes. Callee-saved
registers/stack untouched, BX returns to ARM/Thumb. Other lengths still trap+28.
Use address-aware emitter in root imports, dynamic exports and runtime DLLs.
Generic change, no Quake/title-specific branch. Tests optional unicorn-tests
feature in pocket-kernel, actual native execution rather than encoding snapshots.
Integration correction: runtime DLL thunks live around 0x30000000, outside ARM
B reach of the root helper. Reserve 80 bytes after each executable DLL import
pool and emit the helper there; pool mapping/unload/rollback owns its lifetime.
New execution tests include slot0x30000000 with nearby helper. Original v1
fell back safely but missed DLL speedups. v2 passes 520 API and native tests.
In-game profile identified heavy 16-byte copies as well as UI 4-byte copies.
Extend bounded native path to 0..16 bytes: aligned4/16 use word transfers,
remaining lengths/unaligned addresses use overlap-safe directional byte loop.
1-byte shortcut retained, >16 bytes keep original arguments and +28 host hook.
Helper 136B, reserve256B per executable DLL pool. Native tests cover every
length0..16, all source alignments, overlap both directions, pages, ARM/Thumb,
static/dynamic/far DLL thunk addresses. Three native tests pass (expanded matrix).
Final in-game comparison: actual single-player q3dm1 reached via menu, same
scheduled inputs, original supplied game/configuration; sequential125s runs.
Count only successful eglSwapBuffers returns (exclude scheduler retries).
Common60-80s window, same scene, still-alive player: baseline159 swaps/20.004s
=7.9484fps; optimized206/20.001s=10.2995fps (+29.58%). Later deaths/movement
change view; do not use later faster windows to claim comparable gains.
Live bots, not deterministic timedemo; host GPU is llvmpipe, dev optimized,
240x320. Not physical PC/Android/SteamDeck performance. Both runs exit normally
at requested125s. Profile probes and baseline-disable option removed from all
production code. Menu gain must not be presented as in-game gain.
Final native tests cover lengths0..16; 520 API tests pass,4 ignored.
Final checks: 266 kernel tests, expanded native small-copy matrix, and Android
JNI host Rust check --no-default-features pass; not an APK/device test. Non-ARM
runtime DLL imports retain the original emitter when no local ARM helper exists.


#### Numeric conversion acceleration (2026-10-23)
New native __i64tod/__u64tod helper: convert exact low32 and signed/unsigned high32
to doubles; high*2^32 is exact, sum rounds once to nearest. Temporarily set RN,
disable guest FP exception enables, restore complete FPSCR afterwards. Only
caller-saved integer/VFP temporaries touched. Root shared tick-page helper and
nearby runtime DLL helper pools now reserve512B; signed offset+0x100, unsigned
+0x180. Static imports and GetProcAddress use same native branch emitter.
__dtoi64/__dtou64 use existing CPU-internal register-only arithmetic loop; no
kernel/API scheduling round-trip. Preserve current handlers' truncation,
saturation/NaN/unsigned-nonfinite behavior. Generic, no game path. Initial
source saved before tests. Native edge/random/ARM/Thumb tests pass (2 tests),
CPU 19, kernel 266, API 520 (4 ignored), Android JNI host cargo check passes.
Two 100-second Quake3 q3dm1 bot gameplay runs complete without crash. No
isolated FPS gain claimed: different player death times invalidate comparison.
Controlled 100000 conversions: __i64tod 132ms->9ms; __dtoi64 144ms->28ms.
These are helper microbenchmarks, not expected whole-game FPS gains.



#### Bounded native memset (2026-10-23)
Generic coredll memset shares the existing import/GetProcAddress hybrid loader:
0..16 bytes branch to the nearby ARM helper at copy-helper+0x90. Zero length
returns without touching destination, byte stores handle unaligned addresses,
truncate value to unsigned char and preserve returned R0 destination. No extra
writes or reads. Above16 bytes branch to the existing hooked tail at slot+28,
with original R0/R1/R2 intact; bulk buffers keep host filling. Out-of-branch-range
slots use a one-byte native fallback, other lengths keep dispatch. Helpers fit
existing512B reservation; no new mappings or per-call allocations. No game checks.
Native Unicorn tests cover all lengths0..16, alignments, values, page boundaries,
ARM/Thumb, callee-saved registers, zero length with NULL, memory protection,
and unchanged arguments/memory on large-length fallback. Six native tests pass
including previous copies and numeric conversions. No whole-game FPS claim;
user will confirm hardware performance.


#### CI and release 0.3.6 (2026-10-23)
The CLI ScheduledInputHook must explicitly handle InputEvent::MouseMessage in
its exhaustive update_held match. These injected right/middle/wheel messages
do not represent the scheduler's tracked keys or left-stylus presses; retain
held state and continue delivering them through the ordered input queue.
The supplied CI failed both CLI builds on that missing enum variant, and
rustfmt on gdi, installer validation, datetime, imgdecmp and module order.
Format the affected files; do not remove the enum variant or weaken CI checks.
Workspace Rust version and Android versionName are 0.3.6, Android versionCode10.
The delivered Cargo.lock changes only the eleven local workspace versions;
third-party dependency entries/checksums remain those of the supplied source.
Validation: rustfmt source check and 19 CLI tests both without default features
and with unicorn pass. Full audio/display build is not validated locally due to
missing ALSA. Retry native build with one job succeeded after assembler error.


#### RAR SFX and InstallShield 5 installers (2026-10-24)
Spawn.exe is a RAR 3 SFX containing InstallShield 5 header/volume sets, whose
payload is a standard PocketPC CAB. Detect archive signatures, never filenames.
The installer pipeline now reuses the existing rars dependency to decode RAR/SFX
in memory and feeds the same nested-container logic as ZIP. Do not execute the
Windows stub or its script, and do not require external unrar/7zip/UPX tools.
Preflight every RAR member (including unwanted solid-archive dependencies):
shared expanded-byte budget, <=4096 entries, reject encrypted/split entries;
bound writer output to declared member size and preserve decoder CRC checks.
Archive paths are metadata only; no archive-named host files are written here.
InstallShield 5 uses indexed 58-byte descriptors after the directory index table,
32-bit sizes/offsets, associated same-stem header/volume, and MD5 at descriptor
+42. Versions6..13 retain their existing87-byte descriptor path. Verify volume
version, entry bounds, supported flags, split-volume boundaries, decompressed
size and MD5. Bounded Deflate blocks use the existing decoder; old compression,
split, obfuscated and linked variants remain explicit errors. Format reference:
MIT Unshield; retain the existing docs/third-party notice. Multiple recovered CE
cabinets continue through PreparedImport.cabinet_choices; never choose by game.
Validation: Rust1.90, locked original dependencies, all121 pocket-library tests
pass, including v5 stored/Deflate, index/split/MD5 negatives and RAR SFX/CRC/size
checks. Original supplied Spawn.exe yields Spawn.PPC_2577.CAB (10382599 bytes),
MD5 ddb62747d246f1d002c280de2b193968; normal Library::prepare_import/apply import
in a new temporary library creates the Spawn entry. rustfmt1.90 check passes.
No game launch or Android/Windows cross-compilation claimed; tested import only.


#### CAB recovery, VISE, NSIS and per-game identity (2026-10-24)
Generic import fixes: uncompressed CAB recovery now accepts up to64 inconsistent
CFDATA checksum words (supersedes the previous single-checksum restriction).
Change only checksum metadata in the in-memory recovery copy; strict extraction
remains strict. Reject compression, reserves, spanning, invalid bounds or more
than64 mismatches. Tomb Raider's supplied CAB has two such stale words.
VISE ESIV containers have checked PE overlay/footer boundaries; decode their
word-swapped Deflate with 16-bit stored-block alignment and odd-byte padding.
Probe only within recognized containers; bound scans, outputs and CAB counts.
Legacy solid NSIS bzip2 uses checked length-prefixed blocks after its script
header. FH_FLAGS_UNINSTALL=1 does not imply another CRC; NO_CRC=4. Verify the
installer CRC from byte512. Reuse newtua-nsis0.1.1 via a local path patch adding
only a bounded public decoder; preserve its existing high-level reader behavior.
Radium yields ARM and MIPS CABs; route both through existing user selection.
Never use a game name, fixed payload address or execute host installer code.

Remove the artificial32 waveOut stream cap: kernel AudioMixer already owns
software streams dynamically, independent of host physical voice count. POPHD
mistook the33rd refused waveOutOpen for MEMORY FULL. Confirmed35 successful
opens after the change and progression to language/sound prompts, not gameplay.

PocketPC per-game settings: pocketpc_owner_name, pocketpc_device_name (ASCII
PresetID), pocketpc_device_id (hex PlatformID). Empty fields inherit device
configuration. Desktop and Android expose fields only for PocketPC and apply
them only at non-Gizmondo launches. Registry volatile overrides are shared with
child views and copied independently by isolated_copy. Reads, open/enumeration,
writes/deletion see overrides; flush serializes only the persistent device store.
Virtual keys must not be inserted into device defaults or persisted. Owner is
exposed through the existing ControlPanel/Owner value convention; DEVICE_ID is
encoded by existing DeviceIdentity and consumed by the HAL handler. Do not
confuse raw PlatformID bytes with a game's derived registration code.

Final validation: CAB17, kernel267, library128, WinCE API521 tests pass (4 ignored).
Vendored NSIS decoder44 tests pass. Core identity integration test and Android
JNI host check are logged separately. Full Windows build/APK/UI and complete
POPHD gameplay are not claimed validated here. Keep Cargo.toml/Cargo.lock and
vendor/newtua-nsis together when applying this cumulative patch.


#### HSS MOD pitch and PocketPC GAPI button codes (2026-10-24)
Protracker sample playback used the full7.09MHz PAL system clock instead of
Paula's half-clock3.5468946MHz, raising pitch one octave without changing row
timing. Fix only the sample clock; preserve mixer output rate and module tempo.
Measure absolute rendered PCM tone at22.05/44.1/48kHz and unchanged duration.

The four PocketPC GAPI buttons must be VK_APP1..4 (C1..C4), matching frontend
hardware inputs. The previous table incorrectly replaced P1 with Return and
advertised D2/D3/D4 for the other buttons. Whitelisting clients such as Street
Duel then accepted Return and directions but rejected every PocketPC APP key.
For the PocketPC profile (physical action key=Return), report C1/C2/C3/C4 in
both portrait and landscape. Keep Return independent and do not remap it merely
because an application queries GAPI directions. Gizmondo/legacy's D1..D4 table,
input behavior and button layout remain unchanged.
GAPI supplies four button entries: Street Duel's Change menu accepts those four
and the arrows. Keyboard Ctrl/Space/Shift/Tab/Escape and the separate Return
are not necessarily bindable in that game; do not patch its whitelist.

Historical user-confirmed mapping (superseded by the [current profiles](FRONTENDS.md#factory-input-profiles)): R2=Action, L2=P1, right stick click R3=P2,
R1=P3, left stick click L3=P4, L1=ESC. Face buttons A/B/X/Y keep Ctrl/Space/Shift/Tab.
Move ESC off View long press for PocketPC; View short still toggles fullscreen.
Preserve Gizmondo's mappings including View long secondary key. RightThumb and
LeftThumb are the backend names for actual stick clicks, not stick directions.
Checkpoint saved before tests; final validation logs accompany the delivery.

Reference checking also found fractional sample phase was discarded on every
tracker loop, slightly detuning short samples. Preserve fixed-point phase and
overshoot modulo loop length; do not alter loop bounds or tracker effects.
The absolute tone test now requires the PAL target within1.5Hz at three output
rates. Real menu MOD is independently compared with libopenmpt (no dependency
added to PocketHLE). Actual before/after input runs use the same keys and taps.

Verified after delivery: Street Duel's Change screen accepts P4 for acceleration,
P1 for brake, P2 for handbrake and P3 for pause using identical before/after tap
and key sequences (before rejected all four). Kernel269/library128 Steam Deck/
WinCE API521 tests pass, with4 API tests ignored. HSS regression tests rerun after
phase preservation. Host Android JNI check and rustfmt pass. Absolute reference
tone changes512.760Hz ->258.984Hz (PAL target258.973Hz). Menu MOD spectrum against
libopenmpt improves0.7847 ->0.9569 without transposition; not bit-exact replay.
Full Windows GUI, APK and physical Deck controller not run in this environment.


#### User layout integration and callback-only IMGDECMP (2026-10-24)
User supplied complete keybindings.rs and app.rs supersede the previous Deck
layout. PPC originally used L1=P1, R1=P2, L2=P3, R2=P4, L3=ESC, R3=Action.
The current mapping and host gestures are maintained in the
[frontend factory profiles](FRONTENDS.md#factory-input-profiles), not this
dated integration snapshot. SDL Guide is normalized to Mode; custom profiles
retain those stable backend names.
IMGDECMP: phBM and ppImageRender can both be NULL when pfnImageProgress consumes
the image. Validate the callback as an output sink; retain the temporary DIB
and renderer during the synchronous callback and release afterward unless the
guest AddRefs. Tomb Raider supplies this exact60-byte form for GIF backgrounds.
New synthetic GIF test checks callback delivery,24-bit BGR bits and cleanup.
Checkpoint saved before game verification; SFX investigation remains pending.


#### HSS voice ABI, Microsoft ADPCM, MFC CE3 allocation (2026-10-24)
The original bundled HSS DLL confirms open(rate,bits,stereo,musics,sounds):
1..3 music channels,8..64 sample channels, music channels first in global ids.
Success returns0. Previously swapping the counts, ordering sounds first and
returning wrong global ids left one SFX slot and caused clients to mis-handle
sample channels. playSound returns music_count+sample_index; playMusic returns
music_index. Flag0x10000000 is automatic/free selection; otherwise low16 bits
select a category-relative channel. Flag0x20000000 starts paused. StopMusics
must stop the individual music groups, not only the unused legacy group id,
and must not stop effects. Do not stop all music slots before selecting one.
Tests exercise16 looping effects beside one music stream, full-pool failure,
global channel ids and independent stop behavior. Original DLL is evidence for
ABI only; no native DLL or game file is redistributed in the patch.

Shared PlaySound/HSS decoding now accepts RIFF MS ADPCM tag2 (4bit, mono/stereo).
Honor block predictors, per-channel deltas and histories, custom coefficients,
nibble order, samplesPerBlock and fact length. Bound decoded output, validate
chunk/block/coefficient sizes. Signed prediction uses arithmetic >>8; division
by256 incorrectly rounds negative values. Mono/stereo reference signals decode
bit-identically to libsndfile (10000/20000 samples); no runtime decoder dependency
added. The supplied Street Duel has one previously rejected MS ADPCM SFX and
20 sample effects in total, plus its MOD music. Captured in-game trace includes
sample playback and subsequent engine channel frequency changes while driving.

MFCCE300 ordinal315 is the size-only global C++ operator new allocation export,
used by Spawn after reading array counts. Resolve to ??2@YAPAXI@Z for this DLL
version only and reuse the real common CRT allocator for name and ordinal calls.
No guessed success pointers and no game-name branch. Test writable allocation,
CRT delete interoperability and allocation failure. Four Spawn calls now return
writable blocks and startup reaches its Registration dialog. Supplied key was
entered during diagnostics; activation/gameplay was not confirmed.

Tomb Raider verified with its real CAB install prefix (the first diagnostic
mount was incorrect and stopped before decoding). GIF logos, title/menu image
and attract-mode3D are visible with the callback-only fix. Do not mistake a
probe's missing mount for an emulator resource bug. Diagnostic probe sources and
coordinates are not production changes. No change to the Gizmondo input table.



#### Native GDI completion and classic ARM image permissions (2026-10-25)

MiniDogfight's mission paints a child window backing bitmap through BeginPaint,
BitBlt and EndPaint. The pixels changed while frame_counter stayed at 32143:
periodic headless snapshots moved, but desktop/Android skipped publication because
they gate snapshots on frame_counter. composite_window_layers must preserve the
counter for ordinary overlay polling, but explicit present_window_surfaces with
visible native layers must advance it. Hidden layers and overlay-only passes do
not announce another frame. Regression covers native paint, polling, repaint and
hidden-window completion. Native replay reaches Exercise and updates scenery and
frame_counter together. No title-specific presentation path.

CrazyKart crashed after character/championship selection, at the start of the
race, with FETCH_PROT at 0x00580000. This is its valid ARM DATA image section,
not its stack. IMAGE_SCN_MEM_EXECUTE is absent on that readable/writable section.
ARMv4/v5 classic WinCE has no independent XN permission: readable image pages
can contain instructions. image_memory::image_section_protection centralizes the
effective image mapping for the EXE loader and loaded DLLs. The rule applies to
WINDOWS_CE_GUI ARM/THUMB images with IMAGE_SCN_MEM_READ; explicit execute still
works. ARMNT, MIPS and non-CE images retain explicit execute flags. Write access
is never added by compatibility. PE flags/file bytes, heaps/stacks and
VirtualProtect semantics are unchanged. ARM ARM DDI0406C.d D12.6 describes the
XN permission introduced with VMSAv6. No name/address/signature condition.

Native 60-second replay passes the former CrazyKart crash and reaches an animated
race (lap/timer visible); MiniDogfight Exercise also remains animated. Initial
source is preserved with original imported files and registration data. Generic
replay source/scripts and bounded traces are in the delivery's diagnostic/ tree.



#### Unmasked IMGDECMP RGB555 and shared desktop controllers (2026-10-26)
IMGDECMP's public 16-bit BI_RGB DIB uses RGB555. Its guest bmBits and
dib_rgb555 metadata must agree; the internal RGB565 cache is expanded from
the same quantized 555 samples. GAPI and explicit BI_BITFIELDS 565 are separate
and unchanged. MSR reads bmBits and explicitly widens 555 before its colour
lookup: supplying 565 produces a red index up to63 for a32-entry table.
Tests cover public bits, padding, bottom-up rows and cache consistency.

The standard desktop launcher now uses the same in-game controller overrides
and host View/Menu gestures as the Deck executable. Do not gate controller
behavior on steam-deck; that feature still selects release/config identity
and Deck display fitting. Startup migrates exact old factory maps only via
migrate_desktop_gamepad_layout; customized maps and Android defaults are
preserved. The shared preset remains explicitly selectable in controller
settings. SDL already normalizes by physical position, including Switch Pro:
South=Switch B/Xbox A; East=Switch A/Xbox B.

PocketPC Android portrait uses the landscape thumb size with exchanged screen
axes: min(140dp, portraitHeight/5, portraitWidth/3). Keep both D-pad and aim
stick equal; deckH automatically lifts the two action rows to preserve thumb
clearance. Landscape geometry, guest resolution, inverse touch mapping and
external-controller bindings are unchanged. Gizmondo already uses140dp in both
orientations. Pure Kotlin checks cover rotated size parity, bounds, separation
and QVGA/VGA centre touch mapping.


#### Legacy Inno Setup import (2026-10-26)

The installer reader recognizes the checked legacy Inno loader pointer and
2.0.11 metadata layout, verifies both CRC-framed header streams, and reads
the 41-byte file-location records. Internal single-slice zlib chunks are
validated against their declared sizes and Adler32 before ordinary recursive
CAB detection. Never execute setup scripts or desktop payloads, write their
paths to the host, or guess another Inno version's record layout. External
slices and unsupported versions return explicit errors. The supplied Leo's
Flight Simulator installer yields one 2,624,822-byte CE CAB.


#### Child-process installation namespace (2026-10-26)

Desktop/Deck and Android preserve a game-relative executable reference for
CreateProcess children within the same installation. The resolved host file
is already canonicalized by the launcher; external executables retain their
absolute reference. Do not store an internal child as an absolute GameEntry
path: executable_under_prefix then cannot derive guest_launch_paths, losing
the installation and writable-save mounts. Mini Kayak relaunches itself with
ARG:goNow after Start the adventure, enumerates resources/scenes/*.properties,
and otherwise copies a NULL circuit name. Individual asset basename fallback
can hide the missing mount, but enumeration correctly requires a directory.
Do not change wcscpy NULL semantics or invent a circuit list to hide this.


#### Retained GAPI display memory in event loops (2026-10-26)

GetMessageW/PeekMessageW publish a retained active GAPI framebuffer on the
main thread before the next message/callback. Mini Kayak obtains display
memory once, copies scanlines from its timer callback and does not end the
draw or Sleep per frame. Keeping readback exclusively at GXEndDraw/Sleep
left its game view black despite completed guest writes. Reuse the existing
held-surface readback used by Sleep; do not clear gx_draw_active, add frame
waits, or expose partial images at arbitrary CPU slices or scanline copies.
Non-GAPI and worker message pumps are unaffected by that helper's guard.


#### Mixed CPU/GDI writes into DIB destinations (2026-10-26)

BitBlt and StretchBlt import the selected destination DIB before drawing,
just like text output. A guest can render directly through ppvBits, then
paint a small HUD using GDI. Publishing the entire stale host bitmap after
that HUD previously erased the CPU-rendered scene outside the destination
rectangle. Mini Kayak exercises this path. Keep the source readback and
post-draw guest publication; ordinary non-DIB destinations remain unchanged.


#### User-supplied MFC CE3 runtime and executable data imports (2026-10-27)

The supplied ARM SDK import library identifies MFCCE300 ordinal644 as
AfxGetModuleState; Leo imports345 MFC entries including class/message-map data.
When mfcce300.dll is present beside the executable, prefer that user's native
ARM runtime from its first call rather than mixing HLE CString/allocator/state
with native framework objects. Absence retains the existing HLE behavior. No
Microsoft DLL is bundled or downloaded automatically; use the retail ARM DLL,
not the debug DLL, import library or PDB.

KernelState.image_imports retains the executable's resolved IAT metadata. After
a native graph attaches successfully, resolve all that DLL's executable imports
(names and ordinals, data and functions) before writing any of those slots.
Imported data never calls a thunk and otherwise points at ARM instructions,
not a CRuntimeClass or message map. Existing loader relocations, dependency
order, attach failure and lifetime machinery remain responsible for the runtime.
DocList.dll is a CE OS component routed through HLE imports, like commctrl;
DocList_Create remains unimplemented and must be reported if called. Do not
require a native DocList library merely to bind MFC's unused OS import.

RegDeleteKeyW is implemented as a CE leaf-key deletion: validate a non-null
subkey and parent handle, reject subkeys and open target handles in the local
registry view, remove values/display metadata and refund store pages. Missing
keys return2, invalid handles6, invalid parameters87, non-leaf/open keys5.
Device-shared cross-process open-registry-handle exclusion is not modeled by
the existing per-process Registry handle table. Reference:
https://learn.microsoft.com/en-us/previous-versions/ms961348(v=msdn.10)


#### Invalid DIB sizes during native window initialization (2026-10-27)

CreateDIBSection rejects unsupported header sizes, nonpositive width or zero
height instead of inventing a screen-sized RGB565 bitmap. Valid V2/V3/V4/V5
headers retain their existing handling. The old fallback gave Leo a successful
bitmap for its initial zero-sized window, leaving its software renderer in an
inconsistent state after resize; raster writes then overwrote a live object's
vtable before the buffer. Returning NULL allows the original guest failure and
resize path to initialize its valid 240x136 8-bit scene. No executable patch,
buffer-padding workaround, renderer bounds override or title branch is used.
The supplied retail ARM MFC runtime plus the corrected loader and RegDeleteKeyW
reach a rendered runway and cockpit in a bounded 2,000,000-slice run with halt
on missing API enabled. This is startup/render validation, not a complete flight,
physical Android/Windows/Deck test, or audio validation.


##### Shared PocketPC runtimes and dependency information

Desktop (including Steam Deck/Linux) and Android append
`<library root>/runtimes/pocketpc` to each PocketPC process's native module
search directories, after the executable directory. Gizmondo never enables this
path or the user-facing dependency callback. Files are user-supplied, not bundled.
The existing native loader still validates architecture, relocations and exports.
Do not install system DLLs to replace missing HLE system APIs.

`WinCeDispatcher::capability_registry` constructs the same handler registrations
as `new`, without resetting GLES state. `supports_import` uses dispatch's ordinal
and name resolution. No parallel supported-API manifest is maintained. Information
screens inspect static imports only when opened; a missing registration is not
proof that the game will fail (unused imports and dynamic imports exist). Native
file presence is explicitly not advertised as verified compatibility.

The optional runtime callback is enabled only by PocketPC frontends. It runs in
the existing unresolved-call branch, reports once per DLL per process and does
not change the existing return/halting policy. Implemented API calls do no new
work. System-library diagnostics ask for an HLE implementation; other missing
libraries point to the shared runtime directory. Desktop and Android drain the
notices on their UI thread, independently of missing-API file logging.


##### Guest window captions in the PocketPC shell bar

`KernelState::window_titles` stores the guest caption from CreateWindowExW and
DLGTEMPLATE titles. SetWindowTextW/A updates it; GetWindowTextW and
GetWindowTextLengthW read it alongside native controls. DefWindowProc handles
WM_SETTEXT/WM_GETTEXT/WM_GETTEXTLENGTH for native windows. Normal metadata
cleanup removes captions, including closed modal dialogs. No game/library name
is injected as a caption. The visible PocketPC taskbar follows active_window
(and the legacy main HWND only when no active window has been set).

The shell keeps its PocketHLE icon but renders the guest caption in a smaller
font. Its text clip ends before the right-aligned clock, so long captions remain
on one line underneath the clock's reserved region. Shell visibility, window/DC
origins, client sizes, stylus coordinates and SIP layout are unchanged.

Validation: the supplied SimCity executable passes "Simcity 2000 Pocket PC"
through standard dialog templates, observed in a bounded ARM/Unicorn run at
startup and main-menu creation (not inferred from the library display name).


##### Optional PocketPC left/right touch steering

`GameSettings.pocketpc_touch_steering` defaults false and is exposed only for
PocketPC on desktop and Android. Both runners gate the per-process mapping on
the PocketPC platform. `touch_steering::TouchSteering` maps the already inverse-
rotated directional keys back to display-relative left/right, then transforms
quarter-width/three-quarter-width, mid-height screen points into guest pixels.
Up/down in display orientation remain keys. Down holds a stylus stroke; repeated
key-downs are ignored; release moves the held stylus to the center, followed
by PointerUp at the center before any later host input. Opposite directions use the most
recent press and restore the still-held other direction on release. Real touch,
rotation changes and the SIP cancel an emulated stroke. No game-specific code.
The mapping runs in the common WinCE queued-input path, before native window/
control/stylus routing, not in individual game renderers. No in-game validation
was performed for this option; the user is doing that validation.


##### PocketPC proportional left-stick touch steering

The existing per-game pocketpc_touch_steering option now enables four-direction
D-pad zones and proportional left-stick positioning. No additional setting.
Desktop and Android hide the option for Gizmondo; both runners also gate it on
!is_gizmondo. SDL controller LeftX/LeftY remain distinct analog host events;
only while this option is active does the desktop bypass digital LeftStick
bindings. D-pad bindings and right-stick velocity steering remain available.
Android uses the same normalized host-only TouchSteeringPosition event via JNI
kind 7. Both frontends release the analog gesture on focus loss/disconnect.

Coordinates are display-relative, inverse-rotated once in the common WinCE input
path. The shared TouchSteering state handles per-axis D-pad overrides, analog
18% deadzone with linear scaling beyond it, diagonals, and center move + center
up when all inputs return to rest. Full analog tilt reaches the corresponding
quarter/three-quarter screen positions used by the D-pad. Real touch/rotation/SIP
cancel the gesture; switching to the right-stick velocity path releases touch
steering before starting another stroke. No title-specific behavior.

Validation: four standalone unit tests compile the actual touch_steering module,
covering proportional response, deadzone/center release, diagonals and all four
rotations. Rust syntax formatting succeeds for changed Rust files. No full
frontend build/APK or physical-controller/in-game validation claimed.


##### Integrated capture viewer (desktop / Steam Deck / Linux)

Recordings opens captures in capture_viewer.rs, not ShellExecute/Wine/xdg-open.
The UI owns the texture and decoder workers own FFmpeg contexts and SDL audio.
One mutex-protected replaceable frame slot bounds video buffering; PCM is
44100 Hz signed 16-bit stereo and bounded to ~200ms in SDL AudioQueue. Images
use their full resolution, unlike catalogue thumbnails. Display fits the native
image/video dimensions inside the host viewport; the title bar/Close stay visible.
B/Escape/Close drops the viewer and stops its worker/audio. Leaving Recordings
also closes it. Left/right seek by 5 seconds; the slider supports arbitrary seeks.

The existing static Decoder is now public (still video-static feature gated) to
reuse the opaque C ABI; no FFmpeg structs or external media executable are used.
media.c publishes audio chunk timestamps as well as video PTS, and resets its
sample clock on seek, permitting the player to discard PCM before the requested
seek point. Existing decoder bytes/formats stay unchanged. Seek commands are
coalesced, decoder preroll is bounded per command check, and stale queued audio
is cleared before seeking. No new Cargo dependency or FFmpeg codec build option.
Standalone source/syntax inspection only for this delivery; a full frontend build
and actual video/audio/controller verification remain to be performed.


##### Recorded steering indicator

Recording observes TouchSteeringPosition separately from StylusAim. Under the
PocketPC per-game steering option it draws a proportional direction/diagonal in
the existing bottom-left arrow circle, using the same 18% deadzone. Analog axes
are already display-relative and are not rotated a second time. Held D-pad keys
are transformed back to display space and override each axis exactly as input
steering does. At rest the circle disappears. Recording start seeds held keys
and current left-stick axes. The bottom-right lookup/right-stick marker and the
live emulation display remain unchanged. Two pure-method tests pass in a
standalone harness; no full GUI/video encoding validation was performed here.


##### Capture player seeking, fullscreen and activation

Capture tiles select on one click and open only on double-click or controller A.
The player's full-width slider retains its drag value independently from the
playback position; it sends exactly one seek on release, or a deliberate click/
keyboard edit. Playback progress and slider rounding do not trigger seeks.
This avoids repeated seek/preroll cycles starving playback. Seeking publishes
its target position immediately while decoding preroll on the worker.

Full screen uses the host viewport and fits media proportionally while keeping
controls visible. Button/double-click enter or exit; B/Escape exits fullscreen
first and only closes the viewer when windowed. Drop restores the host's prior
fullscreen state, including a Deck host that was already fullscreen.


##### JIT boolean floating-point comparisons

native_thunks::float_comparison_thunk covers __eqs/__nes/__lts/__les/__gts/__ges
and their double-precision d variants using VFP VCMP and boolean R0 results.
These previously registered CRT helpers now stay in the existing native ARM
thunk path; no game-specific patch, new API table, GPU dispatch, or build option.
Each code sequence uses 7 of the 8 available ARM words; the last word is BX LR.
MI implements < and LS implements <=: signed LT/LE would incorrectly accept
unordered NaN. NE includes unordered; the other predicates exclude it. R4-R11,
SP and non-result argument registers remain unchanged; VFP scratch/condition
flags can change as allowed by the call ABI. Existing Rust fallback handlers
remain unchanged, as do unsupported helpers such as __cmps/__cmpd.

Validation executed the production Rust-generated words with Unicorn 2.1.4 on
the ARM Cortex-A15 model, VFP enabled and FPSCR=0. 14,700 operand/predicate cases
passed across f32/f64: ordinary/random values, +/-0, infinities, smallest/largest
subnormals and quiet/signaling NaNs. Return addresses and preserved registers
were checked. No Mini Kayak in-game timing or end-to-end FPS claim was made.


##### PocketPC controls modes and stylus grid (2026-10-27)

Per-game pocketpc_controls selects normal/zones/grid. Absent/null preserves the
legacy pocketpc_touch_steering bool. The setting is ignored and hidden on Gizmondo.
Both runners enable kernel touch steering only for zones. Frontends snapshot the
mode at game launch, so editing options mid-session cannot leave the host grid
and kernel steering in different modes. Restart the game to apply mode changes.

Grid input is intercepted in the host before guest bindings/rotation: unassigned
keys/buttons/directions retain their normal path. Serialized bindings identify
keyboard/controller separately, using stable controller position names. Back/Guide
and stick-click Android aliases canonicalize to SDL names. Cells store display-
relative normalized centers; convert once into unrotated guest framebuffer pixels,
independent of host window, fullscreen and upscale. Grids are 4x3,5x4,6x5; changing
size clears assignments. Editors follow the game's configured portrait/landscape
orientation. Bindings are unique per cell/input; rebinding moves an existing input.

There is one stylus, not multitouch: last pressed zone wins, release restores the
last still-held zone, last release lifts at that zone. Repeated downs do not change
priority. Focus loss, disconnect, physical touch and rotation release grid strokes.
Assigned stick directions use 18% press/14% release deadzones, ignoring strength
beyond that threshold. Unassigned right-stick aim remains available; held grid
presses take priority over velocity aim. Real screen touch takes priority over both.

Reserved Escape/F7/F8/F9/F10/F11/F12 and View/Menu controller inputs,
plus the configured controller fullscreen input, are refused by capture and runtime
lookups. Never steal these host gestures or silently route them to guest touch.
Grid capture and steering preview are local editor operations, never guest input.
Desktop preview reuses kernel TouchSteering; the Android preview uses the same
18% deadzone, proportional displacement and per-axis arrow priority.


##### Drop-down selection text, architecture-tagged add-ons and ferror (2026-10-27)

CBS_DROPDOWNLIST has no edit buffer: ChildWindow::window_text returns the selected
list item's string, or empty with no valid selection. GetWindowTextW/LengthW and
native WM_GETTEXT/WM_GETTEXTLENGTH use the same accessor. EDIT and other window
text semantics are unchanged. Racing Days calls GetWindowTextW after choosing a
skin/car: returning its empty creation caption made it open a nonexistent asset;
its 3D renderer and RLE8 textures are not the cause. No game-name condition.

A CAB's CPU tag does not establish that its installed payload contains code.
Classify structured data-only packages by their declared destinations and actual
runtime payloads. CabWiz .999 is an installer callback, not an installed game
binary, unless explicitly declared as a runtime file. Preserve add-on callbacks
under the game's private .cab-setups/<cab-hash>/ directory, outside extracted,
with their original install-dir.txt and a persisted order.txt. Do not replace the
base setup DLL or pass its install directory to an add-on for a child directory.
The common complete_cab_registry_setup service consumes these records in order
using the existing ARM registry/time-only capability gate and atomic per-DLL
markers. GUI/file/service installers remain unsupported and are not run partially.
India's CAB declares ARM and contains ADDSETUP.999, but all installed files are
track data. That setup callback only checks the installed application and requires
UI APIs outside the supported profile. Its data files can be mounted normally.
Original payload bytes, filenames, saves and settings must be preserved.

ferror reads process-local sticky FILE error indicators, independent of EOF and
Win32 LastError. CRT byte/line/scanner reads, writes and flush failures set them;
rewind and close clear them. Successful operations and fseek do not clear errors.
fputc reports EOF on failed output. These changes do not alter raw ReadFile or
WriteFile semantics. KernelState constructors must initialize crt_stream_errors.



#### Release 0.3.7 and case-preserving writable copies (2026-10-27)

Rust workspace/local Cargo.lock packages and Android versionName are 0.3.7;
Android versionCode is 11. Do not rewrite older release history.

VFS case-insensitive lookup keys must not become new names when an existing
read-only installed file is copied into its writable save overlay. Preserve the
source spelling for missing path components, respecting already existing upper
layer directories. Opening a file to modify its bytes is not a rename operation.
Racing Days registers inside NSX.car; the previous nsx.car copy changed the name
returned by enumeration. The registered NSX -> Integra -> nsx sequence reproduces
its corruption message; a direct writable ROM test missed the save overlay.
Existing lowercase overlays need a case-only rename, preserving their bytes.
No filename, title or registration bypass belongs in the runtime fix.



#### Software-renderer TLB capacity (2026-10-28)

The vendored Unicorn softmmu starts with 4096 TLB entries and ordinary shrinking
keeps at least that preferred capacity. The hard allocation-failure minimum
remains 64 entries. This is a host CPU translation cache, not a guest RAM or
framebuffer allocation and not the separate translation-block jump cache.
ARM's 1 KiB pages made buffers 2 MiB apart collide at the previous small sizes.
No memory permission, MMU mapping, code invalidation, hook or renderer behavior
is changed. Existing build.rs watches these C/header files for rebuilds.

One headless A/B used the same locally built vendored Unicorn 2.1.5 and CLI
binary, with only a temporary capacity selector: original initial256/min64 vs
initial4096/min4096. The final source retains min64 for allocation failure only.
Mini Kayak Snowfield was launched via the original WinMain command
ARG:goNow/0,1,5,1,0,unnamed. After 60 identified scene updates, 200 updates took
46.032796s vs 27.117476s, a 69.75% throughput increase. Host-side TLB profiling
was disabled during timing. This single dev/headless comparison is not a
promise of those absolute FPS in release/on Android. User gameplay validation
and full platform CI remain separate. All temporary CLI/CPU instrumentation
and runtime cache override were removed from production sources.

An earlier deferred-execute prototype was discarded: its initial game measure
did not demonstrate a gain. Do not restore it or alter image permissions.
When archiving source, exclude only the root target build directory: the tracked
vendor/unicorn-engine-sys/qemu/target directory contains required CPU sources.



#### Executable-data store experiment on fixed TLB (2026-10-28)

Separate trial, not measured or validated in game. At the user's request, they
perform tests. This trial uses the measured fixed 4096-entry cache, not the
separate adaptive TLB experiment; do not combine performance claims.

notdirty_write can clear its slow-write flag for private executable RAM only
when no translated block remains on the whole physical page. The existing
invalidation routine now returns that fact while its page collection is locked.
A bitmap miss outside one block does not imply the rest of the page is code-free.
Externally provided RAM_PREALLOC backing keeps its previous conservative policy.
Clear only the checked translation, since another MMU mode can map the same VA
to different physical code. Memory hooks and snapshot copy-on-write guards are
unchanged. No PE/VirtualAlloc
permission changes or game/PC-specific conditions are introduced.

When translating code, re-arm writes against host backing addresses, including
all virtual aliases in both main/victim TLBs, for both pages of a crossing block.
Do not omit re-arming when hooks exist. Non-RAM code retains a virtual-address
fallback. Existing guest self-modification invalidation and precise-SMC exits
remain in place; this is not the abandoned deferred-execute permission prototype.

Two native regression tests are supplied but not executed: data->code through a
virtual alias followed by writes inside/outside the translated block, and
retention of all memory-write callbacks. No FPS improvement is claimed until
the user tests the trial. Keep the supplied fixed-TLB rollback files.



#### Optional per-game presentation ceiling (2026-10-28)

GameSettings.frame_rate_limit is an enum serialized as "unlimited", "20", "30",
"40", "50", or "60". Missing means Unlimited, preserving all existing display
synchronization; do not remove the modeled 60 Hz LCD/vblank timing. Both game
profiles expose it on desktop/Steam Deck/Linux and Android. Android round-trips
this setting through the shared native JSON; recording FPS remains independent.

Core remembers the request before loading and restores it after frontend screen
replacement. Child launchers use the same per-game settings. The shared GAPI /
DirectDraw / EGL presentation deadline combines the native cadence and the
optional user cap. An explicit vblank credit or DDFLIP_NOVSYNC cannot bypass a
selected cap. Primary unlocks honor it even without the native WAIT flag. No
catch-up bursts: each deadline is based on the last completed presentation;
slow frames are not accelerated. Use existing cooperative wait_display_until
so workers, audio and frontend polling can continue; never sleep for the whole
frame interval in a frontend hook. Do not pace individual GDI drawing commands
or change GetTickCount / game camera calculations. Pure GDI and applications
that never present through these display boundaries have no added frame cap.
This ceiling changes presentation rate, not every independent simulation loop.

Mini Kayak diagnostic observed the original camera blend retaining 95 percent
of its previous position and taking 5 percent of the next target per update.
300 updates at the same deterministic dt matched exactly before/after the SMC
trial. The older engine alone changed camera following when dt changed from
100 to33ms. No sharp 20/30fps threshold was established; 20fps is a compromise
to try, not a validated cure. No game-specific patch or clock modification.



#### Retained-GAPI frame caps and per-game settings groups (2026-10-28)

Mini Kayak renders from WM_TIMER while retaining GXBeginDraw's pointer; it
publishes on GetMessage/PeekMessage, not GXEndDraw. The first cap only covered
explicit presents and therefore did not limit this title. publish_held_framebuffer
now returns an optional cooperative continuation. GetMessageW/PeekMessageW and
positive main-thread Sleep must propagate it before consuming messages or
starting a second wait. Read into scratch, wait only when pixels changed, then
publish; keep gx_draw_active and block raw CPU-slice readback as before. Unchanged
polls do not incur frame waits. Unlimited takes the original immediate path.

Sleep initializes its own deadline before the capped present and preserves it
across the shared display continuation. This overlaps waits rather than adding
Sleep(50) to another50ms at20fps, and retains the deadline across worker handoff.
No game-name condition, GetTickCount change, frontend-only FPS counter change,
or per-BitBlt throttling. OpenGL ES/EGL, DirectDraw and GAPI explicit waits stay
on the shared clock. Pure GDI still has no general cap; do not claim otherwise.

Per-game settings use Controls / Graphics / Sound / Misc on desktop and Android.
Touch controls remain PocketPC-only. Screen/GPU/filter/FPS/rotation are Graphics;
identity and API diagnostics are Misc. Sound has a noninteractive placeholder.
Android reuses the XML categories for dynamic controls/identity, without duplicate
categories or a second preferences store. Existing controller widgets/IDs stay.



#### Revised FPS scale (2026-10-28)

The per-game menu now offers Unlimited (unchanged default),10,15,20,30,45,60.
Rust/Android/core/display validation agree. No migration of the unreleased40/50
settings is provided, as requested by the user. Existing
Unicorn optimizations and retained/explicit presentation waits stay unchanged.
No new Mini Kayak gameplay test or camera-specific patch.



#### Remove GUI halt-on-missing-API option (2026-10-28)

GameSettings no longer stores halt_on_unimplemented. Old JSON keys are ignored;
Kotlin removes the obsolete key when saving its preserved original JSON. The
control, listener and resources are removed from Android and desktop per-game
UI. There is no global GUI halt switch in this source tree. GUI/JNI launch paths
no longer enable the dispatcher's halt mode from legacy settings. CLI explicit
diagnostics / kernel API remain separate. Missing-API logging is unchanged.



#### Writable filename case, host clock and desktop integration (2026-10-28)

Case folding remains a VFS lookup identity, not a host filename. exact_path
uses the normalized but case-preserving guest spelling for missing components.
Existing lower-layer copy-on-write names remain preserved. mount_save_dir also
recovers legacy case-folded file names when a same-prefix installed lower file
and exactly one case-insensitive sibling in each layer identify the spelling.
It renames only the upper leaf, retaining all saved bytes. Ambiguous siblings,
new assets and symlinks are untouched. This repairs old Racing Days registered
NSX.car overlays without a game-name branch or registration rewrite. Gameplay
has not been rerun; 55 VFS tests pass, including recovery and collisions.

Shell taskbar chrome uses the host's local time (localtime_r on Unix,
GetLocalTime on Windows), including host DST. Guest time APIs retain their
existing virtual UTC contract. The exact Unix helper was checked against the
system clock with UTC, Europe/Paris and America/New_York.

Desktop Windows binaries embed the existing logo as a multi-size ICO through
winres. The build script also explicitly exports NVIDIA/AMD high-performance
GPU request variables. These are driver preferences, not an adapter guarantee.
The global options enumerate real DXGI hardware names and open Windows Graphics
settings for the authoritative per-executable choice, requiring a restart.
Do not claim a direct WGL adapter-selection API or alter undocumented registry
values. Hide the Windows settings link under Wine/Proton. The Windows API module
passes isolated type checking; a complete Windows link and device check remain.

Native Linux uses the PocketHLE app_id and PocketHLE.desktop filename together.
tools/install-desktop-entry.sh installs a per-user launcher and hicolor PNG;
it works from a source checkout or the Linux CI archive, which includes the
script and icon. ELF file icons are desktop integration, not PE resources.
The full desktop check here stops at missing GLib/GTK development packages.



#### Windows case-insensitive existence and writable copies (2026-10-28)

Do not use Path::exists to infer a stored filename. On Windows, requesting
models/vehicle.dat succeeds even when the real entry is Models/Vehicle.DAT.
The previous resolver returned that requested lower-case path, so copy-up and
overlay case recovery both copied the wrong spelling despite passing on Linux.
host_path_for_mount now obtains actual directory-entry spelling for existing
components, prefers exact entries over folded siblings, and retains requested
spelling only for missing components. No game names or registration edits.
Two additional regressions cover stored path spelling and exact-case sibling
selection. All 57 VFS tests pass on Linux. Windows/gameplay are not claimed as
executed; the stored-spelling test specifically exposes the old Windows failure.



#### InstallShield 5 raw CabWiz INF packages (2026-10-28)

installer::installshield first preserves prebuilt CAB payloads. If none exist
and a decoded INF declares CESignature="$Windows CE$", decode the original
IS5 file table with source directory indices and MD5 validation. cabwiz.rs
reconstructs ordinary Microsoft cabinets in memory, one per CEDevice variant,
using the declared SourceDisksNames/Files, CopyFiles, DestinationDirs, AddReg
(string/DWORD) and file shortcut target. No host EXE, CabWiz or script runs.
Unknown DefaultInstall directives, copy/registry flags, missing/ambiguous source
files, path traversal, oversized metadata and substitution cycles fail explicitly.
This is a bounded declarative profile, not a full Microsoft INF implementation.
Current text encodings are ASCII/UTF-8/UTF-16LE; raw source directory decoding
is IS5 only. Ordinary existing IS5–13 CAB extraction remains available.

Generated _setup.xml uses the existing long-name/destination/registry importer.
WinCeSetupScript.processor_type is optional; the installer architecture chooser
uses it only if the binary CAB header has no CPU field. The existing shared
chooser therefore offers ARM/MIPS/SH3 without frontend-specific import logic.
The supplied Strip4You installer produces three cabinets. Targeted validation
checks architecture selection, actual ARM PE bytes, data/module presence,
registry values, non-installation of desktop tools and staging isolation.
No gameplay, Windows build, APK build or physical-device test is claimed.



#### MIPS compiler runtime ordinal profile and GAPI explicit frames (2026-10-28)

COREDLL ordinals 2000..2053 are architecture dependent. Microsoft Pocket PC
2000 CE3 MIPS and Standard CE4.2 Mipsii coredll.lib have identical mappings,
but differ from ARM (2028=__fpdiv vs __lts, 2034=__dptoli vs __i64tos,
2036=__litodp vs __gts). The generated MIPS-only overlay is clean-room
integer/name data from those short-import objects; do not ship SDK libraries.
The shared resolver remains ARM by default; core loading, runtime module imports,
dynamic ordinal dispatch and static dependency information select the active
CPU profile. MIPS arithmetic/conversion spellings reuse the existing numerical
handlers. Unimplemented integer-runtime helpers retain missing-handler behavior,
not incorrect ARM aliases. ARM/Gizmondo resolution remains unchanged.

PhoenixStudio's original MIPS sine-table initialization was executed in
isolation with only imported arithmetic/sin substituted. ARM resolution gave
64 identical erroneous entries 1216348096. MIPS resolution produced the expected
64 integer sine samples exactly (0,25,50,75,...). This is not a physical audio
playback test or a claim that every synth feature is validated.

GapiDraw's Strip4You Flip calls GXEndDraw then GXBeginDraw. Its flat full-surface
BltFast branch calls COREDLL memcpy(width*height*2); copying the menu background
is not a completed frame. Remember use of explicit GXEndDraw per display session
and suppress bulk-copy publication/pacing inside that explicit draw. EndDraw
still performs the completed-frame wait/readback; close/new open reset this mode.
GXCloseDisplay is no longer a native constant because it mutates display state.
The retained-pointer full-copy fallback (GXMark, no EndDraw) and voluntary-yield
readback (including titles switching from menus to retained draws) remain intact.
Before the session's first EndDraw, the existing retained fallback is unchanged.
No game-title branch, buffer rewriting, FPS workaround or recording-only fix.



#### Persistent PocketPC system filesystem (2026-10-28)

Current desktop and Android PocketPC runners select memory_division=None;
only Gizmondo selects the quota-managed SDK RAM store by default. PhoenixStudio
reads the owner from HKCU/ControlPanel/Owner, but writes its validated serial
and padding via CreateFileW/WriteFile to \Windows\hpsdata.dll. No RegSetValue
import is used. That path had no mount or Windows directory, so CREATE_ALWAYS
failed (not merely a registry flush problem). Never inject serial/owner data.

Emulator::configure_pocketpc_storage mounts library_root/object-store-pocketpc
as the writable device root and its Windows child as a same-prefix Windows
copy-on-write overlay. It creates only standard CE directories. More-specific
install, game-save, My Documents and card mounts keep their existing priority
and read-only policy. Ordinary host-backed VFS write/flush/close provide
persistence; no shutdown-only RAM snapshot, background thread or bespoke file
redirect is involved. Existing files are never seeded/replaced. Standard child
directories may not escape the store through symlinks. A real quota-managed RAM
profile is explicitly refused by this host-store setup, not silently bypassed.

Desktop (PC/Linux/Deck) and Android configure this only for PocketPC, after
memory-profile selection and before guest execution. Gizmondo is unchanged.
The core API is available to diagnostic callers; CLI does not auto-configure it.
Three core tests cover close/reopen in a fresh VFS, exact saved bytes, game save
precedence, Windows COW, read-only cards and escaping directory symlinks. No
full Phoenix registration UI, Windows build or APK validation is claimed.



#### Device filesystem backing versus installer resource aliases (2026-10-28)

Vfs::mount_filesystem describes a correctly laid-out writable device tree.
It still serves exact paths in normal overlay order, but is not a recursive
resource search pool. Its directory evidence affects suffix lookup only at the
most-specific matched prefix, not a more-specific game's installed namespace.
All preexisting mount/mount_read_only/mount_save_dir calls retain their previous
resource fallback behavior. This is a mount-role distinction, not a title rule.

The newly seeded PocketPC Program Files directory previously made known_parent
true globally and vetoed flat-install fallback for data/foo inside another
mount. Phoenix's startup resource loads exposed this regression. The system
root and Windows backing now use mount_filesystem; game resource resolution
and overlay directory guards remain intact. Core regression fails before this
change and passes after it, including a conflicting system filename and a
real directory in the save overlay. Persistence of Windows files is preserved.
No data migration/deletion/reinstallation or licence rewrite is needed.



#### Android virtual controls in stylus configuration (2026-10-28)

StylusControlsDialog reuses PocketPcControls and PocketPcLayout in a full-window
local input panel. Selecting a grid cell opens the panel; successful capture
closes it, while external keyboard/controller capture remains available. Steering
has a touch-test button, with its preview in the layout's reserved game rectangle.
Device orientation lays out the bank, and game geometry/rotation sets preview aspect.
The preview and input callbacks never own a native session.

Virtual inputs use controller names Touch:Action/A/B/C/P1..P4/Tab/Up/Down/Left/Right
and Touch:StickUp/Down/Left/Right, separate from external controller bindings.
They use the existing serialized string namespace; no new schema variant.
GameActivity routes virtual key edges before normal keys, only for PocketPC grid
mode. Assigned stick components are consumed; other components retain normal aim.
Steering uses the existing normalized native position event (-1000..1000).
Host buttons (Esc, Turbo, Exit, Fullscreen, rotation) are not assignable here.
The existing grid router owns display-to-guest rotation; do not rotate twice.
No Android SDK/build toolchain is available in this workspace; device/APK tests
remain required. Do not claim physical Android testing.



#### Release 0.3.8 (2026-10-28)

Workspace and local Cargo.lock packages are 0.3.8; Android versionName is
0.3.8 and versionCode is 12. Historical release notes remain unchanged.



#### PocketPC GlobalMemoryStatus allocation accounting (2026-10-28)

The unpartitioned PocketPC profile no longer reports fixed 32 MiB total / 24 MiB
free physical memory. It reports mapped heap arena capacity and current free
bytes, with computed load and zero pagefile. Private virtual fields describe
the 32 MiB low slot when present, including when exhausted; high-only profiles
use their actual arena capacity. This is emulated allocator capacity, not host
physical RAM or a claim of a hardware RAM profile. Quota-managed Gizmondo/device
branches remain unchanged. The API is VOID (ReturnedR0(0)).
THPS2's original ARM startup at 0x1b90c/0x1b92c calls GlobalMemoryStatus and
compares dwAvailPhys to 0x1c00000 (28 MiB). The previous 24 MiB constant made
this reject even when the mapped allocator had sufficient room. No binary
patch, registration change, game name or unconditional success is used.



#### Desktop direct library launch (2026-10-28)

Both desktop_main.rs entrypoints accept game name/ID, display mode 0=fullscreen
1=windowed, and filter index 0..7 in UpscaleFilter::ALL menu order. No arguments
retain the launcher. Resolve exact ID first then unique case-insensitive display
name; ambiguity/errors fail explicitly before opening the game. --help documents
the syntax. Reuse spawn_run and its startup/dependency prompts, not a second runner.
Initial CLI display/filter overrides do not write saved preferences. Existing
interactive settings actions retain their normal behavior. The existing CLI
package executable named pockethle remains distinct; GUI binaries are still
pockethle-gui and pockethle-steamdeck. Four pure argument/lookup tests pass;
no full native desktop build is claimed.



#### Multimedia timer callbacks (2026-10-28)

MMTimer/winmm/coredll timeSetEvent/timeKillEvent use a dedicated cooperative
guest thread per timer. Existing Begin/EndPeriod and DevCaps handlers stay.
The original timeSetEvent thunk is the worker's scheduler entry; exact worker
thread + thunk + SP identity separates it from nested callback API calls.
ARM/Thumb callbacks receive ID,0,user,0,0 with the fifth argument on the stack;
restore SP, CPSR and thread-exit continuation after return. One-shot timers end;
periodic deadlines preserve phase but skip missed intervals instead of bursts.
The shared CPU-slice/API scheduler now also services timers in CPU-only loops.

Cancellation sets a flag checked before callback dispatch; synchronous foreign
in-flight cancellation retries cooperatively until callback completion, using
a per-caller thunk/SP join key. No native host thread executes guest callbacks.
The timer's internal thread handle is closed without terminating the worker.
Timers/state belong to KernelState and disappear with the process. No game names
or original DLL/ROM bytes are shipped. Runtime DLLs are not needed for these APIs.

Supported profile: ARM/Thumb function callbacks and EVENT_SET, delays1..65535ms,
up to64 active timers. EVENT_PULSE and non-ARM worker callback ABIs explicitly
fail timeSetEvent with zero; do not claim full Win32/WinCE multimedia semantics.
Event-set targets must resolve to a valid event object. Scheduling is cooperative,
not a hard real-time latency guarantee. BeginPeriod uses existing scheduler polling.

Public contract: Microsoft Learn timeSetEvent (ms713423), timeKillEvent
(dd757630), timeBeginPeriod. Four targeted tests cover callback ABI/Thumb state,
periodic wait/cancel, one-shot completion, invalid input and synchronous join.
No full THPS2 gameplay test has been performed.



#### Companion GLES ordinal metadata (2026-10-28)

gles_exports::local_tables reads only imported GLES CM/CL companions adjacent
to the importing image, with a16MiB input cap and CPU compatibility checks.
Use PE exports at the original base as ordinal->named-handler metadata; never
execute the supplied GPU driver. Main mapping retains original ordinal bindings
and dynamic #ordinal aliases, supplying corrected friendly names through its
resolver. Runtime guest modules use their own companion table or the established
main profile; contradictory main/module profiles fail explicitly.

Missing companion preserves existing global tables. Present but malformed,
missing/unnamed used ordinal or ambiguous aliases are errors, never a fallback
to an unrelated vendor ABI. Unknown exported names retain missing-handler
behavior. Companion search is local only, not recursive and not shared runtimes.
Dependency information uses the same resolver. No title condition/global table
replacement/native code/DLL payload is shipped. Synthetic export-only test DLL
is source-created and contains only four BX LR functions/names for metadata tests.

Actual supplied ChopperFight.exe companion has132 named ordinal mappings, with
14=eglGetDisplay and17=eglInitialize. The old CM global table maps these to
eglGetCurrentContext and eglGetDisplay. Three tests cover CM/CL local metadata,
no-companion fallback, malformed/missing/ambiguous exports. Core no-default
check passes. No full game, Windows or APK run is claimed.

THPS2 optional marker is explicitly a user-requested workaround, not a generic
API implementation: original startup fopen("\\Windows\\wmv9decoder2700g.dll","r")
only checks a nonnull result at that point. The optional text file must live
in the object-store Windows tree, not runtimes/pocketpc; it is not an executable
codec and must never be represented as one. No automatic game-name branch.



#### Automatic 2700G presence marker (2026-10-28)

At the user's explicit request PocketPC storage initialization creates
Windows/wmv9decoder2700G.dll as a plain text presence marker if no case-insensitive
entry already exists. This is an intentional hardware-probe workaround, not
a generic WinCE API implementation or an executable codec. No DLL is bundled.
Use create_new and never truncate or replace existing bytes (including genuine
DLLs, case variants or other existing entries). Storage path containment checks
run before creation. Gizmondo does not call PocketPC storage initialization.
The marker is generated before guest startup on desktop/Deck/Linux/Android.
No game-name branch, registry/registration modification or codec implementation.



#### Global PocketPC processor identity and IOCTL (2026-10-28)

LauncherConfig.pocketpc_processor is a global string (default pxa270), never
a GameSettings field. Desktop/Deck/Linux and Android global Emulator Settings
PocketPC section expose pxa270,pxa255,sa1110,arm. Android keeps the shared key
in originalJson and writes through the existing config pipeline. Both runners
apply the selected profile after loading, before any PocketPC guest instruction;
Gizmondo never enables it. Unknown config values log and use default PXA270.

KernelIoControl IOCTL_PROCESSOR_INFORMATION=0x01010064 implements Microsoft's
576-byte PROCESSOR_INFO layout: version WORD at0; UTF16 core[40] at2; core
revision WORD at82; name[40] at84; processor revision WORD at164; catalogue[100]
at166; vendor[100] at366; DWORD instruction flags at568 and optional clock at572.
Input must be null/0, output writable and >=576; optional returned-size pointer
validated, undersized output gets122 and required size, invalid pointers get87.
Unconfigured/Gizmondo profiles keep ERROR_NOT_SUPPORTED. Unknown other IOCTLs
and existing GET_DEVICEID remain unchanged. Silicon revision/catalogue/clock
remain unspecified; DSP/FPU flags are not invented from an identity choice.
For actual MIPS guest architecture the IOCTL says MIPS, regardless of ARM menu.
The profile is descriptive, not a CPU backend/ISA/clock-speed switch.

Chopper Fight's original 0x22c0c request reads the processor-name at+0x54;
0x11080..0x110d0 checks PXA and27/28. This layout matches that original call.
No game-name condition or executable patch. GetSystemInfo ARM processor-level
uses the selected4/5 revision when enabled, otherwise its old value.
Two unit tests cover all four profiles, exact offsets/bounds, short output,
invalid input, MIPS identity and profile isolation; core no-default check passes.
No full Windows/Android build or gameplay test has been performed.


### Original invariant snapshot (superseded where marked in AGENTS.md)

#### 1. Non-negotiable invariants

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

<a id="history-gpu"></a>
## Graphics implementation and experiments

#### Hidden control repaint and Q3 gameplay handoffs (2026-11-04; original validation snapshot)

The Windows Q3CE 1.1b gameplay log identifies coredll::repaint_controls as the
only GPU-to-CPU mutation site observed. It previously checked is_empty after
refreshing visibility, acquired cpu_mut and marked the frame dirty even with
only hidden startup controls. Use Controls::has_visible_content before acquiring
CPU pixels, exactly as compositor preflight already does. Visible controls and
native rendering retain the existing path; no title-specific exception.

Regression uses an immutable GPU source whose materialize panics: repaint of a
registered hidden panel must preserve GPU authority, residency, counter and
epoch without reading pixels. Re-showing the panel is still paintable. Existing
visible/native composition tests also run. Windows gameplay confirmation remains
necessary; one recorded mutation site does not prove all possible fallbacks
have been eliminated. Keep bounded handoff diagnostics for this validation.

#### CPU handoff provenance (2026-11-04; temporary diagnostics)

A shared publication can already have been consumed when a subsequent CPU update
is emitted. If both SHARED_WINDOW and GPU authority are absent and the last swap
succeeded, report cpu_authority_without_pending_shared_publication rather than
silently returning None. Desktop keyboard/menu composition bypasses acquisition
and reports its own reason once per process.

DeferredFramebuffer logs GPU-to-CPU mutation entry points with track_caller:
cpu_mut (after successful materialization), full overwrite, fill and resize.
At most 32 distinct operation/file/line sites are logged per process, only with
INFO enabled. Pure reads preserve authority and do not log. These diagnostics
do not suppress real CPU images, change fences, defer inputs or alter pacing.
They identify mutation entry points, not proof that pixels actually changed.

#### Deferred mapped GAPI memory (2026-11-04)

Mapped GAPI is no longer an unconditional readback requirement on ARM virtual-
TLB Unicorn. An EGL swap can publish an immutable shared image and replace the
mapped logical RGB565 bytes with a deferred data-memory provider. The first ARM
data access populates that range at TLB fill; HLE read_mem/read_mem_into and
partial write_mem use the same boundary. Invalidate cached TLB entries before
installing a replacement provider. No valid-memory/instruction polling hook is
installed. Native mapping maintenance can request FETCH translations too: those
must neither read pixels nor promote pending data into executable memory.

Provider errors and malformed sizes keep pending memory retryable. Unmapping
all pending bytes releases them without readback; partial unmap first preserves
remaining bytes. Unsupported CPUs, unsuitable permissions/shared mappings and
padded scanline layouts keep the eager path. Packed scanlines and physical
columns encode exactly the logical guest framebuffer; internal GLES scale and
frontend presentation scale are unchanged. Frame slot leases/fences stay intact.

Internal framebuffer polling must not itself materialize pending GPU pixels.
Before resuming guest code, an intervening real GDI mutation must discard the
older deferred publication and mirror the newer CPU image into guest RAM. That
prevents a retained pointer from resurrecting old GPU pixels. Real writes still
invalidate shared presentation through existing authority epochs.

Compositor preflight also distinguishes registered controls from visible paint:
hidden startup panels and an empty direct GAPI panel have no pixels to replay.
Visible children, native bitmap layers, modal/shell/taskbar/keyboard overlays
retain their CPU composition path. Q3's hidden startup controls otherwise forced
CPU authority immediately after a correctly deferred swap.

Native regression covers real ARM pointer reuse across two EGL swaps, packed
and column layouts, native and X4 render sizes, HLE reads/partial writes, GDI
precedence before guest resume, permission failure, replacement, malformed source
and unmap lifetime. Local Q3CE 1.1b menu exercises actual shared export and a UI
consumer while GAPI remains mapped. Gameplay and Windows/Android hardware still
require validation; the earlier Single Player breakpoint was not investigated.
User also reports input stutter on Intel UHD only fullscreen X1, versus NVIDIA
in windowed and fullscreen; presentation/driver interaction remains a separate
open investigation, not a solved NVIDIA-only issue.

#### No-op native paint must preserve GPU authority (2026-11-04)

KernelState::composite_window_layers previously called cpu_mut before collecting
native window layers and checking paint_generation against published_generation.
Thus present_window_surfaces could materialize/invalidate a shared GPU image,
then immediately return because no backing bitmap had changed. EndPaint without
new drawing is not a framebuffer write. Delay CPU mutable access until after
the existing generation decision. Real changed GDI painting, control overlays,
visibility, guest frame accounting and authority-epoch rejection are unchanged.
Do not weaken cpu_mut or reuse old GPU pixels after a real CPU write.

Regression covers absent layers, 1000 unchanged native paints with a deliberately
unavailable CPU reader, exact GPU source identity/revision/epoch preservation,
real green GDI output/readback, and changed but hidden native surfaces. Native
EGL handoff tests retain the texture across 32 no-op validations per swap before
testing the existing stale-authority rejection. This fixes a concrete unnecessary
CPU transition; other legitimate or unrelated CPU transitions remain possible.

WGL experiment rollback in this delivery: upstream eframe change_gl_context
body/signature restored. No custom WGL query, lookup cache or binding reuse
guard remains. Opt-in CPU timing remains available for verification only.

#### Bounded desktop image handoff (2026-11-04)

Desktop/Deck preview transport is a one-image mailbox, not an unbounded mpsc
FIFO. Producer publication replaces and releases the previous pending image;
the UI takes once per update, avoiding both stale GPU leases and an unbounded
drain loop. Input, child-process, GPS, and outcome channels retain mpsc FIFO
semantics. Mailbox receiver teardown releases the pending lease and rejects
future sends; producer clones preserve disconnect semantics.

The shared native snapshot pool is bounded to five slots: one current kernel
image, one pending mailbox image, up to two UI snapshots during receive/replace,
and one slot for capturing their successor. Capacity alone cannot fix an
unbounded queue; both changes are required. Additional slots allocate lazily.
At 320x240 the two extra RGBA textures total 614400 bytes; at 1280x960 about
9.4 MiB, excluding driver allocation overhead. No texture is overwritten while
leased, and existing ready/done fences, retirement, errors, CPU authority and
real-backpressure fallback remain unchanged. Explicit CPU filters, mapped GAPI,
recording/readback, missing sharing, and software rendering are not forced GPU.
Android already uses a latest-image handoff; only the common pool bound changes
there. Desktop shutdown still joins the worker before closing the UI share group.

Tests cover 10000 publications to a stalled UI with prompt obsolete-lease
release, newest-image delivery, producer clones and receiver shutdown. A native
GL/GLES test holds four old pipeline images across 1000 successor captures,
checks no readback and unchanged retained red/new green pixels. Full-pool
backpressure, resize, abandonment, delayed disposal and queued-error detection
are still tested at the new bound. Hardware NVIDIA/Intel visual validation and
complete Windows/Android builds remain separate from local tests.

### GPU rendering

#### UI redraw context reuse (2026-11-03; rolled back)

The supplied NVIDIA reproduction is host input: moving an unused mouse or
holding a key causes severe stutter. Desktop pointer routing already ignores
movement without an active guest stylus stroke, and OS keyboard repeats are
not forwarded as new game key presses. Host UI redraws remain a separate path.

The exact locked eframe 0.27.2 glow backend unconditionally detaches/reattaches
its UI context twice per normal repaint. A local source patch skips these
transitions only when actual context and requested draw/read drawable match.
This removes unnecessary transitions during input-triggered repaints while
preserving real viewport/context switches and restoration after callbacks.
See vendor/eframe/POCKETHLE-PATCH.md for provenance, licenses and validation.

The change does not suppress input, alter VSync, throttle rendering, change
limits or disable FullGPU. Linux native tests pass; its effect on NVIDIA input
stutters requires the user's GTX1650 test. This is not an established fix for
Intel image artifacts or shared snapshot pool pressure.


#### Displayed game FPS (2026-11-03)

The game-screen overlay now counts new images drawn by the frontend, not guest
GXEndDraw/Flip/eglSwapBuffers submissions or queued framebuffer updates. This
policy is common to GAPI, GDI, DirectDraw, software GLES and host-GPU GLES,
including fullscreen, Deck, filters, captures and recording. It reports draw
submission by the frontend, not OS compositor/physical scanout confirmation.
No artificial 60/120 clamp is applied to the measurement.

Desktop snapshots have a monotonic image identity preserved by clones and CPU
fallback conversion. Reconstruction exposes the identity only after drawing;
its callback records a new identity once. Queued images superseded before paint,
retained images after failed replacement, filter/rotation repaints and repeated
UI redraws do not inflate the rate. The plain egui texture path records in an
ordered callback after its image mesh, with the last successfully uploaded
identity. Meter state is detached between game sessions and idle rates become
zero. Producer PresentationStats remain submission telemetry, not overlay FPS.

Android uses the existing GL-thread PresentedFrameCounter after glDrawArrays.
The former native submission-rate override is removed from the overlay; JNI
telemetry is explicitly renamed nativeSubmittedFrameFps. The overlay always
labels the sampled displayed-image rate FPS. No new per-frame glGetError,
readback, glFinish, fence, input-repeat or pacing behavior is introduced.

Validation: real presenter tests on Mesa desktop GL and GLES cover dropped
queues, repaint/filter/rotation identity, failed replacement, shared/CPU pixels
and context recreation. An isolated test of the actual desktop meter covers
400 produced identities with only 57 painted and repeated/capture redraws.
An Android unit test covers 400 submitted objects, 60 drawn and idle redraws;
Android Gradle/APK and full desktop eframe builds are not run in this workspace.
Input-correlated NVIDIA stuttering and Intel texture corruption remain separate
investigations; this metric change is not claimed as their fix.


#### Deferred native error checks with shared presentation (2026-11-03)

The supplied hybrid-laptop investigation measured NVIDIA 561.09 post-draw
`glGetError` latency (584 calls above 2 ms; 11.43 s total, versus 3.7 ms on
Intel). These are reported host-call timings, not a measurement of the driver's
internal cause, and were not repeated on NVIDIA in this environment.

Draw and glFlush submission no longer query the native error state each time.
Guest validation remains immediate. Native failures are checked at explicit
guest glGetError/glFinish, read boundaries (including clean cached reads), and
before a configured shared snapshot export. Pending errors are checked before
texture upload fallback so an earlier draw failure cannot be swallowed by a
compressed-format fallback. Initialization, allocation, clear, snapshot-copy
and post-read checks remain in place. No glFinish or new readback is added.

The FullGPU export boundary propagates a pending batch error to Context's
existing fatal GPU failure path, even if export is disabled or all three slots
are leased. It must not label a native rendering failure as an optional sharing
failure, drain the error and then publish the faulty image. Copy/transport
failures retain their established optional export fallback.

The FPS menus also offer an explicit 120 fps cap globally and per game. It
overrides implicit 60 Hz presentation pacing; explicit guest vertical-blank
waits and the modeled LCD clock remain 60 Hz. UI/vsync and game-internal timing
can still limit displayed output. Default remains 60 fps.

This port retains WGL relay/compatibility contexts, deferred pixel authority,
shared presentation, internal scales, filters, and frame limits. It excludes
all timing probes from the supplied archive. Previously delivered bounded
handoff/pool diagnostics remain; the fresh-texture-per-reuse experiment is off.
This change does not establish a cause or fix for Intel image artifacts or
input-correlated NVIDIA stutters. Device tests and before/after comparisons
remain necessary for those observations.

Validation: 218 default GLES tests pass; native desktop GL/GLES tests cover
zero error queries for steady draw/flush, deferred error delivery, clean reads,
pre-upload protection, full shared-pool error handling, and shared transport.
Windows-gated code passes metadata/type checking on Linux (not a Windows link).


#### GPU logical image reduction (2026-11-03)

This iteration moves the X2/X3/X4 box reduction to a separate GPU pass. It is
automatic with GPU rendering and an effective internal scale above one; native
X1 and software rendering keep their previous algorithms. No new setting,
profiling probe, dependency or CPU optimization change is introduced.

The native color attachment remains unchanged and its full readback is retained
for the existing high-resolution presentation/filter path. The GPU resolves
into an RGBA8 logical-size texture, which is then read into the logical guest
cache. This is not zero-copy presentation: full native readback, logical output
copy/packing and the extra logical-size readback remain. The added pass/readback
can affect wall time on some drivers, especially at X2.

The shader samples exact texel centers using the source's nearest/clamp setup,
recovers byte values, and implements integer-equivalent truncating box averages.
A small quotient bias protects integer boundaries against reciprocal rounding;
it is below the minimum nonzero quotient step for all three factors. The logical
cache retains the existing RGB565 conversion when required. Full-resolution
presentation pixels are not replaced by the reduced image.

Resources are lazy, reused per renderer, recreated when logical size/scale
changes, and released on renderer destruction. The pass requires fragment
highp precision and a verified RGBA8 framebuffer. Failure logs once and retains
the established CPU reduction. Its program and buffer are separate from guest
draw resources; the native framebuffer/program are restored, driver errors are
handled, and pipeline/attribute caches are invalidated before subsequent draws.
This first use may compile a shader; timings below exclude initialization.

Validation on Linux Mesa desktop OpenGL and GLES2:

- Byte-exact logical images against the CPU reference at X2/X3/X4, both RGB565
  and RGBA, in landscape and portrait (24 combinations of format/scale/API/size).
  Random native pixel patterns exercise channel/alpha rounding. Native display
  readback bytes are identical before/after the logical resolve.
- Draw-after-read with non-default viewport, scissor, depth and blend state;
  scale switches, native X1, forced CPU fallback, and an incomplete auxiliary
  FBO to test optional-pass error recovery. Both focused native tests pass.
- Full GLES regression run: 273 pass with the existing benchmark excluded.
  After final precision/packing guards, 218 default tests pass, 56 native tests
  are ignored by default; the two focused tests are explicitly run again.
- WinCE window-swap/internal-resolution export regression passes for all tested
  policies. The updated clean app stack compiles and PVR Racer runs in X4,
  producing animated 3D frames with no reduction fallback warning.

Optimized presentation microbenchmark, RGB565 320x240 logical image, repeated
clear/read-present workload, no profiling code. Three before/after comparisons
in alternating order; five warmed batches of 30 frames per executable/scale.
Numbers are medians over 15 batch averages, measured with CLOCK_THREAD_CPUTIME_ID
on the calling emulation/render thread. Mesa llvmpipe worker CPU is excluded:
this is not total-system CPU, game FPS, or a physical-GPU performance guarantee.

| Scale | Before thread CPU ms/frame | After thread CPU ms/frame | Reduction |
| --- | ---: | ---: | ---: |
| X2 | 1.835 | 1.068 | 41.8% |
| X3 | 2.992 | 1.738 | 41.9% |
| X4 | 4.504 | 2.716 | 39.7% |

Corresponding median wall times were 2.088 -> 1.753 ms, 3.329 -> 3.048 ms and
4.923 -> 4.273 ms. One initial X2 before/after pair had a wall-time increase;
there is host/driver variation and no claim of a universal frame-time gain.
Native X1 is a control and has no retained performance claim.
Windows/MSVC, Android/physical GPUs and complete platform CI were not run.
Apply this three-file delta over the cumulative no-probes optimization lot.


#### CPU preparation optimizations (2026-11-03)

Keep the validated high-resolution presentation and per-draw scratch reuse.
Scaled presentation exports top-down RGBA with exactly the former RGB565 color
quantization and opaque alpha, avoiding the RGB565 pack/unpack round-trip.
Guest memory, native-resolution rendering, filters and brightness are unchanged.
The retained host image costs two extra bytes per physical pixel (+2.34 MiB at
1280x960). Reuse bounded fallback vertex/remap/index storage; guest inputs are
still reread every draw, with cleanup on early exits and no guest-memory cache.

The temporary GPU profiling module and frontend probes have been removed.
No GL calls, shader, readback, upload or draw ordering are changed by this cleanup.


#### RGB surface RGBA readback (Stuntcar Extreme)

The single exposed EGL configuration advertises RGB565 and zero alpha bits.
`glReadPixels(GL_RGBA, GL_UNSIGNED_BYTE)` must therefore return alpha 255 for
in-bounds pixels, even though the internal software/native targets retain RGBA
for rendering. Keep their stored alpha and existing RGB precision unchanged;
this is an API readback conversion, not a texture or shader alteration.

Stuntcar Extreme captures its HUD labels against a blue clear colour with
alpha zero. Returning that internal zero caused the game's CPU image conversion
to blend the blue colour key to black before building the cached label textures.
Returning the missing surface alpha as one preserves the key and lets the game
generate transparent backgrounds. Do not replace black texels globally or add a
game-name branch. The original executable and resources remain unchanged.

Local native ARM execution with the supplied game reproduces the black labels
before the fix and transparent labels after it on software and Mesa GPU paths.
Physical Android/Windows confirmation and complete APK/EXE packaging remain
separate checks. API regressions exercise transparent/partial/opaque internal
clear alpha, opaque readback, unchanged RGB and retained internal alpha on both
software and the native renderer.

#### Configuration

**GPU rendering (experimental)** is enabled by default when the global setting is
missing. Explicit saved Software choices remain honored. Per-game choices are
Emulator default / Software / GPU; restart the game after changing the backend.
Windows uses OpenGL through WGL; Android uses EGL/GLES. Linux uses the shared
frontend; see [desktop parity](FRONTENDS.md#native-service-parity) for platform limits.

Anisotropic filtering offers Off, 2x, 4x and 8x, capped to driver support. It is
available only with GPU rendering. Upscaling and image filters are presentation
options; they do not increase the guest's native rendering resolution.

#### What runs where

The GPU performs transforms, fixed lighting, rasterization and supported texture
sampling/uploads. The CPU executes guest code, validates and reads guest memory,
prepares unsupported vertex/texture formats, and implements API dispatch.
Typed vertex streams, array read coalescing, compact attributes and state caches
reduce CPU preparation. Native compressed uploads and mip generation have
compatibility fallbacks. Software remains a separate supported rendering path.

Presentation still uses synchronous readback into the emulated framebuffer.
General zero-copy presentation, asynchronous readback and elimination of CPU
flat-shading expansion are not implemented. A GPU backend is not a guarantee
of a particular game frame rate.

#### Testing and troubleshooting

Compare the same scene in GPU and Software: geometry, lighting, alpha edges,
HUD, distant textures, camera/video overlays, and offscreen render targets.
Check rotation, captures, repeated launches and demo launcher child/parent return.
Report platform, GPU/driver, game version, renderer and the failing action.
A game can request a rate below 60 Hz (Battlestations requests about 20 Hz).
The 60 Hz presentation ceiling does not speed up or override its requested Sleep.

Startup initialization failures use the existing software fallback. Runtime GPU
failure is reported; restart with Software rather than assuming a partial
fallback preserves native depth state. Mip filtering can differ from Software's
base-level sampling, and optional GLES2 features vary by driver.

#### Implementation reference

The sections below preserve the technical notes previously spread across the
architecture document. Dates and test counts describe the validation at the time
of each change, not a new full release certification. Complete Windows/APK builds
and physical-device game testing require the corresponding host toolchains.

##### Experimental native GLES rasterization (2026-10-11)
Global config.gpu_renderer defaults true, shown in Emulator Settings / Emulator options. GameSettings.gpu_renderer is nullable: null/missing inherits, false forces software, true forces GPU. Both Kotlin JSON models preserve unknown fields. Configure on the emulation thread after creating the dispatcher; each child inherits the launcher configuration and per-game override.

pocket-gles::gpu owns private offscreen native contexts: Windows WGL hidden 1x1 window + OpenGL GLSL120, Android EGL GLES2 pbuffer + GLSL100, headless Linux EGL/OpenGL validation. No new dependencies. CPU validates guest memory and assembles object-space vertices; the vertex shader applies projection/modelview, texture matrices and eye-space fog depth before native rasterization. Shaders/native rasterization handle 4 texture stages, fog, alpha test, blend, depth, culling, color mask, scissor. Guest points/lines now have software and native rasterization (see below). Fixed lighting is now evaluated per vertex by both backends (see below). Flat shading expands triangles and copies provoking color; Supported compressed textures may upload directly; unsupported paths decode on CPU, and texture revisions invalidate GPU uploads.

Native framebuffer RGBA+depth persists across draws. Readback runs only for EGL swap or glReadPixels, flips bottom-up rows to canonical top-down, then existing RGB565/GAPI/DirectShow/filter/screenshot paths consume the result. No zero-copy frontend integration yet. GPU stage allocations and GL resources release on EGL termination, emulator drop, context reset and child return. Contexts are !Send; every operation makes its own context current. EGL display leases count live private contexts, so child teardown cannot terminate parent; Android keeps one process-wide EGL library/display initialization lease because the Android UI shares DEFAULT_DISPLAY, while all game surfaces/contexts are destroyed.

Initialization failure logs software fallback. Runtime GPU failure reports once, sets GL_OUT_OF_MEMORY and stops issuing draws until restart/termination; do not attempt partial software fallback with stale depth. Toggle software and restart for driver problems. Backend identification is a single normal info log, no per-frame instrumentation.

Validation: software/core/library/API unit tests, real OpenGL and GLES2 Mesa driver tests (depth/blend/alpha/textures/revision update/fog/scissor/masks/readback), ten child renderer lifetimes, full guest GLES->RGB565/GAPI/glReadPixels pipeline plus parent resume/EGL termination/reinitialization. Host driver here is Mesa, so real Windows/Android GPU performance and visual game regressions require device tests.


##### GPU portability, settings and presentation follow-up
All native GL/EGL/loader string signatures use std::ffi::c_char, including
ShaderSource's pointer-to-pointer. Android ARM C char is unsigned; hard-coded
i8 signatures compile on Windows/Linux but fail on Android. Keep every GL
function-pointer declaration and transmute signature consistent.

The GPU setting is saved by the existing JNI JSON writers. An older native
library silently discards unknown fields even when it returns ok. Android
settings now read back the renderer field after writes and report stale native
libraries; the per-game renderer avoids a second SharedPreferences value.
Native build failures explicitly stop. APK packaging requires a source and
both-library hash stamp produced only after a successful complete native build;
source changes, changed binaries or a failed native rebuild invalidate it.
Rebuild native libraries and APK together after applying this patch.

WGL checks the actual current context/DC and EGL checks context/display/draw/read surfaces before rebinding, avoiding redundant
context transitions while retaining child/parent restoration. Desktop and Android frame
publication advance on a fixed phase instead of restarting its interval at
the delayed send time; missed slots are skipped without catch-up bursts.
This is a targeted jitter reduction, not proof of stutter-free hardware output.
No new frame logging, worker, persistent native resource or rendering buffer.


##### Native vertex transformations
When a renderer exists, Context::assemble retains object positions and incoming
texture coordinates instead of multiplying each referenced vertex on the CPU.
Projection/modelview and four texture matrices are uploaded per draw, column
major with transpose=false, valid on desktop GL and GLES2. The shader computes
projection * (modelview * object) and abs(eye.z) for fog, then applies the existing
depth-range conversion to clip coordinates. Client-array decoding, normalization,
VBO bounds and guest-memory checks remain CPU-owned. Software uses the original
MVP, eye-space and texture transform path. Constant texture coordinates retain
the previous semantics (identity texture matrix when the unit array is disabled).
Flat provoking colors and strip winding remain unchanged. No new native buffers,
workers or object lifetimes; existing VBO and shader program own the resources.

Native driver comparisons exercise desktop GLSL120 and GLES2 GLSL100 against
CPU-pretransformed geometry: perspective/rotation/translation/nonuniform scale,
all four texture coordinate units, linear fog and triangle/list/strip/fan.
An actual Context/guest-array test checks object-space assembly, drawing and
restoration of CPU transformation after disabling GPU. ARM64/ARMv7 backend
checks and software unit tests accompany this change. Device performance remains
unmeasured; this change does not remove framebuffer readback or CPU draw assembly.


##### Shared fixed lighting (software and native GPU)
LightingState owns eight lights, shared material, model ambient, two-sided,
color-material, normalization and rescale settings. Light position and spot
direction are captured in eye space at setter time. Float/fixed setters and
light/material queries validate enum, range and bounded guest memory, reporting
GL errors; queries are also exposed as named extensions. Session suspension
retains lighting state; release/reset uses existing context ownership.

Software assembly transforms normals with one inverse-transpose matrix per draw,
evaluates ambient/emission/diffuse/specular and positional attenuation/spotlight
per unique indexed vertex, and interpolates front/back colors. Rasterization
selects back colors for two-sided back faces; flat shading uses the provoking
vertex. The native shader evaluates the same equation with eight light uniforms,
one additional normal/fog attribute and one provoking-position attribute.
No extra workers, guest allocations or persistent resource handles. Disabled
lighting keeps prior rendering. Lighting improves visual compatibility, not a
promised speed increase; software adds CPU work when lighting is enabled.

Tests cover guest signed normal arrays, analytical colors, normalization/rescale,
front/back and flat/smooth shading, positional attenuation, spotlights,
color-material, specular, light 7, validation and parent/session retention.
Software/native driver samples agree within two 8-bit channel levels. Host GL
and GLES2 shader driver checks and ARM64/ARMv7 Rust checks pass; complete APK and
Windows device performance still require user builds/tests.

Brightness slider positions remain registry 1/11/21/31. Neutral/default is 11,
unchanged pixels. Minimum 1 now blends 20% toward black (128 becomes 103), between
the earlier 50% and 1/12 settings. The upper range now uses a reduced ceiling:
level 4 (registry 31) matches the previous level 3, blending 1/12 toward white;
level 3 (registry 21) blends 1/24 toward white, halfway from neutral to that ceiling.
For channel 128 the four displayed levels are 103/128/133/138. Guest pixels and
registry values are unchanged; the common presentation path applies this to both
GPU and software.


##### Point and line rasterization (software and native GPU)
GL_POINTS, GL_LINES, GL_LINE_STRIP and GL_LINE_LOOP now render. Context still
validates guest arrays/VBOs, caches indexed vertices and performs common assembly.
Float/fixed point-size and line-width setters reject nonfinite/nonpositive values
with GL_INVALID_VALUE, retain the previous value on error, and default to 1.
Queries expose requested sizes and aliased ranges; software supports 1..64 pixels,
native ranges come from the driver capped at 64. Actual rasterization clamps to
supported ranges without changing queried requested state.

Software points clip their center against six homogeneous planes, expand as
aliased window-space squares with odd/even snapping, and honor target/scissor.
Lines clip endpoints/interpolate attributes, use diamond-exit coverage with the
terminal pixel excluded, and replicate in the minor direction for wide lines.
Attributes are perspective-correct; depth remains window-linear; flat lines keep
the original terminal vertex color even when clipped. Zero-length and invalid
coordinates are discarded. Loops close only when at least two vertices exist;
incomplete independent endpoint pairs are ignored. Culling/two-sided back-face
colors affect triangles only; lighting of points/lines uses their front normal.
Texture combiners, alpha/depth tests, fog, blend/masks and depth writes share the
existing triangle fragment operations. Loops and pixel work are bounded by target
and scissor; no new workers/native handles/guest allocations.

Native drawing reuses the existing program/VBO/FBO, emits independent line pairs
for strips/loops and applies provoking attributes for flat lines. gl_PointSize is
written by the shader (desktop program-point-size enabled only on its private
context), native glLineWidth uses queried limits. All existing transforms,
lighting, texture and fragment state apply. Triangle state is reset on each draw.
No point sprites, point-size arrays/attenuation or POINT_SMOOTH/LINE_SMOOTH support
is claimed by this change; those remain separate features.

Tests cover software clipping, odd/even sizes, width, terminal blending, culling,
scissor, flat colors, texture/fog/blend/masks/depth and actual guest/indexed modes;
real desktop GL and GLES2 compare all four primitive modes, widths and triangle
cull-state restoration. Wide-line coverage/interpolation is driver dependent;
coverage comparisons allow edge variation and use constant colors for wide lines.
Existing lighting/transform/triangle driver tests remain part of validation.


##### Native alpha/depth-clear parity (Carmageddon investigation)
Carmageddon's attached executable imports glAlphaFuncx and sets GL_EQUAL with
fixed reference 0x10000 (1.0) at 0x496bc. The native shader compared raw floating
alpha, while software rounds fragment alpha and reference to 8-bit fixed precision.
Native now rounds a clamped alpha to 0..255 for the test only and receives the
rounded reference as an exact integer-valued float. Color/alpha used for output
and blending remain unchanged. This is shared behavior, not a game-specific hack.

The software HLE clear contract always resets depth for GL_DEPTH_BUFFER_BIT,
even when a preceding HUD pass disabled depth writes. Native previously honored
the write mask during clear, leaving old depth and rejecting subsequent geometry.
Native now temporarily enables depth writes for clear and restores the guest mask;
draws continue honoring the mask. This preserves the existing software compatibility
contract, rather than changing software to strict OpenGL masked-clear behavior.
Color-clear masks are outside this change.

Both regression cases produce visible software output and black native output
before the fix; both pass on real desktop GL and GLES2 afterward. Existing native
lighting/matrices/points/lines/depth/blend/textures/context switching also pass.
No new resources, workers, diagnostic logging or per-title detection. Software
rendering is unchanged. The standalone executable establishes call-site evidence;
the complete Carmageddon run still needs its game data and device testing.

##### Colors: array reads, native indices and private-context state cache
The attached Colors cube routine (0xd5b94) submits eight fixed position/colour
vertices and 36 unsigned-byte triangle indices. Static call-site counts are not
frame-frequency measurements. Dense guest client arrays are now read once per
draw and decoded by stride; spans are capped at 1 MiB per array and four times
the index-count range. Checked addresses, sparse spans or failed speculative
reads keep the old per-element behavior. VBO data remains a host-copy offset.
Prepared data is discarded after vertex assembly, including failed draws; do
not persist it across calls without tracking guest writes.

The native renderer deduplicates complete 24-float records by exact bits and
uploads a reusable 16-bit index buffer if total transfer size decreases. Flat
provoking color/normal/light-position attributes are part of the key, and all
primitive expansion/winding rules are retained. More than 65,536 unique records
or no transfer benefit uses DrawArrays. Packing/deduplication is still CPU work;
this is not a claim that the complete guest draw path is GPU-only. Scratch vectors
and the bounded vertex map are reused and freed with the renderer; its new native
index buffer is deleted alongside the vertex buffer.

Uniform values belong to the private program, cache exact bit patterns and skip
unchanged uploads. Native pipeline snapshots are invalidated by resize/clear,
which change masks/scissor independently of drawing. Each texture ID has cached
filter/wrap parameters; deleting a texture removes its entry to permit native ID
reuse. Texture bindings continue being restored at draw time, including after
readback/resize. Parent/child renderers have separate programs, contexts and caches.

GL_POLYGON_OFFSET_FILL and float/fixed setters now control triangles in both
paths. Software slope is computed in window-depth coordinates, units use 1/65535
consistent with the advertised depth precision. Native uses glPolygonOffset;
physical offset precision remains driver dependent. Points/lines are unaffected.
Queries expose enable, factor and units. glCopyTexImage2D synchronizes GPU colour,
copies bottom-left framebuffer coordinates into canonical texture rows, supports
the five base formats and validates target/dimensions/level/border. Out-of-bounds
source pixels are deterministically zero before format conversion. Only level
zero is stored, matching existing texImage semantics; copy subimages/mip storage
remain unsupported. Copies still read back native pixels and are not advertised
as a zero-copy performance improvement.

The Colors index-sequence tests establish 16 -> 2 guest attribute reads and
3456 -> 840 native transfer bytes (8 records plus 36 ushort indices). Host GL and
GLES2 verify rendering, caches after clear/resize, polygon depth, existing lighting,
alpha/depth, primitives, matrices and independent context resume. API tests cover
fixed conversion, queries, stack tail and GL errors. Full-game FPS requires its
assets and target-device testing; no temporary profiling is shipped.

##### Carmageddon: sequential draws, compact attributes, fused presentation
Carmageddon has four-vertex smooth triangle-strip call sites (e.g. 0x1808c,
0x49898, 0x49bac, 0x4d49c). Counts of static call sites do not establish frequency.
DrawArrays now passes an iterator range instead of allocating indices and skips
the guest-index vertex cache; indexed calls keep their existing cache. Dense array
reads remain per-draw and checked. The native sequential/smooth path forwards
triangles, strips, fans, points and independent lines directly and skips primitive
expansion/deduplication. Flat shading and line strips/loops retain expansion to
preserve provoking attributes and endpoint behavior. Two-sided lighting treats
native strip/fan modes as triangle primitives too. No per-title detection.

The previous fixed 24-float record is now a per-draw layout: position/colour always,
UV pairs only for active stages, normal/fog only for lighting or pretransformed fog,
separate provoking position only for flat lighting. Smooth lighting aliases the
position attribute. Every draw restores enabled/disabled arrays and their offsets
and stride, including transitions between sparse texture units and lighting modes.
Vertex keys use only packed attributes (zero-padding their fixed-size hash key),
retain exact bits and are rebuilt for indexed/expanded draws. Sequential calls skip
that rebuild. Fragment sampling is conditional on the stage being enabled.
All eight shader attributes still exist; disabled unused attributes are not relied
upon for visible output. Unlit/untextured stride is 32 bytes, one texture 40 bytes,
and full flat lighting/multitexture can still use 96. Resources/caches remain owned
by the renderer and share its existing teardown; no new native handles or workers.

Renderer read_present produces canonical RGBA and framebuffer RGB565 in a single
readback traversal with the existing bottom-up/top-down flip and truncation masks.
Both images remain current: glReadPixels/copyTexImage after swap must not see stale
RGBA even though GPU dirty is cleared. A clean readback uses canonical RGBA; short
output preserves the old converter's behavior and trailing odd byte. Context
present_rgb565 selects the native fused path or the existing software converter,
with the same native failure handling. eglSwapBuffers retains DirectShow servicing,
framebuffer dirty marking and mapped GAPI synchronization in their previous order.
Synchronous glReadPixels is retained; no zero-copy frontend claim or pacing change.

Controlled assertions show a textured four-vertex strip at 160 bytes versus the
previous indexed 396; the untextured Colors cube at 328 versus previous 840 bytes.
Driver tests on desktop GL and GLES2 cover direct strips/fans against expanded
reference, flat fallback, layout transitions, short/odd-width presentation buffers,
readback orientation and canonical-cache reuse. Existing lighting, transforms,
alpha/depth, texture, point/line and parent-context tests remain relevant. Guest
presentation tests also copy a texture after a swap before parent suspension.
Upload-time paletted decoding is unchanged; performance benefit in a race requires
actual target-device measurement. No transient profiling is shipped.

##### Four-game native GPU audit: RGB, DXT3, direct indices, mip/LOD and EGL images
The attached executables were Hit and Myth, Richard Burns Rally, Chicane (the
unnamed Autorun attachment identified by its own strings) and Trailblazer.
Static imports/call sites establish possible use, not draw frequency or actual
execution of every engine branch. These additions supersede the earlier notes
about base-only mip storage and expanded native indexed preparation.

RBR's 0x1290c uploads DXT3 (0x83f2), including separate positive mip levels.
DXT3 uses explicit 4-bit alpha and ALWAYS four interpolated BC1 colours even
when endpoint 0 <= endpoint 1; selector 3 must not become transparent. Decode
once to canonical RGBA for both backends, crop partial edge blocks and reject
short payloads. The supported-format query includes DXT3. Existing DXT1/ATC and
palette decoding remains available.

RBR 0x96204 and Chicane 0x1830c request RGB/unsigned-byte readback. RGB, RGBA
and RGB565 now honor GL_PACK_ALIGNMENT (1/2/4/8, default 4), preserve guest
padding bytes and retain bottom-left GL orientation. Large/negative dimensions
are rejected before allocation. Texture upload honors GL_UNPACK_ALIGNMENT by
removing row padding before canonical decode. Pixel-store state belongs to the
execution session, is reset for new games and retained through parent suspension.

Native smooth indexed triangles/strips/fans, points and independent lines remap
validated guest indices to dense ushort indices during assembly. Each unique
vertex is decoded once and packed directly into the compact native layout;
there is no expanded vertex list or float-record hash on this path. Flat shading,
line strips/loops and >65,536 unique vertices retain the established provoking-
attribute/expansion fallback. Sparse arrays retain checked element reads. Existing
sequential-draw optimization and private-context caches remain intact. A four-
vertex textured strip transfers 160 vertex bytes + 8 index bytes. Guest indices
are never treated as unchecked native offsets.

Texture objects retain explicit levels 0..10 (maximum base size 1024), including
mip subimages and framebuffer copies. Shared-palette negative-level uploads are
validated in full before mutation and decoded per level. Base resizing discards
an unrelated old chain. A texture revision invalidates all native levels; missing
levels are derived by bounded box filtering at upload time to keep legacy base-
only assets usable. Native retains all four mip min-filter modes. Per-unit float/
fixed GL_TEXTURE_LOD_BIAS_EXT is passed to fragment texture2D's bias argument,
so driver derivatives select real levels; RBR -0.5, Chicane -1 and Trailblazer +1
are supported. Generate/upload mip chains only when a mip filter is active;
base-only video/dynamic textures do not pay for unused mip generation. Switching
from a base filter to a mip filter forces a complete upload even at the same
image revision. GLES2 non-power-of-two textures
retain full mip/repeat support when ES3 or GL_OES_texture_npot provides it;
otherwise they use base filtering/clamp-to-edge, avoiding incomplete textures. Software still uses its existing base-level/magnification
sampling; derivative-based software mip selection is not implemented here.

Hit and Myth's 0x9069c/0x90708 engine branches create pixmap/pbuffer surfaces.
These now own distinct colour/depth images and, when used, distinct native
renderers. GL state and canonical texture objects stay in Context across surface
switches. Native texture caches are private to each renderer; canonical updates
are uploaded on its next draw. Draw/read surfaces can differ; readPixels and
copyTexImage temporarily select the read surface, then restore the draw image.
Only a window swap presents to the kernel/GAPI framebuffer. Pixmap synchronization
uses GDI bitmap handles and the existing DIB encoding/writeback helper, including
bottom-up/24-bit layouts; wait/swap/change-current/destroy synchronize its pixels.
CopyBuffers also writes to a same-sized GDI bitmap. Unknown native pixmaps and
invalid dimensions/configs report EGL errors. No fabricated alias to the window.

Surface count is bounded to 32 and total target area to 4 Mi pixels; per-image
size is at most 1024x1024. Destroying a current surface defers deletion until
unbind; deleting parked images drops their buffers/native resources immediately.
Session reset/termination/release and parent suspension cover every parked image,
not just the current renderer. Unbinding parks live images and frees deleted ones.
The existing single guest EGL context model remains: multiple independent guest
contexts, bindTexImage/releaseTexImage and arbitrary client-buffer surfaces are
not introduced. There is no proof that Hit's offscreen branch runs in gameplay.

Validation: 134 GLES unit tests; 10 real desktop GL/GLES2 renderer tests (including
explicit mip colours, positive/negative bias and direct native strips); 4 Context
native tests; 28 WinCE GLES tests plus 2 native presentation/offscreen tests.
Offscreen tests cover separate read/draw images, 64 create/destroy cycles, deferred
current destruction, parent resume and bottom-up 24-bit DIB writeback. ARM64 and
ARMv7 pocket-gles Rust checks pass with warnings denied. Complete Windows/APK
builds and four-game FPS/visual regression testing still require target devices.
No temporary game instrumentation or profiling is delivered.
Texture deletion fans out to every parked surface's native cache as well as the
current renderer. Otherwise reusing a guest name with the same revision can show
an old image, and unused native texture objects survive until surface destruction.
A software/native regression recreates the same name while the window renderer is
parked and verifies the updated colour after returning to both images.

The final complete pocket-winceapi no-default-features unit suite, including
both native tests, passes all 284 tests on the available host.

Global GPU rendering now defaults to true in LauncherConfig, including serde
deserialization of old configs without this key and Android Kotlin defaults/JSON
reading. Explicit saved false and per-game software overrides remain honored;
per-game null still inherits the global setting. Startup GPU initialization
failure retains the existing software fallback. The experimental English label
is unchanged.

#### GPU upload/state optimizations (2026-10-11)

GPU texture uploads retain supported compressed block payloads and defer CPU
RGBA decoding. Query driver formats once per renderer; optional compressed/mip
entry points fall back safely. RGB DXT1 uses native RGBA DXT1 to retain the
software compatibility alpha behavior. Software rendering, GPU disable and
subimage updates materialize deferred levels. Successful pixel patches invalidate
only the affected compressed level; resize/deletion releases owned payloads.
Native mip generation is limited to textures without authored guest mip levels.
Partial/complete authored levels remain preserved by the existing CPU chain path.
Driver refusal drains its error and retries RGBA/CPU; native OOM propagates.
Packed vertex layouts cache attribute pointers/enables per private GL context;
uniform value storage is reused; texture stages and guest array descriptors use
fixed-size per-draw arrays. No runtime instrumentation was added.
Validation: 150 GLES tests (all native tests and benchmark included), 284 WinCE
unit tests including native tests, ARM64/ARMv7 Rust GLES checks with warnings
denied. Native route tests cover optional-function absence, compressed upload,
deferred materialization, authored mip preservation and actual driver refusal.
Full Windows/APK builds and game visual/FPS regressions require target devices.

#### GPU depth precision (2026-10-11)

RBR Android reports moving dark terrain patches absent in software. Native FBOs
previously forced 16-bit depth whereas software stores f32. Prefer 24-bit depth
on desktop/GLES3/GLES2 OES_depth24, retaining 16-bit for unsupported GLES2 or
advertised-but-refused/incomplete 24-bit attachments. Allocation failures still
propagate. No new renderbuffer is created; existing resize/drop ownership applies
to both window and offscreen targets. Native regression demonstrates two nearby
layers collapse with 16-bit and separate with 24-bit, matching software after
attachment resizing. 151 GLES tests, 284 WinCE tests and both Android Rust GLES
checks pass. RBR root cause and visible improvement need target-device confirmation.
Filtering is unchanged; base-level software sampling and GPU mip filtering differ.

#### Guest maximum mip level (2026-10-11)

RBR sets GL_TEXTURE_MAX_LEVEL_SGIS to authored count minus one; it was accepted
and ignored. Texture now retains max_level (default 1000) and changes invalidate
revision. Renderer applies native max-level on desktop/GLES3/APPLE extension;
level zero uses matching base minification filtering. Core GLES2 fallback uses
OES_standard_derivatives to estimate implicit LOD and cap texture2D bias. This
estimate can differ from the driver's implementation. Without derivatives or
native caps, restricted textures use base filtering (possible shimmering), not
unsupported GL enums. Unrestricted filtering is unchanged. Shader derivative
extension is enabled only when advertised; uniforms/layout caches remain reused.
Reject negative and nonfinite float setters, preserving texture state on errors.
Native regression covers caps 0/1/default, removing/restoring caps, nearest and
trilinear modes, native/forced shader/no-derivative paths. 151 GLES tests, 284
WinCE tests and Android ARM64/ARMv7 GLES checks pass. Full Windows/APK builds and
RBR visual improvement still need target-device validation. No instrumentation.

#### Configurable anisotropic filtering (2026-10-11; removed)

LauncherConfig.anisotropic_filtering is u8 (0=Off,2,4,8; default0). GameSettings
uses Option<u8>, None inherits global. Android Kotlin mirrors JSON; no independent
SharedPreferences storage. English Emulator options/per-game controls are disabled
when effective GPU is false; GPU selection changes update availability without
forgetting the saved level. All launch paths pass the resolved level to Context.
Selection alone never enables GPU. Invalid runtime choices normalize to Off.
Renderer queries advertised EXT/ARB capability once, caps to driver max (up to8),
caches applied sampler values, restores1 when Off, removes cache entries on
texture deletion. Context initializes new renderers with its selection and
reapplies it when activating parked surfaces; parent context snapshots retain it.
No extra native resource/worker. CPU software filtering is unchanged.
Validation: 153 GLES tests,284 WinCE tests, Rust persistence matrix/reopen, Kotlin
JSON defaults/modes/inheritance and unknown-field roundtrip, core check without
default features, ARM64/ARMv7 Rust GLES checks with warnings denied, Android XML
parse. Native tests read actual driver anisotropy and exercise reset/deletion/
unsupported capability. Full Windows/APK and device UI/visual/FPS tests pending.

#### Reusable typed vertex streams (2026-10-11)

Context merges overlapping/adjacent enabled client-array spans (maximum 1 MiB
per span) and pools owned read buffers across draws. Each draw rereads guest
memory; failed bulk reads retain per-element fallback. Captured VBO bindings
use current host buffer contents, including BufferSubData changes. Index widening
reuses storage. Short reads, checked address overflow and invalid bounds never
expose guest pointers to the driver.
Smooth GPU draws with dense indices or sequential ranges pack native typed
attributes into one reusable owned upload buffer. Position, normalized colors,
normals and active texture coordinates preserve their formats; unsupported
native FIXED or misaligned strides convert to float. Padding is omitted. Dense
indices are rebased without a hash map; holes/sparse ranges, flat shading and
unsupported topology retain the existing Vertex path. Attribute layouts reset
correctly when alternating paths; disabled arrays use current constants. Matrices,
lighting, texture state and synchronous framebuffer presentation are unchanged.
All scratch/native resources remain owned by Context/Renderer and release on drop;
no worker, persistent guest-address cache or new native buffer is introduced.
Validation: 155 GLES tests including real desktop GL/GLES drivers, mutable guest
data, VBO capture/update, normalized colors, first/index offsets and path switches;
284 WinCE tests passed; Android ARM64/ARMv7 checks with warnings denied. Full Windows/APK builds and actual
Colors performance remain device validation tasks. No temporary instrumentation.

#### EGL display cadence (2026-10-11)

GAPI GXEndDraw now shares this modeled display clock (2026-10-21). Wait
before framebuffer readback, including unchanged frames, retain the deadline
across cooperative retries, and record completion after publication. Preserve
DirectDraw's one-shot explicit-blank credit. Snails previously rendered its
timer-driven menu hundreds of times per second. This is the existing 60 Hz
presentation policy, not a claim that the original GXEndDraw ABI guarantees
vertical synchronization. Do not scale the guest clock or rewrite its timers.

Battlestations imports eglSwapBuffers (static call site 0x25b6c). Frontend
snapshot throttling does not constrain the guest simulation; the old EGL
handler completed as fast as rendering allowed. Window swaps now use the same
60 Hz modeled display clock and cooperative wait_display helper as DirectDraw.
This is a general presentation policy, not a game-name branch or guest clock
scaling. Both software and GPU swaps follow it. First/late frames are immediate;
following swaps cannot catch up in bursts. Identical images are paced too.

Wait before conversion/publication/DirectShow/GAPI synchronization, preserving
arguments and deadline across scheduler reentry. Main waits yield to ready
workers or sleep at most 1 ms; worker waits use scheduler deadlines. Explicit
DirectDraw blank credit is consumed once, avoiding double waits on mixed paths.
Record completion in the shared clock after mapped framebuffer synchronization.
Pbuffer/pixmap swaps and eglWaitGL/Native synchronize only, without presentation
pacing or updating the display clock. Parent kernel state retains its own clock;
expired waits complete normally after resume. Pending maps are owned by the
framebuffer/kernel and drop with the process; no new worker/native resource.

Existing eglSwapInterval/NV compatibility acceptance is retained; programmable
intervals greater than one are not newly implemented. The 60 Hz cap is emulator
policy and not a claim of exact original GoForce swap behavior. Native/resource
paths and audio clocks remain unchanged. No temporary instrumentation.

Validation: all 286 WinCE tests pass, including native GPU presentation, EGL
retry arguments/deadline preservation, unchanged-image cadence, late-resume
behavior, offscreen exemption and existing DirectDraw worker/blank credit tests.
The unchanged GLES layer retains its 155-test validation from the vertex patch.
Full Android WinCE cross-check was blocked by absent NDK compiler for rmp3;
this is an environment limitation, not an APK build validation. Windows/APK
builds and Battlestations speed/input/audio must be checked on user devices.

#### Main-thread Sleep duration and Battlestations water cadence (2026-10-11)

Battlestations' timing block 0x268e4..0x2695c requests Sleep to fill a 50 ms
update budget (20 Hz). Actual ARM execution with controlled timeGetTime gives
50/40/20/1 ms sleeps for 0/10/30/49 ms elapsed, and no sleep when already late.
Its simulation counter increments at 0x1fb90; water texture selection is counter
modulo six at 0x23ee0. These are caustic_00000/04/08/12/16/20.tga. Water vertex
heights and colors also depend on that counter. Rendering used to accidentally
limit updates because positive Sleep durations were ignored on main, though
workers already had real scheduler deadlines. EGL's 60 Hz maximum alone cannot
restore a game's lower requested update rate.

Positive main Sleep now owns one monotonic deadline in existing wait_deadlines,
keyed by thread/thunk/SP. Yield to ready workers with resume_worker_reenter,
preserving original arguments and continuation; otherwise sleep host at most
1 ms and retry the same thunk. Do not restart or extend the deadline on retry.
Remove the entry only when elapsed; INFINITE remains pending. Sleep(0), worker
round-robin and worker sleeps retain their old yield behavior. Service waveOut
before argument lookup so callback return restores the interrupted Sleep call;
callbacks nested deeper on the stack cannot consume its deadline. Pending state
belongs to KernelState and releases with process teardown. No new worker/native
resource, clock scaling, title branch, texture edits or runtime instrumentation.

Uploaded TGA textures decode consistently. The game's actual ARM DXT1 compressor
0x1ea74 produces six 8192-byte images with no transparent pixels and mean RGB
142.7..146.1. Consecutive image changes remain part of the original animation;
this patch restores cadence, not temporal interpolation or wake/spray rendering.
Full APK/Windows builds and visible flicker improvement need device validation.

Validation: 290 WinCE unit tests passed, including native GPU and software EGL,
finite/infinite/zero main sleeps, original deadlines and argument preservation,
worker handoff and nested waveOut callbacks with positive/zero Sleep. The GLES
implementation is unchanged from its previously validated 155-test version.

#### Default GLES texture object zero (2026-10-11)

Battlestations binds simulation counter modulo six directly as texture names
0..5 (0x23ee0). The previous Context rejected image uploads to zero and excluded
zero from texture stages, leaving every sixth water frame untextured. Zero is
the GLES1 default texture object; it is not an instruction to disable texturing.
Allow ordinary/compressed/copy uploads and parameters on zero, lazily storing it
in the existing canonical texture map. Complete enabled default textures use
the same software/native stages as named textures. An undefined default image
remains incomplete. GenTextures still never returns zero; DeleteTextures ignores
zero in both canonical and native caches. Native guest zero maps to an allocated
host texture name, and follows existing revision invalidation and renderer Drop.
No additional thread, resource owner, game branch or instrumentation.

Regression covers DXT1 names 0..5 over 18 draws on software, desktop GL and GLES2,
including ignored zero deletion; ordinary default upload and named deletion
returning to the default binding are checked too. Device confirmation of the
Battlestations flash remains required.



#### Signed linear fog ranges (Carmageddon regression)

Actual ARM execution of Carmageddon_539Pa_25102005 reproduces a visible HUD
with the 3D scene replaced by almost-black fog in native GPU mode. The captured
state uses linear fog start -100.5 and end -343.28522. Software preserves the
negative end-start span; the fragment shader previously replaced it with a
positive epsilon, producing a zero fog factor for the world.

The shared native shader now divides by the signed span and uses factor 1 for
a degenerate range, matching software. No game-name branch, renderer switch,
texture edit or context-reset workaround is involved. A native regression checks
Carmageddon's range, descending positive ranges, ordinary ascending ranges and
equal endpoints against the software rasterizer on desktop GL and GLES2.

DirectShow presentation honors AutoShow on genuine Run/Pause graph state
changes (default enabled). Colors hides its intro window while preparing the
graph and relies on Run to reveal it. Live explicit hiding and AutoShow false
remain valid. No GPU shader, readback or video codec change is involved.


##### PVRTC1 texture uploads (2026-10-20)
GeoRally EX uploads RGBA PVRTC1 2bpp/4bpp (0x8C03/0x8C02), including explicit
mips down to 1x1. These uploads previously failed GL_INVALID_ENUM in the shared
context before reaching either rasterizer. Add only PVRTC-specific branches;
retain the previous ATC/DXT/paletted paths and compressed-format enumeration
order (append new formats). CPU decoding uses pinned texture2ddecoder 0.1.2.
Normalize its BGRA output to RGBA; force alpha 255 for RGB tokens only.
PVRTC payloads retain at least 2x2 words (16x8 for 2bpp, 8x8 for 4bpp), including
small mip levels; decode those physical dimensions then crop to logical size.
Require power-of-two dimensions, reject short payloads and nonzero borders.
The existing native compressed upload path may be used when the host supports
the format; otherwise both backends consume the same decoded RGBA data.
This adds full-image upload formats, not compressed subimage support; do not
advertise the complete IMG extension until its remaining entry points conform.
Spec: https://registry.khronos.org/OpenGL/extensions/IMG/IMG_texture_compression_pvrtc.txt
Dependency notices are retained under licenses/texture2ddecoder/.

<a id="history-cpu"></a>
## CPU profiling cleanup

### CPU optimizations without temporary profiling

This cumulative source delivery removes the temporary CPU phase/PC sampling,
backend counters, native Unicorn timing/control 0x5048 and GPU profiling probes.
It does not add a diagnostics module or a Cpu::take_backend_diagnostics method.
The private 0x5049 identity-mapping control is an optimization contract and is
retained. The normal activation log is retained; no benchmark probe is enabled.
Permanent emulator error logging and HARDWARE DIAG remain unchanged.

#### Retained optimizations

- ARM TB key fast path: rereads dynamic ARM/Thumb/IT/VFP inputs on every lookup;
  original fallback for XScale, M-profile, single-step/debug and AArch64 state.
  Full block key, tracing, cflags and invalidity checks remain intact.
- ARM float32-to-int32 truncation: finite normal values with magnitudes from
  one up to (excluding) 2^31 use integer extraction/shifts and preserve inexact
  flags; exceptional, small and boundary inputs retain the generic conversion.
- Direct inter-page ARM block chaining: PocketHLE opts in only after its fixed
  identity virtual TLB callback is installed. Generic Unicorn defaults and
  CPU-TLB fallback retain the original same-page rule. Changing TLB mode clears
  the private contract and flushes translated blocks. Indirect calls are unchanged.
- Win64 pointer atomics for linking/unlinking use 64-bit interlocked operations.
- Previously delivered GPU presentation/scratch optimizations and Agaju's
  InvalidateRgn implementation are included in this cumulative clean bundle.

The rejected division lowering, folded-key comparison, cache-hash specialization,
forced-inline experiments and temporary helper annotations are not included.

#### Installation

Apply patch-files.txt from the repository root, then delete only the optional
obsolete files in removed-files.txt. Rebuild normally so native Unicorn and the
Rust backend agree. Both manifests use Windows paths and CRLF line endings.
No diagnostics modules or sampling API need to be added to the Cpu interface.
The startup message is ARM identity-mapping block chaining enabled.

Host writes to translated code still follow Unicorn's existing explicit cache
invalidation contract. Guest stores, permission changes and unmap/remap are
covered by the retained inter-page regression test. No general virtual-to-
physical remapping guarantee is made for callers violating the private contract.

#### Validation and limits

The final native ARM engine is rebuilt from sources with all PH_NATIVE fields,
macros, counters and diagnostic control removed, rather than merely disabled.
The clean Cpu crate compiles without any diagnostics module or diagnostic trait
method. Its 37 applicable tests pass, including linked-code guest writes,
explicit host invalidation, permission revocation and unmap/remap. MIPS is
excluded from this ARM-only native runtime build.

The complete optimized local non-GUI app stack is compiled against the clean
Cpu/Kernel interfaces. The supplied PVR Racer and GLBenchmark are used for
bounded smoke runs; CPU Float/Integer result export is checked. Float-to-int
results and floating status are compared against the original native helper
on three million input patterns; all comparisons match. Both bounded app runs
complete successfully, with activation confirmed and no temporary profiling output.

Previous approximately 34% timing reduction applies to a synthetic direct
inter-page call workload, not global game FPS. Removing probes may change the
numbers, so no Windows speedup is inferred. Windows/MSVC (including the three
64-bit atomic branches), Android, AArch64/MIPS native builds and full CI remain
unverified locally. No claim of platform-wide conformance is made.

<a id="history-message-pump"></a>
## Blocking message-pump measurements

### Blocking main message pump and worker wakeups

GetMessageW blocks the calling thread until a message exists; it does not stop
other guest threads. After processing already-due messages and attempting the
normal worker handoff, a main pump with parked live workers retries through the
existing one-millisecond wait_display_until continuation. Keep the original
MSG pointer, filters, LR and blocked-call semantics. Do not return FALSE or
invent a message merely to yield. Actual due messages keep their existing
priority, and worker readiness/quantum rules remain unchanged. The existing
single-thread fallback is unchanged. State belongs to the guest process.

Previously the fallback slept until synthetic paint (up to16ms). Worker sleep,
waveOut/event readiness could therefore wait for unrelated paint traffic.

#### Local diagnostic comparison (2026-10-29)

Two user-provided Tomb Raider saves, same executable and tr.opt, QVGA240x320,
Unicorn/software guest rasterizer, unlimited user cap. Separate data copies;
18-second warmup then10-second idle samples; sequential headless Linux runs,
without host audio-cpal. Temporary instrumentation was removed from production.
These are local measurements, not Windows/Android device results.

| Scene | Completed GXEndDraw/s before | Diagnostic bounded-wait/s | Changed images/s before | Diagnostic bounded-wait/s |
| --- | ---: | ---: | ---: | ---: |
| Initial position |31.8|57.2|25.0|25.0|
| Water position |32.2|57.4|32.0|53.7|

The diagnostic variant used a1ms sleep followed by retry of the same main thunk;
production reuses wait_display_until for that continuation. The new regression
checks worker eligibility, original MSG pointer preservation and actual WM_QUIT.
The gameplay impact while moving and physical host audio remain user validation.
A greater presentation rate can expose games coupling simulation to frame rate;
per-game user frame caps remain unchanged.

#### Independent presentation rate

Framebuffer.presentation_stats is a shared, process-owned rate sampler. Successful
GXEndDraw, primary DirectDraw Unlock/Flip, full retained GAPI framebuffer copies
and onscreen eglSwapBuffers record a completion, including identical frames.
Scheduler retries, offscreen EGL swaps and ordinary readback polls do not count.
The shared handle survives presentation-only framebuffer clones; snapshots expose
telemetry without additional pixel transfers. It becomes idle after a second
without completion and resets on GXCloseDisplay. Existing frame caps and pixel
revision semantics remain unchanged.

Desktop/Deck/Linux display completed presentations as FPS; Android polls the
same sampler through JNI, independently of nativePollFrame. Without an explicit
boundary (ordinary GDI primitives or retained GAPI pointer polls), the frontends
retain the old changed-image publication rate and label it IPS. A universal
application frame rate cannot be inferred from arbitrary GDI drawing commands.
No fake vblank or implicit frame boundary is added. The FPS option controls both.

Regression tests cover identical pixels, rate/idle/reset, shared snapshot handles,
GX scheduler retries and identical presentations, EGL swaps/retries and the
blocking-message worker continuation. Native Windows/Android builds and gameplay
with moving camera/audio remain platform validation.

<a id="history-diagnostics"></a>
## Diagnostic/resolution delivery

### Diagnostics and internal-resolution follow-up

Source reference: guizmox/PocketHLE b0422921e1d490932329b059b924a76487e76a94,
rechecked on 2026-11-01. This is the cumulative pending diagnostic / GLES
resolution patch, including the unified tool sources and earlier GPU resolution
implementation. It replaces the previous diagnostic/benchmark/resolution ZIPs;
it is not a complete repository. No game executable or user report is included.

#### Integer correctness

Unicorn can restart its internal execution after a runtime helper redirects PC.
A concurrent slice timeout can leave the persistent stop request set while the
CPU-local exit request has been reset by that restart. The code-hook dispatcher
then skips the callback, yet the intercepted instruction could still execute.
The generic exit helper now honors either pending request and still defers exits
inside a branch delay slot. Explicit PC redirects retain their existing handling.
This fixes the execution boundary, not the benchmark or division inputs/checksums.
No optimization setting is disabled and no game-specific exception is added.

A new ARM backend regression sums 1024 distinct 64-bit divisions across repeated
short time slices. Both return words are checked against an independent host
sum. The existing interworking, register-preservation and helper-resumption tests
are retained.

#### Results and CI

The single ARM HARDWARE DIAG EXE now ends Integer, Floating and OpenGLES on a
compact score page. B/Tab switches to the full text log; A/Enter/Back returns to
the menu; Left/Right changes between completed score pages. Full TXT report keys,
workloads, checksums and median/global calculations are preserved. Missing or
truncated reports show FAIL. Score-page dimensions are checked at native 320x240.

An independent GitHub Actions job builds, verifies and publishes HARDWARE-DIAG,
and runs the ARM diagnostic/report/UI smoke tests. It uses Python 3.12 and the
small Clang/LLD toolchain, with no Rust/FFmpeg build dependency. Other jobs still
build their existing private diagnostic fixtures for integration tests.

#### Rendered resolution and display zoom

The source image for presentation is the actual GPU-rendered image. Desktop
manual display zoom remains relative to that produced image: a 320x240 logical
guest rendered internally at 640x480 and zoomed 2x targets 1280x960 where it fits.
Android has no manual Display scale setting: PocketPC automatically chooses the
largest fitting whole-number multiple of its logical LCD, independently of the
GLES density. A 320x240 guest therefore fits at 960x720 even if GLES produces
640x480 or 1280x960. All fitting integer steps are allowed. Native VGA uses its
own 640x480 logical grid; Gizmondo uses automatic produced-image integer fitting.
Controller fullscreen retains aspect fit. Sample the full-density texture directly.
The Android renderer, reserved PocketPC surface and inverse touch mapping share
the same rectangle. Input still returns logical guest coordinates. GDI, GAPI and
software-rendered games do not gain extra internal geometry detail.

#### Applying and retesting

Copy the files at their original relative paths. patch-files.txt lists payload
paths with Windows separators. removed-files.txt lists obsolete standalone test
files from the unified-tool migration; it contains no new emulator-source deletion.
Keep any more recent local unrelated work when applying this cumulative patch.

Rebuild PocketHLE normally, including the bundled Unicorn engine. The existing
build script tracks the modified C source; deleting all build caches is not
required. For Android, rebuild native libraries and APK together. Re-import
`tools/hardware-diag/dist/HARDWARE-DIAG.zip` to update the diagnostic executable.
Importing the EXE alone does not install the CPU-engine fix.

#### Targeted verification and limits

- Built and archive-verified the integrated ARM diagnostic from the delivery tree.
- ARM CPU workload smoke checks pass all 16 added cases, medians and unchanged
  historical scores; injected wrong arithmetic and backwards clocks are rejected.
- ARM GL smoke checks pass all 10 workloads and median/global aggregation; missing
  APIs, GL errors, black output and stalled clocks are rejected.
- Actual ARM score pages pass score parsing, screen bounds, incomplete/failed
  reports, menu and log navigation checks.
- The CPU regression fails with the uncorrected reference engine and passes with
  the corrected exit behavior. The real compiled diagnostic's unsigned and signed
  division blocks each pass 10,000 repetitions with thousands of timer stops.
  These isolated tests reuse an existing reference-engine library with a private
  test-only exit shim; no shim, offset, control bypass or trace is shipped.
- Compiled the changed C exit helper against controlled stop/restore test doubles,
  including lost CPU exit bits, branch delay slots and explicit PC redirects.
- Six desktop geometry tests and the actual desktop image-filter source check pass,
  including 640x480 -> 1280x960 and portrait dimensions. Rust formatting checked.
- Kotlin tests are supplied for rendered-size scaling, rotation, logical touch
  coordinates and existing native/fullscreen policies. They were not run here.
- Prior native desktop GL/GLES2 internal-resolution, readback and snapshot tests
  remain applicable; the GPU rasterization implementation is unchanged here.

No full desktop/APK build, bundled-engine rebuild, device run or complete GitHub
CI execution was performed here. These targeted checks do not certify a physical
Android device or all games.

<a id="history-resolution"></a>
## Old internal-resolution delivery

### OpenGLES internal resolution

The global **Emulator Settings > Graphics > OpenGLES internal resolution (GPU
only)** setting is available on desktop (Windows/Linux/Steam Deck) and Android.
It applies on the next launch and defaults to **Native (1×)**. The GPU choices are:

- **Native (1×)**: original rendering size.
- **2x - only for 320x240 games**: double 320×240 or 240×320 surfaces; other
  native sizes, including 640×480 and 480×640, remain unchanged.
- **2x - also for 640x480 games**: double every native GLES surface, including
  640×480 → 1280×960 and 480×640 → 960×1280 (and larger native surfaces).

The existing serialized value 2 keeps its previous all-sizes meaning. Value 3
is the new QVGA-only policy; value 1 remains native. The policy is evaluated on
logical surface creation/resize, so a size/orientation change or restored EGL
surface does not inherit the previous surface's effective scale. The normal
no-resize draw path does not evaluate the policy again.

This is actual geometry rasterization into a larger color/depth framebuffer,
not a post-process resize. Textures/sprites keep their original source detail.
It does not change the guest's reported LCD resolution, GAPI memory, viewport
queries, touch coordinates or control hitboxes. Display zoom is applied separately to the
actual rendered image. The software GLES
renderer, GDI, DirectDraw and direct GAPI games keep native rendering.

Native GPU viewport/scissor and point/line sizes account for the density, within
the driver's supported point/line size limits. Guest
read/copy APIs receive a logical-size resolved image. The full-density image is
kept separately for host presentation; it is not downscaled then enlarged again.
The existing readback is reused for both images, with extra conversion/memory
work at 2×. Four times as many rendered pixels can reduce performance. This is a
host renderer option, not a claim about original device hardware capabilities.

Ordinary GDI/GAPI writes invalidate the host-only image. Native SIP/menu overlays
use the native composite while visible. Returning to GLES restores full-density
presentation at the next swap. Rotation/aspect ratio and integer display scaling
use the produced image dimensions: internal 2× makes 640×480, then display 2×
requests 1280×960 (portrait: 480×640 then 960×1280), limited by the available
space. No presentation step first reduces that image to 320×240. Touch mapping
uses the same display rectangle but still returns logical guest coordinates.
The pre-existing presentation policy of native VGA PocketPC games is preserved. Offscreen reads and copy-to-texture
operations preserve guest-sized buffers; this option cannot increase an image
resource's dimensions or remove every low-resolution post-processing effect.

#### Applying

Apply the modified files, rebuild PocketHLE, then choose the setting and relaunch
a GLES game. On Android rebuild/install the native libraries and APK together.
Signing/keystore behavior is unchanged. CI separately publishes HARDWARE DIAG.

Keep Native (1×) for before/after performance comparisons unless deliberately
comparing the two resolutions. Record the chosen setting alongside HARDWARE DIAG
results: the guest sees the same logical viewport, so its benchmark cannot infer
the host-only pixel density.

#### Targeted validation

- Compiled the modified GLES crate using existing dependency artifacts.
- Ran native desktop OpenGL and GLES2 tests that verify real subpixel geometry
  differences within a 2×2 block (rejecting simple pixel replication), logical
  readback size, scissor scaling, resizing and return to 1×.
- Ran context configuration/presentation and software-path tests.
- Existing native depth/blend/alpha/texture/context-switch and GLES2 orientation/
  scissor tests passed with native resolution.
- Ran 14 checks using the actual framebuffer, snapshot producer and JNI header
  source sections, including both frontends' geometry invariants and legacy
  eight-byte/native versus twenty-byte/high-resolution wire headers.
- Added pure Kotlin payload tests for native/high-resolution frames, malformed
  lengths and oversized dimensions. These are included for Android unit tests;
  no complete Android/desktop build or full GitHub CI run was performed here.

#### Steam Deck startup

The dedicated Steam Deck build starts fullscreen even when the previous desktop
configuration was windowed. It requests fullscreen in the initial native window
builder and once on the first UI frame, after native window-state restoration.
Subsequent fullscreen controls work normally. An explicit CLI display argument
still takes priority: `0` fullscreen, `1` windowed. This applies to the dedicated
Steam Deck build (`steam-deck` feature), not automatic detection in a regular
Windows/Linux desktop binary.

#### Recognizing a fixed-size software framebuffer

GAPI imports alone do not establish whether a game's renderer is resolution
independent (nor do they rule out dynamically loaded GL). Source review or a
trace must follow the width, height and signed X/Y byte pitches returned by
GXGetDisplayProperties into buffer allocations, row addressing, drawing loops,
clipping, depth buffers and the 3D projection. A hardcoded 320×240 buffer or a
640-byte row stride for 320 RGB565 pixels is evidence of an assumption, not
proof from the mere presence of those constants. Engines may query display
properties but still render into a fixed-size private buffer and then copy it.
Changing the reported framebuffer size alone cannot safely add rendering detail.

#### Follow-up validation

Policy selection and actual native framebuffer resizing were checked for QVGA,
VGA, both portrait orientations, policy changes, and parked EGL surfaces.
Existing native internal-resolution geometry/readback tests were rerun. Startup
preference/CLI precedence tests passed. Kotlin configuration roundtrip tests are
included for Android CI; no full Android/desktop build or physical Steam Deck
launch was performed here. No game-specific framebuffer patch is included.

<a id="history-desktop-controls"></a>
## Old desktop-panel delivery

### Gizmondo desktop controls

Apply after the preceding diagnostics/resolution, resolution-policy/Steam Deck
and recording-label patches. This patch retains those changes in shared files.
The source reference is guizmox/PocketHLE b0422921e1d490932329b059b924a76487e76a94.
Only Windows/Linux/Steam Deck presentation is changed; Android layout is unchanged.

The photographed console skin is replaced by a dark PocketPC-style control panel.
The five function buttons stay above the game: Home, Volume, Brightness, Geofence
and Power. Geofence is drawn as a vector GPS antenna. L/R are taller downward;
the four media action buttons are circles in the Stop/Rewind/Forward/Play diamond.
The existing cross D-pad stays at the left. All guest controls use the user's
existing bindings and illuminate for held keyboard/controller/pointer input.
Guest key codes are unchanged (including Power/Piano 5 = F11).

Fullscreen Gizmondo now displays this same panel, including on Steam Deck, with
Exit/Full controls. PocketPC fullscreen behavior is unchanged. Image geometry uses
rendered pixels for zoom and logical pixels for guest input. Controls never cover
the game. Small viewports fit the whole image; Steam Deck fullscreen preserves
its aspect-fit policy. Window sizing reserves the same controls/header as drawing
and guards physical-pixel rounding at fractional DPI against lost integer zoom.
Round action buttons also have round pointer hit tests.

Recording labels use the Gizmondo profile. Brightness and Geofence are written in
full; the existing video marker expands horizontally for longer function names.
The PocketPC label profile and short circular video markers are retained.

removed-files.txt contains the unused desktop skin asset, now unreferenced.
patch-files.txt contains relative Windows payload paths. No game file is modified.

Targeted checks: eight geometry tests passed across viewport sizes, rendered
resolutions, DPI values and window/fullscreen scale policies, including control
separation and round hit tests. Three recording tests passed using the actual
label/state/overlay source: both profiles, profile transitions, full long names,
and pixel-identical short markers. Changed Rust files pass rustfmt. No complete
Windows/Linux/Android build, GitHub CI run or physical Steam Deck run was performed.

<a id="history-cerf"></a>
## CERF design comparison

### CERF-inspired integrations

PocketHLE and [CERF](https://github.com/gweslab/cerf) solve different problems:
CERF emulates complete Windows CE devices, while PocketHLE runs individual
native game binaries through a high-level API boundary. CERF is MIT-licensed;
this document records which ideas are useful here and how they map onto
PocketHLE instead of copying CERF's device-specific C++ implementation.

#### Adopted now

##### Safe, layered virtual storage

The VFS now selects the most specific guest mount. This makes a broad ROM or
application mount safe to combine with a narrower save-data mount. Guest paths
remain case-insensitive, `..` cannot escape a mount root, and read-only mounts
are available for ROM and bundled assets.

The intended layout is:

```text
\\ROM\\            read-only extracted game assets
\\Application\\    writable per-game data and saves
```

A frontend can mount these independently with `Emulator::mount_dir` and the
kernel VFS API. The guest never receives an arbitrary host path.

##### Atomic launcher metadata

`library.json`, `config.json`, and per-game manifests are written to a
temporary file and renamed into place. A cancelled or interrupted write should
not leave a half-written JSON manifest that prevents the launcher from opening.

##### Documentation-driven boundaries

CERF's documentation distinguishes the machine layers clearly: CPU/JIT,
board and SoC peripherals, storage, presentation, input, and state. PocketHLE
uses the same separation at a smaller scale:

- `pocket-cpu` owns instruction execution and register/memory access.
- `pocket-kernel` owns process memory, VFS, framebuffer, and scheduling state.
- `pocket-winceapi` owns the host implementations of WinCE APIs.
- frontends own presentation and input translation.

This prevents board-level emulation from leaking into game-specific API
handlers.

#### Deliberately not copied yet

- **Full-device board and peripheral emulation.** It is outside PocketHLE's
  HLE goal and would duplicate CERF's architecture rather than improve the
  current game path.
- **CERF save states.** CERF snapshots CPU, MMU, RAM, flash, peripherals, and
  presentation with a versioned section format and compatibility fingerprint.
  PocketHLE does not yet expose a complete serializable `Process`/CPU state,
  so pretending to support save states would be unsafe.
- **ROM container parsing.** CERF's ROM documentation covers NB0/B000FF,
  IMGFS, and OEM packages. PocketHLE currently imports CAB/ZIP/raw PE files;
  ROM-container support is a separate loader milestone, not a file-copy task.
- **Guest Additions, PCMCIA, serial modem, and network hardware.** These are
  complete-device features that do not belong in the current HLE core.

#### Next implementation targets

1. Add a versioned, section-framed snapshot format after CPU and process state
   have explicit serialization APIs.
2. Add ROM-container inspection/extraction as a separate crate or CLI command,
   reusing the documented format rules without distributing Microsoft binaries.
3. Add deterministic crash reports containing the last guest PC, registers,
   active thunk, and a bounded memory window.

#### References

- [CERF subsystem architecture](https://github.com/gweslab/cerf/blob/main/agent_docs/subsystems.md)
- [CERF feature guide](https://github.com/gweslab/cerf/blob/main/docs/website/content/articles/features.md)
- [CERF pointer/input guide](https://github.com/gweslab/cerf/blob/main/docs/website/content/articles/pointer-input.md)
- [CERF state image format](https://github.com/gweslab/cerf/blob/main/cerf/state/state_image_format.h)
- [CERF ROM container guide](https://github.com/gweslab/cerf/blob/main/docs/website/content/articles/rom-containers.md)
- [CERF command-line diagnostics](https://github.com/gweslab/cerf/blob/main/docs/website/content/articles/command-line.md)

<a id="history-navigator"></a>
## Navigator build 27

### Navigator graphics — cumulative build 27 (historical)

This was a PocketHLE 0.3.3 delivery based on the user's crates(1).zip. Existing
Navigator/CoPilot, VFS, controls, modal-window, Gizmondo path and SkyStriker fixes
were retained. No ARM executable patch or address-zero write bypass was added.

The supplied archive lacked root Cargo manifests and RAMTEST DLL fixtures.
Those metadata/fixtures were obtained from the repository at the time; user
sources took precedence. This is provenance, not permission to replace the
current checkout with an old Git snapshot.

#### Demonstrated changes

Missing ImageList handlers prevented controls from receiving images. A zero
ImageList_Create result propagated into replacements on a null handle. LoadIconW
also returned a constant placeholder, so lists alone would not repair loading.

- Process-owned image lists retain real dimensions, indices, independent copies,
  add/replace/remove/destroy behavior, counts and image sizes.
- Resolve bitmap/icon/cursor resources in their owning module, including native
  DLLs. Decode DIB/bitfields, RGB565, AND masks, monochrome inversion, 32-bit alpha
  and PNG within explicit allocation limits.
- Named COREDLL/COMMCTRL handlers use real pixels for LoadIconW, LoadImageW,
  LoadCursorW, DrawIcon/DrawIconEx and ImageList APIs. Synchronize guest-owned
  DIBs before/after drawing.
- List views use LVITEM images and associated lists, coherent rendering, row
  sizing, hit testing and selection scrolling. Buttons/static controls show
  their images; composition does not fabricate additional completed frames.
- GetFileTime uses available VFS metadata and CompareFileTime compares 64-bit
  FILETIME values. Unavailable timestamps, including RAM files, remain zero.
- SetClassLongW/GetClassLongW retain per-class attributes and subclass behavior;
  RegisterClass/GetClassInfo reflect them.
- ScrollWindow/ScrollWindowEx handle overlap, clipping, exposed rectangles,
  child movement and invalidation. Erase is deferred to exposed areas; GDI
  regions are rectangular in this supported profile.

#### Limits of that delivery

No CoPilot executable, native DLL or map data was supplied for that run. Navigator
was not launched, and neither the original ARM crash nor arrival in navigation
was revalidated. Keyboard-selection testing did not exercise the entire console
input path. Windows startup, DLL resources, menus and navigation remained pending.

LR_LOADFROMFILE, image-list overlays, a complete desktop Win32 ListView and
SW_SMOOTHSCROLL animation were outside that profile. No COMMCTRL ordinal was
invented. Its VALIDATION-BUILD27.txt and verification directory belonged to the
old delivery; they are not implicitly present in the current checkout.
