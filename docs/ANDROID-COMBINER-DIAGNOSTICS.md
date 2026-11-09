# Cross-host NV combiner diagnostics

Baseline: d4d1a4e58e63af3010e9fd725af80a2339e7d389.
This cumulative patch retains the opt-in observations and corrects the GLES
native-resolution guest RGB texture-copy path described below. AR10/AR11
arithmetic, extension policy and frontend presentation are unchanged. No game
name, asset path or executable signature is added.

## Original diagnostic capture (before correction)

1. Install a build with this patch. Enable **Graphics diagnostics**, with log
   verbosity **1**, before starting the game.
2. For the first capture select **software rendering**, since the reported
   defect also occurs there. Restart the game so initialization is captured.
3. Reach the scene displaying the black water, remain there briefly, then stop.
4. Export the complete application log. Disable diagnostics after the test.

For ADB (Windows CMD or PowerShell), clear the old buffer before launching:

```text
adb logcat -c
```

After reproducing, collect the PocketHLE tag without losing its startup records:

```text
adb logcat -d -v threadtime -s PocketHLE:I > siege-android.txt
```

The new records contain `NV_DIAG`. Keep the complete log, including ordinary
warnings/errors. Timing differences and the diagnostic capture's FPS are not
benchmarks. One software run is the initial request; no GPU/backend matrix is
needed to start comparing it with the PC reference.

## What the trace distinguishes

- Extension advertisement, detected client, requested/active GPU and capabilities.
- Bytecode identity and decode/compile/unit-limit rejection versus acceptance.
- A semantic fingerprint excluding process-local program IDs. It hashes a
  deterministic description of the compiled operations, not generated GLSL, so
  inspecting a program does not depend on GPU shader-generation prerequisites.
- Actual uses of a program: constants, geometry, target dimensions, framebuffer
  fetch, coordinate/texture units, modelview/projection/texture matrices.
- Referenced texture dimensions, completeness, filters, format, revision and
  available CPU/compressed bytes. Prefix hashes are explicitly limited to 64 KiB.
- For software framebuffer-fetch passes only, a prefix hash and one stored RGBA
  pixel before the draw. This reads existing CPU storage, without rendering or
  changing pixel ownership. GPU snapshots are not materialized for diagnosis.

A different semantic hash is evidence of a different program, not automatically
an emulator fault. RGBA bytes can legitimately be absent in the GPU backend
because compressed decoding is deferred. Prefix hashes neither cover whole
images nor prove visual equality. Program names, memory addresses and animation
frame contents need not match across launches.

## Cost and bounds

All trace entry points are guarded by the existing graphics-diagnostic flag.
Disabled: no diagnostic hashing, formatting, logging, map insertion or pixel
inspection. The draw path adds a thread-local flag check; no claim of literally
zero instruction overhead is made. No additional GL call, GPU timer, fence wait,
readback or texture decode is introduced by this patch.

Enabled: sample uses 1, 2, 16 and 256 of each compiled runtime program, up to 128
programs per context. The empty trace map allocates only when sampled. Load logs
are capped at 512 events and extension queries at 32 per worker thread. Context
teardown resets draw sampling. Existing graphics timing diagnostics retain their
own behavior when the option is enabled.

## Reference investigation (2026-11-09)

The supplied CAB contains 36 AR10 and 39 AR11 program markers. An instrumented
x86-64 software run advertises the extension, accepts 57 programs, rejects 17 at
compilation and one at the texture-unit limit. These are variant outcomes, not
proof that the rejected variants cause the Android problem.

The CAB reached the title/demo 3D scene without modification. Native logical
presentation is 240x320; intermediate framebuffer-fetch draws include 256x256
surfaces. Observed semantic keys include:

| Program | Observed target | Framebuffer fetch |
| --- | --- | --- |
| baf5a33c8189d79d | 256x256 | yes |
| 830aceb213938d77 | 256x256 | yes |
| faea2c04d77d54d3 | 240x320 | yes, with texture unit 0 |

This run establishes execution of the multipass path; it does not reproduce the
reported Android defect or prove Android restoration/visual correctness. The
PC capture covers the title/demo, not a completed player-controlled level.
The current environment has no connected Android device or full APK toolchain.

Validation: 237 pocket-gles tests; 721 pocket-winceapi tests (13 hardware tests
ignored); Android aarch64 pocket-gles checking; CLI/Unicorn build; rustfmt and
whitespace checks. New tests cover bounded sampling, fixed-width byte hashing,
fingerprints independent of runtime IDs, and nonmutating software-framebuffer
observation. Native GL/GLES tests are not claimed here. Clippy --lib completes
with existing warnings; strict --all-targets -D warnings fails on existing
warnings, so it is not reported as passing.


