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

## Removing library games

Desktop and Android require confirmation before removing a game from the library,
for both PocketPC and Gizmondo. Desktop removal deletes the game's installed
files, saves and per-game settings. Dismissing the confirmation cancels removal.

## Desktop and Gizmondo controls

The desktop Gizmondo panel uses the PocketPC-style dark/translucent design,
not the photographed console skin. The five function controls stay above the
game: Home, Volume, Brightness, Geofence and Power. Geofence uses a GPS-style
symbol. Action buttons are circular in the Stop/Rewind/Forward/Play diamond;
L/R extend downward. Keep existing guest codes and customized bindings.
Brightness is not labelled Light; Geofence is not labelled Alert. A sixth host
EXIT button sits beside the five function controls, using the PocketPC style
and returning to the library; it does not send the guest Power key.

Desktop/Deck game-only fullscreen hides the virtual panel, including EXIT and
FULL. It does not change guest geometry. Do not reintroduce these controls from
an older panel-layout delivery note. Keep the configured FPS overlay discreet
inside the emulated image. Android touch controls are a different layout policy.
Recording overlays use Gizmondo names rather than PocketPC A/B/C/Action labels.

Every game-session exit restores the library viewport captured before launch,
including native game exit, host EXIT, shortcuts and an emulator error return.
Keep fullscreen and maximized state, and restore windowed size/outer position.
Wait for native fullscreen/maximize acknowledgements before applying geometry;
never resize a still-fullscreen surface to the library's windowed dimensions.
Discard game-only zoom/F12 restore sizes so they cannot override this snapshot.
Ending a game while its options are visible uses the same session cleanup. A
queued game-to-game launch retains the original library snapshot.

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

## Desktop recording under encoder load

Recording cadence is independent of the emulated game's FPS limit. Capture keeps
its selected display resolution and input overlays. Capture and muxing use the
same nearest 30/60 FPS time slots, so small early/late VSync jitter does not discard
otherwise on-time paints. Very fast Unlimited paints still produce at most one
capture per output slot; a pause does not trigger a burst of new captures.
The desktop MP4 path uses four conversion slice workers and four H.264 workers
(with four encoder slices). These stages execute sequentially, not as eight
simultaneous encoding tasks. Conversion borrows the input through refcounted
frame views without another full RGBA copy; odd dimensions retain the existing
edge-extension policy. When an MP4 capture is missed,
the writer advances audio and represents the held image through video timestamp
spacing. It does not re-encode the same pixels in every missing 30/60 FPS slot.
A final timestamped image preserves each segment's duration at stop or resize.
AVI retains its constant-rate MJPEG sample repetition. An overloaded encoder can
still capture fewer new images than the requested rate; the fix prevents redundant
catch-up work from causing an escalating backlog, without lowering resolution or
changing the game renderer.

HARDWARE DIAG's TIMETEST EGL pacing assertions require a 60 FPS per-game limit;
Unlimited/120 FPS are useful for performance benchmarks, not that contract test.

## Visual input editor — desktop and Android

Keyboard & controller settings use the same flat, unbranded keyboard/controller
skins on both frontends. Their PNGs, SVG originals and hitbox JSON are bundled
from `frontends/pocket-desktop/assets/input/`; Android's Gradle assets source
points directly at this directory. Neither artwork nor control geometry is
independently recreated on Android. Modifier-only keyboard keycaps are decorative.
Direction labels use ASCII words to avoid unsupported arrow glyphs.

Select a physical input to inspect, replace or clear its assignment, or capture
another input for a command. Blue indicates guest commands, green stylus, purple
host shortcuts and gray unassigned inputs. The controller also displays compact
live mappings on its actual controls. Selection highlights the control; unassigned
commands and desktop host-shortcut details are separate collapsible blocks.
The assignment inspector has its own panel rather than sharing an undifferentiated
text flow. The Gizmondo preview hides Pocket PC stylus settings.

Pocket PC / Gizmondo changes the preview only; saved bindings remain shared.
Per-game grids, button swaps and display rotation still apply during gameplay.
Each frontend displays its own real runtime priority: desktop built-in device and
host shortcuts retain priority, while Android reserves F10 for screenshots and
keeps its existing configurable gamepad mappings. This UI change does not impose
desktop's View/Menu/L1 shortcut policy on Android.

Android's native activity provides the same Keyboard / Controller sections,
Save/Cancel draft semantics, assignment colors, missing commands and physical
capture (keyboard, buttons, sticks, D-pad and triggers). The keyboard fits the
screen initially, with an enlarged horizontally scrollable mode for accurate
touch selection. Native input lists provide an accessible alternative to tapping
a diagram. Controller rows use touch-sized buttons; capture is cancelled when
the activity stops. Draft/profile/selection state survives recreation. Returning
to settings reloads the config before another preference can overwrite it.

Existing Android Back/Guide/LeftStick/RightStick click names accept desktop's
Select/Mode/LeftThumb/RightThumb aliases. Conflicting aliases are removed together
on reassignment, along with guest/stylus/configurable-fullscreen conflicts.
Assigning a stylus command enables it. The existing stylus speed and enable
preferences remain under Pocket PC settings.

## Library tile metadata

Desktop and Android tiles show resolution followed by a compact graphics API hint
(OpenGL ES, Direct3D Mobile, DirectDraw, GAPI, SDL or GDI). The shared detector
inspects the selected executable, resolves COREDLL ordinals through the same
bundled ARM/MIPS maps as runtime, and recognizes explicit graphics symbol strings
when GetProcAddress is used. GDI menu drawing does not override stronger 3D or
framebuffer evidence. SDL is identified as SDL without guessing its video backend.
This remains a static hint rather than proof of the active rendering path; no
DLL traversal, game-name rules, options or metadata migration are introduced.
Missing evidence leaves only resolution. Existing games benefit after restarting
the updated launcher, without reimport or a new Refresh button.

A second line, “Custom settings”, appears for saved per-game overrides or runtime
settings different from standard defaults. Explicit per-game overrides count
as custom even when their value currently equals the global setting. The CPU
baseline follows the global default; explicit Normal controls equal default
controls. Non-default screen/rotation settings count, including those selected
automatically during import. The marker describes the saved configuration, not a
history of user edits; restoring the defaults removes it.
