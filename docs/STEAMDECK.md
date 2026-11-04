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

## Reserved controls during gameplay

| Deck control | Action |
| --- | --- |
| Menu, short press | Next presentation filter |
| Menu, hold 650 ms | Start/stop video recording |
| View, short press | Toggle fullscreen |
| View, hold 650 ms | Gizmondo brightness; exit game on PocketPC |

These host gestures take precedence over an old default-binding migration note.
They change emulator presentation/device controls, not SteamOS volume/brightness.

### PocketPC / WinCE

| Deck control | Guest action |
| --- | --- |
| D-pad / left stick | Directions |
| L1 / R1 | P1 / P2 |
| L2 / R2 | P3 / P4 |
| Left-stick click | Escape |
| Right-stick click | Action / Enter |
| A / B / X / Y | A / B / C / Tab (Ctrl / Space / Shift / Tab) |

P1-P4 are the buttons exposed by GAPI; Action/Enter is separate. A game that
only lists GAPI buttons need not accept every keyboard key. Stick clicks and
motion are independent. If SDL exposes Guide/Mode or Misc1, their long press
can exit a PocketPC game; SteamOS may reserve these controls.

### Gizmondo

| Deck control | Guest action |
| --- | --- |
| D-pad / left stick | Directions |
| A / Y / B / X | Action / A / B / C |
| L1 | Volume (Piano 2) |
| R1 | Home (Piano 1) |
| L2 / R2 | Gizmondo L / R |

Customized Gizmondo bindings remain, except L1 reserved for volume. Game-only
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
switches symbols and Shift changes case. Apply accepts; B cancels. External
file dialogs remain OS dialogs. Initial import may need a trackpad or keyboard.

Colors startup uses directions for language, A to launch and B to cancel. Its
explicit unlock action uses X; see [COLORS.md](COLORS.md).

The Deck edition uses config-steamdeck.json; standard desktop uses config.json.
They may share a library. **Apply Steam Deck preset** restores the preset;
custom bindings should otherwise be preserved. Do not confuse historical
Start=Home/Volume migration values with the reserved Menu gestures above.

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
folder. The standard build retains its own control policy. Native services and
physical Deck behavior require device validation; build success alone is not it.
