# Gizmondo features and hardware

Use Gizmondo options globally or in per-game settings where offered. Hardware
access switches apply at the next launch; the guest starts the actual service.
Windows/Linux share the desktop settings UI, but native hardware bridges are
platform-specific. See [DESKTOP-PARITY.md](DESKTOP-PARITY.md).

## Imports and game information

Demo cards prompt **Keep launcher** or **Separate games**. Separate entries own
directories, settings and saves, while retaining shared resources and card paths.
Known GZGA catalogue names supply game titles, identified from card directories
and markers. Executable contents and BMP filenames never supply titles;
a valid BMP beside the executable supplies cartridge artwork only. Import names remain editable; renaming
does not change game IDs or save paths. Static API detection reports evidence for
Bluetooth, data, GPS, camera, vibration, power and SIM; lack of static evidence
is not proof that a game cannot use a dynamically loaded API.

## Bluetooth

Enable **Bluetooth hardware (Classic / RFCOMM)** before launch, enable the host
radio, and grant Android permissions. Android may request discoverability when
hosting; accept it and start joining within that window. Pair through the OS if
required by secure connections. A discovery search alone does not prove data
exchange. Use two hosts, not two instances on one radio.

The SDK path supports discovery, advertised services, serial/COM and supported
RFCOMM socket operations. This is not a full HCI/L2CAP/controller stack or a
promise of interoperability with physical Gizmondo consoles.
[BTTEST](../tools/bttest/android-host/README.md) covers Android host diagnostics.
Desktop also provides `pockethle-bt-test server`, `scan` and `client ADDRESS`.

## GPS

Host GPS is opt-in and uses real location when available and permitted.
Simulated GPS offers manual coordinates or a point within 1 km of the Eiffel
Tower, selected once per launch. Simulation takes priority if enabled; disable
it and enable host GPS to use the real provider. Manual coordinates are retained.
Fake fixes do not require host-location permission; Bluetooth may independently
require it on older Android versions. Linux supports simulation, not a native
location bridge. [GPSTEST](../tools/gpstest/README.md) checks the ARM contract.

## Internet and Colors

Enable GPRS/data for Colors. The default replacement server origin is
`https://vps-50cdd0fc.vps.ovh.ca`; saved custom origins remain configurable.
On a phone, localhost means that phone. Windows uses WinHTTP; Android uses
HttpURLConnection with normal TLS verification and bounded transfer buffers.
HTTP/HTTPS WinINet support is not general TCP/UDP, FTP or every WinCE option.
[NETTEST](../tools/nettest/README.md) checks HTTP/HTTPS and guest errors.

## Camera

Enable CAM1 and grant permission. Windows uses its webcam backend; Android
prefers the rear camera. The game starts/stops capture through CAM1. Preview is
RGB565 and capture is I420; unsupported controls report errors. This is separate
from PocketPC DirectShow camera support. Pause/exit/last-close release capture. Android Gizmondo camera frames follow
the screen orientation, including reverse landscape, before the game performs
tracking. This rotates pixels without adding a mirror.
[CAMTEST](../tools/camtest/README.md) produces a BMP for checking a real image.

## Controls, vibration and brightness

Per-game D-pad dead zones offer Small/Medium/Large. They affect Android touch and
controller sticks; digital contacts stay digital. Keyboard/controller bindings
remain configurable. Android game vibration uses fixed amplitude where supported;
desktop rumble requires a supported connected controller. OFF/exit releases output;
closing VIB1 alone intentionally preserves the SDK's requested motor state.
[VIBTEST](../tools/vibtest/README.md) checks patterns, stop and reopening.

The Gizmondo Brightness button opens the game's menu through its guest key.
Backlight registry/events control bounded image adjustment; this is not host
screen brightness. The four Colors positions are 1/11/21/31; 11 is neutral and
matches the original image. Missing/invalid values use 11. The minimum darkens
by about 20%, and the maximum blends one twelfth toward white. Level 4 uses the
previous level 3; the new level 3 blends one twenty-fourth toward white, halfway
between neutral level 2 and the new maximum. Neither blacks out
nor whites out the screen. Screenshots follow the existing displayed-image path.

