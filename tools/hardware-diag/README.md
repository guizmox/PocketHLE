# HARDWARE DIAG

One importable diagnostic application, one native ARM executable:
`dist/HARDWARE-DIAG.zip`. Import this ZIP into PocketHLE and launch **HARDWARE DIAG**.
The menu, console, all eight hardware/API suites, OpenGL ES information and both
CPU benchmarks, the memory benchmark and the separate OpenGLES cost benchmark are linked into the same `AUTORUN.EXE`. Choosing a menu item runs
a function on an internal worker thread, not another executable.

The RAM/VFS tests must still create real guest processes to verify process
isolation, suspension, exit codes, handles and reclamation. They relaunch this
**same executable** with private diagnostic arguments. Small DLLs and data files
remain necessary for loading, dependency, paging and invalid-image tests. No
second executable is shipped, and no commercial assets or proprietary SDK are used.

## Use

- Up/Down selects a suite; Enter, Space or the Gizmondo action button runs it.
  Touch users can tap a menu row. `Run all tests` executes the thirteen entries.
- The console shows live checks and elapsed time while the worker runs.
  Up/Down scrolls it; after completion, Enter/Back or the bottom touch area
  returns to the menu. Each menu item retains its last PASS/FAIL result.
- Reports remain in the title's writable `Flash Disk`: `HARDWAREDIAG.TXT`
  summarizes runs, and `HARDWARELIVE.TXT` is a fixed 240-byte latest-progress mailbox. It is
  preallocated before each suite, so reporting never grows Flash usage while
  a storage/quota measurement is in progress. The console samples that mailbox;
  the original detailed suite reports retain every check.
  Original reports (`RAMTEST.TXT`, `PROCTEST.TXT`, `DLLTEST.TXT`, `VFSTEST.TXT`,
  `TIMETEST.TXT`, `CAMTEST.TXT`, `GPSTEST.TXT`, `NETTEST.TXT`, `VIBTEST.TXT`,
  `BTTEST.TXT`) remain available. GPS includes both the binary `GPS1:` interface
  and the `COM8:` serial/NMEA bridge: 4800 baud 8N1 configuration, timeouts,
  fragmented GGA/RMC sentences, checksums/CRLF, permissions and close/reopen.
  `COM8-NMEA.TXT` retains the actual received sentences, including NO_FIX status. RAM lifecycle checks also produce
  `RAMLIFECYCLE.TXT`/`.BIN`.
- Let a running suite finish. For a genuine hang use PocketHLE's Stop action;
  the application does not force-kill a thread inside its own shared process.
  A slow-run notice is not silently converted into a PASS.
- Camera, GPS, Bluetooth, vibration and network tests require the matching
  PocketHLE hardware settings and, where applicable, host permissions/providers.
  A GPS `NO_FIX` is distinguished from an API failure. An API PASS alone does
  not certify the physical hardware.

The original process-suite parent deadline is 15 seconds, with the original
5-second operation deadlines and exit-code requirements retained. Desktop,
Linux and Steam Deck runners must publish a parent's completed exit after its
private RAM is released, before waiting for surviving descendants. The Android
runner already uses this ordering. No diagnostic timeout is increased to hide
this process-lifecycle failure.

## OpenGL ES

`OGLESTEST.TXT` lists the **guest-visible** GL vendor, renderer, version,
extensions, exposed integer limits, viewport bounds and EGL configuration
attributes. Unsupported optional queries are marked unavailable. A colored
triangle and center-pixel readback exercise initialization, submission and
presentation. This is an OpenGL ES 1.x context test, not a host driver inventory,
ES 2.0 implementation or GPU performance benchmark.

## CPU benchmarks

Integer: deterministic 32-bit wrapping arithmetic, xorshift and multiply/add.
Floating point: single-precision multiply/add through the WinCE runtime ABI,
including the native helper fast path when provided by PocketHLE.

