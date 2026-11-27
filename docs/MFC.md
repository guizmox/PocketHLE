# Windows CE 3 ARM MFC bridge

PocketHLE implements the supported `mfcce300.dll` surface in
`crates/pocket-winceapi/src/mfc/`. Desktop and Android use this same bridge,
guest heap, VFS, USER, GDI, scheduler and snapshot implementation. Application
objects remain ARM guest objects; the bridge does not replace their methods
with host objects, patch game files or create a second filesystem.

## Runtime selection and ABI

The supported ABI is Unicode Windows CE 3 ARM retail MFC. Export tables in the
owning Rust modules supply both handler registration and ordinal-name lookup.
The debug DLL has a different ordinal map and must not reuse the retail map.
No Microsoft runtime, SDK or game binary is distributed with the implementation.

An already loaded native runtime retains ownership. Otherwise the existing
runtime search rules can select a supplied compatible DLL before HLE objects
are initialized. Once the HLE module-state root exists, it fixes ownership:
subsequent API calls do not probe missing runtime directories or switch existing
objects to a DLL that appeared later. Native and HLE CString/allocator state
must not be mixed. Unsupported HLE calls report an emulation diagnostic; they
do not silently fall back to a native DLL or return fabricated success.

`mfc.rs` assembles the export tables. `data_imports.rs` distinguishes function
imports from static runtime-class and message-map data imports. Supported data
imports receive guest data addresses after native module attachment. Unknown
data exports fail explicitly rather than presenting executable thunks as data.

`classes.rs` owns the declarative class hierarchy: native size, base getter,
schema, optional factory and virtual slot ordering. Metadata has module/process
lifetime and is rooted in the existing dynamic-export map. Constructors and
factories share this registry. Declaring a virtual slot does not implement it:
unported operations remain explicit errors.

## Ownership and memory

| Resource | Owner and release contract |
| --- | --- |
| Application object storage | Caller, unless allocated by a factory/operator new. Ordinary destruction preserves caller storage; scalar deleting destruction frees it when requested. Unsupported vector destruction fails before mutation. |
| Class metadata and vtables | Shared module/process root. Object destruction never frees metadata or an application's derived vtable. |
| CString storage | Guest reference-counted allocation. Mutations detach shared storage; the last reference frees the allocation. |
| Array storage | Guest array object. Resizing validates bounds, initializes new cells and releases removed owned elements. |
| List/map blocks | Guest CPlex chains. Containers release blocks and owned keys, while borrowed object pointers remain caller-owned. |
| HWND/HMENU/HDC/GDI objects | Existing WinCE services and permanent/temporary MFC wrappers. Temporary wrappers borrow handles; explicit owning destructors use the real service. |
| CFile handles | Existing VFS. Close-on-delete controls ownership; ordinary file opens never imply append mode. |
| Callback scratch | Exact guest continuation/thread/stack frame. Released on completion or a supported nonlocal-return cleanup path. |

A CString is one guest pointer. Its allocation has reference count, UTF-16
length and capacity fields before the text. Copies share unlocked storage;
mutable access detaches it. Replacement allocates before releasing the previous
value. Counted operations preserve embedded NUL units; produced text has an
explicit terminator. Bounded buffer release never scans past capacity.

`arrays.rs` provides the common checked growth and relocation engine for byte,
word, DWORD, object-pointer and CString arrays. String-pointer relocation
transfers references; shrinking destroys removed strings, and regrowth creates
empty strings. Integer cells use their actual stride and are zero-initialized.
Insertion/removal validates ranges and overlapping moves before mutation.

CByteArray uses this same engine and the class registry, with no private
per-instance vtable allocation or separate serializer. Its 20-byte layout is:

| Offset | Field |
| --- | --- |
| +0 | Borrowed vtable pointer |
| +4 | Owned byte buffer |
| +8 | Logical element count |
| +12 | Allocated element capacity |
| +16 | Growth increment |

Its five virtual slots are GetRuntimeClass, deleting destructor, Serialize,
AssertValid and Dump. The shared CRuntimeClass has native size 20, schema 0,
factory and callable CObject base getter. Named class-data imports and retail
ordinal 26 resolve to that same descriptor. Ordinary destruction releases only
the byte buffer and returns to the CObject base identity. A derived guest vtable
is never interpreted as a heap allocation owned by the array.

