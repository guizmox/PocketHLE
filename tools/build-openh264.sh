#!/usr/bin/env bash
set -euo pipefail
project_root="$(cd "$(dirname "$0")/.." && pwd)"
media_target="${1:-x86_64-unknown-linux-gnu}"
media_jobs="${2:-4}"
media_prefix="$project_root/target/native/ffmpeg/$media_target"
h264_archive="$project_root/third-party/openh264/openh264-2.6.0.tar.gz"
h264_build="$project_root/target/native/openh264-build/$media_target"
case "$media_target" in
    x86_64-pc-windows-msvc) h264_os=msvc; h264_library=openh264.lib;;
    x86_64-unknown-linux-gnu) h264_os=linux; h264_library=libopenh264.a;;
    *) echo "Unsupported OpenH264 build target: $media_target" >&2; exit 1;;
esac
printf '%s  %s\n' '4d35ba2c0427906984423fb0b52acbe26ca66605f404846fb2d8ee03a34c47ca' "$h264_archive" | sha256sum -c -
mkdir -p "$h264_build" "$media_prefix"
if [[ ! -f "$h264_build/Makefile" ]]; then
    tar --no-same-owner -xf "$h264_archive" --strip-components=1 -C "$h264_build"
fi
h264_options=(OS="$h264_os" ARCH=x86_64 USE_ASM=No BUILDTYPE=Release PREFIX="$media_prefix")
if [[ "$h264_os" == msvc ]]; then
    h264_options+=(CFLAGS_OPT='-O2 -Ob1 -MD -FS -DNDEBUG')
fi
# Build only the static archive, not demos, tests, shared libraries or DLLs.
make -C "$h264_build" -j "$media_jobs" "${h264_options[@]}" "$h264_library"
make -C "$h264_build" "${h264_options[@]}" install-static
cp "$project_root/third-party/openh264/LICENSE" "$media_prefix/OPENH264-LICENSE"
echo "Static OpenH264 installed in $media_prefix"
