# BTTEST v2

Run **Bluetooth** from the single [HARDWARE DIAG application](../../README.md)
with Bluetooth hardware enabled. The tool retains
Classic inquiry, MAC addresses and Unicode device names from v1, and adds 32
Winsock startup/cleanup cycles plus 8 discovery begin/cancel cycles.
It writes BTTEST.TXT and shows discovered devices. No peers is not a failure or
proof of multiplayer connectivity. It is not BLE or an SDP game-service test.
The diagnostic enables the guest Bluetooth service through BT_MSG, respecting
the emulator permission gate; it does not force the host radio on.

Rebuild: `python3 tools/hardware-diag/suites/bluetooth/build.py --clang clang --lld ld.lld`.
Physical Android/Windows radio and game exchange still need two-host testing.

## Suite build scope

The local build script generates private fixtures for regressions. Build/import
`tools/hardware-diag/dist/HARDWARE-DIAG.zip` through the main tool's README for
normal use. Fixture check counts retain their original revision scope; physical
provider validation remains separate from an API PASS.
