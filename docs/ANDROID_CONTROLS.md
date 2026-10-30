# Global options and Android directional controls

Desktop and Android now have a dedicated **PocketPC options** menu, alongside
**Gizmondo options**. The global PocketPC processor and device identity fields
move there. Android's PocketPC-only stylus configuration also moves there.
Per-game device identity settings and their override behavior are unchanged.

Emulator Settings separates common graphics, video recording (desktop),
diagnostics and miscellaneous settings. **Misc** contains Show FPS, Borderless
fullscreen (desktop) and Library Root. **Optimizations** is always last. Its
existing four settings and launch-only behavior are unchanged.

Android's **Display and controls** has separate global Normal/Big choices for
PocketPC arrows and stylus stick, and for Gizmondo arrows. Gizmondo has a D-pad,
not a stylus stick. Big targets approximately a one-third enlargement. It is
clamped on shorter/narrower screens to fit the available space. Gizmondo reserves
the corresponding wider left bank before measuring the game surface. The action
diamond and shoulder buttons retain their sizes.

## Display size policy (unchanged)

Control sizes depend on Android viewport dimensions and density, not the guest
framebuffer resolution. Increasing the controls reduces the space reserved for
the game. Rendering keeps the image's aspect ratio and chooses an integer scale
no larger than the requested scale or the space that fits. Big may therefore
reduce an effective scale from x3 to x2. If even x1 cannot fit, the existing policy
clips the image instead of applying fractional downscaling.

PocketPC VGA frames (640x480 or 480x640) use the existing QVGA presentation grid.
For example, a landscape VGA game is presented at 320x240 at x1, 640x480 at x2,
and 960x720 at x3, if space permits. Thus these labels are not always multiples
of the native framebuffer. PocketPC Auto is capped at x4 of that grid. Gizmondo
uses multiples of its native display. The separate controller fullscreen mode
uses aspect fit and can scale fractionally.

## PocketPC touch layouts

**PocketPC options → Touch controls → Touch layout** selects **Classic** or
**Controller** globally for Android. Classic remains the default for old and new
configurations. Changes apply at the next launch. Size remains a separate
Normal/Big choice in Display and controls; both layouts support it.

Controller uses the existing input IDs and functions with this arrangement:

* P1/P2 on the left and P3/P4 on the right share one top coordinate and height.
* The D-pad centre and action-diamond centre share one horizontal axis. B and
  Action lie on that axis; A and C lie equally above and below it.
* P1–P4 are raised together when needed to preserve the safety gap above the
  D-pad and diamond in Big mode. Action targets are larger than the early mockup,
  and retain their dimensions between Normal and Big.
* The stylus stick is smaller than the D-pad and sits diagonally below and to
  the left of the action diamond, with a gap between active circular hit
  targets. Transparent oval corners reject a new DOWN before input listeners
  run; accepted touches retain ownership through release or cancellation.
* The auxiliary toolbar stays separate, with smaller targets and fitted text.

Control dimensions and safety gaps use Android density units rather than
percentages of tablet height. Additional space goes to the game viewport rather
than stretching the control groups. Constrained windows scale the complete group
uniformly to fit; the fit scale is calculated for Big in both modes so changing
size does not also shrink P keys/action targets. Portrait reserves a bottom bank
and landscape reserves side banks, using the same game surface/input geometry.
This changes host layout only, not guest framebuffer dimensions or button maps.

Classic preserves the exact existing phone rectangles, including fractional
density and Normal/Big. On viewports whose smaller dimension is at least 600dp,
its landscape action banks have a bounded height and remain just above the thumb
controls; portrait action cells have a bounded width and stay in two side groups.
This avoids stretched tablet banks without changing the phone arrangement used
as the reference. These are density-based dimensions, not device-specific rules;
there is no Huawei model detection or workaround.

## Mapping and steering previews

Grid capture and touch-steering tests instantiate the real PocketPcControls and
PocketPcLayout. Both previews and GameActivity call the same `configure(config)`
method, which selects Classic/Controller and Normal/Big. No parallel preview
layout or native session is introduced. Input ownership, mapping names,
release behavior, and the single inverse game-image rotation remain unchanged.
The preview reads the global configuration when the editor is opened.

Validation covers all three landscape ratios, portrait, six display densities,
both layouts and both size modes: target bounds, non-overlap of hit rectangles,
P-row/action-axis alignment, safety gaps and bounded tablet group dimensions.
Phone snapshots verify unchanged Classic rectangles. Existing display scaling,
fullscreen input and multitouch-routing tests are retained. Rust and Android
configuration tests cover old files and independent layout/size persistence.
No full Android APK build or physical device validation is claimed.

## Compact Controller landscape banks

The landscape action diamond uses shorter offsets and the stylus stick moves
closer to it. Target diameters and P-key sizes remain unchanged on the reference
viewport. Outer margins and the game/control gutter are smaller, and the P row
can sit beside the central toolbar. Toolbar width is bounded by the gap between
the P banks. Portrait and Classic geometry are unchanged.

The reference viewport regression checks the same integer game-image scale as
Classic in Normal and Big. Automatic integer fitting remains unchanged on
smaller screens; no device-name or game-specific sizing path was introduced.
Oval action buttons and the stylus wrapper reject only new touches outside their
oval; ongoing MOVE/UP/CANCEL and split multitouch dispatch keep their existing
behavior. Mapping/steering previews use the same control view and geometry.
