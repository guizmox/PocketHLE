PocketHLE - shared fixed lighting and brightness tuning

Cumulative source patch: retains GPU fixes, GPU vertex transforms, Android FPS,
settings persistence/English labels and Android native build freshness checks.

Fixed lighting is implemented in BOTH software and experimental GPU rendering.
Eight lights, ambient/diffuse/specular/emission, shininess, directional/positional
lights, attenuation, spotlights, color-material, inverse-transpose normals,
normalize/rescale and two-sided lighting are supported. Float/fixed guest API
setters/getters validate arguments and report GL errors. Flat/smooth shading
is preserved. Existing context teardown owns native buffers/programs/textures;
lighting state adds no workers or separately allocated native resources.

This improves the appearance of 3D games that enable GL_LIGHTING. It is not an
FPS promise; software computes lighting per unique indexed vertex on the CPU.
Points/lines now render in both backends; see README-GPU-Primitives.txt.

Brightness minimum now dims by 20%: gray 128 becomes 103. Neutral/default and
higher positions remain unchanged. This is between the previous very dark
minimum and the overly subtle last iteration.

Validation: software GLES tests, real desktop GL/GLES2 lighting shader tests,
CPU/GPU color parity, guest API errors/queries/session state, framebuffer tests
and ARM64/ARMv7 Rust backend checks. Full Windows build and Android APK/device
validation require your toolchains. Test one lit 3D game with each backend and
compare surfaces/reflections; check one unlit game and all brightness steps.
