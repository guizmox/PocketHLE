# Colors: save compatibility, Turf Wars and performance

## Startup action

Windows/Linux/Steam Deck and Android show **Unlock Turf Wars (skip solo missions)**
in the existing Colors startup dialog. On Deck, X invokes the same action.
It runs before starting the game, off the UI thread; Start and repeated actions
are disabled until it finishes. It never runs automatically on later launches.

- Decrypt and validate `config.cfg` and an existing `Save/SaveFile.isv` before
  changing either file.
- Set `TurfWarUnlocked=1`, preserving the other config settings.
- Leave the entire ISV byte-identical, regardless of Money or ServerMoney.
- Never contact the server, grant money, or change registration identity.
- Repeated clicks only keep the unlock flag enabled. They preserve pending solo
  gains, previous online purchases, and the synchronization checkpoint.
- If no save exists, do not invent one. Preserve missions and all other settings.

Only a changed original config gets a unique `config.cfg-before-turf-*.bak`.
Config replacement is atomic. Unknown/corrupt formats, ambiguous case variants
and symlinks are rejected before writing. Use `GameEntry::gizmondo_flash_dir`.

## Save validation

The supplied config decodes to `TurfWarUnlocked=0`; the supplied ISV contains
Money=525 and ServerMoney=525. The codec was reconstructed from the original ARM
byte transform at 0xe9600/0xe96dc, including feedback and rotating key state.
The startup unlock itself requires no executable patch.

Local diagnostic entry points invoked the supplied executable's original config
reader/writer and ISV parser/serializer through PocketHLE. With the generic
absolute-write fix applied, the game opens the original Flash paths for rb/wb.
The original ISV round-trips to exactly its original plaintext. Earlier funding-policy diagnostics also verified that the ARM parser/serializer
accepts changed balances. The current startup action leaves the ISV untouched. Config read/write retains
all settings, and the host unlock changes only TurfWarUnlocked from 0 to 1.
Original uploaded files remain untouched; all runs use copies.

Rust tests cover codec/key rotations, lower/equal/higher balances, a higher server
balance, idempotency, backups, case-insensitive paths, missing saves, malformed
structures and failure before changing config when the save cannot be decoded.
Desktop/Deck and JNI are type-checked under Linux. The changed Kotlin files pass
Kotlin 2.1.21 syntax parsing; this is not a complete Android APK build or device UI
validation. CI should rebuild both native libraries and the APK together.

## Performance investigation

The complete supplied archive uses the same executable as the standalone upload.
The measured scene is the tutorial dialogue with The Bum, after loading central.
Runs use the real ARM CPU, native offscreen GLES on headless Linux/Mesa, silent
host audio, a 240x320 CLI framebuffer and sparse captures. The graphical
launcher normally configures this Gizmondo title at 320x240; these timings
are therefore comparisons of the same diagnostic setup, not final device FPS. These are
not physical Windows, Deck or Android performance measurements.

API timings and guest execution intervals were first collected separately,
grouped by presentation-counter phase. The first two groups include loading;
subsequent groups contain the 3D scene. A return address seen after a CPU slice is
only an attribution clue: it is not a function's execution time. Candidate
functions were then measured directly at their actual entry and return addresses,
without stopping or modifying their instructions.

| Routine in the supplied ARM image | Direct observations |
| --- | --- |
| 0x912b8, return 0x91e04 | 1780 calls, 3.984 seconds cumulative, approximately 2.24 ms/call. A later scene window adds 900 calls and 2.036 seconds, approximately 20 ms per image across about 100 images. |
| 0x26080, return 0x26b00 | Same 1780 calls, 0.134 seconds cumulative, approximately 0.075 ms/call. A frequent return address here did not imply an equally costly function. |
| 0x99544 and nested 0x994b8 | Respectively about 2.68 million and 2.62 million calls during loading; counters stop increasing in the subsequent 3D scene. They do not explain sustained tutorial-frame cost. |

The 0x912b8 disassembly iterates model tracks/transforms and interpolates fixed
point values, including signed 64-bit divisions. **Model-pose/animation update is
an inferred role, not a recovered original symbol name.** Its repeated direct
cost makes it a concrete optimization target. Rendering the scene on the GPU does
not move this guest routine to the GPU. Changing animation update rate or skipping
this function would alter the game; neither is part of this patch.

Loading also issues over a million intercepted runtime calls (many temporary
string allocations/deletions and numeric conversions). The heavily repeated
loading routines are a separate optimization target. Direct hooks on millions
of calls noticeably perturb loading, so their instrumented elapsed times are
not used as uninstrumented loading benchmarks. Timings for other missions and
crowded city scenes may differ; no whole-game speedup is claimed.

All instrumentation, alternate entry points and extra mounts were local probes.
No profiler, permanent game hooks, changed game resource or frame-rate override
is included in production. The generic optimization also handles four pure COREDLL helpers inside the CPU
run loop: `__rt_sdiv64by64`, `__rt_udiv64by64`, `__rt_srem64by64` and
`__rt_urem64by64`. It removes the kernel/dispatcher round-trip, not the guest
animation calculation. Unicorn stops before reading/writing registers and resumes
through the thunk's padded ARM `bx lr`; signed/unsigned results, zero divisors and
signed overflow match the existing handlers exactly. Other backends retain the
normal dispatcher. Older divisor-first helper variants are unchanged.

