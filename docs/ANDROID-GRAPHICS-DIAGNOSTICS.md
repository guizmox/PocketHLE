# Android graphics diagnostics

Baseline: e68ff5f0885dfda587753688c70dc86f51018f8e (deck - pc single build with args).

The cumulative shared-presentation patch supersedes the initial diagnostic ZIP.
After applying it and rebuilding the complete APK (Rust JNI and Kotlin together),
repeat only the GPU Toy Golf run: about 20 seconds of intro and 20 seconds of play.
Keep reconstruction/native resolution and **Show IPS off**. Look for
`PRESENTATION=GPU_SHARED (Android)`. Normal shared images should no longer have
per-frame `read_pixels_wall`, Java pixel copies or texture-byte uploads; real
guest reads/CPU composition can still require readback. The UI now reports
`shared_convert` instead of CPU pixel preparation for these images. A brief
background/resume within the same run checks the surface lifecycle on the device.
The original two-run procedure below remains useful for a new unprofiled device.

Enable **Emulator Settings > Diagnostics > Graphics diagnostics**, keep **Log
verbosity = 1**, then start a game. The switch defaults to off and applies at
next game launch. It is independent of rendering mode, FPS/IPS and frame limits.
Turn it off after collecting the report. Verbosity 2/3 is unnecessary and its
existing per-call debug logs can distort measurements.

Only two runs are needed. Use the same game, scene, native internal resolution
and filter for both; disable optional IPS measurement and recording to avoid
those changing the presentation path. Run approximately 20 seconds of intro and
20 seconds of gameplay in software, then repeat with GPU rendering. Keep the
phone's power mode unchanged. Collect both runs in one log:

```bat
adb logcat -c
adb logcat -v threadtime PocketHLE:I "*:S" > georally-diagnostic.txt
```

Start capture before the first run; stop it with Ctrl+C after the second.
`GPU_DIAG session_start` identifies the game, requested renderer, internal
resolution policy and filter. Existing renderer/error messages show the actual
backend. The UI's renderer/vendor/version are also logged.

## Reading the report

`GPU_DIAG` records are INFO messages, grouped into windows of at least one
second on each participating thread/context. Each active stage reports calls,
total, mean and maximum milliseconds. A final incomplete window is not emitted;
a stopped producer does not generate heartbeat records. Windows are independent
across threads. Stage counts are diagnostic calls, never FPS or IPS.

| Domain / stage | Measured interval |
| --- | --- |
| emulation / guest_draw_cpu | Whole glDrawArrays or glDrawElements handler, including guest-memory preparation and the selected backend |
| emulation / guest_clear_cpu | Whole glClear handler, for either backend |
| emulation / egl_swap_wall | One eglSwapBuffers dispatch, including publication/readback when performed; retries are calls, not frames |
| emulation / submit_cpu | Native draw implementation, including context activation, texture/vertex preparation, shader work on the CPU and GL submission |
| emulation / read_pixels_wall | Existing native glReadPixels calls, including any driver wait charged to the call |
| emulation / materialize_wall | Frame hook's CPU materialization, black-image check and presentation composition |
| emulation / frame_pack_cpu | Building the RGBA snapshot sent to Android |
| jni / jni_copy_wall | Allocating/filling the Java frame array after a snapshot was obtained |
| guest_gpu / guest_draw_gpu_sample | GPU elapsed-query samples around native draw submissions, one attempted sample per 16 draws |
| android / shared_handoff | Whole synchronized JNI consumer callback, including fence submission and nested GL work; do not add it to shared_convert/draw |
| android / shared_convert | CPU submission of RGB565/row/backlight conversion of a shared GPU image, without pixel download |
| android / prepare | Pixel-array copy, optional xBRZ and creation of the upload ByteBuffer |
| android / upload | CPU elapsed time submitting glTexImage2D |
| android / filter | CPU elapsed time submitting optional SMAA passes |
| android / draw | CPU elapsed time preparing/submitting the final fullscreen draw |
| android / draw_gap | Interval between drawing callbacks; includes image arrival, scheduling and previous presentation delays |
| android / present_gpu | GPU elapsed query around upload, filtering and final drawing; CPU pixel preprocessing is excluded |

