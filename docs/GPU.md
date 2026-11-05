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
all Android control modes and desktop/Deck layouts. See
[frontend geometry](FRONTENDS.md#image-geometry-and-input).

The filter carousel includes **No filter**, reconstruction, SMAA variants, xBRZ,
bicubic, Lanczos, bilinear and nearest. A selected filter is not limited to QVGA
or to enlargement: native and internally scaled images can still benefit.
Filters do not regenerate missing texture detail. xBRZ includes CPU preprocessing.
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