Each benchmark warms up, then records three timed passes, elapsed milliseconds,
work iterations, iterations/second and a checked result. The final
`FINAL_INTEGER_SCORE_ITERATIONS_PER_SECOND` /
`FINAL_FLOATING_SCORE_ITERATIONS_PER_SECOND` is the median of the three passes;
higher is faster. No valid final score is reported if computation/timing fails. Work is used and stored
through a volatile sink; it is not an empty loop optimized away. Fixed seeds and
IEEE single-precision rounding give a known result for every block. A benchmark
PASS verifies computation, not performance.

For comparisons, use the same build, device, ARM backend, clock/power settings
and diagnostic version. For an implementation comparison, change only the build
under test, restart the application and compare the median iterations/second.
Host optimizations are always enabled; there are no optimization settings to toggle.
Keep copies of the reports before another run replaces them. This ARM executable
does not measure the MIPS backend; scores are not directly comparable to
GLBenchmark's different workloads. Hardware queries, triangle rendering and
console I/O are outside the timed CPU blocks.

## Build and CI

Python 3, Clang and `ld.lld` are sufficient; no WinCE SDK is needed:

```sh
python tools/hardware-diag/build.py
```

Use `--clang`, `--lld`, `--work` or the `CLANG`/`LLD` environment variables for
custom compiler paths. Only these small freestanding diagnostics are compiled;
this command does not build PocketHLE or its native dependencies.

CI builds this tool and publishes one **HARDWARE-DIAG** artifact. The modules in
`suites/` also generate private fixture binaries for existing Rust/Android runner
regression tests. These are developer fixtures, not additional public diagnostic
apps. Host-provider tests and Bluetooth replay scripts remain under the same
folder; physical-radio checks still require a real peer. The optional native
radio probe is a developer example, not another published diagnostic app:
`cargo run -p pocket-desktop --example hardware-diag-bluetooth-host -- scan`.

