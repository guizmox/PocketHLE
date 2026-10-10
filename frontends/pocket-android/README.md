# PocketHLE — Android frontend

The Android app uses the real Unicorn ARM emulator and static FFmpeg. It imports
Pocket PC CABs, standalone ARM EXEs, ZIP/RAR packages and Windows installers
containing Pocket PC CABs into its private game library.

The library and settings use portrait orientation. Gameplay uses immersive
landscape: a centered game screen, action buttons on the right, D-pad on the left,
L/R shoulders, the five Gizmondo function buttons and Exit. All interface text is
in English. Settings remain in separate menus.

Gizmondo renders at 320×240. Auto selects the largest integer scale that fits the
available game area; explicit ×1/×2/×3/×4 choices are capped at an integer scale
that fits. The image is never resized by a fractional factor. Filters, keyboard
and controller bindings, fixed GPS, device GPS, GPRS/data, the Colors server and
player ID, camera and Bluetooth are configured from Settings.

## Build

Use the complete [Android build guide](../../docs/ANDROID-BUILD.md).
It specifies Linux/WSL2, JDK 17, Rust 1.90.0, NDK r28c, SDK 35 and the pinned
Gradle/FFmpeg versions. From the repository root, after installing prerequisites:

```bash
bash tools/build-android-native.sh 4
bash tools/build-android-apk.sh
```

The native build produces `libpockethle_jni.so` for arm64-v8a and armeabi-v7a.
The APK script builds an installable debug APK, verifies its signature and checks
16 KiB alignment. Output:

```text
frontends/pocket-android/app/build/outputs/apk/debug/app-debug.apk
```

A missing native library fails packaging. Missing Unicorn fails game launch;
there is no automatic fallback to the trace-only CPU. Hardware tests on an Android
device remain necessary for GPS, camera, Bluetooth and Turf Wars.
