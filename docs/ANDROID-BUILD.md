# PocketHLE — cumulative Android update

The source patch applies directly to `PocketHLE(3).zip`. It includes the initial
Android implementation and subsequent changes, with only added/modified files
listed in `patch-files.txt`; it contains no binaries or saves. Extract into the
source root, preserve directories and replace existing files. Existing
Colors/CRT/GPS fixes are retained.

## Interface and settings

- Portrait library: neutral SD card tiles, game names, Gizmondo/Pocket PC tabs.
  Each card has a ⋮ menu: Run, Rename, Settings, Remove. Rename preserves the
  game identifier, folders and saves.
- Main ⋮ → Settings: Emulator Settings, Display and controls, Gizmondo options,
  Keyboard and controllers. All settings remain outside gameplay. Interface text
  and project documentation are maintained in English.
- Immersive landscape gameplay: central image, controls and Exit. Stop, Rewind,
  Forward and Play are on the physical right; D-pad is on the physical left.
  L/R shoulders sit above their respective groups. The layout remains the same
  in both landscape orientations and is not mirrored for right-to-left locales.
- Five function buttons use the desktop skin's Home, Volume, Brightness, Geofence
  and Power symbols, mapped to guest F1/F2/F3/F4/F11. Power is not guest F5.
  The default PC keyboard may use host F5 to produce guest VK_F11.
- Gizmondo's native framebuffer remains 320×240, without rotation or stretching.
  Pocket PC retains its configured native dimensions and rotation.
- Auto chooses the largest integer scale that fits the available game area,
  excluding controls and system insets. Examples: ×1 = 320×240, ×2 = 640×480,
  ×3 = 960×720, ×4 = 1280×960. Auto can use a larger integer on larger screens.
  Explicit ×1/×2/×3/×4 choices are capped at the largest integer scale that fits.
  Remaining space is letterboxed. A viewport smaller than the native frame clips
  at ×1 rather than introducing fractional scaling. Scaling does not increase
  the emulated renderer's resolution.
- Filters: reconstruction, SMAA, SMAA Soft, xBRZ, bicubic, Lanczos, bilinear,
  nearest. Desktop shaders, reference three-pass SMAA and xbrz-rs 0.1.0 ×3
  preprocessing are used. GLES 3 is required.
- Physical keyboard F10 captures actual filtered/rotated GL pixels, excluding
  the UI/buttons. PNGs are stored in `library/screenshots/`.
- Gizmondo options include GPRS/data, Colors server and player ID, real or fixed
  GPS, Bluetooth and camera. Changes take effect on the next game launch.
  Fixed GPS does not require real-location permission; older Bluetooth APIs may
  independently require that permission. Latitude accepts −90..90 and longitude
  −180..180. Satellite counts are not fabricated.
- The existing WinINet bridge redirects Colors' fixed server domain. Supply a
  host, IP or HTTP(S) origin, for example `nas.local:8080`, `192.168.1.10:8080`
  or `http://192.168.1.10:8080`. On a phone, `localhost` means the phone itself,
  not the NAS. This Android update does not require a NAS server change.
- Gizmondo saves use `library/flash/`, mounted at `\Flash Disk\` as on PC.
  Colors identity, configuration and registry data remain persistent.

## Build from Windows using WSL2 / Ubuntu

Use a **Linux SDK/NDK in WSL2**, with Linux Java and Rust. Do not combine a Windows
NDK with Linux cargo. Initial installation requires network access. The scripts
download pinned Gradle and FFmpeg versions when absent and verify their hashes.
FFmpeg is linked statically; no ffmpeg.exe or DLL needs to be copied.

From the source root in Windows CMD, refresh patched file timestamps:

```cmd
powershell -NoProfile -Command "Get-Content 'patch-files.txt' | ForEach-Object { (Get-Item -LiteralPath $_).LastWriteTime = Get-Date }"
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

Completed software/build checks:

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

**Not executed in this environment:** launch on a physical phone, Android Turf
Wars, or physical GPS/camera/Bluetooth tests. Build checks do not replace those
tests. The existing Android camera/GPS/Bluetooth bridge is retained. Windows-specific
Winsock RFCOMM and a general CLR/.NET CF runtime are not ported.

FFmpeg is built without GPL/nonfree components under LGPL 2.1+ terms. xBRZ is
GPL-3.0-only as on PC. License texts are included in assets. SMAA shaders and lookup
tables retain their MIT license. Public APK distribution must include the source
and reconstruction/relinking materials required by these licenses.
