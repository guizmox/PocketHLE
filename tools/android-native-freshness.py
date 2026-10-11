#!/usr/bin/env python3
"""Reject APK builds that would reuse native libraries from another source tree."""
import hashlib
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent.parent
STAMP = ROOT / 'target/android-native-build.json'


def fingerprint():
    digest = hashlib.sha256()
    paths = [ROOT / 'Cargo.toml', ROOT / 'Cargo.lock']
    for folder in ('crates', 'frontends/pocket-android-jni', 'build-support'):
        paths.extend(p for p in (ROOT / folder).rglob('*')
                     if p.is_file() and p.suffix in ('.rs', '.toml', '.c', '.h', '.cmake'))
    paths.extend((ROOT / 'tools').glob('build-*.sh'))
    for path in sorted(paths):
        digest.update(path.relative_to(ROOT).as_posix().encode() + b'\0')
        digest.update(path.read_bytes())
    return digest.hexdigest()


def libraries():
    base = ROOT / 'frontends/pocket-android/app/src/main/jniLibs'
    return {abi: hashlib.sha256((base / abi / 'libpockethle_jni.so').read_bytes()).hexdigest()
            for abi in ('arm64-v8a', 'armeabi-v7a')}


def main():
    mode = sys.argv[1]
    if mode == 'invalidate':
        STAMP.unlink(missing_ok=True)
        return
    actual = {'sources': fingerprint(), 'libraries': libraries()}
    if mode == 'record':
        STAMP.parent.mkdir(parents=True, exist_ok=True)
        STAMP.write_text(json.dumps(actual))
    elif mode == 'check':
        if not STAMP.exists() or json.loads(STAMP.read_text()) != actual:
            raise SystemExit('Native libraries are missing or stale. Run bash tools/build-android-native.sh before building the APK.')
    else:
        raise SystemExit('Expected invalidate, record or check')


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError) as error:
        raise SystemExit(f'Cannot verify native libraries: {error}. Rebuild the native libraries.')
