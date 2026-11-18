# Snapshot phase 5 — suspended launchers and active communications

Base: `snapshots`, `8acdc23f100cb81eec1a87f989f7cbd55109e775`
(`snapshots phase 2`). The delivered source patch is cumulative: phases 3, 4 and
5. No commit or push was made. No supplied game executable, DLL or asset was
modified. Android integration remains a separate task.

## Suspended launchers

Desktop Unicorn now parks an SDCreateProcess caller on its own host thread at
the actual CPU continuation. The foreground child runs on another host thread.
Both use the existing checkpoint rendezvous, per-process sections and device
restore commit barrier. There is no prelaunch parent snapshot and no separate
Gizmondo/Pocket PC serializer. The former isolated foreground-child snapshot
path, its detached-handle filtering and its saved card-program field were removed.
The session section is now version 2 and the WinCE section version 8; recreate
development slots. No compatibility reader is retained.

The suspended owner keeps its native runtime guard on the owning thread.
Capture temporarily reinstalls its GLES/media state there and resuspends it
even if capture unwinds. DirectShow playback stays frozen during suspension;
cold-restored media starts its host clock lazily after the shared restore
barrier. No execution gate is held while waiting for peers or the child.

The archive validates child identity and rejects cyclic waits or missing/exited
targets. Restore cannot execute a caller instruction before its saved child
finishes. Process termination is serviced while the owner waits. Completion is
published after child teardown, then the caller's original guest code resumes.
Queued input releases are retained, so a launching key or stylus press does not
remain stuck. User cancellation is shared across jobs and remains distinct from
an emulation failure in another process.

Native return testing exposed another generic restriction: SDCreateProcess
rejected nonempty command lines. It now forwards them through the existing
WinMain/GetCommandLineW startup initialization, with the same image and output
validation. Unsupported creation flags still fail. This is required for a
launcher that reopens its menu with arguments after its selected game exits.

## Active communications

An external server or Bluetooth peer cannot be rolled back. The implemented
policy preserves data received at capture and explicitly terminates the saved
transport after restore. It never represents a disconnected endpoint as a live
host connection, never fabricates incoming data, and never replays outbound
writes or a submitted HTTP request.

- Native HTTP channels copy the response head and bounded unread mailbox under
  its existing lock. The source continues normally. A completed transfer retains
  its terminal result; an active transfer restores with error 12030 after the
  saved bytes. If headers had not arrived, the saved request reports that abort
  immediately. Pending guest send identity is retained without resubmission.
- Windows Bluetooth Classic uses FIONREAD/MSG_PEEK for non-consuming receive
  capture. COM aliases share one receive boundary per checkpoint and one port
  object after restore. Sockets/COM ports drain saved data before a transport
  error, normally 10054. Pending inquiry becomes cancelled (995); already
  received results survive. Partial-write offsets survive but are never used to
  resend data to the old peer. New connections use the real permission-gated
  backend. Capture does not initiate connect, accept or discovery.
- A receive checkpoint is bounded to 64 KiB per Bluetooth endpoint and 64 MiB
  aggregate process Bluetooth state. Unknown providers reject capture instead
  of claiming to support it. Weak identity/cache references retain no lifetime
  byte history. Windows Bluetooth permission and hardware availability checks
  remain effective on restoration and subsequent operations.

This supports restoring the game with disconnected external endpoints. It does
not promise uninterrupted multiplayer, resumed server-side transactions, or a
rewound radio peer. Android Java HTTP/Bluetooth checkpoint providers are not
implemented by this increment.

## Validation

Linux x86-64, Rust 1.90.0, headless native EGL/OpenGL, Mesa llvmpipe LLVM 19.1.7.
This is native API execution using a software host GL driver, not a physical
GPU or Windows/WGL measurement. No physical audio/radio/microphone device is
available. CPAL reports the missing device and uses its existing silent mode.
The local SDL dependency configuration retains X11/ALSA/controller support and
disables its incompatible Wayland build; repository dependency source is unchanged.

The checks passed under Rust 1.90.0:

| Check | Result |
| --- | --- |
| Core with Unicorn | 31 passed, 2 ignored |
| CPU library | 14 passed |
| Kernel library | 411 passed |
| WinCE API with native video/Unicorn features | 786 passed, 29 ignored |
| Explicit native GLES2 suite | 16 passed, none ignored |
| Desktop default-feature tests | 177 passed, 8 ignored |
| Desktop default features, release | Build passed |
| Desktop without default features | Compilation check passed |
| Rust 1.90 formatting and Git whitespace | Passed, including explicit runner include-tree check |
| Windows Bluetooth adapter bindings | Type check passed; no Windows link/hardware test |

