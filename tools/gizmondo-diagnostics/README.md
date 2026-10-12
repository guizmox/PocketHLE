# Gizmondo diagnostic coverage

These tools complement each other. A PASS is scoped to the tested contracts;
it is not proof that no host/driver resource can leak or that every game works.

| Tool | Coverage added / retained | Remaining device checks |
| --- | --- | --- |
| RAMTEST v8 | Existing paging/TLS/DLL/process tests; 64 mixed lifetimes; 8 repeated process exits; exact guest RAM refunds | OS/JIT/driver memory is not visible through GlobalMemoryStatus |
| VFSTEST v3 | Existing 103 file/share/quota checks; 64 mixed file cycles; persistent run marker | Run again after restarting the emulator for restart persistence |
| TIMETEST v1 | Sleep/waits/timers; audio event/function callbacks; 40 audio lifetimes; identical EGL swap pacing and cleanup | Run Software and GPU; physical audio, DirectDraw games and pause/resume |
| CAMTEST v2 | Existing preview/capture/format checks plus 8 start/preview/stop/close/reopen cycles | Actual camera, two landscape orientations, background/resume |
| GPSTEST v2 | Existing ABI/no-fix checks plus 32 open/read/close cycles | Real provider/permissions; a no-fix result is not a leak |
| NETTEST v2 | Existing HTTP/HTTPS plus 40 nested handle create/cascading-close cycles without extra HTTP sends | Native TLS, pending-transfer cancellation and real connectivity |
| VIBTEST v2 | Existing patterns plus 64 motor ON/OFF cycles | Physical vibration stops on background/exit |
| BTTEST v2 | Existing Classic names/MAC inquiry, 32 startup/cleanup and 8 inquiry cancel/reopen cycles | Two-host RFCOMM/game exchange; no peers does not prove a failure |

Host tests count destruction/cancellation for repeated camera/network ownership
cycles, alongside the existing GPS/vibration/BT tests. An additional Unicorn
suite executes the actual five hardware ARM tools with controlled providers;
camera/GPS/motor/network live-object counts must return to zero. These use controlled
providers; they do not count Android/Windows native driver resources or certify
RSS stability. Real lifecycle checks need the corresponding device too.

## Reproducible host checks

From the repository root:

```text
python tools/gizmondo-diagnostics/run.py
python tools/gizmondo-diagnostics/run.py --guest --clang clang --lld ld.lld
python tools/gizmondo-diagnostics/run.py --native-gpu
```

The default run restores missing RAM/DLL fixtures from the checked-in ZIP on a
fresh clone. `--guest` rebuilds all eight ARM tools before running the
RAM/VFS/timing and hardware suites sequentially; it needs Clang/LLD,
CMake, make and libclang (or the equivalent Windows build tools). The runner
fails fast and does not execute stale fixtures after a failed build. Outputs
and `results.json` are under `target/gizmondo-diagnostics`. Native GPU tests are
explicitly optional and require a working driver; default ignored tests are not
silently counted as executed. Commands have a configurable per-command timeout.

## Device pass

Import the rebuilt diagnostic ZIPs; replace/remove old diagnostic library entries
so an older extracted EXE is not accidentally tested. They are synthetic titles,
not proprietary games. Keep RAMTEST's DLL/process reports as well as RAMTEST.TXT.
Run RAMTEST/VFSTEST twice; restart the emulator between the two VFS runs. Run
TIMETEST in both renderers. Enable only hardware needed for each hardware tool.

Then run real games with repeated entry/exit, launcher child/parent return,
pause/resume, and camera/GPS/BT/network/vibration activity. Check that activity
stops at exit. Compare memory/resource counts after warming the emulator; driver
caches and JIT growth make raw RSS alone an unreliable pass/fail threshold.
No temporary game instrumentation or new production polling is added here.

A detected contract bug is corrected in the accompanying source patch:
DeviceIoControl on a closed/unknown device handle returns FALSE and sets
ERROR_INVALID_HANDLE (6), rather than leaving an old error.
