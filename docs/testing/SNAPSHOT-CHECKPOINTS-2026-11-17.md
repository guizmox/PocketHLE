# Snapshot checkpoint validation — 17 November 2026

Base: branch `snapshots`, commit `495a7f111314a845963dbed7eef24065942484a6`.
These changes cover writable open files, RAM-store files/mappings, coordinated
concurrent processes, native desktop GL resource checkpoints and additional
WinCE/media resources. See [the technical guide](../SNAPSHOTS.md).

## Environment and exercised paths

Rust 1.90.0, Linux x86-64, surfaceless EGL, Mesa llvmpipe (LLVM 19.1.7).
This is a software host OpenGL driver; it does not establish hardware-GPU timings
or Windows/WGL, Steam Deck/Proton or Android compatibility.

Electopia was loaded from the supplied extracted files, with an 800x480 emulated
panel, the programmable profile and a shared **desktop OpenGL** context source.
The installation was mounted read-only at its declared guest installation path,
with a separate writable save overlay. No game binary or asset was patched.
The headless hook requested capture at a kernel continuation, encoded the slot,
dropped the emulator, constructed a fresh normally configured emulator, decoded
and restored the checkpoint and ran guest code again. A second run also captured
after restore, then performed another cold restore of that new checkpoint.
The sampled scene was the animated title screen, not a playable level.
Audio output was disabled in this build; decoder/mixer state was still present.

## Earlier Electopia comparison (before the resource-coverage expansion)

One 90-second run per revision, same scene and setup. Values are **single-run
observations**, not warmed medians; build activity and host scheduling varied.
Do not extrapolate these figures to another driver or game.

| Phase | Original journal path | Resource checkpoint |
| --- | ---: | ---: |
| Capture, synchronous pause | 96 ms | 465 ms |
| Encode/write to memory, background-writer equivalent | 1,336 ms | 895 ms |
| Read/decode | 264 ms | 299 ms |
| Reconstruct ARM, WinCE and graphics | 1,021 ms | 445 ms |
| Compressed session bytes | 22,205,224 | 29,000,913 |

The direct checkpoint trades more explicit resource readback and a larger slot
for reconstruction that does not grow with the lifetime draw-call history.
Disk flush/rename and frontend first-frame delivery are not included in this
table. The existing desktop writer still runs encoding/disk I/O in the background.

## Automated checks

```sh
cargo +1.90.0 fmt --all -- --check
cargo test --locked -p pocket-core -p pocket-kernel -p pocket-cpu \
  --features pocket-core/unicorn --no-default-features --lib -- --test-threads=1
EGL_PLATFORM=surfaceless cargo test --locked -p pocket-winceapi \
  --features unicorn-tests --no-default-features --lib gles2:: \
  -- --include-ignored --test-threads=1
```

Core: 27 passed, 2 ignored. CPU: 54 passed. Kernel: 396 passed, followed by
the additional shared-resume-clock test below.
WinCE API suite with the static media decoder: 782 passed, 27 ignored.
Programmable graphics: 12 passed with native GL tests explicitly enabled.
The graphical checkpoint test cold-reconstructs native names while the previous
context is still alive, preserves a render-produced texture and depth buffer,
checks depth rejection after restore and keeps a linked executable whose guest
shaders were detached and deleted. The original window image also survives.

The VFS tests exercise writable content rollback, independent readers, duplicated
cursors, append mode, pending-write rollback and unchanged files on rejected
validation. The end-to-end RAM mapping test closes the original file handle,
round-trips the section codec and verifies visibility through both mapped aliases
and a newly opened ordinary handle, with unchanged storage accounting.

Quake 3 in an actual level was not tested: no game files for it were supplied
in this workspace. Windows desktop compilation/GUI operation and hardware-driver
performance are not certified by the Linux runs. Concurrent-process evidence is
described separately below.

## Coordinated and native-resource validation

