# PocketHLE — Steam Deck edition

This edition is the dedicated Windows build for SteamOS through Proton.
It is distinct from the native Linux desktop build.

## CI artifact and startup

Download **PocketHLE-steamdeck-x86_64** from a successful CI run and extract
`PocketHLE-steamdeck-x86_64.zip`. Point the existing Steam shortcut at
`pockethle-steamdeck.exe`, enable Proton and select a Gamepad Steam Input layout.
The SDL Proton profile is built in; launch options can remain empty.

The dedicated edition requests fullscreen at window creation and on its first
UI frame after native state restoration. An explicit CLI display argument wins:
0 means fullscreen; 1 means windowed. This does not enable automatic Deck
identification in an ordinary desktop executable.

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

These host gestures take precedence over an old default-binding migration note.
They change emulator presentation/device controls, not SteamOS volume/brightness.

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
SteamOS may reserve these controls before they reach the application.

### Gizmondo factory guest mappings

| Deck control | Guest action |
| --- | --- |
| D-pad / left stick | Directions |
| A / Y / B / X | Action / A / B / C |
| L1 | Volume (Piano 2) |
| R1 | Home (Piano 1) |
| L2 / R2 | Gizmondo L / R |

These are defaults, not locked guest controls: explicit profile assignments can
rebind them, including L1 volume. Reserved host gestures remain protected. Game-only
fullscreen hides the virtual Gizmondo panel and EXIT/FULL buttons; see
[FRONTENDS.md](FRONTENDS.md). It does not change the guest's LCD geometry.

## Library and settings

D-pad/stick selects a card; A launches; X changes category; Y opens the game's
menu; Start opens settings. L1/L2 move to the previous tab and R1/R2 to the next,
wrapping through Gizmondo, PocketPC and Recordings. Selection stays in view.

The game menu includes Information, Play, Rename, Settings and Remove. A accepts;
B returns to the parent menu, then closes it. In settings, up/down moves focus,
left/right adjusts sliders and A activates the selected control. Dropdowns use
up/down, A to accept and B to cancel. Pages scroll to keep focus visible.

A on a text field opens the virtual keyboard. Directions select, A types, Y
switches symbols and Shift changes case. Apply accepts; B cancels.
File browsing for import and backups uses the integrated controller-friendly
dialog. Native error/confirmation dialogs may still be shown by the OS.

Colors startup uses directions for language, A to launch and B to cancel. Its
explicit unlock action uses X; see [COLORS.md](COLORS.md).

The Deck edition uses config-steamdeck.json; standard desktop uses config.json.
They may share a library. **Keyboard bindings** and **External controller bindings**
are separate settings entries. In the controller entry, clicking both sticks
(**L3 + R3**) starts **Assign an input…**; the keyboard entry uses **Ctrl+Enter**.
Both shortcuts are shown on their buttons. Release the combination before
pressing the input to bind. They are editor-only, not gameplay shortcuts.
The assistant captures the physical input
first, distinguishes short/long on release, then lets directions and A select
and apply a guest command. **Assign another input** continues; **Save and finish**
saves without returning to touch/mouse controls. The device picture and selected
input card are read-only. See [FRONTENDS.md](FRONTENDS.md#visual-input-editor--desktop-and-android)
for precedence, timing and the configuration schema.

**Reset keys to defaults** and **Reset controller defaults** replace the old
**Apply Steam Deck preset** action. Each resets its guest table and explicit
short/long overrides across the two device profiles; controller reset also
restores View as the additional fullscreen shortcut. Stylus configuration is
not reset. Save is required. Do not confuse historical Start=Home/Volume
migration values with the reserved Menu gestures above.

## Physical keyboard host keys

F7 cycles the active game image rotation. F8 returns to the library from
gameplay or another launcher page. F9 advances
the same presentation-filter carousel as Menu. F10/F11 retain game capture and
recording, and F12 toggles fullscreen/windowed even in binding pages and their
assistant. F7–F12 cannot be assigned to guest commands, including through a
legacy configuration or a touch grid; the keyboard skin outlines them in red.
See [FRONTENDS.md](FRONTENDS.md#visual-input-editor--desktop-and-android) for
menu behavior and draft persistence. These are physical host keys, independent
of the guest VK codes sent by the Deck's game buttons.

## Captures

A long Menu press toggles recording; release is not required to keep recording.
The flashing REC indicator is local UI, applied after game capture. In Recordings,
A opens the selected capture. Y shares only when a native share service exists;
no fixed list of sharing destinations is fabricated.

## Build

```text
cargo build --release --locked -p pocket-desktop --features steam-deck --bin pockethle-steamdeck
```

Output is target/release/pockethle-steamdeck.exe, or the CI's configured target
folder. The standard desktop and Android use the same factory binding reference;
the dedicated edition has its own startup behavior and configuration filename. Native services and
physical Deck behavior require device validation; build success alone is not it.

The small top-left View/Select (Full) and top-right Menu/Start (Filter) buttons
are reserved for PocketHLE, including their holds. Their mapping outlines are
red and neither gesture can be assigned to a guest command. Fullscreen and
filter taps remain active in launcher menus and the assignment assistant.

Keyboard profiles use PocketPC P1/P2/P3/P4 on number-row 1/2/3/4. Gizmondo
keeps F1–F5 for its piano keys, uses L1/R1 for its shoulder command names, and
has no factory T-to-Brightness alias. PocketPC has no factory Gizmondo F-key
actions. Number-row binding events ignore Shift via physical digit positions;
AZERTY letter mappings and ordinary text entry retain their layout.

F7 cycles the active game's presentation rotation and is reserved. Keyboard Tab
defaults to a 60 ms Center tap only for PocketPC; Guide sends guest Tab.
Center tap and Extra's Rotation remain assignable commands.

PocketPC reserves the right stick and R3 in every assignment editor, including
per-game grids. Double-click R3 to switch between automatic Stylus stick drags
and a visible Pointer. Single R3 does nothing in Stick mode; in Pointer mode it
taps the cursor position after the 300 ms double-click window. A mode notice
appears at the filter-notice location. The only setting is Stylus speed (pixels),
shared by both modes. Old enable and controller-direction options are removed.
See [FRONTENDS.md](FRONTENDS.md#pocketpc-right-stick-stylus-modes) for timing,
priority, cancellation and Android parity.

Desktop launcher buttons are grouped into Game library, Settings, Features and
About. D-pad/left stick navigate their actual two-row positions and reveal
buttons outside the viewport on small windows; A activates and L3 switches
toolbar/page. Filter and Rotation show the selected mode and cycle on activation.
The recording button is red and labelled Stop video while recording.
