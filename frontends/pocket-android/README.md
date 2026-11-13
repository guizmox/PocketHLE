# PocketHLE-Reborn Android frontend

The app and Rust JNI bridge are built together for arm64-v8a and armeabi-v7a,
using Unicorn and static FFmpeg. This directory is the app source, not a second
owner of settings or build policy.

- [Build, WSL2, APK packaging and signing](../../docs/BUILD.md#android)
- [Settings, touch layouts and automatic display fitting](../../docs/FRONTENDS.md)
- [Rendering, filters and per-game GPU profiles](../../docs/GPU.md)
- [Graphics log collection](../../docs/ANDROID-GRAPHICS-DIAGNOSTICS.md)
- [Gizmondo services, shell profiles and Colors](../../docs/GIZMONDO.md)

From the repository root after installing the documented prerequisites:

```sh
bash tools/build-android-native.sh 4
bash tools/build-android-apk.sh
```

The local debug APK is
`app/build/outputs/apk/debug/app-debug.apk` relative to this directory. Keep JNI
libraries and Kotlin/resources from the same revision. Packaging checks do not
replace physical GPS, camera, Bluetooth or gameplay validation.
