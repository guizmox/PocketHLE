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

## Proposed alternative PocketPC layout

The separate visual proposal, **Controller**, reorganizes the existing controls
into a conventional gamepad arrangement: D-pad on the left, four actions in a
right-hand diamond, a distinct lower-right stylus stick and P1–P4 above. It keeps
the existing button functions and separates guest keys from emulator tools.
It is a proposal awaiting validation, not an implemented or selectable option.
The current layout remains in use.

Validation: library configuration tests, pure Kotlin geometry/configuration
checks, display scaling and fullscreen input/geometry regression tests. No full
Android APK or desktop build is claimed.
