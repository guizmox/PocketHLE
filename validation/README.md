# Development validation artifacts

This directory retains harness sources, temporary-build helpers, logs, JSON
observations, images and diffs from earlier investigations. They are test
artifacts, not source modules automatically compiled into the emulator and not
current installation or patch instructions. Some refer to old revisions or
local tools; inspect those assumptions before reusing them. File names alone do
not establish a test date, host or PASS for the current revision.

Current operating instructions belong in [Build and release](../docs/BUILD.md),
[Graphics diagnostics](../docs/ANDROID-GRAPHICS-DIAGNOSTICS.md) and the
[HARDWARE DIAG manual](../tools/hardware-diag/README.md). Recorded conclusions and
limitations belong in [dated validation notes](../docs/testing/README.md) or
[historical validation](../docs/history/VALIDATION.md), with game captures under
[proof](../proof/README.md).

Do not delete these artifacts merely because they are absent from Cargo's
module graph: some are inputs to standalone reproduction scripts. No current
build command should silently depend on an undocumented local validation patch.
