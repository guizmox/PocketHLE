#!/usr/bin/env python3
"""Validate the public one-executable HARDWARE DIAG card without dependencies."""
from pathlib import Path, PurePosixPath
import struct
import sys
import zipfile


def verify(path):
    with zipfile.ZipFile(path) as archive:
        if archive.testzip() is not None:
            raise ValueError('Diagnostic ZIP CRC failure')
        names = archive.namelist()
        if len(names) != len(set(names)):
            raise ValueError('Duplicate archive members')
        for name in names:
            parts = PurePosixPath(name).parts
            if not parts or parts[0] != 'GZHD999989' or '..' in parts or '\\' in name:
                raise ValueError('Unsafe or unexpected card path: ' + name)
        executables = [name for name in names if name.lower().endswith('.exe')]
        if executables != ['GZHD999989/AUTORUN.EXE']:
            raise ValueError('The public application must contain exactly ONE executable')
        if archive.read('GZHD999989/GZHD999989') != struct.pack('<I', 999989):
            raise ValueError('Invalid diagnostic title marker')
        if archive.read('GZHD999989/vfs/asset.bin') != b'CARD-DATA':
            raise ValueError('VFS fixture changed')
        if archive.read('GZHD999989/ram/badproc.bin').startswith(b'MZ'):
            raise ValueError('Invalid-image fixture unexpectedly became an executable')
        for name in [executables[0]] + [name for name in names if name.lower().endswith('.dll')]:
            image = archive.read(name)
            offset = struct.unpack_from('<I', image, 60)[0]
            if image[:2] != b'MZ' or image[offset:offset+4] != b'PE\0\0':
                raise ValueError('Invalid PE: ' + name)
            machine = struct.unpack_from('<H', image, offset+4)[0]
            subsystem = struct.unpack_from('<H', image, offset+24+68)[0]
            if machine != 0x1c0 or subsystem != 9:
                raise ValueError('Expected native ARM Windows CE image: ' + name)
        print('HARDWARE DIAG verified: one ARM EXE, WinCE DLL fixtures, safe paths and ZIP CRCs')


if __name__ == '__main__':
    verify(Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).parent/'dist/HARDWARE-DIAG.zip')
