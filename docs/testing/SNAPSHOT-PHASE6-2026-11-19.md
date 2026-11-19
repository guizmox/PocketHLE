# Snapshot phase 6 — shared GUI engine and host ES transfers

Base: `9651be1` on `snapshots` (its commit title says phase 4; its source includes
the phase 5 transport policy, device graph and suspended-launcher implementation).
This record describes development validation, not physical-device conformance.

## Implementation boundaries

- `pocket-session` owns capture/rendezvous, parked launchers, shared child-job
  completion, cold device restore, background slot writes, slot decoding and
  mount-aware saved-executable resolution. Desktop and Android implement small
  process/frame adapters. They use the existing common section codec.
- Android keeps one JNI session/presentation root through a cold restore. The old
  process graph and CPUs finish before its stop state is reset. Save acknowledgements
  follow atomic slot publication; restore acknowledgements follow the first
  foreground frame after the common restore commit barrier.
- Android HTTP, serial Bluetooth and RFCOMM providers capture bounded unread data
  without consuming/cancelling the source endpoint. Active endpoints restore with
  the existing transport-loss policy. They never replay outbound requests/writes.
- `pocket-gles::gpu::checkpoint_transfer` implements ES GPU staging for both
  fixed-function and programmable snapshots. The GLES2 archive adapter contains
  no second shader/transfer implementation. A complete programmable session also
  captures its fixed-function section, so fixing GLES2 alone did not suffice.
- Color textures, cube faces and mips are sampled by native shaders; ES buffer
  storage is mapped for explicit capture. Accepted texture allocation metadata and
  current compressed blocks replace the former call-history journal.
- Depth uses GPU equality tests to verify a representative that writes back to
  the actual saved native depth; stencil uses eight masked GPU passes. Fixed
  renderer transfers use a private shared context, preserving its state cache.
  Native objects remain on their owning threads. File writers receive bytes only.
- A current program's last successful executable/interface and pending attribute
  bindings survive failed relinks. Restore installs the successful executable,
  restores uniforms, then repeats the genuine failed native link. Native status
  queries and rendering remain native.
- All attachment reads force tight packing before writing their bounded CPU
  destination. An odd-width retained/deleted image with PACK_ALIGNMENT=8 is a
  regression case; the guest packing state must be restored afterwards.

The current section versions are session 2, WinCE 8, fixed-function EGL 2,
programmable EGL 6 and media 2. Recreate earlier development slots. There is no
legacy journal decoder or backwards-compatibility branch.

## Validation commands

Use Rust 1.90.0 and a native GL/ES driver. The Linux runs here used Mesa llvmpipe,
`EGL_PLATFORM=surfaceless`, `GALLIUM_DRIVER=llvmpipe`, `LP_NUM_THREADS=2`. This
executes the native GL pipeline through a software host driver; it is not evidence
of phone or Windows GPU performance. Local development headers/SDK/toolchains
were installed outside the repository. Production build configuration was kept.

```sh
cargo +1.90.0 fmt --all -- --check
rustfmt --edition 2021 --check frontends/pocket-desktop/src/runner.rs
cargo test -p pocket-core --features unicorn --lib snapshot::tests -- --test-threads=1
cargo test -p pocket-gles --lib native_surface_snapshot -- --include-ignored --test-threads=1
cargo test -p pocket-gles --lib native_es_surface_snapshot -- --include-ignored --test-threads=1
cargo test -p pocket-winceapi --lib gles2::renderer -- --include-ignored --test-threads=1
cargo test -p pocket-android-jni --no-default-features --features unicorn --lib checkpoint_io
```

The Android Gradle `testDebugUnitTest` run passed 116 tests, including two real
loopback HTTP tests: a partially received POST response preserves its unread
suffix and original source transfer, and capture before headers neither invents
headers nor cancels the source request. Both assert real completion afterwards;
the partial-response case asserts a single POST.