<a id="window-procedure-exception-boundary-phase-17"></a>

## Continuations and exceptions

Synchronous guest callbacks use `guest_callbacks.rs`; `mfc/callbacks.rs` is the
MFC adapter. Guest heap frames retain the original registers, stack, phase and
owned scratch. Return roots are keyed by thread, thunk and stack depth, so nested
calls do not consume an enclosing callback's frame. Stack placement uses the
CPU's actual stack-argument offset. Host closures and physical pointers are not
part of this state.

Saved-region guards retain caller structures such as the complete last-sent MSG
through nested callbacks and replacement exceptions. USER/GDI notifications,
window creation and destruction, DLL attachment and MFC methods reuse their
existing continuation owners rather than allocating parallel host registries.

CSyncObject and CEvent wrap real WinCE synchronization handles and waits.
The native CE3 CEvent::Unlock contract returns TRUE without signaling the event;
SetEvent/ResetEvent own those transitions. This native behavior must not be
replaced with a fabricated wait result or an implicit signal.

The CE3 macro exception path uses real guest `AFX_EXCEPTION_LINK` records,
setjmp state, AfxThrow/rethrow, cleanup links and virtual exception deletion.
DDV failure reaches this path through CDataExchange::Fail. The shared window
procedure boundary calls the current CWinThread's actual
ProcessWndProcException virtual and restores the prior thread message state.
Only younger current-thread continuations are retired during a nonlocal return.
Temporary wrappers, notification state and supported resource transactions are
cleaned up by their owners before frames are freed.

This is not general MSVC C++ exception or OS SEH unwinding. Unsupported
transactional exception boundaries fail before abandoning a live owner. The
retail CE3 `.pdata` format must not be decoded as modern ARMNT exception data.

## Framework and routing

| Area | Implementation |
| --- | --- |
| Application/module/thread state | `application.rs`, `worker_threads.rs`, `resources.rs`, `recent_files.rs` |
| Runtime hierarchy and handles | `classes.rs`, `hierarchy.rs`, `class_queries.rs`, `handle_maps.rs` |
| Window creation and destruction | `window_creation.rs`, `window_handles.rs`, `window_lifetime.rs`, `safe_owner.rs` |
| Message and command dispatch | `message_maps.rs`, `window_messages.rs`, `command_routing.rs`, `command_ui.rs`, `setting_change.rs` |
| Pretranslation, activation and idle | `pretranslation.rs`, `activation.rs`, `idle.rs`, `message_pump.rs` |
| Dialogs and data exchange | `dialog_creation.rs`, `dialog_modal.rs`, `dialog_records.rs`, `data_exchange*.rs`, `data_validation.rs` |
| Documents/views and editors | `document_*.rs`, `documents.rs`, `views.rs`, `edit_view.rs`, `doc_list*.rs` |
| Splitters and scroll adapters | `splitter_*.rs`, `window_scroll.rs` |
| Files and serialization | `file.rs`, `mirror_file.rs`, `archive*.rs`, `arrays.rs`, `maps.rs`, `lists.rs`, `typed_lists.rs` |
| Graphics wrappers and controls | `gdi.rs`, `device_contexts.rs`, `dc_drawing.rs`, `control_bars.rs`, `common_control_lifetime.rs` |

CWnd uses the 72-byte CE layout: +0x20 holds the HWND and +0x44 belongs to
ShowDoneButton registration, not tooltip storage. The top-level frame tracks
that registration at +0xc8. CFrameWnd uses 232 bytes, including its embedded
CPtrList/CString and module-thread list membership. Construction writes defined
base fields without zeroing derived storage. Window association happens before
PreSubclassWindow and WM_CREATE. Failed creation invokes actual PostNcDestroy;
the bridge does not read an object after a callback allowed to delete it.

Message maps remain guest data with real callable base getters and handlers.
The scalar CE3 signature interpreter covers signatures 1 through 49, preserving
argument narrowing and return conventions; registered messages, command ranges,
notifications and handler-info queries retain their separate contracts. Unknown
and OLE-specific signatures report errors. A handler-info query locates a real
handler without executing its command. Command-UI changes operate on existing
MENU/HWND state; an explicit update-handler veto is honored.

Pretranslation walks the actual permanent-window tree and thread/frame/view
routes. Worker startup uses WinCE thread/event handles and a suspended binding
handshake. Worker return and AfxEndThread share thread-local cleanup and actual
virtual auto-delete policy. Borrowed aliases never replace another thread's
permanent HWND association.