`python tools/hardware-diag/run.py --guest` runs the existing host/guest developer
contract checks. Its hardware-provider limitations remain explicit. The
[original validation report](../../docs/history/VALIDATION.md#original-hardware-diagnostic-validation)
is historical; it does not certify the current application.

Public use starts with the current `dist/HARDWARE-DIAG.zip`, not a historical
standalone RAMTEST/GPSTEST/BTTEST ZIP. Suite-directory READMEs explain their
contracts and private fixture builds; they are not separate installation guides.

Optional bytecode smoke tests (controlled API/clock shims, not real-device or
PocketHLE conformance results): install `tests/requirements.txt`, build the tool,
then run `python tools/hardware-diag/tests/guest_smoke.py`. The normal tool build
and public application do not require these Python packages.

## OpenGLES Benchmark (separate menu entry)

The existing **OpenGL ES information + triangle** test is unchanged. The new
**OpenGLES Benchmark** runs in the same ARM EXE and writes `OGLESBENCH.TXT` to
Flash Disk. No extra application or game modification is needed.

Ten fixed workloads isolate common costs at a 320x240 viewport:

| Report name | Work per frame | Compare with |
| --- | --- | --- |
| geometry_512 / geometry_2048 / geometry_8192 | Small visible triangles, one draw | Each other: geometry load sweep |
| draw_calls_128 | Same 512 triangles in 128 draws | geometry_512: submission cost |
| matrix_animated | Same 512 triangles, modelview rotation | geometry_512: matrix path |
| vertices_rebuilt | Same 512 triangles, guest rebuild of coordinates | geometry_512: guest preparation |
| fill_solid | 16 fullscreen quads, opaque, no depth rejection | Fill baseline |
| fill_texture_nearest | Same quads, 64x64 RGBA checker texture | fill_solid |
| fill_texture_linear | Same scene, linear filtering | fill_texture_nearest |
| fill_texture_blend | Same linear scene, alpha blending | fill_texture_linear |

There is a short visible preview and warmup, followed by three samples of at
least 600 ms per workload. Samples draw eight-frame batches, completed by `glFinish`. They exclude
`eglSwapBuffers`, framebuffer readback, report I/O and progress notifications. Swaps between
samples/preview are deliberately outside the timer. Thus `render_millifps` is
**render throughput**, not display FPS, and includes guest CPU work, emulator
API processing and completion of rendering. This delivery fixes the previously ignored `glFinish` and `glFlush` in the
emulator. Native `glFinish` waits for rendering without a framebuffer download;
`glFlush` only submits work. Software rendering is already synchronous. The
benchmark requires this fix; the EXE cannot detect an old ignored handler.
This is not a GPU timer or a claim
that the physical display exceeds its refresh rate. Matrix animation includes
its API calls; rebuilding includes its guest arithmetic and memory writes.
No emulator scheduling/frame-limit setting is silently modified.

The median of the three rates is reported per test, along with reciprocal frame
microseconds, triangles/second and draw calls/frame. `1000 render_millifps` means
one rendered frame/second. `GLOBAL_INDEX_MILLI / 1000` is the geometric mean of
the ten median rates expressed as a fixed comparison index: 100 means 100
render frames/second in every workload. Higher is faster. This neutral reference
is a comparison convention, not a calibrated hardware score. Compare only the
same benchmark version, resolution, renderer and emulator settings; the report
records guest-visible GL vendor/renderer/version. Settings not exposed by the
WinCE API must be noted by the user, not guessed by the EXE. An API/readback/
clock error or an exceeded 180-second between-workloads budget suppresses the
global score. Extremely slow individual GL calls cannot be interrupted safely.

Normal duration is about 20-40 seconds, depending on presentation and renderer.
Use per-test results to find regressions; the global index alone cannot explain
a bottleneck. Mipmap, lighting and unrelated capabilities remain outside this
small cost probe. Reimport the rebuilt `dist/HARDWARE-DIAG.zip` to update the tool.

Optional controlled ARM smoke checks (not a real renderer benchmark):
`python tests/gles_benchmark_smoke.py`, after building and installing
`tests/requirements.txt`. They exercise the actual EXE with deterministic GL and
clock shims, verify workload counts and median/global calculations, and reject
missing APIs, GL errors, black readback and a stalled clock.

## Additional Integer / Floating cost probes (v2)

Both existing CPU menu entries now append separately named cost probes to their
existing reports. The original v1 workload, its three one-second samples and
`FINAL_INTEGER_SCORE_ITERATIONS_PER_SECOND` / `FINAL_FLOATING_SCORE_ITERATIONS_PER_SECOND`
remain comparable to previous versions. They are NOT replaced by a mixture of
unrelated workloads. The full suite fails if any added computation fails, even
when its historical v1 score remains valid.

Integer adds eight probes: direct ARM uint32 multiply/add, unsigned and signed
32-bit quotient+remainder, direct paired-word uint64 addition/shifts, runtime
64-bit multiplication, and unsigned/signed 64-by-64 division. The division tests
use nonzero varied divisors and validate BOTH quotient/remainder return words
where that WinCE ABI provides them. Inputs cover positive/negative bit patterns,
values with populated high words, carries and wrapping products.

Floating adds ten probes: separate float32 addition, multiplication, division,
square root, signed-int32/float32 roundtrip, float32/float64 roundtrip, float64
add+multiply, float64 division and float64 square root. Normal signed values and
zero are used; roots receive nonnegative values. The double-precision vector
also includes large and small finite magnitudes. Float32/float64 operations use
the WinCE soft-float/runtime ABI, so their rates include API thunk/dispatch costs.
They are NOT host FPU FLOPS. The EXE is ARM; these results do not measure MIPS.

Each new probe warms up, then performs three samples targeting 500 ms, with a
65536-block cap (1024 iterations per block). Early caps are explicitly reported.
`CASE_MEDIAN_ITERATIONS_PER_SECOND` is the comparable per-probe score; higher is
faster. Runtime calls per iteration are recorded. Input loads, loop control and
a common rolling checksum are INCLUDED, never estimated or subtracted. The
checksum consumes every result, including high words, and is verified against
fixed independent expected values in warmup AND every measured block. A bad
result/clock suppresses that probe's score. Other probes continue so the report
still identifies which categories fail. Allow roughly 15 seconds for Integer
and 22 seconds for Floating; slower runtime handlers may take longer.

`tests/cpu_benchmark_reference.py` contains an independent host arithmetic oracle
for the fixed vectors, rounding and expected checksums. The build does NOT
regenerate expected results from emulator output. After installing the existing
optional smoke dependencies, `python tests/cpu_benchmark_smoke.py` executes the
real ARM EXE under deterministic WinCE/clock shims and checks all workloads,
medians, 64-bit ABI return words and deliberately corrupted results. This is not
a substitute for executing HARDWARE DIAG in PocketHLE.

These are performance probes, not exhaustive instruction or IEEE-754 conformance
tests: NaNs, infinities, denormals, exception flags, all rounding modes and every
integer edge case are intentionally outside the timed workload.

The emulator now exposes the missing `glTexParameteri` by name for both Common
and Common-Lite. Existing historical ordinal tables are unchanged. This fixes
the OpenGLES benchmark's early `FAIL missing glTexParameteri`; do not conceal
missing required API support by weakening that test.


## On-screen score pages

Integer, Floating and OpenGLES now finish on one compact 320x240 score page:
one row per probe with its score, plus the historical CPU score or the OpenGLES
global index. CPU values are iterations/second; GL rows are render FPS (not
physical display refresh FPS). Higher is faster. The complete TXT reports and
their existing score keys remain available and unchanged.

Use B/Tab to switch between scores and the complete report, A/Enter/Back to
return to the menu, and Left/Right to switch between completed benchmark pages.
Touch the lower left/right corners to change pages, or the lower center to open
the log. In the menu, B/Tab recalls the selected benchmark's last result.
Missing/truncated reports and failed cases show FAIL, never a successful score.

GitHub Actions publishes `HARDWARE-DIAG` from its independent HARDWARE DIAG job;
the archive contains one integrated ARM executable. This job also verifies the
archive, ARM benchmark/report wiring, and score-page navigation. Re-import the
new diagnostic ZIP to update the executable inside PocketHLE.

## GLBenchmark-style CPU series

The existing per-case IDs and legacy scores are retained. Integer case 7
(`series_i32_signed_divide`) adds an alternating sum of 100000000 divided by
consecutive odd integers, through `__rt_sdiv`. Floating case 9
(`series_f32_runtime`) adds a float32 alternating reciprocal series, using
`__muls`, `__divs`, `__adds`, `__subs`, `__lts` and `__nes`: seven runtime
calls per iteration. Both use 1024 iterations with checked fixed results.
These independently written bounded probes exercise the operation families
observed in GLBenchmark 1.1.5's CPU loops. They do not reproduce its timing,
iteration count, binary or score, and cannot be compared numerically with it.

Integer, Floating and Memory display a black screen with centered
“Running benchmark...” before starting their worker, then a score page.

## Memory benchmark

The same ARM EXE now includes **Memory benchmark**, producing `MEMORY.TXT`.
It allocates two guarded 2 MiB buffers. Ten independently scored workloads:

| Workload | Unit | Better |
| --- | --- | --- |
| Sequential ARM reads: 16 KiB, 256 KiB, 2 MiB | MiB/s | Higher |
| Sequential ARM write: 2 MiB | MiB/s | Higher |
| ARM copy, WinCE memcpy, WinCE memset: 2 MiB | MiB/s | Higher |
| Read one word per 4 KiB page over 2 MiB | Million accesses/s | Higher |
| Dependent pointer chains: 16 KiB, 2 MiB | ns/access | Lower |

Each case warms up and takes three samples targeting 300 ms, with a bounded
block cap. The median is shown on a single screenshot-friendly page. Logical
payload bytes count once for copies; these are not estimates of bus traffic.
Setup, full output verification and report writes are outside timed intervals;
read checksums and loop overhead are included. Fixed results and guards are
checked, and invalid results, clocks or allocation failures suppress scores.
The existing RAM correctness suite remains independent and unchanged.

The dependent chains make each load depend on the preceding result. The page
stride exercises guest translation/memory paths; neither probe certifies host
cache misses or physical RAM latency. All figures measure this ARM guest in
PocketHLE, including emulator overhead, not the physical memory of an original
PocketPC or Gizmondo. There is no misleading combined score across unlike units.
Stop and relaunch HARDWARE DIAG after changing emulator optimizations.
