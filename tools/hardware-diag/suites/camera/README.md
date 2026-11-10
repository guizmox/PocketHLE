# CAMTEST v3 — Gizmondo CAM1 camera

Run **Camera** from the single [HARDWARE DIAG application](../../README.md).
Enable **Gizmondo options → Camera hardware (CAM1)** and close other camera apps.
Use the current integrated diagnostic and a compatible PocketHLE build; no
separate CAMTEST installation or entry rename is needed.

## Results

Find `CAMTEST.TXT` in the library's Flash Disk directory. The test preserves the
original preview and I420 capture checks, then runs eight separate open, format,
start, two-preview, stop and close cycles. Each cycle logs its number, individual
API stages, raw return value, LastError, byte count and elapsed milliseconds.
The report is saved after each cycle, including a failed open or close.

Two independent checks verify DeviceIoControl rejects an invalid or closed
handle with error 6. Before each probe the test deliberately sets LastError to
50. If `camera.first_closed_handle_error_6` fails with error 50 but capture stages
pass, the failure concerns closed-handle error reporting rather than acquisition.
The cycles continue after a failed closed-handle probe to preserve this evidence.

A successful call may preserve an earlier LastError: a PASS line showing 50
is not itself a failure. The result and explicit assertions decide PASS/FAIL.
The report buffer is bounded at 16 KiB; normal output is under 10 KiB.

- `CAMTEST-preview.bmp`: initial RGB565 preview, 320×240, bottom-up rows.
- `CAMTEST-capture.i420`: initial planar Y/U/V capture, 640×480, 460800 bytes.

The test cannot certify the absence of host memory leaks. Inspect the image and
verify the camera is available to another app after exiting. Android pause/resume
requires a separate continuous-camera gameplay test.

## Build

```text
python tools/hardware-diag/suites/camera/build.py --clang clang --lld ld.lld
```

Freestanding ARM/WinCE PE, no proprietary SDK dependencies. Local v3 validation
executes the generated ARM instructions under Unicorn with controlled API hooks:
a successful nine-session run and eight injected failure scenarios (stale error,
open, start, preview, stop, format, close, short preview). They verify stage
identification, bounded reports and guest pixel-buffer release. These hooks are
not the PocketHLE dispatcher or a physical Windows/Android camera test.

## Suite build scope

The local build script generates private fixtures for regressions. Build/import
`tools/hardware-diag/dist/HARDWARE-DIAG.zip` through the main tool's README for
normal use. Fixture check counts retain their original revision scope; physical
provider validation remains separate from an API PASS.
