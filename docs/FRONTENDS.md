# Frontends and controls

User workflows for Desktop, Steam Deck and Android. Use the [settings reference](OPTIONS.md)
for option lists, [Graphics](GPU.md) for rendering, [Snapshots](SNAPSHOTS.md) for
checkpoint internals, and [Build and release](BUILD.md) for installation/builds.
Deck setup is covered in the [Steam Deck guide](STEAMDECK.md).
Unless a heading or paragraph specifies a host platform, workflows apply to both
Desktop and Android. PocketPC/Gizmondo labels refer to the emulated device.

- [Import and launch](#desktop-focus-and-import-launch-settings-prompt)
- [Add data](#adding-data-to-an-installed-game) and [remove games](#removing-library-games)
- [Desktop toolbar](#desktop-button-toolbar) and [snapshots](#experimental-snapshots)
- [Default bindings](#factory-input-profiles), [input editor](#visual-input-editor--desktop-and-android) and [shortcuts](#colors-physical-feedback-and-host-shortcuts)
- [Touch grid](#touch-zone-mapping), [stylus](#pocketpc-right-stick-stylus-modes) and [motion input](#pocketpc-accelerometer)
- [Android controls](#android-controls) and [image fitting](#image-geometry-and-input)
- [Session diagnostics](#session-diagnostics)

## Desktop runtime launch profiles

Run `pockethle-gui` normally or with `--steam-deck`. The profiles use `config.json`
and `config-steamdeck.json`, respectively, while sharing the game catalogue and
per-game data. Both start windowed at 1280×800 physical pixels, capped to the
monitor. An explicit launch display argument of `0` requests fullscreen.
See [Steam Deck setup](STEAMDECK.md) for Proton and Steam shortcuts.

## Desktop button toolbar

The toolbar groups **Game library**, **Settings**, **Features**, **Snapshot** and
**About**. Small windows scroll horizontally. Use directions to select a button,
South to activate it, and **L3** to switch between the toolbar and page.
**Return to game** resumes a session running behind a settings page.

Features provides **Upscale ×2**, **Filter**, **Rotation**, **Fullscreen**,
**Screenshot** and **Record video**. Upscale applies to both device profiles and
all graphics paths; enabling it enlarges a window when needed, up to the monitor
size. It is separate from GLES internal resolution. Filter and rotation cycle
through their available modes. Fullscreen gameplay hides the host toolbar.

Leaving a game restores the library's previous window geometry and fullscreen
state. About returns to its originating page without discarding a settings draft.
Library Backup/Restore is separate from game snapshots and is unavailable during
a game or import.

## First-import setup

The first import for each platform offers a one-time assistant for global
defaults. A multi-game import opens it once. **Keep current settings** leaves
those defaults unchanged. Android also previews its control layout and size;
Big controls can reduce the space available for the game image.

## Desktop focus and import launch-settings prompt

The import and executable-selection workflow below applies to both Desktop and
Android; Desktop additionally manages focus between its toolbar and dialogs.

Import keeps the detected game name; use **Rename** in the game menu to change it.
If several launchable executables are found, choose one before completing import.
The **Pocket PC launch settings** dialog groups screen/rotation advice, relevant
FPS advice and a GPU selector when the OpenGL ES profile is ambiguous:

- **Continue** keeps the current profile.
- **Save profile** applies the selected profile.
- **Open game settings** applies it and opens the full settings page.

PocketPC's **Switch Launch Executable**, below **Add/Replace Data** in the game
menu, changes the selection later. Adding data can also offer a newly installed
EXE. Both paths reuse the same launch advice when needed, without a permanent
library rescan. File pickers, choices and confirmations support controller
navigation independently of gameplay bindings.

## Adding data to an installed game

Choose **Add/Replace Data** from a PocketPC or Gizmondo game's menu:

1. Select a file and its destination: **Application Folder** (`extracted/`) or
   **My Documents** (normally `saves/my-documents/` inside that game's directory).
2. For a ZIP, choose **Extract contents** or **Copy ZIP unchanged**. Extraction
   preserves folders: `test/racer.gba` becomes `my-documents/test/racer.gba`.
3. Confirm any listed replacements. Cancel abandons the whole addition.
4. In Application Folder, an added EXE can replace the launch selection.
   My Documents has no executable-selection step.

The final notice shows the destination. Existing directories are merged and
unrelated files remain. Unsafe archive paths and special files are rejected.
Preparation is staged; operation errors roll back published files and manifests.
If recovery needs manual action, the error gives the retained recovery path.
Additions are unavailable while a game is running or stopping.

## Removing library games

**Remove** asks for confirmation and deletes installed files, saves and per-game
settings. It is unavailable while a game session is running or stopping.
South/Enter confirms; East/Escape cancels.

## Library tile metadata

Tiles show the saved resolution, detected graphics API and **Custom settings**
when overrides differ from defaults. API labels come from the selected executable
and relevant local dependencies; they describe static evidence, not a live renderer
trace. **Information** shows additional detected hardware and executable details.
Replacing or switching an executable refreshes its detection.

Android refreshes the catalogue in the background. Platform tabs filter the
loaded catalogue, and unchanged cards reuse their artwork and metadata.

<a id="experimental-desktop-snapshots"></a>
## Experimental snapshots

Desktop and Android share one snapshot slot per game:

| Action | Desktop | Android |
| --- | --- | --- |
| Save | **Save Snapshot**, F5 | **S**, upper-left physical window corner |
| Restore | **Restore Snapshot**, F6 | **L**, upper-right physical window corner |

Save becomes available after the game produces a frame. Restore can use the
running or selected game; an empty slot does nothing. Both commands can also be
assigned to a controller. A successful save replaces `games/<id>/snapshots/snapshot-01.bin`.
The saved timestamp appears in Desktop tooltips and Android success messages.
Controls are disabled while an operation is pending.

Restore requires the matching executable, mounts and compatible emulated-device
settings. Guest-held buttons and stylus presses are released on restore. Failures
use the [diagnostic dialog](#session-diagnostics); a failed save keeps the previous
slot. See [snapshot coverage and file behavior](SNAPSHOTS.md) for supported
resources, writable-file rollback, process groups and external-service limits.

## Factory input profiles

These are effective defaults; saved assignments can override ordinary inputs.
Number-row keys work with AZERTY and QWERTY layouts.

| Physical input | PocketPC | Gizmondo |
| --- | --- | --- |
| Keyboard Z / Q / S / D | Up / Left / Down / Right | Up / Left / Down / Right |
| Keyboard ↑ / ↓ / ← / → | A / Action / C / B | Stop / Play / Rewind / Forward |
| Keyboard 1 / 2 / 3 / 4 / 5 | P1 / P2 / P3 / P4 / unassigned | Home / Volume / Brightness / Geofence / Power |
| Keyboard A / E | Unassigned | L1 / R1 |
| Keyboard Tab / Escape | Tab / Escape | Unassigned |
| Keyboard ² / grave | Center tap | Unassigned |
| Keyboard T | Turbo | Unassigned |
| Keyboard P | Stylus click / mode | Unassigned |
| Controller L1 / R1 | P1 / P2 | Volume / Home |
| Controller L2 / R2 | P3 / P4 | L1 / R1 |
| Controller North / South | A / Action | Stop / Play |
| Controller East / West | B / C | Forward / Rewind |
| Left-stick click | Esc | Unassigned |
| Right stick / R3 | Stylus movement / click and mode | Unassigned |
| Guide/Mode short press | Tab | Unassigned |
| Extra/Misc1 short press | Rotation | Rotation |

Guide and Extra are customizable and have no default hold action. Extra requires
an OS-delivered input; Android has no standard adapter for it. Controller prompts
use markings supplied by SDL3 on Desktop or Android's input metadata, falling
back to positional names such as Bottom/Right/Left/Top button.

## Visual input editor — desktop and Android

Open **Keyboard** or **Controller** on Desktop; Android uses **Keyboard bindings**
and **External controller bindings**. Choose the PocketPC or Gizmondo device
profile, then select an input on the diagram or in the list to inspect it.

To change an assignment:

1. Choose **Assign an input…**. Its page-specific shortcut is **Ctrl+Enter** for
   keyboard or **L3 + R3** for controller. Release the launch chord before capture.
2. Press and release the physical input. A hold of at least **650 ms** selects
   its long action; a shorter press selects its short action.
3. Choose a command, then **Assign another input** or **Save and finish**.
   Choose **Unassigned** to clear that short or long action.

Directions navigate, Enter/South accepts and Escape/East/Android Back cancels.
**Back to overview** leaves the draft unsaved. Reset restores that input device's
factory bindings for both profiles; it still requires Save. Android loads bindings
at game launch, so restart a running game after saving.

A short-only guest binding acts immediately and can be held. Adding a long action
defers short until release, producing an 80 ms pulse; the long action suppresses
short once its threshold is reached. Focus loss or disconnection cancels pending
holds and releases owned inputs. Explicit short/long assignments precede touch-grid
and ordinary stylus routing; reserved controls always take priority.

### Colors, physical feedback and host shortcuts

Blue identifies guest commands, green stylus actions, purple host shortcuts,
amber long presses, gray unassigned inputs and red reserved controls. Cyan/teal
feedback shows physically held inputs. **Unassigned commands** lists commands
without bindings, including Save/Load Snapshot.

View/Select and Menu/Start are immutable on both platforms:

| Input | Short press | Hold (650 ms) |
| --- | --- | --- |
| View/Select | Fullscreen | Return to library |
| Menu/Start | Next filter | Toggle recording on Desktop; consumed without action on Android |

Host shortcuts take priority over assignments and touch grids. Desktop reserves
physical F5–F12; Android reserves F7–F12. In this table, unqualified actions
apply to both platforms:

| Key | Action |
| --- | --- |
| F5 / F6 | Save / Restore Snapshot on Desktop |
| F7 | Next rotation |
| F8 | Return to library; unsaved drafts are discarded |
| F9 | Next filter |
| F10 | Screenshot on Desktop |
| F11 | Toggle video recording on Desktop |
| F12 | Toggle fullscreen |

F5/F6 have no fixed snapshot action on Android; use S/L or assigned controller
commands. F10/F11 remain reserved without an action on Android. Screenshot assignments
are Desktop-only. PocketPC also reserves the controller's right-stick directions
and R3. Reserving a physical host key does not reserve its guest virtual-key code:
Gizmondo Power can still send guest F11 through the default keyboard **5**.

## Touch zone mapping

In a PocketPC game's **Controls**, choose **Touch zone mapping**. **View Grid**
previews saved assignments without launching; **Configure Grid** starts the game
with a live overlay and these controls below the image:

| Control | Controller shortcut | Effect |
| --- | --- | --- |
| Grid-size selector | L1 + R1 | Cycle grid sizes |
| Configure Grid / Resume Game | L2 + R2 | Enter or leave editing |
| Clear selected assignment | L3 + R3 | Remove the selected cell's binding |
| Clear Grid | — | Remove all bindings, retaining dimensions |

These chords apply only during the grid-configuration session. Editing dims the
image and captures input while the game continues running. Select a cell, then
press a keyboard key, controller input or Android virtual control to bind it.
**Resume Game** saves edits, restores brightness and returns input to the game;
resume before quitting to retain edits. A size chosen outside editing saves
immediately. Changing dimensions clears assignments.

Landscape grids range from **4×3 to 8×7**; portrait transposes dimensions and
assignments (**3×4 to 7×8**). Host-reserved inputs and current PocketPC stylus
keyboard keys are refused. Reassign a stylus key in Keyboard bindings first if
you want to use it in a grid. Bound grid keys generate touches rather than ordinary
guest keys; explicit short/long assignments retain priority. Touch-steering tests
remain available without launching a game.

## PocketPC right-stick stylus modes

The controller right stick is fixed to the stylus. **Double-click R3** within
200 ms to switch mode; keyboard **P** defaults to the same gesture and can be
reassigned. Keyboard stylus directions and the Android aim pad use the current mode.

| Mode | Movement | Single click (R3 or assigned key) |
| --- | --- | --- |
| **Stick** — initial mode each session | Held drag, with edge lift/recentering | No action |
| **Pointer** | Move a visible crosshair | 60 ms stylus tap at the position captured on release |

A double-click switches on release without sending a guest tap. **Stylus speed
(pixels)** applies to both modes, scaling with logical resolution rather than
host zoom or GLES internal resolution. Physical touch and active grids take
priority. Focus loss, disconnection and touch/grid transitions cancel pending
clicks and release synthetic presses. Focus and rotation preserve the mode.

## Center tap

The assignable PocketPC **Center tap** command taps the logical screen centre
for 60 ms. Holding it does not repeat or extend the tap. Real touches and grid
strokes take priority. The default key is **² / grave**; Guide sends Tab instead.

## PocketPC accelerometer

Guest sensor input comes from a physical accelerometer or simulated directions.
Choose the source in [PocketPC settings](OPTIONS.md#global-pocketpc-options).
On Desktop, physical input uses the SDL host sensor, then the selected controller's
sensor; Android uses the device sensor.

Simulated buttons override the stick on their axis; opposite directions cancel.
Full input supplies up to 0.5 g per enabled axis. Neutral rests flat, screen up.
Physical reads require an available sensor and a sample less than one second old.
A game decides how to use readings; steering tilt does not imply acceleration.

Native interfaces share one device-coordinate sample: X right, Y down, Z out of
the screen. Flat screen-up gravity is approximately `(0, 0, -1000)` counts.
Presentation rotation is undone once before interface encoding.

| Interface | Access / result |
| --- | --- |
| HLE `HTCSensorSDK.dll` | Open(1), GetDataOutput, Close; 20-byte result begins with signed XYZ, about 1,000 counts/g; read success is 0 |
| `SEN0:` | CreateFileW + DeviceIoControl `0x01000001`; six bytes of signed 16-bit XYZ, including the legacy zero-length request form |
| `ACC1:` | CreateFileW + DeviceIoControl `0x80122ef0`; unsigned 16-bit XYZ in 0–4095 |

Game-supplied HTC SDKs load normally. Teeter's SDK reads ACC1 and returns 1 on
success. ACC1 encodes signed counts as `(2048+x, 2048+y, 2048-z)`, clamped to
0–4095, so its decoded result matches SEN0 without applying rotation twice.
Stream handles are released with CloseHandle.

### Physical accelerometer mapping

This is the reverse conversion: **real tilt → arrows or aiming**. Enable it in
per-game Controls; see [OPTIONS.md](OPTIONS.md#per-game-pocketpc-options) for
sensitivity and output choices. Relative aiming does not activate the visible
pointer or touch-grid bindings; direct touch takes priority.

The first valid sample calibrates neutral, and movement follows display rotation.
Lost focus, stale/missing samples or grid editing releases motion inputs and
recalibrates on resumption. Calibration is not snapshot data. While mapping is
enabled, **Tilt input** guest-sensor delivery is disabled. Relaunch after changes.

## Android controls

PocketPC offers **Classic** and **Controller** layouts, with separate **Normal / Big**
control sizing. Both keep touch controls outside the game image. Portrait uses a
compact bottom function bar; landscape leaves safety gaps below the physical
snapshot corners. The Controller layout places the stylus stick beneath the action
buttons in landscape and centres it at the bottom in portrait.

Gizmondo has a D-pad, action buttons, L/R and five device-function buttons, plus
host EXIT/FULL. Its optional control-overflow setting allows overlap with the game.
Bigger controls can reduce the fitted image size. Layout previews use the same
controls and geometry as gameplay. FPS/IPS stay inside the game image; snapshot
S/L stay at the physical window corners.

## Image geometry and input

**Screen** changes the dimensions reported to the guest; **Rotate display** turns
the displayed image and adapts touch/directions. GLES internal resolution changes
rendering density. These settings are independent, and input uses logical guest
coordinates through the displayed image rectangle with inverse rotation once.

Android fits standard PocketPC images to whole QVGA steps (240×320 portrait,
320×240 landscape), including native VGA and internally scaled GLES images.
Nonstandard aspect ratios fit proportionally. Gizmondo uses produced-image integer
fitting; controller fullscreen uses aspect fitting. Images too large to fit are
downscaled proportionally. Controls reserve space according to their layout.
Desktop fullscreen uses whole QVGA or WQVGA steps (320×240 or 400×240, with
portrait equivalents). Window zoom and [GPU render scaling](GPU.md#internal-resolution)
are separate.

## PocketPC shell strip

Automatic shell layout hides the navigation strip for a native borderless window
covering the full panel. Work-area windows, captioned windows and dialogs retain
it. Explicit SHFullScreen requests take precedence. Shell dropdowns are drawn on
a temporary presentation copy so dismissal reveals the original game image.

## .NET Compact Framework games

Supported managed applications run through the common interpreter on Desktop and
Android, sharing VFS, forms, resources, audio, graphics, sensors and snapshots.
Static metadata can identify System.Drawing, OpenGL ES and supported sensor use
in **Information**. This indicates interface use, not host hardware availability.
Managed GLES uses the selected GPU/internal-resolution settings, with software
fallback if GPU initialization fails; forms composition can still use CPU presentation.
Detector Veles's emulated compass derives from tilt, not magnetic north.

## Desktop capture gallery

The **Recordings** tab contains screenshots and videos. South opens; East requests
deletion through a confirmation initially selecting Cancel. Left/Right chooses,
South accepts and East cancels. North shares where supported. In the viewer,
East leaves fullscreen or closes playback; it never deletes a capture.

## Desktop recording under encoder load

Recording uses its own selected 30/60 FPS cadence, independent of the game limit.
An overloaded encoder can capture fewer new images. MP4 holds missed images by
timestamp spacing; AVI repeats MJPEG samples. See [recording settings](OPTIONS.md#desktop-specific-graphics-and-recording).

## Windows/Linux native service parity

Windows and Linux use the same Desktop UI, layouts and controller workflows.
Hardware providers and permissions vary by OS; see [Gizmondo services](GIZMONDO.md).
Simulated GPS fixes work on both; the current native host-location provider is
Windows-only. Build with the normal release features for real CPU, audio and video;
[BUILD.md](BUILD.md) lists dependencies and platform requirements.

## Session diagnostics

Launch, runtime and snapshot failures use **Show details**, **Copy diagnostic**
and **Return to library**. A recoverable snapshot failure also offers **Continue**.
Dialogs support the same controller navigation as other launcher choices.

A user-requested stop is not an error. A runtime exception is an error regardless
of exit code; an application-defined exit code alone does not establish a crash.
Early nonzero exits or guest stderr can explain startup failures. Teardown warnings
alone do not explain an earlier exit. For a silent failure, include the selected
executable, settings and full log when reporting it.
