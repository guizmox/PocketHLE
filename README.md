<!--
Copyright (C) 2026 PocketHLE Emulator Project
SPDX-License-Identifier: Apache-2.0 OR MIT
-->

# PocketHLE-Reborn

<p align="center">
  <img src="./frontends/pocket-desktop/assets/pockethle_logo.png" width="25%" alt="PocketHLE-Reborn logo with diagonal Reborn lettering" />
</p>

<p align="center">
  A high-level Windows Mobile / Pocket PC / Gizmondo emulator for Linux, Windows and Android.
</p>

<p align="center">
  <a href="https://discord.gg/jDvzctQdFu">
    <img src="https://img.shields.io/badge/Discord-Join%20our%20Community-5865F2?style=for-the-badge&logo=discord&logoColor=white" alt="Join the PocketHLE-Reborn Discord community" />
  </a>
</p>

<p align="center">
  <strong>Join our Discord for development updates, compatibility discussions, support and community chat.</strong>
</p>

PocketHLE-Reborn is an actively developed fork of the original PocketHLE
project. We chose the name together with PocketHLE's original author,
j92580498-max, to make the two projects easy to distinguish.
[Original PocketHLE project](https://github.com/j92580498-max/PocketHLE).

| If you want to… | Start here |
| --- | --- |
| Install and import a game | [Using PocketHLE-Reborn](README-DETAILED.md#using) |
| Set up a Steam Deck | [Steam Deck guide](docs/STEAMDECK.md) |
| Configure controls or run a .NET game | [Frontend guide](docs/FRONTENDS.md) |
| Build or contribute | [Build and release](docs/BUILD.md) · [Architecture](docs/AGENTS.md) |
| Read the detailed project guide | [Compatibility evidence, status, architecture and roadmap](README-DETAILED.md) |
| Find another guide | [Documentation index](docs/README.md) |

---

> [!NOTE]
> PocketHLE-Reborn runs native and selected .NET Compact Framework Windows CE software for Pocket PC, Windows Mobile and Gizmondo. It supports Pocket PC CAB imports and user-provided Gizmondo game and shell profiles. It is experimental software: compatibility, performance and input behavior vary by game and host platform.

> [!WARNING]
> PocketHLE-Reborn is developed for research and educational purposes. It does not include Microsoft system files, firmware or game data. Use only legally obtained game copies and archives.

## Info

PocketHLE-Reborn is a high-level emulator for Pocket PC 2002/2003, Windows Mobile 5/6 and Gizmondo software. Instead of emulating a complete Windows CE device, it loads the original game executable, runs its ARM code through a CPU backend, and provides clean-room host-side implementations of the Windows CE APIs the game expects.

The project is inspired by [touchHLE](https://github.com/touchHLE/touchHLE) and [EKA2L1](https://github.com/EKA2L1/EKA2L1), with a launcher and library workflow influenced by [j2me-loader](https://github.com/nikita36078/j2me-loader).

## Games Tested

See the [Gizmondo compatibility table](docs/GIZMONDO.md#reported-game-compatibility) for reported game results and known issues. The [GTShell guide](docs/GIZMONDO.md#gtshell-profiles) covers shell profile setup and Bluetooth file transfer.

| Asphalt 4 Elite Racing | Call of Duty 2 |
| :---: | :---: |
| ![Asphalt 4 gameplay](proof/games/asphalt-4-elite-racing.jpg) | ![Call of Duty 2](proof/games/call-of-duty-2.jpg) |

| Zuma Deluxe | Bejeweled |
| :---: | :---: |
| ![Zuma Deluxe](proof/games/zuma-deluxe.jpg) | ![Bejeweled](proof/games/bejeweled.jpg) |


## License

PocketHLE-Reborn is dual-licensed under [Apache-2.0](LICENSE-APACHE) **OR** [MIT](LICENSE-MIT), at your option.

## Contributing

Bug reports, compatibility results, API traces and pull requests are welcome. Please include the game version, target screen size, frontend, CPU backend, command line and relevant logs or frame captures when reporting a problem.

Join the [PocketHLE-Reborn Discord community](https://discord.gg/jDvzctQdFu) for development updates, compatibility discussions and support.

## Special Thanks

* [touchHLE](https://github.com/touchHLE/touchHLE) for the high-level emulation model and project inspiration;
* [EKA2L1](https://github.com/EKA2L1/EKA2L1) for another practical HLE-oriented emulator architecture;
* [j2me-loader](https://github.com/nikita36078/j2me-loader) for launcher and library UX inspiration;
* [Katamaran21](https://github.com/Katamaran21/PocketHLE-GTShell) for GTShell support;
* Nekomo, OLego, TimofeyLednev, Benoît (FX-man), Jérôme (SoldatGG), Tim (Firefly), Marat, and torrenia;
* the PocketHLE-Reborn contributors and testers who provide legally obtained software, traces and compatibility reports.