GPU tests cover RGB565/depth surface round trips on desktop GL and ES, all 256
stencil values with varying depth, cold/repeated restore, distinct cube faces and
mips, legacy alpha/luminance storage, buffers, deleted retained images, guest-name
reuse, uniform relocation and a real failed-link installed executable. Restored
program tests clear the saved color and draw again, preventing a saved screenshot
from masquerading as a working program.

The final native GLES2 renderer run passed all 17 selected tests, and the core
snapshot run passed all eight selected tests. The two fixed-function surface
round trips passed separately on desktop GL and ES. The two Android provider
packet-validation tests and two common session-diagnostic tests also passed.

## Real-game probes

Assets are supplied externally and never included in the source patch. Test
library copies preserve game binaries; executable selection changes only the
ordinary manifest option when testing an individual compilation-card game.

The Desktop opt-in probe is
`runner::snapshot_tests::external_game_device_snapshot_cold_restore`. It uses
`POCKETHLE_SNAPSHOT_TEST_LIBRARY`, `POCKETHLE_SNAPSHOT_TEST_GAME`, timed KEYS/TAPS,
CAPTURE_AFTER, RESTORE_SECONDS and optional RECAPTURE_AFTER. Set
`POCKETHLE_SNAPSHOT_TEST_HOST_ES=1` to share a real ES context; its archive and
coordinator are identical to Android's path.

The Android-runner opt-in host probe is
`runner::snapshot_integration_tests::real_game_save_restore_reuses_session_and_publishes_saved_timestamp`.
It uses `POCKETHLE_TEST_LIBRARY`, `POCKETHLE_TEST_GAME` and optional
`POCKETHLE_TEST_KEYS`/`POCKETHLE_TEST_WARMUP`. It performs two Save/Restore cycles
through the public asynchronous Session commands, verifies the original slot
timestamp and continued frame publication, and checks clean user stop.

Observed host runs:

| Case | Observed scope |
| --- | --- |
| Electopia, Desktop native OpenGL | Title rendering, save, cold restore, continued frames and recapture. |
| Electopia, Desktop sharing an ES context | The same complete session path, including fixed-function ES depth and programmable resources; cold restore and recapture. |
| Electopia, Android runner on Linux | Two Save/Restore cycles in one persistent Session; matching slot timestamps. |
| Fathammer compilation launcher, Android runner on Linux | `autorun.exe` with suspended caller and `bootloader.exe`; two coordinated cold restores. This alone is not a Stuntcar race test. |
| Stuntcar Extreme selected as `SCE/sce.exe`, Desktop sharing an ES context | Saved, cold-restored, continued publishing frames and recaptured successfully. The automated probe did not navigate a race. |

The ordinary native-GL Electopia host probe observed about 1.7–1.9 s to acknowledge
a save and 2.1 s to the first cold-restored frame. The ES probe observed about
3.1 s for save acknowledgements and 1.7 s to its first restored frame. These are
individual debug-optimized development runs under concurrent compilation, not a
controlled performance comparison. The Android runner was executed on Linux for
these measurements; they are not handset timings.

## Packaging and remaining physical validation

Both ARM64 and ARMv7 Android native builds use the pinned static FFmpeg libraries
and the existing 16 KiB page-alignment flags. Run `check-android-native.py` and the
source/library freshness guard before packaging an APK. A source change during
compilation invalidates the verification; rebuild before recording a new stamp.

Both final release native builds passed from the same unchanged source
fingerprint. The ELF/static-FFmpeg/JIT-builtins/16-KiB checks and native freshness
guard passed for both ABIs. The final Desktop Unicorn typecheck also passed.
Android `assembleDebug` then completed successfully with those verified native
libraries; no native-verification task was skipped for APK assembly.

The user will check the red/green quarter-circle buttons, success date/time
notifications and existing error dialog on an Android device. No physical
Bluetooth peer, phone sensors or Windows WGL driver was available for this run.
The new providers are compiled against the real Android APIs; physical radio and
permission-reacquisition validation remains separate from receive-packet tests.
Unsupported resources/capabilities fail explicitly and preserve the previous slot.
Host ES transfers require ES 3 (also required by the Android manifest). Restore
requires the same compatible host API, executable, mounts and emulated settings.
