PocketHLE 09 - Desktop PocketPC UI restoration
Cumulative: includes lots 01-08 R3 and their ownership fix.
Reference: j92580498-max/PocketHLE, commit eea7ac6.
PocketPC restores the original framebuffer + side controls; Gizmondo is unchanged.
Current shared settings, fullscreen, filters, rotation and controllers remain available.
Windows and Linux share this frontend. Android is unchanged from lot08 R3.
Extract over your repository. Apply removed-files.txt, refresh patch-files.txt timestamps, build.
Check portrait/landscape, rotation + stylus, held arrows, Turbo and fullscreen.
Validation: actual restored methods typechecked, formatting passed, Gizmondo function unchanged.
No complete Windows executable or physical UI test was performed here.

R2: remove obsolete PocketPcLayout source/module and replace its shell test with side-pad rotation coverage. No dead-code warning suppression.
