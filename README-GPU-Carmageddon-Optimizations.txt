PocketHLE - cumulative GPU preparation / presentation optimizations

Apply the entire archive at the repository root. It includes the prior Colors
optimizations, Carmageddon fixes, brightness tuning, Android FPS, lighting,
matrices, vertex, point and line support. No release profiling is added.

Changes
1. DrawArrays iterates a range directly: no temporary index vector or guest-index
   vertex hash cache. Native smooth triangles, strips, fans, points and independent
   lines use their native primitive topology and skip expansion/deduplication.
   Flat shading and line strips/loops retain the compatibility expansion path.
2. GPU records contain position/colour and only the active texture coordinates.
   Normals/fog and flat lighting positions are included only when needed. Smooth
   lighting aliases the existing position attribute. Attribute enables/pointers
   are restored every draw. Disabled texture stages skip fragment sampling.
3. Display readback creates canonical top-down RGBA and display RGB565 together,
   avoiding a later full-frame RGBA conversion traversal. The GPU readback itself
   remains synchronous, preserving GAPI, DirectShow, screenshots, glReadPixels and
   copy-to-texture contracts. This is not a zero-copy frontend integration.

Controlled transfer checks
A four-vertex smooth textured strip uses 160 bytes instead of the preceding
396-byte vertex/index payload. Untextured/unlit records use 32 bytes instead of
96. The Colors eight-vertex / 36-index reproduction now uses 328 bytes (untextured,
unlit), versus 840 in the previous patch and 3456 before native indexing. Actual
layouts depend on enabled textures and lighting. Packing/guest decoding remain
CPU work. No whole-game FPS increase is guaranteed by these payload reductions.
Compressed/paletted texture decoding continues at upload time on the CPU.

Validation
GLES unit tests; real desktop GL/GLES2 parity for direct strips/fans, flat fallback,
layout transitions, lighting/matrices/fog, alpha/depth compatibility, textures and
points/lines; fused readback orientation, short output and clean-cache reuse.
Guest presentation checks mapped GAPI pixels, glReadPixels, copy-to-texture after
swap, parent suspension/resume and teardown. Rust checks cover Android ARM64 and
ARMv7 with warnings denied. Full Windows / Android NDK+APK builds and game sessions
must be tested on the target machines.

Game test
Enable GPU rendering and FPS display. Compare the same Carmageddon race/viewpoint
before and after, then check Colors, a lighting-heavy game, captures and a launcher
round trip. Check menus/HUD as well as 3D, since small strips benefit from the new
path. Existing renderer settings and frame pacing remain in effect.
