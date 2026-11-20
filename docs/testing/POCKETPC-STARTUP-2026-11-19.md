# PocketPC startup investigation

Base: `b24d80122d4dc7607dce69e1a8348f918b75a99d` (`snapshots`).
Game binaries and data are unchanged and are not distributed with this patch.

## Brothers in Arms 3D

The earlier DOS-path fix is ported into the current VFS rather than replacing
that file with its pre-snapshot version. The failed resource path was
`\Progra~1\Brothe~1\sprites.bar`; resource reads succeed after the port.
The VFS snapshot code and its file-description/alias ownership remain in place.

A second startup obstruction was traced to the previous PocketPC shell policy.
The game obtains GETRAWFRAMEBUFFER through ExtEscape, without opening a GAPI
display. Its fullscreen native window has style `0x10000000` and rectangle
`(0, 0, 240, 320)`. Its scanout checks a saved DWORD per screen row against the
current raw display before writing. A mismatch causes the game to set its own
pause flag, not an emulator exception.

Instruction hooks locate this transition at `0x92970`. At that point the saved
row values are zero while the first 26 rows of raw storage contain the synthetic
shell taskbar gradient. The relevant check is `0x9259c`; the pause flag is at
`0xf3f54`. The paused loop later fills the table with `0x00ff00ff`, so inspecting
that table only after the pause hides the original mismatch. WM_ACTIVATE was
received with wParam=1; a missing initial activation is not the demonstrated cause.

A temporary diagnostic suppressing shell composition before the first emulated
frame reaches the actual language-selection screen. This is diagnostic evidence,
not a production setting or game-specific bypass. An attempt made only after the
first hook is too late because the initial shell pixels are already in the base
image. The diagnostic code and hooks are kept outside the production tree.

The shared fix models automatic native-window coverage and explicit shell
visibility separately. A visible, borderless native window covering the panel
occupies the navigation strip in Automatic mode. Captioned/work-area windows and
native dialogs retain it. Explicit SHFullScreen show/hide takes precedence, and
GAPI fullscreen retains its existing behavior. Composition does not invent an
application strip before any native window exists; startup work-area queries
still reserve the usual strip. No query of a raw pointer creates a draw lock,
changes graphics profiles or forces a GPU readback.

MoonGBA is the reference for the work-area case: its main window has the same
visible-only style, but rectangle `(0, 26, 240, 294)`. Checking only WS_CAPTION
would incorrectly classify both applications alike. The regression test covers
both rectangle types, captioned and hidden windows, and explicit shell requests.
The shell visibility state is captured through the existing GDI checkpoint;
WinCE section version 10 replaces version 8 without a legacy-format reader.

With the production policy and no diagnostic overrides, the game reaches the
language selector, sound question, mission briefing, cinematic and actual mission
with its character, scenery and HUD. The input sequence uses Enter for menu
selection, F1 for the game's left soft key, and digit 5 to skip its cinematic;
these are real guest key messages, not patched game logic. This is a startup and
mission/snapshot check, not validation of every default controller assignment.
MoonGBA's tested startup image is pixel-identical before and after the policy.

A persisted snapshot taken in the running mission was written, read back and
restored. The resumed game continued producing images. Capture plus synchronous
archive writing took about 746 ms; reconstruction after reading was about 434 ms on this
Linux host. These are harness timings, not Android or Windows UI latency figures.
The regular cold-restore core tests are separate from this live-game restore.

## V-Rally Championship Edition

The original null-vtable crash at PC `0x13720` followed the missing CFile
constructor. The subsequently supplied retail/debug CE3 ARM MFC DLLs establish
the ABI used by the generic [MFC bridge](../MFC.md). The VFS opens must begin at
zero rather than append; a real-file regression covers that cursor contract.

A headless probe initially showed "Install V-Rally from the CD" because the
probe had omitted the imported CAB registry values. Replaying those same values
as the GUI runners already do gets through that check without modifying the
registry contents, executable or game data. This is a correction to the test
harness, not an installation or licence bypass in the emulator.

The next observed calls required real CGdiObject attachment/deletion and
CreatePatternBrush. Bitmap brush pixels now have independent ownership and
use the ordinary GDI drawing/ROP, clipping and DIB synchronization paths.
The brush sampler also participates in native snapshots; WinCE section 10
includes the new brush kind, per-DC origin and palette-relative bitmap data.

The next failure was `CreateBitmap(240, 320, 1, 8, pixels)` returning zero:
the existing handler rejected 8-bit DDBs. V-Rally selects a real palette on its
paint DC and builds a full-frame pattern brush from those indices. Implementing
palette-relative DDBs and pattern sampling produces the language-selection
screen and subsequently the Ford Escort car-selection screen with actual
application assets. The scripted hardware-button path progressed into an actual race: the Ford
Escort moves along the road, with scenery and the race HUD updating. The final
38-second run finished without a guest CPU exception or an unimplemented-call
report. These are Linux host guest-execution
checks, not a claim of Windows or physical Android gameplay validation.

## Brothers in Arms phone context keys (follow-up)

