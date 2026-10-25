# OpenH264 for desktop recording

Cisco OpenH264 2.6.0, BSD license in LICENSE. Pinned source archive from
https://github.com/cisco/openh264/releases/tag/v2.6.0
Upstream full archive SHA-256:
558544ad358283a7ab2930d69a9ceddf913f4a51ee9bf1bfb9e377322af81a69
The bundled archive omits only res/ and testbin/ sample media; source files are unchanged.
Bundled archive SHA-256: 4d35ba2c0427906984423fb0b52acbe26ca66605f404846fb2d8ee03a34c47ca

Built statically by tools/build-openh264.sh, called by tools/build-ffmpeg.sh.
Desktop only. Windows uses the MSVC dynamic CRT (/MD), matching Rust and FFmpeg.
The source build disables assembly so no NASM installation is required.
No OpenH264 or FFmpeg DLL is needed at runtime.
