GPU vertex transformation update

GPU rendering (experimental) now runs modelview/projection transforms,
texture coordinate matrices and eye-space fog depth in the vertex shader.
Guest array decoding, pointer/VBO validation and triangle assembly remain on
CPU. The software renderer retains its original transformation path.

This cumulative patch includes the previous GPU fixes and the minimum
brightness tuning. Use the existing GPU setting; restart the game to apply.
No new dependency, setting or per-frame instrumentation. Rebuild desktop or
both Android native libraries plus APK before testing.

Validation: 117 software GLES tests; four native driver tests on Mesa including
OpenGL and GLES2 CPU/GPU transform comparisons and actual guest arrays;
backend cargo check with warnings denied on ARM64 and ARMv7 Android.
Full native Windows and Android APK builds and physical game performance
remain device checks. No quantified speedup is claimed.
