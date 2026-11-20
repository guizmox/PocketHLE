# Snapshot internals

Desktop and Android checkpoint native ARM WinCE sessions, including concurrently
running processes, and supported Compact Framework interpreter sessions. Gizmondo and Pocket PC share the serializer and restore path. See
[the frontend guide](FRONTENDS.md#experimental-snapshots) for commands
and slot locations.

## Capture and write

Both frontends queue the request into `pocket-session` and capture at a kernel checkpoint on
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
[Shared session coordinator](../crates/pocket-session/src/snapshot.rs) ·
[Desktop adapter](../frontends/pocket-desktop/src/runner/snapshots.rs) ·
[Android adapter](../frontends/pocket-android-jni/src/runner_snapshots.rs) ·
[Slot storage](../crates/pocket-library/src/snapshot_slot.rs)

## Disk layout

| Layer | Contents |
| --- | --- |
| Slot header | Eight-byte magic `PHSNAP01` and a little-endian 64-bit UTC timestamp in Unix milliseconds. |
| Backend header | `PHSECT01` for ARM sections or `PHCFSE01` for managed version 1; followed by one zlib stream. |
| Native stream | Little-endian 16-bit section count, then each section's 16-bit tag, 16-bit version, 64-bit byte length and owner-defined payload. WinCE uses the record envelope below; typed record and other section payloads use fixed-integer bincode. |
| Managed stream | Little-endian 64-bit payload length and a versioned fixed-integer bincode document, using the same bounded codec, zlib and buffered I/O implementation. |

Session identity and continuation, ARM execution, WinCE kernel (including mapped sections), fixed-function
EGL and media are always present. Programmable EGL, HSS and pending input
releases are emitted when applicable. Coordinated sessions also include the
required device-domain section (tag 9, version 1): shared object metadata and
other processes encoded with the same uncompressed section codec inside the one
outer zlib stream. The high bit of the tag marks a required
section. The reader checks for duplicate tags, bounds each section read and
enforces a cumulative 512 MiB budget for section headers and payloads. It can
skip unknown optional sections and rejects unknown required sections.
[Native sections](../crates/pocket-core/src/snapshot.rs) ·
[Shared format/compression](../crates/pocket-snapshot/src/lib.rs) ·
[Managed state](../crates/pocket-netcf/src/runtime_snapshot.rs)

## Independently versioned WinCE records

WinCE container 12 replaces the monolithic section-11 payload with 117 stable,
length-bounded typed records, grouped by domain ID. New optional records use
explicit safe defaults; required data and unsupported versions fail preflight.
An owner can migrate a single record through a scoped decoder without copying
the whole kernel format or adding a PocketPC/Gizmondo branch. Individual nested
payloads still require a version/migration when their representation changes.
The writer keeps only a small length directory; records stream through the same
buffered zlib I/O. Capture/restore resource ownership and validation are shared
and unchanged. This transition requires recreating section-11 development slots;
there is no legacy monolithic reader. Future changes must keep the new reference
fixtures passing or explicitly document why faithful migration is impossible.

[Format, record registry and evolution rules](SNAPSHOT-WINCE-FORMAT.md).

## Verified restore and emulation requirements

Both GUIs use `prepare_async` from the shared archive module. Decode and checksum
verification run off the UI/emulation threads while the old session remains live.
Native reads also validate ARM memory/deferred ranges, registers, hooks,
continuation and thunk indexes without creating a CPU. Managed reads validate
object/free lists, service references, control graphs, images and resource ranges.
Every saved process executable is resolved below current application mounts and
its raw bytes are checked against the captured SHA-256 identity.

The semantic manifest compares the requested device/display policy, host rendering
policy and internal resolution. Native sessions additionally compare processor,
executable heap, EGL/vendor profile and device-identity requirements. CF does not
inherit irrelevant ARM CPU or vendor-driver restrictions. Presentation rotation,
FPS limits, keyboard/controller/grid assignments and physical-provider enablement
are deliberately outside this manifest. Native restore applies the current host
presentation ceiling while preserving explicit guest waits.

`PreparedRestore` is opaque to frontends. It owns the exact decoded archive and
pins the checked target configuration; replacement of the on-disk slot or changes
to launcher settings cannot substitute a different payload/configuration after
verification. Starting another game or leaving cancels a pending Desktop delivery.
An invalid/missing archive never stops the old session. Once preparation succeeds,
the old process graph is joined and the receiving launch reconstructs native
resources on their owning threads. Hardware acquisition, native GPU allocation,
changed dependencies/mounts and other late failures can still fail reconstruction;
preflight is not a guarantee that every external resource will remain available.

[Shared verified archive/slot coordinator](../crates/pocket-session/src/snapshot/archive.rs) ·
[Requirement manifest](../crates/pocket-session/src/compatibility.rs)

## Managed interpreter state

CF capture runs between completed IL callbacks. It retains object IDs and aliases,
free slots, statics and initialized types; forms/controls, images/bitmaps, fonts and
event handlers; timers and collection deadlines; string interning and assembly
resource stream positions; system RNG state, sensors and managed service tables;
XML reader contents and writer/serializer identities; FMOD sample handles and the
shared mixer checkpoint. Host paths, live stack frames, native handles and
assembly pointers are not serialized. Resource slices are bounded offsets into
the fingerprinted assembly. Writer identities are sandboxed names resolved below
the current save root. This does not roll back closed persistent files.

Restore parses metadata without executing `Main`, constructors or initial events.
It rebuilds the VM/service graph and resumes its saved event clock. GLES contexts
reuse existing `ContextSnapshot`/`SurfaceSnapshot`, including native GPU surface
transfers. The framebuffer serializer reuses the kernel's `PixelSnapshot`.
A software fallback is identified by the saved surface, not by the initial GPU
preference; a captured native surface must reconstruct a real native renderer.
Audio uses the existing shared `AudioSnapshot`. Held host inputs are neutralized
because the host input accumulator is freshly created. Sensors resample through
the current configured provider on the next callback.

Normal stop retains CF close handlers. Snapshot replacement discards the old
instance without executing those handlers, preventing shutdown side effects from
being mistaken for part of the restored session. Android reattaches the original
JNI input channel after every cold replacement. All managed Android applications
use this interpreter and snapshot path.

The managed and native payload versions evolve separately. Legacy development
containers are not retained; new WinCE record payloads can use scoped migrations. Current native session identity is section version 3,
WinCE 12, fixed EGL 2, programmable EGL 6 and media 2. Managed payload version is 1.
Recreate development slots after this change.

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
Executable virtual-TLB promotions use ARMv5/v6's 1 KiB tiny-page granularity,
independently of the 4 KiB allocation and image pages. Snapshot validation must
accept these sub-page addresses, while retaining sorted, unique, aligned and
32-bit-range checks. Chix exercises this distinction during ordinary execution.
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
unknown providers still reject capture. Desktop native HTTP channels, Windows Bluetooth Classic and Android HTTP,
serial Bluetooth and RFCOMM providers implement it. Android copies the unread
receive suffix under the provider lock without consuming it or cancelling the
source transfer. Its bounded JNI packet is validated before entering the common
WinCE codec. External services and radio peers cannot be
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
snapshot path. On both desktop OpenGL and host OpenGL ES 3 contexts, GLES 2.0 captures live resources
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
it for the executable still installed in the context. Before relinking a current
linked program, the renderer captures its installed interface. A failed native
link retains that interface with its last successful executable. Restore installs
that executable and its uniform values first, then repeats the actual failed link.
Native link status and subsequent draw/error behavior remain native; no query
result or shader is patched to simulate failure. Delete-pending
current programs retain their table until GL releases them; final release removes
it. No shader source or guest memory is rewritten to force linker assignments.

Identity-linked programs need no location table. Ordinary sessions add no GL
queries/readbacks for this feature; a relocated session refreshes its cached
current program only at `glUseProgram`, including failed calls. Drawing and
uniform storage stay on the native renderer. This removes one linker-identity
restriction, not the other configuration, API and native-driver compatibility
requirements. Use the same graphics settings and emulated device configuration.

Desktop and Android share the same resource archive, object reconstruction,
state restoration and guest-name mapping. The host-specific transfer helpers are
confined to the common `pocket-gles` checkpoint transfer module. There is no draw-history journal or
replay fallback. Desktop prefers its UI's native API; ES-only hosts use the same
ES 3 resource path as Android. An ES 2-only host fails explicitly at capture.

ES 3.0 lacks desktop texture/buffer readback queries and portable depth/stencil
ReadPixels. Buffers use read-only mapping at capture. A private shared ES context
samples live color textures into a GPU staging image. It never switches away
from the owner's installed guest executable. Cube faces, mip levels and legacy
alpha/luminance formats are handled through that same transfer. Allocation
metadata records dimensions/internal format only; compressed textures also retain
the current accepted native blocks because ES has no compressed-block readback.
Accepted definitions, subimages and deletion replace/remove those live records;
ordinary draws accumulate no history or pixel copies. Native rejected definitions
preserve the prior metadata and the guest's GL error. Compressed subimages use
their actual format layout (including RGTC, LATC, BPTC and non-square ASTC
blocks), rather than guessing from the byte count. Whole-image uploads replace
opaque blocks directly, including PVRTC, and can repair a previously unsupported
mirror. Accepted partial updates of unknown layouts still fail capture explicitly;
unsupported/permissive compressed mip generation is not represented as a guessed
image. Accepted empty subimage updates leave both the data and its capture
status unchanged. No compressed data is decoded/recompressed by the snapshot path.

Fixed-function surface capture uses the same ES transfer implementation. Even
a programmable-only game has a fixed-function EGL section; leaving the old
fixed-function ES depth rejection in place would still block its whole-session
snapshot. Fixed-function ES capture/restore binds shared renderbuffers in a
private native context so it does not invalidate the fixed renderer's cached
program/texture/vertex state. Saved surfaces retain their RGB565/RGBA format and
the exact depth representatives needed by that host API. Desktop OpenGL keeps
its direct attachment transfer. No normal drawing path creates these helpers.

Depth/stencil is blitted on the GPU into a shared staging image. Depth capture
uses actual GPU EQUAL tests to find a floating-point representative that writes
back to precisely the stored native depth. The temporary color encoding uses
the unused sign bit of non-negative depth values as a match marker, then removes
that marker before saving. Unmatched pixels fail capture; zero-filled staging
memory can never masquerade as verified depth. This avoids assuming that a driver's
normalized depth sampling and conversion round identically. Capture rejects a
value that cannot be verified. Stencil's eight bit planes are tested and packed
by GPU draws. Restore writes depth with a fragment shader and stencil with masked
GPU passes. CPU work validates and packs serialized bytes; it does not rasterize
the scene. All readbacks and these synchronization points occur at an explicit
snapshot, never in the frame loop. Unsupported resources keep the previous slot.

Large ARM, mapped-file, RAM-store and native GLES2 byte arrays use `serde_bytes`
with bincode's length-prefixed byte representation. This requests bulk writes
and reads through the buffered zlib stream instead of visiting each byte.
The current section versions are listed above; the WinCE payload now uses
independently versioned records. The bulk byte representation remains unchanged.

[Fixed-function EGL](../crates/pocket-winceapi/src/gles/snapshot.rs) ·
[GLES 2.0 resource checkpoint](../crates/pocket-winceapi/src/gles2/renderer/checkpoint.rs) ·
[Shared ES GPU transfers](../crates/pocket-gles/src/gpu/checkpoint_transfer.rs) ·
[GLES2 archive adapter](../crates/pocket-winceapi/src/gles2/renderer/checkpoint/es.rs) ·
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
input. Dependency changes which alter the serialized decoder layout require a version
change and scoped conversion for the zlib record (`0x0307`), or an explicit
rejection if faithful conversion is impossible. They no longer require changing
the whole WinCE container.

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

## Frontend commands and diagnostics

`pocket-session` owns process participation/rendezvous, parked launchers, device
restore coordination, child-job completion, background writing, slot reading and
saved-program resolution. UI adapters provide process construction, frame/audio
publication and diagnostic presentation. There is one engine, not one per GUI.
Both adapters release the old native process resources before a cold restore.
The common device commit barrier still completes before guest continuation.

Android keeps its JNI session handle and persistent presentation root while the
worker joins the old process graph and reconstructs the saved graph. Save/Restore
are serialized; the UI polls for a result only while an operation is pending.
A save acknowledgement means the atomic slot write succeeded. A restore
acknowledgement follows the first frame published after restore commit. Both
include the saved slot timestamp. An absent slot leaves the live game untouched.
Save/preflight failures reuse the existing diagnostic dialog, with Continue when
the session remains live; fatal restore failures use the same dialog without it.
Desktop uses that same diagnostic payload and controller-aware window.

Validation procedures and platform limits: [phase 6 record](testing/SNAPSHOT-PHASE6-2026-11-19.md) · [phase 7 record](testing/SNAPSHOT-PHASE7-2026-11-19.md) · [native compressed textures](testing/SNAPSHOT-COMPRESSED-2026-11-19.md).

### Native window paint state

The GDI record in WinCE 12 includes pending client update regions, the message identity
of each suspended DispatchMessage callback, and the update rectangle retained
across BeginPaint's erase callback. A slot captured while a repaint is in flight
therefore keeps the same invalidation and GAPI pacing policy after restore.
The optional native GDI FPS ceiling also keeps transient BeginPaint/EndPaint
host revision anchors. These anchors are excluded from serialized state and
cleared on direct in-memory restore too; a restored partial paint may complete
without its first ceiling check. Guest pixels, invalid regions and callback
continuations remain saved. This host-only bookkeeping does not change section
12 or require new snapshots by itself.
As with earlier development section changes, no legacy kernel-section reader
is retained; recreate development slots written with older kernel versions.
