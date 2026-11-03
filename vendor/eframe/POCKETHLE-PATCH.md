# PocketHLE temporary UI tracing only

Upstream eframe 0.27.2, original licenses preserved.
All custom WGL binding guards, extension queries and lookup caches are removed.
change_gl_context uses the original upstream body and WindowSurface signature.
run.rs is the original responsive loop. Only opt-in POCKETHLE_UI_TRACE CPU
wall-time scopes and event counters remain, with revision=latest-frame-v4.
No extra GL/WGL call, input filtering, scheduler, shared texture or fence change
is introduced here. Original context detach/attach behavior is restored.
Unused WGL trace variants/columns were removed after rollback.
Unused upstream DummyStorage, frame_start, and unused WinitApp trait methods
and implementations were removed; no global warning suppression was added.
