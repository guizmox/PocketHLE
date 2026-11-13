# Frontends, controls and image geometry

Current settings and input policy for desktop, Deck and Android. Build/signing
instructions belong to [Build and release](BUILD.md); Deck installation belongs
to [Steam Deck](STEAMDECK.md). Graphics execution belongs to [Graphics](GPU.md).

- [Launch profiles](#desktop-runtime-launch-profiles) and [toolbar](#desktop-button-toolbar)
- [Library data](#adding-data-to-an-installed-game), [removal](#removing-library-games) and [captures](#desktop-capture-gallery)
- [Image fitting](#image-geometry-and-input) and [Android touch controls](#android-controls)
- [Factory bindings](#factory-input-profiles) and [assignment editor](#visual-input-editor--desktop-and-android)
- [PocketPC accelerometer](#pocketpc-accelerometer)
- [Stylus modes](#pocketpc-right-stick-stylus-modes) and [Center tap](#center-tap)
- [Focus and dialogs](#desktop-focus-and-import-launch-settings-prompt)

## Shared settings

Windows/Linux use the same desktop source. Android mirrors the shared JSON
contract; changing an Android setting must not discard unrelated or unknown
fields. Device/platform settings, renderer choice and frame limits apply at the
next launch unless an individual live control explicitly says otherwise.

Host optimizations are permanently active; no user optimization switches remain.
See [always-on execution optimizations](AGENTS.md#always-on-execution-optimizations).
In the global graphics settings, **GPU rendering** selects host OpenGL or the
PocketHLE-Reborn software rasterizer for GLES 1.x. For host OpenGL on GLES 1.x and 2.0,
**OpenGL ES internal resolution (host OpenGL)** offers **Native**, **2×**, **3×**
and **4×**. Enable **Apply to all resolutions, including sizes other than
320×240** to extend the chosen multiplier beyond 320×240 and 240×320 surfaces.
The per-game **GPU compatibility profile** controls the guest API, and
**Generic OpenGL ES (2.0, Common)** uses host OpenGL. A fresh Pocket PC import
selects it when the executable imports `libGLESv2.dll`. See [GPU.md](GPU.md)
for frame-limit inheritance and filters.

PocketPC options holds the global PocketPC processor/device identity fields;
Gizmondo options holds its hardware activations. Stylus speed remains in the
input editors. Emulator Settings separates graphics, recording where supported,
diagnostics and miscellaneous settings.

Per-game Graphics settings list **Screen**, **Rotate display**, then **Frame rate
limit**, with an explanation under each option on desktop and Android (Screen
remains hidden for Gizmondo on Android). Screen changes the dimensions reported
to the game and applies at the next launch. WQVGA is available as 240x400
portrait or 400x240 landscape. Rotation turns the presented image and
adapts touch/directional input without changing those dimensions. These
settings are independent. Frame-limit inheritance and its existing help remain
unchanged.

## First-import setup

After the first successful import of a new platform, a one-time assistant offers
its global defaults. A multi-game ZIP or demo card opens one assistant, not one
per title. Existing libraries and updates to installed games do not restart it.
**Keep current settings** completes setup without changing the current choices.
The `first_import_setup.gizmondo` and `first_import_setup.pocket_pc` configuration
flags remember completion independently, including after games are removed.

Desktop and Android offer Gizmondo supersampling and camera, Bluetooth, GPS and
Internet/data bridges, initialized from the saved configuration. Supersampling
uses the global OpenGL ES setting. Enabling it from **Native** selects **2×**
for 320×240 and 240×320 surfaces and enables **GPU rendering**. An existing
multiplier and resolution scope are preserved. The same global options apply
to Pocket PC games using the GLES 1.x host OpenGL path. Hardware choices
remain subject to host permissions.

Android additionally offers Gizmondo Normal/Big directional controls and control
overlap. **Big controls can reduce the game image size.** Allowing overlap keeps
a larger game image, but controls may cover it. PocketPC instead keeps controls
separate from the game image and offers Classic/Controller layouts and Normal/Big
sizes. Its preview can be switched between portrait and landscape without
changing the device orientation. Previews use the actual touch layout components;
no game runs during setup. The existing PocketPC per-game display advice follows
this assistant where applicable.

Saving merges these choices into the latest profile configuration and preserves
unrelated and unknown settings; game executables and library entries are unchanged.

## Desktop runtime launch profiles

One `pockethle-gui` executable supports the standard profile and `--steam-deck`.
There is no Steam Deck Cargo feature or second main file. The argument is parsed
before opening the library, initializing SDL or selecting updates. A runtime
LauncherProfile belongs to each Library handle, including clones, staging
imports, background reload/import jobs and the game runner/children. It chooses
config.json or config-steamdeck.json without process-global state; catalogue and
per-game manifests remain shared. CLI and Android callers of Library::open keep
the Standard profile. Backups continue to include both existing settings files.

Both desktop profiles start windowed at 1280x800 physical pixels, capped to the
monitor; only an explicit positional CLI display argument of 0 requests fullscreen.
Under Wine/Proton, the launcher preserves its existing XInput hints. Native Linux is not forced
to use XInput. Windows Deck-profile updates under Proton choose the Windows archive; native
Linux chooses the Linux archive. There is no separate Deck package or update
platform. Colors controller help is shown whenever a controller is
connected, independent of launch profile. The unused layout argument and old
Deck-specific migration functions/tests are removed. See STEAMDECK.md for Steam
shortcut setup; old executable targets must be changed to pockethle-gui.exe with
--steam-deck. There is no executable-name compatibility shim.

## Desktop button toolbar

Menu/Start remains the immutable filter/recording shortcut; it does not open
settings. Use the Settings toolbar group, with L3 to switch between toolbar and
page. There is no separate Start-to-settings action.

The former Game library, Settings and About dropdown hierarchy is replaced by
direct buttons in four separated groups: Game library, Settings, Features,
About. Each group has at most two rows; small windows scroll horizontally
instead of shrinking labels. Startup uses the fixed 1280x800 outer-window policy
above; the old toolbar-overflow startup widening and its state/helpers are removed.
Controller focus reveals the selected button.
Directions use button geometry within the toolbar; L3 switches between toolbar
and page, and A activates the focused button. The footer lists these controls.
The former F6 section-switch shortcut, interception and help were removed.
F6 remains assignable and retains the existing in-game Colors GPS action.

Features contains Upscale ×2, Filter, Rotation, Fullscreen, Screenshot and
Recording. Filter and Rotation advance their existing carousels and display
the current mode; F9/F7 use the same actions. Upscale/fullscreen show their
active state. Recording shows a red Stop video button while active, and a
disabled Saving video state while encoding. Captures remain game-only and
backup/restore remain disabled while a game/import is active. Filter changes
are synchronized into any open settings draft before saving.

About is a dedicated page retaining version, project links and acknowledgments.
Back/B/Escape return to the originating page without discarding its draft.
Return to game remains available above the buttons while a session runs behind
a settings page. Fullscreen gameplay still hides host chrome. Legacy toolbar
menus, nested-menu expansion state and their obsolete tests were removed;
settings combo boxes retain their popup navigation. Android's settings layout
is unchanged by this desktop-only toolbar.

## Adding data to an installed game

Every PocketPC and Gizmondo game has **Add/Replace Data** in its library tile
overflow menu on desktop and Android. The file picker accepts any single file. Non-ZIP files are
copied with their original filename to `games/<id>/extracted/`. For ZIP files,
the user chooses **Extract contents** or **Copy ZIP unchanged**. Extraction keeps
the archive's directory structure, including wrapper directories; it does not
reimport the game or guess an installation layout.

Preparation reads into a private staging directory, leaving installed content
untouched. Existing directories are merged; unrelated contents remain. Before
any installed file is replaced, a **Yes / Cancel** confirmation lists conflicting
paths, including file/directory type conflicts. Cancel abandons the entire
addition, including non-conflicting files. Replacements use the existing host
filename's casing to respect the guest's case-insensitive filesystem on Linux
and Android as well as Windows. Unsafe paths, duplicate ZIP names, symbolic
links and special files are rejected. A changed conflict set requires a fresh
selection rather than extending prior overwrite consent.

If a copied file is an EXE, or an extracted ZIP contains EXEs, a separate choice
allows **Keep current executable** or selection of one added EXE (with its full
relative path). Keeping the current EXE does not cancel the data addition. A ZIP
copied unchanged does not offer its internal EXEs. Choosing a replacement updates
both `game.json` and `library.json` after the data is published. Files and manifest
changes share rollback on an operation error; rollback failure retains recovery
files and reports their location. This is not a crash/power-loss transaction.

All choices, Cancel and the final notice support keyboard and external controller
navigation, without consulting gameplay bindings. Android uses native focus plus
controller D-pad/left-stick navigation and confirm/back input; desktop uses the
launcher-owned picker and controller widgets. Reserved host shortcuts still work.
After success, a modal notice displays the exact destination directory. Android
retains the pending choice across Activity recreation and releases staged data
when it is cancelled or the library Activity is finished.

Data additions are refused while a game session is running or stopping. Android
holds the session gate during preparation and commit and checks again after a
confirmation; desktop prevents starting a game during the operation. Existing
saves, settings and catalogue entries are retained. This action changes game data
only at the user's request; it is not an emulator compatibility workaround.

## Removing library games

Desktop and Android require confirmation before removing a game from the library,
for both PocketPC and Gizmondo. Desktop removal deletes the game's installed
files, saves and per-game settings. Dismissing the confirmation cancels removal.
Removal is refused while game sessions are running or stopping; Android holds
the existing session gate through the filesystem operation.

Desktop uses an egui-owned, centred confirmation. South/Enter confirms;
East/Escape or the close button cancels, with cancellation winning simultaneous
inputs. Face-button markings use the shared SDL3 label provider. Library actions
and navigation repeat do not reach the page behind the confirmation. Only the
confirmed action calls `Library::remove`; no native dialog takes controller focus.

## Desktop capture gallery

The Recordings tab lists screenshots and videos. From the selected tile,
**South opens** and **East requests deletion** (A/B on Deck); face-button help
uses SDL3 labels from the active controller. The former R3 deletion shortcut is
removed. Native sharing, where available, remains on North. Mouse Delete and
controller deletion open the same confirmation, initially selecting Cancel.
Left selects Cancel, right selects Delete, South accepts the selected choice,
and East cancels. Repeated directional input does not toggle the selection.
In the viewer, East still leaves fullscreen or closes playback; it never deletes.

## Library tile metadata

Desktop and Android tiles show resolution followed by a compact graphics API hint
(OpenGL ES, Direct3D Mobile, DirectDraw, GAPI, GapiDraw, SDL or GDI). The shared detector
inspects the selected executable, resolves COREDLL ordinals through the bundled
ARM/MIPS maps, and recognizes explicit graphics symbols used with
`GetProcAddress`. If the executable has no rendering evidence beyond GDI,
the detector also checks local DLLs referenced by imports or explicit names
in programs that call `LoadLibrary`. A cabinet DLL with a unique matching
filename alias can supply the hint. Unrelated files in the same directory are
excluded. SDL and GapiDraw are shown as middleware when detected.

The label describes static API evidence. A DLL that supports several rendering
paths can contribute a label before the game chooses its active path. Missing
evidence leaves only resolution. Existing games benefit after restarting the
updated launcher, with their library entries and settings intact.

A second line, “Custom settings”, appears for saved per-game overrides or runtime
settings different from standard defaults. Explicit per-game overrides count
as custom even when their value currently equals the global setting. The CPU
baseline follows the global default; explicit Normal controls equal default
controls. Non-default screen/rotation settings count, including those selected
automatically during import. The marker describes the saved configuration, not a
history of user edits; restoring the defaults removes it.

## Android catalogue loading

Library activity resumes reread the current saved catalogue/settings on an IO
coroutine, keeping the existing cards usable until the refreshed snapshot is
ready. Concurrent refresh requests are coalesced. Switching platform tabs only
filters the in-memory snapshot. An unchanged snapshot does not rebind the cards
or decode their artwork again. The JNI list response supplies `is_gizmondo` once
per entry; the filter and card adapter never call the per-game classifier, which
would reopen/migrate the entire catalogue for every card.

Custom-settings badges are still computed from current settings/config at each
refresh. Graphics API detection uses a process-local cache keyed by executable
path, byte length and modification time, including unknown results. Replacing an
executable invalidates its label; restarting the app starts a fresh cache. Cold
loading still analyses executables once and resolves display metadata, but this
work no longer blocks the UI thread. No persistent derived-data database or new
user refresh requirement is introduced.

## Image geometry and input

Sample the produced image directly; use the platform presentation grid for
display fitting and the logical guest size for input coordinates. Apply inverse rotation once, through the same display rectangle.
Internal GLES scaling must not change guest hitboxes or the emulated LCD size.
Prefer integer enlargement according to the platform's presentation policy;
when the produced image is too large, downscale proportionally rather than clip.
A 320x240 display grid, a native VGA image and an internal-resolution multiplier
must not be treated as interchangeable zoom definitions.

Android display size is automatic: the former Display scale preference and its
configuration field have been removed. PocketPC chooses the largest whole-number
multiple of 240x320 (portrait) or 320x240 (landscape) for standard-aspect images that fits the reserved game area, regardless
of the internal GLES rendering resolution. All fitting integer steps are allowed,
without a four-step cap. Thus a 320x240 guest can display at 960x720 whether GLES
produces 320x240, 640x480, 960x720 or 1280x960. Sample the high-density texture
directly; never first resolve it to QVGA. Native VGA uses the same presentation
steps. If space decreases, drop to the previous integer step. Only viewports too
small for one QVGA step use proportional downscaling; nonstandard aspect ratios
retain their native grid.
Gizmondo retains automatic produced-image integer fitting, and controller fullscreen
retains its own aspect-fit geometry. Rendering, pointer overlays and inverse input
mapping use the same shared rectangle; internal density never changes guest hitboxes.

Controls reserve their own area and do not overlap the touchable game image.
Big controls may reduce the fitted game scale. Do not copy a Gizmondo overflow
option onto the PocketPC touchscreen layout.

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
Discard temporary fullscreen restore sizes so they cannot override this snapshot.
Desktop window dimensions are independent of guest resolution, rotation and
image zoom. Starting a game or changing these settings never resizes the native
window. Leaving fullscreen restores the previous windowed size after the native
mode acknowledgement. Both PC and Deck default to a normal, non-maximized
1280x800 physical-pixel outer window, capped independently on each axis to the
current monitor. Measured decorations are subtracted for the client area and DPI
is applied once. Startup waits for native fullscreen/maximize restoration to end
before sizing. Saved fullscreen/window geometry does not override this policy;
an explicit CLI display argument of 0 can still request fullscreen. Later manual
resizing and fullscreen controls remain available.
Ending a game while its options are visible uses the same session cleanup. A
queued game-to-game launch retains the original library snapshot.

## Android controls


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

Controller reuses the existing input IDs and functions with this arrangement:

* Landscape: P1/P2 on the left and P3/P4 on the right sit at the top corners,
  retaining their previous dimensions in Normal and Big. The D-pad is centred
  vertically. A/B/C/Action form a raised diamond below the right P bank; it no
  longer has to share the D-pad's horizontal axis.
* The landscape stylus stick fills the space below the diamond, up to the D-pad
  diameter. On short viewports its diameter is capped to keep the targets apart;
  P-key/action sizes do not shrink just to enlarge the stick.
* Portrait: the stylus stick is centred at the bottom and exactly matches the
  D-pad diameter in both Normal and Big. The stick sits slightly below the D-pad/action axis, with geometric clearance
  for the square D-pad and circular action targets. All portrait controls, including
  the auxiliary toolbar, anchor the bottom. The display uses the full width above
  the bank; Big can reduce it to a lower integer presentation step.
* Transparent oval corners reject a new DOWN before input listeners run;
  accepted touches retain ownership through release or cancellation.
* The auxiliary toolbar stays separate, with smaller targets and fitted text.

Control dimensions and safety gaps use Android density units. The existing fit
envelope is retained so P-key and action sizes remain stable; additional space
goes to the game viewport rather than stretching these keys. Portrait reserves
a bottom bank and landscape reserves side banks. This changes host layout only,
not guest framebuffer dimensions, mappings or the inverse touch transform.

Classic preserves the existing landscape phone rectangles, including fractional
density and Normal/Big. Its portrait toolbar also anchors the bottom, freeing
the area above the control bank for the game. On viewports whose smaller dimension is at least 600dp,
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
top P banks, landscape D-pad centring, portrait stick centring/equal diameter,
safety gaps and bounded tablet group dimensions.
Phone snapshots verify stable landscape Classic rectangles and bottom-anchored portrait tools. Existing display scaling,
fullscreen input and multitouch-routing tests are retained. Rust and Android
configuration tests cover old files and independent layout/size persistence.
No full Android APK build or physical device validation is claimed.

#### Controller sizing and display fit

Landscape margins and the game/control gutter remain compact. The top P row can
sit beside the central toolbar; toolbar width is bounded by the gap between the
P banks. The action diamond and P-key dimensions are unchanged by the new thumb
placement. Classic geometry is unchanged. The landscape reference viewport still
fits the same integer game-image scale as Classic in Normal and Big. Portrait
uses the remaining display space above the larger centred stick, rather than
forcing a previous zoom that would overlap the controls.

Oval action buttons and the stylus wrapper reject only new touches outside their
oval; ongoing MOVE/UP/CANCEL and split multitouch dispatch keep their existing
behavior. Mapping/steering previews use the same control view and geometry.

## Controller button labels in the UI

Desktop controller input uses bundled, statically linked SDL3 (sdl3 crate 0.22.0).
Input IDs and saved bindings stay positional (South/East/West/North); guest
commands on the mapping diagram remain guest commands. UI prompts and physical
input rows obtain their face-button markings from SDL_GetGamepadButtonLabel on
the opened gamepad. There is no PocketHLE-Reborn name/vendor lookup or Nintendo/Xbox/
PlayStation translation table. The selected controller supplies the labels;
accepted button input activates that device. Device removal and SDL mapping
changes refresh the metadata. Unknown markings use explicit position names
(Bottom/Right/Left/Top button), never an assumed Xbox letter. This applies to
navigation hints, the update notification, the virtual keyboard, import
display-settings prompt, full file-dialog help and overwrite/back prompt,
capture gallery/viewer, Colors startup, binding assistant/inspection and the
per-game stylus-grid cell labels.
Shoulder help uses generic Left/Right shoulder terminology rather than Xbox
LB/RB markings; SDL supplies face-button labels, not every auxiliary inscription.
Keyboard A/B shortcuts in the import prompt are still literal keyboard keys.

Android retains native Android input. ControllerLabels queries the active input
device's KeyCharacterMap display label for each normalized face-button keycode;
when Android supplies no printable label, it uses the same explicit position
names. A system keycode named BUTTON_A is not itself evidence that the hardware
button is printed A. No device-name or vendor heuristic is used. Android cannot
guarantee physical markings when its driver supplies incomplete or normalized
metadata; neither platform can recover hardware hidden behind a virtual controller.
Android binding rows, selected-input summaries, assignment messages and per-game
stylus-grid cells use this helper; effective
bindings, reserved inputs and guest commands are unchanged.

SDL3 also owns queued PCM playback in the desktop capture viewer through a
paused-by-default audio device stream. Queue size is measured in source PCM bytes;
seek clears the stream, playback resumes/pauses it, and dropping it closes the
device and balances the audio-subsystem initialization on the playback worker.
CPAL remains the emulator's game-audio backend. No second SDL2 desktop input or
capture-audio dependency is retained.

## Factory input profiles

The effective default map is profile-specific. Explicit saved short/long
assignments take precedence; restoring defaults clears those overrides. Existing
serialized device-button IDs and guest VKs are preserved. `center_tap`,
`rotate_screen` and desktop-only `screenshot` are additional host commands,
consumed by the frontend before any guest-key dispatch.

| Input | PocketPC default | Gizmondo default |
| --- | --- | --- |
| Keyboard 1 / 2 / 3 / 4 | P1 / P2 / P3 / P4 | L1 / R1 / unassigned / unassigned |
| Keyboard Tab | Center tap | Unassigned |
| Keyboard T | TURBO | Unassigned |
| Keyboard F1–F5 | Unassigned | Home / Volume / Brightness / Geofence / Power |
| Keyboard P | Stylus click / mode (customizable) | Unassigned |
| Controller L1 / R1 | P1 / P2 | Volume / Home |
| Controller L2 / R2 | P3 / P4 | L1 / R1 |
| Controller North / South | A / Action | Stop / Play |
| Controller East / West | B / C | Forward / Rewind |
| Left-stick click / right-stick click | Esc / reserved stylus click and mode switch | Unassigned / unassigned |
| Guide/Mode short press | Tab | Unassigned |
| Extra/Misc1 short press | Rotation | Rotation |

Guide and Extra remain customizable, with no default hold. Screenshot is an
optional desktop assignment with no controller default; F10 remains its dedicated
host key. Android hides and ignores Screenshot assignments imported from desktop.
View short toggles fullscreen and View long returns to the library for both
profiles. Menu short cycles filters; Menu long records on desktop and has no
Android action. View/Menu remain immutable. PocketPC also reserves all four right-stick directions
and R3, including their short/long overrides. Extra has no standard Android key
adapter; its configured action only runs if that input is delivered by the OS.

PocketPC lists use ACTION, TAB (Soft1/VK_TAB), ESC (Soft2/VK_ESCAPE), TURBO, A,
B, C and P1–P4. Diagram captions use compact Act, Tab, Esc and direction names.
Gizmondo face labels are Play, Stop, Forward and Rewind; shoulder commands are
L1/R1. The official manual calls the backward media button Rewind. Brightness
remains a freely assignable guest command, never a fixed View/System gesture.
Source: https://fcc.report/FCC-ID/THGGZ2056/591669.pdf (Quick Start Guide, page 33).

The serialized legacy keyboard base map is retained for compatibility. The
shared profile fallback supplies the effective defaults above without rewriting
custom keys. Exact obsolete factory controller maps containing Extra-to-Esc are
migrated out of the shared map; custom assignments are preserved. The common
fixture is `frontends/pocket-desktop/assets/input/deck-input-defaults.json`.

Before egui builds input state, desktop normalizes only physical number-row
key events (0–9), so Shift and AZERTY symbols do not change the assigned input.
Letter layout and committed Text remain unchanged. Android digit names come
from key codes, independently of Unicode characters and modifiers.

## Visual input editor — desktop and Android

### Entry points and shared artwork

Desktop exposes **Keyboard** and **Controller** buttons in the Settings toolbar
group. Android keeps **Keyboard bindings** and **External controller bindings**
entries in its settings. Each opens only the selected input device's screen. Connection status and controller shortcuts belong to the controller
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

**Assign an input…** is the sole binding editor. Its button shows **Ctrl+Enter**
on the keyboard page and **L3 + R3** on the external-controller page. Either
shortcut starts the assistant without clicking the button. L3/R3 means the two
stick-click buttons held together on the same controller (the selected controller
on desktop), not stick directions. These shortcuts apply only to the matching binding page, never
normal gameplay or other settings. Release both launch buttons (Ctrl and Enter,
or L3 and R3) before capture starts; the launch chord is consumed and cannot
become the assigned input. Holding/repeating it does not reopen the assistant.
Focus loss or disconnection clears pending shortcut state.

Start it once, press and release
a physical input, then choose the guest command. Releasing before 650 ms selects
the short assignment; holding at least 650 ms selects the long assignment.
Keyboard arrows or controller D-pad/left stick navigate. Enter or the physical
South button applies; Escape or East cancels the command choice. Android
Back can cancel the assistant. These navigation buttons are physical UI controls,
independent of their saved guest assignments.

After applying, **Assign another input** starts another capture, **Save and
finish** persists the draft, and **Back to overview** returns without saving.
The desktop assistant stays centered in the current viewport, including after a
fullscreen/windowed resize. It owns navigation and its selected-row highlight;
stale launcher widget focus cannot draw a border over the dialog title. Command
lists reveal selection once on entry or when keyboard/controller navigation
moves it. Mouse-wheel scrolling does not repeatedly recenter the selected row.
Android mouse-wheel events are passed to the native ScrollView rather than
being treated as joystick input.

This permits assignment and saving without another mouse/touch interaction.
The normal screen Save/Cancel actions remain. A reserved host input is rejected
and capture restarts with an explanation; it is not silently rebound.

Choose **Unassigned** after a short capture to clear that profile's short action,
or after a long capture to remove its long action. Removing a long action restores
immediate short behavior; it does not clear the short assignment. Short choices
include stylus directions for PocketPC keyboard inputs. Controller stylus
directions are fixed to the right stick and are never assignment choices.
Long stylus actions are not offered. Gizmondo has no stylus choices.
Only **Stylus speed (pixels)** remains below the editor, shared by both modes;
there is no enable switch or configurable controller direction table.

**Reset keys to defaults** and **Reset controller defaults** are directly visible
in their respective screens. Reset restores the corresponding legacy guest
binding table and removes explicit short/long overrides for that input device
in both Pocket PC and Gizmondo profiles. Controller reset also restores the
additional fullscreen shortcut to View/Select. These actions do not reset the
keyboard stylus direction table or speed. Reset edits the draft and still
requires Save. Android bindings are loaded when a game starts; relaunch a running
game after saving. Desktop physical routing reads the saved launcher config.

### Storage, precedence and runtime timing

The Deck layout is the common factory reference on desktop, the runtime Deck profile and
Android, including Pocket PC shoulder/face/thumb mappings and Gizmondo L1 volume.
`deck-input-defaults.json` is checked against Rust and Android defaults by tests.
Standard-profile library loading migrates the recognized old PC factory map
without rewriting custom tables. Deck-specific legacy binding migrations are removed. Legacy keyboard/controller and keyboard stylus tables remain shared between device
profiles; new explicit command overrides are profile-specific.

`short_press_bindings` and `long_press_bindings` are optional maps in the launcher
configuration (`config.json`, or `config-steamdeck.json` for the runtime Deck profile).
Keys are `pocketpc:keyboard:x` or `gizmondo:controller:South`, using canonical
punctuation/lowercase keyboard names and canonical SDL controller names. Short
values are assignable device-button or host-command IDs, or null for explicit
unbinding. Long values are assignable IDs. Empty maps use the current profile
defaults. Explicit overrides
preserve legacy tables; assigning a legacy stylus direction moves conflicting
legacy keyboard entries. A conflicting optional fullscreen
shortcut is cleared when the input is assigned through the assistant.

Reserved host shortcuts and the PocketPC right stick/R3 retain priority over
explicit overrides, grids and optional fullscreen bindings. Factory guest mappings
on other inputs are overridable.
In normal gameplay, explicit physical short/long mappings are consumed before the
per-game grid and ordinary stylus routes. Without an explicit override those
existing routes still apply. Guest direction rotation and Pocket PC button-group
swaps remain in the output path. Native text entry and context-dependent host
shortcuts retain their existing priority; this editor does not replace them.

Device-button short-only bindings emit immediate guest down/up and support held
movement. Host commands consume the trigger and use their own action lifecycle
(for example the Center tap pulse described below).
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
Reserved inputs retain their red border while the dark teal fill indicates a
physical press. Only OS-delivered keys can be shown. Outside the assistant, ordinary bindable
inputs only test feedback. Inside it, they capture or navigate the current step.
Reserved host function keys remain active before either route. Android capture is cancelled when the activity stops;
draft/profile/selection survive recreation, but an active capture is not resumed.
Returning to Android settings reloads saved config before another preference can
overwrite it.

View/Select taps fullscreen; its 650 ms hold returns to the library for both
PocketPC and Gizmondo. It never sends Gizmondo Brightness. Menu/Start taps the next presentation filter; its hold toggles
recording on desktop and has no action on Android. View/Select and Menu/Start (the small top-left and top-right
Steam Deck buttons, shown as Full and Filter) are immutable for both tap and
hold, outlined in red and handled before menu capture/navigation. Guide/Mode
is customizable: its short default is guest Tab on PocketPC and unassigned on
Gizmondo. Extra/Misc1 defaults to Rotation on both profiles and is customizable.
Neither has a default hold. Screenshot is an optional assignable command on
desktop only, with no controller default. Extra has no standard Android key
adapter and is marked desktop-only in that adapter's UI. Physical **F7–F12 are
immutable host keys**, outlined in red
on the keyboard skin (the red border remains during physical feedback):

| Key | Host action |
| --- | --- |
| F7 | Next rotation (0°, 90°, 180°, 270°) |
| F8 | Return to the library from gameplay or another launcher page |
| F9 | Next presentation/upscaling filter, in the same order as Menu |
| F10 | Game Screenshot on desktop; reserved without action on Android |
| F11 | Video recording on desktop; reserved without action on Android |
| F12 | Toggle fullscreen/windowed, including binding pages and the assistant |

Host keys are removed before capture, per-game grids, stylus routes and guest
bindings. Assignment APIs reject them; stored legacy/short/long guest mappings
cannot override them. Reserving physical F11 does not reserve the guest's VK_F11:
Gizmondo Power can still be sent through its normal F5 or another guest binding.
F8 leaves an unsaved binding draft without saving it. In menus F10/F11 stay
reserved rather than becoming new screenshot/recording UI commands. Android game
F12 keeps its existing game-only layout; launcher F12 hides/restores app/system
bars. Android F9 persists the global filter and keeps an open binding draft in
sync; other launcher pages refresh their configuration. On desktop the open
launcher draft is likewise synchronized so Save cannot restore the old filter.
Android mapping pages expose neither Screenshot nor Recording shortcuts. Guide
is freely assignable; Filter remains reserved but its long press has no action
and consumes the short tap. F10/F11 remain reserved without an action.
A host hold consumes its tap. Android filter changes run on the GL thread,
invalidate presentation upload and lazily initialize/reuse SMAA without changing
guest graphics state or geometry.

Android Back/Guide/LeftStick/RightStick names accept SDL's Select/Mode/LeftThumb/
RightThumb aliases. Gizmondo omits the legacy Turbo helper from choices and
missing-command reports; legacy explicitly configured Turbo bindings display as Brightness because both
send VK_F3. Pocket PC keeps its existing Turbo helper.

### Android input-editor selection

Selecting a skin hotspot or a controller row updates the existing skin, row
selection and selected-input summary in place. It must not replace the Activity's
content view merely to select another input. The previous rebuild followed by a
posted scroll restore could expose the top of the page for one frame, then jump
back to the saved position.

Profile changes, resets and keyboard zoom still rebuild the structural content.
Their scroll offsets are restored after layout in a one-shot pre-draw callback;
if restoration changes an offset, that intermediate draw is cancelled. No timed
post-delay or renderer-specific workaround is used. R3's controller row explicitly
shows its fixed mode gesture even though it is a reserved input without a normal
host tap/hold action. This is frontend view-state handling, not a GLES game fix.

## PocketPC accelerometer

On desktop and Android, **Tilt input (accelerometer)** is available in the
global PocketPC options and in each PocketPC game's controls settings.
**Emulator Default** in a game's settings inherits the global choice.
Source changes apply on the next launch.

| Mode | Source and behavior |
| --- | --- |
| **Use physical accelerometer** | Default. Android uses the device's accelerometer. Desktop uses an SDL host accelerometer when available, then tries the selected controller's accelerometer. |
| **Simulate left/right tilt** | The configured directional buttons or the left stick simulate horizontal tilt relative to the displayed game. Vertical tilt stays neutral. |
| **Simulate tilt in four directions** | The directional buttons or the left stick simulate horizontal and vertical tilt relative to the displayed game. |

In simulated modes, a held direction overrides the stick on its axis.
Opposite buttons on one axis cancel each other. The stick uses a 12% dead zone
and supplies proportional tilt beyond it. Full input supplies up to 0.5 g on
each enabled axis. With no input, the virtual device rests flat, screen up.
The guest receives tilt readings. The game decides what to do with them.
For example, Ferrari GT uses tilt to steer and its on-screen pedals to
accelerate and brake.

### .NET Compact Framework games

Detector Veles opens in the standard emulation view. Its managed Windows Forms
screen and embedded bitmap resources are rendered by PocketHLE-Reborn. FMOD CE
WAV effects use the shared PocketHLE-Reborn audio mixer. Detector Veles reads an HTC compass heading. The managed bridge derives it from
the selected Pocket PC tilt source (physical accelerometer or simulated input),
so it represents tilt rather than magnetic north. **Information** detects this
managed sensor reference.

Managed OpenGL ES games use the global GPU renderer setting on desktop and
Android. If the GPU backend cannot initialize, they fall back to software.
The managed Forms overlay is still composited into the CPU framebuffer, so
desktop presentation can report `CPU_UPLOAD` even with GPU rendering.

Games such as Diamond Beer V2 use the same **Tilt input (accelerometer)**
setting and sources on desktop and Android. **Information** reports
accelerometer support when the game's managed executable references the
supported `Sensors.GSensorFactory` or `Sensors.IGSensor` methods. This
indicates that the game uses the interface. Sensor availability depends on
the host and the selected input mode.

The managed interface uses an upright resting orientation in simulated modes
and when no fresh physical sample is available. In Diamond Beer V2, tap the
barrel in the bottom-left corner, select **Duvel**, enable the logo checkbox,
then confirm. The original CAB selects the Duvel preset on first launch but
leaves the logo checkbox off.

### Guest interfaces

The native guest interfaces share the physical or simulated sample. PocketHLE-Reborn
converts it to device coordinates before encoding it for the requested interface.

| Interface | Guest access |
| --- | --- |
| PocketHLE-Reborn implementation of `HTCSensorSDK.dll` | Open with `HTCSensorOpen(1)`, read with `HTCSensorGetDataOutput`, and release with `HTCSensorClose`. A successful read returns zero. The 20-byte result starts with signed X, Y and Z values. PocketHLE-Reborn fills the remaining fields with zeros. |
| `SEN0:` stream device | Open with `CreateFileW` and `OPEN_EXISTING`. Read via `DeviceIoControl` with control code `0x01000001`. The reply is three little-endian signed 16-bit values in X, Y, Z order. The legacy zero output-length form is supported and still requires a writable six-byte buffer. Release with `CloseHandle`. |
| `ACC1:` stream device | Open with `CreateFileW` and `OPEN_EXISTING`. Read with `DeviceIoControl` code `0x80122ef0`. The reply is three little-endian 16-bit words with unsigned values in the range 0–4095. Release with `CloseHandle`. |

One g is approximately 1,000 counts. In device coordinates, positive X
points right, positive Y points down, and positive Z points out through the
screen. The readings follow the direction of gravity, so a device resting
flat with the screen up reports approximately `(0, 0, -1000)`. PocketHLE-Reborn
converts host acceleration into that convention and undoes the presentation
rotation before encoding the sample. A newly launched guest process inherits
its parent's display rotation.

When an application supplies its own `HTCSensorSDK.dll`, PocketHLE-Reborn loads that
DLL so it can retain its own handles and return codes. The SDK bundled with
HTC Teeter reads `ACC1:` and returns 1 on a successful SDK read.

The device-coordinate conversion happens once. For the same sample, the SDK
bundled with HTC Teeter decodes the three `ACC1:` words as
`(raw_x - 2048, raw_y - 2048, 2048 - raw_z)`. PocketHLE-Reborn therefore encodes
signed device counts `(x, y, z)` as `(2048 + x, 2048 + y, 2048 - z)`, clamping
each word to the driver's range. The resulting SDK readings match `SEN0:`
whenever none of the values saturate.

| Simulated input | Signed `SEN0:` or decoded SDK counts | Raw `ACC1:` words |
| --- | --- | --- |
| Flat, screen up | `(0, 0, -1000)` | `(2048, 2048, 3048)` |
| Right at full tilt, display rotated 90° clockwise | `(0, -500, -866)` | `(2048, 1548, 2914)` |

The sensor tests cover neutral, rotated input, and the six-byte stream formats.
The HTC Teeter launch test uses simulated tilt and an intact CAB. It does not
measure axis calibration on physical HTC or Samsung hardware.

Physical mode requires an available sensor and a fresh sample. If the sensor
is unavailable, the guest open call fails. If an available sensor has yet to
produce a sample or its last sample is at least one second old, reads report
that the data is unavailable. Select a simulated mode to use the directional
buttons or left stick on a host without a sensor.

## PocketPC right-stick stylus modes

On Windows/Linux/Deck and Android with an external controller, the PocketPC
right stick is permanently assigned to the stylus. **Double-click R3 to switch
stylus mode** is shown below the input editor and in the read-only selected-input
card. The controller picture shows one **Click / ×2** caption inside the right
stick, with no overlaid direction captions; the rails retain the four direction
assignments and **Mode ×2** on R3. Guide shows **Tab**. Long-press captions sit
below Full/Filter rather than crossing their button outlines. Reserved inputs
use red outlines. Gizmondo keeps its guest mappings.

* **Stick** is the initial mode for each game session: the existing held-stylus
  drag behavior, including edge lift/recentering, remains. A single R3 click does
  nothing.
* **Pointer** moves a visible, outlined crosshair within the logical game screen.
  A single R3 click sends a 60 ms stylus pulse at the position captured on release.
  There is no automatic held drag or edge recentering in this mode.

Keyboard **P** defaults to the same click/mode gesture for PocketPC. **Stylus
click / mode** is an assignable keyboard command, not an immutable key: users
can clear P, choose another key, or change the existing O/K/L/M stylus directions
through the assistant. Assigning the gesture clears that key's old long binding;
it is not offered as a long action. The keyboard picture and selected-input card
show the effective mapping. The right stick/R3 remain fixed on controllers.

The double-click window is 200 ms from the first release to the second press.
The first click is deferred; a second press inside that window cancels it and
switches modes on release, without injecting a guest tap. Repeat key events do
not count as additional clicks. A mode notification uses the same desktop overlay
or Android Toast location as filter changes: **Stylus mode: Stick / Pointer**.

**Stylus speed (pixels)** is the only setting, shared by both modes. Its numeric
value is a QVGA-reference pixels-per-second speed: the output velocity scales uniformly by the logical screen's shorter
edge divided by 240. VGA therefore uses twice the pixel velocity of QVGA, with
the same proportional travel time. This is independent of host zoom, GPU internal
resolution and presentation rotation. Keyboard stylus directions and the
on-screen aim pad use the same current mode. Pointer coordinates follow logical resolution, zoom and the
existing inverse rotation; the crosshair is a host overlay, not guest pixels.
Physical touch and active grids take priority. Touch/grid strokes, focus loss,
disconnection and transitions cancel pending clicks; synthetic pulses always
release. A new game starts in Stick mode, while focus/rotation preserve the mode.

The obsolete `stylus_stick.enabled` and `stylus_stick.controller` fields are no
longer read or emitted; saving removes them. `speed` and keyboard direction
bindings remain compatible. Old explicit short/long or grid assignments targeting
reserved inputs cannot override the fixed controls.

All immutable keyboard inputs (F7–F12) and controller inputs (View/Menu plus the
PocketPC right stick/R3) are refused by per-game grid capture as well as the global
assistant. A reserved-input attempt reports **This input is reserved by PocketHLE-Reborn**.
Existing saved grid entries for these inputs are ignored at runtime.

Per-game grids also refuse the **current effective PocketPC stylus keyboard
keys**, including the click/mode key (P by default) and all configured direction
keys. This is a contextual conflict, not a global keyboard lock. Capture explains
that the key is assigned to the stylus and must first be reassigned in Keyboard
bindings. Runtime routing ignores stale grid bindings for those keys. The policy
uses current profile overrides and direction tables, so a cleared/replaced old
key is freed and a newly assigned key becomes protected from grids. Desktop and
Android use the same precedence.

## Center tap

Center tap is an assignable PocketPC-only host stylus command. Factory bindings
use keyboard Tab. Guide now defaults to the guest Tab command; it no longer
performs Center tap. It emits PointerDown at
(width/2, height/2) of the logical guest framebuffer, then PointerUp after
60 ms; display zoom, GPU resolution and rotation do not change that point.
Holding/repeat does not extend or repeat the pulse. It does not take ownership
from an already-held real stylus or touch grid. A new real touch/grid stroke,
focus loss, screen change or shutdown cancels it and releases the old point.
No synthetic virtual key reaches the guest. Rotation and desktop Screenshot
use the same assignable-command model. F7 remains immutable, while Extra's
factory Rotation can be replaced or cleared. Android exposes Rotation and
PocketPC Center tap, but not Screenshot.

## Desktop focus and import launch-settings prompt

In control settings, mouse clicks and scrolling must not force navigation focus
back to the first registered page widget. A clicked skin hotspot may own an egui
focus ID outside the controller tab list. Keep that mouse focus rather than
repairing it to the top button. Auto-reveal and focus repair are enabled when
keyboard/controller navigation resumes; popup selection keeps its own reveal.
Native touch Start/Move/End/Cancel events and held pointer gestures also suspend
navigation focus repair, including when no mouse-button edge accompanies the
touch event. Each swipe continues from the existing scroll offset.

The **Touch zone mapping** and **Test touch steering** windows use centred,
viewport-bounded fixed dimensions and an internal scroll area. Assignment labels
and feedback do not resize or reposition the window. Cancel assignment retains
its place while disabled outside capture, keeping the controls stationary.

On desktop and Android, a new Pocket PC import keeps the detected name.
Use **Rename** in the library tile menu to change it. When the import contains
multiple launchable executables, select one before the import is committed.
The **Pocket PC launch settings** dialog groups screen and rotation guidance,
FPS advice when applicable, and a GPU profile selector when the executable's
OpenGL ES profile is ambiguous. **Continue** keeps the current profile.
**Save profile** applies a changed selection. **Open game settings** applies
the selected profile and opens the full settings page.

**Switch Launch Executable** is directly below **Add/Replace Data** in the
Pocket PC tile menu. After switching, the GPU profile dialog appears only
when the new executable's profile is ambiguous. The same applies when
**Add/Replace Data** installs an executable and the user selects it for launch.
In that case the profile dialog follows the copy directly. Existing-file
conflicts still require confirmation before any replacement.

Use the controller's directional controls to move focus and its confirm
button to activate a control. The dialog uses the same focus navigation as
other launcher dialogs.

## Desktop input lifecycle

Native frame-ready notifications wake UI processing; coalesce notifications,
not button edges or axis values. Preserve source ownership/refcounts, release
ordering, focus loss, disconnect behavior and the latest-image mailbox contract.
Keyboard/mouse correction is hardware-confirmed for the reported Windows case;
controller/wheel/touch follow-ups have narrower validation (see GPU.md).
Menus/dialogues still need their ordinary explicit redraws and navigation.

## Windows/Linux native service parity

Windows and Linux build the same pocket-desktop binary source (eframe/egui), use
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

## Frontend APIs and dependency notices

Android imports use the prepared transaction: prepare, cabinet/platform choice,
then commit or cancel. Execution uses streaming sessions. JNI callbacks invoked
from native code remain required even when no Kotlin UI method calls them
directly. Previews use live control/card views rather than parallel layouts.

Desktop third-party notices match the locked SDL3, Rust SDL3/sys and bundled
HIDAPI dependencies. Both Windows and Linux archives include the notices;
Android does not bundle desktop SDL3. Historical guest SDL2 ABI references and
orbclient's target-specific transitive dependency are distinct from the desktop
input/playback backend and are not obsolete merely because they mention SDL2.
