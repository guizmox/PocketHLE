# Build, signing and releases

Build from a source checkout, using **Rust 1.90.0** as in CI. Runtime settings
belong to [Frontends](FRONTENDS.md), renderer policy to [Graphics](GPU.md), and
Deck installation to [Steam Deck](STEAMDECK.md). Do not apply historical delivery
instructions to a current checkout.

- [Linux desktop](#linux-desktop)
- [Windows desktop](#windows-desktop)
- [Android / WSL2](#android)
- [Android release signing](#android-release-signing)
- [Validation](#validation)
- [Release checks and publishing](#release-checks)

## Linux desktop

```bash
# Use Rust 1.90.0, matching the CI checks
rustup toolchain install 1.90.0
rustup override set 1.90.0

# Native dependencies for the desktop frontend
sudo apt install -y cmake build-essential pkg-config libclang-dev \
  libgtk-3-dev libxkbcommon-dev libwayland-dev libx11-dev libxcb1-dev \
  libxrandr-dev libxinerama-dev libxi-dev libxcursor-dev \
  libxdamage-dev libxext-dev libxfixes-dev libxss-dev libasound2-dev libssl-dev clang lld python3

# Prepare the default static video dependency
bash tools/build-ffmpeg.sh x86_64-unknown-linux-gnu 4

# Build and test the workspace
cargo build --release --locked --workspace
cargo test --locked --workspace -- --test-threads=1

# Build the real CPU backend
cargo build --release --locked -p pocket-cli --features unicorn
cargo build --release --locked -p pocket-desktop --features unicorn
```

If `glib-sys` reports missing `glib-2.0.pc`, install `pkg-config libgtk-3-dev`.
GTK is retained by the native startup-error/help fallback; library file pickers
are launcher-owned. Do not disable GPU or CPU features to hide a missing dependency. For an Android-only build, use the Android scripts in [the Android section](#android),
which select `pocket-android-jni` rather than the desktop workspace.

The binaries are written to `target/release/`:

* `pockethle` — command-line frontend;
* `pockethle-gui` — desktop launcher.


## Windows desktop

Install Rust's x86_64 MSVC toolchain, Visual Studio C++ Build Tools (VS 2019
16.8 or newer), CMake and MSYS2. In an MSYS2 MSYS terminal, install the utilities
used by the static media build:

```sh
pacman -S --needed make diffutils pkgconf
```

From PowerShell at the repository root:

```powershell
rustup toolchain install 1.90.0 --profile minimal
rustup override set 1.90.0
powershell -ExecutionPolicy Bypass -File tools/build-ffmpeg.ps1 -Jobs 4
cargo build --release --locked -p pocket-cli
cargo build --release --locked -p pocket-desktop
```

`tools/build-ffmpeg.ps1` initializes the x64 MSVC environment when needed; its
`-MsysRoot` argument selects a non-default MSYS2 installation. The output is
`target/release/pockethle.exe` and `pockethle-gui.exe`, unless `CARGO_TARGET_DIR`
selects another directory (CI uses `target/cargo`). No FFmpeg/OpenH264 DLL is
required. Windows/Proton use the same package, with `--steam-deck` selecting the
runtime Deck profile; there is no second Deck build.

## Android

### Linux / WSL2 prerequisites and packaging

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
in Emulator Settings. The manual `.github/workflows/android.yml` and main CI workflow use the same
Android scripts. Local debug packaging and public CI signing have different
policies; see [Android release signing](#android-release-signing).


### Static media and distribution

FFmpeg is built without GPL/nonfree components under LGPL 2.1+ terms. xBRZ is
GPL-3.0-only as on PC. License texts are included in assets. SMAA shaders and lookup
tables retain their MIT license. Public APK distribution must include the source
and reconstruction/relinking materials required by these licenses.

Desktop builds add static OpenH264 and AAC encoding for MP4 recording; Android
uses the decoder-only configuration. Source provenance and redistribution notices
remain alongside [FFmpeg](../third-party/ffmpeg/README.md) and
[OpenH264](../third-party/openh264/README.md). Preserve the corresponding sources,
licenses and reconstruction/relinking materials when distributing binaries.

## Android release signing

For public releases use one permanent release key. On Windows, run
`tools/setup-android-signing.bat` with its PowerShell companion beside it. Java
`keytool` is required; the helper can discover Android Studio's Java or a suitable
JDK. The generated key and secret-value files live under
`%USERPROFILE%/.pockethle/android-signing/`. Back up that entire directory privately,
outside Git. Never regenerate a key already used for a published application.

Configure these four repository Actions secrets with the values produced by the
helper; it guides clipboard entry but does not upload or verify GitHub secrets:

- `POCKETHLE_KEYSTORE_BASE64`
- `POCKETHLE_KEYSTORE_PASSWORD`
- `POCKETHLE_KEY_ALIAS`
- `POCKETHLE_KEY_PASSWORD`

The helper reuses a complete signing directory and rejects an incomplete one
without replacing it. CI restores the keystore with
`tools/prepare-android-signing.sh`; public push/manual builds fail when the
required secrets are absent. PR checks may produce explicitly unsigned APKs;
do not publish those files. See the workflows and script guards for exact modes.

Local builds remain debug by default and retain `~/.android/debug.keystore`.
A different signing key cannot update an existing installation in place. Before
an intentional uninstall/reinstall, back up the complete library outside the
application/prefix, then restore it after installation. Losing or replacing a
release key is not a normal update procedure. No secret belongs in a patch,
repository, documentation attachment or public artifact.

## Validation

After source changes, run the appropriate checks with the pinned toolchain:

```sh
cargo +1.90.0 fmt --all -- --check
cargo test --locked --workspace -- --test-threads=1
```

Native dependencies and the static media prefix are required for the workspace
build/tests. Android packaging runs its unit/resource/ABI/signature/alignment
checks through `tools/build-android-apk.sh`. These are distinct from testing an
installed app on physical Windows/Deck/Android hardware.

Use [HARDWARE DIAG](../tools/hardware-diag/README.md) for fixture generation and
opt-in executable ARM suites. The `arm-diagnostics` feature also enables Unicorn;
ordinary workspace/CI tests do not execute those fixture-dependent suites. CI
still publishes HARDWARE-DIAG and retains ordinary unit tests and native recording
validation. Never erase a gameplay library to run destructive quota/lifecycle
probes. Historical counts are in [the validation archive](history/VALIDATION.md),
not a certification of the current checkout.

## Release checks

The desktop launcher (Windows/Linux and the runtime Steam Deck profile) and
Android catalogue check the latest published stable release of
`guizmox/PocketHLE` once at startup. They use the public GitHub Releases API,
not Actions artifacts or commits. No GitHub account/token is required.

Checks run off the UI thread, with network timeouts and a 1 MiB response cap.
No release, invalid data, API rate limits and offline/TLS failures are silent.
HTTPS certificate validation remains enabled. Desktop supports environment
proxies and system certificate roots, retaining public roots for Proton hosts
whose Windows store is empty. No game/catalogue data is sent or modified.

Only a strictly newer semantic version is offered. Tags use `v0.3.3` or
`0.3.3`; drafts, prereleases and non-version tags are ignored. Build metadata
does not change precedence. Desktop compares the workspace Cargo version.
Android's generated `BuildConfig.UPDATE_VERSION` and displayed `versionName`
both read that same version from the root Cargo.toml. `versionCode` is a separate
monotonically increasing Android package number; it must also increase for a
new distributed APK.

On desktop, the prompt waits until the catalogue is available, outside imports,
gameplay and other launch prompts. Download opens the system browser; Later
dismisses it for this launch. Enter/controller South downloads and Escape/controller East dismisses.
The UI shows the active controller's reported labels, including on Deck. Android uses a dismissible catalogue Snackbar
with Download, deferred until the activity is resumed and imports are finished.
Activity recreation reuses the process result instead of starting more requests.

The matching release asset is selected when unique:

| Edition | Release asset |
| --- | --- |
| Windows PC and Windows under Proton with --steam-deck | Windows ZIP |
| Linux, either launch profile | Linux ZIP or tar.gz |
| Android | APK |

Absent or ambiguous assets open that release's page. Download URLs must belong
to this repository's GitHub release downloads. There is no background package
download, automatic installation, EXE replacement or APK installation request.

### Publishing an update

1. Increase `[workspace.package].version` in root Cargo.toml and refresh the
   workspace-package versions in Cargo.lock without upgrading unrelated dependencies.
   For Android distribution also increase `versionCode`; `versionName` and
   `BuildConfig.UPDATE_VERSION` derive from the root Cargo version.
2. Build and validate the packages from that commit with CI.
3. Publish a stable GitHub Release with the matching tag, for example `v0.3.3`.
   Attach the Windows and Linux packages and the Android APK as
   release assets. Actions artifacts alone do not trigger a release notification.

The checker does not publish releases or change repository settings. An installed
version must not offer an equal-version tag as an update. The published release
set is external state; this guide does not assert which tag is currently deployed.

### Release-check validation

Rust and Android share `crates/pocket-library/tests/data/releases.json`, covering
14 version and asset cases. Rust also tests platform selection and Linux package
formats. Android runs its JUnit cases in `tools/build-android-apk.sh` before APK
assembly; the normal CI already invokes that script. Full native packaging and
physical-device/browser interaction remain checks for the target builds.
