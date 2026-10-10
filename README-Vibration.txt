PocketHLE – Gizmondo vibration on Android

Base: guizmox/PocketHLE main commit 3fd4ea8 (fix ci cargo tests).
The patch was reapplied to this latest commit, preserving the new audio, game
information, GPS fill and stronger virtual-control haptic changes. Do not apply
over an older source snapshot. Full changed files, no SDK source redistributed.
No commit or push performed; local working branch feat/android-vibration.

Extract this ZIP at the repository root, replacing files.
CMD:
powershell -NoProfile -Command "Get-Content 'patch-files.txt' | ForEach-Object { (Get-Item -LiteralPath $_).LastWriteTime = Get-Date }"

Rebuild BOTH native Android ABIs and the APK, using the existing WSL2 setup.
CMD (assuming the standard SDK location used by docs/ANDROID-BUILD.md):
wsl bash -lc "cd /mnt/c/Users/gtristant/source/repos/PocketHLE && export JAVA_HOME=/usr/lib/jvm/java-17-openjdk-amd64 ANDROID_HOME=$HOME/Android/Sdk ANDROID_NDK_HOME=$HOME/Android/Sdk/ndk/28.2.13676358 && bash tools/build-android-native.sh 4 && bash tools/build-android-apk.sh"

If using a different Linux repository or SDK location, adjust these paths.
APK: frontends/pocket-android/app/build/outputs/apk/debug/app-debug.apk
Optional desktop rebuild: cargo build --release -p pocket-desktop
Windows emulates NLED state; this patch adds physical vibration on Android only.

Implemented: VIB1 CreateFile/DeviceIoControl/CloseHandle; NLED count/supports/
settings, NLedGetDeviceInfo and NLedSetDevice; off/on/blink and meta cycles.
Fixed game amplitude 128/255 when supported, native fixed strength otherwise.
Existing virtual-button amplitude 180 is preserved, suppressed only while a
real game waveform is active. Permission VIBRATE already exists in latest main.

Cleanup: CloseHandle preserves motor state (SDK closes before Sleep). An explicit
OFF cancels and frees the native motor/session immediately. Shared-device Drop
also cancels/frees. Failed first activation releases its temporary native motor.
Pause stops physical output; resume restores the current request. Exit clears
all Android sessions and rejects late calls. No Activity reference, worker thread,
or host polling timer; bounded waveform arrays, scoped JNI local references.
LastError: invalid arguments 87, denied access 5, small output 122, unsupported
50, stopped Android session 995, native failure 21. Success preserves LastError.
Microsecond times round up to 1 ms. MetaCycleOn above 1024 is explicitly unsupported.

Validation on latest base: 469 Rust tests pass (kernel170 + library41 + API258).
JNI host type-check with -D warnings and workspace rustfmt pass. VibrationHost
compiles against real Android14 API classes with no warning; JVM fake-platform
harness passes amplitude/fallback/API24, pause/resume, errors, stale identifiers.
Resource regression covers 100 on/off cycles, failed starts and last-owner Drop.
ARM VIBTEST PE compiled. No native Android ABI/APK or physical phone run performed
in this environment; those remain integration checks. No per-frame probes added.

Import tools/vibtest/dist/PocketHLE-VIBTEST.zip into the Android library, run it.
Expect: 0.5s buzz, regular pulses, paired pulses. VIBTEST.TXT is written in its
Flash Disk. Check Home during a pulse stops vibration; returning may resume;
Exit must stop vibration permanently. Verify controls do not replace game pulses.
