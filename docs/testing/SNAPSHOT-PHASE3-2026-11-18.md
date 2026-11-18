# Snapshot phase 3 — 18 November 2026

Base: branch `snapshots`, commit
`8acdc23` (`snapshots phase 2`). This is a generic native ARM checkpoint
increment. It changes neither game binaries/assets nor per-title behavior.
No Git commit or push was made.

## Implementation

- Guest HLE zlib streams are checkpointable without a compressed-input replay
  journal. A pinned upstream miniz_oxide core supplies the serializable decoder;
  a bounded-window adapter preserves the PNG small-output contract. General
  flate2 consumers retain zlib-rs. Partial header/Huffman/match/checksum states,
  completed/failed streams, IDs and reset/close behavior are retained. Output
  permission failures cannot advance or destroy the decoder.
- Completed native-channel HTTP responses retain their unread bounded suffix,
  head, terminal error/EOF and pending guest send identity. Restore reacquires
  the configured native client and never resubmits the captured request.
  Active/unknown transports remain rejected. Late producers cannot overwrite
  a completed or cancelled channel.
- An owned Desktop programmable renderer prefers the same native API as its UI.
  On Linux this avoids selecting ES solely because the guest uses GLES2 and
  unnecessarily recording a lifetime journal. Existing ES-only initialization
  remains available if desktop GL cannot be created. The Android context policy
  is unchanged; programmable snapshot capture explicitly rejects Android's
  non-recorded journal instead of emitting empty graphics state.
- Idle checkpoint polling skips the rendezvous mutex through an atomic flag.
  Membership, cancellation and capture remain coordinated under the original
  lock once a request is active.

The WinCE section is version **7**; programmable graphics remains version 5,
media 2 and device domain 1. Earlier development slots must be recreated.
No legacy development-format reader or per-device duplicate serializer is added.
See [Snapshot internals](../SNAPSHOTS.md) for ownership and format details.

## Validation environment

Linux x86-64, Rust 1.90.0, headless EGL, Mesa llvmpipe LLVM 19.1.7. This is a
software host OpenGL driver, not a physical GPU performance measurement.
Desktop default features include real Unicorn, CPAL and statically built
FFmpeg/OpenH264. No physical audio device is available; opening CPAL reports
that fact and the existing silent-output behavior is used.
SDL was built with its real X11/ALSA/controller backends. The local build
configuration disables SDL's Wayland backend because the available Wayland
headers lack the keyboard-repeat protocol enum required by that SDL release;
no repository dependency source or production build script was patched for this.
These tests do not certify Windows/WGL, Proton, Android or physical devices.

The opt-in Desktop probe uses the actual production runner. It operates only
on a disposable library, captures an explicit slot, destroys the source
emulator and restores into a freshly loaded CPU/kernel/graphics session. Its
periodic PPM images are explicit test readbacks; it is not a throughput bench.
A cold emulator restore does not mean that the operating system's disk cache
has been flushed. Timing observations are single runs, not medians.

## Recorded results

The default-feature release Desktop build succeeded. Rust 1.90 formatting
and `git diff --check` passed. Core: **29 passed, 2 ignored**; CPU: **54
passed**; kernel: **406 passed**; WinCE API: **782 passed, 28 ignored**.
The separately enabled native GLES2 snapshot tests passed **13/13**, including
color/depth/stencil and retained resource identity. The native fixed-function
surface checkpoint test also passed. Ignored tests were not counted as passed.

All four installed-game probes captured a slot, stopped and destroyed the
source emulator, restored into a new emulator and continued for eight seconds.
The last restored images show progressing animation in both Electopia runs
and the Gizmondo introduction; Unfinished Business resumes inside its level.

| Scene / context | Capture | Encode + write | Save response | Restore to first frame | Slot |
| --- | ---: | ---: | ---: | ---: | ---: |
| Electopia attract mode / shared | 1.098 s | 0.768 s | 1.881 s | 1.588 s | 37.79 MiB |
| Electopia attract mode / owned | 0.671 s | 0.554 s | 1.251 s | 0.805 s | 37.78 MiB |
| Unfinished Business / level | 0.640 s | 0.076 s | 0.728 s | 1.265 s | 2.37 MiB |
| Gizmondo GLES1 / animated introduction | 0.243 s | 0.116 s | 0.364 s | 0.872 s | 6.01 MiB |

Capture and background encoding/writing are separate phases; the response
includes their scheduling overhead. These figures are single observations
under llvmpipe with explicit probe readbacks, not physical-GPU timings or
a before/after performance comparison. The probe enables shared export;
the owned-context case deliberately has no UI share source.

