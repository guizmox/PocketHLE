# CPU optimizations without temporary profiling

This cumulative source delivery removes the temporary CPU phase/PC sampling,
backend counters, native Unicorn timing/control 0x5048 and GPU profiling probes.
It does not add a diagnostics module or a Cpu::take_backend_diagnostics method.
The private 0x5049 identity-mapping control is an optimization contract and is
retained. The normal activation log is retained; no benchmark probe is enabled.
Permanent emulator error logging and HARDWARE DIAG remain unchanged.

## Retained optimizations

- ARM TB key fast path: rereads dynamic ARM/Thumb/IT/VFP inputs on every lookup;
  original fallback for XScale, M-profile, single-step/debug and AArch64 state.
  Full block key, tracing, cflags and invalidity checks remain intact.
- ARM float32-to-int32 truncation: finite normal values with magnitudes from
  one up to (excluding) 2^31 use integer extraction/shifts and preserve inexact
  flags; exceptional, small and boundary inputs retain the generic conversion.
- Direct inter-page ARM block chaining: PocketHLE opts in only after its fixed
  identity virtual TLB callback is installed. Generic Unicorn defaults and
  CPU-TLB fallback retain the original same-page rule. Changing TLB mode clears
  the private contract and flushes translated blocks. Indirect calls are unchanged.
- Win64 pointer atomics for linking/unlinking use 64-bit interlocked operations.
- Previously delivered GPU presentation/scratch optimizations and Agaju's
  InvalidateRgn implementation are included in this cumulative clean bundle.

The rejected division lowering, folded-key comparison, cache-hash specialization,
forced-inline experiments and temporary helper annotations are not included.

## Installation

Apply patch-files.txt from the repository root, then delete only the optional
obsolete files in removed-files.txt. Rebuild normally so native Unicorn and the
Rust backend agree. Both manifests use Windows paths and CRLF line endings.
No diagnostics modules or sampling API need to be added to the Cpu interface.
The startup message is ARM identity-mapping block chaining enabled.

Host writes to translated code still follow Unicorn's existing explicit cache
invalidation contract. Guest stores, permission changes and unmap/remap are
covered by the retained inter-page regression test. No general virtual-to-
physical remapping guarantee is made for callers violating the private contract.

## Validation and limits

The final native ARM engine is rebuilt from sources with all PH_NATIVE fields,
macros, counters and diagnostic control removed, rather than merely disabled.
The clean Cpu crate compiles without any diagnostics module or diagnostic trait
method. Its 37 applicable tests pass, including linked-code guest writes,
explicit host invalidation, permission revocation and unmap/remap. MIPS is
excluded from this ARM-only native runtime build.

The complete optimized local non-GUI app stack is compiled against the clean
Cpu/Kernel interfaces. The supplied PVR Racer and GLBenchmark are used for
bounded smoke runs; CPU Float/Integer result export is checked. Float-to-int
results and floating status are compared against the original native helper
on three million input patterns; all comparisons match. Both bounded app runs
complete successfully, with activation confirmed and no temporary profiling output.

Previous approximately 34% timing reduction applies to a synthetic direct
inter-page call workload, not global game FPS. Removing probes may change the
numbers, so no Windows speedup is inferred. Windows/MSVC (including the three
64-bit atomic branches), Android, AArch64/MIPS native builds and full CI remain
unverified locally. No claim of platform-wide conformance is made.
