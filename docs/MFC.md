# Windows CE 3 ARM MFC bridge

The `mfcce300.dll` bridge is shared by Desktop and Android. It operates on guest
objects and the existing kernel heap, VFS and GDI namespaces. It does not patch
applications or synthesize game resources. A user-supplied native runtime still
uses the existing whole-runtime preference; native and HLE CString/allocator
state must not be mixed.

## ABI evidence and supported operations

The retail and debug ARM DLLs supplied for analysis identify the Unicode CE3
layout. Debug export names establish method identity; retail export addresses,
class descriptors and call sites establish the shipped ordinal map. Retail and
debug ordinals differ: do not reuse this map for `mfcce300d.dll`. No runtime DLL
or SDK binary is distributed with the implementation.

Additional retail exports are registered once in the owning module's `EXPORTS`
table; the dispatcher and ordinal-name resolver reuse those same tables.

| Ordinal | Operation |
| --- | --- |
| 97 / 242 | CFile constructor / destructor |
| 175 | CString copy constructor |
| 324 / 361 / 366 | Assign wide string / concatenate / append |
| 800 / 1040 | CGdiObject Attach / DeleteObject |
| 854 / 2063 / 2170 | CFile Close / Open / Rename |
| 1173 | CString Format using the existing wide printf formatter |
| 1229 / 1233 / 2167 | GetBuffer / GetBufferSetLength / ReleaseBuffer |

Existing CString, CByteArray and global new/delete exports retain their own
contracts. This is a partial MFC surface, not a claim of full MFC support.
CArchive serialization, exception throwing and unimplemented runtime-class
exports remain outside this bridge. Failed CFile I/O reports an explicit handler
error rather than pretending that data was read or written.

## Object ownership

A CString is one guest pointer. Its data is preceded by three 32-bit fields:
reference count, UTF-16 length and capacity. Copy construction shares unlocked
storage; buffer acquisition detaches shared storage. Replacement allocates before
releasing the old value. Mutable-buffer scans stop at capacity and every produced
string has an explicit UTF-16 terminator. Object slots are checked before
allocation or replacement; the last reference releases the heap block.

A CFile base object occupies 16 bytes: vtable at +0, VFS handle at +4,
close-on-delete flag at +8 and CString filename at +12. Its 20-slot vtable uses
the retail ordering, including the five inherited CObject slots. The six-word
CRuntimeClass base entry is a callback, not a direct pointer. One process-owned
class-data allocation is rooted in the existing dynamic-export map; snapshot
heap/export capture preserves it without a separate host cache or new MFC file.

Open maps MFC access/share/create modes to real VFS operations. The VFS's append
argument is **false** for ordinary CFile opens; it is not the MFC inheritance
flag. Reads begin at offset zero. Read/write transfers check guest ranges and
use bounded chunks. SetLength follows the native seek-and-truncate behavior and
leaves the cursor at the requested length. Close/destruction release owned file
and CString resources. CGdiObject operations attach actual GDI handles, detach
before deletion, and reuse the normal DeleteObject implementation and selected
object protections.

## Validation

Tests cover CString sharing/detachment, termination and allocation release;
CFile initial cursor, real file reads, length query, seek/truncate and teardown;
and MFC attachment/deletion of real GDI objects. The application startup evidence
and remaining limitations are maintained in
[PocketPC startup validation](testing/POCKETPC-STARTUP-2026-11-19.md).