## Battery, shell status and video

Battery APIs use the host adapter where supported, with existing simulated or
unknown fallbacks. Do not equate desktop UI parity with Linux battery support.
Android reads available airplane/radio/SIM state without collecting subscriber
IDs. Windows uses simulated shell state. ActiveSync remains disconnected; no
complete GSM/TAPI/RIL stack is implemented.

DirectShow provides video/audio playback and supported seeking/window interfaces.
The launcher thumbnail receives EC_VIDEOFRAMEREADY on the first decoded video
frame and is recomposed after the menu is drawn. Final COM Release and stop cancel
and release the decoder's resources. Detailed contracts remain in
[architecture notes](AGENTS.md).

## Diagnostics and compatibility

[Compatibility](GIZMONDO-COMPATIBILITY.md) records maintainer-reported results.
[RAMTEST](../tools/ramtest/README.md) and [VFSTEST](../tools/vfstest/README.md)
exercise memory/process and filesystem contracts. Run hardware tests explicitly;
they do not run automatically when importing a game. GPU differences and visual
regression checks are documented in [GPU.md](GPU.md).


## Colors language

At the bottom of Gizmondo options, **Colors language** selects English, French, German,
Italian or Spanish for the next launch. The preference is global and becomes
available when Colors (GZGA200015 or GZGA270015) is installed. If the chosen translation is
missing, Colors uses English. The installation must contain the translation
beside `bstrings.eng`; no translation files are bundled and the game executable
and original resources remain unchanged. Other titles keep their own language.

Desktop cartridge BMP artwork preserves its aspect ratio, centred within the
white SD-card label. Wide images fit its full width; tall images also respect
the label height. Artwork never stretches into the Gizmondo band.


## GZGA collection ZIP imports

ZIP classification follows this priority:

1. `autorun.exe` and `multiboot.hog` together at the ZIP root: demo card, with
   the existing keep-launcher/separate-games choice.
2. Otherwise, import every `GZGA######` directory containing a guest executable
   as an independent game, even if there is only one, or other executables are
   present. GZGA directories inside wrapper folders are supported. A launcher/HOG
   pair inside a GZGA belongs to that game and does not classify the entire ZIP.
3. Other layouts use the ordinary single-game import.

Each GZGA entry gets only the contents of its own directory. Files and folders
outside runnable GZGA directories are ignored, including Movies and common
resources; this collection import is distinct from demo-card splitting. Known
IDs use the static catalogue, including ALT labels. Data-only GZGA directories
without an executable are resources rather than launchable games.

Desktop and Android block library interaction while importing or splitting a
card, then restore it on completion or error. The desktop also blocks the
underlying interface while the demo-card choice is pending.

ZIP imports recursively discard a sole enclosing directory before classification.
A GZGA boundary is retained for identification. Independent games are installed
as `games/GZGAxxxxxx/extracted/<payload>`, without the archive name or a second
GZGA directory. Unidentified ordinary games use `games/<archive-id>/extracted/`.
Demo cards kept as launchers retain their internal shared card layout; split
cards use the same independent game layout. Reimporting a legacy collection
migrates matching entries while retaining their settings, custom names and saves.

Platform identification accepts an exact GZxx plus six-digit directory name even
when the directory is empty: CatData.zip has root Autorun.exe and GZGA800069/,
without a serial marker file. This is Gizmondo, not PocketPC. The directory serial
identifies the platform independently of catalogue title availability; unknown
serials are not silently equated with another release of the same game.

An exact serial filename alongside the executable also identifies Gizmondo,
with or without a directory or nested marker file. Serial matching ignores case;
extensions such as GZGA800069.txt are not serial markers.

