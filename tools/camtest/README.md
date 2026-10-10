# CAMTEST — Gizmondo CAM1 camera

Import `dist/PocketHLE-CAMTEST.zip` into the library as a Gizmondo game.
Enable **Settings → Gizmondo options → Camera hardware (CAM1)** before launching.
Windows uses the first webcam; Android prefers the rear camera and requests CAMERA
permission. A missing, busy or denied camera produces a real error. Close other
applications using the camera.

The diagnostic executes ARM driver calls, checks the format, reads two previews
at up to 20 fps, captures an I420 image, then stops and closes the camera.
Results are stored in the library's `flash` folder:

- `CAMTEST.TXT`: expect `CAMTEST_RESULT PASS`.
- `CAMTEST-preview.bmp`: inspect the actual image; RGB565, 320×240, bottom-up
  rows and positive BMP height, as in the SDK. Use the updated diagnostic with
  the camera driver orientation fix.
- `CAMTEST-capture.i420`: Y/U/V 640×480, 460800 bytes.

Windows may display filenames in lowercase. PASS confirms the API calls and
capture; also inspect the BMP visually. To check device release, run the test
again, then open the Windows Camera app. On Android, also test pause/resume with
an application that uses a continuous preview: this diagnostic finishes and closes
its capture before displaying its final message.

To view the raw capture using an installed FFmpeg:

```text
ffmpeg -f rawvideo -pixel_format yuv420p -video_size 640x480 -i CAMTEST-capture.i420 -frames:v 1 CAMTEST-capture.png
```

The C source does not depend on the proprietary SDK. Rebuild with Python, Clang
and LLD available:

```text
python tools/camtest/build.py --clang clang --lld ld.lld
```

Original diagnostic validation: real ARM execution with Unicorn and a synthetic
camera, 15 successful checks, BMP dimensions and colors verified. This does not
establish Windows/Android hardware compatibility. Current Android APK build and
hardware test status are recorded in [the build guide](../../docs/ANDROID-BUILD.md).
