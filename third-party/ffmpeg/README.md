# Statically linked media decoding

The archive `ffmpeg-8.0.1.tar.xz` is the unmodified upstream source release:
https://ffmpeg.org/releases/ffmpeg-8.0.1.tar.xz

SHA-256: `05ee0b03119b45c0bdb4df654b96802e909e0a752f72e4fe3794f487229e5a41`

PocketHLE uses FFmpeg under LGPL-2.1-or-later. The source archive includes the
upstream copyright notices and all licenses; COPYING.LGPLv2.1 is also provided
here. The supplied build does not enable GPL, version3, nonfree or external
codec libraries. PocketHLE's own code retains its existing license.

## Windows x64 / MSVC

Install Visual Studio's C++ Build Tools (VS 2019 16.8 or newer) and MSYS2.
In the MSYS2 MSYS shell, install `make` and `diffutils`:

```
pacman -S --needed make diffutils
```

From the project root in cmd.exe:

```
powershell -ExecutionPolicy Bypass -File tools\build-ffmpeg.ps1
cargo build --release -p pocket-desktop
```

The script initializes MSVC when necessary, verifies/extracts the repository's
source archive and builds static libraries under
`target/native/ffmpeg/x86_64-pc-windows-msvc`. The build uses /MD to match Rust's
default Windows system CRT. No FFmpeg EXE or DLL is required at runtime.
The initial build takes time; subsequent Cargo builds reuse the libraries.
The script applies `tools/ffmpeg-msvc-locale.sed` to the extracted configure
script: MSVC compiler/linker detection must accept localized banners (French
Visual Studio puts translated words before "Microsoft"). The source archive
stays unmodified, and the patch is supplied for reproducibility/relinking.
Run the script again when its configuration or the FFmpeg source changes.
Custom MSYS2 path: `-MsysRoot D:\msys64`. Parallel build: `-Jobs 8`.

## Linux x64

GNU Make, Bash and a C compiler are required:

```
bash tools/build-ffmpeg.sh x86_64-unknown-linux-gnu 4
cargo build --release -p pocket-desktop
```

An alternative matching static prefix can be selected with
`POCKETHLE_FFMPEG_STATIC_DIR`. Import libraries are not acceptable: all five
FFmpeg libraries must be genuine static archives built with this configuration.
The bridge compiles against the prefix's own headers; it exposes no native
FFmpeg structures to Rust. Windows ARM64, MinGW, macOS and Android build scripts
are not supplied yet. Consumers with `--no-default-features` do not require the
native backend; RenderFile returns E_NOTIMPL instead of launching an executable.

## Supported media

Generic local-file decoding: ASF, AVI, MOV/MP4, MPEG PS/TS, Matroska, Ogg, WAV,
MP3; WMV1/2/3, VC-1, MPEG-1/2/4, MSMPEG4, H.263/H.264, MJPEG video and WMA1/2,
MP3, MP2, AAC, Vorbis, PCM and selected ADPCM audio. Unsupported codecs return
a playback error. No game-name checks. Encoders, network protocols, external
libraries, hardware decoders and assembly are omitted in this first build.
The default static backend is enabled by pocket-desktop's `video-static`
feature and can also be enabled on pocket-core/pocket-winceapi explicitly.

## Distribution and relinking

For binary releases using this static backend, supply the FFmpeg source archive,
license/notices, the exact build scripts and the corresponding PocketHLE source
and build instructions so recipients can rebuild and relink the executable
against a modified FFmpeg. Keep any local FFmpeg modifications with the release.
The FFmpeg dependency is LGPL; do not describe the complete combined executable
as containing only MIT/Apache code. See https://ffmpeg.org/legal.html and the
license text for the full terms. The decoder does not introduce any FFmpeg DLL
dependency; ordinary operating-system/runtime dependencies remain unchanged.
