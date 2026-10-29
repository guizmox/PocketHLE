# Blocking main message pump and worker wakeups

GetMessageW blocks the calling thread until a message exists; it does not stop
other guest threads. After processing already-due messages and attempting the
normal worker handoff, a main pump with parked live workers retries through the
existing one-millisecond wait_display_until continuation. Keep the original
MSG pointer, filters, LR and blocked-call semantics. Do not return FALSE or
invent a message merely to yield. Actual due messages keep their existing
priority, and worker readiness/quantum rules remain unchanged. The existing
single-thread fallback is unchanged. State belongs to the guest process.

Previously the fallback slept until synthetic paint (up to16ms). Worker sleep,
waveOut/event readiness could therefore wait for unrelated paint traffic.

## Local diagnostic comparison (2026-10-29)

Two user-provided Tomb Raider saves, same executable and tr.opt, QVGA240x320,
Unicorn/software guest rasterizer, unlimited user cap. Separate data copies;
18-second warmup then10-second idle samples; sequential headless Linux runs,
without host audio-cpal. Temporary instrumentation was removed from production.
These are local measurements, not Windows/Android device results.

| Scene | Completed GXEndDraw/s before | Diagnostic bounded-wait/s | Changed images/s before | Diagnostic bounded-wait/s |
| --- | ---: | ---: | ---: | ---: |
| Initial position |31.8|57.2|25.0|25.0|
| Water position |32.2|57.4|32.0|53.7|

The diagnostic variant used a1ms sleep followed by retry of the same main thunk;
production reuses wait_display_until for that continuation. The new regression
checks worker eligibility, original MSG pointer preservation and actual WM_QUIT.
The gameplay impact while moving and physical host audio remain user validation.
A greater presentation rate can expose games coupling simulation to frame rate;
per-game user frame caps remain unchanged.

## Independent presentation rate

Framebuffer.presentation_stats is a shared, process-owned rate sampler. Successful
GXEndDraw, primary DirectDraw Unlock/Flip, full retained GAPI framebuffer copies
and onscreen eglSwapBuffers record a completion, including identical frames.
Scheduler retries, offscreen EGL swaps and ordinary readback polls do not count.
The shared handle survives presentation-only framebuffer clones; snapshots expose
telemetry without additional pixel transfers. It becomes idle after a second
without completion and resets on GXCloseDisplay. Existing frame caps and pixel
revision semantics remain unchanged.

Desktop/Deck/Linux display completed presentations as FPS; Android polls the
same sampler through JNI, independently of nativePollFrame. Without an explicit
boundary (ordinary GDI primitives or retained GAPI pointer polls), the frontends
retain the old changed-image publication rate and label it IPS. A universal
application frame rate cannot be inferred from arbitrary GDI drawing commands.
No fake vblank or implicit frame boundary is added. The FPS option controls both.

Regression tests cover identical pixels, rate/idle/reset, shared snapshot handles,
GX scheduler retries and identical presentations, EGL swaps/retries and the
blocking-message worker continuation. Native Windows/Android builds and gameplay
with moving camera/audio remain platform validation.