The original CPU slice deadline remains fixed across all helper calls, including
an API-free arithmetic loop. Static named/ordinal imports, dynamic exports and
native DLL imports use the same installer; unmapping removes the helper hook.
Tests compare 8,324 inputs with independent wide-integer arithmetic, preserve
registers/flags/FPSCR, return to Thumb, and verify scheduler preemption, remapping,
LastError and neighbouring API dispatch. This is shared emulator code for PC,
Deck and Android; no Colors address or executable fingerprint is used.

An experimental callback implementation was discarded after a loading crash in
one repeat. The delivered implementation only changes registers while Unicorn is
stopped. Repeat-run results and limits are included with the patch. No full-game,
physical-device or universal FPS improvement is asserted.

## Optimization measurements

Alternating old-dispatcher / CPU-local runs of the same diagnostic binary use
minimal identical entry/return probes, the same inputs, and no concurrent build
in measured runs. Between presentation counters 175 and 275, each run executes
exactly 900 calls of 0x912b8. Old path: 1980.196 and 2039.207 ms; CPU-local path:
1884.634 and 2005.930 ms. Mean elapsed time drops about 3.2%, but run-to-run
variation overlaps: this is a modest local result, not a robust whole-game FPS
claim. Both optimized repeats finish the loading/tutorial sequence without a
crash. A separate 320x240 run is a stability check only, not an A/B benchmark.

An isolated diagnostic ARM loop performs 100,000 signed 64-bit divisions through
the original Colors IAT ordinal 2000. Alternating process wall times including
startup: old path 213.6/224.5 ms, CPU-local 118.7/124.3 ms (about 44.5% less).
This isolates call overhead; it is not representative of game-wide performance.
No diagnostic executable or copyrighted game data is distributed in the patch.

## Startup presentation and Turf Wars transfer error

The startup form shows only the unlock button. Its result and balance policy
appear in an acknowledged popup after the action, with no backup notice or
launcher welcome/status text in the form. Backups continue internally. Deck A/B
dismiss the popup without starting a game on the same input.

With the supplied complete game, Fake GPS Paris (48.8584, 2.2945), a local server
at GizmondoColorsTurfWarsServer 0d3765a, actual ARM and GPU rendering, the map GIF
arrives in HTTP 200 but the game displays `Erreur de transfert. Réessayer.`
The original coordinate parser at 0xf2704 returns false if either turf coordinate
is <= 0 (0xf279c..0xf27b0). The signed server grid around Eiffel Tower includes
negative X and zero Y, so the metadata callback rejects it after image decoding.
This is a server protocol mismatch; the hardening commit did not introduce the
signed coordinate scheme. No emulator bypass or game patch is appropriate.

The separate server patch uses a fixed +1,000,000 bias for both client turf IDs,
with inverse conversion on incoming turf commands. GPS degrees, signed SQLite
keys, map projection, ownership and grid origin stay unchanged. Map tile rendering
also converts the wire IDs back to signed storage coordinates before fetching.
The same game now displays the interactive Paris map without the transfer error.
The Linux diagnostic uses a temporary loopback HTTP backend and deterministic
real raster tile bytes; Windows WinHTTP, Android HttpURLConnection and the live
public deployment are not exercised by this run. Restart/redeploy the server
with its patch; updating PocketHLE alone does not repair the server response.

## Independent solo and online wallets

The previous local funding policy is removed. Unlock never writes Money or
ServerMoney. Existing saves previously granted 75,000 are preserved, not reset.
A server starting-money floor of 75,000 (configurable) applies only to new online
registrations. Existing online accounts are not topped up on reconnect.

`pocket-core::colors_wallet` adapts only the supplied retail executable, SHA-256
`065c6f62e793210e7889cab087495a42a84c77b7b41899410156017cb43d7a92`.
Unknown versions remain unchanged. The checked patch changes only the loaded
image; the EXE on disk is never modified. It uses original encrypted save routines
to preserve solo Money and checkpoint ServerMoney to solo Money. The online
amount stays in the online user object. Only positive subsequent solo earnings
are added to the online amount; solo spending does not debit the online wallet.
No graphics paths are changed.

Original ARM functions: balance reader 0x17cd8, save writer 0x17ef4,
response parser 0xf1a9c, registration builder 0xe6880, sync builder 0xe66d4.
The mode-2 registration/sync ACK also checkpoints the local balance. Ordinary
refreshes retain the original game's checkpoint timing. This does not add
transaction IDs or exactly-once recovery after network failures to the legacy
protocol, nor authenticate offline earnings.

`tools/verify_separate_wallets.py` in the companion server patch executes the
original ARM request builders, parser, encrypted ISV I/O and balance arithmetic,
using the exact bytes exported by the Rust compatibility test and a real local
HTTP/SQLite backend. Verified: solo 525 -> online 75,000; online purchase ->45,000
while solo stays 525; solo gain +1,000 ->online 46,000 and solo 1,525; reconnect
adds nothing. Solo spending, forced refresh, saturation, existing accounts,
and the supplied save are also covered. This is an isolated native protocol
fixture, not a full gameplay or physical-device test. Both app and server updates
are required. Starting funds must be changed in deployed custom rules too.