Automated coverage includes real loopback I/O while capturing an active HTTP
POST before headers and after partial body delivery: no second request is sent,
the source receives later data normally, and the restored transfer contains only
the captured prefix then the abort. Bluetooth tests use real TCP loopback as a
transport fixture, not a physical-radio claim. They cover non-consuming capture,
peek/read/error, repeated capture, port alias/lease identity, shared receive
epochs, permission shutdown, cancelled inquiry and partial writes without replay.
A separate type check compiles the actual Windows adapter source against its
windows-sys bindings; this is not a linked Windows build or hardware test.

Core tests cold-restore a real Unicorn parent/child pair, prove that the caller
CPU executes no instruction before child completion, then verify its result.
A cyclic suspended chain is rejected. API tests cover temporary owning-thread
runtime activation, frozen DirectShow time and resuspension after a caught panic.
Desktop tests distinguish shared user stop from peer-failure cancellation.

Fathammer Classics was exercised through the production Desktop runner with
unchanged installed binaries and a disposable library. The actual flow is:

1. `autorun.exe` waits for `bootloader.exe`.
2. The menu exits with selection 42; `autorun` starts `sce/sce.exe`.
3. A snapshot retains PID 1 (`autorun`) and foreground PID 3 (Stuntcar).
4. The source CPUs are destroyed. Cold restore reconstructs both processes and
   returns to Stuntcar's menu, with working input.
5. Quitting Stuntcar runs its credits, exits, resumes `autorun`, and launches
   `bootloader.exe` with command line `foo` through the game's own API calls.
6. Another capture succeeds from the returned Fathammer menu and records PID 1
   plus foreground PID 4 (`bootloader`).

Electopia was also cold-restored from its 3D attract sequence with an owned
native OpenGL context using the final section format. Its saved 800×480 scene
continued rendering after restore. The observed capture took 0.192 s, background
encode/write 0.553 s, save response 0.752 s and first restored frame 1.407 s.

In the final Stuntcar/menu round trip, source capture took 0.409 s, background
encode/write 0.657 s, save response 1.129 s and the first cold-restored frame
1.610 s. Capture from the natively returned Fathammer menu took 0.542 s, its
encode/write 0.430 s and the save response 0.981 s; its manifest contains
`autorun` PID 1 plus `bootloader` PID 4. These are individual runs under
build/test load, with warm OS file caches and explicit test screenshot readbacks.
They are not a steady-state throughput or cold-disk performance benchmark.
Stuntcar menu/launch and native return are tested here; no claim of a complete
race is made.

The opt-in installed-game test remains in `runner/snapshot_tests.rs`. Temporary
standalone probe source is removed before delivery. Besides the existing source
key/tap schedule, it accepts:

| Variable | Meaning |
| --- | --- |
| `POCKETHLE_SNAPSHOT_TEST_RESTORE_KEYS` | Restored-session `milliseconds:WinCE-VK` pairs separated by semicolons. |
| `POCKETHLE_SNAPSHOT_TEST_RESTORE_SECONDS` | Restored-session observation duration; default 8 seconds. |
| `POCKETHLE_SNAPSHOT_TEST_RECAPTURE_AFTER` | Optional restored-session capture time in seconds. |

Use a disposable library: the test replaces its snapshot slot and normal game
code may write saves. For an initialized Stuntcar player profile, an example is:

```sh
export POCKETHLE_SNAPSHOT_TEST_LIBRARY=/path/to/disposable-library
export POCKETHLE_SNAPSHOT_TEST_GAME=GZGA200010
export POCKETHLE_SNAPSHOT_TEST_KEYS='9000:40;9500:40;10500:13;18000:13'
export POCKETHLE_SNAPSHOT_TEST_CAPTURE_AFTER=25
export POCKETHLE_SNAPSHOT_TEST_RESTORE_KEYS='6000:40;7000:40;8000:40;9000:40;10000:40;11000:40;13000:13;25000:32;35000:32'
export POCKETHLE_SNAPSHOT_TEST_RESTORE_SECONDS=70
export POCKETHLE_SNAPSHOT_TEST_RECAPTURE_AFTER=60
EGL_PLATFORM=surfaceless cargo test --locked --release -p pocket-desktop \
  external_game_device_snapshot_cold_restore -- --ignored --nocapture --test-threads=1
```

Stuntcar runs credits before exiting; allow them to finish before recapture.
The key schedule depends on game initialization timing and existing legitimate
save data. Check the emitted process manifest and images; a successful frame
count alone does not prove the selected child or native return.

## Remaining boundaries

- Android UI, provider and device validation is deferred.
- Direct programmable resource capture on a host OpenGL ES context still needs
  the portable resource/depth/stencil path. Desktop ES fallback retains its
  bounded journal; this phase does not alter the phase 4 limitation.
- Active programs whose latest native link failed remain explicitly rejected.
- Hardware checks on Windows/WGL, Proton and Bluetooth/microphone/camera/GPS
  devices remain necessary. Loopback/native-binding checks are narrower evidence.
- Files closed at capture are not rolled back. This is still an open-resource
  checkpoint, not a full save-directory or remote-service rollback. Multi-file
  installation uses the existing in-process rollback guard, not crash-durable
  filesystem transactions.
