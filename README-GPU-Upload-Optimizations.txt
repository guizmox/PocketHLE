PocketHLE - GPU upload optimizations (cumulative)

Included:
- Cache vertex attribute layouts and reuse uniform cache allocations.
- Replace per-draw texture-stage and guest-array descriptor allocations with
  fixed-size storage. Vertex transformation/flat-shading paths remain intact.
- Upload supported block-compressed texture payloads directly to native GL.
  Query each renderer's driver capabilities; optional entry points are not
  required for initialization. Preserve RGB DXT1 transparency compatibility.
- Defer CPU decoding of supported compressed uploads. Materialize RGBA for
  software rendering, GPU disable, subimage updates or native upload fallback.
- Generate missing mipmaps on the GPU when there are no authored guest levels.
  Preserve complete/partial authored chains; fill missing authored-chain levels
  through the existing CPU path. Missing functions or driver refusal use RGBA
  and CPU mip generation. Native allocation failures propagate to GPU fallback.
- Retain the previous cumulative fixes, GPU default setting and explicit
  per-game/software overrides. Shared renderer implementation: Windows/Android.

Lifetime: compressed payloads are owned by the texture and freed on replacement,
resize/deletion/context destruction. Native objects retain existing per-renderer
cleanup. There is no additional worker, persistent frame queue or instrumentation.

Validation:
150 GLES tests including real desktop GL/GLES2 paths and the existing benchmark;
284 WinCE unit tests including native presentation tests; Android ARM64 and ARMv7
pocket-gles Rust checks with warnings denied. New native tests cover compressed
uploads, deferred decoding, missing optional functions, driver refusal and mip
routing. Full Windows release and Android APK builds were not run here.

Expected benefit depends on texture formats, mip use and driver capabilities.
Palette/unsupported formats still decode on CPU. Native generated mip filtering
may differ slightly from the CPU box filter. No measured game FPS gain is claimed.
Synchronous frame readback, expanded flat shading and general vertex preparation
remain unchanged.

Test on device: compare GPU/software in Colors, Carmageddon and the four audited
 games, checking distant texture detail, alpha edges, offscreen views and repeated
 launches. Toggle GPU/software, change games and close the emulator normally.
