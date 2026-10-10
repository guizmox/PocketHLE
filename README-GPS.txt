PocketHLE — GPS on Windows and Android

Extract the patch into the PocketHLE repository root, replacing existing files.

From CMD at the repository root:
  powershell -NoProfile -Command "Get-Content 'patch-files.txt' | ForEach-Object { (Get-Item -LiteralPath $_).LastWriteTime = Get-Date }"
  cargo build --release -p pocket-desktop

Enable "GPS / host location (GPS1)" in the emulator settings before launch.
Windows: enable location services and desktop application access in Windows
Settings. The UI thread requests WinRT permission; acquisition is asynchronous
and does not block ARM execution. Accuracy depends on the PC's location service.
Android: enable GPS and grant precise location permission when launching the game.
Subscriptions pause in the background and close when the game ends.

Windows and Android also support a fixed position through Gizmondo options.
Enable the override and enter a signed latitude (-90 to 90) and longitude
(-180 to 180). It supplies GPS1 independently of host location access. Android
skips real GPS permission for this mode; older Bluetooth transports may still
require a separate location permission. Satellite counts are not fabricated.

Import tools/gpstest/dist/PocketHLE-GPSTEST.zip as a Gizmondo game.
The test waits up to 30 seconds for a position. GPSTEST_RESULT PASS validates
the API contract; GPS_POSITION AVAILABLE separately confirms position reception.
The report and raw snapshot are in Flash Disk (GPSTEST.TXT/GPSTEST.BIN).
COLORS_POSITION_ELIGIBLE YES confirms a valid fix and accuracy below 100 metres.
Colors also requires a recent timestamp. A working but inaccurate Windows
location provider may therefore remain unsuitable for the game.

Supported features: GPS1/native position, packed 180-byte SDK format, units,
UTC date, no-fix state, permissions, sharing, duplication and VFS closing.
Native validation maps to FixValidated; satellite counters remain zero.
Ellipsoidal altitude is not reported as mean sea level altitude.
Geofence writes, SiRF/APM commands and the undocumented version IOCTL explicitly
return ERROR_NOT_SUPPORTED (50). GNS notifications, host clock adjustment and
Android background location are not implemented.

The original GPS delivery reported 437 passing Rust tests. GPSTEST ran on ARM
with Unicorn and a deterministic provider: PASS, position and Colors eligibility
were confirmed. Colors' real ARM GPS constructor/read/destructor routines produced
the expected coordinates and left no VFS handles. One hundred close cycles with
cross-process duplication verified capture teardown at the last handle.
Desktop Linux, Windows native bindings and Android JNI passed cargo check.
The current Android build and software checks are in docs/ANDROID-BUILD.md.
The debug APK has now been compiled and its signature/alignment verified.
Physical Windows/Android GPS hardware remains untested here. The original Colors
test covered its GPS path rather than a complete game session.
No per-frame instrumentation or Colors executable change is required by this bridge.
