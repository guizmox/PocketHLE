# Android graphics diagnostics

Use this guide to collect graphics timings from the current Android build.
Rendering and presentation contracts are maintained in [GPU.md](GPU.md);
build the complete app with matching Rust JNI and Kotlin code as described in
[Build and release](BUILD.md#android). No old diagnostic or cumulative patch ZIP
is needed.

Shared GLES presentation reports `PRESENTATION=GPU_SHARED (Android)` when active.
CPU-origin logical RGB565 images use packed transport and GPU integer conversion:
look for `format=RGB565 conversion=GPU`. xBRZ or an unavailable integer conversion
uses the CPU fallback; alternate render images retain their RGBA transport.
Real guest reads and CPU composition can legitimately require readback.

For comparisons, keep the same game, scene, internal resolution and filter.
Disable Show IPS and recording so they do not change the presentation path.
Collect software and GPU runs as described below. For a specific sharing or
surface-lifecycle issue, a targeted GPU run plus background/resume is useful.
Neither the log's presence nor an older benchmark establishes a device-wide FPS
improvement. Earlier implementation/validation records are kept in the historical archive.

Enable **Emulator Settings > Diagnostics > Graphics diagnostics**, keep **Log
verbosity = 1**, then start a game. The switch defaults to off and applies at
next game launch. It is independent of rendering mode, FPS/IPS and frame limits.
Turn it off after collecting the report. Verbosity 2/3 is unnecessary and its
existing per-call debug logs can distort measurements.

To compare renderers, use a GLES 1.x game and set **OpenGL ES internal
resolution (host OpenGL)** to **Native** in the global graphics settings. Use the
same game, scene and filter for both runs. Run approximately 20 seconds of
intro and 20 seconds of gameplay with global **GPU rendering** off, then
enable it and relaunch the game for the second run. Keep the phone's power
mode unchanged. The **Generic OpenGL ES (2.0, Common)** profile uses host
OpenGL, so collect a host OpenGL run for GLES 2.0 diagnostics.

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
| emulation / frame_pack_cpu | Copying packed RGB565 or preparing alternate RGBA pixels for Android |
| emulation / shared_snapshot_wall | Shared image export, including native copy/fence submission; nested inside swap when applicable |
| jni / jni_copy_wall | Allocating/filling the Java frame array after a snapshot was obtained |
| guest_gpu / guest_draw_gpu_sample | GPU elapsed-query samples around native draw submissions, one attempted sample per 16 draws |
| android / shared_handoff | Whole synchronized JNI consumer callback, including fence submission and nested GL work; do not add it to shared_convert/draw |
| android / shared_convert | CPU submission of RGB565/row/backlight conversion of a shared GPU image, without pixel download |
| android / prepare | Packed/RGBA byte copy, optional CPU expansion/xBRZ and creation of the upload ByteBuffer |
| android / rgb565_convert | CPU submission of integer RGB565 expansion/backlight on the GPU |
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

## NV program observations

The same Android switch enables bounded `NV_DIAG` observations: extension/client
identity; decode/compile/capacity acceptance; semantic program fingerprints;
draw constants, coordinate routes and transforms; referenced texture formats,
filters, completeness, revisions and available payloads. Software feedback draws
may inspect already-owned CPU bytes; GPU snapshots are not materialized for a
trace. Prefix hashes cover at most 64 KiB and are not full-image checksums.
Missing CPU RGBA bytes can be normal when compressed/native decoding is deferred.

Draw observations are limited to uses 1, 2, 16 and 256 for at most 128 runtime
programs per context. Load events are capped at 512 and extension observations
at 32 per worker thread. Disabled diagnostics do not hash, format, inspect
pixels or insert trace-map rows; a flag check remains, not literally zero CPU
instructions. A different semantic hash does not by itself establish a bug.

## Desktop scope

The temporary desktop environment switch, per-program cost/phase counters and
desktop timer extension used for the November investigation are removed. Setting
that old variable no longer activates profiling. Android's pre-existing setting,
ES timer queries and bounded observations remain available as described above.
Functional rendering contracts belong to [GPU.md](GPU.md); dated measurements
and validation limits belong to [the original investigation](history/IMPLEMENTATION-NOTES.md#nv-combiner-and-egl-investigation-2026-11-09).
