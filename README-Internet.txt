PocketHLE — Internet access on Windows and Android

Extract the patch into the repository root, replacing existing files.
The complete files preserve the current RAM/VFS, display, audio, camera,
Bluetooth and GPS changes.

From CMD at the repository root:
powershell -NoProfile -Command "Get-Content 'patch-files.txt' | ForEach-Object { (Get-Item -LiteralPath $_).LastWriteTime = Get-Date }"
cargo build --release -p pocket-desktop

Windows games use the PC's Internet connection. For Colors, configure its server
address and enable GPRS/data in Gizmondo options before starting the game.
Android requires rebuilding both the APK and Rust native libraries; follow
docs/ANDROID-BUILD.md. INTERNET permission is declared, legacy HTTP is allowed,
and HTTPS uses normal certificate validation. The original Internet patch
introduced no additional networking crate.

Scope: nine WinINet HTTP/HTTPS APIs requested by Colors, GET/POST, headers,
status codes, binary streams, session cookies, redirects, proxy handling,
GetLastError, guest CPU yielding, cancellation and cascading close.
The bridge does not implement general Winsock TCP/UDP sockets, FTP, asynchronous
WinINet callbacks or every Windows CE WinINet option. The original delivery
excluded the eight other Colors CRT/date imports identified at that stage;
additional Colors fixes are included in later cumulative patches.
The Internet bridge itself does not recreate the original Gizmondo servers.

Device test: import tools/nettest/dist/PocketHLE-NETTEST.zip and launch it.
Wait for both HTTP and HTTPS requests. NETTEST.TXT is written to the test's
Flash Disk directory; failures record GetLastError in hexadecimal.
Then test Colors and provide its application/API log if needed.

The original Internet delivery reported 442 passing Rust tests and a successful
ARM NETTEST loopback run (GET, POST, 256 KiB per response, UTF-16 ABI, EOF and
handles). Desktop/JNI compilation checks and WinHTTP signature checks passed.
Native Windows/TLS execution was not performed in that validation environment.
The current Android APK has now been built; see docs/ANDROID-BUILD.md for
its checks and remaining physical-device/Turf Wars validation.
No per-frame diagnostic instrumentation was added.
