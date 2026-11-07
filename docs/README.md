# Documentation

Use the domain guides for current behavior. Read a historical note only when
its subsystem or reproduction is relevant. All documentation prose is English;
source identifiers, paths and original product names retain their spelling.

| Guide | Owns |
| --- | --- |
| [AGENTS.md](AGENTS.md) | Core architecture, ABI/memory/scheduler invariants and contributor checks |
| [GPU.md](GPU.md) | Rendering, pixel authority, internal resolution, filters, FPS and input-stutter fix |
| [FRONTENDS.md](FRONTENDS.md) | Shared settings, input profiles/commands, desktop button toolbar, display/input geometry and platform UI parity |
| [ANDROID-BUILD.md](ANDROID-BUILD.md) | Android toolchain, packaging, signing and runner checks |
| [STEAMDECK.md](STEAMDECK.md) | Dedicated Proton edition, controller gestures and distribution |
| [GTSHELL.md](GTSHELL.md) | Optional Gizmondo shell profiles, local XML DOM and separate SD cards |
| [GIZMONDO.md](GIZMONDO.md) | Device services, imports and dated compatibility reports |
| [COLORS.md](COLORS.md) | Colors save/wallet/server contracts and scoped measurements |
| [OPTIMIZATIONS.md](OPTIMIZATIONS.md) | Always-on optimizations and architecture guards |
| [UPDATES.md](UPDATES.md) | Release-check protocol and publishing procedure |
| [references.md](references.md) | External technical sources |
| [IMPLEMENTATION-NOTES.md](IMPLEMENTATION-NOTES.md) | Detailed older contracts, regression cases and historical evidence |

The root [README](../README.md) remains the project overview. Do not duplicate
current policy across a build recipe, an old delivery note and a settings guide.
When behavior changes, update its owning guide and link to it from other pages.

## Validation levels

Keep these separate in every report: source inspection, isolated/unit tests,
backend build, complete package/CI build, native host-provider test and physical
gameplay/device validation. Preserve binary version, platform, renderer, internal
resolution, power state and workload when reporting a performance comparison.
Do not call a diagnostic benchmark exhaustive API or hardware conformance.

## Diagnostics and benchmarks

[HARDWARE DIAG](../tools/hardware-diag/README.md) is one importable ARM application:
its menu calls integrated suites. Process/RAM/VFS checks can relaunch the same
EXE and load private DLL fixtures; that does not make it a launcher for old public
BTTEST/GPSTEST/VFSTEST applications. Original report filenames remain useful.
GPS coverage includes the GPS1 interface and COM8 NMEA bridge. Sensor/network
checks need the corresponding permissions, providers and often a real peer.

```sh
python tools/hardware-diag/build.py
python tools/hardware-diag/run.py --guest
```

Integer, floating-point, memory and GLES cost benchmarks measure selected
workloads and checked results. Compare the same tool revision and settings;
use per-test scores as well as the aggregate. Preserve the detailed text reports,
not just a screenshot. This is not exhaustive CPU/GLES conformance. CI artifacts
and private integration fixtures have distinct purposes; consult the tool README.

Recorded runs remain under [proof](../proof/). Historical test counts and a
maintainer's compatibility report are not a new cross-platform test campaign.
