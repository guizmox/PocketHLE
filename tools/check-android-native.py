#!/usr/bin/env python3
"""Reject invalid packaged bridges (ELF ABI, 16 KiB segments, unresolved JIT and FFmpeg DLLs)."""
import os, platform, re, subprocess
from pathlib import Path
root = Path(__file__).resolve().parents[1]
ndk = Path(os.environ['ANDROID_NDK_HOME'])
host = {'Linux':'linux-x86_64','Darwin':'darwin-x86_64'}[platform.system()]
readelf = ndk/'toolchains/llvm/prebuilt'/host/'bin/llvm-readelf'
for abi, machine in [('arm64-v8a', 'AArch64'), ('armeabi-v7a','ARM')]:
    path = root/'frontends/pocket-android/app/src/main/jniLibs'/abi/'libpockethle_jni.so'
    if not path.is_file(): raise SystemExit(f'Missing: {path}')
    def read(*args): return subprocess.check_output([str(readelf),*args,str(path)],text=True)
    if f'Machine:                           {machine}' not in read('-h'):
        # Spacing is not part of the readelf contract.
        if not re.search(r'Machine:\s+'+machine+r'\s*$',read('-h'),re.M): raise SystemExit(f'Wrong ABI: {path}')
    loads = [line.split() for line in read('-lW').splitlines() if line.lstrip().startswith('LOAD ')]
    if not loads or any(int(row[-1],16)<16384 for row in loads): raise SystemExit(f'Not 16 KiB aligned: {path}')
    dynamic = read('-dW')
    if 'TEXTREL' in dynamic: raise SystemExit(f'Text relocations: {path}')
    if re.search(r'NEEDED.*lib(avcodec|avformat|avutil|swscale|swresample)',dynamic): raise SystemExit(f'FFmpeg is not static: {path}')
    if re.search(r'\bUND\s+__clear_cache\b',read('--dyn-syms','-W')): raise SystemExit(f'Unresolved __clear_cache: {path}')
    print(f'OK {abi}: ELF, static FFmpeg, JIT builtins, 16 KiB alignment')
