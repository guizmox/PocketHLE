# PocketHLE UI context binding patch

Upstream: eframe 0.27.2, registry SHA-256
020e2ccef6bbcec71dbc542f7eed64a5846fc3076727f5746da8fd307c91bab2
Commit: 014327e36535deeca0839b6aca2646191f2bd2fb
Upstream Apache-2.0/MIT licenses are included unmodified.

Only src/native/glow_integration.rs changes upstream implementation code.
The root Cargo patch selects this exact version locally; dependency versions
and enabled features are unchanged. The standalone validation Cargo.lock is
not shipped or used by the PocketHLE workspace.

Stock change_gl_context unconditionally makes the current UI GL context
non-current and current again, twice during a normal single-viewport repaint.
Keyboard repeat and pointer motion request additional redraws. PocketHLE's
shared rendering context can run concurrently on the worker thread. This is a
concrete source of unnecessary driver transitions and a candidate for the
reported NVIDIA input-correlated stalls, not a measured final diagnosis.

Check actual native bindings before transitioning. On WGL compare HGLRC and
WindowFromDC for current draw and (when ARB available) read DCs. On other
backends combine context.is_current with surface.is_current: EGL/GLX surface
checks alone do not verify context identity. External callbacks and different
viewports must still rebind. No cached-current boolean is used.

No input filtering, key-repeat change, vsync/swap-interval change, frame cap,
GL error polling, readback, glFinish or shared-texture change is added.

Validation: real Linux EGL-device test covers 1000 unchanged repaints, another
surface and an external context on the same requested surface. Passed. Full
eframe library compiled on Linux with glow+x11. Three existing upstream
warnings (unused import/unreachable patterns/dead code) remain, see logs.
Windows helper also passes isolated metadata checking with API-shape stubs;
that is not a Windows glutin build/link or NVIDIA runtime test.

Linux validation command:
cargo test --manifest-path vendor/eframe/Cargo.toml --no-default-features --features glow,x11 input_repaint_keeps_current -- --ignored --test-threads=1

Full Windows build and reproducible input tests on GTX1650 remain necessary.
