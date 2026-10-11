PocketHLE - native GPU alpha/depth parity

Cumulative patch; includes points/lines, matrices/vertices, shared lighting,
Android FPS, brightness tuning and earlier native GPU fixes.

The supplied Carmageddon executable uses glAlphaFuncx(GL_EQUAL, 1.0), confirmed
at call site 0x496bc. Its executable SHA-256:
a0a4cdc5172d24e63472353e324ddbac7b8edb2ed121dc17e2cf5b4843fdadc1

Fix 1: native alpha comparisons now use the same rounded 8-bit alpha/reference
as software. Floating interpolation near 1.0 must not incorrectly reject an
otherwise opaque pixel. The test precision changes; blend/output alpha does not.

Fix 2: native explicit depth clears match the existing software HLE contract
when depth writes were disabled by a previous pass. Writes are enabled just for
clear, then the guest mask is restored. Draw depth masks still work. Software
retains its existing behavior; this is an HLE compatibility choice rather than
strict OpenGL masked-clear semantics. No per-game identification is used.

Both new regressions fail before the fix (software visible, native black) and
pass afterward with desktop OpenGL and GLES2. Software tests and existing native
lighting/matrix/primitive/depth/blend/texture tests pass. Rust Android ARM64/ARMv7
checks pass; full platform builds/device runs require your toolchains.

The executable alone establishes the API evidence and enables focused tests;
the full game's asset-dependent rendering has not been run here. Test Carmageddon
in GPU mode after restarting the session, including menus and gameplay. Also
check a previously working GPU game. If it remains black, provide the emulator
log from this run and the complete game archive for a full execution trace.

No temporary instrumentation or new resource lifetimes are included.
