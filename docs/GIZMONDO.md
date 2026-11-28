# Gizmondo guide

Import games, configure hardware and use GTShell or Colors. Unless stated
otherwise, these features are available on both Desktop and Android.
For option values, see [Gizmondo settings](OPTIONS.md#global-gizmondo-options);
for bindings and controller navigation, see [Controls](FRONTENDS.md).

- [Importing games](#imports-and-game-information)
- [VHD images](#vhd-card-images) and [encrypted executables](#encrypted-executables)
- [Hardware](#bluetooth)
- [Game compatibility](#reported-game-compatibility)
- [GTShell](#gtshell-profiles)
- [Colors startup menu](#colors)

## Imports and game information

Import a game archive or executable through the library's **Import** action.
Keep the card's data files and directory structure with its executable.
Demo cards offer **Keep launcher** or **Separate games**. Separate games have
their own library entries, settings and saves.

Known card IDs supply catalogue names. You can rename entries without changing
their save locations. A BMP beside the executable can supply card artwork.
**Information** shows detected graphics and hardware use; dynamically loaded
features may not appear in this list.

### GZGA collection ZIP imports

A ZIP containing runnable `GZGA######` directories can import several games at
once, including directories inside a wrapper folder. Each game receives its own
directory contents; files outside those game directories are not included.
For cards that depend on shared resources, keep the demo-card launcher layout.

A root `autorun.exe` plus `multiboot.hog` identifies a demo card and offers the
launcher/separate-games choice. Other ZIP layouts use ordinary single-game import.

### VHD card images

Import a Gizmondo SD-card dump directly from a **fixed `.vhd` image**. Supported
images contain a single FAT12, FAT16 or FAT32 partition, or an unpartitioned FAT
volume. Dynamic VHD, differencing VHD and VHDX are not supported.

PocketHLE-Reborn extracts the complete card into the library, keeping its folder
structure and launcher rather than splitting its games. A root `autorun.exe`
takes priority. The image filename supplies the initial library name.
You do not need to mount the image in the host OS; the source VHD is unchanged.

### Encrypted executables

Recognized encrypted Gizmondo EXEs are **decrypted automatically in memory when
loaded**, including games started by a card launcher. This works with VHD, ZIP
and executable imports on Desktop and Android. No separate decryption utility
or Windows helper is needed, and the imported files remain unchanged on disk.
A recognized but damaged encrypted executable produces a loading error.

### Updating an installed game

Reimporting an existing game asks for confirmation before replacing its installed
data. Saves, custom titles and game settings are retained. Cancelling abandons
the prepared import. Updating a card kept as a launcher does not split it.

## Saves and card data

SD-card writes are stored under `games/<id>/saves/sd-card/`; imported assets stay
unchanged. Standard games share the library's `flash/` directory for Flash Disk
saves. Recognized alternative builds use `games/<id>/saves/flash/` to avoid
sharing incompatible settings with another version. Reimports retain saves.

For emulator snapshots, see [Save/Restore Snapshot](FRONTENDS.md#experimental-snapshots).
Library **Backup / Restore** is a separate operation that transfers library data.

## Bluetooth

Enable **Bluetooth hardware (Classic / RFCOMM)** before launch and turn on the
host radio. Grant Android permissions and accept discoverability when hosting.
Pair through the OS if the connection requires it, then join from the other host.
Use two hosts rather than two instances sharing one radio.

Supported games use real host Bluetooth. Finding a host does not guarantee a
successful multiplayer connection; interoperability with physical Gizmondo
consoles is not guaranteed.

## GPS

Enable host/device GPS and grant location permission to supply real location
when the provider is available. Native host location is supported on Windows
and Android; Linux supports simulated fixes. Colors offers its own simulated
location choices in the [startup menu](#colors).

## Internet

Enable **GPRS/data** to allow supported games to use the host Internet connection.
Online features still require an available compatible server. Colors provides
its server setting in the startup menu below.

## Camera

Enable **Camera hardware (CAM1)** and grant permission. Supported games request
the host camera themselves. Windows uses a webcam; Android prefers the rear
camera and follows screen orientation. Native camera availability differs by
host platform; enabling the option does not provide missing hardware.

## Controls, vibration and brightness

Keyboard and controller bindings are configurable; Android also provides touch
controls. See [default bindings and input setup](FRONTENDS.md#factory-input-profiles).
Vibration requires supported Android hardware or a rumble-capable host controller.

The Gizmondo Brightness button opens the game's own brightness control. Its
adjustment affects the emulated image rather than the physical screen's brightness.

## Reported game compatibility

See the [compatibility catalogue](COMPATIBILITY.md) for additional titles and
variants. The results below are maintainer-reported; hardware and online features
also depend on host permissions, connected devices and available services.

| Game / card | Status | Additional features |
| --- | --- | --- |
| Agaju — The Sacred Path of Treasure | OK | Camera |
| Alien Hominid | OK | — |
| Ball Busters | OK | — |
| Battlestations | OK | — |
| Carmageddon | OK | — |
| Catapult | OK | Camera, Bluetooth |
| Chicane | OK | Bluetooth |
| Classic Compendium | OK | — |
| Classic Compendium 2 | OK | — |
| Colors | OK | GPS, data, camera |
| Conflict: Vietnam | OK | — |
| Fathammer Classics | OK | — |
| FIFA Football 2005 | OK | — |
| Gizmondo Demonstrations / Demo Card | OK | — |
| Gizmondo Motocross 2005 | OK | — |
| Hit & Myth | OK | — |
| Kiosk Card | OK | — |
| Pocket Ping Pong 2005 | OK | — |
| Richard Burns Rally | OK | — |
| Sticky Balls | OK | — |
| Stuntcar Extreme | Runs with a visual issue reported | Blue artifacts behind some HUD text. Use internal resolution ×1 for now. | Bluetooth |
| Toy Golf | OK | — |
| Trailblazer | OK | — |
| Hockey Rage 2005 | OK | — |
| Interstellar Flames 2 | OK | — |
| Point of Destruction | OK | — |
| SSX 3 | OK | — |

## GTShell profiles

GTShell runs through the standard Desktop and Android applications. You must
supply the original Flash Disk and Flash Disk2 contents, including the shell
executable, required DLLs, skins and language files. An SD card is optional.
These files are not distributed with PocketHLE-Reborn.

### Preparing a profile

Create an importable backup profile:

```sh
python3 tools/make-gtshell-profile.py --flash-disk "device/flash disk" \
  --flash-disk2 "device/flash disk2" --sd-card "device/sd card" \
  --output "gtshell-profile.zip"
```

Use `--gtshell PATH` if you need to select a different shell executable.
Supply physical directories rather than symlinks or Windows junctions.
Restore the ZIP through the normal library backup menu. **Merge without
overwrite** keeps existing settings; replacement can overwrite your library.

### Bluetooth file transfer

Enable Bluetooth on both hosts before launching GTShell. On the receiver, open
**Bluetooth → Receive files**. On the sender, select an image, choose
**Send → Bluetooth**, select the receiving host and accept the transfer there.
Follow the host permissions, discoverability and pairing steps above.
Received images appear in the gallery or `MyPictures` on the profile's Flash Disk.

## Colors

### Startup menu

Retail Colors cards (`GZGA200015` and `GZGA270015`) show **Colors startup** before
launch on Desktop and Android. Demo/alternative cards launch directly and do
not receive these overrides.

| Choice | Effect |
| --- | --- |
| Language | Choose among the languages present on your card. No translations are bundled. The previous selection is remembered. |
| Fake GPS | Disabled initially at each launch. Enable to use a point near the Eiffel Tower or manual latitude/longitude instead of real GPS. |
| Near Eiffel Tower | Selects a simulated point within 800 m for this launch. |
| Manual coordinates | Enter latitude −90..90 and longitude −180..180. Coordinates and the chosen mode are remembered. |
| Fill GPS | Android only: fills the draft coordinates from the device location. Does not enable Fake GPS automatically. |
| Colors multiplayer server | Uses the remembered server address; blank restores the default replacement server. |
| Unlock Turf Wars + Tag Editor | Explicitly unlocks these features while preserving missions, money and other save data. Does not run automatically. On Steam Deck, X invokes this action. |

**Start** saves the choices and launches; **Cancel** discards draft changes.
Simulated GPS does not require host location access. For real GPS, leave Fake GPS
disabled and enable the host/device GPS service.

With **Fake GPS → Near Eiffel Tower** selected, the Geofence button chooses a
new point within 800 m. Its default Desktop key is **4**. Manual coordinates and
real GPS keep the normal Geofence behavior.

The unlock action backs up a changed configuration as `config.cfg-before-turf-*.bak`.
It does not create a missing save or add money. Solo and online balances remain
separate; online play requires a compatible server.
