#!/usr/bin/env python3
"""Build HARDWARE DIAG and its private regression fixtures (Clang/LLD, no SDK)."""
import argparse
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import sys
import zipfile

ROOT = Path(__file__).resolve().parent
CARD = 'GZHD999989'
SUITES = {'ram': 'GZRT999999', 'vfs': 'GZVT999998', 'timing': 'GZTT999995',
          'camera': 'GZCT999997', 'gps': 'GZGP999994', 'network': 'GZNT999993',
          'vibration': 'GZVB999990', 'bluetooth': 'GZBT999996'}
spec = importlib.util.spec_from_file_location('pe_builder', ROOT / 'suites/ram/build.py')
b = importlib.util.module_from_spec(spec)
spec.loader.exec_module(b)
SIG = dict(b.SIGNATURES)
SIG.update({
    'RegisterClassW': ('unsigned int', 'void*'),
    'CreateWindowExW': ('unsigned int', 'unsigned int,const unsigned short*,const unsigned short*,unsigned int,int,int,int,int,unsigned int,unsigned int,unsigned int,void*'),
    'DefWindowProcW': ('unsigned int', 'unsigned int,unsigned int,unsigned int,unsigned int'),
    'ShowWindow': ('unsigned int', 'unsigned int,unsigned int'),
    'SetFocus': ('unsigned int', 'unsigned int'),
    'PostQuitMessage': ('void', 'unsigned int'),
    'DestroyWindow': ('unsigned int', 'unsigned int'),
    'PeekMessageW': ('unsigned int', 'void*,unsigned int,unsigned int,unsigned int,unsigned int'),
    'TranslateMessage': ('unsigned int', 'void*'),
    'DispatchMessageW': ('unsigned int', 'void*'),
    'GetDC': ('unsigned int', 'unsigned int'),
    'ReleaseDC': ('unsigned int', 'unsigned int,unsigned int'),
    'ValidateRect': ('unsigned int', 'unsigned int,void*'),
    'TextOutW': ('unsigned int', 'unsigned int,int,int,const unsigned short*,int'),
    'PatBlt': ('unsigned int', 'unsigned int,int,int,int,int,unsigned int'),
    'SelectObject': ('unsigned int', 'unsigned int,unsigned int'),
    'SetBkMode': ('unsigned int', 'unsigned int,int'),
    'SetTextColor': ('unsigned int', 'unsigned int,unsigned int'),
    'CreateFontIndirectW': ('unsigned int', 'void*'),
    'GetStockObject': ('unsigned int', 'int'),
    'GetTickCount': ('unsigned int', 'void'),
    '__rt_udiv': ('unsigned int', 'unsigned int,unsigned int'),
    '__adds': ('unsigned int', 'unsigned int,unsigned int'),
    '__muls': ('unsigned int', 'unsigned int,unsigned int'),
})
COMMON = ['CreateFileW', 'ReadFile', 'WriteFile', 'CloseHandle', 'SetFilePointer',
          'GetLastError', 'SetLastError', 'GetTickCount', 'Sleep']
WINDOW = ['RegisterClassW', 'CreateWindowExW', 'DefWindowProcW', 'ShowWindow',
          'SetFocus', 'PostQuitMessage', 'DestroyWindow', 'PeekMessageW',
          'TranslateMessage', 'DispatchMessageW']


