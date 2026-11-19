# Snapshot phase 7 — verified restore and managed state

Base: `9651be14f1f24f9779c22a3c5dd5dd6b63b962c7` on `snapshots`, plus the
phase 6 changes. The source delivery is cumulative against that commit.
Old development slots must be recreated. No compatibility reader was added.

## Architecture and ownership

- `pocket-snapshot`: one bounded bincode/zlib implementation for native sections
  and managed documents, buffered reads/writes, strict stream completion/checksum
  and suffix checks. It depends on no emulator/frontend component. `pocket-netcf`
  does not depend on the native `Emulator` or WinCE API crate.
- `pocket-session::snapshot`: kernel rendezvous, parked callers, shared process
  graph and restore commit barrier. `snapshot::archive` owns backend selection,
  off-thread preparation, opaque verified targets, completions and atomic writes.
- `pocket-session::compatibility`: semantic requirements shared by both GUIs.
  Native ARM and CF requirements are distinct. Host input mappings, presentation
  rotation, frame limits and provider enablement do not invalidate a checkpoint.
- Native schemas stay in core/kernel/CPU/GLES owners. Managed VM/service schemas
  stay in `pocket-netcf`. Existing GPU surface/context, audio and framebuffer
  snapshot implementations are reused; no second rasterizer or audio decoder.
- Prepared archives own plain state, not native CPU/GL/device handles. Dropping
  a pending delivery or staged target releases that state. Desktop serializes
  requests through first restored publication; cancellation clears staged data.

Preflight fully decodes and checks the archive before requesting the old session
to stop. It checks native ARM ranges/registers/continuation/hooks/thunk indexes,
managed object/service/control graphs, versioned payloads, executable hashes for
all saved processes and emulation requirements. The checked bytes and target
configuration are retained. Slot replacement cannot swap the validated payload.
Late GPU/device/dependency/file acquisition can still fail during reconstruction;
this does not promise transactional recovery of the old running session from
all external hardware failures.

The standard flate2 Read adapter accepted a stream missing the final zlib bytes
in a new truncation regression test. The shared adapter now requires the actual
DEFLATE StreamEnd/checksum and EOF. It continues to use flate2 for decoding;
there is no new compression implementation. Tests exercise every truncation of
a sample document, one-byte reads, checksum damage and an appended suffix.

## Managed continuation

A managed snapshot preserves IDs, aliases, static/type-initialization state,
free object slots, control trees, timers, fonts, images/bitmaps, string interning,
resource stream positions, RNG, sensors, XML service identities, FMOD handles,
shared mixer state and GLES resources. Capture is between complete callbacks;
it never serializes a live IL stack or host pointer. Restore does not rerun Main,
constructors, initial events or close handlers of the discarded instance.

Control/property/event names remain borrowed during normal execution and become
owned after decoding. New services need no separate snapshot name whitelist.
Object-graph validation is shared between decoded and running representations.
XML writer paths are sandboxed names below the current save root, not serialized
absolute paths. Closed persistent files are not rolled back by this feature.
Android retains its original input channel across managed replacements and
reapplies the current host rotation policy after restoration. The managed runner
uses the launch configuration pinned by preparation rather than reopening it. The host
input accumulator starts neutral and the sensor provider supplies the next sample.

## GPU and Android UI

Native compressed subimages use explicit format layouts, including non-square
ASTC. Whole images, including opaque PVRTC storage, replace the mirror directly;
a full accepted upload can recover a previously unsupported mirror. Unknown
partial layouts still fail capture rather than guess. Permissive compressed mip
states outside the GLES2 contract are not fabricated. The snapshot path does
not decode/recompress native compressed blocks.

Android uses S (Save) and L (Load) quarter circles at the physical app-window
corners, with cutout protection and EXIT/FULL's shared palette. They follow no
guest image geometry. FPS/IPS placement separately follows the rendered image,
including rotation, letterboxing and controller fullscreen. User device testing
covers these controls/popups; no physical-device UI result is claimed here.

## Reproducible validation

Rust 1.90.0; Linux Mesa llvmpipe; `EGL_PLATFORM=surfaceless`,
`GALLIUM_DRIVER=llvmpipe`, `LP_NUM_THREADS=2`. These are native GL/ES host-driver
tests, not measurements of phone or Windows GPU performance. Game assets stay
outside the repository and delivery; no game executable or asset is patched.

