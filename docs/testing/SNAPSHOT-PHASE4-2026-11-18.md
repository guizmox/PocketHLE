# Snapshot phase 4 — native uniform identity

Historical phase validation. For subsequent launcher and active-communication
coverage, see [phase 5](SNAPSHOT-PHASE5-2026-11-18.md).

Base: branch `snapshots`, commit `8acdc23` (`snapshots phase 2`), including
the phase 3 changes described in [phase 3](SNAPSHOT-PHASE3-2026-11-18.md).
The delivered patch is cumulative against that commit. No commit/push, game
binary/asset change, legacy snapshot reader or per-title exception is added.

## Completed increment

Desktop native programmable restore no longer requires the host linker to assign
identical numeric uniform locations. Each saved active uniform element retains
its guest location and is resolved by name on the new host executable. A complete
per-program bijection is retained only when at least one location differs.

All integer/float scalar and vector setters, matrix setters, array counts,
`glGetUniformLocation`, `glGetUniformfv` and `glGetUniformiv` share the mapping.
An unknown guest location is rejected instead of falling through to a native
location that now belongs to a different variable. Repeated capture writes guest
locations; successful guest relinking resets the mapping. A failed guest relink
preserves it for the previous executable still installed in GL, while native
validation errors from setters/queries remain errors. Final program
release removes it, including a delete-pending program restored as current.

The common GLES2 layer owns the mapping; there is no frontend-specific copy.
This is native OpenGL uniform storage and rendering. No shader/binary rewriting,
per-frame resource copy, GPU readback, replay history or fake GL result is added.
The native uniform query still supplies the real location; the table translates
its identifier for the guest. With no relocation there is no new GL query. After
relocation, `glUseProgram` queries the actual current program so rejected calls
cannot change the target used by following setters. Uniform writes themselves
add no GL queries or synchronization.

The serialized uniform record already contained a signed location. That field
now consistently means guest location; the structure and section version are
unchanged. This is not an old-format compatibility path. The cumulative phase 3
WinCE version change still requires recreating earlier development slots.

The location/link lifecycle follows the
[Khronos OpenGL ES 2.0 specification](https://registry.khronos.org/OpenGL/specs/es/2.0/es_full_spec_2.0.pdf),
sections 2.10 and 6.1.8. An installed executable surviving failed relinking is
not permission to fabricate successful uniform updates on an unlinked program.

## Validation

The default-feature Desktop release build passed, as did Rust 1.90 formatting
(`cargo fmt --all -- --check` and the explicit runner include-tree check) and
`git diff --check`. The complete WinCE API library suite passed **784 tests**,
with **29 ignored**. Explicitly enabling the GLES2 native tests passed **16/16**;
this includes the new deterministic relocation regression and the existing
resource, retained-image, depth/stencil, sharing and scaling checks.

The phase 3 CPU/kernel/core validation remains recorded in the previous note;
those suites were not rerun for this GLES2-only increment.

Electopia was tested in its 3D attract mode twice: once with a shared UI context
and once with an owned context. Each run captured at 45 seconds, destroyed the
source emulator, restored into a new emulator and continued for eight seconds.
Both runs exited cleanly and the first/last restored images show progressing
3D animation. The owned case correctly uses CPU presentation without a UI
share source; its rendering and native resource snapshot still use host OpenGL.

| Electopia / context | Capture | Encode + write | Save response | Restore to first frame |
| --- | ---: | ---: | ---: | ---: |
| shared | 0.650 s | 0.665 s | 1.330 s | 1.098 s |
| owned | 0.689 s | 0.632 s | 1.329 s | 1.213 s |

These are individual observations under Linux Mesa llvmpipe LLVM 19.1.7, with
explicit periodic test image readbacks and no physical audio device. Disk caches
were not flushed. This is a cold emulator restore, not a cold OS-cache benchmark
or a measured speedup against phase 3. The probe used the actual production
Desktop runner and a disposable library; supplied game binaries/assets were
unchanged. Its temporary example source was removed after validation.

Reproduction follows the commands and optional installed-game test settings
in the phase 3 note. The targeted native regression can be run with:

```sh
EGL_PLATFORM=surfaceless cargo test --locked --release -p pocket-winceapi \
  --features unicorn-tests,video-static,audio-cpal --lib \
  gles2::renderer::uniforms:: -- --include-ignored --test-threads=1
```

The native regression creates real shaders/programs and deliberately uses a
different saved guest-location namespace, including a collision with a valid
native location. This exercises relocation deterministically on one driver;
it is not a claim that Mesa itself randomly moved locations in that run.
It checks typed writes/queries, arrays/matrices, independent program namespaces,
repeated capture/restore, failed UseProgram, failed/successful relink and final
release, plus actual drawing through the native API. No production-only test
switch is added. Linux llvmpipe remains a software host GL driver; physical
Windows/WGL, Steam Deck/Proton and Android device validation remain outstanding.

## Remaining work

1. Direct resource capture on host OpenGL ES (portable color/depth/stencil,
   buffers and texture levels), replacing the bounded Desktop ES journal.
   Android snapshot UI/provider integration remains separate.
2. Synchronous suspended SD launchers: capture their actual suspended state
   with the child and common device commit barrier, without stale prelaunch
   parent state.
3. Active external I/O/provider policy: active HTTP/POST and Bluetooth cannot
   be rewound remotely. Completed Android Java HTTP responses still need the
   provider implementation. Never replay a POST or fabricate received data.
4. Hardware/platform validation and currently rejected resource states,
   including active programs whose latest link failed. See
   [Snapshot internals](../SNAPSHOTS.md) for the current boundaries.

Closed files outside the open-resource graph still do not constitute a full
save-directory rollback. The filesystem transaction remains an in-process
rollback guard rather than crash-durable multi-file atomic replacement.
