#!/usr/bin/env python3
"""Rescue an installed PocketHLE library before uninstalling a mismatched APK.

Requires Python 3.9+ and adb on PATH. Never uninstalls the app or changes saves.
"""
import argparse
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import sys
import subprocess
import tarfile
import tempfile
import time
import zipfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", nargs="?", default=f"PocketHLE-backup-{int(time.time())}.zip")
    parser.add_argument("--serial", help="ADB device serial (required when several devices are connected)")
    args = parser.parse_args()
    print("""PocketHLE - Android library rescue backup
----------------------------------------
This saves your games, settings and saves to a ZIP on this computer.
It NEVER uninstalls PocketHLE or modifies its library.

Before starting:
  - Install Python 3.9+ and Android Platform Tools (adb on PATH).
  - Enable USB debugging on the Android device and connect it by USB.
  - Unlock it and accept the USB debugging authorization prompt.
  - Close any running game/import and leave enough disk space for TWO copies
    of your library (temporary files plus the finished ZIP).
  - With several devices connected, use --serial DEVICE_SERIAL.


ADB download (official, Windows/Linux):
  https://developer.android.com/tools/releases/platform-tools
Extract the download, then put the platform-tools folder beside this script,
or add its full path to PATH. Android Studio is NOT required.

To enable ADB / USB debugging on the phone or handheld:
  1. Open Settings > About phone (or About device).
  2. Tap Build number seven times; enter your PIN if asked.
     Samsung: About phone > Software information > Build number.
  3. Open Settings > System > Developer options (location varies by device).
  4. Enable USB debugging. Bootloader unlocking / OEM unlocking is NOT needed.
  5. Connect a USB data cable, unlock the device, and accept Allow USB debugging.
  6. If ADB says unauthorized, check that authorization prompt on the device.
Official guide: https://developer.android.com/studio/debug/dev-options

Do NOT uninstall PocketHLE until this script reports \"Verified backup\".
""", flush=True)
    script_dir = Path(__file__).resolve().parent
    os.environ["PATH"] = os.pathsep.join([str(script_dir / "platform-tools"), str(script_dir), os.environ.get("PATH", "")])
    if shutil.which("adb") is None:
        parser.error("adb was not found. Install Android Platform Tools and add adb to PATH.")
    output = Path(args.output).resolve()
    if output.exists():
        parser.error("Output already exists; choose another ZIP name")
    adb = ["adb"] + (["-s", args.serial] if args.serial else [])
    print("[1/4] Checking the ADB connection...", flush=True)
    subprocess.run(adb + ["get-state"], check=True)
    print("[2/4] Stopping PocketHLE and copying its library (large libraries may take a while)...", flush=True)
    subprocess.run(adb + ["shell", "am", "force-stop", "com.pockethle.app"], check=True)
    with tempfile.TemporaryDirectory(prefix="pockethle-rescue-", dir=output.parent) as scratch:
        scratch = Path(scratch)
        library = scratch / "library"
        external = "/sdcard/Android/data/com.pockethle.app/files/library"
        pulled = subprocess.run(adb + ["pull", external, str(library)], check=False)
        if pulled.returncode or not (library / "library.json").is_file():
            print("Direct access unavailable. Trying the installed debug app (run-as)...", flush=True)
            # run-as works only if the installed APK is debuggable. Some Android
            # versions/OEMs also prevent shell access to app external storage.
            shutil.rmtree(library, ignore_errors=True)
            for source in [external, "/data/user/0/com.pockethle.app/files/library"]:
                tar_path = scratch / "rescue.tar"
                with tar_path.open("wb") as stream:
                    result = subprocess.run(adb + ["exec-out", "run-as", "com.pockethle.app", "tar", "-C", source, "-cf", "-", "."], stdout=stream, check=False)
                if result.returncode:
                    continue
                try:
                    library.mkdir(exist_ok=True)
                    with tarfile.open(tar_path, "r:") as tar:
                        for member in tar:
                            relative = PurePosixPath(member.name)
                            if relative.is_absolute() or ".." in relative.parts or "\\" in member.name or ":" in member.name:
                                raise ValueError("Unsafe archive path")
                            target = library.joinpath(*relative.parts)
                            if member.isdir():
                                target.mkdir(parents=True, exist_ok=True)
                            elif member.isfile():
                                target.parent.mkdir(parents=True, exist_ok=True)
                                with tar.extractfile(member) as src, target.open("xb") as dst:
                                    shutil.copyfileobj(src, dst)
                            else:
                                raise ValueError("Links and special files are unsupported")
                    if (library / "library.json").is_file():
                        break
                except (tarfile.TarError, OSError, ValueError) as error:
                    print(f"Could not read {source}: {error}")
                shutil.rmtree(library, ignore_errors=True)
        catalog = library / "library.json"
        if not catalog.is_file():
            raise SystemExit("Backup failed: Android refused access. KEEP THE APP INSTALLED. No usable ZIP was created.")
        value = json.loads(catalog.read_text(encoding="utf-8"))
        if not isinstance(value, dict) or not isinstance(value.get("games"), list):
            raise SystemExit("Invalid library.json. KEEP THE APP INSTALLED.")
        print("[3/4] Creating the restore ZIP...", flush=True)
        staged = scratch / "backup.zip"
        with zipfile.ZipFile(staged, "w", zipfile.ZIP_DEFLATED, allowZip64=True) as archive:
            archive.writestr("__pockethle_backup__.json", '{"format":"PocketHLE","version":1}')
            for path in sorted(library.rglob("*")):
                if path.is_symlink():
                    raise SystemExit("Backup contains a symbolic link; aborting")
                archive.write(path, path.relative_to(library).as_posix())
        print("[4/4] Verifying every ZIP entry...", flush=True)
        with zipfile.ZipFile(staged) as archive:
            bad = archive.testzip()
            if bad:
                raise SystemExit(f"ZIP validation failed: {bad}")
        # Exclusive creation also protects against another process creating this
        # filename while ADB was copying. Remove only our own partial output.
        with output.open("xb") as dst:
            try:
                with staged.open("rb") as src:
                    shutil.copyfileobj(src, dst)
                dst.flush()
                os.fsync(dst.fileno())
            except BaseException:
                output.unlink(missing_ok=True)
                raise
    print(f"Verified backup: {output} ({output.stat().st_size} bytes)")
    print("Keep a separate copy of this ZIP before uninstalling anything.")
    print("After installing the new APK, use PocketHLE: Restore library backup (ZIP).")


def wait_before_close():
    if not sys.stdin.isatty():
        return
    print("\nPress any key to close this window...", end="", flush=True)
    try:
        if os.name == "nt":
            import msvcrt
            msvcrt.getwch()
        else:
            import termios
            import tty
            fd = sys.stdin.fileno()
            settings = termios.tcgetattr(fd)
            try:
                tty.setraw(fd)
                sys.stdin.read(1)
            finally:
                termios.tcsetattr(fd, termios.TCSADRAIN, settings)
    except (OSError, ImportError):
        input()
    print()


if __name__ == "__main__":
    try:
        main()
    except (OSError, subprocess.CalledProcessError, ValueError, zipfile.BadZipFile) as error:
        print(f"Backup failed: {error}. KEEP THE APP INSTALLED.", file=sys.stderr, flush=True)
        raise SystemExit(1)
    except SystemExit as error:
        if isinstance(error.code, str):
            print(error.code, file=sys.stderr, flush=True)
            raise SystemExit(1)
        raise
    finally:
        wait_before_close()