Flattened serial marker files also supply the emulated SD-card volume serial.
The SDK encoding is numeric title ID minus volume serial minus one, modulo
2^32. Reading only nested markers regressed Toy Golf startup after flattening;
the storage driver now decodes either direct files or original nested markers.

New installations enable Camera, GPS and Bluetooth in Gizmondo options by
default. Existing serialized preferences, including disabled values, are
preserved. Host services are still acquired only when requested by the guest;
platform permission handling is unchanged. Simulated GPS remains opt-in.

Serial mount aliases use a matching physical GZGA directory when present.
Only flattened layouts map the alias to the extraction root. This prevents
Supernaturals marker reads from resolving to a directory rather than its
four-byte marker file, while preserving flattened Toy Golf card paths.

Sole-directory unwrapping stops at every exact GZxx###### SDK card boundary,
including GZVT diagnostic cards, not just commercial GZGA directories. SDK
volume checks require those card-relative paths to remain valid.

Android library cards always display their catalogue/custom title below the
card, including artwork-backed tiles. The desktop-style lower band shows the title, platform/resolution and
exact card serial, distinguishing ALT versions even with identical artwork.

Android lower-band titles use horizontal marquee for overflowing names,
including ALT suffixes. Short names remain stationary.

Colors hides its DirectShow video window during intro preparation, then calls
Run with AutoShow enabled. The graph must show that window on an actual Run
or Pause transition. Repeated Run while already playing must not undo a live
explicit hide, and AutoShow disabled must preserve hidden playback. This
fixes audio-only intros without changing decoding or adding a Colors exception.

GZGA800016 is catalogued as Chicane (E3 demo), independently of the licensed
cars version and the retail title.

Additional alternative-version catalogue entries:

| Card ID | Title |
| --- | --- |
| GZGA800015 | Colors (E3 demo) |
| GZGA700016 | Chicane (early demo) |
| GZGA900032 | Conflict Vietnam (Interim Demo) |
| GZGA200097 | Unfinished Business (Gametrac Demo) |

These codes were unused in the shared catalogue when added. The 700/800/900
codes are local variant labels; they are not inferred official cartridge IDs.
Existing Colors and Chicane variants retain their codes and names. Desktop and
Android both read this shared table. Game folders must use the assigned code.

### Updating an installed game

Desktop and Android prepare imports in an isolated temporary library before
publishing files. Existing IDs trigger an English update confirmation listing
the affected games. A mixed GZGA collection lists updates and the count of new
games; cancelling discards the entire prepared import. Normal ZIP classification
and directory unwrapping are unchanged.

Confirmation replaces the imported game contents and refreshes imported metadata
and artwork. Per-game saves, shared Flash Disk data, custom titles and game
settings are retained. Current preferences and saves are read at publication,
not at preparation. Publication uses the existing backup/rollback transaction;
an installation appearing during the dialog requires a new confirmation. The
library UI stays unavailable during preparation, confirmation and publication.
Updating a demo-card launcher does not automatically split it again.

The catalogue names for GZGA900069 and GZGA800056 are respectively
`Catapult (august 2005)` and `Agaju (august 2005)`. Opening an older library
corrects the former automatic May/October names, leaving custom titles intact.

### Supernaturals demo skybox

The supplied GZGA200099 executable sets a camera's far plane to 4.5, while its
sky cube has coordinates +/-4 and some transformed corners lie beyond that
plane. Correct far-plane clipping cuts triangular gaps out of the sky in both
GPU and software rendering. The old software path masked the problem by not
clipping the far plane; commit 64276c2f7920a34e88c4ce859ab4b9257c750824 added
six-plane clipping. The six sky textures are present and decode correctly.

