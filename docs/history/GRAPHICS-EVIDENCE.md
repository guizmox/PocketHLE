# Archived graphics validation excerpts

These excerpts were separated from the current graphics guide during the
20 November 2026 documentation audit. Their original run dates/revisions were
not all recorded; the audit date is **not** a new test date. Measurements and
pending validation describe the original investigations. Use [Graphics](../GPU.md)
for current rendering contracts and [the evidence index](../testing/README.md)
for dated campaigns. No native run was repeated during this documentation edit.

## Desktop input stutter

Windows GTX1650 keyboard validation reported about 60 received snapshots/s and
about 10 ms/s context-detach time, versus about 20 snapshots/s and 925 ms/s before.
These are one hardware reproduction and CPU wall-time observations, not a general
GPU benchmark. Intel UHD fullscreen also reproduced the original symptom.

## Android shared presentation

Local validation covers native GL/GLES pixel/backlight/row-order equivalence,
a producer on another thread, three consumer recreations with retained-image
readback, bounded mailboxes, static-image CPU recovery, wire decoding and the ARM
runner regressions. Android UI/factory Kotlin sources are type-checked against
Android API classes. A complete APK and Samsung/ANGLE runtime validation are
still required; this is not a measured device FPS guarantee or recovery from a
real device-wide EGL_CONTEXT_LOST event.

## Android packed images

Validation compares all 65,536 colors for backlight 1..31, neutral fallback
values and odd-width rows against integer CPU results through native ES3 on
Mesa. Snapshot tests verify immutable packed bytes/brightness and unchanged
logical/high-resolution geometry; protocol and xBRZ-expansion tests preserve
legacy packets. Full APK/device validation remains pending. This extension is
not a claim to have resolved the separately reported 8 FPS at 1x versus 20 FPS
at 2x; those runs still need their presentation/readback diagnostics correlated.

## RGB565 snapshot fixture correction

The previously reported RGB565 snapshot/reduction discrepancy was traced to a
malformed test fixture. It set the renderer's RGB565 policy, then replaced the
actual attachment with GL_RGBA storage before taking an RGB565 snapshot. The
reference and snapshot consequently started from different color precisions.
The fixture now uploads in the actual attachment format, checks the GL error,
and also covers 320x240 at 2x on desktop GL and GLES. No RGB565 production
reduction arithmetic was changed to make this test pass. The corrected fixture
also passes against the unmodified production code of commit 9bebfb5b.
