PocketHLE — Bluetooth Classic / RFCOMM — 2026-10-09

Installation
============
Extract into the PocketHLE repository root, replacing existing files.
The complete files preserve the preceding general API fixes.
Original system DLLs and temporary validation files are not included.

powershell -NoProfile -Command "Get-Content 'patch-files.txt' | ForEach-Object { (Get-Item -LiteralPath $_).LastWriteTime = Get-Date }"
cargo build --release -p pocket-desktop

Enabling Bluetooth
==================
Enable "Bluetooth hardware (Classic / RFCOMM)" in the emulator/Gizmondo options
and save. The setting is disabled by default and applies at the next game launch.
Turn on the Windows Bluetooth radio; PocketHLE does not force its global state.
Pair devices through their operating systems' Bluetooth settings.
BT_MSG enables/disables the emulated service subject to the hardware setting.
Other games do not initiate discovery or connections automatically.

Android uses the same setting. Required Bluetooth/location permissions are
requested before launching a game with Bluetooth enabled. Enable the radio and
pair devices through Android. A denied permission remains an actual error;
the bridge does not simulate a connection to hide the denial.

Windows hardware test, independent of the game
=============================================
The desktop build also produces target\release\pockethle-bt-test.exe.
The GUI never launches this diagnostic automatically.

On the server PC:
target\release\pockethle-bt-test.exe server

On a second PC while the server is waiting:
target\release\pockethle-bt-test.exe scan
target\release\pockethle-bt-test.exe client AA:BB:CC:DD:EE:FF

Replace AA:BB:CC:DD:EE:FF with the server PC address reported by scan.
The timeout is 60 seconds; restart the server if it expires.
PASS confirms a real RFCOMM connection and bidirectional ping/pong.
Scanning alone checks hardware discovery, not data exchange.
Two instances on one PC are insufficient: RFCOMM has no radio loopback.
This diagnostic is a native executable, not a guest ARM test.
After it passes, test multiplayer between two PocketHLE hosts by creating a
session on one and joining from the other. Android needs a rebuilt APK and
an actual second device for multiplayer validation.

Supported SDK path
==================
BT_MSG, WSAStartup/WSACleanup, gethostname, WSALookupServiceBeginW/NextW/End,
RegisterDevice/DeregisterDevice for btd.dll and COM1..COM9, opening COMn:,
SetCommMask/GetCommMask/WaitCommEvent for EV_RXCHAR and cancellation through a
zero mask, and synchronous ReadFile/WriteFile are supported.
Discovery names without the W suffix are recognised too. WS2 is available
through LoadLibrary/GetProcAddress using ordinals from the supplied Gizmondo DLL.

COM handles can be duplicated/transferred between processes while retaining
their connection and permissions. Deregistering a device cancels pending
operations; COM4 can be registered again even if old handles remain open.
Partial writes resume at their offset without duplicating or truncating data.
Guest buffers are checked before I/O.
Windows uses nonblocking Winsock AF_BTH, Bluetooth discovery and SDP advertising.
Android uses secure BluetoothSocket connections, host threads and bounded RX/TX
queues. Both transports share a service UUID per guest channel and respect an
explicit GUID supplied by the program.

Limits and validation
=====================
The original Bluetooth delivery reported 415 passing software tests:
145 kernel, 234 WinCE API and 36 library. A controlled transport tested the SDK
path through the dispatcher: WSADATA/WSAQUERYSET sizes, ARM pointers, discovery,
waiting, exchanges, errors, masks, duplication, cancellation, registration
and partial-write retries. Desktop was checked with Unicorn and audio;
Windows Rust and Android JNI modules were checked against native API types.
The original Linux desktop check temporarily disabled static video and used
xdg-portal; those environment-specific changes were not delivered.
The current Kotlin code, both Android native libraries and the full debug APK
have now compiled. See docs/ANDROID-BUILD.md for exact validation results.
No connection between physical Bluetooth radios has been tested here.
Software checks do not establish real multiplayer compatibility.

The patch does not implement all 79 Winsock and 83 BTD exports. General IP
sockets, direct guest Winsock RFCOMM sockets, low-level HCI/L2CAP/SDP driver APIs,
service/filter searches, overlapped COM I/O, custom MTU/quotas and UART/modem/DCB
control remain outside its scope. REMOTE_DCB and KEEP_DCD are accepted for the
SDK path but do not provide modem/DCB control. Without an available transport,
WSAStartup fails normally; recv never fabricates EOF.
Interoperability with a physical Gizmondo is not guaranteed: its SDK uses a
fixed physical channel, whereas PocketHLE hosts share a service UUID whose
RFCOMM channel is allocated by the operating system. Start with two PocketHLE
hosts. Windows discovery uses the first available radio adapter.
Normal logging and audio are preserved; no temporary instrumentation was added.

Technical references
====================
Supplied Gizmondo SDK: Examples/Bluetooth/Bluetooth.cpp; ws2.dll/btd.dll exports.
https://learn.microsoft.com/en-us/windows/win32/bluetooth/bluetooth-and-wsaqueryset-for-set-service
https://learn.microsoft.com/en-us/windows/win32/bluetooth/bluetooth-and-bind
https://developer.android.com/develop/connectivity/bluetooth/connect-bluetooth-devices
https://developer.android.com/develop/connectivity/bluetooth/bt-permissions
