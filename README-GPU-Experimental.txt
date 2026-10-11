PocketHLE - experimental GPU rendering follow-up

This cumulative patch contains the original GPU patch plus Android C char
portability, native build freshness checks, English renderer labels, settings
readback diagnostics, and presentation cadence/context activation fixes for
Windows and Android. No games, APKs, executables or per-frame instrumentation.

Enable GPU rendering (experimental) under Emulator options. Per-game choices:
Emulator default / Software / GPU (experimental). Restart the game to apply.
Rebuild both Android native libraries before packaging the APK. Old native
libraries silently discard the GPU field; the build now refuses stale libraries.
A failed native build blocks APK packaging even if an older .so is present.

The desktop and Android frame publication clocks retain a fixed phase instead
of accumulating send delays. WGL/EGL skip redundant context activation while
checking actual native state to preserve parent/child context switches.
Physical Windows/Android stuttering still requires a device/game comparison.

Checks completed for this follow-up:
- pocket-gles cargo check for aarch64-linux-android and armv7-linux-androideabi
  with warnings denied (the complete JNI library/APK was not built here).
- 117 software GLES tests and 43 library tests, including native JSON write /
  disk reopen for both global modes and all three per-game modes.
- Two actual Mesa OpenGL/GLES2 driver tests, including depth, blending, texture
  revisions, readback orientation and parent/child context transitions.
- Presentation jitter/pause cadence harness; source/binary freshness checks;
  shell syntax validation; Windows platform source type checking.

Test on hardware: select GPU, close and reopen settings, restart the app and
check the selection again. Launch a game, compare scrolling with Software,
then launch/exit a child game if applicable. Keep normal logs if jitter remains.
