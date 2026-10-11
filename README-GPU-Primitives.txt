PocketHLE - points and lines (software + experimental GPU)

Cumulative source patch; includes prior GPU fixes, vertex/matrix transforms,
shared lighting, Android FPS and brightness minimum at 20% dimming.

Implemented primitive modes: points, independent lines, line strips, line loops.
Float/fixed point size and line width setters now work, validate values and keep
state/query results. Sizes default to 1; software supports 1..64 pixels, GPU uses
its driver's aliased size ranges capped at 64. Requested size queries retain the
value supplied by the game even when rasterization must clamp it.

Software clips primitives before rasterization, uses square points and diamond-
exit lines, avoids duplicating shared collinear segment endpoints, and supports
flat/smooth color, perspective interpolation, textures, fog, alpha/depth tests,
blending, scissor and masks. GPU uses the existing shader/VBO/FBO and native point
and line drawing, with flat provoking attributes and appropriate point-size state.
Existing lighting and transforms apply; triangle culling does not suppress lines
or points. All resources retain their existing per-session ownership/teardown.

This fills a rendering gap. It does not promise a general FPS increase. Native
line-edge coverage can vary by driver, particularly for wide lines. Point sprites,
point-size arrays/attenuation and point/line smoothing remain separate features.

Validation: software rendering/API tests; real desktop GL and GLES2 tests for all
four modes, widths, colors and subsequent triangle state; existing GPU lighting,
matrix, texture/depth/blend and context-switch tests; Rust ARM64/ARMv7 checks.
Complete Windows build and Android APK/device checks remain for your toolchains.

Suggested game tests: compare software/GPU on scenes using particles or line
outlines, then verify an ordinary triangle-based 3D game and launcher transitions.
If a difference appears, report game, backend and a screenshot of the same scene.
