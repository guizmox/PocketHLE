# Settings and options

This is the reference for settings exposed by the Desktop and Android interfaces.
It separates emulator defaults from settings for one PocketPC or Gizmondo game.
For import, controller navigation and the live grid editor, see [Frontend workflows](FRONTENDS.md).
For rendering details and limits, see [Graphics](GPU.md).

## Where to change settings

On Desktop, open **Emulator**, **PocketPC**, **Gizmondo**, **Keyboard** or
**Controller** in the Settings toolbar group. Open a game's menu and choose
**Settings** for its overrides. **Save** commits a settings draft; **Cancel**
discards it. On Android, use the library's settings pages or the game's
**Settings** menu; accepted changes are saved immediately.

Unless stated otherwise, relaunch the game to apply changes. Global defaults
apply to both platforms' games where relevant. An explicit per-game override
takes precedence over its global default. Imported settings can differ from
factory defaults: automatic detection and the import assistant can select them.

Desktop stores defaults in `config.json`, or `config-steamdeck.json` with
`--steam-deck`. Android uses the shared configuration model in its app library.
Per-game entries are persisted in `library.json` and the game's `game.json`. These are shared model
contracts, not a promise that every platform exposes the same controls.

## Emulator options

### Graphics — Desktop and Android

| Option | Choices and behavior |
| --- | --- |
| **GPU rendering** | On by default. Selects host OpenGL for GLES 1.x; off selects PocketHLE's software rasterizer for GLES 1.x. GLES 2.0 uses host OpenGL regardless. This does not change GDI/GAPI software drawing into GPU game rendering. |
| **OpenGL ES internal resolution (host OpenGL)** | **Native**, **2×**, **3×**, **4×**; default Native. Multiplies the native EGL surface size for supported GLES 1.x and 2.0 rendering. Driver/engine limits still apply. |
| **Apply to all resolutions, including sizes other than 320×240** | With a multiplier selected, unchecked restricts scaling to 320×240 and 240×320. Checked also scales other surface sizes. It does not force the guest to report another screen size. |
| **Frame rate limit** | **10**, **15**, **20**, **30**, **45**, **60**, **120 fps**, **Unlimited**; default 60. The default for games without an override. Caps completed presentations, including eligible GDI paint completion, GAPI, DirectDraw and EGL; intermediate updates can be coalesced. A limit cannot speed up a slow game and is independent of recording FPS. |

