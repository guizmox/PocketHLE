# HARDWARE DIAG

One importable diagnostic application, one native ARM executable:
`dist/HARDWARE-DIAG.zip`. Import this ZIP into PocketHLE and launch **HARDWARE DIAG**.
The menu, console, all eight hardware/API suites, OpenGL ES information and both
CPU benchmarks are linked into the same `AUTORUN.EXE`. Choosing a menu item runs
a function on an internal worker thread, not another executable.

The RAM/VFS tests must still create real guest processes to verify process
isolation, suspension, exit codes, handles and reclamation. They relaunch this
**same executable** with private diagnostic arguments. Small DLLs and data files
remain necessary for loading, dependency, paging and invalid-image tests. No
second executable is shipped, and no commercial assets or proprietary SDK are used.

## Use

- Up/Down selects a suite; Enter, Space or the Gizmondo action button runs it.
  Touch users can tap a menu row. `Run all tests` executes the eleven entries.
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

The process suite's aggregate parent wait is two minutes; its individual
5-second operation deadlines and success/exit-code checks are retained. This
avoids reporting a false parent failure when a full successful suite takes
more than fifteen seconds.

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
and diagnostic version. Change the optimization settings in PocketHLE, restart
the application, repeat the benchmark and compare the median iterations/second.
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
contract checks. Its hardware-provider limitations remain explicit. The old
`VALIDATION.md` is a historical report; it does not certify this new application.

Apply the repository patch, then run the supplied `removed-files.txt` cleanup
from the repository root. It lists only obsolete tracked files, no directories,
wildcards, game data or user reports. It does not remove old diagnostic titles
already imported in your personal PocketHLE library.

Optional bytecode smoke tests (controlled API/clock shims, not real-device or
PocketHLE conformance results): install `tests/requirements.txt`, build the tool,
then run `python tools/hardware-diag/tests/guest_smoke.py`. The normal tool build
and public application do not require these Python packages.
