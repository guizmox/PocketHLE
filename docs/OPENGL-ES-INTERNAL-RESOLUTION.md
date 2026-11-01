# OpenGLES internal resolution

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

## Applying

Apply the modified files, rebuild PocketHLE, then choose the setting and relaunch
a GLES game. On Android rebuild/install the native libraries and APK together.
Signing/keystore behavior is unchanged. CI separately publishes HARDWARE DIAG.

Keep Native (1×) for before/after performance comparisons unless deliberately
comparing the two resolutions. Record the chosen setting alongside HARDWARE DIAG
results: the guest sees the same logical viewport, so its benchmark cannot infer
the host-only pixel density.

## Targeted validation

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

## Steam Deck startup

The dedicated Steam Deck build starts fullscreen even when the previous desktop
configuration was windowed. It requests fullscreen in the initial native window
builder and once on the first UI frame, after native window-state restoration.
Subsequent fullscreen controls work normally. An explicit CLI display argument
still takes priority: `0` fullscreen, `1` windowed. This applies to the dedicated
Steam Deck build (`steam-deck` feature), not automatic detection in a regular
Windows/Linux desktop binary.

## Recognizing a fixed-size software framebuffer

GAPI imports alone do not establish whether a game's renderer is resolution
independent (nor do they rule out dynamically loaded GL). Source review or a
trace must follow the width, height and signed X/Y byte pitches returned by
GXGetDisplayProperties into buffer allocations, row addressing, drawing loops,
clipping, depth buffers and the 3D projection. A hardcoded 320×240 buffer or a
640-byte row stride for 320 RGB565 pixels is evidence of an assumption, not
proof from the mere presence of those constants. Engines may query display
properties but still render into a fixed-size private buffer and then copy it.
Changing the reported framebuffer size alone cannot safely add rendering detail.

## Follow-up validation

Policy selection and actual native framebuffer resizing were checked for QVGA,
VGA, both portrait orientations, policy changes, and parked EGL surfaces.
Existing native internal-resolution geometry/readback tests were rerun. Startup
preference/CLI precedence tests passed. Kotlin configuration roundtrip tests are
included for Android CI; no full Android/desktop build or physical Steam Deck
launch was performed here. No game-specific framebuffer patch is included.
