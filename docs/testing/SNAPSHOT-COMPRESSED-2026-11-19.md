# Shared managed sessions and native compressed texture checkpoints

Base: `d1552d659f5a6b14d02a52ceb5528022dd08422c` (`snapshots`).
The checkpoint schemas and versions do not change in this patch.

Desktop and Android execute managed applications through `pocket-netcf` and use
`pocket-session` for Save/Restore coordination. Real external-asset testing uses
Diamond Beer without modifying its executable or game data.

## Native compressed storage

The host-ES current-block mirror understands RGTC1/2, LATC1/2 and BPTC in addition
to the existing S3TC, ATC, ETC/EAC and ASTC layouts. Both signed and unsigned
variants use their specified block sizes. Only calls accepted by the host driver
update the mirror. No extension is newly advertised to the guest by this patch.
The snapshot path neither decodes nor recompresses these blocks.

Accepted zero-area compressed updates leave the image and its capture validity
unchanged, including formats whose partial block layout is unknown. They do not
repair a previously invalid mirror. Storage-size arithmetic is checked for
32-bit hosts before any slice copies.

Whole-image opaque replacement remains supported. Unknown nonempty partial
layouts and compressed mip states not representable by the GLES2 contract still
fail capture explicitly; no byte-count guess, fabricated texture or silent
CPU-format conversion is introduced.

References:

- https://registry.khronos.org/OpenGL/extensions/EXT/EXT_texture_compression_rgtc.txt
- https://registry.khronos.org/OpenGL/extensions/EXT/EXT_texture_compression_latc.txt
- https://registry.khronos.org/OpenGL/extensions/EXT/EXT_texture_compression_bptc.txt

## Validation

Rust 1.90.0, Linux Mesa llvmpipe, `EGL_PLATFORM=surfaceless`,
`GALLIUM_DRIVER=llvmpipe`, `LP_NUM_THREADS=2`.

```sh
cargo test -p pocket-winceapi --lib gles2::renderer -- --include-ignored --test-threads=1
cargo check -p pocket-android-jni -p pocket-desktop --no-default-features --features unicorn
```

Both commands passed. The renderer suite executed 23 tests without failures or
skips. Its new host-ES regression uploads native compressed images, changes a
vertical block strip and cold-restores twice. Compressed checkpoint bytes must
match exactly after each generation, and the native uploads/restorations must
raise no GL error. The test checks the host exposes at least one tested format;
it cannot succeed by skipping all formats. This compares native compressed data,
not decoded screenshot pixels.

Five block-layout unit tests also pass, including empty updates, signed/unsigned
channel storage, non-square ASTC and preservation of untouched blocks. These are
host-driver correctness checks, not measurements on a phone or Windows GPU.

The two public JNI Session tests passed on Diamond Beer: two Save/Restore cycles
through the common managed interpreter, and refusal of truncated/incompatible
slots while the live game continues publishing frames. The fresh run observed
Save acknowledgements around 0.20–0.44 s and Restore around 0.30–0.70 s on this
host. These figures are not Android device performance measurements.

Desktop/JNI checks with and without Unicorn passed. `cargo fmt --all -- --check`
and `git diff --check` passed. Android managed execution enters `pocket-netcf`.

Android release libraries compiled for `arm64-v8a` and `armeabi-v7a` from the same
final source fingerprint. The native verifier and freshness check passed for both
ABIs, including ELF dependencies, static FFmpeg, JIT builtins and 16 KiB alignment.
Windows and physical Android gameplay are not validated by these host runs.

Android `assembleDebug` passed with `verifyNativeLibraries` enabled. The APK
was assembled for validation; the delivery contains source files only.