The installed Ferrari demo was started through `launcher.exe`, not directly
through the game binary. Synthetic taps selected the demo, then capture recorded
three active guest processes: `launcher.exe`, `FGTH_launcher.exe` and
`FGTH_FerrariGT.exe`, with process 3 in the foreground. The original runtimes
were stopped/dropped; restore reconstructed all three on their owning host
threads and resumed the animated motion-controls title screen. This validates
the launcher and GLES2 title scene, not driving in a race. The probe also found
and corrected a difference between normal launch and restore path canonicalization
when the test game's directory was reached through a symlink.

A two-Unicorn-CPU test round-trips the common codec, reconstructs one shared
section and event, verifies cross-process writes and resumes both ARM CPUs. Both continuations receive the same native resume
epoch; a separate clock test checks a CPU that starts later still observes the
elapsed device time.
A wrong executable in one participant cancels the group and leaves a writable
backing unchanged. VFS tests verify common transferred disk/RAM cursors and one
RAM charge, including closure and reopening after reconstruction.

A native GL lifetime test deletes a texture and depth renderbuffer while two
unbound FBOs retain them, reuses the texture's guest name for an independent live
image, restores and verifies color/depth, writes through one old FBO and reads
through the other. It captures/restores again and checks that detaching the last
references removes the private keepers. No GPU image is copied/read during
ordinary deletion; private FBO references retain the original GPU image.

New WinCE checks cover incremental SHA-1 continuation, crypto-key ownership,
DOM aliases and final release, SDL surface contents and neutral input, GPS serial
provider/permission changes, volume rebasing, pending camera delays and CEDB
cursor/backing rollback. Initial CEDB file creation is removed on rollback;
closed persistent databases are not rewound. Logical WinINet tests reopen the
current provider for sessions/connections/unsent requests and verify that restore
never sends a request; an in-flight POST is explicitly refused.

DirectShow tests use the statically linked FFmpeg decoder and the repository's
`seeking.wmv` fixture: preserve the current frame/playhead, drop the old decoder,
reopen/seek and continue decoding. Changed media fails before installing partial
graphs. Microphone lifecycle/closed-callback tests pass; disabled backend restore
fails explicitly. The production reconnect path uses the real cpal backend, but
there is no physical microphone/audio device in this environment, so hardware
capture has not been certified.

The desktop opt-in probe is in
`frontends/pocket-desktop/src/runner/snapshot_tests.rs`. Use a **disposable copy**
of a library: the test replaces the game's snapshot slot. Set
`POCKETHLE_SNAPSHOT_TEST_LIBRARY`, `POCKETHLE_SNAPSHOT_TEST_GAME`, optional
`POCKETHLE_SNAPSHOT_TEST_CAPTURE_AFTER` (seconds) and optional
`POCKETHLE_SNAPSHOT_TEST_TAPS` (`milliseconds:x:y;milliseconds:x:y`, raw guest
coordinates), then run:

```sh
cargo test --locked -p pocket-desktop external_game_device_snapshot_cold_restore \
  -- --ignored --nocapture
```

## Remaining explicit limits

- A suspended synchronous SD-card caller remains outside the existing detached
  foreground snapshot. Processes still waiting for startup cannot rendezvous.
- Host OpenGL ES contexts retain the 96 MiB journal fallback; direct resource
  capture currently uses desktop OpenGL. Desktop controls do not add Android UI.
- Active HTTP transfers, Bluetooth ports/sockets and live zlib decompressors
  remain unsupported. No remote POST or external socket stream is silently
  replayed, and no unavailable hardware is replaced with fabricated input.
- WGL/Windows, Proton/Steam Deck, physical microphone/camera/GPS and Quake 3 in
  gameplay still need validation on those actual configurations/assets.

Development format: WinCE section 6, media section 2, programmable section 5,
device section 1. Previous experimental slots must be recreated.

The final Linux desktop release build completed with default features (Unicorn,
cpal and statically linked FFmpeg): `cargo build --locked --release -p
pocket-desktop`. Rust 1.90.0 `cargo fmt --all -- --check` and a direct `rustfmt
--edition 2021 --check frontends/pocket-desktop/src/runner.rs` both pass. The
latter also covers the runner modules reached through `desktop_main.rs`'s include.
The final default-feature Ferrari cold-restore probe passes; audio device open
attempts report no physical ALSA device in this environment. No game asset or
binary was modified. No Git commit or push was made.
