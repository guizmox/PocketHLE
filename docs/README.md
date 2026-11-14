# Documentation

Start here for current behavior. Each subject has one owning guide; link to it
instead of copying settings, build commands or default bindings into another file.
Documentation prose is English. Source identifiers and original product names
retain their spelling.

## User and release guides

| Task | Guide |
| --- | --- |
| Install on Steam Deck through Proton | [Steam Deck](STEAMDECK.md) |
| Configure games, controls, touch layouts and the desktop toolbar | [Frontends](FRONTENDS.md) |
| Configure an extracted Windows CE image, Explorer or a console session | [Windows CE profiles](WINDOWS-CE.md) |
| Run supported managed Pocket PC games | [.NET Compact Framework](FRONTENDS.md#net-compact-framework-games) |
| Find reported titles and the limits of compatibility evidence | [Compatibility](COMPATIBILITY.md) |
| Use Gizmondo hardware services, GTShell and Colors | [Gizmondo](GIZMONDO.md) |
| Build Linux/Windows/Android, sign an APK or publish an update | [Build and release](BUILD.md) |
| Collect Android graphics/NV logs | [Graphics diagnostics](ANDROID-GRAPHICS-DIAGNOSTICS.md) |

## Contributor references

| Subject | Owner |
| --- | --- |
| Architecture, ABI/memory/scheduler contracts, CPU optimizations and primary sources | [Architecture](AGENTS.md) |
| Rendering, EGL/NV execution, pixel ownership, resolution, filters and pacing | [Graphics](GPU.md) |

The root [README](../README.md) is the project overview. Contributor entry points
remain [AGENTS.md](../AGENTS.md) and [CLAUDE.md](../CLAUDE.md); neither is another
architecture copy. The Android source-directory [README](../frontends/pocket-android/README.md)
is an entry point to these same guides.

## Tool manuals and evidence

Keep operating instructions beside tools that are distributed or used separately:

- [HARDWARE DIAG](../tools/hardware-diag/README.md): fixture generation, integrated
  suites and optional ARM execution. Individual suite READMEs describe their own
  tests, not alternate emulator settings or build policy.
- [Android ADB backup/rescue](../tools/README-android-adb-backup.txt): standalone
  recovery scripts and their safeguards; this notice also ships with those tools.
- [Input artwork](../frontends/pocket-desktop/assets/input/README.md): skin sources
  and hitbox assets, shared with Android.
- [FFmpeg](../third-party/ffmpeg/README.md) and
  [OpenH264](../third-party/openh264/README.md): source provenance and relinking
  notices. License texts remain with the corresponding dependency.
- [proof](../proof/) and [validation](../validation/) contain dated runs/logs.
  They are evidence, not current user instructions.

## Historical records

[Implementation notes](history/IMPLEMENTATION-NOTES.md) retain earlier ABI details,
regressions and investigations with their original anchors. [Validation records](history/VALIDATION.md)
collect old test counts, measurements and superseded delivery handouts.
Current guides take precedence over those snapshots. Do not install an old patch
or recreate a removed setting merely because a historical note mentions it.

## Validation levels

Keep source inspection, isolated/unit tests, backend compilation, complete package/CI
builds, native host-provider tests and physical gameplay/device validation separate.
Preserve binary version, platform, renderer, internal resolution, power state and
workload for performance comparisons. A benchmark or diagnostic PASS is not
exhaustive CPU/API/hardware conformance. Catalogue membership is not a completed
playthrough or a new cross-platform compatibility campaign.