`tools/patch-supernaturals-skybox.py` recognises the exact supplied executable
by SHA256 and changes the camera far plane from 4.5 to 16. The single changed
byte is at file offset 0x12D20 (ARM instruction at guest VA 0x23920); all assets
and other executable bytes are unchanged. This camera is used by Journal and
may also be used by gameplay, so this is a game-executable correction, not a
Journal-only renderer exception. The separate camera instruction at 0x25340
is unchanged. Journal was checked in both rendering modes; full-game coverage
is not implied. PocketHLE's clipping implementation is unchanged.

Usage: `python3 tools/patch-supernaturals-skybox.py Supernaturals.exe fixed.exe`.
The output must not already exist. Unknown builds are rejected before writing.

### Original SDK paths in alternative versions

`GameEntry::install_mounts` keeps declared installation aliases and adds SD Card
and Storage Card aliases for exact GZGA files/directories alongside the executable
or at the extraction root. A marker file maps to its containing game directory;
a physical GZGA directory maps to itself. This is startup-only discovery and does
not rename entries, move files, change saves or guess aliases from game titles.
All variants retaining their original SDK markers use this common path.

Colors (E3 demo), catalogued as GZGA800015, still opens GZGA200015 paths. Its
provided files include that original marker. Without the alias, startup loads an
incomplete registry then dereferences a missing Camera node at 0x000ACBD4.
Mounting the original card path resolves the asset loading and reaches the E3
main menu. Desktop and Android both use the same alias list, including already
installed games; no reimport or binary patch is required.

Known alternative catalogue builds (local GZGA700/800/900 IDs) use their own
`games/<game-id>/saves/flash/` as the guest `\Flash Disk\`, on Windows and Android.
Their SDK save paths stay unchanged, but configurations from another build are
not shared: Colors E3 can crash before its menu when loading the standard build's
`MyGames/GZGA200015 Colors/config.cfg`. Standard titles continue using the existing
shared `flash/`. Existing shared saves are retained; alternatives start with an
empty isolated Flash Disk rather than copying potentially incompatible files.
Reimport updates preserve these isolated saves. Recognition uses catalogue IDs
and declared card aliases, so renaming a title does not change this behavior.

### Unfinished Business: zlib

The provided zlib.dll exports confirm ordinal 22 = inflateInit_, 19 = inflate,
20 = inflateEnd and 2 = compress. Their named/ordinal handlers now implement the
56-byte WinCE z_stream ABI using owned per-process flate2 streams. Input/output
pointers, remaining counts, totals, Adler checksum and zlib status values are
updated across calls. Streams are released by inflateEnd or process teardown.
Version/size checks and bounded buffers reject invalid requests. Compression is
also implemented for the game's save path. Other zlib functions remain outside
this initial subset. No dependency was added and the supplied game DLL is intact.
The supplied demo reaches its rendered first-person scene.

### Initial SDL 1.2 guest support

Named SDL.dll imports now cover video/timer initialization, software 16/24/32-bit
surfaces, RGB(A) mapping, clipping/fills, blits with color key/alpha, lock/unlock,
presentation, keyboard/pointer events, key-state queries, delay/ticks and error/
version queries. Surfaces use the 32-bit SDL 1.2 ABI in guest-owned heap memory;
presentation converts to PocketHLE's RGB565 framebuffer. Delay uses the existing
guest scheduler. Offscreen surfaces and static query pointers survive SDL_Quit
until the caller frees them or the process ends. State is process-owned.

This is a partial SDL 1.2 implementation, separate from the desktop launcher's
host SDL2. SDL2 guest ABI, SDL audio, joystick/CD-ROM subsystems, paletted video,
SDL OpenGL, RWops, SDL_image/SDL_mixer and ordinal-only SDL DLL imports are not
implemented. Unsupported subsystems/video modes report failure; they do not
claim success. Neither supplied demo imports SDL, so validation uses ABI unit
tests and a native ARM diagnostic (Init/SetVideoMode/FillRect/Flip/ExitProcess),
not a claim of complete SDL-game compatibility. Additional DLL/game samples
are needed to establish ordinal mappings and extend coverage.
