# EGL alpha surfaces, shared NV programs and PCM mixer

Baseline: `91b8afbe90352a7763b2f631b24bcb1c2bbde5c1`.

## Rendering

DoomGLES requests a second pbuffer with alpha and uses NV programs uploaded
in its main context from a sharing context. Previously the alpha configuration
was unavailable and only textures/buffers participated in EGL sharing.
Its mirror programs also contain AR11 equality/inequality fragment rejection
operations that were rejected by the compiler.

- Add an RGBA8 EGL configuration (ID 2), keeping RGB565 first/default (ID 1).
  Selection, attribute queries, context/surface configuration IDs, GL component
  queries and readback agree with the actual attachments. RGBA8 supports windows
  and pbuffers; native RGB565 GDI pixmaps retain their existing configuration.
  Making an incompatible context/surface pair current returns EGL_BAD_MATCH.
- Transfer the NV program namespace with its EGL sharing group, using the same
  context-switch ownership model as textures/VBOs. Binding, constants, enables
  and program zero remain local. A body bound in another context survives name
  deletion until that context rebinds. No per-draw shared lock is introduced.
- Compile the supported AR11 equality/inequality rejection operations to shader
  discard on GPU and explicit fragment rejection in software. Rejection does
  not write color or depth. Other unsupported instructions still fail instead
  of being guessed or replaced with a generic shader.

These changes have no executable-name, asset-name or game-hash conditions.
The existing GoForce option, RGB565 presentation and diagnostics gate remain.
Programs previously accepted retain their arithmetic; this does not claim
complete coverage of every AR10/AR11 instruction or bit-exact GoForce hardware.

## Audio mixer

Expose one virtual PCM mixer, with a speaker destination and a connected wave
output source with stereo volume. The mixer owns separate handles and uses
the existing wave-device volume, including current streams and future opens.
It does not change the host system's volume. Implement device enumeration,
GetID, Open/Close, capabilities, line/control queries and volume get/set,
including ANSI/wide aliases. Live mixer handles also work without the optional
HMIXER flag; ONEBYID ignores dwLineID, as documented by WinMM. MMRESULT errors
preserve GetLastError.
Input/MIDI mixers, callbacks and physical mixer controls are not advertised.

## Validation and limits

- Rust formatting; 240 GLES tests, 725 WinCE API tests and 343 kernel tests.
- Native desktop GL and GLES tests for NV routing, ordered framebuffer reads
  and discard/color/depth preservation; native EGL RGBA alpha roundtrip,
  configuration mismatch, pixmap/offscreen lifecycle and parent restoration.
- Linux CLI/Unicorn DoomGLES gameplay in software and native Mesa GPU mode:
  sky and green basin replace the original white regions. A test-only WAD
  copy moves the player start into the courtyard to expose that scene quickly;
  the original game assets are unchanged and no game data is delivered.
- Shared GLES crate cross-checks for aarch64 Android. Full Android build is
  unavailable here: the rmp3 dependency requires the Android NDK C compiler.
  Windows/Android device tests and audio listening remain user-side checks.
- Advisory Clippy runs; the existing tree has warnings and is not clean under
  an additional `-D warnings` policy. No unrelated warning cleanup is included.

Regression focus: RGB565 applications still selecting configuration 1; games
with EGL sharing, context destruction or name deletion; Siege's existing NV
water passes; wave volume and cleanup. Apps explicitly requesting alpha can
now select RGBA8 and take newly available offscreen rendering paths. RGBA texture
copies keep the existing CPU materialization path when no native fast path is
available; this patch does not promise every new alpha operation is GPU-only.
