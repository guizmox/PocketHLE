PocketHLE - Guest mipmap maximum level (cumulative)

RBR sets GL_TEXTURE_MAX_LEVEL (0x813D) to its supplied mip count minus one.
PocketHLE previously accepted this parameter without retaining/applying it.
The native GPU could use generated levels beyond the game's limit. The game
also sets LOD bias -0.5; this bias was already correctly handled.

Retain max_level per guest texture (default 1000), invalidating native uploads
when changed. Desktop GL, GLES3 and GLES2 APPLE_texture_max_level use the native
parameter. Level zero uses matching base-only filtering, avoiding extra mip work.
Other GLES2 devices with OES_standard_derivatives cap the mip bias using a
screen-space derivative estimate. This is an approximation of the driver's
implicit LOD calculation. Very old GLES2 without either capability uses base-only
filtering for restricted textures; no sampling beyond the cap occurs, but distant
textures can shimmer. Unrestricted textures retain their previous filtering.

Changing/growing/removing limits and changing filters refreshes the existing
native cache. Authored levels are retained and no extra image copy/worker/native
object is kept. Texture deletion, per-surface caches and software fallback keep
their existing ownership. Software still samples its base image. Negative and
nonfinite float maximum-level values report GL_INVALID_VALUE without mutation.

Validation: 151 GLES tests including real GL/GLES2, extended mip tests cover caps
0/1/default with nearest and trilinear filters, native and forced GLES2 shader
routes, removing/restoring limits and no-derivatives fallback; WinCE wrapper tests
cover float/fixed setters and invalid values. 284 WinCE tests and both Android
ARM64/ARMv7 Rust GLES checks pass. Full Windows/APK builds not run here.

Includes the previous depth precision fix and all upload optimizations. Retest
RBR terrain/tree detail and moving dark patches; the two fixes address separate
paths. Actual game visual improvement still needs device confirmation. No global
sharpening bias or forced removal of mip filtering has been applied.
