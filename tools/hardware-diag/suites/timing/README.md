# TIMETEST v2 — ARM timing and callback diagnostic

Run **Timing / threads / audio** from the single HARDWARE DIAG executable.
Select the 60 FPS game limit before this pacing contract test; Unlimited is not
its test configuration. Run in Software and GPU modes, restarting the title after
changing the renderer. The report is `\Flash Disk\TIMETEST.TXT`.
The current window-surface revision performs 46 checks. It creates a native
window surface before binding the EGL context. A 43-check report without
`egl.native_window`, `egl.window_surface` and `egl.destroy_surface` comes from an
older binary; rebuild HARDWARE DIAG instead of enabling surfaceless contexts.
No test deadline or lower-bound assertion was relaxed in this revision.

Checks: repeated 20 ms Sleep, single/multiple wait timeouts and manual event
state, worker signaling/exit, timer callback delivery/cancel/restart, waveOut
EVENT/FUNCTION completion including a nested Sleep, 40 waveOut open/close
cycles, identical-frame EGL swap pacing and late-resume cadence, EGL teardown.
The PCM is silent, so the test measures completion rather than sound quality.
It is deliberately a contract diagnostic, not an exact hardware-speed benchmark.
Measured host latency is logged; long scheduling delays produce warnings rather
than fabricated success/failure thresholds. Early completion and incorrect API
results still fail. Worker/audio waits are bounded.

The guest clock has millisecond quantization; lower-bound tolerances are explicit.
Do not interpret a 60 Hz maximum as a requirement that every game run at 60 FPS.
Headless native regression uses software GLES and the virtual audio clock. Actual
CPAL/Android audio output and native GPU runs must also be tested on the device.
DirectDraw, pause/resume and visual/audio quality are covered by host contract
suites/manual games, not fully exercised by this guest program.

Build the integrated tool: `python3 tools/hardware-diag/build.py --clang clang --lld ld.lld`.
The suite-local build remains a private regression fixture.
Native integration: `cargo test -p pocket-core --no-default-features --features arm-diagnostics --test ram_guest native_arm_timing -- --nocapture`.
