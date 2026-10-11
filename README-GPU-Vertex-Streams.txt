PocketHLE - reusable typed GPU vertex streams

Cumulative patch, including anisotropic filtering and all previous GPU fixes.
Shared implementation for Windows and Android. No additional setting required.

Changes
- Reuse guest-array read buffers and decoded index storage across draws.
- Merge adjacent/overlapping client-array reads; always refresh every draw.
- Smooth GPU draws upload typed positions, colors, normals and texture coordinates
  without constructing and repacking intermediate Vertex objects.
- Preserve normalized integer attributes and captured VBO bindings/updates.
- Remove interleaved padding from uploads; dense indices avoid hash-map remapping.
- Preserve existing fallback for sparse indices, flat shading and failed bulk reads.
- Software benefits from read-buffer reuse/coalescing; typed streams are GPU-only.
- No framebuffer readback/presentation changes or game-specific patch.

Measured operation reduction (synthetic Colors-style cube, not whole-game FPS):
8 vertices, fixed positions/colors, 36 indices: native payload 328 -> 264 bytes;
adjacent guest arrays require one bulk read instead of two.

Validation
155 GLES tests passed, including native GL/GLES, data mutation, VBO updates and
packed/typed path switching. Android ARM64/ARMv7 Rust GLES checks passed.
284 WinCE tests passed. Full Windows release/APK compilation and game testing
require target devices.

Suggested device check
Compare the same Colors scene with the previous patch in GPU mode. Also check
menus, lighting/textures, and another game in both software/GPU modes. Performance
gain depends on the proportion of frame time spent preparing geometry.