```sh
cargo fmt --all -- --check
cargo test -p pocket-snapshot --lib
cargo test -p pocket-session --features snapshots --lib
cargo test -p pocket-core --features unicorn --lib snapshot::tests -- --test-threads=1
cargo test -p pocket-winceapi --lib gles2::renderer -- --include-ignored --test-threads=1
```

Observed completed runs:

| Suite | Result |
| --- | --- |
| Shared codec integrity/size tests | 2 passed |
| CF unit tests, including a real software-renderer checkpoint with GPU preference enabled | 3 passed; external asset probes run separately. |
| Semantic compatibility tests | 3 passed |
| Core snapshots, including early invalid-continuation refusal and current FPS with saved waits | 10 passed |
| Native GLES2 renderers and compressed block cases | 20 passed |
| Android Kotlin unit suite | 116 passed |
| Diamond Beer native-GL cold managed checkpoint, two generations | Passed: frame, VM and RNG state compared; ticks continue after restore. |
| Diamond Beer host-ES cold managed checkpoint, two generations | Same checks passed with a genuine ES source and active native GPU renderer. |
| Android Session runner on Linux, Diamond Beer public Save/Restore commands | Two cycles passed in one JNI session with matching slot timestamps. |
| Desktop Diamond Beer cold restore and recapture through shared preparation | Passed; both generations verified with the backend-neutral reader. |
| Public native Session, Electopia | Two Save/Restore cycles passed. |
| Public native Session, Fathammer autorun/bootloader | Two Save/Restore cycles passed with launcher coordination; not an in-race gameplay test. |
| Public Session refusal/cancellation boundary | Truncated slot and changed internal-resolution policy refused while the old game continued publishing frames; a subsequent valid restore passed. |

The public Session probe is
`runner::snapshot_integration_tests::real_game_save_restore_reuses_session_and_publishes_saved_timestamp`.
Set `POCKETHLE_TEST_LIBRARY` and `POCKETHLE_TEST_GAME` to an external test library.
`bad_snapshot_and_changed_settings_leave_the_live_session_running` uses that
same interface to truncate the slot and change the resolution policy; it requires
an error while the old session continues producing frames, then a valid restore.
Use a disposable test-library copy because it deliberately edits its slot/config.

The CF checkpoint probe is
`runtime::snapshot::tests::diamond_beer_cold_checkpoint_continues_without_replaying_main`.
Set `POCKETHLE_DIAMOND_BEER_EXE`, optional `POCKETHLE_DIAMOND_BEER_SETTINGS`,
`POCKETHLE_NETCF_TEST_GPU=1` and optionally `POCKETHLE_NETCF_TEST_HOST_ES=1`.
It uses a temporary save directory and checks a recaptured generation, not only
two reads of the initial slot. Physical input/sensor permissions, Bluetooth peers
and Windows WGL remain device/platform checks; compilation is separate evidence.

The final public-command host runs used nonincremental host artifacts after a
local interrupted/inconsistent incremental winceapi archive produced hidden-symbol
link errors. Rebuilding that package resolved the link failure; no production
optimization/link setting was changed. The completed refusal/cycle tests passed.
The observed Diamond Beer acknowledgements were approximately 0.20–0.40 s for
Save and 0.24–0.32 s for Restore on this host run, not phone timings.

## Build boundaries

`cargo fmt --all -- --check`, `git diff --check` and Desktop/JNI Cargo checks
with `--no-default-features` and with `--no-default-features --features unicorn`
passed. The Desktop real-game probe compiled and linked the Unicorn frontend.
The all-default Linux check could not proceed without the host-target static
FFmpeg prerequisite; no fallback or production feature was changed to hide it.
Android uses the built static FFmpeg libraries for its two target ABIs.
No Windows compilation, physical Android device or live external-radio peer
validation is claimed by these host tests.

Final Android native release builds passed for `arm64-v8a` and `armeabi-v7a`
with one stable source fingerprint. The native verifier passed ELF dependencies,
static FFmpeg, JIT builtins and 16 KiB segment alignment for both libraries.
The final Kotlin run executed 116 tests, with zero failures, errors or skips.
After the final rotation/configuration fixes, the two public JNI Session
refusal/Save/Restore tests were rerun and passed.

`assembleDebug` passed with `verifyNativeLibraries` enabled and the fresh native
libraries above. The APK was assembled for validation only; this delivery is
source-only. Physical Android UI tests remain assigned to the user.
