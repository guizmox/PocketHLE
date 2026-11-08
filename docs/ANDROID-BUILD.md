# PocketHLE — Android build and packaging

Build the current repository checkout using the pinned toolchain below. This is
a build guide, not instructions for applying an old cumulative ZIP.
See [Gizmondo options](GIZMONDO.md) and [GPU rendering](GPU.md).

## Interface and settings

Use [FRONTENDS.md](FRONTENDS.md) for control geometry, settings ownership and
current display fitting, and [GPU.md](GPU.md) for rendered resolution, filters
and FPS. Those policies are not duplicated in this build recipe.


## Build from Windows using WSL2 / Ubuntu

Use a **Linux SDK/NDK in WSL2**, with Linux Java and Rust. Do not combine a Windows
NDK with Linux cargo. Initial installation requires network access. The scripts
download pinned Gradle and FFmpeg versions when absent and verify their hashes.
FFmpeg is linked statically; no ffmpeg.exe or DLL needs to be copied.

From Windows CMD, enter WSL2:

```cmd
wsl
```

In WSL2, change to the source root, for example
`cd /mnt/c/Users/YOUR_NAME/Documents/PocketHLE`. A copy in WSL2's Linux filesystem
can improve build performance.

Install prerequisites once:

```bash
sudo apt-get update
sudo apt-get install -y build-essential cmake pkg-config curl unzip python3 openjdk-17-jdk
```

Install Rust using rustup if necessary: https://rustup.rs/.
Install the Android **Linux** command-line tools from
https://developer.android.com/studio#command-line-tools-only under
`$HOME/Android/Sdk/cmdline-tools/latest/`, with `bin/sdkmanager` directly inside it.

```bash
export JAVA_HOME=/usr/lib/jvm/java-17-openjdk-amd64
export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/28.2.13676358"
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$PATH"
sdkmanager --licenses
sdkmanager 'platform-tools' 'platforms;android-35' 'build-tools;35.0.0' 'ndk;28.2.13676358'
rustup toolchain install 1.90.0 --profile minimal
rustup override set 1.90.0
cargo install cargo-ndk --version 3.5.4 --locked
```

For subsequent builds:

```bash
export JAVA_HOME=/usr/lib/jvm/java-17-openjdk-amd64
export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/28.2.13676358"
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$PATH"
bash tools/build-android-native.sh 4
bash tools/build-android-apk.sh
```

The first script builds FFmpeg and the Rust bridge for **arm64-v8a** and
**armeabi-v7a**. The second builds an installable debug APK and checks signature
and 16 KiB alignment. Versions: AGP 8.7.3, Gradle 8.10.2, Kotlin 2.1.21,
SDK 35, Build Tools 35.0.0, NDK r28c. Minimum Android version is 7.0 (API 24);
targetSdk remains 34. `4` sets the FFmpeg build's parallel job count.

Output:

```text
frontends/pocket-android/app/build/outputs/apk/debug/app-debug.apk
```

Missing native libraries or invalid ABI/alignment/JIT checks fail the build.
Without Unicorn, game launch fails explicitly; it does not fall back to a
trace-only CPU.

With a phone accessible through adb:

```bash
adb install -r frontends/pocket-android/app/build/outputs/apk/debug/app-debug.apk
adb logcat -s PocketHLE
```

Windows adb can install an APK copied out of WSL2. Unimplemented APIs are also
reported in the library's `pockethle-unimplemented.log`, controlled by the option
in Emulator Settings. `.github/workflows/android.yml` supports manual GitHub
Actions builds after integration; no remote workflow was started here.

## Validation and limits

Historical packaged-build evidence (these earlier counts do not certify the
current checkout; rerun the commands above after changes):

- Kotlin/Java compilation, AAPT2 resources and D8 packaging with AndroidX.
- 240 viewport/native-size/rotation/scale combinations: exact integer scaling,
  Auto's largest fitting scale, inverse corner mapping and letterbox rejection.
- Configuration JSON preservation, fixed GPS, filter/scale settings and the
  fifteen SDK control codes, including Power as VK_F11.
- 28 Mesa OpenGL ES renders covering seven GPU modes in four rotations:
  shader compilation, reference three-pass SMAA FBO/lookups and RGBA corners.
  xBRZ uses the desktop Rust implementation.
- JNI Rust type checks, 38 pocket-library tests, XML/TOML/Python/Bash validation.
- Device contract tests: four kernel GPS and one GPS ABI test, six kernel camera
  and six camera ABI tests, and three Bluetooth serial tests. These exercise
  simulation and guest contracts, not physical Android sensors/radios.
- Official FFmpeg 8.0.1 download with SHA-256 verification.
- Release cross-builds of both native ABIs with NDK r28c, Unicorn and static
  FFmpeg; ELF, 16 KiB alignment and JIT symbol checks.
- Two targeted COREDLL `_wcsrev` tests.
- Gradle debug APK build, APK v2 signature verification and
  `zipalign -c -P 16 4`. Both packaged native ELF libraries were checked.

**Not covered by that evidence:** launch on a physical phone, Android Turf
Wars, or physical GPS/camera/Bluetooth tests. Build checks do not replace those
tests. The existing Android camera/GPS/Bluetooth bridge is retained. Windows-specific
Winsock RFCOMM and a general CLR/.NET CF runtime are not ported.

FFmpeg is built without GPL/nonfree components under LGPL 2.1+ terms. xBRZ is
GPL-3.0-only as on PC. License texts are included in assets. SMAA shaders and lookup
tables retain their MIT license. Public APK distribution must include the source
and reconstruction/relinking materials required by these licenses.

## Diagnostics and runner regression tests

Use the single [HARDWARE DIAG application](../tools/hardware-diag/README.md),
not old standalone RAMTEST/VFSTEST installations. It may relaunch its own EXE
for real process tests and needs private DLL/data fixtures; that does not make
it a menu of unrelated diagnostic executables.

```sh
python tools/hardware-diag/build.py
python tools/hardware-diag/run.py --guest
cargo test -p pocket-android-jni --no-default-features --features unicorn --lib
```

Follow that tool's current README for fixture generation. Keep native ABIs and
APK in sync. Run destructive quota/lifecycle probes in an isolated test library;
never erase a gameplay library to obtain a clean diagnostic result. A failed
probe may deliberately retain its private data for inspection.
Host ARM regressions do not certify APK, physical storage or sensor behavior.
