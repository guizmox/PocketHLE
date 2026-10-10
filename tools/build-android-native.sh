#!/usr/bin/env bash
set -euo pipefail
android_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$android_root"
: "${ANDROID_NDK_HOME:?Set ANDROID_NDK_HOME to Android NDK 28.2.13676358}"
android_jobs="${1:-4}"
command -v cargo >/dev/null
command -v cargo-ndk >/dev/null || { echo 'Install cargo-ndk: cargo install cargo-ndk --version 3.5.4 --locked' >&2; exit 1; }
command -v cmake >/dev/null
command -v pkg-config >/dev/null || { echo 'Install pkg-config (required by Unicorn/QEMU configure)' >&2; exit 1; }
rustup target add aarch64-linux-android armv7-linux-androideabi
case "$(uname -s)" in
    Linux) android_host=linux-x86_64;;
    Darwin) android_host=darwin-x86_64;;
    *) echo 'Android native builds require Linux/WSL2 or macOS' >&2; exit 1;;
esac
android_sysroot="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$android_host/sysroot"
[[ -d "$android_sysroot/usr/include" ]] || { echo "Missing NDK sysroot: $android_sysroot" >&2; exit 1; }
for android_pair in 'aarch64-linux-android:arm64-v8a' 'armv7-linux-androideabi:armeabi-v7a'; do
    android_target="${android_pair%%:*}"
    android_abi="${android_pair#*:}"
    bash tools/build-ffmpeg.sh "$android_target" "$android_jobs"
    android_envkey="CMAKE_TOOLCHAIN_FILE_${android_target//-/_}"
    android_rustflags="CARGO_TARGET_${android_target^^}_RUSTFLAGS"
    android_rustflags="${android_rustflags//-/_}"
    # Bindgen uses host libclang, which otherwise finds Linux libc headers.
    # Target-specific arguments keep Android types/headers scoped to this ABI.
    case "$android_target" in
        aarch64-linux-android) android_clang_target=aarch64-linux-android24; android_header_triple=aarch64-linux-android;;
        armv7-linux-androideabi) android_clang_target=armv7a-linux-androideabi24; android_header_triple=arm-linux-androideabi;;
    esac
    android_bindgen_key="BINDGEN_EXTRA_CLANG_ARGS_${android_target//-/_}"
    android_bindgen_args="--target=$android_clang_target --sysroot=\"$android_sysroot\" -isystem \"$android_sysroot/usr/include\" -isystem \"$android_sysroot/usr/include/$android_header_triple\""
    # Global RUSTFLAGS takes precedence over target flags in Cargo. Merge it
    # into the target flags, then unset it so CI's -D warnings keeps alignment.
    env -u RUSTFLAGS "$android_envkey=$android_root/build-support/cmake/$android_target.cmake" \
        "$android_bindgen_key=$android_bindgen_args" \
        "$android_rustflags=${RUSTFLAGS:-} -C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-z,common-page-size=16384" \
        POCKETHLE_FFMPEG_STATIC_DIR="$android_root/target/native/ffmpeg/$android_target" \
        cargo ndk -t "$android_abi" -p 24 -o frontends/pocket-android/app/src/main/jniLibs \
        build --release --locked -p pocket-android-jni
    cp "$android_root/target/native/ffmpeg/$android_target/COPYING.LGPLv2.1" \
        frontends/pocket-android/app/src/main/assets/licenses/FFmpeg-LGPL.txt
    # Upstream Cargo registry carries the xBRZ license. Ship it with the Android assets.
    android_registry="${CARGO_HOME:-$HOME/.cargo}/registry/src"
    if [[ -d "$android_registry" ]]; then
        while IFS= read -r android_license; do
            cp "$android_license" frontends/pocket-android/app/src/main/assets/licenses/xBRZ-LICENSE.txt
            break
        done < <(find "$android_registry" -path '*/xbrz-rs-0.1.0/LICENSE*' -type f)
    fi
 done
python3 tools/check-android-native.py
