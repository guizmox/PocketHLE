Android FPS overlay

Enable or disable Show FPS under Emulator Settings. The existing show_fps JSON
setting is reused and remains persistent. Applies on the next game launch.
The small top-right game-area label updates once per second and counts new
frames drawn by the Android renderer, not emulated loop iterations or physical
screen refresh. Repaints and screenshots do not count as new frames.
It becomes 0 FPS when output stops changing; pause/exit hides it and resume
resets sampling. It does not intercept touch input or appear in screenshots.
No new timer, worker, native resource or per-frame logging.

Cumulative patch includes GPU vertex transforms, previous GPU/Android fixes and
minimum brightness tuning (20% toward black; neutral/default unchanged).
Validation: actual FPS Kotlin classes executed for identity/elapsed time/idle/
pause/reset; real JSON model persistence checked; actual Android renderer
compiled against framework classes with project neighbours stubbed; layout XML
validated. This is not a full APK build or physical-device test.
