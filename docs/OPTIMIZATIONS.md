# Emulator optimizations

The global **Emulator Settings → Optimizations** section is available on desktop,
Steam Deck and Android. All four options default to enabled, including when an
older configuration has no optimization settings. Changes apply to the next game
launch. These options do not change the emulated CPU speed, FPS limit or FPS
counter. They are emulator policies, not properties of ARM hardware.

| Option | Explanation |
| --- | --- |
| Fast memory access | Reduces the cost of repeated writes to private RAM pages that currently contain no translated code. Memory permissions, hooks, shared-memory handling and code invalidation remain active. Disable to compare with the conservative write path. |
| Extended memory translation cache | Remembers more guest-to-host address translations, reducing repeated translation work. Uses additional host memory. Disabled restores the earlier 256-entry initial TLB and 64-entry resize floor; enabled starts at 4,096 entries and keeps that floor during normal resizing. Allocation failure handling remains unchanged. |
| Responsive thread scheduling | Rechecks waiting guest workers promptly while the main thread blocks in `GetMessageW`, rather than waiting for a synthetic repaint. Disable for compatibility comparisons. This does not disable the FPS counter improvements from the same historical commit. |
| Extended jump cache | Remembers more translated-code destinations, reducing jump-cache collisions and repeated lookups. Standard builds use 65,536 active entries when enabled and 4,096 when disabled. Storage remains sized for the build's maximum even when disabled, so this switch does not reclaim that allocation. |

## Selection without an extra execution-time option branch

Native options are passed to the bundled Unicorn engine before its lazy CPU
initialization. Subsequent changes through the private control are rejected.

* TLB capacities are selected during initialization and resizing.
* Fast memory access broadens an existing RAM-flags test; it adds no second
  option test to the store path. The private conservative flag is separate from
  RAM ownership flags, so disabling this option does not change who frees RAM.
* Scheduling binds one of two `GetMessageW` handler specializations before loading
  the game. The dispatcher retains its existing function-pointer call.
* Jump caching selects fixed-size execution and helper specializations at engine
  initialization. The selected helper address is embedded during translation.
  The normal hot hash remains unchanged. Cache insertion and invalidation use
  the selected size consistently, including shared-memory aliases.

`POCKETHLE_JUMP_CACHE_BITS=12`, `14`, or `16` remains a build-time maximum.
Enabled uses that maximum; disabled uses 12 bits. A build with a 12-bit maximum
therefore has the same jump-cache capacity in both states.

## Origins

* Fast memory access: `edbf1ac727203a2ddb7294d7912067a69b3ee1d7`
  (the per-game FPS limit from that commit remains a separate setting).
* Extended memory translation cache: `96e8ea670976eb98f687c40249042312ce89439a`.
* Responsive thread scheduling: `7c9d309de695be46becb99d76250e1476dd663d5`.
* Extended jump cache: `c98e0cfeb71a469dc92b3b3bd677945dd9d5a8e7`.

## Focused validation

`tools/tests/unicorn-optimizations.c` exercises all eight native combinations,
including late/invalid configuration rejection, memory hooks, write protection,
guest self-modifying code, unmap/remap and shared host-RAM aliases. Its header
contains a standalone build command. The Rust CPU test also exercises all eight
combinations through PocketHLE's virtual-TLB integration. Configuration tests
cover old files, partially populated settings and all sixteen option combinations.

On the validation Linux x86-64/GCC 13 ARM-only release build, the normal CPU
execution loop and store helper retained the same instruction sequence apart
from alignment padding. The dirty-write helper changed only an existing test's
immediate mask (`0x01` to `0x41`), with no added instruction or branch.
This verifies the absence of an extra option branch in those paths; it is not a
cross-platform FPS benchmark or a guarantee of identical compiler output on
other hosts. Full Android/desktop builds and device performance testing remain
separate validation steps.
