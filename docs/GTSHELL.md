# GTShell on Windows and Android

This fork adds the MSXML DOM used by GTShell's theme/language loader, correct
ARM entry detection for its supplied PMS DLLs, and independent SD Card mounts.
Both runners preserve the shell in Flash Disk2 while launching a game from
the selected card through SDCreateProcess.

The [Russian tutorial](GTSHELL-RU.md) covers the prepared local profile, Windows
launchers, Android installation, controls and troubleshooting.

Download the Android GTShell APK and Windows x64 package from
[the fork's releases](https://github.com/Katamaran21/PocketHLE-GTShell/releases).
Firmware, Microsoft DLLs and games are not included; create your own profile.

## Prepare your files

Use your complete `flash disk` and `flash disk2` folders, including MFCCE400,
PMS libraries and all shell assets. The tested GTShell executable has SHA-256
`8E8766071B38AAA0D304FB6105E137F657249E3626B0C57275CF014CFDF01367`.
The profile tool also detects this executable when it is named `GTShell0.exe`.

Create a portable library backup with Python 3.9+:

```sh
python tools/make-gtshell-profile.py --flash-disk "/path/flash disk" --flash-disk2 "/path/flash disk2" --sd-card "/path/card" --output "/path/GTShell-profile.zip"
```

Omit `--sd-card` for an empty card. `--gtshell` explicitly selects another ARM
executable. The tool copies bytes unchanged, preserves empty directories and
requires physical input folders. Generated profiles contain user-provided files
and are stored locally.

## Launch

Windows 10/11 x64: open `run-pockethle.cmd` from the release package, then
**Game library → Restore library backup (ZIP)…**. The library is kept alongside
the emulator in `library/`. Install the
[Visual C++ x64 runtime](https://aka.ms/vs/17/release/vc_redist.x64.exe) if
Windows reports a missing VCRUNTIME140.dll.
Android: install **PocketHLE GTShell**, then **⋮ → Restore library backup (ZIP)**.
Select the profile and **Merge (no overwrite)** for the first restore. Run the
GTShell card; choose English (UK)/Blue Steel on first setup, use Home for the
main menu and Play to confirm SD Card game selections.

The Android preview uses application ID `com.pockethle.app.gtshell`, Android 7+
and OpenGL ES 3, with both arm64-v8a and armeabi-v7a. Settings use the standard
English menus. Backups carry both Flash Disk folders and a library-relative
`settings.storage_card_dir`, so host drive letters are unnecessary.

## Build

Follow [ANDROID-BUILD.md](ANDROID-BUILD.md) to install the pinned Linux/WSL tools,
then build the native libraries and the separate GTShell APK:

```sh
bash tools/build-android-native.sh 4
POCKETHLE_GTSHELL_BUILD=1 bash tools/build-android-apk.sh debug
```

The GTShell variant can also be selected with Gradle's `-PpockethleGtshell=true`.
The normal application ID and APK names are preserved for standard builds.

On Windows, use the MSVC Rust toolchain, Visual Studio C++ Build Tools, CMake,
Ninja and libclang, then build the same GUI configuration as the preview:

```powershell
cargo build --release --locked -p pocket-desktop --bin pockethle-gui --no-default-features --features unicorn,audio-cpal
```

The prepared profile uses `max_slices=0` (unlimited), CPU Unicorn, 320×240,
software rendering and a read-only SD Card. Flash Disk is writable; Flash Disk2
writes use the game's normal save overlay. GPRS/GPS/camera/Bluetooth start
disabled in the archived configuration and can be changed in Gizmondo options.
Merge (no overwrite) retains an existing library's global settings.

The Android preview is signed with a persistent local Android SDK debug key.
No physical-phone GTShell session has been validated. The Windows preview
includes Unicorn and audio but is built without the static video decoder;
video playback has not been validated in that package.
