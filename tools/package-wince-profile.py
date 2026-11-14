#!/usr/bin/env python3
"""Package public CE profile templates, or a private cross-platform library backup."""
import argparse
import json
import os
from pathlib import Path
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parent.parent
LAUNCHER = r'''@echo off
setlocal
set "POCKETHLE_CE_GUI=%~dp0pockethle-gui.exe"
if not "%~1"=="" set "POCKETHLE_CE_GUI=%~1"
if not exist "%POCKETHLE_CE_GUI%" (
  echo Put pockethle-gui.exe beside this script, or pass its path as the first argument.
  exit /b 1
)
"%POCKETHLE_CE_GUI%" --ce-profile "%~dp0profiles\wince6-native.json" --windowed
'''


def put_json(archive, name, value):
    archive.writestr(name, json.dumps(value, ensure_ascii=False, indent=2) + '\n')


def add_tree(archive, source, prefix):
    seen = set()
    for path in sorted(source.rglob('*')):
        if path.is_symlink():
            raise ValueError(f'Symbolic links are not supported: {path}')
        if not path.is_file():
            continue
        relative = path.relative_to(source).as_posix()
        key = relative.casefold()
        if key in seen or any(part in ('.', '..') for part in relative.split('/')):
            raise ValueError(f'Non-portable image path: {relative}')
        seen.add(key)
        archive.write(path, f'{prefix}/{relative}')


