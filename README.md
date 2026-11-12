<!--
Copyright (C) 2026 PocketHLE Emulator Project
SPDX-License-Identifier: Apache-2.0 OR MIT
-->

# PocketHLE

<p align="center">
  <img src="./frontends/pocket-desktop/assets/pockethle_logo.png" width="25%" alt="PocketHLE logo" />
</p>

<p align="center">
  A high-level Windows Mobile / Pocket PC / Gizmondo emulator for Linux, Windows and Android.
</p>

<p align="center">
  <a href="https://discord.gg/jDvzctQdFu">
    <img src="https://img.shields.io/badge/Discord-Join%20our%20Community-5865F2?style=for-the-badge&logo=discord&logoColor=white" alt="Join the PocketHLE Discord community" />
  </a>
</p>

<p align="center">
  <strong>Join our Discord for development updates, compatibility discussions, support and community chat.</strong>
</p>

Documentation is maintained in English. See the [documentation index](docs/README.md),
[GPU guide](docs/GPU.md) and [Gizmondo guide](docs/GIZMONDO.md).

---

> [!NOTE]
> PocketHLE runs native Windows CE software for Pocket PC, Windows Mobile and Gizmondo. It supports Pocket PC CAB imports and user-provided Gizmondo game and shell profiles. It is experimental software: compatibility, performance and input behavior vary by game and host platform.

> [!WARNING]
> PocketHLE is developed for research and educational purposes. It does not include Microsoft system files, firmware or game data. Use only legally obtained game copies and archives.

## Info

PocketHLE is a high-level emulator for Pocket PC 2002/2003, Windows Mobile 5/6 and Gizmondo software. Instead of emulating a complete Windows CE device, it loads the original game executable, runs its ARM code through a CPU backend, and provides clean-room host-side implementations of the Windows CE APIs the game expects.

