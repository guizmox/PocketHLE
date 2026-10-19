#!/usr/bin/env python3
"""Repeatable unit/optional ARM diagnostic runner. No commercial assets required."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def run(command, log):
    print('+ ' + ' '.join(map(str, command)), flush=True)
    with log.open('w', encoding='utf-8') as output:
        completed = subprocess.run(command, cwd=ROOT, stdout=output,
                                   stderr=subprocess.STDOUT, text=True)
    print(log, flush=True)
    if completed.returncode:
        print(log.read_text(encoding='utf-8', errors='replace')[-12000:])
        raise SystemExit(completed.returncode)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cargo', default=os.environ.get('CARGO', 'cargo'))
    parser.add_argument('--guest', action='store_true', help='build and execute ARM diagnostics under Unicorn')
    parser.add_argument('--clang', default=os.environ.get('CLANG', 'clang'))
    parser.add_argument('--lld', default=os.environ.get('LLD', 'ld.lld'))
    parser.add_argument('--repeat', type=int, default=1, help='1 full unit run, then diagnostic-only repetitions (1..100)')
    parser.add_argument('--logs', type=Path, default=ROOT / 'verification' / 'storage-memory-local')
    args = parser.parse_args()
    if not 1 <= args.repeat <= 100:
        parser.error('--repeat must be between 1 and 100')
    # Check prerequisites before starting a long run; never silently skip ARM.
    if args.guest:
        for program in (args.clang, args.lld):
            if shutil.which(program) is None:
                parser.error('ARM prerequisite unavailable: ' + program)
    args.logs = args.logs.resolve()
    args.logs.mkdir(parents=True, exist_ok=True)
    units = [args.cargo, 'test', '-p', 'pocket-kernel', '-p', 'pocket-winceapi',
             '--no-default-features', '--locked', '--lib']
    for iteration in range(args.repeat):
        run(units + ([] if iteration == 0 else ['diagnostic_']) +
            ['--', '--test-threads=1'], args.logs / f'unit-{iteration + 1:03}.log')
    if args.guest:
        # ram_guest.rs embeds TIMETEST too, even when a name filter is passed.
        for suite in ('ramtest', 'vfstest', 'timetest'):
            run([sys.executable, str(ROOT / 'tools' / suite / 'build.py'),
                 '--clang', args.clang, '--lld', args.lld], args.logs / f'build-{suite}.log')
        run([args.cargo, 'test', '-p', 'pocket-core', '--no-default-features',
             '--features', 'unicorn', '--locked', '--test', 'ram_guest',
             '--', '--nocapture', '--test-threads=1'], args.logs / 'arm-unicorn.log')
    print('Requested checks completed. Inspect ignored tests in logs; host hardware is not certified.')


if __name__ == '__main__':
    main()
