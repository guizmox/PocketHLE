PocketHLE - main-thread Sleep / Battlestations water cadence

Cumulative patch including EGL 60 Hz pacing, typed vertex streams, anisotropic
filtering and all previous GPU fixes. Shared Windows/Android, software/GPU.

Finding
The game targets a 50 ms update budget (about 20 Hz) with Sleep. PocketHLE
honored positive worker sleeps but returned immediately for main-thread Sleep.
Its simulation counter drives the water's six caustic textures and vertex waves.
Fast rendering therefore made that animation and gameplay run too quickly.
The EGL 60 Hz cap alone does not honor a game's lower requested update rate.

Change
Positive main Sleep now retries cooperatively until its original monotonic
deadline. Ready workers, waveOut callbacks, input and stop remain serviced.
Sleep(0) and worker behavior are retained. No texture interpolation, modified
game assets, wake/spray fix or title-specific cap is introduced. No new native
resource/worker or temporary instrumentation.

Checks on the actual game binary
Timing ARM block requests 50/40/20/1 ms for elapsed 0/10/30/49 ms; no sleep
when late. The six supplied textures are readable. Running the game's ARM
DXT1 compressor produces no unexpected transparent pixels or anomalous frame
brightness. Visual improvement still requires testing in the emulator.

Suggested device check
- Water away from the boat: compare animation speed and abrupt transitions.
- Gameplay and menu speed, audio continuity and input responsiveness.
- Check another game, plus Stuntcar audio and launcher child/parent resume.
- About 20 FPS can be normal here because the game requests that cadence.
Full Windows/APK builds require target toolchains/devices.

Validation: 290 WinCE unit tests passed, including native GPU and software EGL,
finite/infinite/zero main sleeps, original deadlines and argument preservation,
worker handoff and nested waveOut callbacks with positive/zero Sleep. The GLES
implementation is unchanged from its previously validated 155-test version.
