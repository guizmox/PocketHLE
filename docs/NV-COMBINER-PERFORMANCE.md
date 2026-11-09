# NV combiner performance investigation — 2026-11-09

Baseline: 91b8afbe90352a7763b2f631b24bcb1c2bbde5c1. Cumulative with the
EGL alpha, shared NV programs, extended AR11 and mixer implementation.

## Measurement method

Linux x86-64, Unicorn, native Mesa llvmpipe, VGA 480x640, internal resolution
1x, graphics diagnostics disabled. This machine has no physical NVIDIA GPU.
Values below count completed guest presentations per second, **not desktop
frontend FPS**, and do not predict GTX 1650 or Android absolute performance.
Runs are sequential, with no concurrent compilation. The harness holds a native
sharing context and drains shared exports. Benchmark counters and screenshots
are local test tools; none of that instrumentation is included in this patch.

Siege is left untouched for 35 seconds to enter its automatic 3D demo. Captures
verify the demo, rather than the logo/menu, at the measurement boundaries.
The camera moves, so these short windows are not frame-identical benchmarks.
Doom uses a separate test-only WAD with its start moved into the outdoor
courtyard; original assets are preserved and no game data is distributed.
The timed window begins after eight seconds. Boundary captures are outside
the measured interval. IPS-enabled tests match the supplied Windows log.

## Initial comparison (eight-second windows)

| Version | Siege demo presentations/s | Doom outdoor presentations/s |
| --- | ---: | ---: |
| Original baseline (missing Doom effects) | 18.75 | 30.49 |
| EGL alpha/shared programs/mixer patch | 34.11 | 1.62 |
| Extended AR11 patch | 31.12 | 1.87 |

The original Doom image is incomplete, so 30.49 is not a performance target
for an equivalent rendered image. The large Doom regression already appears
when its reflection passes become available, before the extended AR11 patch.
Siege's reported severe regression is **not reproduced** by this comparison.
A repeated extended Siege run gives 24.75; moving-scene and timing variability
prevent a precise small-difference claim.

## Measured bottleneck and correction

The extended Doom run makes 86,755 framebuffer feedback copies for 15 completed
presentations: 5,784 copies per image. A combiner that only reads destination
alpha was using the same per-triangle ordering path as combiners which modify
the destination channels they read. Its draw masks alpha writes, making those
repeated copies redundant. The correction computes conservative framebuffer
channel dependencies at program load and captures once per complete draw when
those channels cannot change. Actual feedback hazards retain primitive order.
Native vertex streams can also be reused in the stable-channel case.

The first corrected Doom run produces 17.37 presentations/s, with 175 feedback
copies per image: approximately 9.3x the extended run and 97% fewer copies per
image. A second corrected run gives 18.83 presentations/s and 174 feedback copies per image. Both versions show the sky and basin in the scene captures. Remaining
copies include genuine ordered feedback; this is not a claim that all copies
can disappear or that the game has no other bottleneck.

Siege measures 30.11 with the correction and IPS enabled. Its roughly five to
six feedback copies per image are far below Doom's previous thousands. This
correction therefore should not be presented as a measured Siege speed fix.
Without IPS, the extended and corrected Siege runs give 34.66 and 35.41 over
12-second windows, with no main presentation ReadPixels calls. Their difference
is too small for a speedup claim given the observed variability.

## Supplied Windows log

The log selects NVIDIA GeForce GTX 1650 and an RGBA8888 480x640 EGL surface.
`SHARED_HANDOFF=CPU reason=ips_requires_cpu_pixels` means enabled IPS forces
CPU pixel acquisition for presentation; GLES rendering still uses the GPU.
The initial RGB565 CPU_UPLOAD message precedes EGL surface creation and does
not establish software GLES fallback. Repeated renderer messages alone do not
measure per-frame context creation. No AR11 timing or graphics diagnostic
breakdown is present, so this log cannot assign the Siege slowdown to a pass.

## Remaining optimization candidates and regression checks

- Measure the actual NVIDIA demo with IPS disabled/enabled at the same point.
  Local IPS-on Siege performs one main readback per presentation; the measured
  wall time inside it is roughly one second per eight-second window. Driver
  synchronization can cost more elsewhere, so that is not total readback cost.
- Investigate texture barriers for genuine same-pixel ordered feedback on
  capable desktop GL hosts, preserving the existing Android/unsupported path.
  No barrier-based change is included here.
- Investigate RGBA texture copy/subimage paths separately. No CPU CopyTexImage
  fallback was counted in the measured Siege windows. These counters do not
  cover CopyTexSubImage or every shared snapshot materialization.
- Verify Siege water and Doom reflections, including overlapping geometry,
  with software/GPU, IPS on/off, and 1x/2x. Stable-alpha optimization is GPU-only;
  software arithmetic is unchanged.

Generic overlapping-triangle tests compare stable-alpha and writable-alpha
feedback on software, desktop GL and GLES, including additive blending and
expected pixel values. Writable hazards and logic operations retain their old
ordering path. Previously documented validation and the pre-existing native
RGB565 snapshot-test discrepancy still apply. No game signature is used.

Final checks: cargo fmt --all --check, git diff --check, 247 regular GLES tests,
the native stable/writable-alpha feedback test (desktop GL and GLES), and
pocket-gles aarch64-linux-android cross-check pass. Earlier cumulative WinCE
and kernel suites remain documented in EGL-ALPHA-NV-MIXER.md.
