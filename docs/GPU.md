# GPU rendering

## Configuration

**GPU rendering (experimental)** is enabled by default when the global setting is
missing. Explicit saved Software choices remain honored. Per-game choices are
Emulator default / Software / GPU; restart the game after changing the backend.
Windows uses OpenGL through WGL; Android uses EGL/GLES. Linux uses the shared
frontend; see [desktop parity](DESKTOP-PARITY.md) for platform limits.

Anisotropic filtering offers Off, 2x, 4x and 8x, capped to driver support. It is
available only with GPU rendering. Upscaling and image filters are presentation
options; they do not increase the guest's native rendering resolution.

## What runs where

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

## Testing and troubleshooting

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

## Implementation reference

The sections below preserve the technical notes previously spread across the
architecture document. Dates and test counts describe the validation at the time
of each change, not a new full release certification. Complete Windows/APK builds
and physical-device game testing require the corresponding host toolchains.

### Experimental native GLES rasterization (2026-10-11)
Global config.gpu_renderer defaults true, shown in Emulator Settings / Emulator options. GameSettings.gpu_renderer is nullable: null/missing inherits, false forces software, true forces GPU. Both Kotlin JSON models preserve unknown fields. Configure on the emulation thread after creating the dispatcher; each child inherits the launcher configuration and per-game override.

pocket-gles::gpu owns private offscreen native contexts: Windows WGL hidden 1x1 window + OpenGL GLSL120, Android EGL GLES2 pbuffer + GLSL100, headless Linux EGL/OpenGL validation. No new dependencies. CPU validates guest memory and assembles object-space vertices; the vertex shader applies projection/modelview, texture matrices and eye-space fog depth before native rasterization. Shaders/native rasterization handle 4 texture stages, fog, alpha test, blend, depth, culling, color mask, scissor. Guest points/lines now have software and native rasterization (see below). Fixed lighting is now evaluated per vertex by both backends (see below). Flat shading expands triangles and copies provoking color; Supported compressed textures may upload directly; unsupported paths decode on CPU, and texture revisions invalidate GPU uploads.

Native framebuffer RGBA+depth persists across draws. Readback runs only for EGL swap or glReadPixels, flips bottom-up rows to canonical top-down, then existing RGB565/GAPI/DirectShow/filter/screenshot paths consume the result. No zero-copy frontend integration yet. GPU stage allocations and GL resources release on EGL termination, emulator drop, context reset and child return. Contexts are !Send; every operation makes its own context current. EGL display leases count live private contexts, so child teardown cannot terminate parent; Android keeps one process-wide EGL library/display initialization lease because the Android UI shares DEFAULT_DISPLAY, while all game surfaces/contexts are destroyed.

Initialization failure logs software fallback. Runtime GPU failure reports once, sets GL_OUT_OF_MEMORY and stops issuing draws until restart/termination; do not attempt partial software fallback with stale depth. Toggle software and restart for driver problems. Backend identification is a single normal info log, no per-frame instrumentation.

Validation: software/core/library/API unit tests, real OpenGL and GLES2 Mesa driver tests (depth/blend/alpha/textures/revision update/fog/scissor/masks/readback), ten child renderer lifetimes, full guest GLES->RGB565/GAPI/glReadPixels pipeline plus parent resume/EGL termination/reinitialization. Host driver here is Mesa, so real Windows/Android GPU performance and visual game regressions require device tests.


### GPU portability, settings and presentation follow-up
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


### Native vertex transformations
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


### Shared fixed lighting (software and native GPU)
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
the earlier 50% and 1/12 settings. Upper steps and ceiling remain unchanged.


### Point and line rasterization (software and native GPU)
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


### Native alpha/depth-clear parity (Carmageddon investigation)
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

### Colors: array reads, native indices and private-context state cache
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

### Carmageddon: sequential draws, compact attributes, fused presentation
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

### Four-game native GPU audit: RGB, DXT3, direct indices, mip/LOD and EGL images
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

## GPU upload/state optimizations (2026-10-11)

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

## GPU depth precision (2026-10-11)

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

## Guest maximum mip level (2026-10-11)

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

## Configurable anisotropic filtering (2026-10-11)

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

## Reusable typed vertex streams (2026-10-11)

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

## EGL display cadence (2026-10-11)

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

## Main-thread Sleep duration and Battlestations water cadence (2026-10-11)

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

## Default GLES texture object zero (2026-10-11)

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



## Signed linear fog ranges (Carmageddon regression)

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
