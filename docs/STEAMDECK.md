# PocketHLE-Reborn — Steam Deck profile

The desktop uses one executable. The Steam Deck profile is selected at launch
with `--steam-deck`; it is not a separate Cargo feature or executable.

## CI artifact and startup

Download **PocketHLE-Reborn-windows-x86_64** and extract its ZIP. The Deck uses the
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

## Controller reference

Deck A/B/X/Y markings correspond to SDL's South/East/West/North positions.
Other controllers display the labels supplied by SDL3, with position names as
the fallback. Saved guest assignments remain positional; physical markings do
not change the emulated command.

Use the shared [factory profiles](FRONTENDS.md#factory-input-profiles) for the
PocketPC and Gizmondo guest tables, and [host shortcuts](FRONTENDS.md#colors-physical-feedback-and-host-shortcuts)
for View/Menu tap/hold actions and reserved keys. Those tables are maintained in
one place rather than copied into a second Deck preset. View and Menu are the
small upper-left and upper-right buttons, not L1/R1. SteamOS may consume Guide or
Extra before PocketHLE-Reborn receives them.

PocketPC's right stick is fixed to the stylus; double-click R3 changes its mode.
See [stylus modes](FRONTENDS.md#pocketpc-right-stick-stylus-modes) for click timing,
keyboard assignment, speed and per-game grid conflicts.

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
explicit unlock action uses X; see [the Colors reference](GIZMONDO.md#colors).

The runtime Deck profile uses config-steamdeck.json; standard desktop uses config.json.
The game catalogue and per-game data remain shared within the same library root. Old Deck factory-binding
migrations have been removed; saved settings are read without those rewrites.
Under Proton, the default Documents/PocketHLE-Reborn location may be inside the shortcut's
Windows prefix. Another shortcut/prefix can therefore resolve to another library.
An existing default Documents/PocketHLE folder is renamed on first use when the destination is absent.
For updates, keep the existing shortcut and back up the entire PocketHLE-Reborn library
outside the Proton prefix before replacing program files.

Desktop exposes separate **Keyboard** and **Controller** toolbar buttons.
**Ctrl+Enter** on the keyboard page or **L3 + R3** on the controller page starts
**Assign an input…**. Release the launch combination, then capture the physical
input and select a guest command. Reset, short/long precedence, draft saving,
physical feedback and the complete workflow are maintained in
[the shared input-editor reference](FRONTENDS.md#visual-input-editor--desktop-and-android).

The toolbar groups Game library, Settings, Features and About. Directions follow
its two-row layout; A activates and L3 switches toolbar/page. Finger scrolling
and mouse/trackpad selection retain the page offset; controller navigation reveals
its target. The post-import **PocketPC launch settings** prompt directly uses
A = Not Now and B = Settings, independently of old widget focus. See
[toolbar behavior](FRONTENDS.md#desktop-button-toolbar) and
[focus/dialog rules](FRONTENDS.md#desktop-focus-and-import-launch-settings-prompt).

## Physical keyboard host keys

F7–F12 are reserved for PocketHLE-Reborn, independent of guest VKs. The
[shared host-key table](FRONTENDS.md#colors-physical-feedback-and-host-shortcuts)
owns their actions and platform differences; keyboard defaults are in
[factory profiles](FRONTENDS.md#factory-input-profiles). F8 is shown on Library
and F12 remains available for fullscreen in the binding editor.

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
