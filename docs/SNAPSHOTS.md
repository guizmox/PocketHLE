# Desktop snapshot internals

The desktop frontend checkpoints an ARM WinCE session, including concurrently
running processes. Gizmondo and Pocket PC share the serializer and restore path. See
[the frontend guide](FRONTENDS.md#experimental-desktop-snapshots) for commands
and slot locations.

## Capture and write

The desktop runner queues the request and captures at a kernel checkpoint on
the emulation thread. This boundary provides a continuation PC and a consistent
view of CPU, kernel and graphics state. Synchronously suspended launchers remain
participants: the snapshot includes each parked caller and its active child,
using the same coordinated process sections as concurrent launches. Restore
rebuilds that graph before either continuation can execute.

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

Session identity and continuation, ARM execution, WinCE kernel (including mapped sections), fixed-function
EGL and media are always present. Programmable EGL, HSS and pending input
releases are emitted when applicable. Coordinated sessions also include the
required device-domain section (tag 9, version 1): shared object metadata and
other processes encoded with the same uncompressed section codec inside the one
outer zlib stream. The high bit of the tag marks a required
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
| Regular VFS files | Mount and relative path, metadata, access/share contract, cursor and append mode; writable backing contents. Duplicate handles share their original description and cursor. |
| RAM object store | File/directory namespace, backing bytes, storage charges, open descriptions, cursors, aliases and mapped file backing identity. |
| Gizmondo devices | BTH1 driver state and lease, VIB1 settings and lease, GPS1 fix status, and CAM1 stream state. GPS and camera reconnect through the configured providers. Pending camera waits retain their remaining delay. |
| MAS1 | Decoder state, submitted data, pending completions and playback cursor. Aliases refer to the same decoder and playback stream. |
| Pocket PC accelerometer | SEN0 and ACC1 stream types and guest handles. Reads resume through the current hardware or simulated input provider. |
| Audio and media | Mixer queues and voice cursors; HSS speaker/channel/clip state; DirectShow COM objects, frame, playhead, settings and pending completion waits. Loaded media reopens through the current VFS and native FFmpeg decoder. Microphone handles reopen the real capture backend and retain guest buffers, pending PCM, callbacks and active state. Unavailable hardware/backend fails restore. |
| GPS serial | COM8 aliases, buffered NMEA bytes, DCB/timeouts/mask and next-poll delay. The current permission-gated GPS service supplies new fixes. |
| Volumes | Guest handle and mount identity; native paths come from the current mounts. |
| Cryptography | Providers, keys, incremental SHA-1 state and ownership graph. No plaintext input history is retained. |
| MSXML | DOM documents/nodes, attributes and COM aliases, with ownership/cycle/reference validation. |
| SDL | Surfaces and guest pixel addresses, event queues and keyboard buffer. Restore neutralizes held keyboard/mouse input and emits matching releases. |
| WinINet | Sessions, connection metadata and unsent requests reopen the current native HTTP client. Native channel transfers retain received response headers, unread bytes and the pending guest send continuation. Completed transfers retain their terminal result; active transfers restore as connection aborted (12030) after the saved bytes. Missing response headers return the abort immediately. Restore never resends the old request. |
| Bluetooth Classic | Registered COM ports, open aliases, receive queues, sockets, inquiry progress, partial-write offsets and timeout settings. Restore retains received data then reports transport loss (normally 10054); pending discovery is cancelled (995). It does not reconnect endpoints, restart discovery or replay outbound writes. New operations use the current permission-gated native service. |
| zlib.dll HLE streams | Decoder state, bounded sliding output window, partial header/dictionary negotiation, checksum, terminal status and handle allocator. Restore resumes the stream at its saved input/output position without replaying consumed input. |
| CEDB | Open Contacts Database cursors, allocator and database contents. Persistent backing is queued into the common file transaction using the current launch path. Closed persistent databases are not rewound. |

Unsupported live resources cause capture to fail with a diagnostic. HTTP and
Bluetooth providers must explicitly support non-consuming receive capture;
unknown providers still reject capture. Desktop native HTTP channels and Windows
Bluetooth Classic implement it. External services and radio peers cannot be
rewound: their captured endpoints restore as disconnected, rather than pretending
the old connection is still live. Physical providers are reacquired only through
their normal permission and availability checks.

When adding a handle type, check both the guest handle table and the backing
object. Recreate the backing object through its subsystem, reconnect aliases to
the same instance and preserve the guest-visible handle values. Test continued
use and closure after a cold restore, including two handles for the same object
when the API supports duplication.
[Handle table](../crates/pocket-kernel/src/handle_snapshot.rs) ·
[VFS resources](../crates/pocket-kernel/src/vfs_snapshot.rs) ·
[Scheduler](../crates/pocket-kernel/src/schedule_snapshot.rs) ·
[Kernel orchestration](../crates/pocket-kernel/src/kernel_snapshot.rs)

## Section objects and mapped views

A section can stay alive through a mapped view after the guest closes its
section handle. PocketHLE captures each section of the saved process once and
records its handle aliases, name and views. Views retain their exact guest
address, file offset, length and protection. If a view alone retains the
original file description, the VFS encodes a private description and releases
its temporary handle after reopening the backing.

ARM capture checks that the kernel accounts for each shared CPU region. The
kernel records the backing bytes and host resource metadata in the
WinCE section. Restore reopens the original mounted file with its saved share
contract or recreates an anonymous allocation. Each restored view and ordinary
file access share that backing. The restore checks the path and size of mapped
files and checks the contents of read-only sections. A successful restore
rewinds the captured byte range of writable mapped files on disk. PocketHLE
copies these bytes after validating and rebuilding the rest of the session.
The capture budget for section backing bytes is 256 MiB.

RAM-backed mappings use the same section/view reconstruction. The object-store
snapshot preserves backing identity, including private descriptions retained
only by a view. Restored reads through an ordinary handle and through either
mapped alias refer to the same bytes. Storage charges are reconstructed once
for each RAM file, with pathname charges and directory charges checked against
the configured storage partition. Coordinated restore reconstructs shared
sections and RAM backings once, then maps them into each restored CPU.

Ordinary mounted files open for writing or appending retain their captured
contents, cursor and append mode. Independent readers of that backing see the
restored contents too. Restore first validates/reconstructs the session, then
opens every destination and prepares rollback copies before changing a file.
An error while installing the pending file writes restores those copies. The
original mounted path must still exist. This is an in-process rollback guard,
not a crash-durable transaction across several host files. Read-only files not
covered by a writable captured description retain size and timestamp checks.
Files closed at capture are not rolled back; capturing open descriptions is not
a snapshot of the complete `saves` directory. Each ordinary writable file is
limited to 256 MiB.

Test alias visibility, read-only protection, view lifetime after closing handles,
append-after-restore behavior, and a rejected restore that leaves host files
intact.
[Mapping snapshots](../crates/pocket-kernel/src/file_mapping_snapshot.rs) ·
[Section objects and views](../crates/pocket-kernel/src/file_mapping.rs) ·
[VFS mapping leases](../crates/pocket-kernel/src/vfs_mapping.rs)

## Coordinated processes

[Checkpoint rendezvous](../crates/pocket-core/src/checkpoint_domain.rs) registers
each executing CPU. A request validates that membership matches the active guest
process domain. Every participant reaches its own kernel boundary before any
callback captures. All callbacks finish before any CPU resumes. Callbacks run
without the WinCE execution gate: holding that gate while waiting would prevent
other CPUs from completing their current API. Failure cancels the capture;
missing participants time out; a process starting or leaving cancels the request.
One common guest/native clock epoch defines all captured timer delays.

Each owning host thread captures and restores its own thread-local GLES/media
runtime. The device section stores the handle namespace, waitable objects,
transfers, process relationships and genuinely shared MAS transports once.
Process-local DirectShow/mixer voices stay private even if guest addresses match.
Per-process CPU/kernel/graphics sections use the existing codec rather than an
independent second snapshot format.

[Device reconstruction](../crates/pocket-core/src/device_snapshot.rs) first checks
all executable fingerprints and current mounts. A shared VFS/section cache
reconstructs description identity, cursors, RAM charges, orphan backings and
mapped aliases. The common handle graph then binds to those native objects.
After every process adopts the domain, a single file transaction applies pending
writes, mapped bytes are installed and all continuations can resume. Temporary
reconstruction references are released before commit so they cannot keep closed
files or section allocations alive for the remainder of the game. Startup or
resource reconstruction failure cancels peers; no peer resumes partially restored.

The desktop runner respawns saved concurrent processes using their saved guest
module paths, resolved through current application mounts. No CreateProcess call
is synthesized. Foreground process identity is restored independently of the
root process used to own host child jobs. An already exited launcher does not
need to execute again. A synchronous SD-card caller stays on its owning host
thread at the real launch continuation while the child runs on its own thread.
Its GLES/media runtime is suspended there and temporarily reinstalled only for
coordinated capture. DirectShow clocks remain frozen during that wait; restored
DirectShow clocks start lazily on first service, after the device commit barrier.
The same guard resuspends resources even when capture unwinds. Parent input is
neutralized with matching releases when it resumes.

The WinCE section records the waiting child identity. Validation rejects missing
or exited targets, competing pending launches and cycles. Cold restore parks the
caller before executing any ARM instruction. Child completion is published after
CPU/kernel teardown; the caller then receives the exit result and resumes its
original code, including a later SDCreateProcess with a command line. The former
isolated foreground-child snapshot format/path has been removed. No game
executable is patched and no launch is replayed to reconstruct the saved state.
A process still waiting for startup acknowledgement cannot join a capture and the
request is rejected rather than producing an incomplete archive. User stop is
shared across process jobs; a peer emulation failure cancels the group and keeps
its diagnostic distinct from user cancellation.

## Graphics and performance

Fixed-function EGL restores its context and resources through the fixed-function
snapshot path. With a desktop OpenGL host context, GLES 2.0 captures live resources
and context state at the checkpoint boundary: shader sources and linked executable
sources, uniforms and attribute locations, buffer storage, texture faces and
mipmap levels, compressed texture bytes, renderbuffer contents, framebuffer
attachments, window color/depth/stencil, bindings and draw state. Resource data is
read back only for an explicit checkpoint. Ordinary draws no longer accumulate
a lifetime replay journal on this path. Rebuilding resources preserves guest
object names while allocating fresh native names in the current share group.
Deleted textures/renderbuffers retained by an unbound FBO keep a private FBO
reference to the same GPU image. This does not copy or read pixels during play.
Capture reads that image only at F5; restore recreates its private backing and
releases its native name after all FBO references are installed. Attachment
identity distinguishes old and reused names, including two FBOs sharing a deleted
image. Detachment/deleting the last FBO releases the private keeper. Texture,
renderbuffer and FBO names created implicitly by binding are also enumerated.

Compiled shader sources and each successfully linked program's shader sources
are retained separately from current attachments. Detaching/deleting a shader or
changing its source after linking therefore does not destroy the saved executable.
Native restore resolves each active uniform element by name and preserves its
saved guest location through a per-program guest/native bijection when the new
linker assigns different locations. Scalar, vector, matrix and array updates,
`glGetUniformLocation` and both value queries use that correspondence. Repeated
capture saves guest locations again, not the current host numbers. An invalid
guest location cannot accidentally refer to another valid native uniform.
A successful guest relink discards the old correspondence; a failed link keeps
it for the executable still installed in the context. Capturing an active
program whose latest link failed remains explicitly unsupported. Delete-pending
current programs retain their table until GL releases them; final release removes
it. No shader source or guest memory is rewritten to force linker assignments.

Identity-linked programs need no location table. Ordinary sessions add no GL
queries/readbacks for this feature; a relocated session refreshes its cached
current program only at `glUseProgram`, including failed calls. Drawing and
uniform storage stay on the native renderer. This removes one linker-identity
restriction, not the other configuration, API and native-driver compatibility
requirements. Use the same graphics settings and emulated device configuration.

The direct resource path requires desktop OpenGL queries and depth/stencil
transfers. Owned Desktop GLES2 contexts now select the same native API as the UI:
desktop OpenGL on Linux/Windows and OpenGL ES on Android. A missing share source
therefore no longer forces Linux through the lifetime journal. If a Linux host
cannot create desktop OpenGL, initialization can still use its existing ES path.
That fallback, or a Desktop context explicitly shared with an OpenGL ES source,
retains the bounded journal fallback:
96 MiB for unique calls and chronological indices, with exact call replay.
Android does not record that journal and explicitly rejects programmable capture;
it must never return a successful but empty graphics checkpoint. Desktop controls
do not imply Android snapshot integration. Unsupported resource states fail
explicitly and keep the previous slot.

Large ARM, mapped-file, RAM-store and native GLES2 byte arrays use `serde_bytes`
with bincode's length-prefixed byte representation. This requests bulk writes
and reads through the buffered zlib stream instead of visiting each byte.
The session section is version 2, WinCE section version 8, media section version 2
and programmable EGL section version 5. Development snapshots using earlier
section versions must be recreated.

[Fixed-function EGL](../crates/pocket-winceapi/src/gles/snapshot.rs) ·
[GLES 2.0 resource checkpoint](../crates/pocket-winceapi/src/gles2/renderer/checkpoint.rs) ·
[GLES 2.0 journal fallback](../crates/pocket-winceapi/src/gles2/renderer/journal.rs) ·
[GLES 2.0 object names](../crates/pocket-winceapi/src/gles2/renderer/names.rs) ·
[RAM object store](../crates/pocket-kernel/src/vfs_ram_snapshot.rs)

Profile capture, compression and write, read and decode, ARM/WinCE restore,
graphics reconstruction and first published frame separately. A Linux Mesa
development run over saved GLBenchmark 2.0, Street Duel and HTC Teeter sessions
measured approximately 0.9–1.0 seconds saved in read and decode with block reads.
The comparison used the same zlib files, with five measured restores per
variant after warm-up. Graphics replay dominated the sampled GLBenchmark restore.


## Live decompressor checkpoints

Only guest HLE `zlib.dll` streams use the dedicated miniz_oxide 0.8.9 core with
its upstream Serde state representation. Archive imports, snapshot zlib encoding,
PNG/image libraries and other flate2 users keep the workspace zlib-rs backend.
No native pointers, compressed-input history or lifetime output archive is saved.
The decoder retains its bit/Huffman/checksum state and at most 64 KiB of sliding
output history. Its caller-visible output boundary is exact: it does not stage
future PNG rows in an internal 32 KiB output queue or consume the zlib trailer
before those rows have been returned. Window compaction is batched, not repeated
for every small row. The saved allocator preserves live handles and avoids
reusing closed IDs merely because a restore occurred.

An undersized `Z_FINISH` can be retried after restore. Reset, terminal checksum
errors and the `Z_NEED_DICT` negotiation state survive too; this does not add
an `inflateSetDictionary` API. Guest output/state permissions are checked before
decoder progress or destruction, so an invalid output cannot silently consume
input. Dependency changes which alter the serialized decoder layout require a
WinCE section version change. No compatibility reader for development version 6
is retained.

A supplied native `zlib.dll` remains native ARM code, as described in
[Graphics](GPU.md#native-image-decoder-dependencies-and-zlib-streams). Its state
is already part of ARM memory. Do not remove or replace game DLLs to manufacture
HLE coverage. Native-game validation and synthetic HLE continuation tests are
distinct evidence.

## Idle and terminal resource behavior

The process rendezvous keeps an atomic idle flag. With no capture pending,
ordinary frame/input polls skip the domain mutex. Once requested, the locked
membership and rendezvous state remain authoritative, including cancellation
and timeout. This does not make capture itself asynchronous: native resource
capture still happens at the coordinated boundary; encoding/writing follows on
the existing background worker.

HTTP responses are received data, not requests to replay. A channel checkpoint
copies its bounded unread suffix and response head under the producer mailbox
lock without consuming data, cancelling the live source or waiting for the
remote server. A completed result stays terminal; an active result becomes
connection aborted in the checkpoint only. Later source callbacks cannot change
that saved result. Reading the restored suffix yields its recorded EOF/error;
a pending `HttpSendRequest` continuation finishes without submitting another POST.
The current native client remains available for new requests.

Windows Bluetooth capture uses nonblocking FIONREAD and MSG_PEEK, never recv
without peek or an implicit connect/accept. Each port/socket receive checkpoint
is limited to 64 KiB; process Bluetooth state is limited to 64 MiB and validates
handle namespaces, counts and partial-write offsets. Shared COM aliases use one
immutable receive boundary per checkpoint epoch and one restored port object.
The source cache and restored identity cache hold weak references, not a lifetime
history. Reads drain saved data before reporting the saved transport error;
writes fail without contacting the old peer. Inquiry results already received
remain available, while an in-flight inquiry becomes cancelled.

External server/radio state is not rolled back, and uninterrupted network sessions
are not promised. See the [phase 5 validation note](testing/SNAPSHOT-PHASE5-2026-11-18.md)
for exercised configurations and platform limits.
