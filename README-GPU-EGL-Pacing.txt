PocketHLE - EGL window presentation cadence

Cumulative patch including typed vertex streams, anisotropic filtering and
previous GPU fixes. Shared Windows/Android implementation.

Fix
Battlestations runs too quickly in GPU mode. Previously eglSwapBuffers did not
limit guest execution: the frontend limited snapshots, but not game updates.
Window swaps now use the existing modeled 60 Hz DirectDraw display clock.
Both rendering backends obey the same cap. Offscreen rendering remains unpaced.
Waits are cooperative and preserve the guest call arguments. No game-specific
branch, audio-speed change, host driver vsync dependency or new resource/worker.

Test on your device
- Battlestations: compare menu navigation and gameplay speed with software.
- Check sound/input remain responsive and pause/resume behaves normally.
- Check another fast GPU game and Colors for visual/speed regressions.
- Games below 60 FPS are not accelerated by this change.

This patch caps window presentation; it does not emulate the original GPU's
processing cost or implement programmable EGL swap intervals greater than one.
Full Windows release/APK builds and Battlestations gameplay need device testing.

Automated validation
286 WinCE tests passed, including actual native GPU, software cadence, retry
argument/deadline preservation, offscreen exemption and existing worker waits.
Full Android WinCE cross-check could not complete: the available environment
lacks the NDK C compiler needed by rmp3. No full APK/Windows build claim.
