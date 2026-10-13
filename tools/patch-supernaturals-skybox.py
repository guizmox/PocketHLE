#!/usr/bin/env python3
"""Fix the far camera plane in the supplied Supernaturals demo executable.

This deliberately recognises one exact executable, not every Supernaturals
release. No game assets are included. See docs/GIZMONDO.md for the diagnosis.
"""

import argparse
import hashlib
from pathlib import Path

ORIGINAL_SHA256 = "3c53a5741a7312a5d0f9054b098cfa2e4280dd9bfb8d8a989bb0303ac3ff682f"
PATCHED_SHA256 = "6895f371e478e2e55fbc94911da145b898267aa5a0f998497b96b0d62ce61df7"
OFFSET = 0x12D20  # Guest VA 0x23920, PE image base 0x10000.
ORIGINAL = bytes.fromhex("12 29 a0 e3")  # mov r2, #0x48000: 4.5 in 16.16.
PATCHED = bytes.fromhex("40 29 a0 e3")  # mov r2, #0x100000: 16 in 16.16.


def patch(data: bytes) -> bytes:
    digest = hashlib.sha256(data).hexdigest()
    if digest == PATCHED_SHA256:
        return data
    if digest != ORIGINAL_SHA256 or data[OFFSET : OFFSET + 4] != ORIGINAL:
        raise ValueError("Unsupported executable; no files were changed")
    result = data[:OFFSET] + PATCHED + data[OFFSET + 4 :]
    if hashlib.sha256(result).hexdigest() != PATCHED_SHA256:
        raise ValueError("Patch verification failed; no files were changed")
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path, help="Original Supernaturals.exe")
    parser.add_argument("output", type=Path, help="New executable; must not exist")
    args = parser.parse_args()
    try:
        result = patch(args.input.read_bytes())
        # Exclusive creation also protects the input from accidental overwrite.
        with args.output.open("xb") as output:
            output.write(result)
    except (OSError, ValueError) as error:
        parser.exit(1, f"{error}\n")
    print(f"Saved {args.output}: SHA256 {PATCHED_SHA256}")


if __name__ == "__main__":
    main()
