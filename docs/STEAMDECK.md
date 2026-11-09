# PocketHLE — Steam Deck profile

The desktop uses one executable. The Steam Deck profile is selected at launch
with `--steam-deck`; it is not a separate Cargo feature or executable.

## CI artifact and startup

Download **PocketHLE-windows-x86_64** and extract its ZIP. The Deck uses the
standard `pockethle-gui.exe`; there is no separate Deck archive.
Point the existing Steam shortcut at that executable, select **Proton** or **Proton Experimental** and
a Gamepad Steam Input layout, and put this in Steam's launch options:

```text
--steam-deck
```

This selects `config-steamdeck.json` and the controller profile previously used
by the dedicated build. Both PC and Deck start in a normal, non-maximized window:
1280x800 physical pixels including borders, capped to a smaller monitor. DPI and
measured decorations determine its client dimensions. Saved fullscreen/viewport
state cannot override startup; fullscreen remains available through the UI. Under Wine/Proton, SDL uses the
existing XInput hints; no Proton version-name or controller-brand detection is
added. On native Linux the flag also selects the settings/controller profile,
but does not force the Windows XInput driver. Its updates use the Linux package.

SDL3 supplies button labels. The A/B/X/Y names below describe Deck markings;
external controller UI prompts use SDL's labels, or explicit positions if unknown.
No hardware identity is inferred from the executable's filename or a device name.

An explicit CLI display argument still wins: `0` means fullscreen and `1` means
windowed. For example:

```text
pockethle-gui.exe --steam-deck "Game name or ID" 1
```

## System gestures and customizable command defaults

| Deck control | Action |
| --- | --- |
| Menu, short press | Next presentation filter |
| Menu, hold 650 ms | Start/stop video recording |
| View, short press | Toggle fullscreen |
| View, hold 650 ms | Return to library on both PocketPC and Gizmondo |
| Extra/Misc1, short press (if delivered) | Next rotation (customizable) |
| Extra/Misc1, hold 650 ms (if delivered) | Unassigned by default |
| Guide/Mode, short press (if delivered) | PocketPC Tab; Gizmondo unassigned (customizable) |

View/Select and Menu/Start are the small top-left and top-right buttons. Both
their tap and hold actions are reserved and outlined in red in the editor;
fullscreen/filter taps also work in launcher menus and the assignment assistant.
These change emulator controls, not SteamOS volume/brightness. Guide and Extra
remain customizable, and SteamOS may consume them before PocketHLE receives them.

### PocketPC / WinCE factory guest mappings

| Deck control | Guest action |
| --- | --- |
| D-pad / left stick | Directions |
| L1 / R1 | P1 / P2 |
| L2 / R2 | P3 / P4 |
| Left-stick click | Escape |
| Right-stick click | Reserved: single click taps only in Pointer mode; double-click switches stylus mode |
| A / B / X / Y | Action / B / C / A (Enter / Space / Shift / Ctrl) |

P1-P4 are the buttons exposed by GAPI; Action/Enter is separate. A game that
only lists GAPI buttons need not accept every keyboard key. Stick clicks and
motion are independent. Guide/Mode sends guest Tab on PocketPC when SDL receives it and has
no default long action. Extra/Misc1 cycles rotation on both profiles. Screenshot
is optional and assignable on desktop.
View hold exits either device profile; it never sends Brightness.

