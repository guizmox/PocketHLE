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

## Code-hook lookup cache

The bundled Unicorn engine caches the first and last matching code-hook list
nodes for repeated guest addresses. It retains registration order and the
existing bounds/deletion checks. Hook addition, deletion and deferred cleanup
invalidate the cache. A hook that mutates the list during its callback resumes
the original traversal rather than skipping newly appended hooks.

The cache is per engine, has 256 entries and is used only for non-sentinel code
notifications with at least 16 code hooks. Smaller lists, other hook types and
instruction-counter sentinel notifications retain the ordinary traversal. Full
64-bit addresses and generations are checked; hash collisions cause a lookup,
never a callback substitution. Nested callbacks use local bounds so a colliding
inner lookup cannot replace the outer notification's remaining hooks.

This changes host lookup work, not ARM arithmetic, translated-block validity,
thread scheduling, guest timing or the presentation policy. It applies without
application detection or settings. Storage costs 8 KiB per engine on 64-bit
hosts (6 KiB with 32-bit pointers). Cold lookups can do more work than the old
stop-at-first-callback walk; repeated import/helper addresses are the target.

`tools/tests/unicorn-code-hooks.c` checks registration order, overlapping hooks,
callback-time insertion/deletion, stop requests, deletion/re-registration, count
mode, Thumb IT, wildcard hooks, independent engines, nested execution, full-width
address collisions, AArch64 and MIPS. Its throughput test checks the same number
of callbacks and is not a prediction of application FPS.

### Q3CE validation (2026-11-07)

Linux x86-64, native Release build, 640x480, GPU backend using Mesa llvmpipe.
The same recorded Q3DM1 scene renders 580 timedemo frames per pass. Runs are
sequential (reference, cache, cache, reference), without tracing or background
builds. The replay fixes camera/server snapshots; it does not benchmark live
server/bot simulation.

| Code-hook lookup | First pass | Second pass | Engine frame rate |
| --- | ---: | ---: | ---: |
| Reference | 48.9 s | 49.1 s | 11.8–11.9 fps |
| Cached | 31.1 s | 25.0 s | 18.6–23.2 fps |

These are local measurements, not a prediction for Windows, physical GPUs,
Android or other workloads. Both variants hit the same existing UIARM undefined
instruction after completing/logging the timedemo, on return to the interface;
this optimization does not suppress that separate failure.

Native validation: 714 API tests, 341 kernel tests and 49 CPU tests pass. The API
suite excludes exactly one independently reproduced baseline dialog-geometry
failure (client height 85 versus expected 84); 13 existing tests remain ignored.
Dedicated C tests also pass against the reference and cached ARM/AArch64/MIPS
engines, including callback mutation, nested execution/deferred hook deletion,
Thumb IT, instruction counting and 64-bit address collisions. The original
native permissions, self-modifying code and shared-memory tests pass as well.

A final rebuild of the delivered source (including the nested-deletion guard)
repeats the 580 frames in 25.8 s, 22.5 fps. Formatting and diff checks pass.

## Graphics paths

Native shared-texture caching, stable-channel framebuffer feedback batching and
NV sampler/uniform preparation are maintained in [GPU.md](GPU.md#nv-combiner-execution).
They apply automatically when their rendering path is used. Renderer choice and
the PocketPC NV extension compatibility gate remain separate supported settings;
these optimizations do not add game-specific overrides or change frame limits.