CPU intervals are **wall-clock durations**, including possible preemption or
blocking. They are not CPU-utilization percentages. Inclusive/nested intervals
must not be added: guest_draw_cpu includes submit_cpu; materialize_wall may
include read_pixels_wall. Likewise CPU and GPU intervals overlap.

GPU intervals use `EXT_disjoint_timer_query`, with 64-bit nanosecond results.
The emulation and UI contexts have their own bounded query pools (16 and 8).
Results are read only after QUERY_RESULT_AVAILABLE. A full pool skips a sample;
`gpu_skipped` counts these events. `gpu_disjoint` counts invalidated batches;
invalid results are discarded. UI `gpu_overflow` means the native result reader
could not supply a valid signed 64-bit value. These counters carry zero duration.
Capability checks log `timer=unavailable` explicitly when unsupported. No CPU
submission duration is substituted for a missing GPU measurement.

Native draw samples do not measure the entire frame: clears, other transfers,
unsampled draws and presentation are outside them. Do not multiply a sampled
sum by 16 and call it GPU utilization. UI queries do not include GLSurfaceView's
subsequent EGL swap, Android composition or physical scanout. Draw gaps can
expose delays outside the measured GL region, but do not by themselves identify
which component caused them. Elapsed-query behavior on tiled GPUs is also
implementation-dependent; these measurements are evidence for localization,
not a hardware busy-percentage meter.

## Scope and validation

There is no added glFinish, result polling loop, fence wait or pixel readback.
The existing rendering choice, authority/coherency, image publication,
RGB565 handling, input, FPS/IPS and pacing are preserved. Enabling diagnostics
still has overhead (clocks, aggregated logs and sampled GPU queries). Default-off
execution issues no diagnostic GL queries, takes no diagnostic JNI lock and does
not create diagnostic timestamps.

The ZIP contains source changes and this guide, not an APK. Validation includes
Rust formatting, GLES and Android-runner unit tests, config persistence, query
pool/disjoint tests and Kotlin aggregation/config tests. The Android renderer
sources were additionally type-checked against Android API classes with app
boundary stubs; that is not a complete APK build. The full NDK/FFmpeg toolchain
and the Samsung device remain necessary for APK/runtime validation.

Local checks on this change:

- `cargo fmt --all -- --check` and `git diff --check`: passed.
- GLES: 234 passed, 72 native graphics tests ignored by default. Eight shared-texture native GL/EGL tests were also run explicitly on Mesa, all passed.
- WinCE API: 713 passed, 13 hardware tests ignored by default.
- Android runner with Unicorn: 19 passed (the upstream cleanup removed the old base64 example test), including ARM RAM/process/VFS probes
  after generating the repository's diagnostic fixtures.
- Library setting persistence: passed.
- Kotlin aggregation/configuration and frame protocol: 7 passed with Kotlin 2.1.21.
- GLES Rust code checked for aarch64-linux-android; the exact new JNI timer
  function also checked separately for that target against the real JNI crate.
- Renderer/timer Kotlin sources type-checked against Android 15 API classes,
  with stubs for app-boundary dependencies only.

No complete APK build, device benchmark or physical GPU timer validation is
claimed by these checks. Existing hardware tests remain excluded here.

Merge audit for the current baseline:

- Applied the implementation as a three-way delta onto 9cfdcd1, with the
  pre-merge source/patch retained independently.
- Kept the upstream removal of Android screenshot support, legacy run/import
  JNI functions, base64 and unused submitted-FPS telemetry.
- SDL3 dependencies, Cargo.lock, controller-label changes and UI cleanup are
  taken directly from the upstream commit; this patch does not replace them.
- The AndroidFrameRenderer conflict was resolved by retaining GPU transport,
  timing and resource cleanup while keeping screenshot code removed.

## e68ff5f rebase and complete file audit

All 30 entries in the previous ZIP were compared, not just selected source
files. Only pocket-library/src/lib.rs and patch-files.txt overlap the new commit;
none of the nine added source/documentation files collides with a new upstream
file. The complete source delta applies cleanly in three-way mode to e68ff5f.
Compared with the previous ZIP (before this guide/manifest update), only the
merged library source differs. Its delta against upstream is limited to the
diagnostic configuration field/default and persistence assertions. The runtime
Standard/SteamDeck profiles and their config routing remain exactly upstream.
The manifest is regenerated for this cumulative ZIP; desktop/CI/profile files
outside the library source are not overwritten by the patch.