Resource lookup searches the primary, extension, language and system scopes in
framework order with strict module identity. Resource-module preference is not
a second resource store. Profile settings and recent-file lists use the existing
registry and CString owners. Recent-file entries, menu updates and virtual
teardown preserve their actual reference lifetimes.

## Dialogs and controls

Modeless and MFC modal dialogs share the WinCE template parser, control owner,
subclass association and initialization callbacks. CDialog retains its 124-byte
CE layout and borrowed template/parent pointers. Populated DLGINIT processing
uses checked resource bounds, temporary UTF-16 copies and real child messages.
`dialog_children.rs` and the MFC preparation adapter create registered custom
classes in templates through the ordinary CreateWindowExW path before
WM_INITDIALOG. The MFC parent is associated first, so its creation hook cannot
be consumed by an unrelated child. Their real creation, size, painting and
destruction procedures remain authoritative; they are not replaced by
static controls. Class/title/creation bytes have one guest-frame-owned temporary
allocation, released on return or nonlocal unwind. Modeless and USER-modal
dialogs share this initializer. Unregistered unknown classes are not synthesized.

CWnd::GetDlgItem queries the real parent HWND and returns the existing permanent
or temporary borrowed CWnd wrapper. A missing child returns NULL. CWnd text
reads/writes share USER messages and the same CString buffer owner as DDX_Text.
CListBox::GetText(CString&) reads the requested list row through
LB_GETTEXTLEN/LB_GETTEXT. Supported control-site/container branches dispatch the
actual guest virtual methods rather than fabricating OLE objects.

CDialog/CFrameWnd WM_SETTINGCHANGE handlers use their embedded SHACTIVATEINFO
and the thread's original MSG. SPI_SETSIPINFO updates the live SIP state and
resizes the window to the intersection of SIP visible desktop and system work
area. The CWnd base route broadcasts the original message to descendants and
calls Default. Splitter display changes recalculate layout only for a live,
visible HWND. USER/GDI read live metrics; the bridge adds no separate metric cache.

The parent is disabled and restored through actual EnableWindow callbacks.
Synchronous SendMessageW and queued DispatchMessageW resolve the same per-HWND
procedure, including native control subclasses. A synthetic owner without an
application procedure does not inherit another window's dispatcher. The legacy
primary-window fallback is retained only while no per-HWND procedures exist.
Tab traversal uses the same child/sibling walk as GetNextDlgTabItem, respecting
visibility, disabled ancestors and WS_EX_CONTROLPARENT.

DoModal invokes real PumpMessage, IsIdleMessage and ContinueModal virtuals.
EndModalLoop posts WM_NULL to wake the queue. OnOK validates through UpdateData;
it does not invent a successful validation result. DDX/DDV use the same HWND,
text, selection, range and checked-state owners as ordinary WinCE controls.

USER's DialogBoxIndirectParamW modal pump also routes shell command-bar input
through the shared shell-menu projection before client control hit testing.
Commands retain their actual owner, ID and null menu lParam. Only messages in
the modal window tree are dispatched; the application's underlying queue is
preserved. Default unhandled OK/Cancel commands follow the dialog completion
path, while guest EndDialog results remain authoritative. A handled TRUE
DialogProc result prevents default completion, and completed callbacks are
processed before another queued input can replace their return value.

Button, static, EDIT, list/combo, progress, slider and tab adapters reuse the
shared USER/GDI renderer. Native child subclasses receive original pointer
messages before default mutation and notifications. WM_CTLCOLOR preserves the
parent's real brush and text attributes. Trackbars share a system-face raised
thumb, recessed channel, separate focus indication and horizontal/vertical tick
styles; TBS_NOTHUMB suppresses the thumb. Native control colors are not replaced
with a game-specific skin.

UpdateWindow synchronously paints an invalid visible window. It does not enter
hidden windows or children of hidden parents; their invalid/erase state remains
available for a later show. Explicit SendMessage painting is not filtered by
this UpdateWindow policy. This uses the existing USER visibility tree and
continuation owner, without a separate snapshot field.

EDIT layout, painting, selection, caret movement, hit testing and line messages
share the selected-font layout engine. Viewport, undo and modified state belong
to the control. Clipboard transfers retain normal global-memory ownership;
physical clipboard pointers and host paths are never serialized.

