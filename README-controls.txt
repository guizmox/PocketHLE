PocketHLE - D-pad presets, controller vibration, Gizmondo brightness
Base: 280ac1f (main)

Extract the ZIP into the PocketHLE repository, replacing the included files.
No SDK files or commercial game binaries are included.

Windows CMD, from the repository root:
powershell -NoProfile -Command "Get-Content 'patch-files.txt' | ForEach-Object { (Get-Item -LiteralPath $_).LastWriteTime = Get-Date }"
cargo build --release -p pocket-desktop

Android CMD (native Rust rebuild is required):
wsl bash -lc "cd /mnt/c/Users/gtristant/source/repos/PocketHLE && bash tools/build-android-native.sh 4 && bash tools/build-android-apk.sh"

Manual checks:
1. Per-game settings: choose Small, Medium or Large D-pad dead zone. Restart the
   app and check that each game retains its own value. Android touch neutral
   radii: 16%, 30.5%, 45%. Controller directional sticks use matching presets;
   digital D-pad buttons retain their direct contacts.
2. Colors: press Brightness (Piano 3 / F3), use its existing slider. Check all
   four brightness positions and a screenshot. F3 does not cycle host levels.
3. Select a rumble-capable controller in the desktop input settings. Import
   tools/vibtest/dist/PocketHLE-VIBTEST.zip if a dedicated test is desired.
   Check start, blinking, stop, loss of focus, game exit and disconnect. Motor
   strength is fixed at 50%. Physical Windows/Switch Pro compatibility must be
   confirmed on the user's machine; SDL reports unsupported hardware as error50.

Implementation notes:
- Existing WinCE registry/event protocol: HKCU ControlPanel\Backlight,
  OnLevel/DimLevel and BackLightChangeEvent. Native RGB565 surfaces unchanged.
- Bounded SDL command queue and motor table; no worker per vibration request.
  Native pulses expire after100ms; worker shutdown stops and joins.
- Existing vibration API error/LastError handling preserved.

Local validation:
- 172 kernel +259 WinCE API +42 library tests passed (no default features).
- 7 tests passed using actual desktop SDL/input/rumble source, including native
  SDL virtual input and100 motor-session cleanup cycles.
- Actual supplied Colors ARM setter/getter replay: all4levels and notification.
- Android full Kotlin source compilation against Android14/AndroidX passed;
  generated resource IDs were used, so this is not an APK build.
- JVM checks:58,800 touch positions, hysteresis and per-game JSON preservation.
- Windows release build and physical Android/Switch Pro tests not run locally.
- Desktop cargo check passed on Linux without default features; local SDL check
  disabled unused X11/Wayland/DBus support. Repo build configuration unchanged.
