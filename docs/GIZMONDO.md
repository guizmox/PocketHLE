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
by about 20%, and the maximum blends one sixth toward white; neither blacks out
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
