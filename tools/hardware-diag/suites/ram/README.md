# PocketHLE guest RAM diagnostic

Run **RAM / paging / TLS / processes** from the single
[HARDWARE DIAG application](../../README.md). It executes original ARM diagnostic
code without a proprietary SDK or commercial assets. The integrated console
returns to the menu after completion; private standalone fixtures may retain
their acknowledgment dialog.

Inspect `RAMTEST.TXT` and the secondary `DLLTEST.TXT`, `DEPTEST.TXT`,
`PROCTEST.TXT` and `ORPHANTEST.TXT` reports. The final process-exit callbacks may
write their PASS after acknowledgment. Run again after restarting PocketHLE and
retain the detailed report plus application log on failure.

Suite-specific version numbers and check counts below describe their named
fixture revisions. Read the current integrated reports for the running build;
these counts are not an assertion that a new build was executed on every host.

The report is created at `\Flash Disk\RAMTEST.TXT` on writable storage. Its host
copy is `flash/ramtest.txt` below the PocketHLE library root. The EXE's SD card
remains read-only. Use the configured library root when locating reports; it may differ from the
Windows Documents default or live inside a Proton prefix.

Version 8 retains the full v7 suite and adds 64 mixed allocation/event/mutex/
thread/DLL cycles with exact RAM/address-space refunds, plus eight consecutive
process launch/exit/handle-close cycles with RAM reclamation after each child.
This measures guest-accounted resources, not native GPU/driver allocations.

Version 7 adds a multiprocess suite: normal/suspended process creation, command
lines and identifiers, explicit duplication, private TLS, remote thread control,
waits and exit codes, a child outliving its parent, and rollback on insufficient
RAM or invalid PROCESS_INFORMATION output. Four RAMTEST checks surround the
suite and verify memory reclamation. Processes have their own CPUs and host
threads; SDCreateProcess retains the existing return-to-launcher behavior.
Automatic handle inheritance is rejected according to the CE contract; use
DuplicateHandle.

Version 6 adds 33 TLS/error checks: 64 slots, exhaustion, invalid parameters,
reset on reallocation, isolation between main and two workers, direct KData
writes, reuse while a worker waits, TLS preservation during DllMain and wait
errors. Successful TlsGetValue clears GetLastError, even for a null value.
Other successful TLS operations preserve the error. Get/Set implement WinCE's
minimal index validation for 0..63, including unallocated slots.

The diagnostic checks thread DllMain notifications, ordering and context, followed
by process detach events in DLLTEST.TXT. It checks native imports by name/ordinal,
shared dependencies, retention by explicit LoadLibrary, cycles, missing dependencies
and exports, and rollback after a rejected attach.

`pageprobe.dll` checks first-use loading of code, initialized data and zero pages,
unique page accounting, modified data, and unload/reload without leaks. The program
warms its own pages before measuring to isolate these operations. It also checks
reserve/commit/decommit/release, allocation reclamation and realloc failures,
thread-local GetLastError, 32 stack cycles, 24 DLL cycles with multiple references,
exports, rejected DllMain, load errors, RAM redistribution and refusal to shrink
the program partition below occupied pages. The initial partition is restored.
Measurements are hexadecimal bytes, except page counts returned by
GetSystemMemoryDivision.

`GZRT999999` selects the Gizmondo profile through normal title detection. The kernel
has no diagnostic-specific behavior. The program targets PocketHLE's exposed APIs;
compatibility with physical hardware has not been verified.

## Rebuild the binaries

Python 3, Clang and LLD with ARM support are required. Original local validation
used Clang/LLD 18.1.3:

```sh
python3 tools/hardware-diag/suites/ram/build.py --clang clang --lld ld.lld
```

The script compiles freestanding C and assembles PE32/WinCE headers, imports and
exports. `dist/GZRT999999` binaries are private integration fixtures, not a second public
app to import. Rebuild after
changing C sources.

```sh
cargo test -p pocket-core --no-default-features --features arm-diagnostics --test ram_guest -- --nocapture
```

The test mounts SD read-only and Flash Disk writable, executes the EXE through
Unicorn, acknowledges the final dialog and checks the report, exit code, restored
partition, unloaded modules and reclaimed worker stacks. It removes its files
afterward. Production games do not enable diagnostic logging.

The `dist/fixtures` tests also execute ExitProcess(77), and a case where main calls
ExitThread(11) before the last worker returns 33. They are not included in the
importable ZIP. All three variants are tested with Unicorn.


## Version 9: boundaries, fragmentation and pressure

The complete successful run now has **145 checks (0x91)**. Adds 32 mixed-size
allocations around 8-byte and 4 KiB boundaries, full zero/data verification,
interleaved free/reallocation, bounded progressive 1 MiB allocations until OOM,
verification that existing allocations survive the failure, full RAM/address
space restitution, and all-byte zero verification after decommit/recommit.
The OOM test stops after at most 32 attempts and uses only guest VirtualAlloc
budgets. It does not intentionally exhaust Android/Windows host memory.

See [the historical storage/memory report](../../../../docs/history/VALIDATION.md#storage-memory-validation-2026-10-19) for host-side regression tests and the remaining
platform checks. Rebuild the diagnostic ZIP to obtain these new scenarios.
