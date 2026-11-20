# PocketPC raw framebuffer copies and Android corner controls

Base: `b24d80122d4dc7607dce69e1a8348f918b75a99d` (`snapshots`).

## Partial GAPI writes

A partial HLE memory copy to a CPU-resident, tightly packed raw framebuffer now
updates only the overlapping bytes in the CPU image. Publication is coalesced by
the existing display poll. Previously every scanline reread the entire display,
advanced its revision and caused another kernel readback. A scanline is not an
explicit completed frame, and must not establish the explicit-present backoff.

Explicit GX draws, locked DirectDraw primaries, column-major/padded displays and
GPU-owned images retain their existing boundaries and conversion paths. This
change does not enable CPU readbacks for GLES games that merely query a raw
framebuffer pointer. Pending publication is host work, not a new snapshot section:
RAM and the current CPU image already contain the writes. Restore clears the
pending flag; a cold frontend publishes its captured first image independently.

## Prey measurement

Unmodified `Prey3D.exe`, entered gameplay using its normal keyboard menus.
Linux Unicorn host, no physical Android device. Two sequential 36-second runs
used the same inputs, capture hook, 60-FPS frontend limit and executable. Temporary
row-copy tracing identified complete 320-row passes; process CPU counters were
sampled once per second. Profiling switches are not production settings.

The game requests a 62-ms timer, approximately 16.13 updates per second. Both
runs attained that cadence on this host. During roughly 28–35 seconds in-game:

| Path | Wall time | Process CPU time | Complete passes | CPU time/pass |
| --- | ---: | ---: | ---: | ---: |
| Original scanline publication | 7.058 s | 3.19 s | 113 | 28.23 ms |
| Coalesced partial copies | 7.058 s | 2.20 s | 114 | 19.30 ms |

This is about 31% less process CPU time in that sample, not a measured Android
FPS gain. The presentation overlay must not be interpreted as proof that this
software-rendered game produces 60 distinct images per second. The original
phone slowdown still needs verification with the optimized Android build.

## DOS paths

The earlier DOS-name fix is ported as a focused diff into the current VFS,
retaining its snapshot implementation. Expansion happens before selecting
mounts, writable overlays and sharing identities. Literal names take precedence;
ambiguous generated aliases are refused. Tests cover virtual mount ancestors,
nested files, writable overlays, sharing conflicts and ambiguous aliases.
Brothers in Arms now opens its resource files through its short paths. Successful
resource access alone does not establish successful graphical startup.

## Android controls

The Classic and Controller landscape banks reserve the physical snapshot radius
plus an 8-dp safety gap. Classic shortens both action rows while preserving its
lower bank edge. Controller lowers the P bank, D-pad and stylus stick, shortens
the P keys, and brings its face diamond closer to the stick. The top toolbar,
guest screen geometry and portrait layouts retain their established policies.

The Kotlin suite passed all 116 tests. Geometry checks include both layouts,
Normal/Big sizes, phone/tablet viewports and fractional densities. They check
actual corner clearance as well as existing target overlap and integer fitting.
Physical Android play testing remains separate from these host checks.

## Completed Rust validation

Rust 1.90.0: all 416 kernel unit tests passed; 781 WinCE API unit tests passed
with 35 opt-in integrations left ignored by the ordinary suite. The selected
host-GL regression `gpu_guest_draw_readpixels_gapi_and_parent_resume` also passed
when explicitly enabled, including guest draws, CPU reads and parent resumption.
The separate locked-scanline regression passed. These checks include the DOS
alias and existing VFS snapshot tests. Rust formatting and whitespace checks pass.

The 12 native core snapshot tests pass with WinCE section version 10, including
cold execution resumption, device groups, mapped-file aliases and archive
validation. The current Desktop and Android JNI frontend checks pass with Unicorn.

The copied ranges also update the existing last-observed guest sample. If GDI
paints after those copies, the poll publishes the CPU image in API order rather
than replaying unchanged guest bytes over newer GDI pixels. The mixed-order
regression checks both the retained GDI region and the unaffected copied region.
A missing polling history uses the existing full synchronization fallback once;
partial fallbacks do not establish a completed-frame polling lease. Explicit GX
presentation remains on its established path.

The complete kernel run exposed an unrelated preview-test assertion that queried
the public audio cursor. That query deliberately advances silent playback using
wall time, which makes the preview assertion race a short sample buffer. The test
now checks the stored cursor and unchanged device-clock flag directly, then
validates real draining through the public API. Production audio is unchanged.

The final implementation was checked again after the GDI-order protection was
added: a 7.044-second gameplay sample used 2.27 seconds of process CPU time,
versus 3.19 seconds over 7.058 seconds in the original sample (about 29% lower
CPU use after normalizing wall time). A trial taken during concurrent native
compilation used 3.41 CPU seconds and is not a controlled comparison; native
builds were stopped for the repeat. These host results support an approximate
30% CPU reduction, not a guaranteed phone frame rate.

## Android packaging

The final native sources compiled in release mode for both `arm64-v8a` and
`armeabi-v7a` from one stable source fingerprint. ELF dependencies, static FFmpeg,
JIT builtins and 16-KiB alignment checks passed for both ABIs. The source/library
freshness check passed without an override. Gradle `testDebugUnitTest assembleDebug`
completed successfully with `verifyNativeLibraries` enabled. The follow-up Kotlin
suite has 121 tests, with no failures, errors or skips. The APK was assembled for
validation; this delivery contains source files only. Physical Android and
Windows gameplay remain user-device validation tasks.
