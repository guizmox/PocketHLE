PocketHLE - GPU depth precision (cumulative)

The native renderer previously forced DEPTH_COMPONENT16 on every host. The
software renderer retains f32 window depth. Closely spaced terrain/decal layers
can therefore collapse to one depth value on the GPU, especially at distance.

Prefer DEPTH_COMPONENT24 on desktop GL, GLES3 and GLES2 with GL_OES_depth24.
Keep DEPTH_COMPONENT16 on other GLES2 devices. If a driver advertises 24-bit
support but rejects the attachment or yields an incomplete framebuffer, retry
16-bit. Native allocation failures propagate to the existing GPU fallback.
The attachment is resized and destroyed with the existing renderbuffer; no
additional buffer or worker is retained. Applies to window and offscreen targets
on both Windows and Android.

Validation: 151 GLES tests including real native desktop GL/GLES2; the new test
shows close depth layers collapse in 16-bit but separate in 24-bit and match
software, including attachment resizing. Android ARM64/ARMv7 Rust GLES checks
and 284 WinCE tests pass. Complete Windows/APK builds were not run here.

This addresses a demonstrated precision gap. The exact cause of Richard Burns
Rally's moving dark patches still requires an on-device game test. Compare the
same course/camera with the preceding cumulative and this patch.

Texture filtering is unchanged. Software currently samples the base level,
whereas GPU rendering honors mip filters and LOD bias, which can look smoother
or blurrier at distance. These are separate from depth-layer conflicts.
Includes all previous upload optimizations and cumulative fixes.
