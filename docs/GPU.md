# Graphics, presentation and performance

This is the current graphics guide. Older rasterizer milestones, regression
investigations and historical test counts are retained in
[implementation notes](history/IMPLEMENTATION-NOTES.md#history-gpu), not as current
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

## Renderer identity

Guest `glGetString(GL_RENDERER)` returns the active host OpenGL renderer name
when a native renderer exists, or `PocketHLE Software Rasterizer` in PocketHLE's
software mode. The host name is captured once during renderer initialization
and follows the current EGL surface's renderer; querying it does not bind a
context or synchronize GPU work. Guest strings retain the existing process-owned
allocation cache, so earlier returned pointers survive a backend change.

This identifies the host OpenGL implementation, including ANGLE or a host
software renderer such as llvmpipe when present; it is not a host GPU inventory.
Guest `GL_VENDOR`, `GL_VERSION` and extensions still describe PocketHLE's emulated
API. The shared implementation applies to desktop and Android. Games which
branch on renderer-name substrings can consequently select different paths.

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

## PocketPC graphics compatibility options

PocketPC games select **GPU compatibility profile** in their own **Graphics**
settings, immediately above **GPU rendering**, on desktop and Android:

| Profile | Compatibility behavior |
| --- | --- |
| Generic OpenGL ES (1.1, Common) | Standard behavior; no Intel bias or NVIDIA combiners. This is the default. |
| NVIDIA GoForce 5500 (OpenGL ES 1.1, Common) | Allows detected clients to use AR10/AR11 combiners; standard depth bias. |
| Intel 2700G (OpenGL ES 1.0, Common-Lite) | Enables the Intel depth-bias model and opaque coplanar tolerance; NVIDIA combiners disabled. |

The persisted per-game `gpu_compatibility_profile` enum replaces both global
compatibility booleans. Old global values are ignored, not inherited; existing
entries without the new field use Generic. Selection applies on the next launch
in software and GPU modes and is ignored for Gizmondo. Gizmondo's existing driver
and combiner policy and standalone CLI defaults are unchanged. The version/profile
labels identify compatibility targets; they do not remove supported API entry
points or change the host renderer identity.

An imported PocketPC game receives one combined launch-settings prompt for
screen/orientation, optional FPS advice and, when the existing executable analysis
identifies OpenGL ES, the GPU compatibility profile. The FPS detector and message
are shared across desktop and Android. Import advice does not select a GPU profile
or use game names. NVIDIA selection gates extension advertisement and entry points;
it permits existing program detection rather than forcing combiner rendering.

### Intel 2700G depth-bias model

**Intel 2700G depth-bias compatibility** retains the reference driver's integer
encoding: ignore the slope factor, arithmetic-shift the signed 16.16 units,
and clamp the signed magnitude. The constant offset is modeled in the 16.16
primitive-input domain, independently of the advertised 24-bit depth storage.
This is an inferred hardware model, based on the driver's fixed-point vertex
and command path, rather than a claim of bit-exact 2700G rasterization. It
restores the continuous projected headlight pass in GeoRally EX without a
title check, binary patch or disabled depth testing.

Software applies that normalized constant directly. Native GL/GLES measures
the host polygon-offset step once per renderer/depth format on the first
nonzero Intel-biased draw or eligible tolerance replay, using a temporary 2x2
target. The measured step is cached; explicit guest polygon offsets use native
polygon offset without readbacks, shader depth writes or additional rendering
passes. The setup preserves the live
surface, clears its temporary resources and invalidates only the affected
host state caches. It does not run for an inactive profile or a draw needing
neither nonzero offset nor tolerance.

The stronger constant offset can change shadows or decals in other applications
using this profile. Foreground occlusion remains enabled. The ordinary GLES
offset policy, guest depth-bit queries, render scaling, presentation, screenshots
and recordings retain their existing paths. Shared code serves desktop and
Android; native GL/GLES tests cover both depth formats and 1x/2x rendering.

### Intel 2700G opaque coplanar tolerance

The same existing Intel profile also permits a bounded color-only replay for
nearly coplanar opaque triangles. This is an **empirical compatibility
approximation**, not a measured MBX register value or exact GLES depth semantics.
THPS2's fixed-point projected decals cross their backing ramp's submitted depth
as the camera moves, with reversed `GEQUAL` testing and no polygon offset. A
normal draw preserves the nearest real depth; a second draw may contribute color
within `16 / 65536` of that depth, towards the camera. It never writes depth,
so the tolerance does not accumulate across draws. The sign is reversed for
`LEQUAL` versus `GEQUAL`. There is no game, executable or texture identification.

Eligibility requires enabled depth testing and writes, the default depth range,
`LEQUAL` or `GEQUAL`, no explicit polygon offset, no logic operation and at least
one writable color channel. Blending must be disabled or standard
`SRC_ALPHA, ONE_MINUS_SRC_ALPHA`. The replay accepts only fully opaque fragments
and only when the original alpha test also accepts them; translucent fragments
therefore retain their original single-pass behavior. Points, lines, strict
`LESS`/`GREATER` tests, additive/multiplicative effects and NV combiners keep their
existing paths. The explicit GeoRally EX depth-bias model above is unchanged.

Software reuses transformed vertices and the existing constant-offset rasterizer.
Native GL/GLES reuses buffers, textures and the current general/affine shader,
temporarily setting alpha rejection, depth-write mask and calibrated polygon
offset, then restoring host state. Alpha-test-free fast shader variants are not
used for these draws. There is no per-draw readback, new texture upload, shader
depth write or format/presentation change. Both backends, render scaling and the
shared desktop/Android implementation use the same rule.

Eligible triangles are rasterized twice: this has a real GPU cost, or CPU cost
in software mode. Nearby hidden opaque surfaces within the tolerance can become
visible or change overlap order in other Intel-profile games. Generic and NVIDIA
profiles do not enable this Intel approximation. See the
[THPS2 validation record](../proof/thps2-hangar/README.md) for before/after images,
measured costs and the limits of local testing.

## GoForce 4500 visibility approximation

The Gizmondo GoForce 4500 profile applies an **empirical visibility approximation**
to opaque fixed-function triangles. It is not a recovered SC-10 hardware rule or
exact OpenGL ES depth behavior. There is no game, executable or texture-ID test
and no new setting. PocketPC profiles, including GoForce 5500 and Intel 2700G,
keep their separate behavior.

The ordinary draw establishes unbiased depth. A color-only replay uses
`z' = z + max((z - w) / 12, -2 * w * cap)` for vertices in front of the eye.
The cap is `1 / 4096` through window depth `126 / 128`, increases linearly
to `1 / 1024` at `127 / 128`, and stays at that value beyond it.
The resulting margin still diminishes towards the far plane and never
accumulates in the depth buffer.
The transform precedes clipping and interpolation. It is not a per-fragment
comparison against a depth texture. Near-plane clipping stays enabled.

A uniform bound of `1 / 4096` left visible water/land intersections on a low
Battlestations flight. A uniform `1 / 1024` could expose hidden geometry from
a different batch through a nearby car body. The depth-dependent cap retains
the distant margin and reduces these nearby overlaps. Coverage protection also
prevents the replay from replacing pixels already made normally visible by the
same consecutive drawing batch. A batch is defined by pipeline,
modelview/projection, texture bindings, image revisions/sampler state and
texture stages. These are rendering states,
not an inferred game or object identity. Material/transform changes start a new
batch, so the absolute bound remains necessary for cross-batch hidden surfaces.

Native GL/GLES uses internal stencil coverage in a packed D24S8 attachment.
The upgrade occurs only at a full writable depth clear, preserving live depth
on partial or masked clears. Hosts retaining D16 or rejecting packed storage
keep the bounded replay without coverage protection and log this limitation.
The internal stencil is not advertised as a guest stencil attachment. A batch
uses an eight-bit reference; storage is cleared only on reference wrap or
allocation/resize. Stencil testing/writes are restored after the draw. Software
uses one byte per logical pixel, allocated lazily for eligible geometry.
Both paths invalidate batch identity on clears, relevant image/cache changes
and surface/client-state transitions. Ordinary unbiased depth remains intact.

Eligibility excludes blending, logic operations, explicit polygon offsets,
non-default depth ranges, tests other than LESS/LEQUAL, disabled depth writes,
points, lines and NV combiner programs. Alpha rejection and color masks remain
effective. The profile follows created, parked and reset EGL contexts and does
not propagate into another device profile.

The native replay reuses uploaded vertices/indices, textures and shader; software
reuses transformed vertices. There is no extra pass beyond the existing replay,
new shader variant, per-frame readback or instrumentation. Coverage adds stencil
work and per-batch state comparison; software adds coverage accesses and storage.
This is not a zero-cost compatibility feature. Render scaling, presentation,
recordings and screenshots retain their existing paths. Shared code serves
desktop and Android; physical Android/Windows GPU validation remains required.

The supplied driver, `libgles_cm(2).dll`, identifies GoForce 4500 version
01.01.0014. Its ARM depth/matrix routines and reconstructed EGL selection do not
establish the empirical tolerance above. Actual captured water and terrain
intersect; original matrix arithmetic and a D16 host attachment did not remove
the observed patch. This evidence must not be described as exact hardware
emulation. Close hidden surfaces in other titles can still become visible,
and a compatibility replay can shift shore intersections.

See [the combined validation record](../proof/goforce4500-coverage/README.md)
for the low-flight and angled-car comparisons, test scope and limitations.
Earlier experiments remain in [the initial note](../proof/battlestations-midway/README.md)
and [the bounded revision](../proof/goforce4500-bounded-depth/README.md).

## Native fixed-point vertex attributes

Advertising `GL_FIXED` fetch does not guarantee that the host preserves exact
16.16 coordinates. During renderer creation, supported hosts undergo one small
private framebuffer check against the existing float conversion. It covers
positive, negative, fractional and extreme values, plus implicit components of
two-, three- and four-component arrays. Float equality and deliberate inequality
are controls; failure or an inconclusive check disables native fixed fetch for
that renderer. There is no driver-name or application-name selection.

The existing stream builder then converts fixed components to `GL_FLOAT` before
upload, preserving capture, VBO bindings, strides and index semantics. Other
native attribute types remain eligible. Transforms, lighting and rasterization
still run on the host GPU. The check is shared by desktop and Android and does
not change the Intel depth-bias model or Gizmondo driver rules.

This addresses PowerVR SDK Vase/Trilinear backgrounds on hosts where fixed `1.0`
arrives above `1.0` and is clipped. It does not move clip planes, add coordinate
epsilons or modify guest binaries. Validation uses a temporary 8x3 RGBA target,
at most three tiny readbacks, and releases its resources before guest rendering.
The result is stored per renderer; no validation or readback occurs per draw.
Hosts that pass retain native fetch. Failed hosts pay CPU conversion costs for
fixed streams, with four bytes per component before and after conversion.

## PVRTC texture capability

Both renderers expose `GL_IMG_texture_compression_pvrtc` alongside its four
PVRTC1 RGB/RGBA 2bpp/4bpp formats in `GL_COMPRESSED_TEXTURE_FORMATS`. Full
uploads, explicit small mip levels and whole-level compressed replacement use
the shared texture validation/decoding path. Partial PVRTC rectangles remain
invalid, as required by the extension. A host without native PVRTC support uses
the existing decoder and uploads RGBA texels; host extension support does not
control the guest capability.

Clients such as DoomGLES 2700G check the extension string before loading their
compressed assets. Listing the formats without the extension leaves those
textures unloaded. Advertising this implemented capability is shared across
platforms and does not require a device profile or game-specific detection.

## EGL surfaces and shared objects

By default, configuration 1 is RGB565 and configuration 2 is RGBA8888.
Both support windows and pbuffers; only RGB565 supports native GDI pixmaps.
An application may request WINDOW_BIT|PBUFFER_BIT with alpha=8 while creating
only an offscreen image. Removing RGBA8 window capability prevents such clients
from selecting their alpha passes (as observed in DoomGLES).

`eglChooseConfig` filters first, then sorts before truncating the output. EGL 1.1
prefers more bits in explicitly requested nonzero color components, then a
smaller total buffer size and configuration ID (other criteria are equal here).
Omitted, zero and DONT_CARE components do not affect that color preference.
Queries, configuration IDs and readback describe the actual attachments.
Incompatible context/surface pairs return EGL_BAD_MATCH.

Each PocketPC game's **Graphics → OpenGL ES surface format** offers **Default
(RGB565 + RGBA8888)** and **RGBA8888 only** on desktop and Android. The persisted
`pocketpc_egl_rgba8888_only` boolean defaults to false. The opt-in profile exposes
only configuration 2 consistently through enumeration, selection, queries and
creation; configuration 1 then returns EGL_BAD_CONFIG. It keeps RGBA8 windows
and pbuffers available and does not advertise EGL pixmaps. This can help clients
whose offscreen and onscreen format choices otherwise take incompatible client
state-cache paths. It does not repair the client's cache or change AR10/AR11
arithmetic, and is not selected by game name, executable or shader signature.

The choice is fixed before the first eglInitialize and stays fixed through
termination/reinitialization; suspended parent sessions retain their own choice.
A new session resets to the default. Both frontends apply it only to PocketPC,
in software and GPU modes. CLI launches retain the default. There is no per-draw
profile branch or pixel comparison. Games requiring an exact RGB565 EGL format
or an EGL pixmap may fail with this option enabled; alpha-capable surfaces can
also enable different guest rendering passes and affect performance. Keep the
default unless needed. Direct GAPI/GX/GDI RGB565 memory and presentation are not
changed by the EGL catalog choice.

EGL share groups own named textures, buffers and NV program bodies. Bindings,
constants, enables and texture object zero are local to each context. Bound
objects survive name deletion while another context retains them. Namespace
ownership moves only at context switches; draws do not acquire a shared lock.

Surface renderers retain native texture uploads for live nonzero object
identities within the same group. Existing image-revision and mip/filter checks
still update changed images. On a shared-context switch, prune unlinked objects
and invalidate the surface's cached object zero. Crossing unrelated groups or
an implicit/no-context boundary clears those surface caches: equal numeric IDs
and revisions must never alias across namespaces. Cache maintenance runs with
the owning native context current and is distinct from guest glDeleteTextures(0),
which remains ignored. Useful images stay resident longer, increasing possible
VRAM use; orphan/group/renderer cleanup remains required. Immutable snapshots,
reader fences and GPU/CPU pixel authority are separate contracts and unchanged.

## NV combiner execution

The CPU decodes supported AR10/AR11 programs at load; native GPU execution uses
cached host shaders, while software rendering evaluates the supported arithmetic
on the CPU. New programs must fail explicitly when unsupported rather than
substitute a guessed shader or select a game-specific path.

Fetches can select x/y independently: selector banks 0..7 and 8..15 identify x
and y. Storage tracks six coordinate sets; capacity checks reject unavailable
sets. AR10 retains its own four-coordinate/two-temporary encoding limits;
AR11 supports its additional registers and signed quarter-unit immediate
literals. Supported equality/inequality rejection discards a fragment without
writing color or depth. This is not complete or bit-exact GoForce conformance.

Five/six-coordinate GPU programs use separate shader variants and attributes
8/9, preserving normal/fog/provoking/point locations, ordinary four-coordinate
packing and compact deduplication keys. Wider programs need at least ten vertex
attributes, ten varying vectors and seven fragment texture units on the host;
program loading checks that capacity. Software has six-coordinate storage too.
Expanded CPU vertex records carry two additional coordinate sets; preserve the
compact upload path when those coordinates are unnecessary.

A program's conservative sampled-texture mask controls native sampler
preparation. Coordinate-only stages still supply transformed/projective vertex
coordinates, but their texture objects and LOD limits cannot trigger uploads,
bindings or an unnecessary range shader. Referenced high samplers still need
the established white fallback when their image is incomplete and absent from
enabled stages. Fixed-function rendering prepares/rebinds its units normally.
Inactive host vector/matrix uniform locations return before payload/cache work;
active uniforms retain their per-program value cache. These rules preserve
NV/fixed-function transitions rather than changing guest texture state.

Framebuffer reads use a conservative channel-dependency mask computed at load.
When a draw cannot write any channel the program reads, capture the destination
once and batch the draw in native order. RGB565 has no writable alpha channel.
True read/write hazards and logic operations retain the existing per-primitive
snapshot/order path. Depth, blending and color masks remain effective. No extra
per-draw program decode, screenshot comparison or GPU readback is introduced.

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

## Paired color capture (desktop diagnostics)

Set `POCKETHLE_COLOR_CAPTURE` to an output directory before launching PocketHLE.
At a completed onscreen EGL swap, an enabled session checks once per second for
`capture.request` in that directory. Creating the empty file requests one pair;
the emulator consumes the file. Recreate it to capture again. Offscreen swaps
and cooperative pacing retries do not consume requests. When the variable is
unset, there is no file polling, image allocation or additional GPU readback.
The disabled path retains only the capture-state check at the swap boundary.

Windows Command Prompt, from the repository directory:

```bat
set "POCKETHLE_COLOR_CAPTURE=%CD%\color-captures"
target\release\pockethle-gui.exe
```

In a second Command Prompt opened in the same repository directory, once the
camera is stationary at the desired scene:

```bat
type nul > color-captures\capture.request
```

Each request writes three files with one timestamp/sequence stem:

- `.render.png`: logical RGBA render readback before display publication. An
  RGB565 EGL attachment has already quantized its rendering; this capture does
  not reconstruct discarded precision. Use the RGBA8888-only profile to test
  an RGBA attachment and check `render_format` in the JSON.
- `.presentation.png`: CPU reference for that same publication, including its
  display color conversion and backlight, before frontend filters, rotation,
  UI overlays and host-window composition. It is not a window screenshot.
- `.json`: dimensions, render format, active backend, requested guest dithering
  state and completed-presentation count. Dithering state is not proof that the
  backend implements it.

Start with internal resolution 1x, no frontend filter and neutral backlight to
compare corresponding pixels directly. Higher-density presentation can have
larger dimensions than the logical render capture. Both software and GPU paths
are supported. A requested capture synchronously reads/encodes images and can
cause a short pause; do not use its frame timing as a performance measurement.
Errors are logged without turning diagnostics into a guest EGL error.
Disable for subsequent launches with `set "POCKETHLE_COLOR_CAPTURE="`.

The paired-capture regressions verify a real software EGL swap and a native-GL
swap using shared GPU publication, consumption of one request, PNG contents,
RGBA shade preservation, separate guest RGB565 bytes and publication identity. Native validation uses
Mesa llvmpipe; Windows device testing remains required. The regular WinCE API
suite passes with the optional native tests excluded.

## Preserve the selected EGL color precision

The per-game RGBA8888-only option controls the advertised EGL catalog. It does
not turn RGB565 rendering into 32-bit rendering after the fact. Presentation
now follows the actual EGL attachment: an RGBA8888 attachment retains its RGB
bytes until host filtering/backlight, while a real RGB565 attachment keeps the
existing 16-bit display expansion. Host display alpha remains opaque.
This applies equally to default-selected RGBA configurations and the optional
profile; there is no game-name, texture or combiner-program test.

| Path | Presentation contract |
| --- | --- |
| Software EGL | Retain same-size RGBA presentation beside logical RGB565 memory. |
| GPU with CPU publication | Reuse native readback at 1x/scaled sizes; RGBA surfaces avoid display quantization. |
| Shared GPU, desktop | Immutable snapshot carries its source format; the display shader preserves RGBA or expands RGB565, then applies backlight. |
| Shared GPU, Android | Same shader and format policy through the synchronized JNI callback; rebuild native libraries and APK together. |
| Deferred CPU read/capture | Optional RGBA presentation is valid even at logical size; reads retain GPU authority and publication identity. |
| Guest GAPI memory and IPS | Logical RGB565 conversion and the existing logical-pixel IPS definition remain. |
| Screenshots, recording, filters | Frontend presentation snapshots retain RGBA; CLI display PPM captures now use presentation pixels too. Logical snapshot_ppm remains a separate memory diagnostic. |
| Real CPU drawing | CPU/GDI/GAPI writes invalidate older alternate/GPU images and win; the next EGL swap can publish RGBA again. |

SIP/shell-menu and system-video composition keep their established logical
CPU composition policy; they can select a 16-bit composite while active. This
change does not introduce a high-color GDI renderer or a mixed-precision overlay
engine. An old RGBA image must never cover a newer CPU drawing operation.

The shader change adds no GPU readback to shared display. CPU publication now
retains/transfers four-byte presentation pixels for RGBA surfaces, including at
1x; this costs buffer space/bandwidth compared with packed two-byte display
pixels. A 1x shared RGBA materialization downloads RGBA once and packs the guest
RGB565 copy on the CPU. Scaled paths retain their established logical reduction
and separate native presentation preparation.

The reported THPS2 pair established RGBA rendering followed by an unintended
RGB565 display reduction: a sampled floor region went from 209 colors to 16.
The fix preserves rendered bytes; it does not reconstruct colors already lost
in RGB565 textures or attachments and does not implement missing dithering.


### Snapshot-reference test correction

The previously reported RGB565 snapshot/reduction discrepancy was traced to a
malformed test fixture. It set the renderer's RGB565 policy, then replaced the
actual attachment with GL_RGBA storage before taking an RGB565 snapshot. The
reference and snapshot consequently started from different color precisions.
The fixture now uploads in the actual attachment format, checks the GL error,
and also covers 320x240 at 2x on desktop GL and GLES. No RGB565 production
reduction arithmetic was changed to make this test pass. The corrected fixture
also passes against the unmodified production code of commit 9bebfb5b.
