# Windows / Linux desktop parity

Both platforms build the same pocket-desktop binary source (eframe/egui), use
identical Gizmondo and Pocket PC layouts, library tabs, import policies, GPU
options, filters, screenshots, game settings, information panels and credits.
SDL controller handling and software/GPU geometry code are shared.

The simulated GPS controls previously had a Windows-only cfg gate. The gate
is removed from the UI and simulated position setup: manual and Eiffel Tower
fixes are now available on Linux as well. Host GPS consent remains Windows-only
because Linux has no native location provider in this tree. Host Bluetooth,
camera, battery and Internet implementations depend on the platform; identical
settings do not imply those native services have Linux parity.

Build with the same default Cargo features on both systems. A Linux build with
--no-default-features disables real CPU, audio and static video: it is not the
same functional release. Linux file dialogs use GTK3; display/input is X11 or
Wayland. Full desktop UI and physical native devices require device testing.
No actual Windows/Linux visual comparison has been performed in this package.