def compile_integrated(args, card):
    """Link all ordinary suites and process-test roles into ONE ARM image."""
    import struct
    work = args.work / 'integrated'
    work.mkdir(parents=True, exist_ok=True)
    modules = {}
    for suite in SUITES:
        spec = importlib.util.spec_from_file_location('hd_' + suite, ROOT / 'suites' / suite / 'build.py')
        module = importlib.util.module_from_spec(spec)
        # VFS's helper import and RAM's build dependencies use their local folder.
        sys.path.insert(0, str(ROOT / 'suites/ram'))
        spec.loader.exec_module(module)
        modules[suite] = module
    core_names = list(dict.fromkeys(list(SIG) + ['CreateThread', 'GetExitCodeThread']))
    dlls = {'coredll.dll': core_names}
    for module in modules.values():
        groups = getattr(module, 'DLLS', {'coredll.dll': getattr(module, 'NAMES', [])})
        for dll, names in groups.items():
            target = dlls.setdefault(dll, [])
            for api in names:
                if api not in target: target.append(api)
    ram_names = __import__('json').loads((ROOT / 'suites/ram/src/imports.json').read_text())
    vfs_names = __import__('json').loads((ROOT / 'suites/vfs/src/imports.json').read_text())
    for api in ram_names + vfs_names:
        if api not in core_names: core_names.append(api)
    signatures = dict(SIG)
    for module in modules.values():
        signatures.update(getattr(module, 'SIG', {}))
        signatures.update(getattr(module, 'ADDITIONAL', {}))
    # One PE import table, at a fixed address outside the linked code/data.
    import_rva = 0xf0000
    data = bytearray(0x4000)
    cursor, iat, ilt, dllname = 0x2000, 0x200, 0x1000, 0x80
    addresses = {}
    for group, (dll, names) in enumerate(dlls.items()):
        data[group*20:group*20+20] = struct.pack('<IIIII', import_rva+ilt, 0, 0, import_rva+dllname, import_rva+iat)
        raw = dll.encode() + b'\0'
        data[dllname:dllname+len(raw)] = raw
        dllname += len(raw)
        for api in names:
            if api not in signatures: raise ValueError('No signature: ' + api)
            raw = b.U16(0) + api.encode() + b'\0'
            data[cursor:cursor+len(raw)] = raw
            data[iat:iat+4] = b.U32(import_rva+cursor)
            data[ilt:ilt+4] = b.U32(import_rva+cursor)
            addresses[api] = 0x10000 + import_rva + iat
            iat += 4; ilt += 4; cursor = b.align(cursor+len(raw), 2)
        iat += 4; ilt += 4
    if dllname >= 0x200 or iat >= 0x1000 or ilt >= 0x2000 or cursor > len(data):
        raise ValueError('Unified import table overflow')
    header = '#ifndef HARDWARE_IMPORTS_H\n#define HARDWARE_IMPORTS_H\n'
    for api, address in addresses.items():
        ret, params = signatures[api]
        header += f'#define {api} (*({ret} (**)({params}))0x{address:08x}u)\n'
    (work / 'imports.h').write_text(header + '\n#endif\n')
    files = {'launcher': ROOT/'src/hardware_diag.c', 'integrated': ROOT/'src/integrated.c',
             'gles': ROOT/'src/gles_info.c', 'integer': ROOT/'src/benchmark.c',
             'floating': ROOT/'src/benchmark.c', 'glesbench': ROOT/'src/gles_benchmark.c',
             'proctest': ROOT/'suites/ram/src/proctest.c',
             'procworker': ROOT/'suites/ram/src/procworker.c',
             'vfsworker': ROOT/'suites/vfs/src/vfstest.c'}
    for suite in SUITES:
        files[suite] = ROOT/'suites'/suite/'src'/({'ram':'ramtest','vfs':'vfstest','timing':'timetest','camera':'camtest','gps':'gpstest','network':'nettest','vibration':'vibtest','bluetooth':'bttest'}[suite]+'.c')
    objects = []
    for name, source in files.items():
        obj = work / (name + '.o')
        defines = ['-DHARDWARE_DIAG', '-DHARDWARE_DIAG_MONOLITHIC']
        if name not in ('launcher', 'integrated'):
            defines += ['-Dentry=diag_'+name+'_entry', '-include', str(ROOT/'src/diag_client.h'),
                        '-include', str(ROOT/'src/monolithic.h')]
            for helper in ('__aeabi_memclr', '__aeabi_memclr4', '__aeabi_memclr8', 'memcpy', 'memset'):
                defines.append('-D'+helper+'=diag_'+name+'_'+helper)
        elif name == 'launcher':
            defines += ['-D__aeabi_memclr4=diag_launcher_memclr4']
        if name == 'ram': defines += ['-DHARDWARE_DIAG_RAM']
        if name == 'vfsworker': defines += ['-DWORKER']
        if name == 'integer': defines += ['-DINT_EXPECTED=0xef7a1aa1u']
        if name == 'floating': defines += ['-DFLOATING', '-DFLOAT_EXPECTED=0x457bc037u']
        subprocess.run([args.clang, '--target=armv4t-none-eabi', '-marm', '-mfloat-abi=soft',
                        '-ffreestanding', '-fno-builtin', '-fshort-wchar', '-fno-stack-protector',
                        '-fno-unwind-tables', '-fno-asynchronous-unwind-tables', '-O1',
                        '-I', str(work), *defines, '-c', str(source), '-o', str(obj)], check=True)
        objects.append(obj)
    # Per-suite zero-initialized globals are isolated and reset on each run.
    # Do not silently discard future nonzero mutable initializers.
    for name in list(SUITES) + ['gles','integer','floating','glesbench']:
        blob = (work/(name+'.o')).read_bytes()
        shoff = struct.unpack_from('<I',blob,32)[0]
        shsize, shnum, shstr = struct.unpack_from('<HHH',blob,46)
        sections = [struct.unpack_from('<IIIIIIIIII',blob,shoff+i*shsize) for i in range(shnum)]
        names = sections[shstr]; strings = blob[names[4]:names[4]+names[5]]
        for section in sections:
            end = strings.find(b'\0',section[0]); label = strings[section[0]:end]
            if label.startswith(b'.data') and section[5]:
                raise ValueError(name+': initialized writable globals need an explicit reset policy')
    script = 'ENTRY(entry)\nSECTIONS { . = 0x11000; .image : { __image_start = .; *(.text.entry) *(.text*) *(.rodata*) *(.data*)\n'
    for name in list(SUITES) + ['gles','integer','floating','glesbench']:
        script += f'. = ALIGN(4); diag_{name}_bss_start = .; "{(work/(name+".o")).as_posix()}"(.bss* COMMON) diag_{name}_bss_end = .;\n'
    script += '*((.bss*)) *(COMMON) __image_end = .; } /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.comment*) *(.note*) } }\n'
    # LLD's input section syntax, without a nested wildcard group.
    script = script.replace('*((.bss*))', '*(.bss*)')
    (work/'image.ld').write_text(script)
    common = [args.lld, '-m', 'armelf', '-T', str(work/'image.ld'), *(obj.as_posix() for obj in objects)]
    subprocess.run([*common, '-o', str(work/'image.elf')], check=True)
    subprocess.run([*common, '--oformat=binary', '-o', str(work/'image.bin')], check=True)
    code = (work/'image.bin').read_bytes(); syms = b.symbols(work/'image.elf')
    if len(code) >= import_rva-0x1000: raise ValueError('Integrated image overlaps imports')
    b.pe(card/'AUTORUN.EXE', 0x10000, syms['entry']-0x10000,
         [('.image',0x1000,b.align(len(code),4096),code,0xe0000060),
          ('.idata',import_rva,len(data),bytes(data),0xc0000040)],
         {1:(import_rva,20*(len(dlls)+1)),12:(import_rva+0x200,iat-0x200)})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--clang', default=os.environ.get('CLANG', 'clang'))
    parser.add_argument('--lld', default=os.environ.get('LLD', 'ld.lld'))
    parser.add_argument('--work', type=Path, default=ROOT / 'build')
    args = parser.parse_args()
    args.work = args.work.resolve()
    card = args.work / 'card' / CARD
    if card.exists():
        shutil.rmtree(card)
    card.mkdir(parents=True)
    env = dict(os.environ, HARDWARE_DIAG_LIVE='1')
    # The suites are private fixtures, not separate downloadable applications.
    # Keep their native binaries for existing integration tests after relocation.
    for suite, old_card in SUITES.items():
        folder = ROOT / 'suites' / suite
        subprocess.run([sys.executable, str(folder / 'build.py'), '--clang', args.clang,
                        '--lld', args.lld, '--work', str(args.work / suite)], env=env, check=True)
        dest = card / suite
        dest.mkdir()
        for file in sorted((folder / 'dist' / old_card).iterdir()):
            # Only real loading/process-test fixtures accompany the ONE EXE.
            if file.is_file() and (file.suffix.lower() in ('.dll', '.bin') or file.name=='badproc.exe'):
                shutil.copyfile(file, dest / ('badproc.bin' if file.name=='badproc.exe' else file.name))
    compile_integrated(args, card)
    (card / CARD).write_bytes(b.U32(999989))
    out = ROOT / 'dist' / 'HARDWARE-DIAG.zip'
    out.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(out, 'w', zipfile.ZIP_DEFLATED) as archive:
        for file in sorted(card.rglob('*')):
            if file.is_file():
                archive.write(file, file.relative_to(card.parent).as_posix())
    subprocess.run([sys.executable, str(ROOT / "verify.py"), str(out)], check=True)
    print(out)


if __name__ == '__main__':
    main()
