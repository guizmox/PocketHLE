#!/usr/bin/env python3
"""Run bounded contract suites; keep hardware/gameplay validation separate."""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import zipfile

ROOT = Path(__file__).resolve().parents[2]


def restore_ram_fixtures():
    """A fresh clone ignores raw DLLs; recover missing fixtures from the checked-in ZIP."""
    directory = ROOT / "tools/ramtest/dist"
    with zipfile.ZipFile(directory / "PocketHLE-RAMTEST.zip") as archive:
        for member in archive.infolist():
            parts = Path(member.filename).parts
            if member.is_dir() or len(parts) != 2 or parts[0] != "GZRT999999" or parts[1] in (".", ".."):
                raise ValueError("Unexpected RAM fixture archive layout")
            destination = directory / parts[0] / parts[1]
            if destination.exists():
                continue
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(archive.read(member))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--guest", action="store_true", help="rebuild ARM fixtures and run Unicorn integration tests")
    parser.add_argument("--native-gpu", action="store_true", help="also execute normally ignored native GPU tests (needs a working host driver)")
    parser.add_argument("--clang", default="clang")
    parser.add_argument("--lld", default="ld.lld")
    parser.add_argument("--timeout", type=int, default=900, help="seconds per build/test command")
    parser.add_argument("--output", type=Path, default=ROOT / "target/gizmondo-diagnostics")
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    args.output.mkdir(parents=True, exist_ok=True)
    commands = []
    if args.guest:
        for tool in ("ramtest", "vfstest", "timetest", "camtest", "gpstest", "nettest", "vibtest", "bttest"):
            commands.append(("build-" + tool, [sys.executable, str(ROOT / "tools" / tool / "build.py"), "--clang", args.clang, "--lld", args.lld]))
    else:
        try:
            restore_ram_fixtures()
        except (OSError, ValueError, zipfile.BadZipFile) as exc:
            parser.error("Cannot restore RAM fixtures: " + str(exc))
    for package in ("pocket-kernel", "pocket-winceapi", "pocket-gles", "pocket-library"):
        commands.append((package, ["cargo", "test", "--locked", "-p", package, "--no-default-features", "--lib"]))
    if args.guest:
        commands.append(("guest-arm", ["cargo", "test", "--locked", "-p", "pocket-core", "--no-default-features", "--features", "unicorn", "--test", "ram_guest", "--test", "device_guest", "--", "--nocapture", "--test-threads=1"]))
    if args.native_gpu:
        commands.append(("native-gpu", ["cargo", "test", "--locked", "-p", "pocket-gles", "--lib", "--", "--ignored", "--skip", "benchmark_inside_triangle_fast_path"]))
        commands.append(("native-presentation", ["cargo", "test", "--locked", "-p", "pocket-winceapi", "--no-default-features", "--lib", "gles::tests::", "--", "--ignored"]))
    results = []
    for name, command in commands:
        log = args.output / (name + ".log")
        print("Running " + name, flush=True)
        error = None
        with log.open("w", encoding="utf-8") as output:
            try:
                code = subprocess.run(command, cwd=ROOT, stdout=output, stderr=subprocess.STDOUT, timeout=args.timeout).returncode
            except (OSError, subprocess.TimeoutExpired) as exc:
                code = 1
                error = str(exc)
                output.write("\nRUNNER ERROR: " + error + "\n")
        results.append({"suite": name, "exit_code": code, "log": str(log), "error": error})
        print(("PASS " if code == 0 else "FAIL ") + name, flush=True)
        # A failed build must not run an old diagnostic binary.
        if code != 0:
            break
    report = {"result": "PASS" if len(results) == len(commands) and all(r["exit_code"] == 0 for r in results) else "FAIL",
              "guest_arm_requested": args.guest, "native_gpu_requested": args.native_gpu,
              "limits": "Default cargo ignored tests stay ignored. No physical camera/BT/GPS/audio or game run is certified. This is not an OS memory leak detector.",
              "suites": results}
    (args.output / "results.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print("Report: " + str(args.output / "results.json"))
    return 0 if report["result"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
