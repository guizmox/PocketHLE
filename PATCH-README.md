# PocketPC startup, motion controls and Android layouts

Apply these replacement files to the `snapshots` branch at commit
`b24d80122d4dc7607dce69e1a8348f918b75a99d` (snapshots phase 6), preserving paths.
This package includes the previous PocketPC startup/framebuffer fixes and the
follow-up controls/MFC work. `patch-files.txt` lists every included path.
`remove-files.txt` is empty. No games, native runtime DLLs, diagnostic probes or
precompiled libraries are distributed in this source patch.

## Changes

- Preserve the generic DOS short-path VFS fix, snapshot ownership and file aliases.
- Separate automatic native fullscreen coverage from explicit shell show/hide
  requests. Brothers in Arms no longer pauses itself on a synthetic taskbar
  mismatch; MoonGBA's work-area window retains the shell strip.
- Coalesce partial GAPI framebuffer copies at existing presentation boundaries,
  preserving subsequent GDI painting and the established GPU/explicit GX paths.
- Add the opt-in PocketPC **Map Tab/Esc to F1/F2** setting on Desktop and Android.
  On-screen labels follow the setting. One shared Rust adapter maps both edges
  for keyboard, controller and virtual controls. No numeric-key compatibility
  workaround or game-name detection is added.
- Implement the CE3 ARM MFC CFile/CString/CGdiObject operations required by
  V-Rally through real VFS, heap and GDI objects. Add owned bitmap brushes,
  per-DC brush origins and palette-relative 8-bit CreateBitmap data. The supplied
  runtime DLLs were ABI evidence, not files required or bundled by this patch.
- Add per-game physical accelerometer sensitivity on both frontends for PocketPC
  and Gizmondo: Disabled / Low / Medium / High. PocketPC additionally selects
  Arrows or Stick. Stick reuses relative aiming without activating the visible
  cursor or grid assignments. Motion mapping and Tilt input are mutually
  exclusive. Existing simulated-tilt labels now explicitly say **with arrows**.
- Reuse SDL3 / Android physical sensors and one shared Rust motion converter,
  including calibration, rotation, filtering, hysteresis and input release.
- Reserve space around Android S/L buttons in Classic and Controller landscape
  layouts. In portrait, both PocketPC modes use the same compact footer geometry.
  Reduce the Android Gizmondo function bar's height by one third, scaling its
  icons and host-action text while preserving horizontal fit.

The audio preview change is a test correction; production audio is unchanged.
AYGSHELL ordinal 183 is identified as SHNavigateBack in diagnostics. No false
success handler is added for unsupported shell back-navigation.

The WinCE snapshot section is now **version 10** for shell/GDI state. Recreate
existing development snapshots. There is no legacy-format compatibility reader.
Host motion calibration and held commands are not archived guest state.

## Evidence and validation

See the startup and framebuffer reports in `docs/testing/`, the MFC ABI guide in
`docs/MFC.md`, the user controls in `docs/FRONTENDS.md` and rendering ownership in
`docs/GPU.md`. The actual BIA executable reached the first mission using mapped
Tab/F1; mapped Escape/F2 chose No and reached its main menu. V-Rally progressed
from the original null-vtable crash through language and car selection into an
actual race.
The scripts use ordinary guest messages and intact application assets.

Physical Windows/Android gameplay and physical sensor feel still require the
user's devices. Headless guest runs and synthetic sensor unit tests do not
establish hardware latency or a phone FPS result. Full build/test results for
this delivery are recorded in the startup report.
