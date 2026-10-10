PocketHLE — Gizmondo CAM1 camera on Windows and Android

Extract the complete source files into the repository root, replacing existing
files. The camera delivery preserves the preceding Bluetooth and SDL2 controller
fixes. Follow the patch's file list when merging it.

WINDOWS — from CMD after extraction:

cd /d C:\Users\gtristant\source\repos\PocketHLE
powershell -NoProfile -Command "Get-Content 'patch-files.txt' | ForEach-Object { (Get-Item -LiteralPath $_).LastWriteTime = Get-Date }"
set CMAKE_POLICY_VERSION_MINIMUM=3.5
cargo build --release -p pocket-desktop

Enable "Camera hardware (CAM1)" in the emulator/Gizmondo options before launching
a game or diagnostic. It is disabled by default. The setting permits access;
the game actually starts capture through CAM_START.
Windows uses the first webcam. Allow desktop applications to access the camera
in Windows Privacy settings if needed.

ARM TEST — import into the library:
tools/camtest/dist/PocketHLE-CAMTEST.zip

Launch CAMTEST with the camera enabled. It writes camtest.txt,
camtest-preview.bmp and camtest-capture.i420 into the library's flash directory.
Wait for CAMTEST_RESULT PASS and open the BMP to check the actual image.
Run it again to check camera release and reopening.
See tools/camtest/README.md for details and I420 conversion with FFmpeg.
The diagnostic replaces only its own CAMTEST output files.

ANDROID
=======
The sources include CAMERA permission handling, pause/resume and closing.
Enable the camera before launch; CAMERA permission is requested when needed.
The rear camera is preferred, otherwise the first available camera.
Build both native libraries and the APK using docs/ANDROID-BUILD.md, which covers
the pinned SDK/NDK/JDK/Gradle setup and static FFmpeg dependencies.
The debug APK is written to:
frontends/pocket-android/app/build/outputs/apk/debug/app-debug.apk

IMPLEMENTED CONTRACT
====================
CAM1: SETFORMAT, GETFORMAT, START, STOP, PREVIEW and CAPTURE.
Preview: top-down RGB565, dimensions divisible by eight up to 640x480, at most 20 fps.
Capture: 640x480 I420 (Y/U/V). Guest buffers are validated before frame consumption.
Deadlines survive scheduler retries; sharing and duplication are respected.
The last CloseHandle or STOP releases hardware capture.
An absent or denied camera does not produce a simulated image.
Undocumented IOCTLs 2107/2108/2109 and overlapped operations return unsupported.
Pocket PC DirectShow camera support and undocumented sensor controls are outside
this CAM1 driver's scope.

VALIDATION
==========
The original camera delivery reported 456 passing software tests
(151 kernel, 37 library, 245 WinCE API and 23 desktop).
ARM CAMTEST ran with Unicorn and a synthetic camera: 15 checks passed.
The 320x240 BMP had correct RGB565 colours and no vertical inversion.
Windows Media Foundation and Rust/JNI bindings were type-checked. In the original
Linux JNI integration check, the unavailable Android logger was omitted from a
temporary file; delivered sources were unchanged. Desktop checks used Unicorn
and CPAL; static FFmpeg was not rebuilt during that original validation.
The current full Android native/APK build has now succeeded, including FFmpeg.
See docs/ANDROID-BUILD.md for software checks and packaging validation.
Real Windows webcam and Android camera operation remain untested here.
I420 uses standard Y/U/V order; the Gizmondo SDK specifies YUV420 without
explicitly documenting physical hardware chroma plane order.

CAMTEST is an optional diagnostic that must be imported and launched manually.
It does not run automatically or enable additional game diagnostic logging.