## GLES guest texture-copy correction

The Android software capture confirms `requested_gpu=false`, 57 accepted
programs, complete 256x256 and 128x128 textures 25/26, and execution of the
intermediate framebuffer-fetch passes. The earlier GPU capture has the same
semantic programs but both textures remain 0x0, revision zero. The same PC
software run at 480x640 still initializes the intermediate textures correctly;
resolution alone does not explain that difference.

A native GLES regression reproduces `GL_INVALID_OPERATION` (0x0502) when
`copy_guest_rgb_texture` uses CopyTexImage2D with GL_RGBA directly from an
RGB565 framebuffer. RGB565 has no alpha channel. Desktop OpenGL accepts this
case; the GLES copy restrictions reject it. A failed offscreen surface retains
its failed state separately from the visible surface, explaining why a broken
intermediate image need not stop the rest of the game from rendering.

The correction reuses `resolve_logical_image(..., read_pixels=false)` for GLES
at native resolution as well as the already-supported supersampled path. Its
GPU shader creates an RGBA8 image with opaque alpha before the native RGBA copy.
The complete immutable copy/subimage/import chain therefore keeps its existing
RGBA8 storage contract. No CPU download or alternate combiner arithmetic is
introduced. The cached resolve target/program are reused across copies.

Scope/cost: GLES shared guest-copy operations at 1x acquire one extra GPU pass;
this is not a global per-frame conversion. Desktop OpenGL's native-resolution
copy stays unchanged, and supersampled copies already used this resolve. The
existing CPU fallback remains if the optional GPU resolve is unavailable.
Texture precision, logical crop dimensions, orientation, opaque RGB alpha,
immutable ownership and fences follow the established supersampled path.

Validation: the native copy test fails before the change with 0x0502 and passes
afterwards. Three native tests cover GL/GLES, native/supersampled crops,
subimages, retained original pixels, producer teardown and cross-context import.
Pixel reads in tests are validation oracles only. Ordinary GLES tests, Android
Rust checking and formatting also pass. The new device capture validates the
software reference, not the corrected GPU build: Android visual confirmation is
still required. Keep diagnostics disabled after that check. This does not claim
a complete APK build or a measured performance change.


## Verify the corrected Android GPU build

Enable GPU rendering, restart the game, and replay the same scene/settings.
The expected result is populated intermediate textures and visible water. Keep
the diagnostic option disabled for ordinary performance checks. If the defect
persists, capture continuously from before launch with diagnostics enabled and
attach the complete log; do not infer the backend from a saved preference alone.
This ZIP replaces the earlier diagnostic ZIP and retains its opt-in traces.


## GLES3 combiner precision candidate

The supplied camera-motion video follows the native-resolution RGB-copy fix.
The report describes water sliding/jittering while the camera moves, but stable
appearance at rest. This is a separate symptom; its precise cause has not been
reproduced on the connected-device side (no device is available here).

The common ES shader compiler used mediump texture-coordinate varyings and
mediump fragment floats, unlike desktop OpenGL's default precision. Projective
coordinates, framebuffer coordinates and cascaded combiner calculations can
lose subpixel precision on mobile hardware that implements mediump at reduced
precision. This is a plausible mechanism, not proof that it caused this video.

This candidate requests highp texture-coordinate varyings (matched in vertex
and fragment stages) and fragment floating-point calculations for NV combiner
programs on ES3 hosts, where fragment highp is guaranteed. It covers the ordinary
GLSL ES 1.00 and explicit-LOD GLSL ES 3.00 generated variants. Color/fog varyings
keep their established precision. There is no scene/game test or animation
adjustment. Desktop GL, ES2 compatibility, non-NV shaders and the software
combiner remain on their established precision policy.

The shader is still cached by program/range as before. Version/precision
selection runs at shader creation, not every draw. Increased GPU precision may
increase GPU arithmetic/register cost on some devices; no universal performance
guarantee is claimed. Diagnostic-disabled behavior is unchanged.

Validation: 237 ordinary GLES tests and six explicitly executed native tests
cover combiner coordinate routing, ordered framebuffer fetch, both LOD shader
variants, ordinary color precision, retained guest copies and supersampled
subimages, across desktop GL and GLES. Android ARM64 checking, formatting and
whitespace checks pass. No physical-phone jitter improvement or APK build is
claimed. Compare the same camera sequence/settings with the previous RGB-copy
build, preferably with diagnostics disabled. This cumulative ZIP retains the
RGB-copy fix and opt-in instrumentation and replaces the earlier ZIP.
