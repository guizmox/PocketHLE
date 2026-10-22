# PocketHLE Unicorn source fork

Based on the crates.io `unicorn-engine-sys` 2.1.5 source package. Upstream
licenses and architecture sources are retained.

## Changes

- Increase the indirect jump destination cache from 4,096 to 65,536 entries.
  The hash, translation validity checks, page invalidation and branch chaining
  rules are unchanged. On a 64-bit host this costs 480 KiB extra per CPU; on a
  32-bit host it costs 240 KiB. This is host memory, not emulated guest RAM.
- Build these native sources with CMake instead of silently accepting a system
  Unicorn from pkg-config, which would bypass the change.
- Watch source files relative to this crate so Cargo rebuilds native code after
  changes to the vendored sources.

This changes the small destination lookup cache, not the TCG generated-code
buffer. No executable signatures, game names or game-specific algorithms are
used. The experimental cross-page chaining change was rejected and is absent.

## Reproduce the comparison

The default is 16 cache bits. To restore the upstream capacity for comparison,
set `POCKETHLE_JUMP_CACHE_BITS=12` before building. Allowed values are 12, 14
and 16; changing the variable triggers a native rebuild. For direct CMake builds,
use `-DUNICORN_JUMP_CACHE_BITS=12` or `16` instead.

From the workspace root, on a shell supporting inline environment variables:

```sh
POCKETHLE_JUMP_CACHE_BITS=12 cargo run --release -p pocket-cpu --features unicorn --example jump_cache_bench
POCKETHLE_JUMP_CACHE_BITS=16 cargo run --release -p pocket-cpu --features unicorn --example jump_cache_bench
cargo test -p pocket-cpu --features unicorn
cargo test -p pocket-core --features unicorn --test arithmetic_guest --test native_helpers
```

The synthetic benchmark executes the same number of ARM calls in both builds
and checks its result. Its targets deliberately collide in the original cache;
its speedup is not a prediction for every application. Real applications must
also be measured. No binary or game assets are included in this fork.

## Validation on 2026-10-22

Linux x86-64, native engine built in Release, 320 x 240. Racer's slow camera
phase was measured between seconds 5 and 33, excluding initial startup. The
GPU renderer used Mesa llvmpipe in this environment; physical GPU drivers,
Windows, Android and Steam Deck were not run here.

| Renderer | Mean frame time, 12 bits | Mean frame time, 16 bits | p95, 12 bits | p95, 16 bits |
| --- | ---: | ---: | ---: | ---: |
| GPU path (llvmpipe) | 105.2 ms | 22.8 ms | 275.2 ms | 35.0 ms |
| Software | 136.1 ms | 43.5 ms | 299.7 ms | 59.0 ms |

These are observations from one application and environment, not guaranteed
speedups. Some frame-time spikes remain. A captured ARM computation produced
identical registers and a matching SHA-256 of its writable memory after replay
with both cache capacities.

Validation included 19 pocket-cpu tests, the arithmetic guest integration test,
two native memory-helper integration tests, synthetic ARM calls with colliding
jump destinations, code rewriting/invalidation, unmap/remap, execute permissions,
code hooks and timeout checks. Small call/return programs also passed in MIPS32
little/big endian and AArch64. Cube reached a textured in-game scene during a
12-second GPU smoke run; no comparative Cube performance claim is made.
