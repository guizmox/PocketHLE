PocketHLE - cumulative native GPU four-game update

Includes all preceding GPU/lighting/brightness/FPS/Carmageddon/Colors fixes.
New work applies to Hit and Myth, Richard Burns Rally, Chicane and Trailblazer.

- RGB unsigned-byte readback, correct pack/unpack row alignment.
- DXT3 explicit alpha and its mip levels, shared decoder for software/GPU.
- Direct preparation of unique indexed vertices and native ushort topology.
- Real native mip filtering, explicit/generated missing levels, per-unit LOD bias.
- Distinct EGL pbuffer/pixmap images, read/draw switching, GDI/DIB writeback,
  bounded ownership, deferred deletion and parent/session teardown.
- Texture deletion/reuse clears every surface cache, including parked renderers.

Software retains its current base-level sampling. GLES2 NPOT assets use base
filtering/clamp-to-edge only when full native NPOT support is unavailable. Flat provoking-vertex cases retain the compatibility path.
Single guest EGL context support is retained; this is not a full EGL implementation.

Validation: 134 GLES unit tests, 10 desktop GL/GLES2 native renderer tests,
4 native Context tests, 28 WinCE GLES tests and 2 native API/presentation tests.
The full WinCE unit suite also passes: 284 tests, including native tests.
Rust ARM64/ARMv7 checks pass with warnings denied. No complete Windows release
or Android APK was built in this environment. No temporary profiling/log spam.

Apply all files in this ZIP over the same PocketHLE checkout as the preceding
cumulative patch. patch-files.txt lists every delivered file. Do not apply it
as a standalone project. Existing settings and opt-in experimental GPU remain.

Windows CMD:
cd /d C:\Users\gtristant\source\repos\PocketHLE
powershell -NoProfile -Command "Expand-Archive -LiteralPath (Join-Path $env:USERPROFILE 'Downloads\PocketHLE-GPU-Four-Games.zip') -DestinationPath '.' -Force"
powershell -NoProfile -Command "Get-Content 'patch-files.txt' | ForEach-Object { (Get-Item -LiteralPath $_).LastWriteTime = Get-Date }"
cargo build --release -p pocket-desktop

Android from CMD:
wsl bash -lc "cd /home/gtristant/projects/PocketHLE && unzip -o /mnt/c/Users/gtristant/Downloads/PocketHLE-GPU-Four-Games.zip && bash tools/build-android-native.sh 4 && bash tools/build-android-apk.sh"

Test with GPU enabled: menus, gameplay, HUD/fonts/textures, transitions and exit
for all four games. RBR/Chicane RGB capture paths and Hit offscreen engine branches
may depend on game data/options. Compare with software and keep an eye on Colors
and Carmageddon as prior regressions. Report visual artifacts/FPS with platform,
scene and renderer. Static executable audits do not predict runtime frame rates.
