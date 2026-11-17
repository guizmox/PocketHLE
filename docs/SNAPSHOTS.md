# Desktop snapshot internals

The desktop frontend checkpoints an ARM WinCE session, including concurrently
running processes. Gizmondo and Pocket PC share the serializer and restore path. See
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
| Logical WinINet | Sessions, connection metadata and unsent requests reopen the current native HTTP client and retain guest handle/parent identity. No request is sent during restore. In-flight transfers are refused. |
| CEDB | Open Contacts Database cursors, allocator and database contents. Persistent backing is queued into the common file transaction using the current launch path. Closed persistent databases are not rewound. |

Unsupported live resources cause capture to fail with a diagnostic. Remaining
gaps include active Bluetooth ports/sockets, HTTP transfers and a live
zlib decompressor. Physical hardware and remote services are not serialized;
restore must reacquire the configured provider instead of fabricating data.

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
need to execute again. A synchronous SD-card launch retains the existing isolated
foreground-child capture path; its suspended caller is deliberately outside that
snapshot. A CPU still waiting for startup acknowledgement cannot join a capture
and the request is rejected rather than producing an incomplete archive.

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
Restoration still checks that the host assigns the saved uniform locations.
Use the same driver, graphics settings and emulated device configuration.

The direct resource path requires desktop OpenGL queries and depth/stencil
transfers. Host OpenGL ES contexts retain the previous bounded journal fallback:
96 MiB for unique calls and chronological indices, with exact call replay.
Desktop snapshot controls do not imply Android snapshot integration. Unsupported
resource states fail explicitly and keep the previous slot.

Large ARM, mapped-file, RAM-store and native GLES2 byte arrays use `serde_bytes`
with bincode's length-prefixed byte representation. This requests bulk writes
and reads through the buffered zlib stream instead of visiting each byte.
The WinCE section is version 6, media section version 2 and programmable EGL
section version 5; development snapshots using earlier section versions must be recreated.

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
