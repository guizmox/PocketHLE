# Desktop snapshot internals

The desktop frontend checkpoints one foreground ARM WinCE process. Gizmondo
and Pocket PC share the serializer and restore path. See
[the frontend guide](FRONTENDS.md#experimental-desktop-snapshots) for commands
and slot locations.

## Capture and write

The desktop runner queues the request and captures at a kernel checkpoint on
the emulation thread. This boundary provides a continuation PC and a consistent
view of CPU, kernel and graphics state. A supported synchronously launched child
can be captured as the foreground program while its caller is suspended. The
snapshot identifies that child so the launcher can reopen it directly.

The captured state is moved to a background writer. The writer encodes it into a
temporary file in the slot directory, flushes and syncs the file, then persists
it over the slot. The previous slot remains available if capture or writing
fails. Capture itself, including any required graphics readback, happens before
the writer starts.
[Desktop checkpoint hook](../frontends/pocket-desktop/src/runner.rs) ·
[Slot storage](../crates/pocket-library/src/snapshot_slot.rs)

## Disk layout

| Layer | Contents |
| --- | --- |
| Slot header | Eight-byte magic `PHSNAP01` and a little-endian 64-bit UTC timestamp in Unix milliseconds. |
| Session header | Eight-byte magic `PHSECT01`, followed by one zlib stream. |
| Decompressed stream | Little-endian 16-bit section count, then each section's 16-bit tag, 16-bit version, 64-bit byte length and fixed-integer bincode payload. |

Session identity and continuation, ARM execution, WinCE kernel, fixed-function
EGL and media are always present. Programmable EGL, HSS and pending input
releases are emitted when applicable. The high bit of the tag marks a required
section. The reader checks for duplicate tags, bounds each section read and
enforces a cumulative 512 MiB budget for section headers and payloads. It can
skip unknown optional sections and rejects unknown required sections.
[Session codec](../crates/pocket-core/src/snapshot.rs)

## Restore and ARM block reads

The launcher reads the slot and opens the selected executable with its normal
device configuration and mounts. Restore checks the executable fingerprint and
thunk indices. It releases the bootstrap CPU mappings, installs the saved kernel
state, reconstructs Unicorn against the restored memory budget and verifies
the continuation PC. It then reinstalls EGL and media state, restores the
guest clock for execution and resumes the saved continuation. Physical input
starts neutral. The kernel queues release messages for keys or stylus presses
the guest had already received.

ARM mappings and image-page blobs carry their original bincode length-prefixed
byte representation. Their Serde reader requests a byte buffer, so bincode can
read the whole blob into owned storage through the buffered zlib reader. The
reader still checks section bounds. The subsequent Unicorn restore preserves
guest addresses, permissions, lazy image residency and deferred memory ranges.
[ARM memory snapshot](../crates/pocket-cpu/src/unicorn_memory_snapshot.rs) ·
[ARM execution snapshot](../crates/pocket-cpu/src/unicorn_execution_snapshot.rs)

## Handle coverage

| Resource | Saved state and restore behavior |
| --- | --- |
| Process-local handle table | Guest handle values, aliases, closed entries, names, thread control, process and thread exit state, and Toolhelp snapshots. Supported object types include the current process, threads, events, semaphores, mutexes, files and devices. Scheduler state supplies the synchronization objects and waits. |
| Regular VFS files | Mount and relative path, file size and modification time, access/share contract, and cursor. Restore reopens the backing file and preserves a single cursor across duplicate handles. |
| Gizmondo devices | BTH1 driver state and lease, VIB1 settings and lease, GPS1 fix status, and CAM1 stream state. GPS and camera reconnect through the configured providers. Capture requires camera requests to have completed. |
| MAS1 | Decoder state, submitted data, pending completions and playback cursor. Aliases refer to the same decoder and playback stream. |
| Pocket PC accelerometer | SEN0 and ACC1 stream types and guest handles. Reads resume through the current hardware or simulated input provider. |
| Audio and media | Mixer queues and voice cursors, HSS speaker/channel/clip state, and idle DirectShow graphs. |

Unsupported live resources cause capture to fail with a diagnostic. Current gaps
include shared or transferred process handles, file mappings, VFS RAM files and
volume handles, Bluetooth COM ports, host serial and network connections,
cryptography objects, database cursors, MSXML and SDL state. Active microphone
capture and DirectShow graphs with loaded media also block capture.

When adding a handle type, check both the guest handle table and the backing
object. Recreate the backing object through its subsystem, reconnect aliases to
the same instance and preserve the guest-visible handle values. Test continued
use and closure after a cold restore, including two handles for the same object
when the API supports duplication.
[Handle table](../crates/pocket-kernel/src/handle_snapshot.rs) ·
[VFS resources](../crates/pocket-kernel/src/vfs_snapshot.rs) ·
[Scheduler](../crates/pocket-kernel/src/schedule_snapshot.rs) ·
[Kernel orchestration](../crates/pocket-kernel/src/kernel_snapshot.rs)

## Graphics and performance

Fixed-function EGL restores its context and resources through the fixed-function
snapshot path. GLES 2.0 rebuilds resources and framebuffer contents by replaying
the guest calls through the host driver. The programmable journal stores
identical calls once in a dictionary and keeps a chronological stream of indices.
Replay remaps native object names to preserve the identifiers the guest already
holds. The journal budgets 96 MiB for the dictionary and order stream. Draws
still execute in chronological order, so a long journal can take longer to
replay even when its encoded size is small.
[Fixed-function EGL](../crates/pocket-winceapi/src/gles/snapshot.rs) ·
[GLES 2.0 journal](../crates/pocket-winceapi/src/gles2/renderer/journal.rs) ·
[GLES 2.0 object names](../crates/pocket-winceapi/src/gles2/renderer/names.rs)

Profile capture, compression and write, read and decode, ARM/WinCE restore,
graphics reconstruction and first published frame separately. A Linux Mesa
development run over saved GLBenchmark 2.0, Street Duel and HTC Teeter sessions
measured approximately 0.9–1.0 seconds saved in read and decode with block reads.
The comparison used the same zlib files, with five measured restores per
variant after warm-up. Graphics replay dominated the sampled GLBenchmark restore.