With the per-game `pocketpc_phone_softkeys` setting enabled, the actual ARM
executable was run with Tab (`0x09`) and Escape (`0x1b`) input sequences through
the same `pocket_session::input::phone_softkeys` adapter used by both GUI runners.
Tab selected Yes and the scripted path reached the first mission; Escape
selected No and reached the main menu. The earlier direct-F1 test alone did
not validate the launcher's controls. These headless runs validate the guest
and shared adapter, not physical Android touch or controller operation.

AYGSHELL ordinal 183 is called from the game's Escape branch, at return PC
`0x9322c`. It is not the call that handles the sound prompt's F1/F2 keys.

Ordinal 183 is named `SHNavigateBack` in the WM6 SDK import-library export dump
([source](https://blog.ch3cooh.jp/entry/20101202/1291222800)). The ordinal table
now identifies it in diagnostics, but no successful-return handler was invented:
full shell back-navigation remains unsupported. The tested BIA sound choices
use F1/F2 and do not call it. Numeric 2/4/6/8 are handled by the game together
with Up/Left/Right/Down; no numeric-pad compatibility layer is added.

## Follow-up controls and build validation

Rust 1.90 checks for Desktop and Android JNI with Unicorn pass. The complete
kernel/WinCE run passes 420 and 785 tests respectively (35 pre-existing ignored
WinCE tests). The library suite passes 238 tests; the shared session/input suite
passes 10. The 12 selected Unicorn core snapshot tests pass with WinCE section
10. Android's final Kotlin unit suite passes 121 tests, with no failures.

Motion tests cover comfortable neutral calibration, upright-device pitch, all
four presentation rotations, sensitivity, shared-key release, direct touch and
zone priority, relative-aim-only output and foreground reset. These are synthetic
sensor inputs, not physical sensor measurements. Android source and native
release builds are validated separately from Windows/phone gameplay.

Desktop binary build also completed with
`cargo build -p pocket-desktop --no-default-features --features unicorn`. This
Linux validation build omits optional CPAL/static-video features; it does not
change their delivery defaults or establish audible playback on the test host.
Both Android native release ABIs were rebuilt with their normal features from
one stable source fingerprint:
`37aa3cd3be69f00ae06ee1e2d15a7f711d9acdd18e5814db1152dc86c2063cdc`.
ELF dependencies, static FFmpeg, JIT builtins and 16-KiB alignment checks pass
for both. Gradle `assembleDebug` succeeds with `verifyNativeLibraries` enabled;
the assembled APK contains both native ABIs. Source freshness was checked
without a bypass. The APK is a build-validation artifact; delivery is source.

## Follow-up: catalogue loading and 0.5.0

Both frontends now share `game_card_identity` and `game_card_display` for card
metadata. The existing process-local PE cache also holds static GLES imports and
CEUX/HI_RES_AWARE. Metadata reads parse the file without allocating mapped guest
sections or BSS; the execution loader is unchanged. File length/mtime and local
DLL dependencies retain the existing invalidation rules. Legacy title migration
only scans identities for names that can actually be migrated. Accelerated-build
selection first checks for matching sibling names, preserving its existing
selection policy. No persistent metadata cache or library JSON schema is added.

The explicit profiling example is reproducible with:

```sh
cargo run -p pocket-library --example profile-catalogue -- /path/to/library
```

On a local 70-card fixture made from ten copies of seven supplied installations,
the first metadata traversal in the process decreased from **689.5 ms to
233.8 ms**, with identical metadata checksum (52760). Later traversals decreased
from **233–246 ms to 2.7–2.8 ms**. Opening the catalogue itself was 0.8–1.4 ms
afterward. These measure local file-backed metadata, not an Android device boot,
a physically cold disk or the entire time until the UI appears.

The shared phone-control option now also maps Turbo (VK_F3) to digit 5. Its
persisted setting and default are unchanged; both edges use the same adapter.
Default Turbo behavior and Action/Enter remain independent. The native and
managed guest delivery paths reuse that adapter on Desktop and Android.

Release 0.5.0 is inherited by all 14 workspace packages, including pocket-netcf.
Android derives versionName/update comparisons from the same root version and
uses versionCode 24. No third-party dependencies were upgraded for this bump.

### V-Rally steering follow-up: presentation boundary

The supplied steering video is still an open visual report. A 64-second real ARM
run entered a race and exercised left/right steering. Captures taken immediately
after `CreateBitmap` were compared with the following displayed frame, using
the selected logical palette and the display's RGB565 quantization. All twelve
sampled pairs matched exactly (zero differing pixels, full 240×320 images).
Thus the observed mountain panorama is already present in the guest-produced
8-bit bitmap on this run; neither pattern-brush copying nor final presentation
introduced it in those pairs. This game uses CPU image generation and GDI,
not GLES. This is a fault-localization result, not proof that its ARM calculations
or its native-device appearance are correct. No speculative CPU arithmetic or
rendering change is included for this report. Temporary capture/trace probes
are excluded from the source patch.

0.5.0 verification: 260 targeted Rust unit tests and all 121 Kotlin unit tests
passed. Desktop/JNI host compilation checks passed with Unicorn and optional
host audio/static video disabled. Default Android native release libraries built
for both ABIs, including static FFmpeg/JIT and 16 KiB checks; native freshness,
unit tests and APK assembly passed. The built APK reports versionName 0.5.0 and
versionCode 24. Rust 1.90 formatting and whitespace checks passed. Physical-device
control feel remains a separate user test.
