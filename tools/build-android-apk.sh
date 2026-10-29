#!/usr/bin/env bash
set -euo pipefail
android_root="$(cd "$(dirname "$0")/.." && pwd)"
# Preserve the local/WSL workflow's app-debug.apk and persistent SDK debug key.
# GitHub Actions continues to produce release APKs with stable/unsigned signing.
android_build_type="${1:-${POCKETHLE_ANDROID_BUILD_TYPE:-}}"
if [[ -z "$android_build_type" ]]; then
    if [[ "${GITHUB_ACTIONS:-false}" == true ]]; then
        android_build_type=release
    else
        android_build_type=debug
    fi
fi
case "$android_build_type" in
    debug) android_gradle_task=assembleDebug;;
    release) android_gradle_task=assembleRelease;;
    *) echo 'Usage: bash tools/build-android-apk.sh [debug|release]' >&2; exit 2;;
esac
if [[ $# -gt 1 ]]; then
    echo 'Usage: bash tools/build-android-apk.sh [debug|release]' >&2
    exit 2
fi
: "${ANDROID_HOME:?Set ANDROID_HOME to the Android SDK installed in Linux/WSL2 or macOS}"
python3 "$android_root/tools/android-native-freshness.py" check
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
"$android_gradle_dir/bin/gradle" -p "$android_root/frontends/pocket-android" --no-daemon testDebugUnitTest "$android_gradle_task"
android_apk_dir="$android_root/frontends/pocket-android/app/build/outputs/apk/$android_build_type"
android_version="$(python3 - "$android_root/Cargo.toml" <<'PYVERSION'
import pathlib, re, sys
print(re.search(r'^version\s*=\s*"([^"]+)"', pathlib.Path(sys.argv[1]).read_text(), re.M)[1])
PYVERSION
)"
mkdir -p "$android_root/dist/android"
if [[ "$android_build_type" == debug ]]; then
    android_apk="$android_apk_dir/app-debug.apk"
    android_output="$android_root/dist/android/PocketHLE-$android_version-android-debug.apk"
    "$ANDROID_HOME/build-tools/35.0.0/apksigner" verify --verbose --print-certs "$android_apk"
    echo 'Local debug APK: signed with the persistent Android SDK debug key.'
elif [[ -n "${POCKETHLE_KEYSTORE:-}" ]]; then
    android_apk="$android_apk_dir/app-release.apk"
    android_output="$android_root/dist/android/PocketHLE-$android_version-android-universal.apk"
    "$ANDROID_HOME/build-tools/35.0.0/apksigner" verify --verbose --print-certs "$android_apk"
else
    android_apk="$android_apk_dir/app-release-unsigned.apk"
    android_output="$android_root/dist/android/PocketHLE-$android_version-android-universal-unsigned.apk"
    echo "No signing secrets: producing an explicitly unsigned APK. No temporary debug key is used."
fi
LD_LIBRARY_PATH="$ANDROID_HOME/build-tools/35.0.0/lib64${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
    "$ANDROID_HOME/build-tools/35.0.0/zipalign" -c -P 16 -v 4 "$android_apk"
cp "$android_apk" "$android_output"
echo "APK: $android_output"
echo "Gradle APK: $android_apk"
