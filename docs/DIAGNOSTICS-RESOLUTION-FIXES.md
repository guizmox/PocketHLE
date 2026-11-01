# Diagnostics and internal-resolution follow-up

Source reference: guizmox/PocketHLE b0422921e1d490932329b059b924a76487e76a94,
rechecked on 2026-11-01. This is the cumulative pending diagnostic / GLES
resolution patch, including the unified tool sources and earlier GPU resolution
implementation. It replaces the previous diagnostic/benchmark/resolution ZIPs;
it is not a complete repository. No game executable or user report is included.

## Integer correctness

Unicorn can restart its internal execution after a runtime helper redirects PC.
A concurrent slice timeout can leave the persistent stop request set while the
CPU-local exit request has been reset by that restart. The code-hook dispatcher
then skips the callback, yet the intercepted instruction could still execute.
The generic exit helper now honors either pending request and still defers exits
inside a branch delay slot. Explicit PC redirects retain their existing handling.
This fixes the execution boundary, not the benchmark or division inputs/checksums.
No optimization setting is disabled and no game-specific exception is added.

A new ARM backend regression sums 1024 distinct 64-bit divisions across repeated
short time slices. Both return words are checked against an independent host
sum. The existing interworking, register-preservation and helper-resumption tests
are retained.

## Results and CI

The single ARM HARDWARE DIAG EXE now ends Integer, Floating and OpenGLES on a
compact score page. B/Tab switches to the full text log; A/Enter/Back returns to
the menu; Left/Right changes between completed score pages. Full TXT report keys,
workloads, checksums and median/global calculations are preserved. Missing or
truncated reports show FAIL. Score-page dimensions are checked at native 320x240.

An independent GitHub Actions job builds, verifies and publishes HARDWARE-DIAG,
and runs the ARM diagnostic/report/UI smoke tests. It uses Python 3.12 and the
small Clang/LLD toolchain, with no Rust/FFmpeg build dependency. Other jobs still
build their existing private diagnostic fixtures for integration tests.

## Rendered resolution and display zoom

The source image for display zoom is the actual GPU-rendered image. For example,
320x240 logical + internal 2x = 640x480 rendered; display 2x then targets
1280x960 where the viewport permits it. Portrait is 480x640 then 960x1280.
The display renderer, PocketPC reserved surface, desktop window sizing/filtering
and inverse touch mapping use that same rendered size. Input still returns
logical guest coordinates. Existing native-resolution behavior, including the
legacy native-VGA PocketPC policy and controller fullscreen aspect fit, remains.
GDI, GAPI and software-rendered games do not gain extra internal geometry detail.

## Applying and retesting

Copy the files at their original relative paths. patch-files.txt lists payload
paths with Windows separators. removed-files.txt lists obsolete standalone test
files from the unified-tool migration; it contains no new emulator-source deletion.
Keep any more recent local unrelated work when applying this cumulative patch.

Rebuild PocketHLE normally, including the bundled Unicorn engine. The existing
build script tracks the modified C source; deleting all build caches is not
required. For Android, rebuild native libraries and APK together. Re-import
`tools/hardware-diag/dist/HARDWARE-DIAG.zip` to update the diagnostic executable.
Importing the EXE alone does not install the CPU-engine fix.

## Targeted verification and limits

- Built and archive-verified the integrated ARM diagnostic from the delivery tree.
- ARM CPU workload smoke checks pass all 16 added cases, medians and unchanged
  historical scores; injected wrong arithmetic and backwards clocks are rejected.
- ARM GL smoke checks pass all 10 workloads and median/global aggregation; missing
  APIs, GL errors, black output and stalled clocks are rejected.
- Actual ARM score pages pass score parsing, screen bounds, incomplete/failed
  reports, menu and log navigation checks.
- The CPU regression fails with the uncorrected reference engine and passes with
  the corrected exit behavior. The real compiled diagnostic's unsigned and signed
  division blocks each pass 10,000 repetitions with thousands of timer stops.
  These isolated tests reuse an existing reference-engine library with a private
  test-only exit shim; no shim, offset, control bypass or trace is shipped.
- Compiled the changed C exit helper against controlled stop/restore test doubles,
  including lost CPU exit bits, branch delay slots and explicit PC redirects.
- Six desktop geometry tests and the actual desktop image-filter source check pass,
  including 640x480 -> 1280x960 and portrait dimensions. Rust formatting checked.
- Kotlin tests are supplied for rendered-size scaling, rotation, logical touch
  coordinates and existing native/fullscreen policies. They were not run here.
- Prior native desktop GL/GLES2 internal-resolution, readback and snapshot tests
  remain applicable; the GPU rasterization implementation is unchanged here.

No full desktop/APK build, bundled-engine rebuild, device run or complete GitHub
CI execution was performed here. These targeted checks do not certify a physical
Android device or all games.
