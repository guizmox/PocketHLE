# Statically linked media

The archive `ffmpeg-8.0.1.tar.xz` is the unmodified upstream source release:
https://ffmpeg.org/releases/ffmpeg-8.0.1.tar.xz

SHA-256: `05ee0b03119b45c0bdb4df654b96802e909e0a752f72e4fe3794f487229e5a41`

PocketHLE uses FFmpeg under LGPL-2.1-or-later. The source archive contains upstream
notices/licenses; COPYING.LGPLv2.1 is also provided here. PocketHLE's own license
is separate. Do not describe the combined executable as containing only MIT/Apache
code. The authoritative license texts are unchanged.

## Build and runtime

Follow [Build and release](../../docs/BUILD.md) for Windows/MSVC, Linux and Android.
`tools/build-ffmpeg.sh` and its Windows PowerShell wrapper verify the source hash,
apply the reproducible MSVC configure-locale fix to the extracted working copy,
and build static libraries. No FFmpeg executable or DLL is required at runtime.
Windows uses /MD, matching Rust and OpenH264. Prefixes live under
`target/native/ffmpeg/<target>`; `POCKETHLE_FFMPEG_STATIC_DIR` can select an
alternate compatible static prefix. Import libraries are not static archives.

The bridge uses that prefix's headers and exposes an opaque C interface, not
FFmpeg structure layouts, to Rust. The `video-static` feature selects this backend.
No-default-feature consumers do not gain it implicitly. macOS, Windows ARM64 and
MinGW are not supplied build targets.

## Capabilities

The shared build supports selected ASF, AVI, MOV/MP4, MPEG PS/TS, Matroska, Ogg,
WAV and MP3 inputs through the decoder/demuxer list in `tools/build-ffmpeg.sh`.
It includes WMV/VC-1, MPEG video, H.263/H.264, MJPEG and selected WMA/MP3/AAC/
Vorbis/PCM/ADPCM decoding. DirectShow metadata and seeking use the same bridge.
Unsupported formats return a playback error; there are no game-name exceptions.

Desktop builds additionally link [OpenH264](../openh264/README.md) and AAC encoding
with MOV/MP4 muxing for recording. Android remains decoder-only. Network protocols,
assembly, GPL/nonfree components and hardware codecs are not enabled by this build;
static OpenH264 is a deliberate desktop dependency, not a claim of no external
codec libraries.

## Distribution and relinking

Supply the corresponding FFmpeg/PocketHLE sources, notices, exact build scripts
and any local changes so recipients can reconstruct and relink against modified
FFmpeg. Preserve the OpenH264 and other bundled dependency notices as applicable.
See https://ffmpeg.org/legal.html and the actual license texts for terms.