Host OpenGL can itself be implemented by a software driver. PocketHLE does not
bundle a selectable llvmpipe backend. See [rendering and presentation](GPU.md#rendering-and-presentation)
before interpreting `CPU_UPLOAD` or `GPU_SHARED` messages.

### Desktop-specific graphics and recording

| Option | Choices and behavior |
| --- | --- |
| **Graphics adapter** / **Choose GPU in Windows…** | On native Windows, shows detected adapters and opens Windows Graphics settings. Select this executable there, then restart PocketHLE. This is an OS preference, not an in-app adapter switch. Not shown under Wine/Proton or on Linux. |
| **VSync (PC / Steam Deck)** | **Synchronize with the display**, on by default. Controls the host window's swap synchronization. Restart the application to apply; independent of the game's frame limit. |
| **Recording format** | **AVI (MJPEG + PCM)** or **MP4 (H.264 + AAC)** when the encoder is available. An unavailable MP4 encoder is reported in the interface. |
| **Recording quality / frame rate** | Low, Medium or High at **30 FPS** or **60 FPS**. Default Medium at 30 FPS. Affects recordings, not emulation speed or internal resolution. |

Android has no settings for Desktop VSync, Desktop recording presets or the
Desktop **Upscale ×2** window action. GLES internal-resolution scaling is
available on both platforms and is a separate feature.

### Misc and diagnostics

| Option | Availability and behavior |
| --- | --- |
| **Show FPS on game screen** / **Show FPS** | Both; on by default. Displays the presentation counter inside the game image. It is not a count of game simulation steps or unique visual changes. |
| **Show IPS on game screen** / **Show IPS** | Both; off by default. Changed images per second, based on the logical RGB565 image. GLES measurement uses CPU pixel readback and can affect performance; this is not an instruction counter. |
| **Executable heap** | Both; Enabled or Disabled, default Enabled. Some games require executable heap memory; others can run faster with it disabled. PocketPC games can override it individually. |
| **Library root** / **Game library folder** | Read-only display of the active library directory. Not a folder picker. Desktop can select a custom root with `POCKETHLE_LIBRARY`; see [library storage](../README-DETAILED.md#library). |
| **Clean up CAB install files** | Both. Moves verified redundant installer aliases into `cab-alias-backup-*` under the library root, preserving their game-relative paths. The result reports the backup path and skipped/failed folders. It does not delete files merely because they have a numbered extension. Run while no game/import is active. |
| **Verbosity (0..3)** / **Log verbosity** | Both; normal default 1. Higher levels produce more diagnostic output and can affect timing. |
| **Log unimplemented APIs** / **Unimplemented API report** | Both. Separate missing-API report: Desktop `pockethle-unimplemented-apis.log`, Android `pockethle-unimplemented.log`, under the library root. Desktop applies after Save, including a running game. |
| **Graphics diagnostics** | Android only; off by default, next launch. Collects CPU/transfer timings and sampled GPU timings. Disable after the investigation; see [collecting and interpreting logs](ANDROID-GRAPHICS-DIAGNOSTICS.md). |

The cleanup operation and imports use the common CAB/library implementation.
Backups are retained on disk; the button is not an automatic backup deletion tool.

### Android display and controls

Under **Display and controls**:

| Option | Choices and behavior |
| --- | --- |
| **Control opacity** | 10–100%; adjusts the on-screen control overlay. |
| **Filter** | No filter, Reconstruction, SMAA, SMAA Soft, xBRZ, Bicubic, Lanczos, Bilinear, Nearest. Presentation filtering is separate from GLES internal resolution. xBRZ includes CPU work. See [filters](GPU.md#display-size-and-filters). |
| PocketPC arrows and stick size | **Normal** or **Big**. Bigger controls can reduce the space available for the game image. |
| Gizmondo arrows and action buttons size | **Normal** or **Big**. Independent of the PocketPC control size. |

## Global PocketPC options

These are defaults for PocketPC/Windows Mobile games, not Gizmondo settings.
Android groups the layout choice under **Touch controls**.

| Option | Choices and behavior |
| --- | --- |
| **Processor profile** | **Intel PXA270 (Dell Axim X51v)**, **Intel PXA255**, **StrongARM SA-1110**, **Generic ARM**. Changes the reported processor identity, not CPU execution speed or the CPU backend. A MIPS executable retains its MIPS identity. |
| **Tilt input (accelerometer)** | **Use physical accelerometer**, **Simulate left/right tilt with arrows**, **Simulate tilt in four directions with arrows**. Per-game PocketPC settings can override this default. |
| **Device name (Preset ID)** | Readable device name, encoded as the PocketPC identity. Desktop labels it ASCII; Android labels it WinCE. |
| **Console identifier (Platform ID — hexadecimal)** | Platform identifier bytes in hexadecimal. Identity fields are validated; changing identity can affect software that stores registrations or device-specific configuration. |
| **Touch layout** | Android only: **Classic** or **Controller**, default Classic. Same guest commands, different on-screen arrangement; independent of per-game Control mode. |

Tilt input supplies the *guest accelerometer*. It is different from the
per-game accelerometer mapping below, which turns physical movement into arrows
or relative stick aiming.

## Global Gizmondo options

| Option | Behavior |
| --- | --- |
| **GPS / host location (GPS1)** / **GPS / device location (GPS1)** | Allows GPS1 to use the real host location provider where available. Host permission and location availability still apply. |
| **GPRS/data** | Allows supported Gizmondo data requests to use the host Internet connection. |
| **Bluetooth hardware (Classic / RFCOMM)** | Enables the supported real Bluetooth Classic provider; does not emulate a remote peer. Requires host hardware, service and permissions. |
| **Camera hardware (CAM1)** | Allows the game to acquire the host camera when requested. Android uses the supported device camera provider. |
| **Device name (Preset ID)** | Readable Gizmondo device identity; encoded for the Gizmondo API rather than PocketPC's ASCII identity. |
| **Console identifier (Platform ID — hexadecimal)** | Gizmondo platform identifier bytes. Separate from PocketPC's identity. |
| **Maximize screen size by allowing controls to overflow it** | Android only. Allows controls to overlap the game to leave a larger image; aspect ratio is preserved. Off by default. |

Hardware switches default on in a fresh configuration. They allow host access;
they cannot guarantee that hardware or permission is present. See the
[Gizmondo service guide](GIZMONDO.md) for provider and platform limits.

## Keyboard and external controller options

Both frontends provide separate **Keyboard bindings** and **External controller
bindings** editors (Desktop toolbar: **Keyboard**, **Controller**). Select the
PocketPC or Gizmondo device profile, inspect assignments, then use **Assign an
input…**. Short and long presses can have different assignments; a long press
uses the shared 0.65-second threshold. Reset restores the selected factory profile.

**Stylus speed (pixels)** ranges from 30 to 1200, default 240. It controls
PocketPC relative stylus movement. The controller editor uses connected host
controllers; pairing and driver configuration remain host OS tasks.

**Save Snapshot** and **Load Snapshot** are assignable controller commands for
both device profiles. Physical **F5–F12** are reserved host shortcuts and cannot
be assigned to a guest command or grid zone. The interface marks reserved keys
in red. See the [input editor](FRONTENDS.md#visual-input-editor--desktop-and-android)
and [host shortcut table](FRONTENDS.md#colors-physical-feedback-and-host-shortcuts)
for the full command list, reset behavior and immutable shortcuts.

## Per-game PocketPC options

### Controls

| Option | Choices and behavior |
| --- | --- |
| **D-pad dead zone** | **Small (sensitive)**, **Medium**, **Large (precise)**; default Large. Controls directional activation from analog input. |
| **Tilt input** | Emulator default, or one of the three global Tilt input modes. Supplies the guest sensor APIs. |
| **Accelerometer mapping** | Slider: **Disabled**, **Low sensitivity**, **Medium sensitivity**, **High sensitivity**. Uses a physical host/device/controller accelerometer as control input. Hold the device in the intended playing position at launch for calibration. The Tilt input control is disabled while this mapping is active. |
| **Accelerometer output** | **Arrows** or **Stick**; available when mapping is enabled. Stick uses relative aiming, without moving the visible stylus or changing grid assignments. |
| **Map Tab/Esc/Turbo to F1/F2/5** | Remaps those three logical commands to phone soft keys F1/F2 and numeric 5. Applies to keyboard, controller and virtual controls, including releases. Existing assignments still work; on-screen labels follow the mapping. |
| **Swap action / P buttons** | Exchanges A/B/C/Action with P1–P4 for the supported control paths. Separate from the Tab/Esc/Turbo mapping. |
| **Control mode** | **Normal**, **Touch steering**, **Touch zone mapping**. Touch steering exposes its test workflow. Touch zone mapping exposes **Configure Grid** and **View Grid**. |

**Configure Grid** launches the game with a grid overlay. Below the image,
choose the grid size and enter/leave editing with **Configure Grid / Resume Game**.
**L1 + R1** cycles sizes, **L2 + R2** toggles editing and **L3 + R3** clears the
selected assignment; these combinations are listened to only in this workflow.
The strong dark veil appears only during editing, when the game receives no
configuration inputs. **Clear grid** clears all assignments. Resume commits
assignments; they persist after closing and reopening the game.

Landscape sizes are **4×3**, **5×4**, **6×5**, **7×6**, **8×7**; portrait uses
the transposed sizes **3×4** through **7×8**. Assign keyboard keys, controller
inputs or Android layout buttons. **View Grid** previews saved assignments
without launching the configurator. This maps to the existing single stylus;
it does not add a multitouch API to a game.

### Graphics

| Option | Choices and behavior |
| --- | --- |
| **Screen** | **Automatic (Pocket PC)**, 240×320, 320×240, 176×220, 240×400, 400×240, 480×640, 640×480, 480×800, 800×480; Desktop also exposes **480×320 (Handheld PC)**. Changes the panel dimensions reported to the guest. Automatic uses executable display metadata where available; detection cannot prove every game's desired resolution. |
| **Rotate display** | 0°, 90° clockwise, 180°, 90° counterclockwise. Rotates presentation and adapts input; does not change the emulated panel dimensions. |
| **Frame rate limit** | Emulator default, 10/15/20/30/45/60/120 fps, Unlimited. Emulator default inherits the global choice; explicit Unlimited overrides it. |
| **GPU compatibility profile** | **Generic OpenGL ES (1.1, Common)**, **NVIDIA GoForce 5500 (OpenGL ES 1.1, Common)**, **Intel 2700G (OpenGL ES 1.0, Common-Lite)**, **Generic OpenGL ES (2.0, Common)**. Emulates guest driver capabilities; does not select the host GPU. |
| **GAPI display compatibility** | **Linear scanlines** or **Rotated LCD columns** (Android: **Columns (early portrait LCD)**). Compatibility with guest framebuffer pitch/orientation expectations; not a general rotation control. |
| **OpenGL ES surface format** | **Auto**, **RGB565 only**, **RGBA8888 only**. GLES 1.x configuration catalogue policy. Auto follows the chosen driver's catalogue/order and the game's EGL request; it is not a random format selection. |

Import or an executable switch can detect GLES2 imports and select its profile.
Ambiguous GLES1 Common applications can prompt for the driver; an imported profile
is still user-selectable. Changing the profile does not translate a GLES1 binary
into a GLES2 application. Information uses the selected launch executable for
detection. See [import workflows](FRONTENDS.md#desktop-focus-and-import-launch-settings-prompt).

### Misc, sound and identity

| Option | Choices and behavior |
| --- | --- |
| **Executable heap** | **Use emulator setting**, **Enabled**, **Disabled**. Under Misc on Desktop and Graphics on Android. |
| **OEM model (readable text)** | Optional reported device model override. |
| **Platform type (readable text)** | Optional reported platform override. |
| **Random seed override (compatibility)** | Empty uses normal behavior; a decimal value 0–4294967295 forces the supplied seed for intercepted `srand` calls. Not a hardware identifier. |
| **Owner name (PocketPC)** | Optional guest owner-name override. |
| **Device name (Preset ID — ASCII)** | Per-game readable identity override. |
| **Preset ID override (hexadecimal)** | Explicit Preset ID bytes; takes precedence over the readable per-game name. |
| **PocketPC ID (Platform ID — hexadecimal)** | Per-game platform identifier bytes. Empty identity overrides inherit the corresponding defaults. |

**Sound** currently states **No per-game sound options yet**. A game's own
Enable sounds prompt is guest UI. **Support libraries**, where shown, is imported
dependency information, not an editable DLL-override setting.

## Per-game Gizmondo options

| Section / option | Choices and behavior |
| --- | --- |
| Controls: **D-pad dead zone** | Small, Medium or Large, as for PocketPC. |
| Controls: **Tilt input** | Physical accelerometer; the PocketPC simulated-tilt selector is not offered. |
| Controls: **Accelerometer → arrows** | Disabled, Low, Medium or High sensitivity. Maps real physical tilt to directional controls; no PocketPC Arrows/Stick output selector. |
| Graphics: **Screen** | Desktop offers native **320×240** or explicit **240×320** portrait. Hidden on Android, which uses the Gizmondo panel. |
| Graphics: **Rotate display** | Same four presentation rotations as PocketPC. |
| Graphics: **Frame rate limit** | Emulator default or the same explicit limits as PocketPC. |

Gizmondo uses the GoForce 4500 driver profile and its RGB565 EGL catalogue.
There is no per-game GPU-profile selector or EGL surface-format override. Global
GPU rendering and GLES internal-resolution choices still apply. Gizmondo has no
PocketPC Control mode, phone-key swap, executable-heap override or per-game
PocketPC identity section. Its hardware and identity defaults live in the global
Gizmondo options. Sound has no per-game options.

## Other launcher and in-game actions

These are actions or live controls, rather than additional per-game settings.

| Action | Availability and purpose |
| --- | --- |
| **Information** | Both; detected game/API/hardware information and the selected launch executable. |
| **Switch Launch Executable** | PocketPC only, below **Add/Replace Data** in the game menu. Lists installed executables, saves the selection and runs the shared launch-advice workflow where necessary. No permanent library rescan is required. |
| **Add/Replace Data** | PocketPC only. **Application Folder** targets `extracted`; **My Documents** targets `saves/my-documents`. ZIP directory structure is retained. My Documents uses overwrite decisions without an executable-selection step. |
| **Rename**, **Remove**, **Game ID** | Library management. Removing a title is distinct from stopping its emulation. |
| **Upscale ×2** | Desktop presentation/window-size action for both device profiles and all graphics paths. Makes room for the enlarged image; separate from GLES render scaling and absent from Android's UI. |
| **Filter**, **Rotation**, **Fullscreen** | Presentation actions. Desktop F9 cycles filters, F7 rotates and F12 toggles fullscreen. Android exposes its display controls and filter setting. |
| **Screenshot**, **Record video** | Desktop F10 and F11. Separate from snapshots and game FPS. |
| **Save Snapshot**, **Restore Snapshot** | Desktop F5/F6; Android **S / L** at the physical window corners. One slot per game, timestamp shown on success; an empty restore does nothing. Errors use the diagnostic dialog. See [snapshot controls](FRONTENDS.md#experimental-snapshots) and [format/coverage](SNAPSHOTS.md). |
| **Volume**, **Brightness**, **Power**, **Geofence** | Gizmondo device controls. Colors can use the startup **Fake GPS** choice, manual coordinates or a randomized location near the Eiffel Tower; language, multiplayer origin and unlock workflow are described in [Colors](GIZMONDO.md#colors). |
| **Backup**, **Restore**, capture library and updates | Launcher maintenance workflows; see [Frontends](FRONTENDS.md). Backup restoration is distinct from loading a game snapshot. |

Import assistants configure the same settings listed here; they are not a
second set of options. A diagnostic dialog offers details, copy and return to
library, plus Continue when the current session can still run. See
[session diagnostics](FRONTENDS.md#session-diagnostics) for error classification.
