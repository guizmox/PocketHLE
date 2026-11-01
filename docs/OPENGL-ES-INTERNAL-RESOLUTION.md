# OpenGLES internal resolution

The global **Emulator Settings > Graphics > OpenGLES internal resolution (GPU
only)** setting is available on desktop (Windows/Linux/Steam Deck) and Android.
It applies on the next launch and defaults to **Native (1×)**. Select **2×** with
GPU rendering enabled to render twice the logical width and height:

- 320×240 guest surface → 640×480 rendered image;
- 240×320 portrait surface → 480×640 rendered image;
- a guest that already uses VGA is likewise doubled.

This is actual geometry rasterization into a larger color/depth framebuffer,
not a post-process resize. Textures/sprites keep their original source detail.
It does not change the guest's reported LCD resolution, GAPI memory, viewport
queries, touch coordinates, control layout or display zoom. The software GLES
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
continue to use the logical image dimensions. Offscreen reads and copy-to-texture
operations preserve guest-sized buffers; this option cannot increase an image
resource's dimensions or remove every low-resolution post-processing effect.

## Applying

Apply the modified files, rebuild PocketHLE, then choose the setting and relaunch
a GLES game. On Android rebuild/install the native libraries and APK together.
There is no keystore/signature or build workflow change in this patch.

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