## Frames, documents and graphics

CE command bars use the verified 124/164/224-byte
CControlBar/CToolBar/CCeCommandBar layouts and actual shell windows. They contribute zero docked client area as on CE3. Normal
and shell toolbars share ToolbarState and its message interpreter; shell MENU
records provide the text/submenu projection. Resource-loaded bitmap strips are
owned by the bar; caller HBITMAPs stay borrowed. CToolBar's bitmap field is an
HBITMAP, not an image-list handle. HWND teardown restores subclass procedures,
releases owned resources and removes frame membership.

Static/dynamic splitter layout keeps ideal, minimum and current pane sizes,
then delegates real geometry, focus, capture and scrolling to USER. Tracking
uses reversible GDI painting and guest fields. Unchanged geometry does not
manufacture size callbacks. Client/paint DC wrappers pair acquisition and release;
GDI selections retain the real DC state and borrowed/owned distinction.

The document/view implementation covers SDI template selection, real document
open/save, view association, lifecycle and CE document-list paths. Document-list
metadata uses guest VFS paths and belongs to its actual HWND. CEditView search
uses ordinary modeless dialogs and CString storage. Template failure preserves
a borrowed background frame/view, and destruction removes manager registration.
Desktop CMDI/docking/OLE families absent from this CE3 ABI are not synthesized.

## VFS and archives

CFile is a 16-byte guest object with a borrowed class vtable, VFS handle,
close-on-delete flag and CString filename. Its 20-slot table retains the native
virtual order. Open/share/create, bounded read/write, seek, truncate, flush,
rename and remove use real VFS operations. SetLength leaves the cursor at the
new length. Scoped wrappers do not introduce a host path resolver or file cache.
The filename/mode constructor reuses base Open and owns the resulting handle.
Failure raises the ordinary CFileException with its OS cause and guest path,
after retiring the initialized filename and any acquired handle; it does not
leave a partly constructed file owner behind.

CAB installation preserves explicitly declared empty application directories
from structured MSCE DIRS and XML FileOperation metadata. Import materializes
safe application-relative directories; launch repairs missing directories through
the mounted VFS/save overlay, preserving existing files and installed sources.
An unrelated sibling or system directory is not flattened into the application.
This does not change CFile/CreateFile parent-directory requirements: undeclared
missing paths still fail with the normal WinCE error.

CArchive retains its 68-byte CE layout, buffer ownership and cursor fields in
guest memory, including cursor changes made by compiled inline operators.
Storing flushes buffered data; loading rewinds unread buffered bytes at teardown.
Caller buffers remain borrowed. Reads/writes dispatch the actual CFile virtuals,
including guest subclass overrides. Count, string, array and object-graph codecs
share this stream and its class/object identity maps. They do not maintain a
second host representation of the application's archive.

CMirrorFile temporary names come from the VFS allocator and remain tracked even
when their handle has closed. Failure cleanup retires unpublished temporary
files. Restore validates mount/parent ownership and retains rollback through the
complete session transaction; ordinary external-file validation is not relaxed
to recreate an arbitrary file.

## Snapshot integration and boundaries

Guest object bytes, heap allocations, TRY links, module roots, class metadata
and continuations use existing process memory/export capture. USER/GDI/VFS
resources use their existing independently versioned WinCE records. No MFC
snapshot file, frontend-specific state branch or duplicate allocator is added.
Host scheduling/lookup hints are transient and reset on restore.

EDIT extensions, indeterminate check states, clipboard ownership, document-list
metadata, shell toolbars, window-creation scratch and mirror-file names are
captured by their USER/GDI/VFS owners. The
[WinCE record registry](SNAPSHOT-WINCE-FORMAT.md#permanent-record-registry) owns
the record IDs, current payload versions and required/optional flags; those
values are not maintained in a second MFC-specific schema.
Current-thread identity, saved stack/return frames and borrowed versus owned
resources must survive restoration together. A code change to host painting or
routing alone does not require a serialized schema change.

The supported surface is substantial but partial. OLE containers/sites and
general compiler/OS exception unwinding remain outside it. Unsupported imports,
virtual operations and resource transactions produce explicit diagnostics rather
than successful no-ops. The authoritative API inventory is the registered source
export tables, not a claim of full Microsoft MFC compatibility.
