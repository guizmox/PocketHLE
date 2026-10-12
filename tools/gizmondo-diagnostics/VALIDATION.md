# Diagnostic update validation

Base: guizmox/PocketHLE `4123cff2c98c10592bc44a3ac4d55fcbc7e924a5`.
Clang 18.1.8 / LLVM LLD, Rust 1.90.0; tests use locked dependencies and warnings denied.

- Kernel: 188 tests passed.
- WinCE APIs: 288 passed; three native presentation tests initially ignored, then all three passed explicitly.
- GLES: 136 passed; 20 native-driver tests passed explicitly. The timing benchmark remains excluded.
- Library: 50 passed.
- Unicorn: five RAM/VFS/timing integrations plus five hardware integrations passed.
- RAMTEST: 136 checks passed, including DLL/DEP/PROC/ORPHAN side reports.
- VFSTEST: 108 checks passed on both runs, marker counter advanced 1 to 2.
- TIMETEST: 44 checks passed; silent audio completion and software EGL timing.
- Controlled CAM/GPS/VIB/NET providers returned live counts to zero; BT startup and inquiry state cleaned up.
- All eight ARM/WinCE diagnostics rebuilt; ZIP CRC and PE machine/subsystem checked.
- Full workspace rustfmt and git diff whitespace checks passed.

The one-shot runner was exercised with `--guest --native-gpu` and returned PASS.
Native rendering used Mesa surfaceless/software-host GL/GLES, not a physical GPU.
Hardware providers were controlled test adapters, not Windows/Android devices.
No complete Windows GUI/APK build or physical radio/camera/GPS/TLS/audio test was
performed here. No RSS/driver/JIT leak certification is claimed.

Detected corrections: closed/unknown DeviceIoControl handles set LastError 6;
DirectShow test fixtures share a test-only graph gate to avoid parallel resets.
No production resource monitor, game-specific instrumentation or unrelated
PocketPC upstream-sync change is included.
