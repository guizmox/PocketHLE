PocketHLE - Anisotropic filtering option (cumulative)

English Emulator options on Windows/Android: Anisotropic filtering (GPU only).
Choices: Off (default), 2x, 4x, 8x. Per-game settings additionally offer Emulator
default. Save choices across restart; global GPU false and per-game software
selection disable the control while preserving the saved choice. Changing GPU
selection updates control availability immediately. Applies on the next launch.

The running renderer queries EXT/ARB anisotropy once per native context, clamps
the requested level to the driver maximum and applies the texture parameter only
when its per-texture cached value changes. Unsupported adapters retain ordinary
filtering without sending unsupported enums. Software stays unchanged; choosing
anisotropy alone never initializes a GPU context. Applies to current/new/parked
native surfaces through the context setting and to both Android launch paths.

This improves detail on oblique surfaces such as roads; it cannot reconstruct
missing detail from the original 320x240 image. Cost depends on GPU, texture
traffic and selected level. Try 4x first, compare 8x if useful. No RBR-specific
bias or global mipmap removal. Previous guest mip-limit and depth fixes included.

Lifetime: no extra native image, renderbuffer or worker. Cache entries are removed
alongside native texture deletion, including parked surface renderers. Entire
cache/config state is released with the context. Turning Off restores native
anisotropy 1 on the next use of each texture.

Validation: 153 GLES tests including desktop GL/GLES2 anisotropy driver readback,
clamping, Off reset, deletion and unsupported-adapter simulation; 284 WinCE tests;
Rust configuration persistence across all global/per-game modes and library
reopen; Kotlin JSON defaults/roundtrip/inheritance/unknown-key preservation;
Android ARM64/ARMv7 Rust GLES checks and no-default-features core check, warnings
denied. Android preference XML parsed. Full Windows release/Android APK builds
and on-device UI/visual/performance testing were not run here.
