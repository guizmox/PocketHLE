PocketHLE cumulative delivery 06: Linux-Parity
Base: GitHub main c8e2d25 (2026-10-12).
Includes deliveries 01 through 06. Full source files, not a git patch.
Apply in increasing order, or apply this cumulative ZIP directly to the base.
R2 replaces the original 05 implementation and is cumulative with unchanged 01-04.
If you already applied 06/07/08, apply the matching R2 or the latest 08-R2;
do not apply 05-R2 over a later feature delivery unless you intend to revert it.
Do not apply an earlier ZIP after a later one: shared files would be reverted.

New feature check:
Build Linux and Windows with the same default Cargo features. Compare library tabs, Gizmondo/Pocket PC layouts, import prompt, filters, GPU options, screenshots and simulated GPS controls. Native OS service availability differs; see docs/DESKTOP-PARITY.md.

Windows CMD (replace NN with this delivery number):
cd /d C:\Users\gtristant\source\repos\PocketHLE
powershell -NoProfile -Command "Expand-Archive -LiteralPath (Join-Path $env:USERPROFILE 'Downloads\PocketHLE-06-Linux-Parity-R3.zip') -DestinationPath '.' -Force"
powershell -NoProfile -Command "Get-Content 'patch-files.txt' | ForEach-Object { (Get-Item -LiteralPath $_).LastWriteTime = Get-Date }"
powershell -NoProfile -Command "Get-Content 'removed-files.txt' | ForEach-Object { if (Test-Path -LiteralPath $_) { Remove-Item -LiteralPath $_ } }"
cargo +1.90.0 fmt --all -- --check
cargo test -p pocket-library --lib
cargo build --release -p pocket-desktop

Android / WSL, from repository root after applying the files:
bash tools/build-android-native.sh
bash tools/build-android-apk.sh
Linux: use the same default features as Windows and the documented native dependencies.

Validation limits:
Shared library tests and host JNI type checking were executed with warnings denied.
Android Kotlin sources were type-checked against Android/AndroidX jars using a generated R stub;
this is not an APK/NDK build or an on-device UI test. Full desktop builds and native devices
remain user validation. No temporary runtime instrumentation is introduced.

R2 correction:
The Pocket PC controls XML and its button drawables match upstream eea7ac6.
The Gizmondo layout is unchanged. Rotate/Exit are persistent emulator actions;
rotation persistence, inverse touch/directional mapping and per-game orientation
remain the requested additions. The original toolbar/fullscreen toggle and Turbo
F3 are restored. Portrait reserves the measured control strip; landscape uses
an overlay, without the R1 fixed bottom band.
The unused R1 PocketPcControls.kt is explicitly removed by removed-files.txt.

R2 checks:
Android application Kotlin source type-check passed (generated R stub, Android/AndroidX jars).
Pocket PC XML parsed; control subtree and upstream button drawables compared equal.
No guest rendering, kernel, VFS or host device backend changed in this correction.
On-device visual validation and the full APK build remain required.
Test: portrait and landscape, Turbo, all four Rotate states, stylus/directions,
fullscreen with/without controls, Back from fullscreen, Exit, restart persistence,
and a Gizmondo title to verify its layout is unchanged.

Title catalogue update:
Names are compiled from the maintainer's Liste_Jeux.xlsx, IDs/title columns only.
Known static names precede BMP names, while real BMP artwork remains preferred.
The 200030 Motocross / 200041 Furious Phil correction is included.
Existing ID-shaped names migrate automatically; custom names are preserved.
Bottom/card IDs belong to the selected executable, not shared launcher resources.
No reimport is required for ID corrections or placeholder-name migration.
Unknown codes retain the previous name fallback.

R3 uses mutually exclusive Keep launcher / Separate games branches. The other cumulative source files are byte-identical to the current R2. Source fingerprints are in SOURCE-SHA256.txt.
