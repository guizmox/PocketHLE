#!/usr/bin/env bash
set -euo pipefail
# Called by build-ffmpeg.ps1 with the MSVC environment inherited, or directly
# on Linux. All source code is supplied in the repository, not downloaded.
project_root="$(cd "$(dirname "$0")/.." && pwd)"
media_target="${1:-x86_64-unknown-linux-gnu}"
media_jobs="${2:-4}"
media_prefix="$project_root/target/native/ffmpeg/$media_target"
media_build="$project_root/target/native/ffmpeg-build/$media_target"
media_source="$project_root/target/native/ffmpeg-source/ffmpeg-8.0.1"
media_archive="$project_root/third-party/ffmpeg/ffmpeg-8.0.1.tar.xz"
mkdir -p "$media_build" "$media_prefix" "$(dirname "$media_source")"
printf '%s  %s\n' '05ee0b03119b45c0bdb4df654b96802e909e0a752f72e4fe3794f487229e5a41' "$media_archive" | sha256sum -c -
if [[ ! -f "$media_source/configure" ]]; then
    tar --no-same-owner -xf "$media_archive" -C "$(dirname "$media_source")"
fi
options=(
    --prefix="$media_prefix" --disable-programs --disable-doc --disable-debug
    --disable-shared --enable-static --disable-autodetect --disable-network
    --disable-asm --disable-everything --disable-avfilter --disable-avdevice --enable-avformat --enable-avcodec
    --enable-avutil --enable-swscale --enable-swresample --enable-protocol=file
    --enable-demuxer=asf,avi,mov,mpegps,mpegts,matroska,ogg,wav,mp3
    --enable-decoder=wmv1,wmv2,wmv3,vc1,mpeg4,msmpeg4v1,msmpeg4v2,msmpeg4v3,mpeg1video,mpeg2video,h263,h264,mjpeg,wmav1,wmav2,mp3float,mp2,aac,vorbis,pcm_s16le,pcm_u8,adpcm_ms,adpcm_ima_wav
    --enable-parsers --enable-pic
)
if [[ "$media_target" == x86_64-pc-windows-msvc ]]; then
    # Keep the vendored source archive unmodified; apply the reproducible
    # configure-only locale fix to the extracted working source instead.
    if [[ ! -f "$media_source/configure.upstream" ]]; then
        cp "$media_source/configure" "$media_source/configure.upstream"
    fi
    sed -f "$project_root/tools/ffmpeg-msvc-locale.sed" "$media_source/configure.upstream" > "$media_source/configure"
    # Rust's normal MSVC build uses the dynamic system CRT (/MD). FFmpeg
    # itself remains static; no FFmpeg DLLs or executables are produced.
    options+=(--toolchain=msvc --arch=x86_64 --target-os=win64 --extra-cflags=-MD)
elif [[ "$media_target" != x86_64-unknown-linux-gnu ]]; then
    echo "Unsupported native build target: $media_target" >&2
    exit 1
fi
cd "$media_build"
bash "$media_source/configure" "${options[@]}"
make -j "$media_jobs"
make install
if [[ "$media_target" == x86_64-pc-windows-msvc ]]; then
    # This upstream release names even MSVC COFF archives lib*.a. Give the
    # same static archives the names Rust's MSVC linker expects.
    for name in avformat avcodec avutil swscale swresample; do
        cp "$media_prefix/lib/lib$name.a" "$media_prefix/lib/$name.lib"
    done
fi
cp "$media_source/COPYING.LGPLv2.1" "$media_prefix/"
cp "$project_root/third-party/ffmpeg/README.md" "$media_prefix/BUILD-NOTES.md"
echo "Static FFmpeg installed in $media_prefix"