Unfinished Business loads its supplied **native ARM `zlib.dll`**. That game
validates ordinary DLL/CPU restoration, not the new HLE decoder. Separate
section-codec and guest-API tests cover active HLE streams, fragmented headers,
small output buffers, window rollover, terminal errors and protected outputs.
The other Gizmondo archive is catalogued as Virtual Squash but selects
`Ball Busters.exe`; this run covers its animated 3D introduction, not gameplay.

A supplementary synthetic 2 MiB decoder comparison checked every output byte.
With 4 KiB output calls, the original HLE implementation measured 209–212 MiB/s
and the new adapter 861–1012 MiB/s; with 64 KiB calls, 206–258 versus
1060–1107 MiB/s. The 94-byte case varied substantially (old 152–173, new
84–165 MiB/s), so this does **not** establish a speedup for every call size.
These are isolated decoder observations, not game FPS measurements. No
temporary benchmark executable source is included in the production patch.

## Reproduction

```sh
cargo +1.90.0 fmt --all -- --check
rustfmt --edition 2021 --check frontends/pocket-desktop/src/runner.rs
cargo test --locked --release -p pocket-core -p pocket-kernel -p pocket-cpu \
  --features pocket-core/unicorn,pocket-core/video-static,pocket-core/audio-cpal \
  --lib -- --test-threads=1
cargo test --locked --release -p pocket-winceapi \
  --features unicorn-tests,video-static,audio-cpal --lib \
  -- --test-threads=1
EGL_PLATFORM=surfaceless cargo test --locked --release -p pocket-winceapi \
  --features unicorn-tests,video-static,audio-cpal --lib gles2:: \
  -- --include-ignored --test-threads=1
cargo build --locked --release -p pocket-desktop
```

For the opt-in installed-game probe, set `POCKETHLE_SNAPSHOT_TEST_LIBRARY` and
`POCKETHLE_SNAPSHOT_TEST_GAME`, then run the ignored
`external_game_device_snapshot_cold_restore` Desktop test. It overwrites the
snapshot slot: use a disposable library. `POCKETHLE_SNAPSHOT_TEST_CAPTURE_AFTER`
is in seconds. `POCKETHLE_SNAPSHOT_TEST_TAPS` uses
`milliseconds:x:y;milliseconds:x:y` in guest coordinates;
`POCKETHLE_SNAPSHOT_TEST_KEYS` uses `milliseconds:virtual_key` with decimal WinCE
virtual-key codes. `POCKETHLE_SNAPSHOT_TEST_OWNED_CONTEXT=1` selects an owned
context for the test instead of providing the UI share source. These are test
inputs, not runtime optimization controls or end-user settings.

## Remaining work, in priority order

This list records the phase 3 boundary. The subsequent
[phase 4 increment](SNAPSHOT-PHASE4-2026-11-18.md) implements the uniform-location
mapping mentioned in item 4; its remaining-work list is the newer reference.

1. **Direct resource checkpoints on host OpenGL ES.** The desktop GL resource
   serializer does not provide portable ES depth/stencil/texture queries.
   Implement explicit GPU-assisted plane transfers at capture/restore, native
   buffer reads where supported, texture/mipmap/compressed-image metadata and
   deleted-image identity. Preserve error state, caller bindings and resource
   cleanup. Never add per-frame GPU readbacks or replace the missing state with
   cleared images. Android snapshot UI is a separate missing integration.
2. **Preserve synchronous suspended launchers.** The existing foreground-child
   snapshot remains deliberately detached from its suspended SD-card caller.
   A complete solution needs a checkpoint boundary with access to the parent's
   CPU, kernel and suspended graphics/media ownership, then the common device
   serializer and commit barrier. Reusing a parent snapshot taken before the
   child started would save stale shared-file/device state. Test return to the
   launcher, nested handoffs, shared cursors/mappings and cancellation.
3. **Active external I/O and provider completion.** Completed native HTTP
   channels are now covered. Active HTTP/POST streams, Bluetooth ports/sockets
   and providers which cannot expose an immutable terminal response remain
   unsupported. External services and peers cannot be rewound by a local save.
   Any future disconnect/reconnect policy must use real native providers and
   produce real guest errors, without replaying POSTs or fabricating input.
   Android's Java HTTP adapter has not implemented completed-response capture.
4. **Native driver/platform validation and uniform identity.** Run on Windows
   WGL, Steam Deck/Proton, real microphone/camera/GPS and Quake 3 in gameplay.
   The native programmable restore still requires the same uniform locations
   from the host linker. A future per-program guest/native uniform map must
   cover scalar/vector/matrix/array queries and updates, repeat capture, failed
   and successful relink, and program deletion. Keep this in the common GLES2
   layer; do not rewrite shader/game data to force locations.

Closed files outside the captured open-resource graph and external filesystem
changes are still not a full directory rollback. The existing file transaction
is an in-process rollback guard, not a crash-durable multi-file filesystem
transaction. These policies were not changed in this increment.
