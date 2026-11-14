#!/usr/bin/env python3
"""Package corresponding source, locked Cargo dependencies and native library source."""
import argparse
import json
from pathlib import Path
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    vendor = ROOT / 'vendor-cargo'
    config = ROOT / 'target/vendor-config.toml'
    if not vendor.is_dir() or not config.is_file():
        raise SystemExit('Run cargo vendor --locked vendor-cargo > target/vendor-config.toml first')
    files = subprocess.check_output(['git', 'ls-files', '-z'], cwd=ROOT).decode().split('\0')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    prefix = 'PocketHLE-Reborn-source/'
    # Cargo archives deliberately use epoch timestamps for reproducibility.
    # ZIP's DOS date range begins in 1980; clamp dates without changing sources.
    with zipfile.ZipFile(args.output, 'w', zipfile.ZIP_DEFLATED, compresslevel=6,
                         strict_timestamps=False) as archive:
        for name in sorted(filter(None, files)):
            path = ROOT / name
            if not path.is_file() or name == '.cargo/config.toml':
                continue
            archive.write(path, prefix + name)
        for path in sorted(vendor.rglob('*')):
            if path.is_file():
                archive.write(path, prefix + path.relative_to(ROOT).as_posix())
        original = (ROOT / '.cargo/config.toml').read_text()
        archive.writestr(prefix + '.cargo/config.toml', original + '\n' + config.read_text())
        archive.writestr(prefix + 'REBUILD-AND-RELINK.txt',
            'Corresponding source for the PocketHLE-Reborn binary distribution.\n'
            'Contains the exact application source, Cargo.lock, all locked Cargo dependencies,\n'
            'FFmpeg 8.0.1 source and OpenH264 2.6.0 source, plus build/packaging scripts.\n\n'
            'xBRZ-containing binary combinations are distributed under GPL-3.0 terms.\n'
            'Individual source components keep their original licenses.\n'
            'FFmpeg uses LGPL 2.1+ configuration; no GPL/nonfree codec is enabled.\n\n'
            'Windows: initialize MSVC x64 and LLVM, install MSYS2 make/diffutils/pkgconf,\n'
            'run tools/build-ffmpeg.ps1, then cargo build --release --locked --offline\n'
            '-p pocket-cli -p pocket-desktop --features pocket-cli/unicorn.\n'
            'Android: use the pinned JDK17/SDK35/NDKr28c in docs/BUILD.md and\n'
            'run tools/build-android-native.sh followed by tools/build-android-apk.sh.\n'
            'Native source archives live under third-party; modify their extracted source\n'
            'under target/native/ffmpeg-source or target/native/openh264-source and rebuild\n'
            'with the supplied scripts to relink the application with your modified libraries.\n'
            'For Android, sign rebuilt APKs with your own key; the publisher key is private.\n')
        archive.writestr(prefix + 'SOURCE-REVISION.json', json.dumps(dict(
            commit=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT).decode().strip()), indent=2))
    with zipfile.ZipFile(args.output) as archive:
        bad = archive.testzip()
        if bad:
            raise SystemExit(f'Source ZIP integrity check failed: {bad}')
    print(args.output)


if __name__ == '__main__':
    main()
