#!/usr/bin/env python3
"""Create a portable PocketHLE backup from user-supplied GTShell/card files."""
import argparse
import hashlib
import json
import os
import struct
import tempfile
import zipfile
from pathlib import Path

TESTED_SHELL_SHA256 = "8e8766071b38aaa0d304fb6105e137f657249e3626b0c57275cf014cfdf01367"


def reject_link(path):
    # Python 3.9 does not have Path.is_junction; lstat still exposes Windows
    # reparse points, so a junction must not disappear behind resolve().
    attributes = getattr(path.lstat(), "st_file_attributes", 0)
    if path.is_symlink() or attributes & 0x400:
        raise ValueError(f"Use physical folders and files, not links: {path}")


def physical_directory(path):
    reject_link(path)
    root = path.resolve(strict=True)
    if not root.is_dir():
        raise ValueError(f"Not a folder: {root}")
    return root


def entries_in(root):
    root = physical_directory(root)
    for path in sorted(root.rglob("*")):
        reject_link(path)
        relative = path.relative_to(root)
        if any(any(character in part for character in "\\:\0") for part in relative.parts):
            raise ValueError(f"Non-portable filename: {relative}")
        if not path.resolve(strict=True).is_relative_to(root):
            raise ValueError(f"File leaves its folder: {path}")
        if path.is_file() or path.is_dir():
            yield path, relative.as_posix(), path.is_dir()


def choose_shell(flash2, explicit):
    if explicit is not None:
        reject_link(explicit)
        shell = explicit.resolve(strict=True)
    else:
        candidates = [path for path in flash2.iterdir()
                      if path.is_file() and path.name.casefold() in ("gtshell.exe", "gtshell0.exe")]
        known = [path for path in candidates
                 if hashlib.sha256(path.read_bytes()).hexdigest() == TESTED_SHELL_SHA256]
        if not known:
            raise ValueError("The tested GTShell.exe/GTShell0.exe was not found. Select your executable with --gtshell.")
        shell = known[0]
    data = shell.read_bytes()
    if len(data) < 64 or data[:2] != b"MZ":
        raise ValueError(f"Not a Windows CE executable: {shell}")
    pe = struct.unpack_from("<I", data, 60)[0]
    if pe + 24 > len(data) or data[pe:pe + 4] != b"PE\0\0":
        raise ValueError(f"Invalid PE executable: {shell}")
    machine, _, _, _, _, _, characteristics = struct.unpack_from("<HHIIIHH", data, pe + 4)
    if machine not in (0x1C0, 0x1C2, 0x1C4) or characteristics & 0x2000:
        raise ValueError(f"GTShell must be an ARM executable: {shell}")
    return shell


def build_profile(args):
    flash = physical_directory(args.flash_disk)
    flash2 = physical_directory(args.flash_disk2)
    shell = choose_shell(flash2, args.gtshell)
    if not any(path.is_file() and path.name.casefold() == "mfcce400.dll" for path in flash2.iterdir()):
        raise ValueError("flash disk2 must include the supplied mfcce400.dll and all shell assets.")
    card = physical_directory(args.sd_card) if args.sd_card else None
    output = args.output.resolve()
    for root in [flash, flash2] + ([card] if card else []):
        if output.is_relative_to(root):
            raise ValueError("Save the profile ZIP outside its input folders.")

    # Paths stay relative to the restored library on both Windows and Android.
    game = {
        "platform": "gizmondo", "id": "gtshell", "display_name": "GTShell + SD Card" if card else "GTShell",
        "provider": "Gizmondo", "executable": "extracted/GTShell.exe", "source_cab": "manual:GTShell.exe",
        "install_dir": "\\Flash Disk2\\", "install_dirs": ["\\Flash Disk2\\"], "save_prefix": "\\Flash Disk2\\",
        "registry": [], "imported_at": 0, "icon": None, "companions": ["extracted/mfcce400.dll"],
        "settings": {
            "cpu_backend": "unicorn", "max_slices": 0, "instructions_per_slice": 50000,
            "screen": "landscape", "rotation": "none", "gpu_renderer": False,
            "frame_rate_limit": "unlimited", "storage_card_dir": "cards/sd-card",
        },
    }
    config = {
        "schema_version": 1, "default_cpu_backend": "unicorn", "verbosity": 1,
        "log_unimplemented_apis": True, "gpu_renderer": False, "fullscreen": False,
        "upscale_filter": "nearest",
        "gps_enabled": False, "gprs_enabled": False, "camera_enabled": False, "bluetooth_enabled": False,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = tempfile.NamedTemporaryFile(prefix=".gtshell-profile-", suffix=".zip", dir=output.parent, delete=False)
    temporary_path = Path(temporary.name)
    temporary.close()
    seen = set()

    def add(archive, name, path=None, data=None, directory=False):
        key = name.casefold().rstrip("/")
        if key in seen:
            raise ValueError(f"Duplicate portable path: {name}")
        seen.add(key)
        if path is not None:
            archive.write(path, name)
        else:
            info = zipfile.ZipInfo(name + "/" if directory else name)
            info.create_system = 3
            info.external_attr = ((0o40700 if directory else 0o100600) << 16) | (0x10 if directory else 0)
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, b"" if directory else data)

    try:
        with zipfile.ZipFile(temporary_path, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
            add(archive, "__pockethle_backup__.json", data=b'{"format":"PocketHLE","version":1}')
            add(archive, "library.json", data=json.dumps({"schema_version": 1, "games": [game]}, indent=2).encode())
            add(archive, "games/gtshell/game.json", data=json.dumps(game, indent=2).encode())
            add(archive, "config.json", data=json.dumps(config, indent=2).encode())
            for directory in ["flash", "games/gtshell/extracted", "games/gtshell/saves", "cards/sd-card"]:
                add(archive, directory, directory=True)
            for path, relative, directory in entries_in(flash):
                add(archive, "flash/" + relative, path=None if directory else path, directory=directory)
            for path, relative, directory in entries_in(flash2):
                if relative.casefold() not in ("gtshell.exe", "gtshell0.exe"):
                    add(archive, "games/gtshell/extracted/" + relative, path=None if directory else path, directory=directory)
            add(archive, "games/gtshell/extracted/GTShell.exe", path=shell)
            if card:
                for path, relative, directory in entries_in(card):
                    add(archive, "cards/sd-card/" + relative, path=None if directory else path, directory=directory)
        os.replace(temporary_path, output)
    finally:
        temporary_path.unlink(missing_ok=True)
    print(f"Profile: {output}")
    print(f"GTShell: {shell}")
    print(f"GTShell SHA256: {hashlib.sha256(shell.read_bytes()).hexdigest()}")
    print("Restore with the library's Restore backup menu on PC or Android.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--flash-disk", type=Path, required=True, help="Complete Flash Disk folder")
    parser.add_argument("--flash-disk2", type=Path, required=True, help="Complete Flash Disk2 folder")
    parser.add_argument("--gtshell", type=Path, help="Explicit GTShell executable; default detects the tested build")
    parser.add_argument("--sd-card", type=Path, help="Optional full SD Card root containing autorun.exe")
    parser.add_argument("--output", type=Path, required=True, help="Portable PocketHLE backup ZIP")
    args = parser.parse_args()
    try:
        build_profile(args)
    except (OSError, ValueError, zipfile.BadZipFile) as error:
        parser.exit(1, f"Profile creation failed: {error}\n")


if __name__ == "__main__":
    main()
