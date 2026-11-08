# Graphics, presentation and performance

This is the current graphics guide. Older rasterizer milestones, regression
investigations and historical test counts are retained in
[implementation notes](IMPLEMENTATION-NOTES.md#history-gpu), not as current
feature switches or universal compatibility claims.

## Rendering and presentation

The guest CPU still executes application logic and WinCE API calls. Host GPU
rendering moves supported transforms, lighting, rasterization and texture work
to the native backend; guest memory validation, unsupported format preparation
and dispatch remain CPU work. Software GLES is a separate supported backend.

Shared presentation passes an immutable GPU image to a compatible UI consumer.
It avoids the normal CPU pixel readback/upload round trip when that path is
available. CPU presentation remains necessary for real CPU/GDI composition,
unsupported sharing or layouts, recording/readback requests and software paths.
A CPU_UPLOAD log does not by itself mean software GLES rendering. A shared
publication already consumed is not a new image to publish again.

Do not describe all presentation as synchronous readback or claim every frame is
zero-copy. The correct path depends on pixel authority and the consumer.
Initialization failure may select software; runtime GPU failure must be reported,
not silently hidden by continuing with stale software depth or color state.

## Authority and lifetimes

- Preserve CPU/GPU authority epochs, immutable snapshots, leases and ready/done
  fences. Never overwrite a leased texture or display a stale image after a
  newer CPU mutation.
- Desktop uses a latest-image mailbox rather than an accumulating frame FIFO.
  Superseded images release outside its lock. The native pool is bounded to five
  slots; bounded queueing and bounded resource storage are separate requirements.
- ARM identity virtual-TLB mapped GAPI can retain a deferred data provider.
  Real data reads and partial writes materialize; TLB maintenance FETCH requests
  must not read pixels or promote pending data into executable memory.
- Real GDI writes invalidate older GPU authority and update mapped guest data
  before guest execution resumes. Unsupported memory layouts/backends use the
  established eager path.
- Empty, hidden or unchanged native/control paints do not constitute CPU writes.
  Preflight visible content and generation changes before acquiring cpu_mut.
  This includes the generic hidden-control repaint correction used by Q3CE.
- Finish the worker before closing the UI share group. Preserve parent/child
  context restoration, EGL surface lifetime and restart behavior.

## Internal resolution

Internal rasterization resolution, guest logical resolution and display zoom are
three different quantities. The global GPU-only setting applies next launch.
Portrait equivalents are included in the QVGA/VGA policies.

| Serialized value | Menu choice | Effective GPU rendering |
| --- | --- | --- |
| 1 | Native (1x) | Native logical surface size |
| 3 | 2x, only for 320x240 games | Double QVGA; larger surfaces stay native |
| 4 | 3x, only for 320x240 games | Triple QVGA; larger surfaces stay native |
| 5 | 4x, only for 320x240 games | Quadruple QVGA; larger surfaces stay native |
| 2 | 2x, also for 640x480 games | Double supported native surfaces, including VGA |

The nonsequential values preserve compatibility: value 2 retains its original
all-sizes meaning. Do not renumber them to match menu order.
Evaluate the policy on surface creation/resize, including parked/restored
surfaces. Viewport/scissor and point/line density follow the effective scale.
Queries, input coordinates and guest read/copy buffers stay logical-sized.
Present the produced high-density image directly; do not downscale to QVGA and
then upscale it again. Real CPU overlays use the appropriate native composite.

Software GLES, GDI, DirectDraw and direct GAPI do not gain internal geometry
detail from this option. Source textures and low-resolution text/sprites retain
their original detail. Multipliers increase pixel count quadratically and may
increase GPU, memory or logical-resolve cost.

## Display size and filters

Display geometry uses produced image dimensions, preserves aspect ratio and
shares its rectangle with inverse input mapping. Prefer the applicable integer
presentation grid when enlarging; proportionally downscale an oversized produced
image instead of clipping it. Do not promise identical zoom labels/policies for
all Android control modes and desktop/Deck layouts. Android display size is
automatic. PocketPC uses all fitting integer multiples of 240x320 or 320x240 for standard-aspect images,
independent of internal rendering resolution (for example, a 320x240 guest
rendered at 1280x960 can display directly at 960x720). Native VGA uses that same QVGA presentation grid. Reducing the available area
drops to the previous integer step; only areas smaller than one QVGA step need
proportional downscaling. Nonstandard aspect ratios retain their native grid. Gizmondo keeps produced-image integer fitting; fullscreen
keeps aspect fit. The removed Display scale setting has no backend configuration
field or manual-zoom path. See [frontend geometry](FRONTENDS.md#image-geometry-and-input).

The filter carousel includes **No filter**, reconstruction, SMAA variants, xBRZ,
bicubic, Lanczos, bilinear and nearest. A selected filter is not limited to QVGA
or to enlargement: native and internally scaled images can still benefit.
Filters do not regenerate missing texture detail. xBRZ includes CPU preprocessing.
Android Menu tap cycles this same carousel on the GL thread. Changing filters
invalidates the presentation upload (including xBRZ dimensions); SMAA is only
processed while selected and is reused within the current GL context.
Standalone configurable anisotropic filtering was removed; historical descriptions
of its controls and JSON fields are not current settings documentation.

## FPS and frame limits

**Show FPS** measures image identities drawn by the frontend. **Show IPS**
measures changes to the logical RGB565 image. GX compares completed images
using the existing readback. EGL measures completed window swaps, and GDI
measures published images after drawing operations have been grouped.
With both options enabled, the badge combines these measurements and updates
once per second at the top right. Both rates are independent of physical scanout.

EGL uses its CPU pixel presentation path while measuring IPS, including when
shared textures would otherwise be available. This can affect performance.

The global limit is inherited by a game's **Emulator default** choice; an explicit
per-game choice overrides it. Current choices include 60, 120 and Unlimited;
apply changes at the next launch. A cap does not accelerate a game that sleeps
or simulates more slowly. Internal resolution does not bypass the game limit.
Host VSync, explicit guest vertical-blank waits and the modeled display clock
remain distinct; Unlimited is not a promise of unlimited physical refresh.
Desktop general settings expose VSync (enabled by default, applied after restarting
PocketHLE). Disabling it can allow tearing. The desktop publisher follows the
selected game limit for completed presentations, including 120 and Unlimited;
there is no fixed 16 ms ceiling on explicit EGL/GX/DirectDraw presents. Without
an explicit presentation boundary, Unlimited coalesces intermediate dirty GDI
updates at 60 Hz to avoid treating every blit as a complete image.

## Input-induced desktop stutter

The clean fix uses native frame-ready notifications and a coalescing mailbox.
Run-mode UI follows arriving images, with a 100 ms fallback when images stop.
The fallback is not a 10 FPS cap. Inputs remain queued; no game/GPU name test
or modified deadzone is required.

The local egui 0.27.2 adaptation adds runtime repaint_on_input, default true and
not persisted. Eframe temporarily disables automatic input-follow-up repaints
for the image-driven root Run update, preserving explicit UI requests and
ordinary menu behavior. The fix does not remove glFlush, cache WGL lookups,
change fences or require a diagnostic launcher.

Windows GTX1650 keyboard validation reported about 60 received snapshots/s and
about 10 ms/s context-detach time, versus about 20 snapshots/s and 925 ms/s before.
These are one hardware reproduction and CPU wall-time observations, not a general
GPU benchmark. Intel UHD fullscreen also reproduced the original symptom.

The controller complement changes SDL publication wakes while retaining every
analog value and button transition. The wheel/touch complement defers Run-mode
wheel and touch-Moved redraws; touch start/end/cancel remain immediate. These
complements passed targeted software tests but their hardware validation is still
pending in this session. Do not report them as confirmed Switch Pro/touch fixes.
Android uses a separate frontend and is not covered by the eframe correction.

## Validation and compatibility boundaries

For Android performance investigations, use the current
[graphics log collection guide](ANDROID-GRAPHICS-DIAGNOSTICS.md).

HARDWARE DIAG samples costly paths; it is not exhaustive OpenGL ES conformance.
Compare software/GPU geometry, lighting, alpha, textures/mips, HUD, offscreen
surfaces, read/copy operations, child/parent return, rotation and captures.
Khronos API names being registered does not alone prove complete semantics.
Compatibility also includes the observed Gizmondo GoForce driver behavior;
do not silently substitute a vendor quirk for the general API contract.

Use the same binary, scene, settings, resolution, device power state and driver
for before/after comparisons. Report CPU submissions, GPU rendering and displayed
images separately. Do not infer a platform-wide speedup from a synthetic test.
The archived maximum-mip, texture validation, alpha/depth-clear and compressed-
texture notes retain narrower reproduction details and their limits.

## Recognizing a fixed-size software renderer

GAPI imports alone neither prove resolution independence nor exclude dynamically
loaded GLES. Follow reported width/height and signed byte pitches into allocation,
row addressing, clipping, depth and projection. Hardcoded 320x240 or a 640-byte
RGB565 row is evidence of an assumption, not sufficient proof by itself. A game
may query the display but still render to a fixed private buffer. Increasing the
reported framebuffer size alone cannot safely add internal rendering detail.

## Android shared GLES presentation

GPU-enabled Android sessions create an independently retained EGL root before
starting any emulator process. Each replaceable GLSurfaceView context and each
native GLES renderer shares that root. The root is destroyed only after all
session/child jobs have joined. Native root metadata closes before Java destroys
the root. Android's existing process-wide display initialization lease also covers
borrowed contexts; the framework can release its own initialization without
invalidating the native renderers. This relies on Android EGL's reference-counted
initialization, not a portable EGL refcount assumption.

A valid shared window publication is taken before the frame hook performs any
CPU black scan, image-change scan or composition. The frontend transfers a
28-byte geometry/token descriptor, not image pixels or a persistent GL name.
Bounded latest/presented mailboxes retain immutable leases. A synchronous JNI
callback samples the texture inside the existing SharedFrame consumer/fence
scope; it does not retain the source name after returning.

The UI performs the established RGB565/row-order/backlight conversion into its
own texture on the GPU, using the same SHARED_DISPLAY_FRAGMENT as desktop, then
applies its existing filters/rotation/display geometry. GPU-created images retain
their physical density and logical guest size. CPU presentation remains for
software rendering, xBRZ preprocessing, IPS measurement and real CPU/GDI/system
video composition. Real guest reads still materialize pixels coherently.

Surface destruction releases only the consumer guard and context-local objects;
the independent root and last immutable image remain valid for surface recreation.
Shared UI texture/program objects are deleted through the root before the old
consumer context disappears, avoiding accumulation across recreations. If the
optional sharing/conversion transport fails, a forced CPU refresh can republish
the current image even when the guest is idle. It does not increment the guest
publication or completed-presentation counters. GPU renderer failures retain
their existing fatal reporting; this is not a software-renderer recovery scheme.

The log distinguishes `PRESENTATION=GPU_SHARED (Android)` from `CPU_UPLOAD`.
The diagnostic `shared_convert` stage times CPU submission of the GPU conversion;
it is separate from CPU pixel preparation and from asynchronous GPU execution.
No GL finish, CPU fence wait or unconditional readback is added. Snapshot pool
pressure can still select the established CPU path.

Local validation covers native GL/GLES pixel/backlight/row-order equivalence,
a producer on another thread, three consumer recreations with retained-image
readback, bounded mailboxes, static-image CPU recovery, wire decoding and the ARM
runner regressions. Android UI/factory Kotlin sources are type-checked against
Android API classes. A complete APK and Samsung/ANGLE runtime validation are
still required; this is not a measured device FPS guarantee or recovery from a
real device-wide EGL_CONTEXT_LOST event.

## Android packed CPU-origin images

Logical GAPI/GX/GDI images can keep their native little-endian RGB565 bytes all
the way through JNI. A versioned `-3` header carries logical dimensions and a
frozen backlight value, followed by exactly two bytes per pixel. No guest drawing
is moved to the GPU: the game/HLE still writes CPU memory, and publication/IPS
boundaries are unchanged. The optimization is host presentation preparation.

The ES3 presenter uploads the bits as an R16UI texture and performs exact integer
5/6/5 bit expansion and backlight arithmetic into its ordinary RGBA8 texture.
SMAA/reconstruction/rotation then use their existing path. This avoids relying
on normalized RGB565 sampling's rounding and keeps filtering after brightness,
as on the former CPU-conversion path. Row zero remains the logical top row.

`Framebuffer::presentation_rgb565_unadjusted` exposes only a complete native
logical image with a valid 16-bit byte layout. The existing desktop
presentation_rgb565 method retains its neutral-brightness-only contract.
Alternate/high-density render images keep their previous RGBA transport, without
losing physical resolution. xBRZ explicitly expands packed pixels on the CPU;
an unavailable/failing integer conversion also falls back to CPU expansion.
The GLES shared-texture transport remains separate and unchanged.

The UI logs `format=RGB565 conversion=GPU` for packed uploads, and explicitly
reports RGBA/CPU fallback. The input/output textures and programs follow the
same surface/share-group teardown rules as the shared presenter. Diagnostics
remain default-off; `rgb565_convert` measures CPU submission of this GPU pass.
The new opt-in `shared_snapshot_wall` span covers native shared snapshot export,
so another 1x/2x capture can distinguish export cost from explicit pixel readback.
It is inclusive of host activation/copy/fence submission, not GPU busy time.

Validation compares all 65,536 colors for backlight 1..31, neutral fallback
values and odd-width rows against integer CPU results through native ES3 on
Mesa. Snapshot tests verify immutable packed bytes/brightness and unchanged
logical/high-resolution geometry; protocol and xBRZ-expansion tests preserve
legacy packets. Full APK/device validation remains pending. This extension is
not a claim to have resolved the separately reported 8 FPS at 1x versus 20 FPS
at 2x; those runs still need their presentation/readback diagnostics correlated.

## Native image-decoder dependencies and zlib streams

A native graphics DLL can fail before making any display calls if one of its
imported dependencies is missing from the runtime HLE classification. `zlib.dll`
is implemented in HLE and must be recognized there too; native import graphs
must not require a separate zlib binary just to reach that implementation. If a
real zlib DLL is supplied, its exports and stream state remain native: HLE/native
inflate handles must not be mixed. The dependency graph and direct static imports
both preserve that provider choice.
Native dependencies remain transactional and real missing libraries/exports
still fail loading. No game-name or DLL-version-specific fallback is added.

Some CE DLLs tagged IMAGE_FILE_MACHINE_THUMB expose an ARM DllMain attach stub.
The entry-mode detector recognizes the two-instruction ARM sequence
`str lr,[sp,#-4]!; cmp r1,#1` for Thumb-tagged images, alongside the existing ARM
prologues. Matching only the header or the first halfword is insufficient.
Genuine Thumb entries and other first-word lookalikes retain Thumb mode.

The guest zlib ABI includes named `crc32` (incremental seed, null/empty buffer
semantics) and `inflateReset` (same stream handle, reset decoder/totals/adler).
Streaming decompression uses flate2's pure Rust zlib-rs backend. The former
miniz backend can consume all compressed input while buffering output beyond
the caller's available output space. Older native libpng clients that process
IDAT only while avail_in is nonzero then report "Not enough image data".
A synthetic small-row regression covers that client contract without game data.
Cargo feature unification also selects this backend for other flate2 consumers;
archive imports, backup compression/restoration and existing guest zlib streams
must be included in regression checks. This is a semantic compatibility change,
not a measured performance guarantee.

EverQuest Episode I reproduced the original black framebuffer with its supplied
native MFC runtime while GapiDraw calls returned zero after dependency-load
failure. After these common fixes it reaches the character-slot screen through GAPI;
a scheduled pointer tap opens class selection/name entry. Further gameplay and
device-specific behavior require separate validation.
No game executable, image asset or registration check is modified.

## GDI nearest-color queries

`GetNearestColor` is a read-only query of the requested DC. PALETTEINDEX resolves
through its selected logical palette (the stock palette by default); RGB and
PALETTERGB resolve directly on the current non-palettized RGB565 device. The
result is an ordinary COLORREF with the renderer's RGB565 bit expansion, not a
palette index or an unconditional zero. Invalid DCs or unresolved palette entries
return CLR_INVALID. No drawing, pixel readback or frame publication is required.
The shared handler applies to desktop and Android and the existing ordinal 952
mapping. This does not add a hardware palette or change the drawing APIs' color
resolution. Other surface formats/palette realization remain separate contracts.

A synthetic regression covers stock/custom per-DC palettes, direct/PALETTERGB
colors, quantization, invalid handles/indices and unchanged publication count.
A missing query can affect a caller's color conversion or transparency setup,
but implementing it alone is not evidence that a reported gameplay artifact is
fixed; compare the same scene before and after.

## GAPI drawing threads and retained-pointer publication

GXBeginDraw records the acquiring thread. A message-pump poll or nonzero Sleep may
publish a retained display pointer only on the thread that acquired that active
draw, including a worker retaining its pointer. A main-thread yield does not complete another thread's drawing;
GXEndDraw still publishes the finished image. CPU-slice polling remains suppressed
while a draw is active, and the existing main-owned retained-pointer path remains
available, including a transition from explicit EndDraw frames to retained access.
This is thread ownership, not a game-specific wait or a change to GAPI pitches,
colors, frame pacing, input polling or GPU authority.

EverQuest's native graphics path reproduces intermediate menu frames with
silhouettes missing when main-thread message polling publishes a worker's draw.
The issue remains with GetNearestColor implemented. A synthetic worker-draw/main-
poll regression and the existing retained-GAPI tests cover the ownership guard.
Device-specific presentation and later gameplay still need separate validation.
