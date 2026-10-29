#!/usr/bin/env python3
"""Rescue an installed PocketHLE library before uninstalling a mismatched APK.

Requires Python 3.9+ and adb on PATH. Never uninstalls the app or changes saves.
"""
import argparse
import json
import os
from pathlib import Path, PurePosixPath
import shutil
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
    output = Path(args.output).resolve()
    if output.exists():
        parser.error("Output already exists; choose another ZIP name")
    adb = ["adb"] + (["-s", args.serial] if args.serial else [])
    subprocess.run(adb + ["get-state"], check=True)
    subprocess.run(adb + ["shell", "am", "force-stop", "com.pockethle.app"], check=True)
    with tempfile.TemporaryDirectory(prefix="pockethle-rescue-", dir=output.parent) as scratch:
        scratch = Path(scratch)
        library = scratch / "library"
        external = "/sdcard/Android/data/com.pockethle.app/files/library"
        pulled = subprocess.run(adb + ["pull", external, str(library)], check=False)
        if pulled.returncode or not (library / "library.json").is_file():
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
        staged = scratch / "backup.zip"
        with zipfile.ZipFile(staged, "w", zipfile.ZIP_DEFLATED, allowZip64=True) as archive:
            archive.writestr("__pockethle_backup__.json", '{"format":"PocketHLE","version":1}')
            for path in sorted(library.rglob("*")):
                if path.is_symlink():
                    raise SystemExit("Backup contains a symbolic link; aborting")
                archive.write(path, path.relative_to(library).as_posix())
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
    print("Keep a separate copy. Restore it in PocketHLE 0.3.9 using Restore library backup (ZIP).")


if __name__ == "__main__":
    main()
