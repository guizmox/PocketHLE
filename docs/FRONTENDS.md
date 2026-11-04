# Frontends, controls and image geometry

This is the current UI policy reference. Platform build instructions remain in
[ANDROID-BUILD.md](ANDROID-BUILD.md) and [STEAMDECK.md](STEAMDECK.md).

## Shared settings

Windows/Linux use the same desktop source. Android mirrors the shared JSON
contract; changing an Android setting must not discard unrelated or unknown
fields. Device/platform settings, renderer choice and frame limits apply at the
next launch unless an individual live control explicitly says otherwise.

Host optimizations are permanently active; no user optimization switches remain.
See [OPTIMIZATIONS.md](OPTIMIZATIONS.md). GPU/software renderer selection is a
separate supported choice. Frame-limit inheritance and filters are defined in
[GPU.md](GPU.md), not duplicated here.

## Desktop and Gizmondo controls

The desktop Gizmondo panel uses the PocketPC-style dark/translucent design,
not the photographed console skin. The five function controls stay above the
game: Home, Volume, Brightness, Geofence and Power. Geofence uses a GPS-style
symbol. Action buttons are circular in the Stop/Rewind/Forward/Play diamond;
L/R extend downward. Keep existing guest codes and customized bindings.
Brightness is not labelled Light; Geofence is not labelled Alert.

Desktop/Deck game-only fullscreen hides the virtual panel, including EXIT and
FULL. It does not change guest geometry. Do not reintroduce these controls from
an older panel-layout delivery note. Keep the configured FPS overlay discreet
inside the emulated image. Android touch controls are a different layout policy.
Recording overlays use Gizmondo names rather than PocketPC A/B/C/Action labels.

## Image geometry and input

Use the produced image size for display fitting and the logical guest size for
input coordinates. Apply inverse rotation once, through the same display rectangle.
Internal GLES scaling must not change guest hitboxes or the emulated LCD size.
Prefer integer enlargement according to the platform's presentation policy;
when the produced image is too large, downscale proportionally rather than clip.
A 320x240 display grid, a native VGA image and an internal-resolution multiplier
must not be treated as interchangeable zoom definitions.

Android's current ordinary display rectangle uses the produced image and
integer enlargement when it fits; oversized images aspect-fit downward.
Its controller-fullscreen path has its own aspect-fit geometry. The former
statement that every VGA image first becomes a QVGA presentation grid, or must
clip at x1, is obsolete. Verify platform/layout-specific scale choices against
the shared geometry helpers and their tests.

Controls reserve their own area and do not overlap the touchable game image.
Big controls may reduce the fitted game scale. Do not copy a Gizmondo overflow
option onto the PocketPC touchscreen layout.

## Desktop input lifecycle

Native frame-ready notifications wake UI processing; coalesce notifications,
not button edges or axis values. Preserve source ownership/refcounts, release
ordering, focus loss, disconnect behavior and the latest-image mailbox contract.
Keyboard/mouse correction is hardware-confirmed for the reported Windows case;
controller/wheel/touch follow-ups have narrower validation (see GPU.md).
Menus/dialogues still need their ordinary explicit redraws and navigation.

## Android controls


Desktop and Android now have a dedicated **PocketPC options** menu, alongside
**Gizmondo options**. The global PocketPC processor and device identity fields
move there. Android's PocketPC-only stylus configuration also moves there.
Per-game device identity settings and their override behavior are unchanged.

Emulator Settings separates graphics, recording where supported, diagnostics
and miscellaneous settings. Host optimizations are fixed implementation behavior;
the former four user-selectable optimization switches have been removed.

Android's **Display and controls** has separate global Normal/Big choices for
PocketPC arrows and stylus stick, and for Gizmondo arrows. Gizmondo has a D-pad,
not a stylus stick. Big targets approximately a one-third enlargement. It is
clamped on shorter/narrower screens to fit the available space. Gizmondo reserves
the corresponding wider left bank before measuring the game surface. The action
diamond and shoulder buttons retain their sizes.

#### Control size and display fitting

Controls depend on Android viewport dimensions and density, not guest resolution.
Big controls reserve more space and can reduce the fitted game scale. Use the
current image/input geometry policy above; do not restore the former clipping
or mandatory VGA-to-QVGA reduction behavior.

#### PocketPC touch layouts

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

#### Mapping and steering previews

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

#### Compact Controller landscape banks

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

## Native service parity


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
