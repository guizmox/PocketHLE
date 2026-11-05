# Emulator optimizations

Host optimizations are always active on desktop, Steam Deck and Android. There
are no optimization switches in the settings, configuration model or execution
policies. Legacy `optimizations` JSON members are ignored and dropped on save.
These changes do not change the emulated CPU speed, FPS limit or FPS counter.

| Optimization | Fixed behavior |
| --- | --- |
| Fast memory access | Repeated writes to private RAM without translated code use the fast path. Permissions, hooks, shared-memory handling and code invalidation still apply. |
| Extended memory translation cache | Start with 4,096 entries and use a 4,096-entry resize floor. Allocation failure handling is preserved. |
| Responsive thread scheduling | Recheck ready guest workers promptly when the main thread blocks in `GetMessageW`, using the existing wait continuation. |
| Extended jump cache | Always use the build's full jump cache, consistently for lookup, insertion and invalidation. Standard builds have 65,536 entries. |
| Batched image page loading | Deferred image accesses always use the batch materializer, retaining page budgets, permissions and unload behavior. |

The old private Unicorn optimization control, conservative RAM flag, legacy jump
execution/helper routines and message-pump specialization have been removed.

MIPS uses the virtual TLB for flat process mappings when Unicorn accepts
PocketHLE's private mapping-only callback contract. The callback resolves page
permissions without restoring guest branch state during a successful refill.
Faults still restore the faulting instruction state, and MIPS page permissions
remain explicit. Kernel segments, including the cached and uncached aliases
of physical memory, still use architectural translation. Older native
libraries fall back to the architectural TLB for all addresses.
Set `POCKETHLE_CPU_TLB=1` before launch to force that fallback for diagnosis.
ARM-only direct block chaining and lazy image paging remain architecture-guarded.

`POCKETHLE_JUMP_CACHE_BITS=12`, `14` or `16` remains a build-time cache capacity,
not a runtime switch.

## Origins

* Fast memory access: `edbf1ac727203a2ddb7294d7912067a69b3ee1d7`.
* Extended memory translation cache: `96e8ea670976eb98f687c40249042312ce89439a`.
* Responsive thread scheduling: `7c9d309de695be46becb99d76250e1476dd663d5`.
* Extended jump cache: `c98e0cfeb71a469dc92b3b3bd677945dd9d5a8e7`.

## Focused validation

`tools/tests/unicorn-optimizations.c` verifies the fixed native path, including
memory hooks, write protection, self-modifying code and shared host-RAM aliases.
Its header contains a standalone build command. Rust CPU tests cover the ARM
virtual-TLB integration, MIPS execution and image-page lifecycle/budget behavior.
Configuration tests verify that old optimization switches are discarded.

## Retained ARM-specific optimizations

These are architecture-guarded fast paths, not user switches:

- Recompute dynamic ARM/Thumb/IT/VFP inputs for the ARM TB lookup key; retain
  fallback for XScale, M-profile, debugging and AArch64 states.
- Use the finite normal float32-to-int32 truncation fast path only in its checked
  magnitude range, preserving inexact flags and generic exceptional/boundary cases.
- Permit inter-page direct ARM chaining only after the identity virtual-TLB
  contract is installed. TLB-mode changes clear the contract and flush blocks.
  CPU-TLB fallback and indirect calls keep their established behavior.
- Keep Win64 link/unlink pointer atomics 64-bit. Preserve guest-code store,
  permissions, unmap/remap and explicit host-code invalidation requirements.

The private 0x5049 identity-mapping control is a retained optimization contract;
the temporary 0x5048 profiling control, timing fields, sampling module and
Cpu::take_backend_diagnostics are not production dependencies. Rejected division
lowering and cache/hash experiments are not retained optimizations.

Historical synthetic gains and original validation counts are recorded in
[implementation notes](IMPLEMENTATION-NOTES.md#history-cpu). They are not a
promise of the same speedup on games, Windows, Android or MIPS.
