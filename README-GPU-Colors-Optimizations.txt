PocketHLE - cumulative GPU / Colors optimizations

Apply all files at the repository root. This archive includes the previous GPU,
Carmageddon, lighting, vertices, primitives, Android FPS and brightness changes.
It contains source files and existing repository test fixtures, not a game binary.

Changes
- Native indexed drawing: identical complete GPU vertex records share a record
  and use a reusable unsigned-short element buffer. All flat-shading/provoking
  attributes remain in the key. Primitive expansion/winding remain unchanged.
  DrawArrays is retained when indexing increases transfer size or exceeds 65,536
  unique records. CPU packing still takes time; no guaranteed FPS gain is claimed.
- Guest arrays: bounded per-draw contiguous reads for dense position, colour,
  texture-coordinate and normal arrays. Interleaved stride is respected. VBOs
  retain their existing host-copy path. Sparse/overflowing/unreadable spans use
  the existing element reads. No cross-draw guest-data cache.
- Private-context cache of uniform values, native pipeline state and per-texture
  parameters. Clear/resize invalidate the pipeline; deletion drops texture
  parameters. Parent/child contexts own independent caches.
- glPolygonOffset and glPolygonOffsetx now drive triangle depth offset in both
  renderers. Native driver precision remains implementation-dependent; software
  uses the existing 16-bit depth declaration for the units term.
- glCopyTexImage2D copies framebuffer pixels with GL bottom-left coordinates,
  validates arguments and supports RGB/RGBA/alpha/luminance formats. Like the
  existing texture path, only base level is stored. Native pixels are synchronized
  before copying to the canonical image, so this operation still requires a GPU
  readback. glCopyTexSubImage2D and compressed subimages remain outside this patch.

Controlled Colors evidence
The attached executable's cube routine uses eight vertices and 36 indices.
The reproduced index sequence reduces attribute reads from 16 to 2 and native
vertex/index transfers from 3,456 to 840 bytes. These are per-draw operation and
payload reductions, not whole-game frame-rate measurements.

Validation
- GLES unit tests, API dispatch/stack-argument tests and native desktop GL/GLES2
  rendering tests, including alpha/depth compatibility, matrices, lighting,
  points/lines, indexed transfer size, state invalidation and context suspension.
- Rust cross-checks for Android ARM64 and ARMv7 with warnings denied.
- Full Windows build, Android NDK/APK build and complete Colors gameplay still
  require the user's machines and game data. No release profiling is added.

Recommended game test
Enable GPU rendering. Run Colors in the same scene and camera position as before,
compare FPS with the prior version, then check textures, shadows and menus.
Also briefly check Carmageddon, a lighting-heavy title, and a launcher round trip.
