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

### Entry points and shared artwork

Settings has two entries: **Keyboard bindings** and **External controller
bindings**, on desktop and Android. Each opens only the selected input device's
screen. Connection status and controller shortcuts belong to the controller
screen. **Device profile** selects Pocket PC or Gizmondo guest assignments;
it does not change the platform of a library game.

Both screens use the same flat, unbranded skins. PNGs, editable SVG originals
and hitbox JSON live in `frontends/pocket-desktop/assets/input/`; Android's Gradle
assets source points directly at that directory. Artwork and geometry are not
recreated independently. Direction labels use ASCII words. Extra/Misc1 has a
visible three-dot button on the controller skin, separate from Menu.

The keyboard initially fits the Android screen and can be enlarged and scrolled
horizontally. Controller rows provide touch-sized selection buttons. Selecting
an input on the picture or a row changes only the read-only **Selected input**
summary. It shows the physical input and its available short/long actions.
There are no editable assignment dropdowns, Apply/Clear forms or target-first
capture dialogs. Their temporary state and duplicate capture routing have been
removed, along with the old stylus assignment preferences.

### One assignment workflow

**Assign an input…** is the sole binding editor. Start it once, press and release
a physical input, then choose the guest command. Releasing before 650 ms selects
the short assignment; holding at least 650 ms selects the long assignment.
Keyboard arrows or controller D-pad/left stick navigate. Enter or the physical
South/A button applies; Escape or East/B cancels the command choice. Android
Back can cancel the assistant. These navigation buttons are physical UI controls,
independent of their saved guest assignments.

After applying, **Assign another input** starts another capture, **Save and
finish** persists the draft, and **Back to overview** returns without saving.
This permits assignment and saving without another mouse/touch interaction.
The normal screen Save/Cancel actions remain. A reserved host input is rejected
and capture restarts with an explanation; it is not silently rebound.

Choose **Unassigned** after a short capture to clear that profile's short action,
or after a long capture to remove its long action. Removing a long action restores
immediate short behavior; it does not clear the short assignment. Short choices
also include stylus directions for Pocket PC inputs supported by the existing
stylus router. Long stylus actions are not offered. Inputs with a built-in guest
mapping that the stylus router prioritizes do not offer a stylus choice.
Stylus enable/speed remain separate settings. Gizmondo has no stylus choices.

**Reset keys to defaults** and **Reset controller defaults** are directly visible
in their respective screens. Reset restores the corresponding legacy guest
binding table and removes explicit short/long overrides for that input device
in both Pocket PC and Gizmondo profiles. Controller reset also restores the
additional fullscreen shortcut to View/Select. These actions do not reset the
stylus direction tables, enable state or speed. Reset edits the draft and still
requires Save. Android bindings are loaded when a game starts; relaunch a running
game after saving. Desktop physical routing reads the saved launcher config.

### Storage, precedence and runtime timing

The Deck layout is the common factory reference on desktop, dedicated Deck and
Android, including Pocket PC shoulder/face/thumb mappings and Gizmondo L1 volume.
`deck-input-defaults.json` is checked against Rust and Android defaults by tests.
Library loading migrates recognized legacy factory maps without rewriting custom
tables. Legacy keyboard/controller and stylus tables remain shared between device
profiles; new explicit guest overrides are profile-specific.

`short_press_bindings` and `long_press_bindings` are optional maps in the launcher
configuration (`config.json`, or `config-steamdeck.json` for the dedicated edition).
Keys are `pocketpc:keyboard:x` or `gizmondo:controller:South`, using canonical
punctuation/lowercase keyboard names and canonical SDL controller names. Short
values are guest button names, or null for explicit unbinding. Long values are
guest button names. Empty maps preserve existing behavior. Explicit guest overrides
preserve legacy tables; assigning a legacy stylus direction moves conflicting
legacy entries. Assigning stylus enables it. A conflicting optional fullscreen
shortcut is cleared when the input is assigned through the assistant.

Reserved host shortcuts retain priority; factory guest mappings are overridable.
In normal gameplay, explicit physical short/long mappings are consumed before the
per-game grid and ordinary stylus routes. Without an explicit override those
existing routes still apply. Guest direction rotation and Pocket PC button-group
swaps remain in the output path. Native text entry and context-dependent host
shortcuts retain their existing priority; this editor does not replace them.

A short-only binding emits immediate guest down/up and supports held movement.
With a long binding on the same input, short waits for release and emits an 80 ms
pulse. At 650 ms the long action goes down and stays down until release,
suppressing short. If the event loop first observes the deadline at release,
the long action emits an observable 80 ms pulse instead. Source refcounts preserve
simultaneous inputs. Auto-repeat does not restart hold timing. Focus loss and
disconnection cancel pending holds without generating a short action.

### Colors, physical feedback and host shortcuts

Blue marks guest commands, green stylus, purple host shortcuts, amber long presses
and gray unassigned inputs. Custom long actions appear in amber captions or
keycap indicators; the read-only summary gives their full name. Unassigned guest
commands are listed separately (collapsible on desktop, a read-only list on
Android). The controller-shortcut reference is separate from the summary.

Cyan borders with dark teal fill identify physical held inputs, independent of
assignment colors and mouse selection. Keys, buttons, stick directions and
triggers clear on release, focus loss or device removal. Desktop uses the selected
controller; Android combines connected sources and keeps key/axis channels
separate. Axis hysteresis avoids flicker. Modifier keycaps are not assignable but
can show logical modifier feedback, so both matching keycaps may light together.
Only OS-delivered keys can be shown. Outside the assistant, testing inputs does
not trigger guest/host actions or alter bindings. Inside it, inputs capture or
navigate the current step. Android capture is cancelled when the activity stops;
draft/profile/selection survive recreation, but an active capture is not resumed.
Returning to Android settings reloads saved config before another preference can
overwrite it.

View/Select taps fullscreen; its 650 ms hold is Gizmondo Brightness or Pocket PC
return to library. Menu/Start and Extra/Misc1 tap the next presentation filter
and hold to toggle recording. Guide/Mode holds to return to library on Pocket PC
only, when delivered by the OS. Extra has no standard Android key adapter and is
marked desktop-only. F10 screenshots and F12 fullscreen work on both frontends;
video recording/F11 remain desktop-only and Android labels them unavailable.
A host hold consumes its tap. Android filter changes run on the GL thread,
invalidate presentation upload and lazily initialize/reuse SMAA without changing
guest graphics state or geometry.

Android Back/Guide/LeftStick/RightStick names accept SDL's Select/Mode/LeftThumb/
RightThumb aliases. Gizmondo omits the legacy Turbo helper from choices and
missing-command reports; old Turbo bindings display as Brightness because both
send VK_F3. Pocket PC keeps its existing Turbo helper.

## Library tile metadata

Desktop and Android tiles show resolution followed by a compact graphics API hint
(OpenGL ES, Direct3D Mobile, DirectDraw, GAPI, GapiDraw, SDL or GDI). The shared detector
inspects the selected executable, resolves COREDLL ordinals through the same
bundled ARM/MIPS maps as runtime, and recognizes explicit graphics symbol strings
when GetProcAddress is used. GDI menu drawing does not override stronger 3D or
framebuffer evidence. SDL and GapiDraw are identified as middleware without guessing their video backend.
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