The project is inspired by [touchHLE](https://github.com/touchHLE/touchHLE) and [EKA2L1](https://github.com/EKA2L1/EKA2L1), with a launcher and library workflow influenced by [j2me-loader](https://github.com/nikita36078/j2me-loader).

## Games Tested

See the [Gizmondo compatibility table](docs/GIZMONDO.md#reported-game-compatibility) for reported game results and known issues. The [GTShell guide](docs/GIZMONDO.md#gtshell-profiles) covers shell profile setup and Bluetooth file transfer.

| Asphalt 4 Elite Racing | Call of Duty 2 |
| :---: | :---: |
| ![Asphalt 4 gameplay](proof/games/asphalt-4-elite-racing.jpg) | ![Call of Duty 2](proof/games/call-of-duty-2.jpg) |

| Zuma Deluxe | Bejeweled |
| :---: | :---: |
| ![Zuma Deluxe](proof/games/zuma-deluxe.jpg) | ![Bejeweled](proof/games/bejeweled.jpg) |

> [!IMPORTANT]
> The screenshots above are representative game references supplied for the project presentation. The checked-in, reproducible emulator proof currently covers Asphalt 4 WVGA and Call of Duty 2's title-screen boot through the OpenGL ES layer. Zuma Deluxe and Bejeweled have game-specific compatibility work in the codebase, but should not be described as fully verified here without a corresponding checked-in run capture.

Additional compatibility probes and rendering captures are available under [`proof/`](proof), including Asphalt 2 3D, Crazy Taxi, Diamond Twister, Splinter Cell Conviction and other Windows Mobile titles.

The [maintainer's compatibility and test library](docs/COMPATIBILITY.md) lists
**135 entries: 78 PocketPC / Windows Mobile and 57 Gizmondo**, updated from the
supplied catalogue on 2026-11-10. This includes games, demos, benchmarks and
utilities; catalogue membership is not a per-title pass/fail or completion report.

## Status

PocketHLE can currently:

* import and unpack Windows Mobile `.CAB` archives;
* restore long filenames and installation paths from `_setup.xml`, or
  from the binary `MSCE` install header older cabinets use instead;
* select the intended game executable instead of helper binaries;
* load native ARM and MIPS PE images and inspect their imports;
* execute ARM code with the Unicorn Engine backend;
* intercept imported Windows CE DLL calls through HLE thunks;
* provide ordinal-to-name resolution for `coredll.dll` and `aygshell.dll`;
* emulate core windowing, GDI, GAPI, registry, filesystem, timing, input and audio paths;
* present GDI and GAPI software framebuffers, use host OpenGL by default for GLES 1.x with a PocketHLE software fallback, and use host OpenGL for GLES 2.0;
* keep supported OpenGL ES frames on the GPU from rendering through desktop presentation, avoiding CPU pixel readback and re-upload;
* run a shared game library with per-game settings on desktop and Android;
* build Linux, Windows and Android artifacts through GitHub Actions.

The desktop frontend offers fullscreen (F12), screenshots (F10), video
recording (F11), and separate VSync and game FPS settings. Desktop and Android
offer global GLES 1.x rendering options. The host OpenGL path supports Native,
2×, 3× and 4× internal resolution, with a checkbox to include surfaces other
than 320×240 and 240×320. The Android frontend offers per-game settings,
orientation controls and on-screen controls. See the [frontend guide](docs/FRONTENDS.md) and
[GPU guide](docs/GPU.md) for platform-specific behavior.

Gizmondo profiles can use host Bluetooth Classic for supported RFCOMM workflows.
GTShell image transfer from Windows to Android is confirmed.

GPU-only presentation applies to compatible GLES frames. GDI composition,
software rendering, capture readback and unsupported sharing configurations
can use CPU pixels. The guest CPU still executes the game logic.

PocketHLE is not a full Windows CE emulator. Some games still stop during CRT initialization, dynamic imports, worker-thread setup or unimplemented APIs. A successful boot or first frame does not automatically mean that an entire game is playable from start to finish.

## Architecture

```
+----------------+    +----------------+    +----------------+
| pocket-cli     |    | pocket-desktop |    | pocket-android |
| CLI / captures |    | egui launcher  |    | Kotlin + JNI   |
+--------+-------+    +--------+-------+    +--------+-------+
         \                     |                     /
          \                    |                    /
           +-------------------+--------------------+
                               |
                      +--------v--------+
                      |   pocket-core   |
                      | Emulator / VFS  |
                      +--------+--------+
                               |
          +--------------------+--------------------+
          |                    |                    |
  +-------v-------+    +-------v--------+   +-------v--------+
  | pocket-cab    |    | pocket-pe      |   | pocket-library |
  | CAB installer |    | ARM/MIPS PE    |   | game database  |
  +---------------+    +----------------+   +----------------+
                               |
                      +--------v--------+
                      | pocket-kernel   |
                      | process / GDI   |
                      | HLE dispatch    |
                      +--------+--------+
                               |
                  +------------+------------+
                  |                         |
          +-------v-------+         +-------v--------+
          | pocket-cpu    |         | pocket-winceapi |
          | stub/Unicorn  |         | coredll/gx/hss  |
          +---------------+         +----------------+
```

When a game starts:

1. `pocket-cab` extracts the archive and reconstructs its installed files and registry values.
2. `pocket-pe` parses the PE image, sections and imported symbols.
3. `pocket-kernel` maps the image, patches the import address table with HLE thunks and creates the guest process state.
4. `pocket-cpu` executes the guest ARM or MIPS code.
5. `pocket-winceapi` dispatches imported Windows CE calls to Rust handlers for graphics, files, windows, input, registry, timing and audio.
6. The selected frontend displays the framebuffer and forwards host input back to the guest.

## Using

Download a release archive for your platform, import a legally obtained Pocket PC
CAB or supported Windows installer, and choose the game in the desktop or Android
launcher. PocketHLE extracts the Windows CE payload without running the Windows
setup program.

The Pocket PC importer supports these installer and archive families:

* standalone Windows CE CABs and supported embedded CABs in Windows executables;
* ZIP and ZIP self-extracting archives, plus supported RAR 3 self-extracting archives;
* InstallShield 5 and 6–13 cabinet sets, including supported InstallShield 5
  CabWiz INF layouts without a prebuilt CE CAB;
* NSIS installers, including supported solid and legacy bzip2 variants;
* legacy Inno Setup 2.0.11 installers with internal zlib payloads;
* supported Wise sequential DEFLATE, VISE ESIV and PocketSetup PS2 installers.

Support depends on the actual installer variant and its payload. Keep matching
header and volume files together in the archive when the installer uses a set.

For command-line runs:

```bash
# Inspect a cabinet
pockethle inspect-cab ~/Games/Asphalt4.cab

# Run a cabinet; the launcher extracts and mounts its install directory
pockethle run ~/Games/Asphalt4.cab

# Run with the real ARM CPU backend and capture frames
pockethle run ~/Games/Asphalt4.cab \
  --cpu unicorn \
  --dump-frames-to /tmp/pockethle-frames \
  --max-frames 8

# Inspect or run an extracted executable
pockethle pe-info /tmp/pockethle/Asphalt4.exe
pockethle run /tmp/pockethle/Asphalt4.exe --cpu unicorn
```

The CLI also supports `.exe` and `.zip` inputs. Use `--cpu stub` for trace-only and loader tests; it does not execute real game instructions.


### Steam Deck (Proton)

Use the **Windows archive** (`PocketHLE-windows-x86_64.zip`) on Steam Deck.
Add `pockethle-gui.exe` as a non-Steam game, choose **Proton** or
**Proton Experimental** in the shortcut's Compatibility settings, and add this
to its **Launch Options**:

```text
--steam-deck
```

The Deck uses the same executable and update package as Windows; there is no
separate Steam Deck download. The argument selects the Deck controller/settings
profile. For updates, keep the existing Steam shortcut and back up your library
before replacing program files: its data may live inside the Proton prefix.
See the [Steam Deck guide](docs/STEAMDECK.md) for details.

## Build

Use the [build, signing and release guide](docs/BUILD.md) for pinned toolchains,
Linux/Windows prerequisites, Android/WSL2, static media, tests and public signing.
For a prepared source checkout with its dependencies installed:

```sh
cargo build --release --locked -p pocket-desktop
```

The [Steam Deck guide](docs/STEAMDECK.md) covers installation through Proton;
Deck uses the Windows package rather than a separate build. The
[documentation index](docs/README.md) routes settings, controls, graphics,
compatibility and diagnostics to their current owners.

## Library

Desktop and Android share the `pocket-library` model:

```
<library-root>/
├── library.json
├── config.json
└── games/
    └── <sanitized-id>/
        ├── game.json
        ├── source.cab
        └── extracted/
            └── ... game files ...
```

On Linux and Windows, the default location is `~/.local/share/PocketHLE/library` or the platform equivalent. Android stores it under the app's external files directory.

## Roadmap

* complete the remaining CRT and legacy ARM startup paths;
* expand dynamic import and thread/runtime compatibility;
* improve resource, GDI and dialog coverage across more games;
* add broader gamepad, keyboard and touch mapping;
* continue improving audio mixing and host output;
* add reproducible compatibility captures for more titles;
* improve desktop and Android UX and diagnostics.

## Legal notice

PocketHLE does not contain or distribute copyrighted Microsoft system DLLs, firmware or game assets. The Windows CE API layer is a clean-room reimplementation based on public API behavior and ordinal data. Users are responsible for the games and archives they provide.

## License

PocketHLE is dual-licensed under [Apache-2.0](LICENSE-APACHE) **OR** [MIT](LICENSE-MIT), at your option.

## Contributing

Bug reports, compatibility results, API traces and pull requests are welcome. Please include the game version, target screen size, frontend, CPU backend, command line and relevant logs or frame captures when reporting a problem.

Join the [PocketHLE Discord community](https://discord.gg/jDvzctQdFu) for development updates, compatibility discussions and support.

## Special Thanks

* [touchHLE](https://github.com/touchHLE/touchHLE) for the high-level emulation model and project inspiration;
* [EKA2L1](https://github.com/EKA2L1/EKA2L1) for another practical HLE-oriented emulator architecture;
* [j2me-loader](https://github.com/nikita36078/j2me-loader) for launcher and library UX inspiration;
* [Katamaran21](https://github.com/Katamaran21/PocketHLE-GTShell) for GTShell support;
* Nekomo, OLego, TimofeyLednev, Benoît (FX-man), Jérôme (SoldatGG), Tim (Firefly), Marat, and torrenia;
* the PocketHLE contributors and testers who provide legally obtained software, traces and compatibility reports.
