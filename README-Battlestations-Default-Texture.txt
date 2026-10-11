PocketHLE cumulative patch: default GLES texture zero

Fixes uploads, parameters and rendering of the GLES1 default texture object 0.
Battlestations cycles water textures 0..5; previously name 0 was untextured.
Applies to software and GPU on Windows and Android; no title-specific workaround.
Keeps all previous cumulative GPU, anisotropy, vertex and timing corrections.
Zero deletion is ignored; resources retain existing context/renderer ownership.

Validation: 157 GLES tests including real desktop GL/GLES2 drivers.
Full Windows/APK builds and game visual confirmation require target devices.