The right stick is fixed to the PocketPC stylus. Double-click R3 switches Stick
and Pointer modes; a single click does nothing in Stick mode and taps the cursor
in Pointer mode after the 200 ms double-click window. Keyboard P defaults to the
same gesture; its assignment and O/K/L/M stylus directions remain customizable.
Stylus speed uses a QVGA reference, giving equal proportional travel time at
QVGA and VGA. Per-game grids refuse reserved inputs and current stylus keys.
See [stylus modes](FRONTENDS.md#pocketpc-right-stick-stylus-modes) for timing,
input priority and Android behavior.

### Gizmondo factory guest mappings

| Deck control | Guest action |
| --- | --- |
| D-pad / left stick | Directions |
| A / Y / B / X | Play / Stop / Forward / Rewind |
| L1 | Volume (Piano 2) |
| R1 | Home (Piano 1) |
| L2 / R2 | Gizmondo L1 / R1 |

These are defaults, not locked guest controls: explicit profile assignments can
rebind them, including L1 volume. Reserved host gestures remain protected. Game-only
fullscreen hides the virtual Gizmondo panel and EXIT/FULL buttons; see
[FRONTENDS.md](FRONTENDS.md). It does not change the guest's LCD geometry.

## Library and settings

D-pad/stick selects a card; A launches and Y opens the game's menu.
L3 switches between page and toolbar; use the toolbar to open settings.
Menu remains the reserved filter shortcut, including in the library. L1/L2 move to the previous tab and R1/R2 to the next,
wrapping through Gizmondo, PocketPC and Recordings. Selection stays in view.

The game menu contains Information, Play, Rename, Settings, Add/Replace Data,
Remove and Game ID. A accepts;
B returns to the parent menu, then closes it. In settings, up/down moves focus,
left/right adjusts sliders and A activates the selected control. Dropdowns use
up/down, A to accept and B to cancel. Pages scroll to keep focus visible.

A on a text field opens the virtual keyboard. Directions select, A types, Y
switches symbols and Shift changes case. Apply accepts; B cancels.
File browsing for import and backups uses the integrated controller-friendly
dialog. Game removal uses an integrated confirmation with A/Enter to remove
and B/Escape to cancel. Native message fallbacks are reserved for startup errors
and Windows command-line help before a working launcher viewport exists.

Colors startup uses directions for language, A to launch and B to cancel. Its
explicit unlock action uses X; see [COLORS.md](COLORS.md).

The runtime Deck profile uses config-steamdeck.json; standard desktop uses config.json.
The game catalogue and per-game data remain shared within the same library root. Old Deck factory-binding
migrations have been removed; saved settings are read without those rewrites.
Under Proton, the default Documents/PocketHLE location may be inside the shortcut's
Windows prefix. Another shortcut/prefix can therefore resolve to another library.
For updates, keep the existing shortcut and back up the entire PocketHLE library
outside the Proton prefix before replacing program files.

Desktop exposes separate **Keyboard** and **Controller** buttons in the
Settings toolbar group. In the controller entry, clicking both sticks
(**L3 + R3**) starts **Assign an input…**; the keyboard entry uses **Ctrl+Enter**.
Both shortcuts are shown on their buttons. Release the combination before
pressing the input to bind. They are editor-only, not gameplay shortcuts.
The assistant captures the physical input
first, distinguishes short/long on release, then lets directions and A select
and apply a guest command. **Assign another input** continues; **Save and finish**
saves without returning to touch/mouse controls. The device picture and selected
input card are read-only. See [FRONTENDS.md](FRONTENDS.md#visual-input-editor--desktop-and-android)
for precedence, timing and the configuration schema.

The toolbar groups Game library, Settings, Features and About. Directions follow
its two-row layout; A activates and L3 switches toolbar/page. Filter and Rotation
show the current mode and cycle on activation. Recording shows a red **Stop video**
button while active. See [the toolbar reference](FRONTENDS.md#desktop-button-toolbar).

Finger scrolling and mouse/trackpad clicks retain the current page offset;
controller navigation reveals its target. The touch-zone editor stays centred
with fixed bounds while assignments change. The post-import PocketPC display
prompt uses A = Not Now and B = Settings directly, without moving focus first.

**Reset keys to defaults** and **Reset controller defaults** replace the old
**Apply Steam Deck preset** action. Each resets its guest table and explicit
short/long overrides across the two device profiles; controller reset also
restores View as the additional fullscreen shortcut. Stylus configuration is
not reset. Save is required. Menu remains reserved regardless of stored guest assignments.

## Physical keyboard host keys

F7 cycles the active game image rotation. F8 returns to the library from
gameplay or another launcher page. F9 advances
the same presentation-filter carousel as Menu. F10/F11 retain game capture and
recording, and F12 toggles fullscreen/windowed even in binding pages and their
assistant. F7–F12 cannot be assigned to guest commands, including through a
legacy configuration or a touch grid; the keyboard skin outlines them in red.
These are physical host keys, independent of guest VK codes. PocketPC keyboard
P1–P4 default to number-row 1–4; Tab is the assignable Center tap. Gizmondo uses
F1–F5 for its piano keys and 1/2 for L1/R1, with no extra T-to-Brightness alias.
Number-row bindings ignore Shift without changing letter layouts or text entry.
See the [factory profiles](FRONTENDS.md#factory-input-profiles) and
[host-key reference](FRONTENDS.md#colors-physical-feedback-and-host-shortcuts)
for defaults, reserved inputs and platform differences.

## Captures

A long Menu press toggles recording; release is not required to keep recording.
The flashing REC indicator is local UI, applied after game capture. In Recordings,
A opens the selected capture; B requests deletion with a confirmation initially
set to Cancel. Left/right chooses Cancel/Delete, A confirms and B cancels.
Inside the viewer B still leaves fullscreen or closes playback. These face-button
labels follow SDL3 on other controllers. Y shares only when a native share
service exists; no fixed list of sharing destinations is fabricated.

## Build

```text
cargo build --release --locked -p pocket-desktop
```

The Windows output is `target/release/pockethle-gui.exe` (or the configured Cargo
target directory). CI compiles it once and publishes the Windows archive for both PC and Deck.
Deck-profile updates under Proton select that same Windows archive.
The desktop and Android keep the same factory binding reference. Physical
Proton/controller behavior still requires device validation.
