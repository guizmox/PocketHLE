#!/usr/bin/env bash
set -euo pipefail
android_root="$(cd "$(dirname "$0")/.." && pwd)"
: "${ANDROID_HOME:?Set ANDROID_HOME to the Android SDK installed in Linux/WSL2 or macOS}"
android_gradle_version=8.10.2
android_gradle_dir="$android_root/target/tools/gradle-$android_gradle_version"
if [[ ! -x "$android_gradle_dir/bin/gradle" ]]; then
    mkdir -p "$android_root/target/tools"
    android_download="$android_root/target/tools/gradle-$android_gradle_version-bin.zip"
    curl --fail --location --retry 3 "https://services.gradle.org/distributions/gradle-$android_gradle_version-bin.zip" -o "$android_download"
    curl --fail --location --retry 3 "https://services.gradle.org/distributions/gradle-$android_gradle_version-bin.zip.sha256" -o "$android_download.sha256"
    printf '%s  %s\n' "$(tr -d '\r\n' < "$android_download.sha256")" "$android_download" | sha256sum -c -
    unzip -q -o "$android_download" -d "$android_root/target/tools"
fi
python3 "$android_root/tools/check-android-native.py"
"$android_gradle_dir/bin/gradle" -p "$android_root/frontends/pocket-android" --no-daemon assembleDebug
android_apk="$android_root/frontends/pocket-android/app/build/outputs/apk/debug/app-debug.apk"
"$ANDROID_HOME/build-tools/35.0.0/zipalign" -c -P 16 -v 4 "$android_apk"
"$ANDROID_HOME/build-tools/35.0.0/apksigner" verify "$android_apk"
echo "APK: $android_apk"