def package(output, image=None, explorer=None, cerf=None):
    private = image is not None
    if not private and (explorer is not None or cerf is not None):
        raise ValueError('Private application folders require --image')
    if private:
        image = image.resolve(strict=True)
        if not (image / 'Windows').is_dir() and (image / 'fs/Windows').is_dir():
            image = image / 'fs'
        if not (image / 'Windows/coredll.dll').is_file():
            raise ValueError('Select the filesystem containing Windows/coredll.dll')
        if not (image / 'Windows/explorer.exe').is_file():
            raise ValueError('The image must contain the original Windows/explorer.exe')
        if explorer is not None:
            explorer = explorer.resolve(strict=True)
            if not (explorer / 'explorer-portable.exe').is_file() or not (explorer / 'pceshell.dll').is_file():
                raise ValueError('Portable Explorer EXE and pceshell.dll are required')
        if cerf is not None:
            cerf = cerf.resolve(strict=True)
            if not cerf.is_dir():
                raise ValueError('--cerf-apps must name the ARM application directory')
    output = output.resolve()
    if any(source is not None and output.is_relative_to(source) for source in (image, explorer, cerf)):
        raise ValueError('Write the ZIP outside the supplied image and application directories')
    output.parent.mkdir(parents=True, exist_ok=True)
    profiles = {name: json.loads((ROOT / f'profiles/{name}.json').read_text(encoding='utf-8'))
                for name in ('wince6-native', 'wince6-native-cerf', 'wince6-portable', 'wince6-cmd')}
    if private:
        for name in ('wince6-native', 'wince6-native-cerf'):
            profiles[name].update(executable='../ce-images/wince6/fs/Windows/explorer.exe',
                                  image_root='../ce-images/wince6/fs')
        if (image / 'Windows/cmd.exe').is_file():
            profiles['wince6-cmd'].update(executable='../ce-images/wince6/fs/Windows/cmd.exe',
                                         image_root='../ce-images/wince6/fs')
        else:
            profiles.pop('wince6-cmd')
        profiles['wince6-native-cerf']['mounts'] = [dict(guest_prefix='\\CERF\\', host_dir='../ce-apps/cerf/arm')]
        if explorer is not None:
            profiles['wince6-portable'].update(executable='../games/wince6-desktop/extracted/explorer-portable.exe',
                                              image_root='../ce-images/wince6/fs')
        if cerf is not None:
            profiles['wince6-native']['mounts'] = profiles['wince6-native-cerf']['mounts']
    mounts = [dict(guest_prefix='\\Windows\\', host_dir='ce-images/wince6/fs/Windows')]
    if cerf is not None:
        mounts.append(dict(guest_prefix='\\CERF\\', host_dir='ce-apps/cerf/arm'))
    settings = dict(profile_name='wince6-native', image_root='ce-images/wince6/fs',
                    data_dir='ce-data/wince6-native', app_prefix='\\Windows\\',
                    command_line='', classic_windows=True, mounts=mounts)
    with tempfile.NamedTemporaryFile(dir=output.parent, prefix='.wince-profile-', suffix='.zip', delete=False) as temp:
        pending = Path(temp.name)
    try:
        with zipfile.ZipFile(pending, 'w', zipfile.ZIP_DEFLATED, compresslevel=6,
                             strict_timestamps=False) as archive:
            for name, profile in profiles.items():
                put_json(archive, f'profiles/{name}.json', profile)
            archive.writestr('run-wince6-gui.cmd', LAUNCHER.replace('\n', '\r\n'))
            archive.writestr('run-wince6-portable.cmd', LAUNCHER.replace('wince6-native.json', 'wince6-portable.json').replace('\n', '\r\n'))
            if 'wince6-cmd' in profiles:
                archive.writestr('run-wince6-cmd.cmd', LAUNCHER.replace('wince6-native.json', 'wince6-cmd.json').replace('\n', '\r\n'))
            put_json(archive, 'windows-ce-android.json', dict(screen='vga_landscape', rotation='none', max_slices=0, windows_ce=settings))
            archive.write(ROOT / 'docs/WINDOWS-CE.md', 'README-Windows-CE.md')
            if private:
                put_json(archive, '__pockethle_backup__.json', dict(format='PocketHLE', version=1))
                game = dict(id='wince6-native-desktop', platform='pocket_pc', display_name='Windows CE 6 — Original Explorer',
                            provider='Windows CE', executable='extracted/explorer.exe',
                            source_cab='wince6-native-private.zip', install_dir=None, install_dirs=[],
                            save_prefix=None, registry=[], imported_at=0, icon=None,
                            companions=[],
                            settings=dict(screen='vga_landscape', rotation='none', max_slices=0, windows_ce=settings))
                games = [game]
                put_json(archive, 'games/wince6-native-desktop/game.json', game)
                archive.write(image / 'Windows/explorer.exe', 'games/wince6-native-desktop/extracted/explorer.exe')
                if explorer is not None:
                    portable_settings = dict(settings, profile_name='wince6-portable', data_dir='ce-data/wince6',
                                             app_prefix='\\Application\\', command_line='\\Application\\',
                                             mounts=[mount for mount in mounts if mount['guest_prefix'] != '\\Windows\\'])
                    portable = dict(game, id='wince6-desktop', display_name='Windows CE 6 — Portable Explorer',
                                    executable='extracted/explorer-portable.exe', source_cab='wince6-desktop-private.zip',
                                    companions=['extracted/pceshell.dll'],
                                    settings=dict(screen='vga_landscape', rotation='none', max_slices=0, windows_ce=portable_settings))
                    games.append(portable)
                    put_json(archive, 'games/wince6-desktop/game.json', portable)
                    add_tree(archive, explorer, 'games/wince6-desktop/extracted')
                put_json(archive, 'library.json', dict(schema_version=1, games=games))
                add_tree(archive, image, 'ce-images/wince6/fs')
                if cerf is not None:
                    add_tree(archive, cerf, 'ce-apps/cerf/arm')
                archive.writestr('README-IMPORT.txt',
                    'PRIVATE USER BACKUP — do not publish as an emulator release.\n\n'
                    'Android: install PocketHLE-Reborn, open Restore backup, select this ZIP,\n'
                    'choose Merge without overwriting, then launch Windows CE 6 — Original Explorer.\n'
                    'Windows: Restore backup in the GUI, or extract and use run-wince6-gui.cmd.\n'
                    'The device registry is created from the included CE hives on first launch.\n')
            else:
                archive.writestr('README-IMPORT.txt',
                    'PUBLIC PROFILE TEMPLATES — supply your own CE image.\n'
                    'Extract beside the Windows emulator. Place the image in wince6/fs and\n'
                    'run run-wince6-gui.cmd to start its original Windows/explorer.exe.\n'
                    'The CERF profile expects applications in cerf/arm. Portable Explorer is\n'
                    'optional: place it in portable-explorer/bin and use run-wince6-portable.cmd.\n'
                    'On Android use the CE profile settings/image importer, or restore a complete private backup.\n')
        with zipfile.ZipFile(pending) as archive:
            bad = archive.testzip()
            if bad:
                raise ValueError(f'ZIP integrity check failed: {bad}')
            count = len(archive.infolist())
        os.replace(pending, output)
        print(f'{"PRIVATE BACKUP" if private else "PROFILE TEMPLATES"}: {output} ({count} files, {output.stat().st_size} bytes)')
    finally:
        pending.unlink(missing_ok=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--image', type=Path)
    parser.add_argument('--explorer-bin', type=Path)
    parser.add_argument('--cerf-apps', type=Path)
    args = parser.parse_args()
    package(args.output, args.image, args.explorer_bin, args.cerf_apps)
